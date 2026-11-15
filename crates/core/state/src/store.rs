use std::any::Any;
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
/// state. `Failed` is for bugs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Ready,
    /// The state holder panicked. The store stops calling it and the rest of the app carries on
    /// (ARCH-09).
    Failed(String),
}

type Listener<S> = Box<dyn FnMut(&Arc<S>)>;

/// Runs a [`StateHolder`].
///
/// It publishes a new immutable snapshot after every change and contains failures, so a
/// failing feature never takes down the shell (ARCH-09). Containing a panic needs
/// `panic = "unwind"`, which the workspace keeps (see ARCHITECTURE.md).
pub struct Store<H: StateHolder> {
    holder: H,
    snapshot: Arc<H::State>,
    health: Health,
    listeners: Vec<Listener<H::State>>,
}

impl<H: StateHolder> Store<H> {
    pub fn new(holder: H) -> Self {
        let snapshot = Arc::new(holder.state().clone());
        Self {
            holder,
            snapshot,
            health: Health::Ready,
            listeners: Vec::new(),
        }
    }

    /// The latest snapshot.
    pub fn state(&self) -> Arc<H::State> {
        Arc::clone(&self.snapshot)
    }

    pub fn health(&self) -> &Health {
        &self.health
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
        if self.health != Health::Ready {
            return None;
        }
        let holder = &mut self.holder;
        match panic::catch_unwind(AssertUnwindSafe(|| step(holder))) {
            Ok(result) => {
                self.publish();
                Some(result)
            }
            Err(payload) => {
                // The holder may be half-way through a change, so it is never called again.
                self.health = Health::Failed(panic_message(payload.as_ref()));
                None
            }
        }
    }

    fn publish(&mut self) {
        if *self.holder.state() == *self.snapshot {
            return;
        }
        self.snapshot = Arc::new(self.holder.state().clone());
        for listener in &mut self.listeners {
            listener(&self.snapshot);
        }
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown failure".to_owned()
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

        assert_eq!(*store.health(), Health::Failed("counter broke".into()));
        assert_eq!(*store.state(), (1, true));
        assert!(!store.back());
    }
}
