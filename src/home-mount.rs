//! Lounge as the TV's Home, run as root by `tv/home.sh` (`lounge --home COMMAND`).
//!
//! LG's Home is a native lifecycle-v2 program whose LS2 role is bound to its executable path.
//! Mounting Lounge's executable over that path leaves LG's manifest, window type and role in
//! place, so webOS itself starts Lounge as com.webos.app.home on every path to Home. The mount
//! lives in RAM: removing the boot hook and restarting always gives back LG's Home.
//!
//! This used to be `tv/home.cjs` on the TV's Node.js, which costs 4-5 s of CPU to start. Every
//! file, mount, process and Luna call goes through `System`, so the tests run it against a model
//! TV; nothing touches the machine they run on.
use crate::platform::{APP_ID as APP, HOME_ID as HOME, LG_HOME_EXE as EXE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io;

const MANIFEST_DIR: &str = "/usr/palm/applications/com.webos.app.home";
const CONFIG: &str = "/var/lib/lounge/home.json";
const BACKUP: &str = "/var/lib/lounge/home-backup.json";
const HOOK: &str = "/var/lib/webosbrew/init.d/10-lounge-home";
/// What the boot hook links to: the root-owned copy of `tv/home.sh`.
const HOOK_TARGET: &str = "/var/lib/lounge/home.sh";
/// A normal tile launch of Lounge opens the Home instance instead. The app's jail shares
/// /var/run but not this mount; /var/run is RAM, like the mount.
pub const MARKER: &str = "/var/run/lounge-home";
// LG snapshots its Home with CRIU: LG's Home dumps itself (Lounge never does), sam keeps the
// image in /tmp and a copy in /mnt/lg/cmn_data, and restores Home from it. With Lounge mounted
// CRIU rejects that image ("bad size"), but the failed restore leaves the next process ids low:
// Lounge then takes ids LG's input apps' images need, and their restores fail. So no image of
// LG's Home is left for sam while Lounge is Home. sam deletes both copies itself after such a
// failure; LG's Home writes them again the next time it runs (after disable, or a cold boot).
const CHECKPOINT: &str = "/tmp/checkpoint/com.webos.app.home";
const PERSISTENT_CHECKPOINT: &str = "/mnt/lg/cmn_data/checkpoint/com.webos.app.home";
const HASH_CHECK: &str = "/tmp/checkpoint/hash_check_com.webos.app.home";
const BOOT_COPY: &str = "/tmp/finish_criu_boottime_copy";
// What the earlier Home Button left behind: its compositor key filter and the Home settings
// pointed at a launcher. With Lounge as com.webos.app.home they are all stock again.
const FILTER: &str = "/var/lib/lounge/home-key-filter.js";
const CHAIN: &str = "com.webos.surfacemanager.keyFilters";
const OLD_LAUNCHERS: [&str; 2] = [APP, "com.ecklf.luma"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
}

/// Everything the Home commands do to the TV.
pub trait System {
    fn read(&self, file: &str) -> io::Result<String>;
    /// The path exists (a symlink itself, not followed).
    fn exists(&self, file: &str) -> bool;
    /// Device and inode, following symlinks; None when it cannot be read.
    fn stat(&self, file: &str) -> Option<(u64, u64)>;
    /// Replaces `file` whole (written beside it, then renamed), mode 644.
    fn write(&self, file: &str, text: &str) -> Result<(), String>;
    /// Removes a file or symlink (never followed); one already gone is fine.
    fn unlink(&self, file: &str) -> Result<(), String>;
    fn list(&self, directory: &str) -> Result<Vec<String>, String>;
    fn remove_dir(&self, directory: &str) -> Result<(), String>;
    fn symlink(&self, target: &str, link: &str) -> Result<(), String>;
    /// A symlink's target; None when `link` is missing or not a symlink.
    fn read_link(&self, link: &str) -> Result<Option<String>, String>;
    fn real_path(&self, path: &str) -> io::Result<String>;
    /// Runs a program to its end (10 s at most) and returns its stdout.
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String>;
    fn pause(&self, seconds: u64);
    fn pids(&self) -> Result<Vec<u32>, String>;
    /// Signals `pid`; one already gone is fine.
    fn kill(&self, pid: u32, signal: Signal) -> Result<(), String>;
    fn log(&self, line: &str);
}

/// `home.json`: written by `enable`, read by every command and by `tv/home.sh` (line by line,
/// so always pretty-printed).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    method: String,
    #[serde(rename = "appDir", default, skip_serializing_if = "Option::is_none")]
    app_dir: Option<String>,
}

