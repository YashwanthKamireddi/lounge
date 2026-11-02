//! The Home commands against a model TV: its files, mounts, processes and Luna services.
//! Nothing touches this machine or a real TV.
use super::*;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

const APP_DIR: &str =
    "/media/developer/apps/usr/palm/applications/io.github.yashwanthkamireddi.lounge";
const LG_INODE: u64 = 100;
const LOUNGE_INODE: u64 = 200;
const OUR_ROOT: &str = "/apps/usr/palm/applications/io.github.yashwanthkamireddi.lounge/lounge";
const EXE: &str = "/usr/bin/com.webos.app.home";
const FULL_EXE: &str = "/usr/bin/com.webos.app.fullhome";
const FULL_DIR: &str = "/usr/palm/applications/com.webos.app.fullhome";
const CHECKPOINT: &str = "/tmp/checkpoint/com.webos.app.home";
const PERSISTENT_CHECKPOINT: &str = "/mnt/lg/cmn_data/checkpoint/com.webos.app.home";
const HASH_CHECK: &str = "/tmp/checkpoint/hash_check_com.webos.app.home";
/// The staged Home's root where /var is its own file system, as on webOS 10 and 11.
const STAGE_ROOT: &str = "/lib/lounge/app";
const STAGE_INODE: u64 = 300;
/// The boot log where LG's Home left an image: the mount waits for sam's boot copy.
const IMAGE_LEFT: &str = "boot: an image of LG's Home is left; mounting after sam's boot copy";
const WEBOS_7: &str = "Rockhopper release 7.6.0-11";
const WEBOS_5: &str = "Rockhopper release 5.5.0-31";
const WEBOS_6: &str = "Rockhopper release 6.4.0-21";
const WEBOS_10: &str = "Rockhopper release 10.3.1-3006";
const WEBOS_11: &str = "Rockhopper release 11.2.0-3701";

fn glasshouse() -> Value {
    json!({"handler": "handleGlasshouseKeys", "file": "/var/lib/tvweb/remotekeys-filter.js"})
}
fn old_filter() -> Value {
    json!({"handler": "handleLoungeHome", "file": FILTER})
}
fn stock_filter() -> Value {
    json!({"handler": "handleSystemKeys", "file": "/usr/lib/qt5/qml/KeyFilters/systemUi.js"})
}

#[derive(Clone)]
struct Mount {
    target: String,
    root: String,
    inode: u64,
}

fn our_mount() -> Mount {
    Mount {
        target: EXE.into(),
        root: OUR_ROOT.into(),
        inode: LOUNGE_INODE,
    }
}

struct Process {
    /// The command line, NUL-separated.
    cmdline: String,
    inode: u64,
    /// Ignores SIGTERM.
    stubborn: bool,
}

struct State {
    front: String,
    files: BTreeMap<String, String>,
    directories: BTreeSet<String>,
    links: BTreeMap<String, String>,
    inodes: BTreeMap<String, u64>,
    mounts: Vec<Mount>,
    processes: BTreeMap<u32, Process>,
    calls: Vec<String>,
    logs: Vec<String>,
    chain: Value,
    idle_app: Value,
    default_apps: Value,
    mount_takes: bool,
    /// A plain umount fails as "target is busy" (a running Home executes the file).
    busy: bool,
    normal_running: bool,
    next_pid: u32,
    /// sam's reading of com.webos.app.home's manifest: Lounge's staged one, or LG's.
    sam_reads_lounge: bool,
    /// A block of Home that sam has not been released from.
    blocked: bool,
    /// sam reads Home's manifest again on a release (else it keeps what it read).
    sam_rereads: bool,
    /// The TV's Home Auto Launch setting.
    home_auto_launch: &'static str,
    /// The boot manager opens this input in front after this many pauses (late, over Home).
    late_input: Option<(u32, &'static str)>,
    pauses: u32,
}

/// A process started as `argv0` with sam's launch argument.
fn process(argv0: &str, id: &str, inode: u64) -> Process {
    Process {
        cmdline: format!("{argv0}\0{{\"nid\":\"{id}\",\"appId\":\"{id}\"}}\0"),
        inode,
        stubborn: false,
    }
}

struct Fake(RefCell<State>);

fn not_found() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

impl Fake {
    /// What `path` is: the mount on top of it, else the file itself.
    fn inode(state: &State, path: &str) -> Option<u64> {
        if let Some(mount) = state.mounts.iter().rev().find(|mount| mount.target == path) {
            return Some(mount.inode);
        }
        if let Some(index) = state.directories.iter().position(|name| name == path) {
            return Some(
                state
                    .inodes
                    .get(path)
                    .copied()
                    .unwrap_or(2000 + index as u64),
            );
        }
        let index = state.files.keys().position(|name| name == path)?;
        Some(
            state
                .inodes
                .get(path)
                .copied()
                .unwrap_or(1000 + index as u64),
        )
    }

    fn exe_inode(state: &State) -> u64 {
        Self::inode(state, EXE).unwrap()
    }

