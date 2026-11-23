//! What a bug in MeeGone tells the host: where it happened, never what it was about (PRIV-03,
//! ADR 0042).
//!
//! `android.rs` turns a panic into a Java exception, whose message reaches logcat and, when an
//! exception isn't caught, the crash report. A panic's own message can quote the data it was
//! handling: slicing a string at a bad index prints the string, which could be a notification's
//! text. So [`hide_messages`] replaces the panic hook with one that keeps only the panic's file
//! and line, and [`message`] reports that instead of the panic's payload. The default hook's
//! printout is dropped too: it carries the same message.

use std::cell::RefCell;
use std::panic::{self, PanicHookInfo};
use std::sync::Once;

thread_local! {
    /// Where this thread's latest panic happened, until [`message`] takes it.
    static PLACE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Replaces the process's panic hook with one that keeps only where a panic happened. Only the
/// first call does anything.
pub(crate) fn hide_messages() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| panic::set_hook(Box::new(remember)));
}

fn remember(info: &PanicHookInfo<'_>) {
    let place = info
        .location()
        .map(|at| format!("{}:{}", at.file(), at.line()));
    PLACE.with(|latest| *latest.borrow_mut() = place);
}

/// What to say about the panic this thread just caught: its file and line, if the hook kept
/// them, and never its message.
pub(crate) fn message() -> String {
    match PLACE.with(|latest| latest.borrow_mut().take()) {
        Some(place) => format!("MeeGone panicked at {place}"),
        None => "MeeGone panicked".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caught_panic_says_where_but_never_what() {
        // PRIV-03: std's own message for a bad slice quotes the string being sliced.
        hide_messages();
        let text = String::from("Dinner at Alice's?");

        let caught = panic::catch_unwind(|| text[..100].len());

        assert!(caught.is_err());
        let said = message();
        assert!(
            said.starts_with("MeeGone panicked at src/panics.rs:"),
            "{said}"
        );
        assert!(!said.contains("Alice"), "{said}");
        // Taken once: a later failure without a panic doesn't repeat it.
        assert_eq!(message(), "MeeGone panicked");
    }
}
