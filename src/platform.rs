//! Blocking platform work runs on one worker, never in the SDL/UI thread.
//! TV calls originate from this native executable's LS2 connection, not a CLI
//! process trying to impersonate its application identity.
use crate::{
    app_artwork,
    model::{App, Input, Inventory, demo_inventory, valid_app_id},
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::PathBuf, sync::mpsc, thread};

pub const APP_ID: &str = "io.github.yashwanthkamireddi.lounge";
pub const HOME_ID: &str = "com.webos.app.home";

/// While Lounge is hidden, LG's stock Home surfacing (an app was closed, or the
/// TV woke) is answered at once by bringing Lounge forward. Push-based: no polling.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
pub fn watch_foreground(
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake: Wake,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    // webOS's own foreground event decides, not Lounge's window flag: SDL
    // reports visibility late and out of order, and trusting it left LG Home
    // on screen after quitting some system apps.
    let returning = std::sync::Arc::new(AtomicBool::new(false));
    let mut previous = String::new();
    crate::luna::subscribe(
        "luna://com.webos.applicationManager/getForegroundAppInfo",
        &json!({"subscribe": true}),
        move |reply| {
            // A denied or dropped subscription replies returnValue:false: hand
            // back to polling rather than silently stop watching.
            if reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
                status(&format!(
                    "foreground events ended: {reply}; polling meanwhile"
                ));
                alive.store(false, Ordering::Relaxed);
                let (alive, wake) = (alive.clone(), wake.clone());
                resubscribe("foreground events", move || {
                    watch_foreground(alive.clone(), wake.clone())?;
                    alive.store(true, Ordering::Relaxed);
                    Ok(())
                });
                return;
            }
            let front = reply
                .get("appId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            eprintln!("Lounge foreground: {front:?} at {} ms", clock_ms());
            note_opened(&front);
            let closed = std::mem::replace(&mut previous, front.clone());
            // An app closing leaves nothing in front for a moment before
            // webOS raises LG Home: act then, so LG Home never shows. LG Home
            // itself in front is the fallback. Lounge launching an app (it was
            // in front) is not a close.
            let app_closed =
                front.is_empty() && !closed.is_empty() && closed != APP_ID && closed != HOME_ID;
            if (front != HOME_ID && !app_closed) || returning.swap(true, Ordering::AcqRel) {
                return;
            }
            let returning = returning.clone();
            let wake = wake.clone();
            thread::spawn(move || {
                let front_now = || {
                    call(
                        "luna://com.webos.applicationManager/getForegroundAppInfo",
                        json!({}),
                    )
                    .ok()
                    .and_then(|reply| reply.get("appId")?.as_str().map(str::to_owned))
                    .unwrap_or_default()
                };
                // A gap between two apps (one launching another) also leaves
                // nothing in front: wait a beat and stand aside if an app came.
                if app_closed {
                    thread::sleep(std::time::Duration::from_millis(120));
                    let now = front_now();
                    if !now.is_empty() && now != HOME_ID {
                        returning.store(false, Ordering::Release);
                        return;
                    }
                }
                // Raising an already-running Lounge is cheap, so re-assert
                // often: LG Home loses the race within a frame or two.
                for _ in 0..12 {
                    raise_self();
                    wake();
                    thread::sleep(std::time::Duration::from_millis(250));
                    let now = front_now();
                    eprintln!("Lounge return: front is now {now:?} at {} ms", clock_ms());
                    if now != HOME_ID && !now.is_empty() {
                        break;
                    }
                }
                returning.store(false, Ordering::Release);
            });
        },
    )
}

/// Wall-clock milliseconds, for timing foreground events in the debug log.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
fn clock_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_millis())
}

/// Waking from standby (LG's Quick Start+) is a power-on as far as anyone
/// watching knows: Lounge comes to the front, as it does after a cold start.
/// Only a wake counts; the screen coming back from Screen Off or the
/// screensaver leaves whatever was playing in front.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
pub fn watch_power() -> Result<(), String> {
    use std::sync::atomic::Ordering;
    // Kept across resubscribes, so a stream that restarts around a wake still
    // knows the TV was off.
    static PREVIOUS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    crate::luna::subscribe(
        "luna://com.webos.service.tvpower/power/getPowerState",
        &json!({"subscribe": true}),
        move |reply| {
            if reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
                status(&format!("power events ended: {reply}"));
                resubscribe("power events", watch_power);
                return;
            }
            let Some(state) = reply.get("state").and_then(Value::as_str) else {
                return;
            };
            // Turning off: the state stays "Active" while LG works through
            // "Request Power Off" / "Prepare Active Standby", then becomes
            // "Active Standby" (or "Suspend"). webOS asks every app to quit
            // on the way; Lounge stays resident instead (see `STANDBY`).
            let processing = reply
                .get("processing")
                .and_then(Value::as_str)
                .unwrap_or("");
            let Ok(mut previous) = PREVIOUS.lock() else {
                return;
            };
            let (going, woke) = power_step(&previous, state, processing);
            if going != STANDBY.swap(going, Ordering::Relaxed) {
                status(&format!(
                    "power: {state}{}",
                    if processing.is_empty() {
                        String::new()
                    } else {
                        format!(" ({processing})")
                    }
                ));
            }
            *previous = state.to_owned();
            drop(previous);
            if woke {
                status("woke from standby: Lounge to the front");
                // LG's quick-boot daemon closed every app at power-off; webOS
                // replaces a run that stayed (it kills it, ~1.9 s, then starts
                // a fresh one). So this run asks for Lounge and steps aside at
                // once: the fresh Lounge starts without that wait.
                thread::spawn(|| {
                    thread::spawn(raise_self);
                    thread::sleep(std::time::Duration::from_millis(150));
                    status("woke: launch asked; this run steps aside for a fresh start");
                    std::process::exit(0);
                });
            }
        },
    )
}

