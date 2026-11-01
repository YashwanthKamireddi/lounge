//! Lounge as the TV's Home, run as root by `tv/home.sh` (`lounge --home COMMAND`).
//!
//! Which way depends on the TV, read from what it has, never from a model list:
//!
//! - `executable` (webOS 5 to 9): LG's Home is a native lifecycle-v2 program whose LS2 role is
//!   bound to its executable path. Mounting Lounge's executable over that path leaves LG's
//!   manifest, window type and role in place, so webOS itself starts Lounge as LG's Home on
//!   every path to Home. On webOS 5 and 6 that Home is com.webos.app.fullhome where the Home
//!   key opens it (`general.defaultApps.home`), else com.webos.app.home.
//! - `manifest` (webOS 10 and 11): LG Home's role is bound to its app id instead, so a mounted
//!   executable would run without it. A Home app staged in /var/lib/lounge/app (LG Home's id,
//!   links to the installed package) is mounted over LG Home's app folder, and sam is made to
//!   read that one manifest again (`blockSystemApp`, then `releaseBlockSystemApp`).
//!
//! Anything else is refused before anything changes. Every mount lives in RAM: removing the
//! boot hook and restarting always gives back LG's Home.
//!
//! This used to be `tv/home.cjs` on the TV's Node.js, which costs 4-5 s of CPU to start. Every
//! file, mount, process and Luna call goes through `System`, so the tests run it against a model
//! TV; nothing touches the machine they run on.
use crate::platform::{APP_ID as APP, FULL_HOME_ID as FULL_HOME, HOME_ID as HOME};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io;

const MANIFEST_DIR: &str = "/usr/palm/applications/com.webos.app.home";
const CONFIG: &str = "/var/lib/lounge/home.json";
const BACKUP: &str = "/var/lib/lounge/home-backup.json";
const HOOK: &str = "/var/lib/webosbrew/init.d/10-lounge-home";
/// What the boot hook links to: the root-owned copy of `tv/home.sh`.
const HOOK_TARGET: &str = "/var/lib/lounge/home.sh";
/// The Home app the `manifest` method mounts over LG Home's app folder.
pub const STAGE: &str = "/var/lib/lounge/app";
/// A normal tile launch of Lounge opens the Home instance instead. The app's jail shares
/// /var/run but not this mount; /var/run is RAM, like the mount.
pub const MARKER: &str = "/var/run/lounge-home";
/// The Home id that launch opens, where it is not com.webos.app.home.
pub const TARGET_MARKER: &str = "/var/run/lounge-home-target";
/// "Rockhopper release 10.3.1-3006": the webOS version decides the method.
const RELEASE: &str = "/etc/starfish-release";
/// webOS 11's sam requires `forceBlockSystemApp` with the block; webOS 10's has no such field.
const BLOCK_SCHEMA: &str = "/etc/palm/schemas/sam/applicationManager.blockSystemApp.schema";
const TARGETS: [&str; 2] = [HOME, FULL_HOME];
// LG snapshots its Home with CRIU: LG's Home dumps itself (Lounge never does), sam keeps the
// image in /tmp and a copy in /mnt/lg/cmn_data, and restores Home from it. With Lounge mounted
// CRIU rejects that image ("bad size"), but the failed restore leaves the next process ids low:
// Lounge then takes ids LG's input apps' images need, and their restores fail. So no image of
// LG's Home is left for sam while Lounge is Home. sam deletes both copies itself after such a
// failure; LG's Home writes them again the next time it runs (after disable, or a cold boot).
// See `checkpoint_of`.
const BOOT_COPY: &str = "/tmp/finish_criu_boottime_copy";
// What the earlier Home Button left behind: its compositor key filter and the Home settings
// pointed at a launcher. With Lounge as com.webos.app.home they are all stock again.
const FILTER: &str = "/var/lib/lounge/home-key-filter.js";
const CHAIN: &str = "com.webos.surfacemanager.keyFilters";
const OLD_LAUNCHERS: [&str; 1] = [APP];

fn exe_of(id: &str) -> String {
    format!("/usr/bin/{id}")
}