/// What `status` reports, in this order: `tv/home.sh` and Settings read it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Status {
    #[serde(rename = "returnValue")]
    pub return_value: bool,
    pub method: &'static str,
    pub enabled: bool,
    pub mounted: bool,
    pub stale: bool,
    pub foreign: bool,
    pub hook: bool,
    pub marker: bool,
    pub checkpoint: bool,
    #[serde(rename = "appDir")]
    pub app_dir: Option<String>,
}

/// Only a mount of Lounge's executable is Lounge's, also after a package update replaced the
/// mounted file.
pub fn ours(root: Option<&str>) -> bool {
    root.is_some_and(|root| {
        root.strip_suffix("//deleted")
            .unwrap_or(root)
            .ends_with("/io.github.yashwanthkamireddi.lounge/lounge")
    })
}

/// The key filter chain without Lounge's old Home Button filter (Glasshouse's own entry stays).
fn without_old_filter(chain: &[Value]) -> Vec<Value> {
    chain
        .iter()
        .filter(|entry| entry.get("file").and_then(Value::as_str) != Some(FILTER))
        .cloned()
        .collect()
}

/// What LG's boot manager opens at a cold boot: Live TV or an input. Anything else in front
/// was opened by someone.
fn startup_screen(app: &str) -> bool {
    app == "com.webos.app.livetv"
        || app.strip_prefix("com.webos.app.").is_some_and(|rest| {
            rest.strip_prefix("hdmi").is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
            }) || rest.strip_prefix("externalinput.").is_some_and(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
        })
}

fn same(a: Option<(u64, u64)>, b: Option<(u64, u64)>) -> bool {
    a.is_some() && a == b
}

pub struct Home<S: System> {
    pub system: S,
}

impl<S: System> Home<S> {
    pub fn new(system: S) -> Self {
        Self { system }
    }

    fn json(&self, file: &str) -> Result<Value, String> {
        let text = self.system.read(file).map_err(|e| format!("{file}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))
    }

    fn write_json(&self, file: &str, value: &impl Serialize) -> Result<(), String> {
        let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        self.system.write(file, &(text + "\n"))
    }

    fn luna(&self, uri: &str, payload: Value) -> Result<Value, String> {
        let output = self.system.run(
            "/usr/bin/luna-send",
            &["-n", "1", "-w", "5000", uri, &payload.to_string()],
        )?;
        let start = output
            .find('{')
            .ok_or_else(|| format!("{uri} gave no reply"))?;
        let reply = serde_json::Deserializer::from_str(&output[start..])
            .into_iter::<Value>()
            .next()
            .and_then(Result::ok)
            .filter(|reply| reply.get("returnValue").is_some_and(Value::is_boolean))
            .ok_or_else(|| format!("{uri} gave no Luna reply"))?;
        Ok(reply)
    }

    /// The reply of a call that must succeed.
    fn accepted(&self, uri: &str, payload: Value, action: &str) -> Result<Value, String> {
        let reply = self.luna(uri, payload)?;
        if reply["returnValue"] != true {
            return Err(format!("{action}: {reply}"));
        }
        Ok(reply)
    }

    fn foreground(&self) -> Result<String, String> {
        let reply = self.accepted(
            "luna://com.webos.applicationManager/getForegroundAppInfo",
            json!({}),
            "Read the foreground app",
        )?;
        Ok(reply["appId"].as_str().unwrap_or_default().to_owned())
    }

    fn mount_lines(&self) -> Result<Vec<(String, String)>, String> {
        let text = self
            .system
            .read("/proc/self/mountinfo")
            .map_err(|e| format!("/proc/self/mountinfo: {e}"))?;
        Ok(text
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split(' ').collect();
                Some((fields.get(3)?.to_string(), fields.get(4)?.to_string()))
            })
            .collect())
    }

