//! The clock every animation reads. It is the real one, except while
//! recording (`--record`): then it steps exactly one frame per rendered
//! frame, so the capture plays back at a steady 60 fps however long each
//! frame took to draw. Work that measures real time (start-up stages,
//! compose cost) and talks to the TV keeps `Instant::now`.
//!
//! Otherwise each frame reads one time, on the display's refresh grid (`begin_frame`): the
//! loop wakes a few ms early or late, but the panel shows frames only at refreshes, so motion
//! is computed for the refresh it lands on and every step of an animation is the same size.
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
    /// The time the frame being built shows, set by `begin_frame`.
    static FRAME: Cell<Option<Instant>> = const { Cell::new(None) };
}

pub fn now() -> Instant {
    RECORDING.get().map_or_else(
        || FRAME.get().unwrap_or_else(Instant::now),
        |(start, stepped)| start + stepped,
    )
}

/// Starts a frame on a display refreshing every `period`. Between frames of a running animation
/// the frame time moves by whole refreshes (the real gap, rounded), so it never drifts more than
/// half a refresh from real time; after an idle gap, or with no known period, it is real time.
pub fn begin_frame(period: Duration) {
    if RECORDING.get().is_some() {
        return;
    }
    let real = Instant::now();
    let shown = match FRAME.get() {
        Some(previous) if !period.is_zero() => {
            let gap = real.saturating_duration_since(previous);
            if gap > period * 4 {
                real
            } else {
                let refreshes = (gap.as_secs_f64() / period.as_secs_f64()).round().max(1.);
                previous + period.mul_f64(refreshes)
            }
        }
        _ => real,
    };
    FRAME.set(Some(shown));
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
    fn frames_land_on_the_refresh_grid() {
        let period = Duration::from_micros(16_667);
        super::begin_frame(period);
        let first = super::now();
        std::thread::sleep(Duration::from_millis(19));
        assert_eq!(
            super::since(first),
            Duration::ZERO,
            "one time for the whole frame"
        );
        super::begin_frame(period);
        assert_eq!(
            super::now() - first,
            period,
            "a late wake still moves one refresh"
        );
    }

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
