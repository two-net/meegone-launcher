use std::cell::Cell;
use std::fmt::Debug;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;

/// Applies a feature's intents and owns its state; there is one per feature (ARCH-05).
///
/// Side effects such as launching an app or writing a store go through the data layer. A state
/// holder never sees the UI, so tests can drive it directly.
pub trait StateHolder {
    /// The immutable snapshot the UI renders.
    type State: Clone + PartialEq + Debug;
    /// Something the user did, or a change the system reported.
    type Intent: Debug;

    fn state(&self) -> &Self::State;

    fn handle(&mut self, intent: Self::Intent);

    /// Closes this feature's topmost transient element, such as a folder, menu, search, edit
    /// mode or close mode. Returns false if nothing was open (HOME-07, HOME-08).
    fn on_back(&mut self) -> bool {
        false
    }
}

/// Whether a feature is working.
///
/// Expected problems, such as missing access or no connection, are part of the feature's own
/// state. `Failed` is for bugs. It keeps nothing of the panic: its message can quote the data
/// being handled, and the panic hook has already logged where it happened (PRIV-03, ADR 0043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ready,
    /// The feature's code panicked. Its store stops calling it, and the rest of the app carries
    /// on (ARCH-09).
    Failed,
}

type Listener<S> = Box<dyn FnMut(&Arc<S>)>;

/// Runs a [`StateHolder`].
///
/// It publishes a new immutable snapshot after every change and contains failures, so a
/// failing feature never takes down the shell (ARCH-09). It is the feature's fuse: a panic in
/// its intents, its listeners or any of its code run through [`Store::contain`] stops the
/// feature for good. Containing a panic needs `panic = "unwind"`, which this crate requires.
pub struct Store<H: StateHolder> {
    holder: H,
    snapshot: Arc<H::State>,
    health: Cell<Health>,
    listeners: Vec<Listener<H::State>>,
}

impl<H: StateHolder> Store<H> {
    pub fn new(holder: H) -> Self {
        let snapshot = Arc::new(holder.state().clone());
        Self {
            holder,
            snapshot,
            health: Cell::new(Health::Ready),
            listeners: Vec::new(),
        }
    }

    /// The latest snapshot. After a failure, the last one published before it.
    pub fn state(&self) -> Arc<H::State> {
        Arc::clone(&self.snapshot)
    }

    pub fn health(&self) -> Health {
        self.health.get()
    }

    /// Whether the feature is working.
    pub fn is_ready(&self) -> bool {
        self.health.get() == Health::Ready
    }

    /// Stops the feature, as a panic in its code would: the host found a bug in its views
    /// (ARCH-09, ADR 0043).
    pub fn fail(&self) {
        self.health.set(Health::Failed);
    }

    /// Runs some of this feature's code outside its intents, such as building what its views
    /// show. Returns `None` without running it once the feature has failed, and a panic inside
    /// it stops the feature (ARCH-09).
    ///
    /// `step` only computes: whoever calls it changes their own state with its result, after
    /// it returns, so a panic never leaves them half-way through a change.
    pub fn contain<R>(&self, step: impl FnOnce() -> R) -> Option<R> {
        Self::guard(&self.health, step)
    }

    /// Calls `listener` with every new snapshot.
    pub fn subscribe(&mut self, listener: impl FnMut(&Arc<H::State>) + 'static) {
        self.listeners.push(Box::new(listener));
    }

    /// Applies an intent. Ignored once the feature has failed.
    pub fn dispatch(&mut self, intent: H::Intent) {
        self.run(|holder| holder.handle(intent));
    }

    /// Asks the feature to close its topmost transient element; see [`StateHolder::on_back`].
    pub fn back(&mut self) -> bool {
        self.run(H::on_back).unwrap_or(false)
    }

    fn run<R>(&mut self, step: impl FnOnce(&mut H) -> R) -> Option<R> {
        // The holder may be half-way through a change when it panics, so it is never called
        // again. Publishing is inside too: a listener is the feature's own view keeping up.
        Self::guard(&self.health, || {
            let result = step(&mut self.holder);
            publish(&self.holder, &mut self.snapshot, &mut self.listeners);
            result
        })
    }

    /// Runs `step` while the feature works, and stops the feature if it panics.
    fn guard<R>(health: &Cell<Health>, step: impl FnOnce() -> R) -> Option<R> {
        if health.get() != Health::Ready {
            return None;
        }
        let step = || {
            #[cfg(any(test, feature = "faults"))]
            faults::fire(std::any::type_name::<H>());
            step()
        };
        match panic::catch_unwind(AssertUnwindSafe(step)) {
            Ok(result) => Some(result),
            Err(_) => {
                health.set(Health::Failed);
                None
            }
        }
    }
}

fn publish<H: StateHolder>(
    holder: &H,
    snapshot: &mut Arc<H::State>,
    listeners: &mut [Listener<H::State>],
) {
    if *holder.state() == **snapshot {
        return;
    }
    *snapshot = Arc::new(holder.state().clone());
    for listener in listeners {
        listener(snapshot);
    }
}

/// Bugs made to order, for tests and the debug build's fault check (ADR 0043). Never in a
/// release build: only tests and the `faults` feature, which only the debug build turns on,
/// compile it.
#[cfg(any(test, feature = "faults"))]
pub mod faults {
    use std::cell::RefCell;