/// What one reply of LG's power stream means, given the state before it:
/// (the TV is off or on its way there, it has just woken). Turning off, LG
/// reports "Active" with a power-off `processing` step ("Request Power
/// Off", "Prepare Active Standby"), then "Active Standby" or "Suspend". The
/// screen saver ("Request Screen Saver", "Screen Saver") and Screen Off
/// leave the TV on: a quit then is a real quit. A wake is "Active" straight
/// after Standby or Suspend.
#[cfg_attr(
    not(all(feature = "webos", target_os = "linux", target_arch = "arm")),
    allow(dead_code)
)]
pub fn power_step(previous: &str, state: &str, processing: &str) -> (bool, bool) {
    let off = |text: &str| {
        ["Standby", "Suspend", "Power Off"]
            .iter()
            .any(|word| text.contains(word))
    };
    let going = off(state) || off(processing);
    let woke = state == "Active"
        && processing.is_empty()
        && (previous.contains("Standby") || previous.contains("Suspend"));
    (going, woke)
}

/// The TV is turning off, or is off (Active Standby, Suspend): a quit webOS
/// sends then is the power-off sweep, not a request to close Lounge. Lounge
/// stays resident, hidden, so the wake brings it straight back to the front
/// (LG's own power-on otherwise restores the last input).
pub static STANDBY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
pub fn watch_power() -> Result<(), String> {
    Err("Power events need an ARM webOS build".into())
}

/// Subscribes again after a stream ended (LG's service restarted, say), so
/// Lounge never stops listening until a relaunch: after 3 s, then 10, then
/// every 30 s until the bus takes it.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
fn resubscribe(what: &'static str, subscribe: impl Fn() -> Result<(), String> + Send + 'static) {
    // A stream that ends again soon after it came back (a service refusing
    // it, say) waits longer each time: 3 s, 10 s, 30 s, then every 5 minutes,
    // until it has held for 10 minutes.
    static ENDINGS: std::sync::Mutex<Vec<(&str, u32, Option<std::time::Instant>)>> =
        std::sync::Mutex::new(Vec::new());
    let first = ENDINGS.lock().map_or(3, |mut endings| {
        let now = std::time::Instant::now();
        let index = endings
            .iter()
            .position(|(name, ..)| *name == what)
            .unwrap_or_else(|| {
                endings.push((what, 0, None));
                endings.len() - 1
            });
        let (_, count, last) = &mut endings[index];
        if last.is_some_and(|at| now.duration_since(at) > std::time::Duration::from_secs(600)) {
            *count = 0;
        }
        *last = Some(now);
        *count += 1;
        [3, 10, 30, 300][(*count as usize - 1).min(3)]
    });
    thread::spawn(move || {
        for (attempt, wait) in [first]
            .into_iter()
            .chain([10])
            .chain(std::iter::repeat(30))
            .enumerate()
        {
            thread::sleep(std::time::Duration::from_secs(wait));
            match subscribe() {
                Ok(()) => {
                    status(&format!("{what}: on again (attempt {})", attempt + 1));
                    return;
                }
                Err(error) => eprintln!("Lounge {what}: {error}; trying again"),
            }
        }
    });
}

/// Keep `cache` holding webOS's open apps, pushed on every change, so the app
/// switcher opens at once from memory instead of asking the bus on Back.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
pub fn watch_running(
    cache: std::sync::Arc<std::sync::Mutex<Option<Vec<String>>>>,
) -> Result<(), String> {
    let again = cache.clone();
    crate::luna::subscribe(
        "luna://com.webos.applicationManager/running",
        &json!({"subscribe": true}),
        move |reply| {
            let ids = reply.get("running").map(card_ids);
            if ids.is_none() && reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
                status(&format!("running events ended: {reply}"));
                let cache = again.clone();
                resubscribe("running events", move || watch_running(cache.clone()));
            }
            // A dropped subscription clears the cache: Back then asks directly.
            if let Ok(mut cache) = cache.lock() {
                *cache = ids;
            }
        },
    )
}

/// The remote's Home key launches Lounge (Glasshouse's key filter). When Lounge
/// is already in front, LG's SDL swallows that launch, so no window event
/// arrives; webOS still reports it as a lifecycle "launch", which calls `home`.
///
/// LG Home being launched (LG ThinQ's Home button and others go straight to
/// it, past the remote's key filter) is answered the same moment, before it
/// draws, by raising Lounge: `take_home` is false in safe mode.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
pub fn watch_launches(home: Wake, take_home: bool) -> Result<(), String> {
    crate::luna::subscribe(
        "luna://com.webos.applicationManager/getAppLifeEvents",
        &json!({"subscribe": true}),
        move |reply| {
            if reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
                status(&format!("launch events ended: {reply}"));
                let home = home.clone();
                resubscribe("launch events", move || {
                    watch_launches(home.clone(), take_home)
                });
                return;
            }
            if reply.get("event").and_then(Value::as_str) != Some("launch") {
                return;
            }
            match reply.get("appId").and_then(Value::as_str) {
                // Lounge raising itself (wake, LG Home taken over) is not the
                // Home key: that would close Settings and jump to the top.
                Some(APP_ID) if raised_self_recently() => {}
                Some(APP_ID) => home(),
                Some(HOME_ID) if take_home => {
                    // Off the bus thread: a call from inside its callback
                    // would wait on itself.
                    thread::spawn(raise_self);
                }
                _ => {}
            }
        },
    )
}