    fn luna(&self, uri: &str, payload: Value) -> Value {
        let mut state = self.0.borrow_mut();
        let method = uri.rsplit('/').next().unwrap();
        if method == "getForegroundAppInfo" {
            return json!({"returnValue": true, "appId": state.front});
        }
        if method == "getSystemSettings" && payload["keys"] == json!(["homeAutoLaunch"]) {
            return json!({"returnValue": true, "settings": {"homeAutoLaunch": state.home_auto_launch}});
        }
        if method == "getAppInfo" {
            let main = if state.sam_reads_lounge {
                format!("{MANIFEST_DIR}/lounge")
            } else {
                format!("file://{EXE}")
            };
            return json!({"returnValue": true, "appInfo": {"id": HOME, "main": main}});
        }
        if method == "blockSystemApp" {
            state.calls.push(format!("blockSystemApp {payload}"));
            state.blocked = true;
            return json!({"returnValue": true});
        }
        state.calls.push(match payload["id"].as_str() {
            Some(id) => format!("{method} {id}"),
            None => method.to_owned(),
        });
        match method {
            "launch" => {
                state.front = payload["id"].as_str().unwrap().to_owned();
                json!({"returnValue": true})
            }
            "running" => {
                json!({"returnValue": true, "running": if state.normal_running { json!([{"id": APP}]) } else { json!([]) }})
            }
            "closeByAppId" => {
                state.normal_running = false;
                json!({"returnValue": true})
            }
            "getConfigs" => json!({"returnValue": true, "configs": {
                CHAIN: state.chain, "profile.idleApp": state.idle_app,
            }}),
            "setConfigs" => {
                if let Some(chain) = payload["configs"].get(CHAIN) {
                    state.chain = chain.clone();
                }
                if let Some(idle) = payload["configs"].get("profile.idleApp") {
                    state.idle_app = idle.clone();
                }
                json!({"returnValue": true})
            }
            "getSystemSettings" => json!({"returnValue": true, "settings":
                if state.default_apps.is_null() { json!({}) } else { json!({"defaultApps": state.default_apps}) }}),
            "deleteSystemSettings" => {
                state.default_apps = Value::Null;
                json!({"returnValue": true})
            }
            "releaseBlockSystemApp" => {
                if !state.blocked {
                    return json!({"returnValue": false, "errorText": "app is not blocked"});
                }
                state.blocked = false;
                if !state.sam_rereads {
                    return json!({"returnValue": true});
                }
                // sam reads the manifest that is there now.
                state.sam_reads_lounge = state
                    .mounts
                    .iter()
                    .rev()
                    .find(|mount| mount.target == MANIFEST_DIR)
                    .is_some_and(|mount| mount.root == STAGE_ROOT);
                json!({"returnValue": true})
            }
            _ => panic!("Unexpected Luna call {uri}"),
        }
    }
}

impl System for Fake {
    fn read(&self, file: &str) -> io::Result<String> {
        let state = self.0.borrow();
        if file == "/proc/self/mountinfo" {
            return Ok(state
                .mounts
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{} 20 179:54 {} {} rw - ext4", 30 + i, m.root, m.target))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        if let Some(pid) = file
            .strip_prefix("/proc/")
            .and_then(|rest| rest.strip_suffix("/cmdline"))
        {
            let process = state
                .processes
                .get(&pid.parse().unwrap())
                .ok_or_else(not_found)?;
            return Ok(process.cmdline.clone());
        }
        state.files.get(file).cloned().ok_or_else(not_found)
    }

    fn exists(&self, file: &str) -> bool {
        let state = self.0.borrow();
        if let Some(pid) = file.strip_prefix("/proc/")
            && let Ok(pid) = pid.parse()
        {
            return state.processes.contains_key(&pid);
        }
        state.files.contains_key(file)
            || state.directories.contains(file)
            || state.links.contains_key(file)
    }

    fn stat(&self, file: &str) -> Option<(u64, u64)> {
        let state = self.0.borrow();
        if let Some(pid) = file
            .strip_prefix("/proc/")
            .and_then(|rest| rest.strip_suffix("/exe"))
        {
            return Some((1, state.processes.get(&pid.parse().ok()?)?.inode));
        }
        Some((1, Self::inode(&state, file)?))
    }

    fn write(&self, file: &str, text: &str) -> Result<(), String> {
        self.0
            .borrow_mut()
            .files
            .insert(file.to_owned(), text.to_owned());
        Ok(())
    }

    fn unlink(&self, file: &str) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        if state.links.remove(file).is_none() {
            state.files.remove(file);
        }
        Ok(())
    }

    fn list(&self, directory: &str) -> Result<Vec<String>, String> {
        let prefix = format!("{directory}/");
        let state = self.0.borrow();
        Ok(state
            .files
            .keys()
            .chain(state.links.keys())
            .filter_map(|file| file.strip_prefix(&prefix).map(str::to_owned))
            .collect())
    }

    fn make_dir(&self, directory: &str) -> Result<(), String> {
        self.0.borrow_mut().directories.insert(directory.to_owned());
        Ok(())
    }

    fn remove_dir(&self, directory: &str) -> Result<(), String> {
        self.0.borrow_mut().directories.remove(directory);
        Ok(())
    }

    fn symlink(&self, target: &str, link: &str) -> Result<(), String> {
        self.0
            .borrow_mut()
            .links
            .insert(link.to_owned(), target.to_owned());
        Ok(())
    }

    fn read_link(&self, link: &str) -> Result<Option<String>, String> {
        Ok(self.0.borrow().links.get(link).cloned())
    }

    fn real_path(&self, path: &str) -> io::Result<String> {
        if self
            .0
            .borrow()
            .files
            .contains_key(&format!("{path}/appinfo.json"))
        {
            Ok(path.to_owned())
        } else {
            Err(not_found())
        }
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        match program {
            "/usr/bin/luna-send" => {
                let reply = self.luna(args[4], serde_json::from_str(args[5]).unwrap());
                Ok(reply.to_string())
            }
            "/bin/mount" => {
                let mut state = self.0.borrow_mut();
                state.calls.push(format!("mount {}", args.join(" ")));
                if state.mount_takes {
                    let inode = Self::inode(&state, args[1]).unwrap();
                    let root = if args[1] == STAGE {
                        STAGE_ROOT
                    } else {
                        OUR_ROOT
                    };
                    state.mounts.push(Mount {
                        target: args[2].into(),
                        root: root.into(),
                        inode,
                    });
                }
                Ok(String::new())
            }
            "/bin/umount" => {
                let mut state = self.0.borrow_mut();
                state.calls.push(format!("umount {}", args.join(" ")));
                if state.busy && args[0] != "-l" {
                    return Err(format!("/bin/umount {} failed: target is busy", args[0]));
                }
                let target = args.last().unwrap();
                if let Some(index) = state.mounts.iter().rposition(|m| m.target == *target) {
                    state.mounts.remove(index);
                }
                Ok(String::new())
            }
            _ => panic!("Unexpected command {program}"),
        }
    }

    fn pause(&self, _seconds: u64) {
        let mut state = self.0.borrow_mut();
        state.pauses += 1;
        if let Some((after, input)) = state.late_input
            && state.pauses == after
        {
            state.front = input.into();
        }
    }

    fn pids(&self) -> Result<Vec<u32>, String> {
        Ok(self.0.borrow().processes.keys().copied().collect())
    }

    fn kill(&self, pid: u32, signal: Signal) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        let name = match signal {
            Signal::Term => "SIGTERM",
            Signal::Kill => "SIGKILL",
        };
        state.calls.push(format!("kill {pid} {name}"));
        if signal == Signal::Kill || !state.processes.get(&pid).is_some_and(|p| p.stubborn) {
            state.processes.remove(&pid);
        }
        Ok(())
    }

    fn log(&self, line: &str) {
        // The start marker only times the boot (with diagnostics on); the tests check decisions.
        if line != "boot: started" && !line.starts_with("boot: image files:") {
            self.0.borrow_mut().logs.push(line.to_owned());
        }
    }
}

struct Options {
    front: &'static str,
    mounts: Vec<Mount>,
    config: Option<Value>,
    old_home: bool,
    normal_running: bool,
    /// /etc/starfish-release; None: the TV has none.
    release: Option<&'static str>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            front: "com.webos.app.livetv",
            mounts: vec![],
            config: None,
            old_home: true,
            normal_running: false,
            release: Some(WEBOS_7),
        }
    }
}