    /// The root of the mount on `target` that is on top, or None.
    fn mount_root(&self, target: &str) -> Result<Option<String>, String> {
        Ok(self
            .mount_lines()?
            .into_iter()
            .filter(|(_, mounted)| mounted == target)
            .map(|(root, _)| root)
            .next_back())
    }

    /// Anything mounted over LG Home's manifest belongs to another integration.
    fn manifest_mounted(&self) -> Result<bool, String> {
        Ok(self.mount_lines()?.iter().any(|(_, target)| {
            target == MANIFEST_DIR
                || target
                    .strip_prefix(MANIFEST_DIR)
                    .is_some_and(|rest| rest.starts_with('/'))
        }))
    }

    fn refuse_foreign(&self) -> Result<Option<String>, String> {
        let root = self.mount_root(EXE)?;
        if (root.is_some() && !ours(root.as_deref())) || self.manifest_mounted()? {
            return Err("Another integration is mounted over LG Home; nothing was changed".into());
        }
        Ok(root)
    }

    fn mounted(&self, app_dir: &str) -> bool {
        same(
            self.system.stat(&format!("{app_dir}/lounge")),
            self.system.stat(EXE),
        )
    }

    fn unmount(&self) -> Result<(), String> {
        for _ in 0..3 {
            let Some(root) = self.mount_root(EXE)? else {
                break;
            };
            if !ours(Some(&root)) {
                return Err("Another integration is mounted over LG Home".into());
            }
            if let Err(error) = self.system.run("/bin/umount", &[EXE]) {
                // A running Home executes the mounted file: detach it instead.
                self.system.log(&format!("{error}; detaching the mount"));
                self.system.run("/bin/umount", &["-l", EXE])?;
            }
        }
        if self.mount_root(EXE)?.is_some() {
            return Err("The Lounge Home mount is still active".into());
        }
        Ok(())
    }

    fn mount(&self, app_dir: &str) -> Result<(), String> {
        if self.mount_root(EXE)?.is_some() && !self.mounted(app_dir) {
            self.unmount()?;
        }
        if !self.mounted(app_dir) {
            self.system
                .run("/bin/mount", &["--bind", &format!("{app_dir}/lounge"), EXE])?;
        }
        if !self.mounted(app_dir) {
            return Err("The Home bind mount did not take".into());
        }
        Ok(())
    }

    /// Files are unlinked, never followed (the /tmp pages are a symlink into the persistent
    /// copy); the persistent directory itself stays, as sam leaves it.
    fn remove_checkpoint(&self) -> Result<(), String> {
        for directory in [CHECKPOINT, PERSISTENT_CHECKPOINT] {
            if !self.system.exists(directory) {
                continue;
            }
            for name in self.system.list(directory)? {
                self.system.unlink(&format!("{directory}/{name}"))?;
            }
        }
        if self.system.exists(CHECKPOINT) {
            self.system.remove_dir(CHECKPOINT)?;
        }
        self.system.unlink(HASH_CHECK)
    }

    /// Processes started as LG's Home executable. `keep`: leave a running copy of the current
    /// Lounge alone, so applying again never restarts a healthy Home.
    fn home_processes(&self, keep: Option<&str>) -> Result<Vec<u32>, String> {
        let exe = keep.and_then(|dir| self.system.stat(&format!("{dir}/lounge")));
        let mut found = vec![];
        for pid in self.system.pids()? {
            match self.system.read(&format!("/proc/{pid}/cmdline")) {
                Ok(cmdline) => {
                    if cmdline.split('\0').next() == Some(EXE)
                        && !same(exe, self.system.stat(&format!("/proc/{pid}/exe")))
                    {
                        found.push(pid);
                    }
                }
                // A process may exit while /proc is read.
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        || error.raw_os_error() == Some(3) => {}
                Err(error) => return Err(format!("/proc/{pid}/cmdline: {error}")),
            }
        }
        Ok(found)
    }