#[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
pub fn watch_launches(_home: Wake, _take_home: bool) -> Result<(), String> {
    Err("Launch events need an ARM webOS build".into())
}

#[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
pub fn watch_running(
    _cache: std::sync::Arc<std::sync::Mutex<Option<Vec<String>>>>,
) -> Result<(), String> {
    Err("Running-app events need an ARM webOS build".into())
}

#[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
pub fn watch_foreground(
    _alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _wake: Wake,
) -> Result<(), String> {
    Err("Foreground events need an ARM webOS build".into())
}

/// Marks Lounge's own launches of itself (bringing itself back over LG Home),
/// so the relaunch webOS reports for them is not taken for the Home key.
fn self_launch() -> Value {
    json!({"lounge": "self"})
}

/// When Lounge last launched itself (see `raise_self`).
static RAISED: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Notes that Lounge is launching itself: the launch event that follows is
/// Lounge's own, not the Home key.
fn note_raised() {
    if let Ok(mut raised) = RAISED.lock() {
        *raised = Some(std::time::Instant::now());
    }
}

/// Brings Lounge to the front by launching itself (see `note_raised`).
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
fn raise_self() {
    note_raised();
    let _ = call(
        "luna://com.webos.applicationManager/launch",
        json!({"id": APP_ID, "params": self_launch()}),
    );
}

#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
fn raised_self_recently() -> bool {
    RAISED
        .lock()
        .ok()
        .and_then(|raised| *raised)
        .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(1500))
}

/// Wakes Lounge's event loop from another thread (see main's `wake`).
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;

pub enum Request {
    Refresh,
    Launch(String),
    /// Which app webOS currently shows (read while Lounge is hidden).
    Foreground,
    Running,
    Close(String),
    /// The remote's Home key: Some(true/false) to give it to Lounge or back
    /// to LG, None to read which it is.
    Home(Option<bool>),
    /// Remove an installed app (confirmed in Lounge's alert first).
    Uninstall(String),
}
pub enum Response {
    Inventory(Result<(Inventory, Option<String>), String>),
    Profile(Option<crate::model::Profile>),
    Launched(Result<(), String>),
    Foreground(Option<String>),
    Running(Vec<String>),
    Closed(String, Result<(), String>),
    /// Whether the Home key opens Lounge, after the request.
    Home(Result<bool, String>),
    Uninstalled(String, Result<(), String>),
}

pub struct Platform {
    sender: mpsc::Sender<Request>,
    pub receiver: mpsc::Receiver<Response>,
    pub webos: bool,
    /// Open apps as webOS last pushed them (None until the first event, or
    /// once the subscription drops).
    pub running: std::sync::Arc<std::sync::Mutex<Option<Vec<String>>>>,
}