fn enabled() -> Value {
    json!({"enabled": true, "method": "executable", "appDir": APP_DIR})
}

fn tv(options: Options) -> Home<Fake> {
    let mut files = BTreeMap::new();
    for (file, text) in [
        (
            format!("{MANIFEST_DIR}/appinfo.json"),
            json!({"id": HOME, "type": "native", "main": format!("file://{EXE}"),
                   "nativeLifeCycleInterfaceVersion": 2})
            .to_string(),
        ),
        (
            format!("{APP_DIR}/appinfo.json"),
            json!({"id": APP, "type": "native", "main": "lounge"}).to_string(),
        ),
        (format!("{APP_DIR}/lounge"), "executable".into()),
        (EXE.into(), "LG Home".into()),
        (format!("{CHECKPOINT}/MD5SUM"), "sums".into()),
        (format!("{CHECKPOINT}/pages-1.img"), "link".into()),
        (HASH_CHECK.into(), "0".into()),
        (
            format!("{PERSISTENT_CHECKPOINT}/pages-1.img.ori"),
            "pages".into(),
        ),
        (
            "/mnt/lg/cmn_data/checkpoint/com.webos.app.inputcommon/pages-1.img.ori".into(),
            "pages".into(),
        ),
        (FILTER.into(), "filter".into()),
        ("/var/lib/lounge/home-button.js".into(), "helper".into()),
    ] {
        files.insert(file, text);
    }
    if let Some(config) = &options.config {
        files.insert(CONFIG.into(), serde_json::to_string_pretty(config).unwrap());
    }
    if let Some(release) = options.release {
        files.insert(RELEASE.into(), format!("{release}\n"));
    }
    let old = options.old_home;
    Home::new(Fake(RefCell::new(State {
        home_auto_launch: "on",
        late_input: None,
        pauses: 0,
        front: options.front.into(),
        files,
        directories: [
            "/var/lib/webosbrew/init.d",
            CHECKPOINT,
            PERSISTENT_CHECKPOINT,
            "/var/lib/lounge",
        ]
        .map(str::to_owned)
        .into(),
        links: BTreeMap::new(),
        inodes: [
            (format!("{APP_DIR}/lounge"), LOUNGE_INODE),
            (EXE.into(), LG_INODE),
            (STAGE.into(), STAGE_INODE),
        ]
        .into(),
        mounts: options.mounts,
        processes: [(1981, process(EXE, HOME, LG_INODE))].into(),
        calls: vec![],
        logs: vec![],
        chain: if old {
            json!([glasshouse(), old_filter(), stock_filter()])
        } else {
            json!([glasshouse(), stock_filter()])
        },
        idle_app: json!({"appId": if old { APP } else { HOME }}),
        default_apps: if old {
            json!({"home": APP})
        } else {
            Value::Null
        },
        mount_takes: true,
        busy: false,
        normal_running: options.normal_running,
        next_pid: 3000,
        sam_reads_lounge: false,
        blocked: false,
        sam_rereads: true,
    })))
}

impl Home<Fake> {
    fn state(&self) -> std::cell::RefMut<'_, State> {
        self.system.0.borrow_mut()
    }

    fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }

    fn run(&self, command: &str) -> Result<Status, String> {
        self.main(command, None, None)
    }

    /// Lounge started as Home by sam.
    fn start_lounge_as_home(&self) {
        let mut state = self.state();
        let pid = state.next_pid;
        state.next_pid += 1;
        let inode = Fake::exe_inode(&state);
        state.processes.insert(pid, process(EXE, HOME, inode));
    }
}

fn config_of(home: &Home<Fake>) -> Value {
    serde_json::from_str(&home.state().files[CONFIG]).unwrap()
}

#[test]
fn enable_puts_lounge_in_home_place_removes_the_old_home_button_and_opens_home() {
    let home = tv(Options {
        normal_running: true,
        ..Options::default()
    });
    home.run("enable").unwrap();
    assert_eq!(
        home.calls(),
        [
            "getConfigs".to_owned(),
            "setConfigs".into(),
            "setConfigs".into(),
            "getSystemSettings".into(),
            "deleteSystemSettings".into(),
            format!("mount --bind {APP_DIR}/lounge {EXE}"),
            "kill 1981 SIGTERM".into(),
            "launch com.webos.app.home".into(),
            "running".into(),
            format!("closeByAppId {APP}"),
        ]
    );
    {
        let state = home.state();
        // LG's own filters and Glasshouse's stay, in order; the Home settings are stock again.
        assert_eq!(state.chain, json!([glasshouse(), stock_filter()]));
        assert_eq!(state.idle_app, json!({"appId": HOME}));
        assert!(state.default_apps.is_null());
        assert!(!state.files.contains_key(FILTER));
        assert!(!state.files.contains_key("/var/lib/lounge/home-button.js"));
        let backup: Value = serde_json::from_str(&state.files[BACKUP]).unwrap();
        assert_eq!(backup["idleApp"]["appId"], APP);
        assert_eq!(backup["defaultApps"]["home"], APP);
        // No image of LG's Home is left for sam to restore over Lounge, in RAM or persistent.
        assert!(
            !state
                .files
                .keys()
                .any(|file| file.starts_with(&format!("{CHECKPOINT}/")))
        );
        assert!(!state.files.contains_key(HASH_CHECK));
        assert!(
            !state
                .files
                .contains_key(&format!("{PERSISTENT_CHECKPOINT}/pages-1.img.ori"))
        );
        assert!(
            state.files.contains_key(
                "/mnt/lg/cmn_data/checkpoint/com.webos.app.inputcommon/pages-1.img.ori"
            )
        );
        assert_eq!(state.links[HOOK], "/var/lib/lounge/home.sh");
        assert_eq!(state.files[MARKER], format!("{APP}\n"));
        assert_eq!(state.front, HOME);
    }
    assert_eq!(config_of(&home)["enabled"], true);
    // home.sh reads the settings line by line.
    let text = home.state().files[CONFIG].clone();
    assert!(
        text.contains("\"enabled\": true") && text.contains(&format!("\"appDir\": \"{APP_DIR}\""))
    );
    let status = home.status().unwrap();
    assert!(status.mounted && status.hook && !status.checkpoint && !status.stale);
    assert_eq!(status.method, "executable");
}