/// LG's image of the Home `id`: in RAM, persistent, and sam's hash of it.
fn checkpoint_of(id: &str) -> [String; 3] {
    [
        format!("/tmp/checkpoint/{id}"),
        format!("/mnt/lg/cmn_data/checkpoint/{id}"),
        format!("/tmp/checkpoint/hash_check_{id}"),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    Executable,
    Manifest,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::Manifest => "manifest",
        }
    }
}

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
    /// Creates a directory, mode 755; one already there is fine.
    fn make_dir(&self, directory: &str) -> Result<(), String>;
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
    /// Only where it is not com.webos.app.home.
    #[serde(rename = "homeId", default, skip_serializing_if = "Option::is_none")]
    home_id: Option<String>,
}

impl Settings {
    /// The method and Home id these settings name; anything else was not written by Lounge.
    fn way(&self) -> Result<(Method, &'static str), String> {
        let id = match self.home_id.as_deref() {
            None => HOME,
            Some(id) => TARGETS
                .into_iter()
                .find(|target| *target == id)
                .ok_or_else(|| format!("{CONFIG}: unknown Home {id}"))?,
        };
        match self.method.as_str() {
            "" | "executable" => Ok((Method::Executable, id)),
            "manifest" if id == HOME => Ok((Method::Manifest, id)),
            method => Err(format!("{CONFIG}: unknown method {method} for {id}")),
        }
    }
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
    /// Only where it is not com.webos.app.home.
    #[serde(rename = "homeId", skip_serializing_if = "Option::is_none")]
    pub home_id: Option<&'static str>,
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

/// Only a mount of Lounge's staged Home folder is Lounge's. The root is relative to the file
/// system holding it: /var/lib/lounge/app, or /lib/lounge/app where /var is its own.
pub fn staged(root: Option<&str>) -> bool {
    root.is_some_and(|root| {
        root.strip_suffix("//deleted")
            .unwrap_or(root)
            .ends_with("/lounge/app")
    })
}

/// The webOS major version in /etc/starfish-release ("Rockhopper release 7.6.0-11").
fn webos_version(release: &str) -> Option<u32> {
    let release = release.to_ascii_lowercase();
    let rest = release[release.find("release ")? + "release ".len()..].trim_start();
    let end = rest.find('.')?;
    rest[..end].parse().ok()
}

/// A package artwork name the staged Home may link to: a plain name beside the executable.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains('/')
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

/// Whether one process is a Home for `id`: LG's native executable, LG's Flutter Home, or
/// Lounge started from the staged Home (`nid`/`appId` in sam's launch argument).
fn home_process(cmdline: &str, id: &str) -> bool {
    let args: Vec<&str> = cmdline.split('\0').collect();
    let argv0 = args.first().copied().unwrap_or_default();
    let name = argv0.rsplit('/').next().unwrap_or_default();
    let launched_as = || {
        args.get(1)
            .and_then(|arg| serde_json::from_str::<Value>(arg).ok())
            .is_some_and(|launch| launch["nid"] == id || launch["appId"] == id)
    };
    argv0 == exe_of(id)
        || (name == "flutter-client" && args.windows(2).any(|pair| pair == ["-i", id]))
        || (name == "lounge" && launched_as())
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

    /// Anything but Lounge's staged Home mounted over (or inside) LG Home's app folder `id`
    /// belongs to another integration.
    fn manifest_foreign(&self, id: &str) -> Result<bool, String> {
        let directory = format!("/usr/palm/applications/{id}");
        Ok(self.mount_lines()?.iter().any(|(root, target)| {
            (target == &directory && !(id == HOME && staged(Some(root))))
                || target
                    .strip_prefix(&directory)
                    .is_some_and(|rest| rest.starts_with('/'))
        }))
    }

    /// Anything but Lounge's executable mounted over LG's Home executable `id`.
    fn exe_foreign(&self, id: &str) -> Result<bool, String> {
        let root = self.mount_root(&exe_of(id))?;
        Ok(root.is_some() && !ours(root.as_deref()))
    }

    fn refuse_foreign(&self, id: &str) -> Result<(), String> {
        if self.exe_foreign(id)? || self.manifest_foreign(id)? {
            return Err("Another integration is mounted over LG Home; nothing was changed".into());
        }
        Ok(())
    }

    fn mounted(&self, app_dir: &str, id: &str) -> bool {
        same(
            self.system.stat(&format!("{app_dir}/lounge")),
            self.system.stat(&exe_of(id)),
        )
    }

    /// Lounge's staged Home is the mount on top of LG Home's app folder.
    fn stage_mounted(&self) -> Result<bool, String> {
        Ok(staged(self.mount_root(MANIFEST_DIR)?.as_deref()))
    }

    /// The staged Home is mounted and runs this installation's executable.
    fn stage_current(&self, app_dir: &str) -> Result<bool, String> {
        Ok(self.stage_mounted()?
            && same(self.system.stat(STAGE), self.system.stat(MANIFEST_DIR))
            && self
                .system
                .read_link(&format!("{STAGE}/lounge"))?
                .as_deref()
                == Some(format!("{app_dir}/lounge").as_str()))
    }

    /// Removes Lounge's mount on `path`, `owned` telling Lounge's from another's.
    fn unmount(&self, path: &str, owned: fn(Option<&str>) -> bool) -> Result<(), String> {
        for _ in 0..3 {
            let Some(root) = self.mount_root(path)? else {
                break;
            };
            if !owned(Some(&root)) {
                return Err("Another integration is mounted over LG Home".into());
            }
            if let Err(error) = self.system.run("/bin/umount", &[path]) {
                // A running Home executes the mounted file: detach it instead.
                self.system.log(&format!("{error}; detaching the mount"));
                self.system.run("/bin/umount", &["-l", path])?;
            }
        }
        if self.mount_root(path)?.is_some() {
            return Err("The Lounge Home mount is still active".into());
        }
        Ok(())
    }

    fn mount(&self, app_dir: &str, id: &str) -> Result<(), String> {
        let exe = exe_of(id);
        if self.mount_root(&exe)?.is_some() && !self.mounted(app_dir, id) {
            self.unmount(&exe, ours)?;
        }
        if !self.mounted(app_dir, id) {
            self.system.run(
                "/bin/mount",
                &["--bind", &format!("{app_dir}/lounge"), &exe],
            )?;
        }
        if !self.mounted(app_dir, id) {
            return Err("The Home bind mount did not take".into());
        }
        Ok(())
    }

    /// Writes the Home app sam reads in LG Home's place: Lounge's own manifest under LG Home's
    /// id, hidden, and links to the installed executable, libraries, assets and artwork. It
    /// declares no lifecycle version (v1): Lounge's role cannot register as Home with sam, which
    /// then starts it afresh for each Home press.
    fn stage(&self, app_dir: &str) -> Result<(), String> {
        let mut manifest = self.json(&format!("{app_dir}/appinfo.json"))?;
        if manifest["id"] != APP || manifest["type"] != "native" || manifest["main"] != "lounge" {
            return Err("No installed native Lounge package found".into());
        }
        self.system.make_dir(STAGE)?;
        let mut names = vec!["lounge", "assets", "lib"];
        for key in ["icon", "largeIcon"] {
            if let Some(name) = manifest[key].as_str().filter(|name| plain_name(name)) {
                names.push(name);
            }
        }
        for name in names {
            let source = format!("{app_dir}/{name}");
            let link = format!("{STAGE}/{name}");
            if !self.system.exists(&source)
                || self.system.read_link(&link)?.as_deref() == Some(source.as_str())
            {
                continue;
            }
            self.system.unlink(&link)?;
            self.system.symlink(&source, &link)?;
        }
        manifest["id"] = json!(HOME);
        manifest["title"] = json!("Home");
        manifest["visible"] = json!(false);
        manifest["appCategory"] = json!(["home"]);
        if let Some(fields) = manifest.as_object_mut() {
            fields.remove("nativeLifeCycleInterfaceVersion");
        }
        self.write_json(&format!("{STAGE}/appinfo.json"), &manifest)
    }

    fn mount_stage(&self, app_dir: &str) -> Result<(), String> {
        if !self.stage_mounted()? {
            self.stage(app_dir)?;
            self.system
                .run("/bin/mount", &["--bind", STAGE, MANIFEST_DIR])?;
        } else if !self.stage_current(app_dir)? {
            // Lounge moved (a developer and a store install): the links follow it.
            self.stage(app_dir)?;
        }
        if !self.stage_current(app_dir)? {
            return Err("The Home bind mount did not take".into());
        }
        Ok(())
    }

    /// The staged Home's links and manifest, once nothing is mounted from it. Links are
    /// unlinked, never followed: the package stays.
    fn remove_stage(&self) -> Result<(), String> {
        if !self.system.exists(STAGE) || self.stage_mounted()? {
            return Ok(());
        }
        for name in self.system.list(STAGE)? {
            self.system.unlink(&format!("{STAGE}/{name}"))?;
        }
        self.system.remove_dir(STAGE)
    }

    /// Whether sam has read Lounge's staged Home (`lounge`) or LG's own as com.webos.app.home.
    fn sam_reads(&self, lounge: bool) -> Result<bool, String> {
        let reply = self.accepted(
            "luna://com.webos.applicationManager/getAppInfo",
            json!({"id": HOME}),
            "Read Home's manifest",
        )?;
        let info = &reply["appInfo"];
        let main = info["main"].as_str().unwrap_or_default();
        Ok(info.is_object() && (main.rsplit('/').next() == Some("lounge")) == lounge)
    }

    /// A block left in place would keep Home from starting, across a reboot too: the release
    /// always follows the block, retried.
    fn release(&self) -> Result<(), String> {
        let mut last = String::new();
        for _ in 0..3 {
            match self.luna(
                "luna://com.webos.applicationManager/releaseBlockSystemApp",
                json!({"id": HOME}),
            ) {
                Ok(reply) => {
                    let text = reply["errorText"]
                        .as_str()
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if reply["returnValue"] == true
                        || (text.contains("not") && text.contains("blocked"))
                    {
                        return Ok(());
                    }
                    last = reply.to_string();
                }
                Err(error) => last = error,
            }
            self.system.pause(1);
        }
        Err(format!("Release Home: {last}"))
    }

    /// Makes sam read Home's manifest again, without restarting sam: blocking a system app and
    /// releasing it rereads that one app (webOS 10 and 11). `lounge`: the staged Home is
    /// expected, else LG's own.
    fn reread(&self, lounge: bool) -> Result<(), String> {
        if self.sam_reads(lounge)? {
            return Ok(());
        }
        let mut payload = json!({"id": HOME});
        if self
            .system
            .read(BLOCK_SCHEMA)
            .is_ok_and(|schema| schema.contains("\"forceBlockSystemApp\""))
        {
            payload["forceBlockSystemApp"] = json!(false);
        }
        let blocked = self
            .luna(
                "luna://com.webos.applicationManager/blockSystemApp",
                payload,
            )
            .and_then(|reply| {
                let text = reply["errorText"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if reply["returnValue"] == true
                    || (text.contains("already") && text.contains("blocked"))
                {
                    Ok(())
                } else {
                    Err(format!("Block Home: {reply}"))
                }
            });
        // Released even where the block seemed to fail: it may have taken all the same.
        match (blocked, self.release()) {
            (Err(block), Err(release)) => return Err(format!("{block}; {release}")),
            (Err(error), _) | (_, Err(error)) => return Err(error),
            _ => {}
        }
        for _ in 0..15 {
            if self.sam_reads(lounge)? {
                return Ok(());
            }
            self.system.pause(1);
        }
        Err("sam did not read Home's manifest again".into())
    }

    /// Files are unlinked, never followed (the /tmp pages are a symlink into the persistent
    /// copy); the persistent directory itself stays, as sam leaves it.
    fn remove_checkpoint(&self, id: &str) -> Result<(), String> {
        let [checkpoint, persistent, hash_check] = checkpoint_of(id);
        for directory in [&checkpoint, &persistent] {
            if !self.system.exists(directory) {
                continue;
            }
            for name in self.system.list(directory)? {
                self.system.unlink(&format!("{directory}/{name}"))?;
            }
        }
        if self.system.exists(&checkpoint) {
            self.system.remove_dir(&checkpoint)?;
        }
        self.system.unlink(&hash_check)
    }

    /// Processes started as the Home `id`. `keep`: leave a running copy of the current Lounge
    /// alone, so applying again never restarts a healthy Home.
    fn home_processes(&self, keep: Option<&str>, id: &str) -> Result<Vec<u32>, String> {
        let exe = keep.and_then(|dir| self.system.stat(&format!("{dir}/lounge")));
        let mut found = vec![];
        for pid in self.system.pids()? {
            match self.system.read(&format!("/proc/{pid}/cmdline")) {
                Ok(cmdline) => {
                    if home_process(&cmdline, id)
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

    fn stop_home(&self, keep: Option<&str>, id: &str) -> Result<usize, String> {
        let pids = self.home_processes(keep, id)?;
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

    fn launch_home(&self, id: &str) -> Result<(), String> {
        self.accepted(
            "luna://com.webos.applicationManager/launch",
            json!({"id": id}),
            "Open Home",
        )
        .map(|_| ())
    }

    /// Right after the previous Home stopped, sam can accept a launch without starting
    /// anything: Home in front is the success condition.
    fn open_home(&self, id: &str) -> Result<(), String> {
        for i in 0..15 {
            if (i == 0 || i == 6)
                && let Err(error) = self.launch_home(id)
            {
                self.system.log(&error);
            }
            if self.foreground()? == id {
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

    /// LG's Home `id` is a native lifecycle-v2 program at /usr/bin/`id`.
    fn native_home(&self, id: &str) -> Result<(), String> {
        let stock = self
            .json(&format!("/usr/palm/applications/{id}/appinfo.json"))
            .unwrap_or_default();
        if stock["id"] != id
            || stock["type"] != "native"
            || stock["main"] != format!("file://{}", exe_of(id)).as_str()
            || stock["nativeLifeCycleInterfaceVersion"] != 2
        {
            return Err(
                "This TV's Home is not a native lifecycle-v2 program Lounge can stand in for"
                    .into(),
            );
        }
        Ok(())
    }

    /// LG's Home folder holds LG's own Home manifest (or Lounge's staged one, mounted).
    fn manifest_home(&self) -> Result<(), String> {
        if self.stage_mounted()? {
            return Ok(());
        }
        let stock = self
            .json(&format!("{MANIFEST_DIR}/appinfo.json"))
            .unwrap_or_default();
        if stock["id"] != HOME || stock["main"] == "lounge" {
            return Err("This TV's Home manifest is not one Lounge can stand in for".into());
        }
        Ok(())
    }

    /// On webOS 5 and 6 the Home key opens LG's full-screen Home where the TV says so.
    fn home_key_opens_full_home(&self) -> bool {
        let reply = self.luna(
            "luna://com.webos.settingsservice/getSystemSettings",
            json!({"category": "general", "keys": ["defaultApps"]}),
        );
        match reply {
            Ok(reply) => reply["settings"]["defaultApps"]["home"] == FULL_HOME,
            Err(error) => {
                self.system.log(&error);
                false
            }
        }
    }

    /// How Lounge can be this TV's Home, from what the TV has. `wanted`: a Home id asked for by
    /// hand (`home.sh enable APP_DIR com.webos.app.fullhome`).
    fn choose(&self, wanted: Option<&str>) -> Result<(Method, &'static str), String> {
        let wanted = match wanted {
            None => None,
            Some(id) => Some(
                TARGETS
                    .into_iter()
                    .find(|target| *target == id)
                    .ok_or_else(|| format!("{id} is not a Home Lounge can stand in for"))?,
            ),
        };
        let version = self
            .system
            .read(RELEASE)
            .ok()
            .as_deref()
            .and_then(webos_version);
        let Some(version) = version else {
            // No version to go by: only LG Home's own executable shape, the one every webOS 5
            // to 9 TV seen so far has.
            if wanted.is_some_and(|id| id != HOME) {
                return Err(format!(
                    "{FULL_HOME} as Lounge's Home is not supported where the webOS version is unknown"
                ));
            }
            self.native_home(HOME)?;
            return Ok((Method::Executable, HOME));
        };
        match version {
            5..=9 => {
                let id = match wanted {
                    Some(id) => id,
                    None if version <= 6 && self.home_key_opens_full_home() => FULL_HOME,
                    None => HOME,
                };
                if id == FULL_HOME && version > 6 {
                    return Err(format!(
                        "{FULL_HOME} as Lounge's Home is not supported on webOS {version}"
                    ));
                }
                self.native_home(id)?;
                Ok((Method::Executable, id))
            }
            10 | 11 => {
                if wanted.is_some_and(|id| id != HOME) {
                    return Err(format!(
                        "{FULL_HOME} as Lounge's Home is not supported on webOS {version}"
                    ));
                }
                self.manifest_home()?;
                Ok((Method::Manifest, HOME))
            }
            _ => Err(format!(
                "Lounge as Home is not supported on webOS {version} (webOS 5 to 11 are)"
            )),
        }
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

    /// Puts Lounge in Home's place. `open`: bring Home to the front even if it was not.
    fn apply(&self, settings: &Settings, open: bool) -> Result<(), String> {
        let (method, id) = settings.way()?;
        self.refuse_foreign(id)?;
        let app_dir = settings.app_dir.as_deref().unwrap_or("undefined");
        if !self.system.exists(&format!("{app_dir}/lounge")) {
            return Err("Lounge is no longer installed".into());
        }
        let front = self.foreground().unwrap_or_else(|error| {
            self.system.log(&error);
            String::new()
        });
        let applied = (|| {
            match method {
                Method::Executable => {
                    self.mount(app_dir, id)?;
                    self.system.write(MARKER, &format!("{APP}\n"))?;
                    if id != HOME {
                        self.system.write(TARGET_MARKER, &format!("{id}\n"))?;
                    }
                    self.remove_checkpoint(id)?;
                }
                Method::Manifest => {
                    self.mount_stage(app_dir)?;
                    self.system.write(MARKER, &format!("{APP}\n"))?;
                    self.reread(true)?;
                }
            }
            let stopped = self.stop_home(Some(app_dir), id)?;
            // Never take the screen from an input or app someone chose: only where the Home
            // just stopped was in front.
            if open || (front == id && stopped > 0) {
                self.open_home(id)?;
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

    /// Gives Home back to LG exactly: no mount, no marker, no hook, sam reading LG's own
    /// manifest. Works with or without a config, whichever way Lounge was Home.
    fn restore(&self) -> Result<(), String> {
        let (method, id) = self
            .config()
            .ok()
            .flatten()
            .and_then(|settings| settings.way().ok())
            .unwrap_or((Method::Executable, HOME));
        let front = match self.foreground() {
            Ok(front) => Some(front),
            Err(error) => {
                self.system
                    .log(&format!("{error}; restoring the files first"));
                None
            }
        };
        if self.exe_foreign(id)? {
            return Err("Another integration is mounted over LG Home".into());
        }
        self.system.unlink(MARKER)?;
        self.system.unlink(TARGET_MARKER)?;
        self.remove_hook()?;
        // Lounge's mount over either Home; another integration's over the other one stays.
        for target in TARGETS {
            let exe = exe_of(target);
            if ours(self.mount_root(&exe)?.as_deref()) {
                self.unmount(&exe, ours)?;
            }
        }
        let was_staged = self.stage_mounted()?;
        if was_staged {
            self.unmount(MANIFEST_DIR, staged)?;
        }
        if was_staged || method == Method::Manifest {
            self.reread(false)?;
        }
        if let Err(error) = self.remove_stage() {
            self.system.log(&error);
        }
        self.stop_home(None, id)?;
        // Nothing in front means Home itself went away: LG's Home takes its place.
        if front.is_some_and(|front| front == id || front.is_empty()) {
            self.launch_home(id)?;
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

    fn enable(&self, app_dir: Option<&str>, wanted: Option<&str>) -> Result<(), String> {
        let (method, id) = self.choose(wanted)?;
        let app_dir = self.installation(app_dir)?;
        self.refuse_foreign(id)?;
        if self.system.exists(HOOK) && !self.hook_owned()? {
            return Err(format!("{HOOK} is already in use"));
        }
        // Switching Home ways (another Home id, say) goes through LG's own Home.
        if let Some(saved) = self.config()?.filter(|settings| settings.enabled)
            && saved.way().ok() != Some((method, id))
        {
            return Err(format!(
                "Lounge is Home in another way here ({}); disable it first",
                saved.method
            ));
        }
        let mut settings = Settings {
            enabled: false,
            method: method.name().into(),
            app_dir: Some(app_dir),
            home_id: (id != HOME).then(|| id.to_owned()),
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

    /// At boot: the mount first, where it can go before sam starts Home, then the full apply
    /// once sam is up and its own CRIU boot work is done (bounded waits).
    fn boot(&self) -> Result<(), String> {
        let Some(settings) = self.config()?.filter(|settings| settings.enabled) else {
            return Ok(());
        };
        let app_dir = settings.app_dir.as_deref().unwrap_or("undefined");
        if !self.system.exists(&format!("{app_dir}/lounge")) {
            self.system.log("Lounge is not installed; LG Home stays");
            return Ok(());
        }
        let (method, id) = settings.way()?;
        if method == Method::Executable {
            self.mount_early(app_dir, id);
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
        self.home_after_boot(id);
        Ok(())
    }

    /// The mount, as early in a cold boot as the hook runs: no sam, no Luna. sam then starts
    /// Lounge as Home from the first time, and LG's Home never runs. Only where no image of
    /// LG's Home is left for sam to restore (CRIU refuses it with Lounge mounted, see
    /// `checkpoint_of`); `apply` then mounts after sam's boot copy, as before. Whatever happens
    /// here, the boot apply that follows checks and finishes it.
    fn mount_early(&self, app_dir: &str, id: &str) {
        let [checkpoint, persistent, _] = checkpoint_of(id);
        if self.system.exists(&checkpoint)
            || (self.system.exists(&persistent)
                && self
                    .system
                    .list(&persistent)
                    .map_or(true, |names| !names.is_empty()))
        {
            self.system
                .log("boot: an image of LG's Home is left; mounting after sam's boot copy");
            return;
        }
        let running = self
            .home_processes(Some(app_dir), id)
            .map_or(0, |pids| pids.len());
        let mounted = (|| {
            self.refuse_foreign(id)?;
            self.mount(app_dir, id)?;
            self.system.write(MARKER, &format!("{APP}\n"))?;
            if id != HOME {
                self.system.write(TARGET_MARKER, &format!("{id}\n"))?;
            }
            Ok::<(), String>(())
        })();
        self.system.log(&match mounted {
            Ok(()) if running == 0 => "boot: mounted before LG's Home started".to_owned(),
            Ok(()) => format!("boot: mounted; LG's Home was already running ({running})"),
            Err(error) => format!("boot: early mount failed: {error}"),
        });
    }

    /// A cold boot shows Home. LG's boot manager opens the last input (or Live TV) and, unlike
    /// a Quick Start wake, never starts Home: once it has, Home is opened over it. Only what the
    /// boot manager opens is replaced; an app someone opened meanwhile stays. Bounded: 60 s at
    /// most.
    fn home_after_boot(&self, id: &str) {
        for i in 0..30 {
            // sam is busy with the boot; ask again (and say why on the last try).
            let app = self.foreground().unwrap_or_else(|error| {
                if i == 29 {
                    self.system.log(&error);
                }
                String::new()
            });
            if app == id {
                return;
            }
            if !app.is_empty() && !startup_screen(&app) {
                self.system
                    .log(&format!("boot: {app} is in front; Home stays behind it"));
                return;
            }
            if !app.is_empty() {
                match self.launch_home(id) {
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
        let (method, id) = match &settings {
            Some(settings) => settings.way()?,
            None => (Method::Executable, HOME),
        };
        let app_dir = settings.as_ref().and_then(|s| s.app_dir.clone());
        let (ours_mounted, current) = match method {
            Method::Executable => (
                ours(self.mount_root(&exe_of(id))?.as_deref()),
                app_dir.as_deref().is_some_and(|dir| self.mounted(dir, id)),
            ),
            Method::Manifest => (
                self.stage_mounted()?,
                match app_dir.as_deref() {
                    Some(dir) => self.stage_current(dir)?,
                    None => false,
                },
            ),
        };
        let [checkpoint, persistent, _] = checkpoint_of(id);
        Ok(Status {
            return_value: true,
            method: method.name(),
            enabled: settings.is_some_and(|s| s.enabled),
            mounted: ours_mounted && current,
            stale: ours_mounted && !current,
            foreign: self.exe_foreign(id)? || self.manifest_foreign(id)?,
            hook: self.hook_owned()?,
            marker: self.system.exists(MARKER),
            checkpoint: self.system.exists(&checkpoint)
                || (self.system.exists(&persistent) && !self.system.list(&persistent)?.is_empty()),
            app_dir,
            home_id: (id != HOME).then_some(id),
        })
    }

    /// One `home.sh` command; the reply is what is true afterwards. `target`: the Home id
    /// `enable` stands in for, where not chosen from the TV.
    pub fn main(
        &self,
        command: &str,
        app_dir: Option<&str>,
        target: Option<&str>,
    ) -> Result<Status, String> {
        match command {
            "enable" => self.enable(
                app_dir.filter(|dir| !dir.is_empty()),
                target.filter(|id| !id.is_empty()),
            )?,
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

/// `lounge --home COMMAND [APP_DIR [HOME_ID]]`: the reply line and whether it succeeded.
pub fn command(
    home: &Home<impl System>,
    command: &str,
    app_dir: Option<&str>,
    target: Option<&str>,
) -> (String, bool) {
    match home.main(command, app_dir, target) {
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

    fn make_dir(&self, directory: &str) -> Result<(), String> {
        use std::os::unix::fs::DirBuilderExt;
        match std::fs::DirBuilder::new().mode(0o755).create(directory) {
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
                Err(format!("{directory}: {error}"))
            }
            _ => Ok(()),
        }
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