impl Platform {
    /// `fixture` (desktop only) is a directory holding a TV's raw `listApps.json`
    /// and `inputs.json` replies, parsed exactly as the TV parses them.
    pub fn new(webos: bool, fixture: Option<PathBuf>) -> Self {
        let (sender, requests) = mpsc::channel();
        let (responses, receiver) = mpsc::channel();
        // The preview's own removals: its sample apps stay gone on refresh.
        let removed = std::sync::Arc::new(std::sync::Mutex::new(HashSet::<String>::new()));
        thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                let response = match request {
                    // Reading the app list (through Homebrew's elevated exec)
                    // can take seconds: run it beside this worker so a launch,
                    // close or setting pressed meanwhile never waits behind it.
                    // State's `loading` flag keeps it to one read at a time.
                    Request::Refresh => {
                        let responses = responses.clone();
                        let fixture = fixture.clone();
                        let removed = removed.clone();
                        thread::spawn(move || {
                            let mut inventory = if webos {
                                inventory()
                            } else if let Some(directory) = &fixture {
                                fixture_inventory(directory)
                            } else {
                                Ok((demo_inventory(), None))
                            };
                            if let (Ok((inventory, _)), Ok(removed)) =
                                (&mut inventory, removed.lock())
                            {
                                inventory.apps.retain(|app| !removed.contains(&app.id));
                            }
                            let _ = responses.send(Response::Inventory(inventory));
                            let profile = if webos {
                                read_profile().unwrap_or_else(|error| {
                                    eprintln!("Lounge profile: {error}");
                                    None
                                })
                            } else {
                                crate::model::Profile::from_fields("Lounge", Some("#7360E7"))
                            };
                            let _ = responses.send(Response::Profile(profile));
                        });
                        continue;
                    }
                    Request::Running => Response::Running(if webos {
                        running_apps().unwrap_or_default()
                    } else {
                        fixture
                            .as_ref()
                            .and_then(|dir| std::fs::read(dir.join("running.json")).ok())
                            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                            .and_then(|value| value.get("running").map(card_ids))
                            .unwrap_or_else(|| {
                                ["netflix", "hotstar", "youtube.leanback.v4", "amazon"]
                                    .map(str::to_owned)
                                    .to_vec()
                            })
                    }),
                    Request::Close(id) => {
                        let result = if !webos {
                            Ok(())
                        } else if !valid_app_id(&id) {
                            Err("Invalid application identifier".into())
                        } else {
                            elevated_close(&id)
                        };
                        Response::Closed(id, result)
                    }
                    // Removal waits for webOS to finish (seconds), on its own
                    // thread: a launch or close meanwhile never queues behind it.
                    Request::Uninstall(id) => {
                        let responses = responses.clone();
                        let removed = removed.clone();
                        thread::spawn(move || {
                            let result = if webos {
                                elevated_uninstall(&id)
                            } else {
                                crate::uninstall::command(&id).map(|_| {
                                    if let Ok(mut removed) = removed.lock() {
                                        removed.insert(id.clone());
                                    }
                                })
                            };
                            let _ = responses.send(Response::Uninstalled(id, result));
                        });
                        continue;
                    }
                    Request::Home(set) => Response::Home(if webos {
                        let result = home_button(set);
                        status(&match &result {
                            Ok(true) => "home button: opens Lounge".to_owned(),
                            Ok(false) => "home button: opens LG Home".to_owned(),
                            Err(error) => format!("home button: {error}"),
                        });
                        result
                    } else {
                        Err("The Home button is set on the TV".into())
                    }),
                    Request::Foreground => Response::Foreground(if webos {
                        call(
                            "luna://com.webos.applicationManager/getForegroundAppInfo",
                            json!({}),
                        )
                        .ok()
                        .and_then(|reply| reply.get("appId")?.as_str().map(str::to_owned))
                    } else {
                        None
                    }),
                    // A launch never queues behind a slower request (closing
                    // an app through elevated exec, reading settings): it
                    // goes to the bus at once on its own thread.
                    Request::Launch(id) => {
                        let responses = responses.clone();
                        thread::spawn(move || {
                            let result = if !webos {
                                Ok(())
                            } else if crate::scroll_preview::is_placeholder(&id) {
                                Err("Scroll preview cards cannot be launched".into())
                            } else if !valid_app_id(&id) {
                                Err("Invalid application identifier".into())
                            } else {
                                let payload = if id == APP_ID {
                                    note_raised();
                                    json!({"id": id, "params": self_launch()})
                                } else {
                                    json!({"id": id})
                                };
                                call("luna://com.webos.applicationManager/launch", payload)
                                    .map(|_| ())
                            };
                            // Lounge raising itself is no app launch: no reply,
                            // so it never clears an app's opening or shows its error.
                            if id != APP_ID {
                                let _ = responses.send(Response::Launched(result));
                            }
                        });
                        continue;
                    }
                };
                if responses.send(response).is_err() {
                    break;
                }
            }
        });
        Self {
            sender,
            receiver,
            webos,
            running: std::sync::Arc::default(),
        }
    }

    pub fn send(&self, request: Request) {
        let _ = self.sender.send(request);
    }
}

/// What Lounge's TV connections are doing, one line each, in /tmp (memory,
/// gone at reboot): readable on the TV without a debug restart.
const STATUS: &str = "/tmp/lounge-status";

/// A new Lounge starts its own section of the status log: the previous run's
/// last lines are kept (how it ended matters most, e.g. across a power
/// cycle), bounded so the file in /tmp (memory) never grows.
pub fn status_reset() {
    const KEEP: usize = 150;
    let previous = std::fs::read_to_string(STATUS).unwrap_or_default();
    let lines: Vec<&str> = previous.lines().collect();
    let kept = lines[lines.len().saturating_sub(KEEP)..].join("\n");
    let _ = std::fs::write(STATUS, if kept.is_empty() { kept } else { kept + "\n" });
    status(&format!("{STARTED}{})", std::process::id()));
}

/// Opens a run's section of the status log.
const STARTED: &str = "--- started (pid ";
/// A run's opening lines kept when its section is shortened: start-up's
/// stages and which TV events are on.
const RUN_HEAD: usize = 40;
/// Its latest lines kept.
const RUN_TAIL: usize = 110;
/// Lines a run appends before its section is shortened again. Every Home
/// press adds one, so a run of weeks grew the file (in memory) without end.
const RUN_APPENDS: usize = 200;
/// Lines appended since the section was last shortened; also serialises
/// writers (the TV watchers log from their own threads).
static APPENDED: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

/// Records `line` (also on stderr).
pub fn status(line: &str) {
    eprintln!("Lounge {line}");
    use std::io::Write;
    let mut appended = APPENDED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(STATUS)
        .and_then(|mut file| {
            // Milliseconds: a power-on is a matter of a few seconds.
            let at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            writeln!(file, "{}.{:03} {line}", at.as_secs(), at.subsec_millis())
        });
    *appended += 1;
    if *appended >= RUN_APPENDS {
        *appended = 0;
        if let Some(shorter) = std::fs::read_to_string(STATUS)
            .ok()
            .and_then(|log| shorten(&log))
        {
            let _ = std::fs::write(STATUS, shorter);
        }
    }
}

/// `log` with this run's section cut to its opening and latest lines (and
/// earlier runs' lines as they are), or None when it is short enough.
fn shorten(log: &str) -> Option<String> {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines
        .iter()
        .rposition(|line| line.contains(STARTED))
        .unwrap_or(0);
    let run = &lines[start..];
    if run.len() <= RUN_HEAD + RUN_TAIL + 1 {
        return None;
    }
    let left_out = run.len() - RUN_HEAD - RUN_TAIL;
    let mut out: Vec<String> = lines[..start + RUN_HEAD]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    out.push(format!("... {left_out} lines left out"));
    out.extend(
        run[run.len() - RUN_TAIL..]
            .iter()
            .map(|line| (*line).to_owned()),
    );
    Some(out.join("\n") + "\n")
}

