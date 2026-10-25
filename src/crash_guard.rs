//! Detects repeated crashes across launches. A marker holds the start times
//! of runs that have not exited cleanly; a clean exit removes it.
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Two unclean exits within this window put the next run in safe mode.
const WINDOW_SECONDS: u64 = 120;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// Record this start. Returns true when the previous runs crashed twice
/// within the window (each earlier start still in the marker never exited).
pub fn begin(marker: &Path) -> bool {
    begin_at(marker, now())
}

fn begin_at(marker: &Path, now: u64) -> bool {
    let crashed: Vec<u64> = fs::read_to_string(marker)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|value| value.parse().ok())
        .filter(|start| now.saturating_sub(*start) <= WINDOW_SECONDS)
        .collect();
    let safe = crashed.len() >= 2;
    let mut starts = crashed;
    if safe {
        // Safe mode starts the count afresh, so a fixed build recovers.
        starts.clear();
    }
    starts.push(now);
    let text: Vec<String> = starts.iter().map(u64::to_string).collect();
    let _ = fs::write(marker, text.join(" "));
    safe
}

/// A clean exit: nothing crashed.
pub fn end(marker: &Path) {
    let _ = fs::remove_file(marker);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_quick_crashes_mean_safe_mode_and_a_clean_exit_resets() {
        let directory = std::env::temp_dir().join(format!("luma-guard-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let marker = directory.join("running.marker");
        let _ = fs::remove_file(&marker);
        assert!(!begin_at(&marker, 1000)); // first start
        assert!(!begin_at(&marker, 1010)); // after one crash
        assert!(begin_at(&marker, 1020)); // after two crashes: safe mode
        assert!(!begin_at(&marker, 1030)); // count restarted
        end(&marker);
        assert!(!begin_at(&marker, 1040));
        // Old crashes outside the window do not count.
        assert!(!begin_at(&marker, 1040 + WINDOW_SECONDS + 1));
        let _ = fs::remove_dir_all(&directory);
    }
}