    thread_local! {
        /// The faults waiting on this thread.
        static ARMED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// Makes the next contained step named `key` on this thread panic, once.
    ///
    /// `key` is a state holder's type name, such as `EventsHolder`, which fails at the
    /// feature's next intent or contained step, or a name the host's readers fire, such as
    /// `EventsReader`.
    pub fn arm(key: &str) {
        ARMED.with_borrow_mut(|armed| armed.push(key.to_owned()));
    }

    /// Forgets every fault armed on this thread.
    pub fn disarm() {
        ARMED.with_borrow_mut(Vec::clear);
    }

    /// Panics if a fault is armed for `name`, a type's path or a plain name, and disarms it.
    pub fn fire(name: &str) {
        let armed = ARMED.with_borrow_mut(|armed| {
            let at = armed.iter().position(|key| {
                name == key
                    || name
                        .strip_suffix(key.as_str())
                        .is_some_and(|path| path.ends_with("::"))
            })?;
            Some(armed.remove(at))
        });
        if let Some(key) = armed {
            panic!("fault injected: {key}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Debug)]
    enum Op {
        Add(u32),
        Open,
        Fail,
    }

    #[derive(Default)]
    struct Counter {
        state: (u32, bool),
    }

    impl StateHolder for Counter {
        type State = (u32, bool);
        type Intent = Op;

        fn state(&self) -> &Self::State {
            &self.state
        }

        fn handle(&mut self, intent: Op) {
            match intent {
                Op::Add(n) => self.state.0 += n,
                Op::Open => self.state.1 = true,
                Op::Fail => panic!("counter broke"),
            }
        }

        fn on_back(&mut self) -> bool {
            std::mem::take(&mut self.state.1)
        }
    }

    #[test]
    fn publishes_a_snapshot_only_when_the_state_changes() {
        let mut store = Store::new(Counter::default());
        let published = Rc::new(RefCell::new(Vec::new()));
        store.subscribe({
            let published = Rc::clone(&published);
            move |state| published.borrow_mut().push(**state)
        });

        store.dispatch(Op::Add(2));
        store.dispatch(Op::Add(0));
        store.dispatch(Op::Add(1));

        assert_eq!(*published.borrow(), [(2, false), (3, false)]);
        assert_eq!(*store.state(), (3, false));
    }

    #[test]
    fn earlier_snapshots_stay_unchanged() {
        let mut store = Store::new(Counter::default());
        let before = store.state();

        store.dispatch(Op::Add(5));

        assert_eq!(*before, (0, false));
        assert_eq!(*store.state(), (5, false));
    }

    #[test]
    fn back_closes_what_is_open_then_reports_nothing_open() {
        let mut store = Store::new(Counter::default());
        store.dispatch(Op::Open);

        assert!(store.back());
        assert!(!store.back());
        assert_eq!(*store.state(), (0, false));
    }

    #[test]
    fn a_panicking_holder_is_contained_and_never_called_again() {
        let mut store = Store::new(Counter::default());
        store.dispatch(Op::Add(1));
        store.dispatch(Op::Open);

        store.dispatch(Op::Fail);
        store.dispatch(Op::Add(1));

        assert_eq!(store.health(), Health::Failed);
        assert!(!store.is_ready());
        assert_eq!(*store.state(), (1, true));
        assert!(!store.back());
    }

    #[test]
    fn contained_code_runs_while_the_feature_works_and_returns_its_result() {
        let mut store = Store::new(Counter::default());
        store.dispatch(Op::Add(3));

        let read = store.contain(|| store.state().0 * 2);

        assert_eq!(read, Some(6));
        assert!(store.is_ready());
    }

    #[test]
    fn a_panic_in_contained_code_stops_the_feature_and_it_never_runs_again() {
        // ARCH-09: a bug in what a feature shows stops the feature, not the caller.
        let mut store = Store::new(Counter::default());
        store.dispatch(Op::Add(1));

        let failed: Option<u32> = store.contain(|| panic!("view broke"));
        let mut ran = false;
        let after = store.contain(|| ran = true);
        store.dispatch(Op::Add(1));

        assert_eq!(failed, None);
        assert_eq!(after, None);
        assert!(!ran);
        assert_eq!(store.health(), Health::Failed);
        assert_eq!(*store.state(), (1, false));
    }

    #[test]
    fn a_listener_that_panics_stops_its_feature_not_the_caller() {
        let mut store = Store::new(Counter::default());
        store.subscribe(|state| assert!(state.0 < 2, "listener broke"));

        store.dispatch(Op::Add(1));
        store.dispatch(Op::Add(1));
        store.dispatch(Op::Add(1));

        assert_eq!(store.health(), Health::Failed);
        // The snapshot it was publishing stays: what the views last saw is what they keep.
        assert_eq!(*store.state(), (2, false));
    }

    #[test]
    fn a_feature_failed_from_outside_ignores_intents_and_keeps_its_last_snapshot() {
        // ADR 0043: the host reports a bug in the feature's own views.
        let mut store = Store::new(Counter::default());
        store.dispatch(Op::Add(4));

        store.fail();
        store.dispatch(Op::Add(1));

        assert_eq!(store.health(), Health::Failed);
        assert_eq!(*store.state(), (4, false));
        assert_eq!(store.contain(|| 1), None);
    }

    #[test]
    fn an_armed_fault_fires_once_in_the_next_contained_step() {
        let mut store = Store::new(Counter::default());
        let other = Store::new(Counter::default());
        faults::arm("Counter");

        store.dispatch(Op::Add(1));
        faults::arm("CounterReader");
        faults::fire("Reader");
        let read = other.contain(|| faults::fire("CounterReader"));

        assert_eq!(store.health(), Health::Failed);
        assert_eq!(*store.state(), (0, false));
        assert_eq!(read, None);
        assert_eq!(other.health(), Health::Failed);
        // Each fired once and is gone.
        faults::fire("CounterReader");
        faults::fire(std::any::type_name::<Counter>());
    }
}