/// LG's preloader starts Lounge once at boot (a one-shot entry in its
/// supportAppList). Lounge comes forward only over the boot screen, LG Home; with
/// an app in front, or no answer, it stands aside before any window exists.
pub fn preload_may_show() -> bool {
    call(
        "luna://com.webos.applicationManager/getForegroundAppInfo",
        json!({}),
    )
    .ok()
    .and_then(|reply| reply.get("appId")?.as_str().map(str::to_owned))
    .is_some_and(|front| front.is_empty() || front == HOME_ID || front == APP_ID)
}

fn call(uri: &str, payload: Value) -> Result<Value, String> {
    #[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
    let response = crate::luna::call(uri, &payload);
    #[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
    let response: Result<Value, String> = {
        let _ = payload;
        Err("Native Luna services are only available in an ARM webOS build".into())
    };
    response.and_then(checked_response).map_err(|error| {
        let error = format!("{uri}: {error}");
        eprintln!("Lounge TV service: {error}");
        error
    })
}

fn checked_response(value: Value) -> Result<Value, String> {
    if value.get("returnValue").and_then(Value::as_bool) != Some(true) {
        return Err(value
            .get("errorText")
            .and_then(Value::as_str)
            .unwrap_or("The TV denied this action")
            .chars()
            .take(240)
            .collect());
    }
    Ok(value)
}

