//! What happened to the apps on screen, kept across power-offs: each app that came to the
//! front, every launch, stop and close webOS reports (with its reason when it gives one), and
//! Lounge's own opens and closes. When the family says a video "just stopped", this shows what
//! took its place, and whether the app ended by itself, was closed for memory, or was closed
//! by Lounge.
//!
//! On the TV only, and only as Home (root): the status log in /tmp is gone after every
//! power-off, this file is not. Events are rare (a few per app opened), so the flash sees a
//! line now and then, and the file is cut back to its newest lines once it grows.

#[cfg(feature = "webos")]
const FILE: &str = "/var/lib/lounge/history.log";
/// The file is cut back to its newest `KEEP` lines once it passes `TRIM_AT` bytes.
#[cfg(feature = "webos")]
const TRIM_AT: u64 = 48 * 1024;
#[cfg(feature = "webos")]
const KEEP: usize = 400;

/// The TV's offset from UTC, read once at start (reading it later, with threads running, is
/// refused on Linux).
#[cfg(feature = "webos")]
static OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();

#[cfg(feature = "webos")]
pub fn set_offset(offset: time::UtcOffset) {
    let _ = OFFSET.set(offset);
}

#[cfg(not(feature = "webos"))]
pub fn set_offset(_offset: time::UtcOffset) {}

/// Serialises writers: the TV's event streams call in from their own threads.
#[cfg(feature = "webos")]
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Appends `line`, stamped with the local date and time.
#[cfg(feature = "webos")]
pub fn note(line: &str) {
    use std::io::Write;
    if !crate::platform::is_home() {
        return;
    }
    let _guard = WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = time::OffsetDateTime::now_utc()
        .to_offset(OFFSET.get().copied().unwrap_or(time::UtcOffset::UTC));
    let stamp = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(FILE)
        .and_then(|mut file| {
            writeln!(file, "{stamp} {line}")?;
            file.metadata()
        });
    if let Ok(metadata) = written
        && metadata.len() > TRIM_AT
        && let Ok(text) = std::fs::read_to_string(FILE)
    {
        let lines: Vec<&str> = text.lines().collect();
        let kept = lines[lines.len().saturating_sub(KEEP)..].join("\n") + "\n";
        // Replaced in one rename: a power cut mid-write leaves the old file or the new one.
        let partial = format!("{FILE}.new");
        if std::fs::write(&partial, kept).is_ok() {
            let _ = std::fs::rename(&partial, FILE);
        }
    }
}

#[cfg(not(feature = "webos"))]
pub fn note(_line: &str) {}
