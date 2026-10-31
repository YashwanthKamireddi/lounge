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
    argv0: String,
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
}

struct Fake(RefCell<State>);

fn not_found() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

impl Fake {
    fn exe_inode(state: &State) -> u64 {
        state
            .mounts
            .iter()
            .rev()
            .find(|mount| mount.target == EXE)
            .map_or(LG_INODE, |mount| mount.inode)
    }

    fn luna(&self, uri: &str, payload: Value) -> Value {
        let mut state = self.0.borrow_mut();
        let method = uri.rsplit('/').next().unwrap();
        if method == "getForegroundAppInfo" {
            return json!({"returnValue": true, "appId": state.front});
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
            return Ok(format!(
                "{}\0{{\"appId\":\"com.webos.app.home\"}}\0",
                process.argv0
            ));
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
        if file == EXE {
            return Some((1, Self::exe_inode(&state)));
        }
        let index = state.files.keys().position(|name| name == file)?;
        Some((
            1,
            state
                .inodes
                .get(file)
                .copied()
                .unwrap_or(1000 + index as u64),
        ))
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
        Ok(self
            .0
            .borrow()
            .files
            .keys()
            .filter_map(|file| file.strip_prefix(&prefix).map(str::to_owned))
            .collect())
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
                    let inode = state.inodes[args[1]];
                    state.mounts.push(Mount {
                        target: args[2].into(),
                        root: OUR_ROOT.into(),
                        inode,
                    });
                }
                Ok(String::new())
            }
            "/bin/umount" => {
                let mut state = self.0.borrow_mut();
                state.calls.push(format!("umount {}", args.join(" ")));
                if state.busy && args[0] != "-l" {
                    return Err(format!("/bin/umount {EXE} failed: target is busy"));
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

    fn pause(&self, _seconds: u64) {}

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
        self.0.borrow_mut().logs.push(line.to_owned());
    }
}

struct Options {
    front: &'static str,
    mounts: Vec<Mount>,
    config: Option<Value>,
    old_home: bool,
    normal_running: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            front: "com.webos.app.livetv",
            mounts: vec![],
            config: None,
            old_home: true,
            normal_running: false,
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
    let old = options.old_home;
    Home::new(Fake(RefCell::new(State {
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
        inodes: [(format!("{APP_DIR}/lounge"), LOUNGE_INODE)].into(),
        mounts: options.mounts,
        processes: [(
            1981,
            Process {
                argv0: EXE.into(),
                inode: LG_INODE,
                stubborn: false,
            },
        )]
        .into(),
        calls: vec![],
        logs: vec![],
        chain: if old {
            json!([glasshouse(), old_filter(), stock_filter()])
        } else {
            json!([glasshouse(), stock_filter()])
        },
        idle_app: json!({"appId": if old { "com.ecklf.luma" } else { HOME }}),
        default_apps: if old {
            json!({"home": APP})
        } else {
            Value::Null
        },
        mount_takes: true,
        busy: false,
        normal_running: options.normal_running,
        next_pid: 3000,
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
        self.main(command, None)
    }

    /// Lounge started as Home by sam.
    fn start_lounge_as_home(&self) {
        let mut state = self.state();
        let pid = state.next_pid;
        state.next_pid += 1;
        let inode = Fake::exe_inode(&state);
        state.processes.insert(
            pid,
            Process {
                argv0: EXE.into(),
                inode,
                stubborn: false,
            },
        );
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
        assert_eq!(backup["idleApp"]["appId"], "com.ecklf.luma");
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
    let (reply, ok) = command(&home, "status", None);
    assert!(ok);
    assert_eq!(
        reply,
        format!(
            "{{\"returnValue\":true,\"method\":\"executable\",\"enabled\":true,\"mounted\":true,\
             \"stale\":false,\"foreign\":false,\"hook\":false,\"marker\":false,\"checkpoint\":true,\
             \"appDir\":\"{APP_DIR}\"}}"
        )
    );
    let (reply, ok) = command(&home, "dance", None);
    assert!(!ok);
    assert_eq!(
        reply,
        r#"{"returnValue":false,"errorText":"Unknown command dance"}"#
    );
    assert_eq!(
        command(&tv(Options::default()), "status", None).0,
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
            Some("/media/cryptofs/apps/usr/palm/applications/other")
        )
        .unwrap_err()
        .contains("No installed native Lounge package")
    );
    assert!(home.calls().is_empty());
    home.main("enable", Some(APP_DIR)).unwrap();
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
            [format!("boot: opened Home over {front}")]
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
            [format!("boot: {front} is in front; Home stays behind it")]
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