/// The signed-in LG profile, read as LG Home reads it (its account data is
/// private to system apps, so through Homebrew's elevated service). The
/// command is fixed and filters on the TV: only the nickname and avatar
/// colour ever leave it, never the account's e-mail or numbers.
fn read_profile() -> Result<Option<crate::model::Profile>, String> {
    let command = r#"if [ "$(id -u)" != 0 ]; then echo '{"returnValue":false}'; exit 0; fi
U=$(/usr/bin/luna-send -n 1 -w 4000 luna://com.webos.service.accountmanager/getLoginID '{"serviceName":"LGE"}' | sed -n 's/.*"lastSignInUserNo":"\([A-Za-z0-9]*\)".*/\1/p')
[ -n "$U" ] || { echo '{"returnValue":true,"profile":{}}'; exit 0; }
F=$(/usr/bin/luna-send -n 1 -w 4000 luna://com.webos.service.accountmanager/getAccountData "{\"serviceName\":\"LGE\",\"userNo\":\"$U\",\"keys\":[\"userData\"]}" | grep -oE '"profile(Nick|Bg)":"[^"\\]*"' | tr '\n' ',' | sed 's/,$//')
echo "{\"returnValue\":true,\"profile\":{$F}}""#;
    let envelope = call(
        "luna://org.webosbrew.hbchannel.service/exec",
        json!({"command": command}),
    )?;
    let profile = inventory_payload(&envelope, "profile")?;
    Ok(crate::model::Profile::from_fields(
        profile
            .get("profileNick")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        profile.get("profileBg").and_then(Value::as_str),
    ))
}

fn inventory_read(uri: &str, field: &str) -> Result<Value, String> {
    // Only these fixed inventory commands use Homebrew's elevated service.
    // Use the same one-reply invocation as the web launcher, without guessing
    // CLI behavior from a firmware-version string.
    let command = format!(
        "if [ \"$(id -u)\" = 0 ]; then /usr/bin/luna-send -n 1 -w 4000 {uri} '{{}}'; else printf '%s\\n' '{{\"returnValue\":false,\"errorText\":\"Homebrew Channel needs root access\"}}'; fi"
    );
    let envelope = call(
        "luna://org.webosbrew.hbchannel.service/exec",
        json!({"command": command}),
    )?;
    inventory_payload(&envelope, field).map_err(|error| {
        let error = format!("{uri}: {error}");
        eprintln!("Lounge TV inventory: {error}");
        error
    })
}

pub fn read_app_icons(source: &app_artwork::Source) -> Result<Vec<Vec<u8>>, String> {
    if !cfg!(feature = "webos") {
        return Err("Installed artwork is only read on TV builds".into());
    }
    // Fast path: the app list already names the icon file. One elevated
    // `od` dump of it answers in milliseconds, where starting node on this
    // TV takes seconds (and can time out under load). Paths are validated
    // in Rust and limited to plain filename characters before they reach
    // the shell.
    let plain = |path: &str| {
        !path.is_empty()
            && path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
    };
    let files: Vec<String> = source
        .listed_icon_paths()
        .into_iter()
        .filter_map(|path| path.to_str().map(str::to_owned))
        .filter(|path| plain(path))
        .collect();
    if !files.is_empty() {
        let tries = files
            .iter()
            .map(|file| {
                format!(
                    "if [ -f {file} ] && [ $(wc -c < {file}) -le {limit} ]; then od -An -v -tx1 {file} | tr -d ' \\n'; exit 0; fi",
                    limit = app_artwork::MAX_FILE
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let command =
            format!("printf '{{\"returnValue\":true,\"icons\":[\"'; ( {tries} ); printf '\"]}}'");
        if let Ok(envelope) = call(
            "luna://org.webosbrew.hbchannel.service/exec",
            json!({"command": command}),
        ) && let Ok(icons) = inventory_payload(&envelope, "icons")
            && let Some(hex) = icons.get(0).and_then(Value::as_str)
            && !hex.is_empty()
            && let Ok(bytes) = decode_hex(hex)
        {
            return Ok(vec![bytes]);
        }
    }
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let command = format!(
        "/usr/bin/node -e {} {}",
        quote(include_str!("../tv/read-app-icon.cjs")),
        quote(&serde_json::to_string(source).map_err(|e| e.to_string())?)
    );
    let envelope = call(
        "luna://org.webosbrew.hbchannel.service/exec",
        json!({"command": command}),
    )?;
    let icons = inventory_payload(&envelope, "icons")?;
    let mut remaining = app_artwork::MAX_FILE;
    icons
        .as_array()
        .ok_or("Invalid installed icon reply")?
        .iter()
        .map(|value| {
            let hex = value.as_str().ok_or("Invalid installed icon data")?;
            if hex.len() % 2 != 0 || hex.len() / 2 > remaining {
                return Err("Installed icon reply exceeds its size limit".into());
            }
            remaining -= hex.len() / 2;
            hex.as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let digit = |b: u8| {
                        (b as char)
                            .to_digit(16)
                            .ok_or("Invalid installed icon encoding")
                    };
                    Ok((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
                })
                .collect()
        })
        .collect()
}

fn output_excerpt(output: &str) -> String {
    output
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control())
        .take(320)
        .collect()
}

fn decode_hex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) || hex.len() / 2 > app_artwork::MAX_FILE {
        return Err("Installed icon reply exceeds its size limit".into());
    }
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b: u8| {
                (b as char)
                    .to_digit(16)
                    .ok_or("Invalid installed icon encoding")
            };
            Ok((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
        })
        .collect()
}

fn inventory_payload(envelope: &Value, field: &str) -> Result<Value, String> {
    let stdout = envelope
        .get("stdoutString")
        .and_then(Value::as_str)
        .unwrap_or("");
    let stderr = envelope
        .get("stderrString")
        .and_then(Value::as_str)
        .unwrap_or("");
    let failure = |reason: &str| {
        format!(
            "{reason}; stdout: {}; stderr: {}",
            output_excerpt(stdout),
            output_excerpt(stderr),
        )
    };
    for key in ["returnCode", "exitCode"] {
        if let Some(code) = envelope.get(key)
            && code != 0
            && code != "0"
        {
            return Err(failure(&format!(
                "Homebrew inventory command failed ({key}={code})"
            )));
        }
    }
    if stdout.trim().is_empty() {
        return Err(failure("Homebrew returned no inventory"));
    }
    if stdout.len() > 2 * 1024 * 1024 {
        return Err("Homebrew inventory exceeds the 2 MiB limit".into());
    }
    // Parse an entire JSON object, including pretty-printed multi-line replies.
    // A stream parser permits CLI timing text after that object; a preamble can
    // precede its opening brace. Never mistake a nested one-line app for a reply.
    let start = stdout
        .find('{')
        .ok_or_else(|| failure("Homebrew inventory contains no JSON object"))?;
    let response = serde_json::Deserializer::from_str(&stdout[start..])
        .into_iter::<Value>()
        .next()
        .ok_or_else(|| failure("Homebrew inventory contains no JSON reply"))?
        .map_err(|error| {
            failure(&format!(
                "Homebrew inventory JSON could not be parsed: {error}"
            ))
        })?;
    if !response.is_object() || !response.get("returnValue").is_some_and(Value::is_boolean) {
        return Err(failure("Homebrew inventory JSON is not a Luna response"));
    }
    let response = checked_response(response)?;
    response
        .get(field)
        .cloned()
        .ok_or_else(|| format!("Missing TV inventory field: {field}"))
}

/// Apps someone brought to the front since the TV started, one id per line.
/// /tmp is memory, so the list starts empty at every boot and outlives a
/// restart of Lounge itself.
const OPENED: &str = "/tmp/lounge-opened";

#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
fn note_opened(id: &str) {
    if id.is_empty() || id == APP_ID || id == HOME_ID || !valid_app_id(id) {
        return;
    }
    let known = std::fs::read_to_string(OPENED).unwrap_or_default();
    if known.lines().any(|line| line == id) {
        return;
    }
    use std::io::Write;
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(OPENED)
        .and_then(|mut file| writeln!(file, "{id}"));
}

/// The open apps the switcher shows: on the TV only those someone opened.
/// LG's preloader starts apps (Netflix, YouTube, inputs) nobody asked for;
/// they run, but they were never open in any sense a viewer would recognise.
pub fn opened_only(mut ids: Vec<String>, webos: bool) -> Vec<String> {
    if webos {
        let known = std::fs::read_to_string(OPENED).unwrap_or_default();
        ids.retain(|id| known.lines().any(|line| line == id));
    }
    ids
}

/// App IDs of open full-screen ("card") apps from a `running` reply.
fn card_ids(running: &Value) -> Vec<String> {
    running
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("defaultWindowType").and_then(Value::as_str) == Some("card"))
        .filter_map(|entry| entry.get("id")?.as_str())
        .filter(|id| valid_app_id(id))
        .map(str::to_owned)
        .collect()
}

fn running_apps() -> Result<Vec<String>, String> {
    // The direct bus call avoids a shell spawn; elevated exec is the fallback
    // where this executable's role may not read the running list.
    let started = std::time::Instant::now();
    match call("luna://com.webos.applicationManager/running", json!({})) {
        Ok(reply) if reply.get("running").is_some() => {
            eprintln!("Lounge running: bus {:?}", started.elapsed());
            return Ok(card_ids(&reply["running"]));
        }
        Ok(reply) => eprintln!("Lounge running: bus reply without list: {reply}"),
        Err(error) => eprintln!("Lounge running: bus refused ({error})"),
    }
    let ids = card_ids(&inventory_read(
        "luna://com.webos.applicationManager/running",
        "running",
    )?);
    eprintln!("Lounge running: elevated {:?}", started.elapsed());
    Ok(ids)
}

/// Settings › General › Home Button. LG's compositor opens LG Home for the
/// Home key whatever the default Home is; a key filter in its chain is the
/// one native way to change that. Lounge's (`tv/home-key-filter.js`) and the
/// helper that puts it in or takes it out (`tv/home-button.js`) are written
/// to /var/lib/lounge and run as root through Homebrew Channel's exec.
fn home_button(set: Option<bool>) -> Result<bool, String> {
    const HELPER: &str = include_str!("../tv/home-button.js");
    const FILTER: &str = include_str!("../tv/home-key-filter.js");
    let mode = match set {
        Some(true) => "on",
        Some(false) => "off",
        None => "status",
    };
    let command = format!(
        "if [ \"$(id -u)\" != 0 ]; then echo '{{\"returnValue\":false,\"errorText\":\"Homebrew Channel needs root access\"}}'; exit 0; fi\n\
         mkdir -p /var/lib/lounge\n\
         cat > /var/lib/lounge/home-button.js <<'LOUNGE_EOF'\n{HELPER}LOUNGE_EOF\n\
         cat > /var/lib/lounge/home-key-filter.next <<'LOUNGE_EOF'\n{FILTER}LOUNGE_EOF\n\
         /usr/bin/node /var/lib/lounge/home-button.js {mode}"
    );
    let envelope = call(
        "luna://org.webosbrew.hbchannel.service/exec",
        json!({"command": command}),
    )?;
    inventory_payload(&envelope, "on")?
        .as_bool()
        .ok_or_else(|| "The TV did not say where Home goes".into())
}

fn elevated_close(id: &str) -> Result<(), String> {
    let command = close_command(id);
    let envelope = call(
        "luna://org.webosbrew.hbchannel.service/exec",
        json!({"command": command}),
    )?;
    inventory_payload(&envelope, "returnValue").map(|_| ())
}

fn fixture_inventory(directory: &std::path::Path) -> Result<(Inventory, Option<String>), String> {
    let read = |file: &str, field: &str| -> Result<Value, String> {
        let bytes = std::fs::read(directory.join(file)).map_err(|e| format!("{file}: {e}"))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{file}: {e}"))?;
        value
            .get(field)
            .cloned()
            .ok_or_else(|| format!("{file}: missing {field}"))
    };
    let apps = normalize_apps(&read("listApps.json", "apps")?)?;
    let inputs = normalize_inputs(&read("inputs.json", "devices")?)?;
    Ok((Inventory { apps, inputs }, None))
}

fn inventory() -> Result<(Inventory, Option<String>), String> {
    let apps = normalize_apps(&inventory_read(
        "luna://com.webos.applicationManager/listApps",
        "apps",
    )?)?;
    let inputs = inventory_read("luna://com.webos.service.eim/getAllInputStatus", "devices")
        .and_then(|raw| normalize_inputs(&raw));
    match inputs {
        Ok(inputs) => {
            eprintln!(
                "Lounge TV inventory: {} apps, {} inputs",
                apps.len(),
                inputs.len()
            );
            Ok((Inventory { apps, inputs }, None))
        }
        Err(error) => Ok((
            Inventory {
                apps,
                inputs: vec![],
            },
            Some(format!("Inputs could not be refreshed: {error}")),
        )),
    }
}

fn title(value: Option<&str>, fallback: &str) -> String {
    let clean: String = value
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    if clean.trim().is_empty() {
        fallback.into()
    } else {
        clean.trim().into()
    }
}

pub fn normalize_apps(raw: &Value) -> Result<Vec<App>, String> {
    let entries = raw
        .as_array()
        .filter(|a| a.len() <= 1000)
        .ok_or("Invalid app inventory")?;
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for entry in entries {
        let Some(id) = entry
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_app_id(id))
        else {
            continue;
        };
        let live = id == "com.webos.app.livetv";
        let visible = entry.get("visible");
        // Earlier launchers this one replaces (Luma and its prototypes) are
        // never listed as apps.
        if id == APP_ID
            || id == "com.ecklf.luma"
            || id == "com.luma.native"
            || id == "com.luma.launcher"
            || id == "com.luma.launcher.native"
            || id == "com.webos.app.home"
            || id.starts_with("com.webos.app.hdmi")
            || id.starts_with("com.webos.app.externalinput.")
            || (!live && (visible == Some(&json!(false)) || visible == Some(&json!("false"))))
            || (!live
                && ["com.webos.", "com.palm.", "com.lge."]
                    .iter()
                    .any(|prefix| id.starts_with(prefix))
                && visible != Some(&json!(true)))
            || !seen.insert(id)
        {
            continue;
        }
        let mut app = App::new(id, &title(entry.get("title").and_then(Value::as_str), id));
        app.icon = Some(app_artwork::Source::new(id, entry));
        app.removable = crate::uninstall::removable(entry);
        apps.push(app);
    }
    apps.sort_by_key(|a| a.title.to_lowercase());
    Ok(apps)
}

