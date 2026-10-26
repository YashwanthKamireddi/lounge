//! Presentation pacing is measured only in forced-redraw profiling mode. Idle
//! wakeups are not frames, and CPU submission time is not a GPU timestamp.
use crate::{raster::Size, renderer::RenderStats, state::State};
use serde::Serialize;
use std::time::{Duration, Instant};

const WARMUP: usize = 10;

#[derive(Default)]
pub struct Profile {
    presented: usize,
    previous: Option<Instant>,
    intervals: Vec<f64>,
    cpu: Vec<f64>,
    build: Vec<f64>,
    compose: Vec<f64>,
    slow: Vec<String>,
}
#[derive(Serialize)]
pub struct Timing {
    pub samples: usize,
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}
#[derive(Serialize)]
pub struct Summary {
    pub presented: usize,
    pub warmup_frames: usize,
    pub pacing: Option<Timing>,
    pub cpu_submission: Option<Timing>,
    /// CPU geometry building inside the render call, before any GL command.
    pub cpu_build: Option<Timing>,
    /// Scene composition (layout) before rendering.
    pub compose: Option<Timing>,
    /// What happened in each frame that missed two refreshes or more.
    pub slow_frames: Vec<String>,
}
impl Profile {
    #[cfg(test)]
    pub fn record(&mut self, now: Instant, cpu_ms: f64, exclude: bool) {
        self.record_split(now, cpu_ms, 0., 0., exclude);
    }
    pub fn record_split(
        &mut self,
        now: Instant,
        cpu_ms: f64,
        build_ms: f64,
        compose_ms: f64,
        exclude: bool,
    ) {
        self.presented += 1;
        if self.presented > WARMUP
            && !exclude
            && let Some(previous) = self.previous
        {
            self.intervals
                .push(now.duration_since(previous).as_secs_f64() * 1000.);
            self.cpu.push(cpu_ms);
            self.build.push(build_ms);
            self.compose.push(compose_ms);
        }
        self.previous = Some(now);
    }
    /// The interval just recorded, when it counted toward the summary.
    pub fn last_interval(&self) -> Option<f64> {
        (self.presented > WARMUP)
            .then(|| self.intervals.last().copied())
            .flatten()
    }
    pub fn note_slow(&mut self, note: String) {
        self.slow.push(note);
    }
    pub fn pause(&mut self) {
        self.previous = None;
    }
    pub fn summary(&self) -> Summary {
        Summary {
            presented: self.presented,
            warmup_frames: WARMUP,
            pacing: summarize(&self.intervals),
            cpu_submission: summarize(&self.cpu),
            cpu_build: summarize(&self.build),
            compose: summarize(&self.compose),
            slow_frames: self.slow.clone(),
        }
    }
}
fn summarize(samples: &[f64]) -> Option<Timing> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(Timing {
        samples: sorted.len(),
        mean_ms: sorted.iter().sum::<f64>() / sorted.len() as f64,
        p95_ms: sorted[(sorted.len() * 95).div_ceil(100) - 1],
        max_ms: *sorted.last()?,
    })
}

/// One presented frame, as the frame log sees it.
pub struct Frame {
    /// Frames presented before this one.
    pub number: usize,
    pub compose_ms: f64,
    /// A screenshot frame: kept out of the pacing figures.
    pub exclude: bool,
    pub width: u32,
    pub height: u32,
    /// Time since Lounge started.
    pub uptime: Duration,
    /// Log the drawable size whenever it changes (diagnostic runs).
    pub verbose: bool,
}

/// Per-frame logging: the TV's slow-frame log (`/tmp/lounge-debug`), the
/// profiler's notes on slow frames, and drawable size changes.
pub struct FrameLog {
    slow_log: bool,
    last_presented: Instant,
    was_animating: bool,
    last_faults: u64,
    /// Texture uploads and glyph tessellations at the last profiled frame.
    last_counts: (usize, usize),
    last_drawable: Option<Size>,
}

impl FrameLog {
    /// `slow_log`: log every frame that misses the panel's refresh.
    pub fn new(slow_log: bool) -> Self {
        Self {
            slow_log,
            last_presented: Instant::now(),
            was_animating: false,
            last_faults: crate::debug::major_faults(),
            last_counts: (0, 0),
            last_drawable: None,
        }
    }

    /// A frame was presented. `profile` is the profiler in a profiling run.
    pub fn presented(
        &mut self,
        state: &State,
        stats: &RenderStats,
        profile: Option<&mut Profile>,
        frame: Frame,
    ) {
        // TV debug log: every frame that misses the panel's refresh.
        if self.slow_log {
            let now = Instant::now();
            let interval = now.duration_since(self.last_presented).as_secs_f64() * 1000.;
            // The first frame after an idle gap is not slow: Lounge sat
            // still (correctly) until the next key arrived.
            // In Move every frame is logged, with the memory pages read
            // back from swap since the last one, to tell a drawing
            // stall from a paging one.
            let slow =
                interval > 30. && interval < 1000. && state.animating() && self.was_animating;
            if slow || state.moving.is_some() {
                let faults = crate::debug::major_faults();
                eprintln!(
                    "Lounge {} at {} ms: {interval:.1} ms (compose {:.1}, cpu {:.1}, build {:.1}, draws {}, uploads {}, page faults +{}) page {:?} overlay {}",
                    if slow { "slow frame" } else { "move frame" },
                    frame.uptime.as_millis(),
                    frame.compose_ms,
                    stats.cpu_submission_ms,
                    stats.cpu_build_ms,
                    stats.draw_calls,
                    stats.texture_uploads,
                    faults.saturating_sub(self.last_faults),
                    state.page,
                    state.overlay.is_some(),
                );
                self.last_faults = faults;
            }
            self.last_presented = now;
            self.was_animating = state.animating();
        }
        if let Some(profile) = profile {
            profile.record_split(
                Instant::now(),
                stats.cpu_submission_ms,
                stats.cpu_build_ms,
                frame.compose_ms,
                frame.exclude,
            );
            let (uploads, tessellations) = (stats.texture_uploads, stats.glyph_tessellations);
            if let Some(interval) = profile.last_interval()
                && interval > 30.
            {
                profile.note_slow(format!(
                    "frame {}: {interval:.1} ms, cpu {:.1}, uploads +{}, glyphs +{}, draws {}, scroll {:.0}/{:.0}, focus {:?}",
                    frame.number,
                    stats.cpu_submission_ms,
                    uploads - self.last_counts.0,
                    tessellations - self.last_counts.1,
                    stats.draw_calls,
                    state.scroll,
                    state.shown_scroll,
                    state.focus,
                ));
            }
            self.last_counts = (uploads, tessellations);
        }
        let drawable = Size::new(frame.width, frame.height);
        if self.last_drawable != Some(drawable) {
            if frame.verbose || self.slow_log {
                eprintln!(
                    "Lounge drawable: {}x{}, density {:.3}x, 4K buffer: {}",
                    frame.width,
                    frame.height,
                    drawable.scale(),
                    drawable.is_4k()
                );
            }
            self.last_drawable = Some(drawable);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn profiling_excludes_warmup_and_capture_stalls() {
        let mut profile = Profile::default();
        let start = Instant::now();
        for index in 0..20 {
            profile.record(start + Duration::from_millis(index * 16), 2., index == 19);
        }
        let summary = profile.summary();
        let pacing = summary.pacing.unwrap();
        assert_eq!(pacing.samples, 9);
        assert_eq!(pacing.mean_ms, 16.);
        assert_eq!(pacing.p95_ms, 16.);
        assert_eq!(summary.cpu_submission.unwrap().mean_ms, 2.);
    }
}
