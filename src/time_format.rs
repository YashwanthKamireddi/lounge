//! Lounge's clock follows the TV's own 12/24-hour format, the system
//! service's `timeFormat` preference ("HH12" or "HH24"): one source of truth
//! for every clock on the TV (HIG: respect system-wide settings, never keep a
//! copy). webOS 7's own Settings has no control for it, so Settings ›
//! General › Clock format in Lounge sets the TV's value rather than one of
//! its own. Both calls are in the bus's public group (Lounge's role).
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// 0 until the TV has said, then 1 (12-hour) or 2 (24-hour).
static FORMAT: AtomicU8 = AtomicU8::new(0);
/// A change the TV refused, not yet reported.
static REFUSED: AtomicBool = AtomicBool::new(false);

#[cfg_attr(
    not(all(feature = "webos", target_os = "linux", target_arch = "arm")),
    allow(dead_code)
)]
const GET: &str = "luna://com.webos.service.systemservice/getPreferences";
#[cfg_attr(
    not(all(feature = "webos", target_os = "linux", target_arch = "arm")),
    allow(dead_code)
)]
const SET: &str = "luna://com.webos.service.systemservice/setPreferences";

/// Whether the TV shows a 24-hour clock; None until it has said (the
/// desktop preview never asks: there it is whatever was last chosen).
pub fn hour_24() -> Option<bool> {
    match FORMAT.load(Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// A change was refused since last asked (it is then undone: `hour_24`
/// says what the TV still has).
pub fn refused() -> bool {
    REFUSED.swap(false, Ordering::Relaxed)
}

/// A `getPreferences` reply's clock format, if it names one.
#[cfg_attr(
    not(all(feature = "webos", target_os = "linux", target_arch = "arm")),
    allow(dead_code)
)]
pub fn parse(reply: &Value) -> Option<bool> {
    match reply.get("timeFormat").and_then(Value::as_str)? {
        "HH24" => Some(true),
        "HH12" => Some(false),
        _ => None,
    }
}

/// The `setPreferences` payload for a format.
#[cfg_attr(
    not(all(feature = "webos", target_os = "linux", target_arch = "arm")),
    allow(dead_code)
)]
pub fn payload(hour_24: bool) -> Value {
    json!({"timeFormat": if hour_24 { "HH24" } else { "HH12" }})
}

fn store(hour_24: bool) -> u8 {
    FORMAT.swap(if hour_24 { 2 } else { 1 }, Ordering::Relaxed)
}

/// Sets the TV's format. The clock changes at once; if the TV refuses, it
/// goes back to what the TV has and `refused` says so.
pub fn set(hour_24: bool) {
    let previous = store(hour_24);
    #[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
    std::thread::spawn(move || {
        let result = crate::platform::bus(SET, payload(hour_24)).or_else(|error| {
            eprintln!("Lounge clock format: bus refused ({error}); asking as root");
            crate::platform::elevated(
                &format!(
                    "/usr/bin/luna-send -n 1 -w 3000 {SET} '{}'",
                    payload(hour_24)
                ),
                "returnValue",
            )
        });
        if let Err(error) = result {
            crate::platform::status(&format!("clock format not changed: {error}"));
            FORMAT.store(previous, Ordering::Relaxed);
            REFUSED.store(true, Ordering::Relaxed);
        }
    });
    #[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
    let _ = previous;
}

/// Follows the setting for as long as Lounge runs: pushed by the system
/// service (a change from anywhere shows at once), or, where the hub
/// refuses the stream, read as root now and every few minutes.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
pub fn watch() {
    let request = json!({"keys": ["timeFormat"], "subscribe": true});
    let subscribed = crate::luna::subscribe(GET, &request, |reply| {
        if reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
            crate::platform::status(&format!("clock format events ended: {reply}"));
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(30));
                watch();
            });
            return;
        }
        if let Some(hour_24) = parse(&reply) {
            store(hour_24);
        }
    });
    match subscribed {
        Ok(()) => crate::platform::status("clock format events: on"),
        Err(error) => {
            crate::platform::status(&format!("clock format events: {error}; reading as root"));
            std::thread::spawn(|| {
                crate::background_priority();
                loop {
                    match crate::platform::elevated(
                        &format!(
                            "/usr/bin/luna-send -n 1 -w 3000 {GET} '{{\"keys\":[\"timeFormat\"]}}'"
                        ),
                        "timeFormat",
                    ) {
                        Ok(format) => {
                            if let Some(hour_24) = parse(&json!({ "timeFormat": format })) {
                                store(hour_24);
                            }
                        }
                        Err(error) => eprintln!("Lounge clock format: {error}"),
                    }
                    std::thread::sleep(std::time::Duration::from_secs(300));
                }
            });
        }
    }
}

#[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
pub fn watch() {}

/// 0 until the TV has said, then 1 (its clock is set) or 2 (not yet: no network since power-on,
/// so it reads a default date such as 1 January).
static TIME: AtomicU8 = AtomicU8::new(0);

/// Whether the TV's clock is set. Unknown counts as set: only the TV's own word hides the time.
pub fn time_valid() -> bool {
    TIME.load(Ordering::Relaxed) != 2
}

/// A `getSystemTime` reply's `timeValid`, if it has one.
#[cfg_attr(
    not(all(feature = "webos", target_os = "linux", target_arch = "arm")),
    allow(dead_code)
)]
pub fn parse_valid(reply: &Value) -> Option<bool> {
    reply.get("timeValid").and_then(Value::as_bool)
}

/// Follows whether the TV's clock is set, pushed by the system service as it syncs.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
pub fn watch_time() {
    let request = json!({"subscribe": true});
    let subscribed = crate::luna::subscribe(
        "luna://com.palm.systemservice/time/getSystemTime",
        &request,
        |reply| {
            if reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
                crate::platform::status(&format!("time events ended: {reply}"));
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    watch_time();
                });
                return;
            }
            if let Some(valid) = parse_valid(&reply) {
                TIME.store(if valid { 1 } else { 2 }, Ordering::Relaxed);
            }
        },
    );
    match subscribed {
        Ok(()) => crate::platform::status("time events: on"),
        Err(error) => crate::platform::status(&format!("time events: {error}")),
    }
}

#[cfg(not(all(feature = "webos", target_os = "linux", target_arch = "arm")))]
pub fn watch_time() {}