pub fn normalize_inputs(raw: &Value) -> Result<Vec<Input>, String> {
    let entries = raw
        .as_array()
        .filter(|a| a.len() <= 128)
        .ok_or("Invalid input inventory")?;
    // Some firmware reports `connected: true` for every port, so there it
    // says nothing and `hdmiPlugIn` (the cable sense) decides. Elsewhere
    // `connected` is meaningful and `hdmiPlugIn` can stay false for a plugged
    // device in standby (a 65UQ801C0SB, webOS 7.6: HDMI 2 connected, every
    // hdmiPlugIn false), so either one marks the port as in use.
    let hdmi = entries.iter().filter(|e| {
        e.get("appId")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("com.webos.app.hdmi"))
    });
    let connected_everywhere = hdmi.clone().count() > 1
        && hdmi
            .clone()
            .all(|e| e.get("connected").and_then(Value::as_bool) == Some(true));
    let mut inputs = Vec::new();
    for entry in entries {
        let Some(id) = entry.get("appId").and_then(Value::as_str) else {
            continue;
        };
        let Some(number) = id
            .strip_prefix("com.webos.app.hdmi")
            .and_then(|s| s.parse::<u8>().ok())
            .filter(|n| *n > 0 && *n < 100)
        else {
            continue;
        };
        if id != format!("com.webos.app.hdmi{number}")
            || entries
                .iter()
                .filter(|e| e.get("appId").and_then(Value::as_str) == Some(id))
                .count()
                != 1
        {
            continue;
        }
        if entry
            .get("id")
            .is_some_and(|v| v != &format!("HDMI_{number}"))
        {
            continue;
        }
        if entry
            .get("port")
            .is_some_and(|v| v != &json!(number) && v != &number.to_string())
        {
            continue;
        }
        let port = format!("HDMI {number}");
        inputs.push(Input {
            id: id.into(),
            title: title(entry.get("label").and_then(Value::as_str), &port),
            port,
            connected: {
                let plugged = entry.get("hdmiPlugIn").and_then(Value::as_bool);
                let connected = entry
                    .get("connected")
                    .and_then(Value::as_bool)
                    .filter(|_| !connected_everywhere);
                match (plugged, connected) {
                    (None, None) => None,
                    (plugged, connected) => Some(plugged == Some(true) || connected == Some(true)),
                }
            },
        });
    }
    Ok(inputs)
}

