use crate::model::Preferences;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn default_path(webos: bool) -> Result<PathBuf, String> {
    if webos {
        #[cfg(all(feature = "webos", unix))]
        return tv_path();
        #[cfg(not(all(feature = "webos", unix)))]
        return Err("TV storage requires a webOS build".into());
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .ok_or("Set HOME, XDG_CONFIG_HOME or --preferences for native preferences")?;
    Ok(base.join("lounge/preferences.json"))
}

#[cfg(all(feature = "webos", unix))]
fn tv_path() -> Result<PathBuf, String> {
    use std::{
        io::Write,
        os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    };

    // As Home the executable runs at LG's path: the package it came from decides.
    let installation = crate::platform::install_dir()?;
    let legacy = installation.join("preferences.json");
    let id = crate::platform::APP_ID;
    // These volumes persist across boots. The app-specific directory also
    // survives replacement of the installed payload on an ordinary update.
    // Production native jails expose internal storage instead of developer.
    let developer = PathBuf::from("/media/developer")
        .join(id)
        .join("preferences.json");
    let shared_developer = PathBuf::from("/media/developer/temp")
        .join(id)
        .join("preferences.json");
    let internal = PathBuf::from("/media/internal")
        .join(format!(".{id}"))
        .join("preferences.json");
    let mut candidates = if installation.starts_with("/media/cryptofs") {
        [internal, developer, shared_developer]
    } else {
        [developer, shared_developer, internal]
    };
    // Keep a previously used destination even if another candidate becomes
    // writable after a reboot; do not accidentally restart with fresh settings.
    candidates.sort_by_key(|path| !path.exists());
    let mut failures = Vec::new();
    for path in candidates {
        let directory = path.parent().ok_or("Invalid TV storage directory")?;
        // Create only this app's leaf, not a missing system mount hierarchy.
        let created = fs::DirBuilder::new().mode(0o700).create(directory);
        if let Err(error) = created
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            failures.push(format!("{}: {error}", directory.display()));
            continue;
        }
        let metadata = match fs::symlink_metadata(directory) {
            Ok(metadata) => metadata,
            Err(error) => {
                failures.push(format!("{}: {error}", directory.display()));
                continue;
            }
        };
        // Do not silently reuse a symlink or root-created debug directory as
        // another process's private app storage. As Home, Lounge runs as root
        // and uses the app's own directory (see `give_to_folder_owner`).
        let user = unsafe { libc::geteuid() };
        if !metadata.is_dir() || (user != 0 && metadata.uid() != user) {
            failures.push(format!(
                "{} is not owned by the app user",
                directory.display()
            ));
            continue;
        }
        let probe = directory.join(format!(".write-probe-{}", std::process::id()));
        let writable = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&probe)?;
            let result = file
                .write_all(b"Lounge storage probe")
                .and_then(|_| file.sync_all());
            drop(file);
            // Only remove a probe that this process created successfully.
            let cleanup = fs::remove_file(&probe);
            result.and(cleanup)
        })();
        if let Err(error) = writable {
            failures.push(format!("{}: {error}", directory.display()));
            continue;
        }
        if !path.exists() {
            // Import JSON only from the old path in THIS installation, never
            // browser storage or another app ID's preferences.
            match fs::read(&legacy) {
                Ok(bytes) => {
                    let mut preferences: Preferences =
                        serde_json::from_slice(&bytes).map_err(|e| {
                            format!(
                                "Existing preferences {} could not be read: {e}",
                                legacy.display()
                            )
                        })?;
                    preferences.sanitize();
                    save(&path, &preferences)?;
                    eprintln!(
                        "Lounge preferences migrated from {} to {}",
                        legacy.display(),
                        path.display()
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "Existing preferences {} could not be read: {error}",
                        legacy.display()
                    ));
                }
            }
            let old_cache = crate::weather_service::cache_path(&legacy);
            let cache = crate::weather_service::cache_path(&path);
            if !cache.exists()
                && let Some(reading) = crate::weather_service::load_cache(&old_cache)
                && let Err(error) = crate::weather_service::save_cache(&cache, &reading)
            {
                eprintln!("Lounge weather cache migration: {error}");
            }
        }
        eprintln!("Lounge persistent storage: {}", directory.display());
        return Ok(path);
    }
    Err(format!(
        "No writable persistent TV storage found: {}",
        failures.join("; ")
    ))
}

