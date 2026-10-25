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
    Ok(base.join("luma-native/preferences.json"))
}

#[cfg(all(feature = "webos", unix))]
fn tv_path() -> Result<PathBuf, String> {
    use std::{
        io::Write,
        os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    };

    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let legacy = executable.with_file_name("preferences.json");
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
    let mut candidates = if executable.starts_with("/media/cryptofs") {
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
        // another process's private app storage.
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
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
                .write_all(b"Luma storage probe")
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
                        "Luma preferences migrated from {} to {}",
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
                eprintln!("Luma weather cache migration: {error}");
            }
        }
        eprintln!("Luma persistent storage: {}", directory.display());
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
    fs::write(&temporary, bytes).map_err(|e| {
        format!(
            "Could not write preferences temporary file {}: {e}",
            temporary.display()
        )
    })?;
    fs::rename(&temporary, path)
        .map_err(|e| format!("Could not replace preferences file {}: {e}", path.display()))
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
