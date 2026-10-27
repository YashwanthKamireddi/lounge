//! Finds the installed package that owns the running executable, also when that executable
//! runs mounted in LG Home's place.
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Where the TV installs Lounge (developer mode, then Homebrew Channel's store path). The
/// linker reads the same list for the library path an executable mounted elsewhere needs.
pub fn directories() -> impl Iterator<Item = PathBuf> {
    include_str!("../tv/install-directories.txt")
        .lines()
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub directory: PathBuf,
    /// The running executable is this package's current file (false: a package update has
    /// replaced the file a Home mount still holds).
    pub current: bool,
}

fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::metadata(a), fs::metadata(b)) {
            (Ok(a), Ok(b)) => {
                a.is_file() && b.is_file() && a.dev() == b.dev() && a.ino() == b.ino()
            }
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        fs::canonicalize(a)
            .ok()
            .zip(fs::canonicalize(b).ok())
            .is_some_and(|(a, b)| a == b)
    }
}

fn is_package(directory: &Path) -> bool {
    if !directory.join("assets").is_dir() || !directory.join("lounge").is_file() {
        return false;
    }
    fs::read(directory.join("appinfo.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|manifest| {
            manifest.get("id").and_then(serde_json::Value::as_str) == Some(super::APP_ID)
                && manifest.get("type").and_then(serde_json::Value::as_str) == Some("native")
                && manifest.get("main").and_then(serde_json::Value::as_str) == Some("lounge")
        })
}

/// A normal launch uses the directory beside its executable. As Home (`home`), the package
/// whose executable is the mounted one; after an update replaced that file, the first
/// installed package, marked not current so the mount is renewed.
pub fn find(
    executable: &Path,
    home: bool,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<Found, String> {
    if !home {
        let directory = executable
            .parent()
            .ok_or("The executable has no directory")?
            .to_path_buf();
        return Ok(Found {
            directory,
            current: true,
        });
    }
    let candidates: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|dir| is_package(dir))
        .collect();
    if let Some(directory) = candidates
        .iter()
        .find(|directory| same_file(&directory.join("lounge"), executable))
    {
        return Ok(Found {
            directory: directory.clone(),
            current: true,
        });
    }
    candidates
        .into_iter()
        .next()
        .map(|directory| Found {
            directory,
            current: false,
        })
        .ok_or_else(|| "No installed Lounge package found for Home".to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "lounge-install-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn install(&self, location: &str, bytes: &[u8]) -> PathBuf {
            let directory = self.0.join(location).join(super::super::APP_ID);
            fs::create_dir_all(directory.join("assets")).unwrap();
            fs::write(directory.join("lounge"), bytes).unwrap();
            fs::write(
                directory.join("appinfo.json"),
                serde_json::json!({"id": super::super::APP_ID, "type": "native", "main": "lounge"})
                    .to_string(),
            )
            .unwrap();
            directory
        }

        /// A hard link has the same device and inode a file bind mount shows.
        fn mount(&self, directory: &Path) -> PathBuf {
            let executable = self.0.join("usr/bin/com.webos.app.home");
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            let _ = fs::remove_file(&executable);
            fs::hard_link(directory.join("lounge"), &executable).unwrap();
            executable
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_normal_launch_uses_its_own_directory() {
        let fixture = Fixture::new();
        let developer = fixture.install("developer", b"one");
        let found = find(&developer.join("lounge"), false, []).unwrap();
        assert_eq!(found.directory, developer);
        assert!(found.current);
    }

    #[test]
    fn home_finds_the_package_by_its_mounted_file_not_its_bytes() {
        let fixture = Fixture::new();
        let developer = fixture.install("developer", b"same");
        let store = fixture.install("cryptofs", b"same");
        for selected in [&developer, &store] {
            let executable = fixture.mount(selected);
            let found = find(&executable, true, [developer.clone(), store.clone()]).unwrap();
            assert_eq!(&found.directory, selected);
            assert!(found.current);
        }
    }

    #[test]
    fn home_after_an_update_finds_the_package_and_knows_its_mount_is_stale() {
        let fixture = Fixture::new();
        let developer = fixture.install("developer", b"old");
        let executable = fixture.mount(&developer);
        // An update writes a new file; the mount keeps the old one.
        fs::remove_file(developer.join("lounge")).unwrap();
        fs::write(developer.join("lounge"), b"new").unwrap();
        let found = find(&executable, true, [developer.clone()]).unwrap();
        assert_eq!(found.directory, developer);
        assert!(!found.current);
    }

    #[test]
    fn home_never_takes_a_directory_that_is_not_a_lounge_package() {
        for manifest in [
            "not json",
            r#"{"id":"other","type":"native","main":"lounge"}"#,
            r#"{"id":"io.github.yashwanthkamireddi.lounge","type":"web","main":"lounge"}"#,
        ] {
            let fixture = Fixture::new();
            let directory = fixture.install("developer", b"x");
            let executable = fixture.mount(&directory);
            fs::write(directory.join("appinfo.json"), manifest).unwrap();
            assert!(find(&executable, true, [directory]).is_err());
        }
    }
}