pub fn load(path: &Path) -> Result<Preferences, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut preferences: Preferences = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Preferences could not be read: {e}"))?;
            preferences.sanitize();
            Ok(preferences)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(e) => Err(format!("Preferences could not be read: {e}")),
    }
}

/// As `load`, but a damaged file (a power cut mid-write, say) never stops
/// Lounge from starting: it is kept aside as `<name>.bad-<time>`, never
/// overwritten, and Lounge starts with defaults. Unreadable storage still fails.
pub fn load_or_recover(path: &Path) -> Result<Preferences, String> {
    match load(path) {
        Ok(preferences) => Ok(preferences),
        Err(error) if path.is_file() => {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            let kept = path.with_extension(format!("json.bad-{stamp}"));
            fs::rename(path, &kept).map_err(|e| {
                format!(
                    "{error}; could not keep it aside as {}: {e}",
                    kept.display()
                )
            })?;
            eprintln!(
                "Lounge {error}; kept as {} and started with defaults",
                kept.display()
            );
            Ok(Preferences::default())
        }
        Err(error) => Err(error),
    }
}

pub fn save(path: &Path, preferences: &Preferences) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Could not prepare preferences directory {}: {e}",
                parent.display()
            )
        })?;
    }
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(preferences)
        .map_err(|e| format!("Could not encode preferences: {e}"))?;
    // On flash before the rename: a power cut then leaves the old file or
    // the new one, never a half-written one.
    let written = fs::File::create(&temporary).and_then(|mut file| {
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        give_to_folder_owner(&temporary)
    });
    written.map_err(|e| {
        format!(
            "Could not write preferences temporary file {}: {e}",
            temporary.display()
        )
    })?;
    fs::rename(&temporary, path)
        .map_err(|e| format!("Could not replace preferences file {}: {e}", path.display()))?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        // The rename itself reaches the flash with the folder.
        let _ = fs::File::open(parent).and_then(|folder| folder.sync_all());
    }
    Ok(())
}

/// As Home Lounge runs as root but writes in the app's own folders: what it writes belongs to
/// the folder's owner, so the normal app still reads and replaces it after Home goes back to
/// LG. Elsewhere it changes nothing.
pub fn give_to_folder_owner(path: &Path) -> std::io::Result<()> {
    #[cfg(all(feature = "webos", unix))]
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        use std::os::unix::fs::MetadataExt;
        let folder = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let owner = fs::symlink_metadata(folder)?;
        if owner.uid() != 0 {
            std::os::unix::fs::lchown(path, Some(owner.uid()), Some(owner.gid()))?;
        }
    }
    #[cfg(not(all(feature = "webos", unix)))]
    let _ = path;
    Ok(())
}

/// Saves preferences off the drawing thread: on the TV a write to its flash
/// can take long enough to stall a frame. Only the newest copy is written
/// (rapid changes collapse into one write); failures come back on `errors`,
/// and finishing waits for the last write.
pub struct Writer {
    sender: Option<std::sync::mpsc::Sender<Preferences>>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub errors: std::sync::mpsc::Receiver<String>,
}

impl Writer {
    pub fn new(path: std::path::PathBuf) -> Self {
        let (sender, requests) = std::sync::mpsc::channel::<Preferences>();
        let (failures, errors) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            while let Ok(mut preferences) = requests.recv() {
                while let Ok(newer) = requests.try_recv() {
                    preferences = newer;
                }
                if let Err(error) = save(&path, &preferences) {
                    let _ = failures.send(error);
                }
            }
        });
        Self {
            sender: Some(sender),
            thread: Some(thread),
            errors,
        }
    }

    pub fn save(&self, preferences: &Preferences) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(preferences.clone());
        }
    }

    /// Writes whatever is still queued, then stops.
    pub fn finish(&mut self) {
        self.sender = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.finish();
    }
}