    fn stop_home(&self, keep: Option<&str>) -> Result<usize, String> {
        let pids = self.home_processes(keep)?;
        for &pid in &pids {
            self.system.kill(pid, Signal::Term)?;
        }
        let alive = || -> Vec<u32> {
            pids.iter()
                .copied()
                .filter(|pid| self.system.exists(&format!("/proc/{pid}")))
                .collect()
        };
        for _ in 0..8 {
            if alive().is_empty() {
                break;
            }
            self.system.pause(1);
        }
        for pid in alive() {
            self.system.kill(pid, Signal::Kill)?;
        }
        for _ in 0..3 {
            if alive().is_empty() {
                break;
            }
            self.system.pause(1);
        }
        if !alive().is_empty() {
            return Err("The previous Home process has not stopped".into());
        }
        Ok(pids.len())
    }

    fn launch_home(&self) -> Result<(), String> {
        self.accepted(
            "luna://com.webos.applicationManager/launch",
            json!({"id": HOME}),
            "Open Home",
        )
        .map(|_| ())
    }

    /// Right after the previous Home stopped, sam can accept a launch without starting
    /// anything: Home in front is the success condition.
    fn open_home(&self) -> Result<(), String> {
        for i in 0..15 {
            if (i == 0 || i == 6)
                && let Err(error) = self.launch_home()
            {
                self.system.log(&error);
            }
            if self.foreground()? == HOME {
                return Ok(());
            }
            self.system.pause(1);
        }
        Err("Home did not come to the front".into())
    }

    fn hook_owned(&self) -> Result<bool, String> {
        Ok(self.system.read_link(HOOK)?.as_deref() == Some(HOOK_TARGET))
    }

    fn install_hook(&self) -> Result<(), String> {
        if !self.system.exists("/var/lib/webosbrew/init.d") {
            return Err(
                "The Homebrew Channel boot hooks (/var/lib/webosbrew/init.d) are missing".into(),
            );
        }
        if self.system.exists(HOOK) && !self.hook_owned()? {
            return Err(format!("{HOOK} is already in use"));
        }
        if !self.hook_owned()? {
            self.system.symlink(HOOK_TARGET, HOOK)?;
        }
        Ok(())
    }

    fn remove_hook(&self) -> Result<(), String> {
        if self.hook_owned()? {
            self.system.unlink(HOOK)?;
        }
        Ok(())
    }

    fn config(&self) -> Result<Option<Settings>, String> {
        if !self.system.exists(CONFIG) {
            return Ok(None);
        }
        serde_json::from_value(self.json(CONFIG)?)
            .map(Some)
            .map_err(|e| format!("{CONFIG}: {e}"))
    }

    fn stock_home(&self) -> Result<(), String> {
        let stock = self.json(&format!("{MANIFEST_DIR}/appinfo.json"))?;
        if stock["id"] != HOME
            || stock["type"] != "native"
            || stock["main"] != format!("file://{EXE}").as_str()
            || stock["nativeLifeCycleInterfaceVersion"] != 2
        {
            return Err(
                "This TV's Home is not a native lifecycle-v2 program Lounge can stand in for"
                    .into(),
            );
        }
        Ok(())
    }

