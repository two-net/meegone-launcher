use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::tokens::Tokens;

/// A screen, or a part of one, belonging to one feature.
///
/// It draws the latest immutable state with the theme's tokens and turns input into intents
/// through a [`Dispatch`] (ARCH-05). It keeps no other copy of the state (ARCH-06). Android's
/// views draw it, asking the Rust app what to show (ADR 0006).
pub trait View {
    type State;

    fn render(&mut self, state: &Self::State, tokens: &Tokens);
}

/// Content that one feature shows inside another feature's view, such as the image-of-the-day
/// card inside Events. Only the shell puts it there (ARCH-03, ADR 0002).
pub trait Slot {
    fn render(&mut self, tokens: &Tokens);
}

/// Sends a view's intents to its feature's state holder. The shell creates one per view.
pub struct Dispatch<I: 'static>(Rc<dyn Fn(I)>);

impl<I: 'static> Dispatch<I> {
    pub fn new(send: impl Fn(I) + 'static) -> Self {
        Self(Rc::new(send))
    }

    pub fn send(&self, intent: I) {
        (self.0)(intent);
    }
}

impl<I: 'static> Clone for Dispatch<I> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

/// The latest snapshot of a feature's state, which the shell keeps current for content shown
/// inside another feature (a [`Slot`]).
pub struct Latest<S>(Rc<RefCell<Arc<S>>>);

impl<S> Latest<S> {
    pub fn new(state: Arc<S>) -> Self {
        Self(Rc::new(RefCell::new(state)))
    }

    pub fn set(&self, state: Arc<S>) {
        *self.0.borrow_mut() = state;
    }

    pub fn get(&self) -> Arc<S> {
        Arc::clone(&self.0.borrow())
    }
}

impl<S> Clone for Latest<S> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}
