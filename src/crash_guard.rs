//! Lounge as the TV's Home must never leave the TV without one. A marker in /tmp (memory,
//! cleared at boot) holds the start times of runs that have not yet proved healthy; a healthy
//! run or a clean exit removes it. Starting again and again without getting there means a
//! broken build: Lounge then gives Home back to LG before it draws anything.
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Where Lounge as Home keeps its unproven starts.
pub const HOME_MARKER: &str = "/tmp/lounge-home-starts";
/// This many unproven starts within the window, this one included, is a crash loop.
const STARTS: usize = 3;
const WINDOW_SECONDS: u64 = 60;
/// Alive this long after the first presented frame is healthy.
pub const HEALTHY_AFTER_FRAME_SECONDS: u64 = 5;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// Records this start. True when it completes a crash loop.
pub fn begin(marker: &Path) -> bool {
    begin_at(marker, now())
}

fn begin_at(marker: &Path, now: u64) -> bool {
    let mut starts: Vec<u64> = fs::read_to_string(marker)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|value| value.parse().ok())
        .filter(|start| now.saturating_sub(*start) <= WINDOW_SECONDS)
        .collect();
    starts.push(now);
    let looping = starts.len() >= STARTS;
    let text: Vec<String> = starts.iter().map(u64::to_string).collect();
    let _ = fs::write(marker, text.join(" "));
    looping
}

/// A healthy run, or a clean exit: nothing crashed.
pub fn end(marker: &Path) {
    let _ = fs::remove_file(marker);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_unproven_starts_within_a_minute_are_a_crash_loop() {
        let directory = std::env::temp_dir().join(format!("lounge-guard-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let marker = directory.join("starts");
        let _ = fs::remove_file(&marker);
        assert!(!begin_at(&marker, 1000));
        assert!(!begin_at(&marker, 1010));
        assert!(begin_at(&marker, 1020), "third start without a healthy run");
        end(&marker);
        assert!(
            !begin_at(&marker, 1030),
            "a healthy run starts the count again"
        );
        // Starts outside the window do not count.
        assert!(!begin_at(&marker, 1030 + WINDOW_SECONDS + 1));
        assert!(!begin_at(&marker, 1030 + 2 * WINDOW_SECONDS + 2));
        let _ = fs::remove_dir_all(&directory);
    }
}