/// Quitting from the switcher. `closeByAppId` only sends a web app to the
/// background (its page keeps running); WebAppMgr's `killApp` ends it. So
/// `killApp` goes first, and anything it does not end (a native app, which
/// WebAppMgr does not know, or firmware without the method) is closed by
/// the application manager as before. `id` passed valid_app_id: only ASCII
/// alphanumerics and . _ - reach the shell.
pub fn close_command(id: &str) -> String {
    format!(
        "K=$(/usr/bin/luna-send -n 1 -w 3000 luna://com.palm.webappmanager/killApp '{{\"appId\":\"{id}\"}}' 2>/dev/null)\n\
         case \"$K\" in *'\"returnValue\":true'*|*'\"returnValue\": true'*) printf '%s\\n' \"$K\"; exit 0;; esac\n\
         /usr/bin/luna-send -n 1 -w 4000 luna://com.webos.applicationManager/closeByAppId '{{\"id\":\"{id}\"}}'"
    )
}

/// Removes `id` through the elevated helper, waiting up to its own 15 s for
/// webOS to finish. A reply lost on the way is checked against the TV's
/// complete app list (never by asking for the removal a second time).
fn elevated_uninstall(id: &str) -> Result<(), String> {
    let command = crate::uninstall::command(id)?;
    let exec = "luna://org.webosbrew.hbchannel.service/exec";
    #[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
    let reply = crate::luna::call_within(
        exec,
        &json!({"command": command}),
        std::time::Duration::from_secs(20),
    );
    #[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
    let reply: Result<Value, String> = {
        let _ = (exec, command);
        Err("Native Luna services are only available in an ARM webOS build".into())
    };
    let Err(error) = reply
        .and_then(checked_response)
        .and_then(|envelope| inventory_payload(&envelope, "id"))
    else {
        return Ok(());
    };
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(std::time::Duration::from_millis(500));
        }
        match inventory_read("luna://com.webos.applicationManager/listApps", "apps")
            .and_then(|apps| crate::uninstall::confirms_removal(&apps, id))
        {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(read) => {
                eprintln!("Lounge uninstall check: {read}");
                break;
            }
        }
    }
    Err(error)
}

/// One reply from a TV service through Lounge's own bus connection (the
/// role the hub gave this executable), for modules outside this worker.
pub fn bus(uri: &str, payload: Value) -> Result<Value, String> {
    call(uri, payload)
}

/// One fixed command run as root through Homebrew Channel's exec, and the
/// `field` of the Luna-style JSON reply it prints.
pub fn elevated(command: &str, field: &str) -> Result<Value, String> {
    let envelope = call(
        "luna://org.webosbrew.hbchannel.service/exec",
        json!({"command": command}),
    )?;
    inventory_payload(&envelope, field)
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn a_long_run_keeps_its_start_and_latest_lines() {
        let mut log = String::from("1.000 previous run's last line\n");
        log += &format!("2.000 {STARTED}7)\n");
        for press in 0..500 {
            log += &format!("3.000 home key: back to the top {press}\n");
        }
        let short = shorten(&log).expect("long enough to shorten");
        let lines: Vec<&str> = short.lines().collect();
        assert_eq!(lines.len(), 1 + RUN_HEAD + 1 + RUN_TAIL);
        assert_eq!(lines[0], "1.000 previous run's last line");
        assert!(lines[1].contains(STARTED), "the run's start stays");
        assert_eq!(lines[RUN_HEAD + 1], "... 351 lines left out");
        assert_eq!(
            *lines.last().unwrap(),
            "3.000 home key: back to the top 499"
        );
        // Shortened, it stays put until it grows again.
        assert_eq!(shorten(&short), None);
    }
}