    fn installation(&self, app_dir: Option<&str>) -> Result<String, String> {
        let candidates: Vec<String> = match app_dir {
            Some(dir) => vec![dir.to_owned()],
            None => crate::platform::install_directories()
                .map(|dir| dir.to_string_lossy().into_owned())
                .collect(),
        };
        for candidate in candidates {
            let directory = match self.system.real_path(&candidate) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("{candidate}: {error}")),
            };
            let manifest = match self.system.read(&format!("{directory}/appinfo.json")) {
                Ok(text) => serde_json::from_str::<Value>(&text).ok(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("{directory}/appinfo.json: {error}")),
            };
            if manifest.is_some_and(|app| {
                app["id"] == APP && app["type"] == "native" && app["main"] == "lounge"
            }) && self.system.exists(&format!("{directory}/lounge"))
            {
                return Ok(directory);
            }
        }
        Err("No installed native Lounge package found".into())
    }

    /// Mounts Lounge in Home's place. `open`: bring Home to the front even if it was not.
    fn apply(&self, settings: &Settings, open: bool) -> Result<(), String> {
        self.refuse_foreign()?;
        let app_dir = settings.app_dir.as_deref().unwrap_or("undefined");
        if !self.system.exists(&format!("{app_dir}/lounge")) {
            return Err("Lounge is no longer installed".into());
        }
        let front = self.foreground().unwrap_or_else(|error| {
            self.system.log(&error);
            String::new()
        });
        let applied = (|| {
            self.mount(app_dir)?;
            self.system.write(MARKER, &format!("{APP}\n"))?;
            self.remove_checkpoint()?;
            let stopped = self.stop_home(Some(app_dir))?;
            // Never take the screen from an input or app someone chose: only where the Home
            // just stopped was in front.
            if open || (front == HOME && stopped > 0) {
                self.open_home()?;
            }
            Ok(())
        })();
        if let Err(error) = applied {
            if let Err(recovery) = self.disable() {
                return Err(format!(
                    "{error}; restoring LG Home also failed: {recovery}"
                ));
            }
            return Err(error);
        }
        Ok(())
    }

    /// Gives Home back to LG exactly: no mount, no marker, no hook. Works with or without a
    /// config.
    fn restore(&self) -> Result<(), String> {
        let front = match self.foreground() {
            Ok(front) => Some(front),
            Err(error) => {
                self.system
                    .log(&format!("{error}; restoring the files first"));
                None
            }
        };
        let root = self.mount_root(EXE)?;
        if root.is_some() && !ours(root.as_deref()) {
            return Err("Another integration is mounted over LG Home".into());
        }
        self.system.unlink(MARKER)?;
        self.remove_hook()?;
        if root.is_some() {
            self.unmount()?;
        }
        self.stop_home(None)?;
        // Nothing in front means Home itself went away: LG's Home takes its place.
        if front.is_some_and(|front| front == HOME || front.is_empty()) {
            self.launch_home()?;
        }
        Ok(())
    }

    fn clean_up_old_home(&self) -> Result<Vec<String>, String> {
        let mut backup = if self.system.exists(BACKUP) {
            self.json(BACKUP)?
        } else {
            json!({})
        };
        if !backup.is_object() {
            backup = json!({});
        }
        let mut notes = vec![];
        let configs = self
            .accepted(
                "luna://com.webos.service.config/getConfigs",
                json!({"configNames": [CHAIN, "profile.idleApp"]}),
                "Read the key filter chain",
            )?
            .get("configs")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(chain) = configs[CHAIN].as_array() {
            let kept = without_old_filter(chain);
            if kept.len() != chain.len() {
                if backup["keyFilters"].is_null() {
                    backup["keyFilters"] = Value::Array(chain.clone());
                }
                self.write_json(BACKUP, &backup)?;
                self.accepted(
                    "luna://com.webos.service.config/setConfigs",
                    json!({"configs": {CHAIN: kept}}),
                    "Remove the old Home key filter",
                )?;
                notes.push("old Home key filter removed".to_owned());
            }
        }
        for file in [
            FILTER,
            "/var/lib/lounge/home-key-filter.next",
            "/var/lib/lounge/home-button.js",
        ] {
            self.system.unlink(file)?;
        }
        let idle = &configs["profile.idleApp"];
        if idle["appId"]
            .as_str()
            .is_some_and(|id| OLD_LAUNCHERS.contains(&id))
        {
            if backup["idleApp"].is_null() {
                backup["idleApp"] = idle.clone();
            }
            self.write_json(BACKUP, &backup)?;
            self.accepted(
                "luna://com.webos.service.config/setConfigs",
                json!({"configs": {"profile.idleApp": {"appId": HOME}}}),
                "Restore the idle app",
            )?;
            notes.push(format!("profile.idleApp is {HOME} again"));
        }
        let general = self.luna(
            "luna://com.webos.settingsservice/getSystemSettings",
            json!({"category": "general", "keys": ["defaultApps"]}),
        )?;
        let apps = &general["settings"]["defaultApps"];
        if let Some(entries) = apps.as_object()
            && apps["home"]
                .as_str()
                .is_some_and(|id| OLD_LAUNCHERS.contains(&id))
        {
            if backup["defaultApps"].is_null() {
                backup["defaultApps"] = apps.clone();
            }
            self.write_json(BACKUP, &backup)?;
            let rest: serde_json::Map<String, Value> = entries
                .iter()
                .filter(|(key, _)| *key != "home")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            if rest.is_empty() {
                self.accepted(
                    "luna://com.webos.settingsservice/deleteSystemSettings",
                    json!({"category": "general", "keys": ["defaultApps"]}),
                    "Restore the default Home",
                )?;
            } else {
                self.accepted(
                    "luna://com.webos.settingsservice/setSystemSettings",
                    json!({"category": "general", "settings": {"defaultApps": rest}}),
                    "Restore the default Home",
                )?;
            }
            notes.push("general.defaultApps.home is stock again".to_owned());
        }
        Ok(notes)
    }

    fn close_normal(&self) -> Result<(), String> {
        let running = self.accepted(
            "luna://com.webos.applicationManager/running",
            json!({}),
            "Read running apps",
        )?;
        let normal = running["running"]
            .as_array()
            .is_some_and(|running| running.iter().any(|entry| entry["id"] == APP));
        if normal {
            self.luna(
                "luna://com.webos.applicationManager/closeByAppId",
                json!({"id": APP}),
            )?;
        }
        Ok(())
    }

    fn enable(&self, app_dir: Option<&str>) -> Result<(), String> {
        self.stock_home()?;
        let app_dir = self.installation(app_dir)?;
        self.refuse_foreign()?;
        if self.system.exists(HOOK) && !self.hook_owned()? {
            return Err(format!("{HOOK} is already in use"));
        }
        let mut settings = Settings {
            enabled: false,
            method: "executable".into(),
            app_dir: Some(app_dir),
        };
        self.write_json(CONFIG, &settings)?;
        let applied = (|| {
            self.install_hook()?;
            for note in self.clean_up_old_home()? {
                self.system.log(&note);
            }
            self.apply(&settings, true)
        })();
        if let Err(error) = applied {
            self.remove_hook()?;
            return Err(error);
        }
        settings.enabled = true;
        self.write_json(CONFIG, &settings)?;
        // A normal Lounge (the one that asked, say) would write the same settings: Home
        // replaces it.
        if let Err(error) = self.close_normal() {
            self.system.log(&error);
        }
        Ok(())
    }

    fn disable(&self) -> Result<(), String> {
        if let Some(mut settings) = self.config()? {
            settings.enabled = false;
            self.write_json(CONFIG, &settings)?;
        }
        self.restore()
    }

    /// At boot, after sam is up and its own CRIU boot work is done (bounded waits).
    fn boot(&self) -> Result<(), String> {
        let Some(settings) = self.config()?.filter(|settings| settings.enabled) else {
            return Ok(());
        };
        let app_dir = settings.app_dir.as_deref().unwrap_or("undefined");
        if !self.system.exists(&format!("{app_dir}/lounge")) {
            self.system.log("Lounge is not installed; LG Home stays");
            return Ok(());
        }
        for i in 0..30 {
            match self.foreground() {
                Ok(_) => break,
                // sam is not up yet.
                Err(error) => {
                    if i == 29 {
                        self.system.log(&error);
                    }
                    self.system.pause(2);
                }
            }
        }
        for _ in 0..30 {
            if !self.system.exists("/tmp/criu_enable") || self.system.exists(BOOT_COPY) {
                break;
            }
            self.system.pause(2);
        }
        self.apply(&settings, false)?;
        self.home_after_boot();
        Ok(())
    }

    /// A cold boot shows Home. LG's boot manager opens the last input (or Live TV) and, unlike
    /// a Quick Start wake, never starts Home: once it has, Home is opened over it. Only what the
    /// boot manager opens is replaced; an app someone opened meanwhile stays. Bounded: 60 s at
    /// most.
    fn home_after_boot(&self) {
        for i in 0..30 {
            // sam is busy with the boot; ask again (and say why on the last try).
            let app = self.foreground().unwrap_or_else(|error| {
                if i == 29 {
                    self.system.log(&error);
                }
                String::new()
            });
            if app == HOME {
                return;
            }
            if !app.is_empty() && !startup_screen(&app) {
                self.system
                    .log(&format!("boot: {app} is in front; Home stays behind it"));
                return;
            }
            if !app.is_empty() {
                match self.launch_home() {
                    Ok(()) => self.system.log(&format!("boot: opened Home over {app}")),
                    Err(error) => self.system.log(&error),
                }
                return;
            }
            self.system.pause(2);
        }
    }

    pub fn status(&self) -> Result<Status, String> {
        let settings = self.config()?;
        let root = self.mount_root(EXE)?;
        let app_dir = settings.as_ref().and_then(|s| s.app_dir.clone());
        let mounted = app_dir.as_deref().is_some_and(|dir| self.mounted(dir));
        Ok(Status {
            return_value: true,
            method: "executable",
            enabled: settings.is_some_and(|s| s.enabled),
            mounted: ours(root.as_deref()) && mounted,
            stale: ours(root.as_deref()) && !mounted,
            foreign: (root.is_some() && !ours(root.as_deref())) || self.manifest_mounted()?,
            hook: self.hook_owned()?,
            marker: self.system.exists(MARKER),
            checkpoint: self.system.exists(CHECKPOINT)
                || (self.system.exists(PERSISTENT_CHECKPOINT)
                    && !self.system.list(PERSISTENT_CHECKPOINT)?.is_empty()),
            app_dir,
        })
    }

    /// One `home.sh` command; the reply is what is true afterwards.
    pub fn main(&self, command: &str, app_dir: Option<&str>) -> Result<Status, String> {
        match command {
            "enable" => self.enable(app_dir.filter(|dir| !dir.is_empty()))?,
            "disable" => self.disable()?,
            "boot" => self.boot()?,
            "apply" => {
                let settings = self
                    .config()?
                    .filter(|settings| settings.enabled)
                    .ok_or("Lounge is not enabled as Home")?;
                self.apply(&settings, false)?;
            }
            "status" => {}
            _ => return Err(format!("Unknown command {command}")),
        }
        self.status()
    }
}

