//! The clock every animation reads. It is the real one, except while
//! recording (`--record`): then it steps exactly one frame per rendered
//! frame, so the capture plays back at a steady 60 fps however long each
//! frame took to draw. Work that measures real time (start-up stages,
//! compose cost) and talks to the TV keeps `Instant::now`.
//!
//! Only the UI thread reads it, so a recording is that thread's state:
//! nothing else (another test, a TV-service thread) ever sees stepped time.
use std::{
    borrow::Borrow,
    cell::Cell,
    time::{Duration, Instant},
};

thread_local! {
    /// While recording: when it started, and how far it has been stepped.
    static RECORDING: Cell<Option<(Instant, Duration)>> = const { Cell::new(None) };
}

pub fn now() -> Instant {
    RECORDING
        .get()
        .map_or_else(Instant::now, |(start, stepped)| start + stepped)
}

/// How long ago `earlier` was, on this clock.
pub fn since(earlier: impl Borrow<Instant>) -> Duration {
    now().saturating_duration_since(*earlier.borrow())
}

/// From here on time moves only with `step`.
pub fn record() {
    RECORDING.set(Some((Instant::now(), Duration::ZERO)));
}

pub fn step(frame: Duration) {
    if let Some((start, stepped)) = RECORDING.get() {
        RECORDING.set(Some((start, stepped + frame)));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn a_recording_clock_moves_only_when_stepped() {
        super::record();
        let start = super::now();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(super::since(start), Duration::ZERO);
        super::step(Duration::from_millis(16));
        assert_eq!(super::since(start), Duration::from_millis(16));
    }
}