#[test]
fn the_status_reply_keeps_its_shape() {
    let home = tv(Options {
        config: Some(enabled()),
        mounts: vec![our_mount()],
        ..Options::default()
    });
    let (reply, ok) = command(&home, "status", None, None);
    assert!(ok);
    assert_eq!(
        reply,
        format!(
            "{{\"returnValue\":true,\"method\":\"executable\",\"enabled\":true,\"mounted\":true,\
             \"stale\":false,\"foreign\":false,\"hook\":false,\"marker\":false,\"checkpoint\":true,\
             \"appDir\":\"{APP_DIR}\"}}"
        )
    );
    let (reply, ok) = command(&home, "dance", None, None);
    assert!(!ok);
    assert_eq!(
        reply,
        r#"{"returnValue":false,"errorText":"Unknown command dance"}"#
    );
    assert_eq!(
        command(&tv(Options::default()), "status", None, None).0,
        "{\"returnValue\":true,\"method\":\"executable\",\"enabled\":false,\"mounted\":false,\
         \"stale\":false,\"foreign\":false,\"hook\":false,\"marker\":false,\"checkpoint\":true,\
         \"appDir\":null}"
    );
}

#[test]
fn enable_on_a_clean_tv_changes_no_settings() {
    let home = tv(Options {
        old_home: false,
        ..Options::default()
    });
    home.run("enable").unwrap();
    assert_eq!(
        home.calls()[..3],
        [
            "getConfigs".to_owned(),
            "getSystemSettings".into(),
            format!("mount --bind {APP_DIR}/lounge {EXE}"),
        ]
    );
}

#[test]
fn enable_takes_the_package_it_is_given_and_refuses_one_that_is_not_lounge() {
    let home = tv(Options::default());
    assert!(
        home.main(
            "enable",
            Some("/media/cryptofs/apps/usr/palm/applications/other"),
            None
        )
        .unwrap_err()
        .contains("No installed native Lounge package")
    );
    assert!(home.calls().is_empty());
    home.main("enable", Some(APP_DIR), None).unwrap();
    assert_eq!(config_of(&home)["appDir"], APP_DIR);
}

#[test]
fn another_integration_over_lg_home_stops_enable_before_anything_changes() {
    for mount in [
        Mount {
            target: MANIFEST_DIR.into(),
            root: "/tvweb/home-takeover".into(),
            inode: 7,
        },
        Mount {
            target: EXE.into(),
            root: "/var/lib/other/launcher".into(),
            inode: 7,
        },
    ] {
        let home = tv(Options {
            mounts: vec![mount],
            ..Options::default()
        });
        assert!(
            home.run("enable")
                .unwrap_err()
                .contains("Another integration")
        );
        let state = home.state();
        assert!(state.calls.is_empty() && state.links.is_empty());
        assert!(!state.files.contains_key(MARKER));
    }
}

#[test]
fn a_mount_that_does_not_take_is_rolled_back_to_stock_lg_home() {
    let home = tv(Options {
        old_home: false,
        ..Options::default()
    });
    home.state().mount_takes = false;
    assert!(home.run("enable").unwrap_err().contains("did not take"));
    {
        let state = home.state();
        assert!(state.links.is_empty() && state.mounts.is_empty());
        assert!(!state.files.contains_key(MARKER));
    }
    assert_eq!(config_of(&home)["enabled"], false);
    assert!(!home.status().unwrap().mounted);
}

#[test]
fn a_cold_boot_opens_lounge_over_the_input_or_live_tv_the_boot_manager_chose() {
    for front in [
        "com.webos.app.hdmi2",
        "com.webos.app.livetv",
        "com.webos.app.externalinput.av1",
    ] {
        let home = tv(Options {
            config: Some(enabled()),
            front,
            ..Options::default()
        });
        home.run("boot").unwrap();
        assert_eq!(
            home.calls(),
            [
                format!("mount --bind {APP_DIR}/lounge {EXE}"),
                "kill 1981 SIGTERM".into(),
                "launch com.webos.app.home".into(),
            ]
        );
        assert_eq!(home.state().front, HOME);
        assert_eq!(
            home.state().logs,
            [
                IMAGE_LEFT.to_owned(),
                format!("boot: opened Home over {front}"),
            ]
        );
    }
}

#[test]
fn a_cold_boot_leaves_an_app_someone_opened_in_front() {
    for front in [
        "netflix",
        "com.webos.app.hdmi",
        "com.webos.app.externalinput.AV1",
    ] {
        let home = tv(Options {
            config: Some(enabled()),
            front,
            ..Options::default()
        });
        home.run("boot").unwrap();
        assert_eq!(
            home.calls(),
            [
                format!("mount --bind {APP_DIR}/lounge {EXE}"),
                "kill 1981 SIGTERM".to_owned(),
            ]
        );
        assert_eq!(home.state().front, front);
        assert_eq!(
            home.state().logs,
            [
                IMAGE_LEFT.to_owned(),
                format!("boot: {front} is in front; Home stays behind it"),
            ]
        );
    }
}

#[test]
fn the_boot_apply_opens_lounge_where_lg_home_was_in_front() {
    let home = tv(Options {
        config: Some(enabled()),
        front: HOME,
        ..Options::default()
    });
    home.run("boot").unwrap();
    assert_eq!(
        home.calls(),
        [
            format!("mount --bind {APP_DIR}/lounge {EXE}"),
            "kill 1981 SIGTERM".into(),
            "launch com.webos.app.home".into(),
        ]
    );
}

#[test]
fn boot_does_nothing_when_disabled_or_when_lounge_was_removed() {
    for config in [
        None,
        Some(json!({"enabled": false, "method": "executable", "appDir": APP_DIR})),
    ] {
        let home = tv(Options {
            config,
            ..Options::default()
        });
        home.run("boot").unwrap();
        assert!(home.calls().is_empty());
    }
    let removed = tv(Options {
        config: Some(enabled()),
        ..Options::default()
    });
    removed.state().files.remove(&format!("{APP_DIR}/lounge"));
    removed.run("boot").unwrap();
    assert!(removed.calls().is_empty());
}

#[test]
fn applying_again_keeps_a_running_lounge_home() {
    let home = tv(Options {
        config: Some(enabled()),
        mounts: vec![our_mount()],
        front: HOME,
        ..Options::default()
    });
    home.state().processes.clear();
    home.start_lounge_as_home();
    home.run("apply").unwrap();
    assert!(home.calls().is_empty());
    assert_eq!(home.state().processes.len(), 1);
}