/// `lounge --home COMMAND [APP_DIR]`: the reply line and whether it succeeded.
pub fn command(home: &Home<impl System>, command: &str, app_dir: Option<&str>) -> (String, bool) {
    match home.main(command, app_dir) {
        Ok(status) => (serde_json::to_string(&status).unwrap_or_default(), true),
        Err(error) => (failure(&error), false),
    }
}

/// `{"returnValue":false,"errorText":...}`, in that order.
pub fn failure(error: &str) -> String {
    #[derive(Serialize)]
    struct Failure<'a> {
        #[serde(rename = "returnValue")]
        return_value: bool,
        #[serde(rename = "errorText")]
        error_text: &'a str,
    }
    let end = error
        .char_indices()
        .nth(400)
        .map_or(error.len(), |(index, _)| index);
    serde_json::to_string(&Failure {
        return_value: false,
        error_text: &error[..end],
    })
    .unwrap_or_default()
}

/// The TV itself. Only built for the TV backend: a desktop run never mounts or kills anything.
#[cfg(all(feature = "webos", unix))]
pub struct Tv;

#[cfg(all(feature = "webos", unix))]
impl System for Tv {
    fn read(&self, file: &str) -> io::Result<String> {
        std::fs::read_to_string(file)
    }

    fn exists(&self, file: &str) -> bool {
        std::fs::symlink_metadata(file).is_ok()
    }

