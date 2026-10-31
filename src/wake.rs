//! Wakes the event loop from any thread the moment background work has a result for it: a TV
//! service reply, a decoded image, a forecast, a saved file. The loop then sleeps until input,
//! one of these, or its next timer (see `State::quiet_for`), instead of polling its queues.
//!
//! Coalesced: however many results arrive while the loop is busy, one wake event is queued;
//! the loop re-arms it (`taken`) before it reads the queues, so nothing sent after the read
//! goes unnoticed.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

type Push = Box<dyn Fn() + Send + Sync>;

static PUSH: OnceLock<Push> = OnceLock::new();
static QUEUED: AtomicBool = AtomicBool::new(false);

/// How the loop is woken (an SDL user event); set once at start.
pub fn set(push: impl Fn() + Send + Sync + 'static) {
    let _ = PUSH.set(Box::new(push));
}

/// A result is waiting for the loop.
pub fn now() {
    if !QUEUED.swap(true, Ordering::AcqRel)
        && let Some(push) = PUSH.get()
    {
        push();
    }
}

/// The loop is about to read every queue: the next result wakes it again.
pub fn taken() {
    QUEUED.store(false, Ordering::Release);
}

/// `result` of a send to the loop, passed through after waking it (the message is queued
/// first, so the loop finds it).
pub fn sent<T>(result: T) -> T {
    now();
    result
}