#[test]
fn after_a_package_update_the_mount_is_stale_and_applying_mounts_the_new_file() {
    let stale = Mount {
        target: EXE.into(),
        root: format!("{OUR_ROOT}//deleted"),
        inode: 150,
    };
    let home = tv(Options {
        config: Some(enabled()),
        mounts: vec![stale],
        ..Options::default()
    });
    home.state().processes.clear();
    assert!(home.status().unwrap().stale);
    home.run("apply").unwrap();
    assert_eq!(
        home.calls(),
        [
            format!("umount {EXE}"),
            format!("mount --bind {APP_DIR}/lounge {EXE}")
        ]
    );
    assert!(home.status().unwrap().mounted);
}

#[test]
fn disable_gives_home_back_to_lg_exactly() {
    let home = tv(Options {
        config: Some(enabled()),
        mounts: vec![our_mount()],
        front: HOME,
        ..Options::default()
    });
    home.state().processes.clear();
    home.start_lounge_as_home();
    home.state()
        .links
        .insert(HOOK.into(), "/var/lib/lounge/home.sh".into());
    home.state().files.insert(MARKER.into(), format!("{APP}\n"));
    home.run("disable").unwrap();
    assert_eq!(
        home.calls(),
        [
            format!("umount {EXE}"),
            "kill 3000 SIGTERM".into(),
            "launch com.webos.app.home".into(),
        ]
    );
    {
        let state = home.state();
        assert!(state.mounts.is_empty() && state.links.is_empty());
        assert!(!state.files.contains_key(MARKER));
    }
    assert_eq!(config_of(&home)["enabled"], false);
}

#[test]
fn a_busy_mount_is_detached_and_a_home_that_ignores_sigterm_is_killed() {
    let home = tv(Options {
        config: Some(enabled()),
        mounts: vec![our_mount()],
        front: "youtube.leanback.v4",
        ..Options::default()
    });
    home.state().busy = true;
    home.state().processes.get_mut(&1981).unwrap().stubborn = true;
    home.run("disable").unwrap();
    assert_eq!(
        home.calls(),
        [
            format!("umount {EXE}"),
            format!("umount -l {EXE}"),
            "kill 1981 SIGTERM".into(),
            "kill 1981 SIGKILL".into(),
        ]
    );
    assert!(home.state().mounts.is_empty());
    assert!(home.state().logs[0].ends_with("target is busy; detaching the mount"));
}

#[test]
fn disable_leaves_an_app_in_front_and_opens_lg_home_where_home_itself_had_gone() {
    let app = tv(Options {
        config: Some(enabled()),
        mounts: vec![our_mount()],
        front: "youtube.leanback.v4",
        ..Options::default()
    });
    app.run("disable").unwrap();
    assert!(
        !app.calls()
            .contains(&"launch com.webos.app.home".to_owned())
    );
    let gone = tv(Options {
        config: Some(enabled()),
        mounts: vec![our_mount()],
        front: "",
        ..Options::default()
    });
    gone.run("disable").unwrap();
    assert!(
        gone.calls()
            .contains(&"launch com.webos.app.home".to_owned())
    );
}

#[test]
fn disable_works_without_any_record_of_an_enable_and_never_unmounts_a_foreign_mount() {
    tv(Options::default()).run("disable").unwrap();
    let foreign = tv(Options {
        mounts: vec![Mount {
            target: EXE.into(),
            root: "/var/lib/other/launcher".into(),
            inode: 7,
        }],
        ..Options::default()
    });
    assert!(
        foreign
            .run("disable")
            .unwrap_err()
            .contains("Another integration")
    );
    assert_eq!(foreign.state().mounts.len(), 1);
}

// A cold boot where the hook runs before sam starts Home.

#[test]
fn a_cold_boot_mounts_lounge_before_sam_starts_home_where_no_lg_image_is_left() {
    let home = tv(Options {
        config: Some(enabled()),
        old_home: false,
        ..Options::default()
    });
    {
        let mut state = home.state();
        state
            .files
            .retain(|file, _| !file.contains("checkpoint/com.webos.app.home"));
        state.files.remove(HASH_CHECK);
        state.directories.remove(CHECKPOINT);
        // sam has not started any Home yet.
        state.processes.clear();
    }
    home.run("boot").unwrap();
    assert_eq!(
        home.calls(),
        [
            format!("mount --bind {APP_DIR}/lounge {EXE}"),
            "launch com.webos.app.home".into(),
        ]
    );
    assert_eq!(
        home.state().logs,
        [
            "boot: mounted before LG's Home started",
            "boot: opened Home over com.webos.app.livetv",
        ]
    );
    // What sam starts as Home from now on is Lounge.
    home.start_lounge_as_home();
    assert!(
        home.state()
            .processes
            .values()
            .all(|p| p.inode == LOUNGE_INODE)
    );
    assert_eq!(home.state().files[MARKER], format!("{APP}\n"));
}

#[test]
fn a_cold_boot_where_lg_home_already_runs_mounts_early_and_then_replaces_it() {
    let home = tv(Options {
        config: Some(enabled()),
        front: HOME,
        old_home: false,
        ..Options::default()
    });
    {
        let mut state = home.state();
        state
            .files
            .retain(|file, _| !file.contains("checkpoint/com.webos.app.home"));
        state.directories.remove(CHECKPOINT);
    }
    home.run("boot").unwrap();
    assert_eq!(
        home.calls(),
        [
            format!("mount --bind {APP_DIR}/lounge {EXE}"),
            "kill 1981 SIGTERM".into(),
            "launch com.webos.app.home".into(),
        ]
    );
    assert_eq!(
        home.state().logs[0],
        "boot: mounted; LG's Home was already running (1)"
    );
}

// webOS 5 and 6: LG's Home and full-screen Home are both native lifecycle-v2 programs.

const FULL_INODE: u64 = 101;

fn full_home_manifest() -> String {
    json!({"id": FULL_HOME, "type": "native", "main": format!("file://{FULL_EXE}"),
           "nativeLifeCycleInterfaceVersion": 2})
    .to_string()
}

/// `key_opens_full`: the TV's Home key opens com.webos.app.fullhome (general.defaultApps).
fn full_home_tv(release: &'static str, key_opens_full: bool, options: Options) -> Home<Fake> {
    let home = tv(Options {
        release: Some(release),
        old_home: false,
        ..options
    });
    {
        let mut state = home.state();
        state
            .files
            .insert(format!("{FULL_DIR}/appinfo.json"), full_home_manifest());
        state.files.insert(FULL_EXE.into(), "LG full Home".into());
        state.inodes.insert(FULL_EXE.into(), FULL_INODE);
        state
            .processes
            .insert(1982, process(FULL_EXE, FULL_HOME, FULL_INODE));
        if key_opens_full {
            state.default_apps = json!({"home": FULL_HOME});
        }
    }
    home
}