    fn stat(&self, file: &str) -> Option<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(file)
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()))
    }

    fn write(&self, file: &str, text: &str) -> Result<(), String> {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let next = format!("{file}.new");
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .open(&next)
            .and_then(|mut handle| handle.write_all(text.as_bytes()))
            .and_then(|()| std::fs::rename(&next, file))
            .map_err(|e| format!("{file}: {e}"))
    }

    fn unlink(&self, file: &str) -> Result<(), String> {
        match std::fs::remove_file(file) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(format!("{file}: {error}"))
            }
            _ => Ok(()),
        }
    }

    fn list(&self, directory: &str) -> Result<Vec<String>, String> {
        std::fs::read_dir(directory)
            .and_then(|entries| {
                entries
                    .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect()
            })
            .map_err(|e| format!("{directory}: {e}"))
    }

    fn remove_dir(&self, directory: &str) -> Result<(), String> {
        std::fs::remove_dir(directory).map_err(|e| format!("{directory}: {e}"))
    }

    fn symlink(&self, target: &str, link: &str) -> Result<(), String> {
        std::os::unix::fs::symlink(target, link).map_err(|e| format!("{link}: {e}"))
    }

    fn read_link(&self, link: &str) -> Result<Option<String>, String> {
        match std::fs::read_link(link) {
            Ok(target) => Ok(Some(target.to_string_lossy().into_owned())),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || error.kind() == io::ErrorKind::InvalidInput =>
            {
                Ok(None)
            }
            Err(error) => Err(format!("{link}: {error}")),
        }
    }

    fn real_path(&self, path: &str) -> io::Result<String> {
        std::fs::canonicalize(path).map(|path| path.to_string_lossy().into_owned())
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        run_bounded(program, args, std::time::Duration::from_secs(10))
    }

    fn pause(&self, seconds: u64) {
        std::thread::sleep(std::time::Duration::from_secs(seconds.max(1)));
    }

    fn pids(&self) -> Result<Vec<u32>, String> {
        Ok(std::fs::read_dir("/proc")
            .map_err(|e| format!("/proc: {e}"))?
            .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
            .collect())
    }

    fn kill(&self, pid: u32, signal: Signal) -> Result<(), String> {
        let signal = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: kill only signals the process id given; it touches no memory.
        if unsafe { libc::kill(pid as libc::pid_t, signal) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(format!("kill {pid}: {error}"))
    }

    fn log(&self, line: &str) {
        eprintln!("Lounge Home: {line}");
    }
}

/// Runs `program` to its end within `timeout`; past it the program is killed. Its stdout,
/// or why it failed (its stderr).
#[cfg(all(feature = "webos", unix))]
pub fn run_bounded(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<String, String> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    let command = || {
        [program]
            .iter()
            .chain(args)
            .copied()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{} failed: {e}", command()))?;
    // Each pipe on its own reader, so a full one never stalls the program.
    fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut bytes = vec![];
            if let Some(pipe) = pipe {
                let _ = pipe.take(1 << 20).read_to_end(&mut bytes);
            }
            bytes
        })
    }
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} failed: timed out", command()));
            }
            Err(error) => return Err(format!("{} failed: {error}", command())),
        }
    };
    let text = |reader: thread::JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned()
    };
    let (stdout, stderr) = (text(stdout), text(stderr));
    if !status.success() {
        return Err(format!("{} failed: {}", command(), stderr.trim()));
    }
    Ok(stdout)
}

#[cfg(test)]
#[path = "home-mount-tests.rs"]
mod tests;