fn full_enabled() -> Value {
    json!({"enabled": true, "method": "executable", "appDir": APP_DIR, "homeId": FULL_HOME})
}

fn full_mount() -> Mount {
    Mount {
        target: FULL_EXE.into(),
        root: OUR_ROOT.into(),
        inode: LOUNGE_INODE,
    }
}

#[test]
fn on_webos_5_and_6_lounge_stands_in_for_the_home_the_home_key_opens() {
    for release in [WEBOS_5, WEBOS_6] {
        let home = full_home_tv(release, true, Options::default());
        home.run("enable").unwrap();
        assert_eq!(
            home.calls(),
            [
                // Which Home the Home key opens, then the old Home Button's settings.
                "getSystemSettings".to_owned(),
                "getConfigs".into(),
                "getSystemSettings".into(),
                format!("mount --bind {APP_DIR}/lounge {FULL_EXE}"),
                "kill 1982 SIGTERM".into(),
                "launch com.webos.app.fullhome".into(),
                "running".into(),
            ]
        );
        {
            let state = home.state();
            // LG's other Home is left as it is.
            assert_eq!(state.mounts.len(), 1);
            assert!(state.processes.contains_key(&1981));
            assert_eq!(state.files[TARGET_MARKER], format!("{FULL_HOME}\n"));
            assert_eq!(state.files[MARKER], format!("{APP}\n"));
            assert_eq!(state.front, FULL_HOME);
            // The full-screen Home's setting is the TV's own: it stays.
            assert_eq!(state.default_apps, json!({"home": FULL_HOME}));
        }
        assert_eq!(config_of(&home), full_enabled());
        let (reply, ok) = command(&home, "status", None, None);
        assert!(ok);
        assert_eq!(
            reply,
            format!(
                "{{\"returnValue\":true,\"method\":\"executable\",\"enabled\":true,\"mounted\":true,\
                 \"stale\":false,\"foreign\":false,\"hook\":true,\"marker\":true,\"checkpoint\":false,\
                 \"appDir\":\"{APP_DIR}\",\"homeId\":\"{FULL_HOME}\"}}"
            )
        );
        // Applying again keeps the Lounge sam started as the full-screen Home.
        home.state()
            .processes
            .insert(3100, process(FULL_EXE, FULL_HOME, LOUNGE_INODE));
        home.state().calls.clear();
        home.run("apply").unwrap();
        assert!(home.calls().is_empty());
        // Disable gives that Home back, and only that one.
        home.run("disable").unwrap();
        assert_eq!(
            home.calls(),
            [
                format!("umount {FULL_EXE}"),
                "kill 3100 SIGTERM".into(),
                "launch com.webos.app.fullhome".into(),
            ]
        );
        let state = home.state();
        assert!(state.mounts.is_empty() && state.links.is_empty());
        assert!(!state.files.contains_key(MARKER) && !state.files.contains_key(TARGET_MARKER));
        assert!(state.processes.contains_key(&1981));
    }
}

#[test]
fn on_webos_5_the_usual_home_is_the_default_and_the_full_screen_one_can_be_named() {
    let home = full_home_tv(WEBOS_5, false, Options::default());
    home.run("enable").unwrap();
    assert_eq!(
        home.calls()[2..4],
        [
            "getSystemSettings".to_owned(),
            format!("mount --bind {APP_DIR}/lounge {EXE}"),
        ]
    );
    assert!(config_of(&home).get("homeId").is_none());
    // Another Home only after LG's own is back.
    assert!(
        home.main("enable", None, Some(FULL_HOME))
            .unwrap_err()
            .contains("disable it first")
    );
    home.run("disable").unwrap();
    home.state().calls.clear();
    home.main("enable", None, Some(FULL_HOME)).unwrap();
    assert!(
        home.calls()
            .contains(&format!("mount --bind {APP_DIR}/lounge {FULL_EXE}"))
    );
    assert_eq!(config_of(&home), full_enabled());
}

#[test]
fn a_cold_boot_on_webos_5_mounts_the_full_screen_home_first() {
    let home = full_home_tv(
        WEBOS_5,
        true,
        Options {
            config: Some(full_enabled()),
            ..Options::default()
        },
    );
    home.run("boot").unwrap();
    assert_eq!(
        home.calls(),
        [
            format!("mount --bind {APP_DIR}/lounge {FULL_EXE}"),
            "kill 1982 SIGTERM".into(),
            "launch com.webos.app.fullhome".into(),
        ]
    );
    assert_eq!(
        home.state().logs,
        [
            "boot: mounted; LG's Home was already running (1)",
            "boot: opened Home over com.webos.app.livetv",
        ]
    );
    assert_eq!(home.state().files[TARGET_MARKER], format!("{FULL_HOME}\n"));
}

#[test]
fn a_foreign_mount_over_the_full_screen_home_is_never_touched() {
    let home = full_home_tv(
        WEBOS_6,
        true,
        Options {
            mounts: vec![Mount {
                target: FULL_EXE.into(),
                root: "/var/lib/other/launcher".into(),
                inode: 7,
            }],
            ..Options::default()
        },
    );
    assert!(
        home.run("enable")
            .unwrap_err()
            .contains("Another integration")
    );
    // Lounge was never that Home here: disable gives back the one it was, and leaves it.
    home.run("disable").unwrap();
    // Where Lounge's settings name that Home, nothing is changed at all.
    home.state()
        .files
        .insert(CONFIG.into(), full_enabled().to_string());
    assert!(
        home.run("disable")
            .unwrap_err()
            .contains("Another integration")
    );
    assert_eq!(home.state().mounts.len(), 1);
    assert!(
        !home
            .state()
            .calls
            .iter()
            .any(|call| call.starts_with("umount"))
    );
}

#[test]
fn disable_with_a_full_home_mount_but_no_settings_still_gives_it_back() {
    let home = full_home_tv(
        WEBOS_5,
        true,
        Options {
            mounts: vec![full_mount()],
            ..Options::default()
        },
    );
    home.run("disable").unwrap();
    assert!(home.state().mounts.is_empty());
}

// webOS 10 and 11: LG Home's role is bound to its app id; Lounge's staged Home app is mounted
// over LG Home's app folder and sam reads it again.

fn staged_tv(release: &'static str, options: Options) -> Home<Fake> {
    let home = tv(Options {
        release: Some(release),
        old_home: false,
        ..options
    });
    {
        let mut state = home.state();
        state.files.insert(
            format!("{MANIFEST_DIR}/appinfo.json"),
            json!({"id": HOME, "type": "native", "main": "/usr/bin/flutter-client"}).to_string(),
        );
        state.files.remove(EXE);
        state.files.insert(
            format!("{APP_DIR}/appinfo.json"),
            json!({"id": APP, "type": "native", "main": "lounge", "title": "Lounge",
                   "icon": "app-icon.png", "largeIcon": "../escape.png",
                   "nativeLifeCycleInterfaceVersion": 2})
            .to_string(),
        );
        state
            .files
            .insert(format!("{APP_DIR}/app-icon.png"), "icon".into());
        for directory in ["assets", "lib"] {
            state.directories.insert(format!("{APP_DIR}/{directory}"));
        }
        state.processes.insert(
            1981,
            Process {
                cmdline: "/usr/bin/flutter-client\0-i\0com.webos.app.home\0".into(),
                inode: 50,
                stubborn: false,
            },
        );
        if release == WEBOS_11 {
            state.files.insert(
                BLOCK_SCHEMA.into(),
                r#"{"properties": {"id": {}, "forceBlockSystemApp": {}}}"#.into(),
            );
        }
    }
    home
}

fn staged_enabled() -> Value {
    json!({"enabled": true, "method": "manifest", "appDir": APP_DIR})
}

fn lounge_staged(home: &Home<Fake>) -> u32 {
    let mut state = home.state();
    let pid = state.next_pid;
    state.next_pid += 1;
    state.processes.insert(
        pid,
        process(&format!("{MANIFEST_DIR}/lounge"), HOME, LOUNGE_INODE),
    );
    pid
}

#[test]
fn on_webos_10_and_11_the_staged_home_is_mounted_and_sam_reads_it() {
    for release in [WEBOS_10, WEBOS_11] {
        let home = staged_tv(release, Options::default());
        home.run("enable").unwrap();
        let calls = home.calls();
        assert_eq!(
            calls[..3],
            [
                "getConfigs".to_owned(),
                "getSystemSettings".into(),
                format!("mount --bind {STAGE} {MANIFEST_DIR}"),
            ]
        );
        // webOS 11's sam needs forceBlockSystemApp with the block; webOS 10's has no such field.
        assert!(calls[3].starts_with("blockSystemApp "));
        assert_eq!(
            calls[3].contains("\"forceBlockSystemApp\":false"),
            release == WEBOS_11
        );
        assert_eq!(
            calls[4..],
            [
                "releaseBlockSystemApp com.webos.app.home".to_owned(),
                "kill 1981 SIGTERM".into(),
                "launch com.webos.app.home".into(),
                "running".into(),
            ]
        );
        {
            let state = home.state();
            assert!(state.sam_reads_lounge && !state.blocked);
            let staged: Value =
                serde_json::from_str(&state.files[&format!("{STAGE}/appinfo.json")]).unwrap();
            assert_eq!(staged["id"], HOME);
            assert_eq!(staged["main"], "lounge");
            assert_eq!(staged["visible"], false);
            assert_eq!(staged["appCategory"], json!(["home"]));
            assert!(staged.get("nativeLifeCycleInterfaceVersion").is_none());
            assert_eq!(
                state.links[&format!("{STAGE}/lounge")],
                format!("{APP_DIR}/lounge")
            );
            assert_eq!(
                state.links[&format!("{STAGE}/lib")],
                format!("{APP_DIR}/lib")
            );
            assert_eq!(
                state.links[&format!("{STAGE}/app-icon.png")],
                format!("{APP_DIR}/app-icon.png")
            );
            // Artwork outside the package is never linked.
            assert!(!state.links.keys().any(|link| link.contains("escape")));
            assert_eq!(state.files[MARKER], format!("{APP}\n"));
        }
        assert_eq!(config_of(&home), staged_enabled());
        let status = home.status().unwrap();
        assert!(status.mounted && status.hook && !status.stale && !status.foreign);
        assert_eq!(status.method, "manifest");
        // Applying again keeps the running staged Home and asks sam nothing new.
        let lounge = lounge_staged(&home);
        home.state().calls.clear();
        home.run("apply").unwrap();
        assert!(home.calls().is_empty());
        // Disable: off LG Home's folder, sam reads LG's manifest again, the stage is gone.
        home.run("disable").unwrap();
        let calls = home.calls();
        assert_eq!(calls[0], format!("umount {MANIFEST_DIR}"));
        assert!(calls[1].starts_with("blockSystemApp "));
        assert_eq!(
            calls[2..],
            [
                "releaseBlockSystemApp com.webos.app.home".to_owned(),
                format!("kill {lounge} SIGTERM"),
                "launch com.webos.app.home".into(),
            ]
        );
        let state = home.state();
        assert!(!state.sam_reads_lounge && !state.blocked);
        assert!(state.mounts.is_empty() && state.links.is_empty());
        assert!(!state.directories.contains(STAGE));
        assert!(!state.files.keys().any(|file| file.starts_with(STAGE)));
        assert!(!state.files.contains_key(MARKER));
    }
}

#[test]
fn a_cold_boot_on_webos_10_mounts_the_staged_home_once_sam_is_up() {
    let home = staged_tv(
        WEBOS_10,
        Options {
            config: Some(staged_enabled()),
            ..Options::default()
        },
    );
    home.run("boot").unwrap();
    let calls = home.calls();
    assert_eq!(calls[0], format!("mount --bind {STAGE} {MANIFEST_DIR}"));
    assert_eq!(
        calls[2..],
        [
            "releaseBlockSystemApp com.webos.app.home".to_owned(),
            "kill 1981 SIGTERM".into(),
            "launch com.webos.app.home".into(),
        ]
    );
    assert!(home.state().sam_reads_lounge);
}

#[test]
fn a_staged_home_sam_does_not_read_is_rolled_back_to_lg_home() {
    let home = staged_tv(WEBOS_11, Options::default());
    home.state().sam_rereads = false;
    assert!(
        home.run("enable")
            .unwrap_err()
            .contains("did not read Home's manifest")
    );
    let state = home.state();
    assert!(state.mounts.is_empty() && state.links.is_empty() && !state.blocked);
    assert!(!state.directories.contains(STAGE));
    assert!(!state.files.contains_key(MARKER));
    assert!(!state.sam_reads_lounge);
}

#[test]
fn disable_finds_a_staged_home_without_settings_and_never_touches_a_foreign_one() {
    let left = staged_tv(
        WEBOS_10,
        Options {
            mounts: vec![Mount {
                target: MANIFEST_DIR.into(),
                root: STAGE_ROOT.into(),
                inode: STAGE_INODE,
            }],
            ..Options::default()
        },
    );
    left.state().sam_reads_lounge = true;
    left.run("disable").unwrap();
    assert!(left.state().mounts.is_empty() && !left.state().sam_reads_lounge);
    let foreign = staged_tv(
        WEBOS_10,
        Options {
            mounts: vec![Mount {
                target: MANIFEST_DIR.into(),
                root: "/tvweb/home-takeover".into(),
                inode: 7,
            }],
            ..Options::default()
        },
    );
    assert!(
        foreign
            .run("enable")
            .unwrap_err()
            .contains("Another integration")
    );
    assert!(foreign.calls().is_empty());
    foreign.run("disable").unwrap();
    assert_eq!(foreign.state().mounts.len(), 1);
    assert!(foreign.status().unwrap().foreign);
}

// Anything else is refused before anything changes, in words Settings can classify.

#[test]
fn a_home_lounge_cannot_stand_in_for_is_refused_before_anything_changes() {
    let flutter_home = |release| {
        let home = tv(Options {
            release: Some(release),
            ..Options::default()
        });
        home.state().files.insert(
            format!("{MANIFEST_DIR}/appinfo.json"),
            json!({"id": HOME, "type": "native", "main": "/usr/bin/flutter-client"}).to_string(),
        );
        home
    };
    let cases: Vec<(Home<Fake>, Option<&str>)> = vec![
        // webOS 4 and an unknown future webOS.
        (
            tv(Options {
                release: Some("Rockhopper release 4.9.0-53"),
                ..Options::default()
            }),
            None,
        ),
        (
            tv(Options {
                release: Some("Rockhopper release 12.0.0-1"),
                ..Options::default()
            }),
            None,
        ),
        // webOS 9 with a Home that is not a native lifecycle-v2 program.
        (flutter_home("Rockhopper release 9.2.0-10"), None),
        // No version to go by, and not LG's executable Home either.
        (
            {
                let home = flutter_home(WEBOS_7);
                home.state().files.remove(RELEASE);
                home
            },
            None,
        ),
        // The full-screen Home only on webOS 5 and 6, and only where the TV has one.
        (tv(Options::default()), Some(FULL_HOME)),
        (
            tv(Options {
                release: Some(WEBOS_5),
                ..Options::default()
            }),
            Some(FULL_HOME),
        ),
        (staged_tv(WEBOS_10, Options::default()), Some(FULL_HOME)),
        // A Home that is no Home at all.
        (tv(Options::default()), Some("com.webos.app.livetv")),
    ];
    for (home, target) in cases {
        let (reply, ok) = command(&home, "enable", None, target);
        assert!(!ok);
        let text = serde_json::from_str::<Value>(&reply).unwrap()["errorText"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            text.contains("stand in for") || text.contains("not supported"),
            "{text}"
        );
        let state = home.state();
        assert!(state.calls.is_empty() && state.mounts.is_empty() && state.links.is_empty());
        assert!(!state.files.contains_key(CONFIG) && !state.files.contains_key(MARKER));
    }
}

#[test]
fn without_a_release_file_lg_executable_home_is_still_lounge_to_stand_in_for() {
    let home = tv(Options {
        release: None,
        ..Options::default()
    });
    home.run("enable").unwrap();
    assert!(home.status().unwrap().mounted);
}

#[test]
fn the_webos_version_is_read_from_the_release_line() {
    assert_eq!(webos_version("Rockhopper release 7.6.0-11"), Some(7));
    assert_eq!(webos_version("Rockhopper release 10.3.1-3006\n"), Some(10));
    assert_eq!(webos_version("Ostrich Release 5.5.0"), Some(5));
    assert_eq!(webos_version("release seven"), None);
    assert_eq!(webos_version(""), None);
    assert!(staged(Some("/lib/lounge/app")) && staged(Some("/var/lib/lounge/app//deleted")));
    assert!(!staged(Some("/tvweb/home-takeover")) && !staged(None));
}

#[test]
fn only_a_mount_of_lounge_executable_is_lounge_also_once_replaced() {
    assert!(ours(Some(OUR_ROOT)));
    assert!(ours(Some(
        "/cryptofs/apps/usr/palm/applications/io.github.yashwanthkamireddi.lounge/lounge//deleted"
    )));
    assert!(!ours(Some("/var/lib/tvweb/home-takeover")));
    assert!(!ours(None));
}

#[test]
fn errors_are_one_bounded_line() {
    let long = "x".repeat(1000);
    let reply: Value = serde_json::from_str(&failure(&long)).unwrap();
    assert_eq!(reply["errorText"].as_str().unwrap().len(), 400);
}

/// The TV's own runner: luna-send, mount and umount never hold a command past their budget.
#[cfg(all(feature = "webos", unix))]
#[test]
fn a_program_runs_to_its_end_or_is_stopped_at_its_deadline() {
    use std::time::{Duration, Instant};
    let second = Duration::from_secs(1);
    assert_eq!(
        run_bounded("/bin/sh", &["-c", "echo reply; echo noise >&2"], second).unwrap(),
        "reply\n"
    );
    let failed = run_bounded(
        "/bin/sh",
        &["-c", "echo target is busy >&2; exit 32"],
        second,
    );
    assert_eq!(
        failed.unwrap_err(),
        "/bin/sh -c echo target is busy >&2; exit 32 failed: target is busy"
    );
    let started = Instant::now();
    assert!(
        run_bounded(
            "/bin/sh",
            &["-c", "exec sleep 30"],
            Duration::from_millis(200)
        )
        .unwrap_err()
        .ends_with("timed out")
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(run_bounded("/nonexistent/luna-send", &[], second).is_err());
}

#[test]
fn a_cold_boot_opens_lounge_over_an_input_the_boot_manager_opens_late_unless_auto_launch_is_off() {
    // The boot manager opened the last input well after Home was up (seen on webOS 22: Home at
    // 43 s, HDMI 2 at 66 s): Home opens over it.
    let home = tv(Options {
        config: Some(enabled()),
        front: HOME,
        ..Options::default()
    });
    home.state().late_input = Some((5, "com.webos.app.hdmi2"));
    home.run("boot").unwrap();
    assert_eq!(home.calls().last().unwrap(), "launch com.webos.app.home");
    assert_eq!(home.state().front, HOME);
    // With the TV's Home Auto Launch off, the input it opens stays, as with LG's Home.
    let off = tv(Options {
        config: Some(enabled()),
        front: "com.webos.app.hdmi2",
        ..Options::default()
    });
    off.state().home_auto_launch = "off";
    off.run("boot").unwrap();
    assert!(
        !off.calls()
            .iter()
            .any(|call| call == "launch com.webos.app.home")
    );
}
