//! Which apps the context menu may offer to uninstall, and the elevated
//! command that removes one. The rules are by exact identity and install
//! location, never by title: a renamed app must not slip past them.
use crate::{
    model::valid_app_id,
    platform::{APP_ID, FULL_HOME_ID, HOME_ID},
};
use serde_json::Value;

/// Apps never removed from Lounge, whatever webOS says about them: Lounge
/// itself, LG's Home, and Homebrew Channel (removing it would take Lounge's
/// elevated access with it).
const PROTECTED: [&str; 4] = [APP_ID, HOME_ID, FULL_HOME_ID, "org.webosbrew.hbchannel"];

/// Whether Lounge would ever remove `id`: not protected, not LG's (every
/// `com.webos.`, `com.palm.` and `com.lge.` app, removable or not, as LG's
/// own menus decide those), and not one of Lounge's preview cards.
pub fn allowed(id: &str) -> bool {
    valid_app_id(id)
        && !crate::scroll_preview::is_placeholder(id)
        && !id.starts_with("lounge.")
        && !id.starts_with("org.webosbrew.hbchannel")
        && !PROTECTED.contains(&id)
        && !["com.webos.", "com.palm.", "com.lge."]
            .iter()
            .any(|prefix| id.starts_with(prefix))
}

/// Whether a `listApps` entry is an app the owner installed and may remove.
/// webOS's own `removable` flag decides when it gives one (false is final);
/// without it, only apps installed to the writable partitions (the Content
/// Store's and developer apps) are, never one in the read-only system image.
pub fn removable(entry: &Value) -> bool {
    let Some(id) = entry.get("id").and_then(Value::as_str) else {
        return false;
    };
    let flag = |key: &str| match entry.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::String(value)) => value.parse().ok(),
        _ => None,
    };
    let installed = entry
        .get("folderPath")
        .and_then(Value::as_str)
        .is_some_and(|path| {
            ["/media/cryptofs/apps/", "/media/developer/apps/"]
                .iter()
                .any(|root| path.starts_with(root))
                && !path.contains("/../")
        });
    allowed(id) && flag("systemApp") != Some(true) && flag("removable").unwrap_or(installed)
}

/// The shell command Homebrew Channel's exec runs to remove `id`: Lounge's own
/// executable (`lounge --uninstall-app ID`, see `remove`), which checks the
/// same rule again.
pub fn command(id: &str) -> Result<String, String> {
    if !allowed(id) {
        return Err("This app is protected from removal".into());
    }
    Ok(crate::platform::helper_command("--uninstall-app", id))
}

/// Output beyond this (stdout and stderr together) ends a removal as failed.
#[cfg_attr(not(feature = "webos"), allow(dead_code))]
const OUTPUT_LIMIT: usize = 64 * 1024;

/// Watches luna-send's stream of replies to one removal. It prints a series
/// of objects, optionally pretty printed: braces are counted outside strings
/// rather than taking the first acceptance for completion. Success is the
/// completion status (31).
#[cfg_attr(not(feature = "webos"), allow(dead_code))]
pub struct Removal {
    id: String,
    buffer: Vec<u8>,
    total: usize,
    replies: Vec<Value>,
}

#[cfg_attr(not(feature = "webos"), allow(dead_code))]
impl Removal {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            buffer: vec![],
            total: 0,
            replies: vec![],
        }
    }

    /// Takes more of luna-send's output. Some(outcome) once the removal is
    /// decided.
    pub fn output(&mut self, bytes: &[u8], stdout: bool) -> Option<Result<(), String>> {
        self.total += bytes.len();
        if self.total > OUTPUT_LIMIT {
            return Some(Err("App removal output limit exceeded".into()));
        }
        if !stdout {
            return None;
        }
        self.buffer.extend_from_slice(bytes);
        loop {
            let start = self.buffer.iter().position(|&b| b == b'{')?;
            let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
            let mut end = None;
            for (i, &c) in self.buffer.iter().enumerate().skip(start) {
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        quoted = false;
                    }
                } else if c == b'"' {
                    quoted = true;
                } else if c == b'{' {
                    depth += 1;
                } else if c == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
            }
            let end = end?;
            let reply: Value = match serde_json::from_slice(&self.buffer[start..end]) {
                Ok(reply) => reply,
                Err(error) => return Some(Err(format!("Invalid app removal reply: {error}"))),
            };
            self.buffer.drain(..end);
            if self.replies.len() >= 64 {
                return Some(Err("Too many app removal replies".into()));
            }
            self.replies.push(reply.clone());
            let details = &reply["details"];
            if reply["returnValue"] == false
                || reply["statusValue"] == 25
                || details.get("errorCode").is_some()
            {
                let text = |value: &Value| value.as_str().map(str::to_owned);
                return Some(Err(text(&reply["errorText"])
                    .or_else(|| text(&details["reason"]))
                    .unwrap_or_else(|| "App removal failed".into())));
            }
            if reply["statusValue"] == 31 {
                if reply["id"] != self.id.as_str()
                    || details["packageId"]
                        .as_str()
                        .is_some_and(|package| package != self.id)
                {
                    return Some(Err("App removal identity mismatch".into()));
                }
                return Some(Ok(()));
            }
        }
    }

    /// The reply the context menu reads: returnValue, errorText (on failure),
    /// id and every reply seen.
    pub fn reply(&self, outcome: &Result<(), String>) -> Value {
        let mut reply = serde_json::json!({"returnValue": outcome.is_ok()});
        if let Err(error) = outcome {
            reply["errorText"] = error.as_str().into();
        }
        reply["id"] = self.id.as_str().into();
        reply["replies"] = self.replies.clone().into();
        reply
    }
}

/// `lounge --uninstall-app ID`, run as root through Homebrew Channel's exec.
/// One normal appInstallService/remove subscription, never dev/remove
/// (Homebrew's own uninstall), which refuses apps from LG's Content Store;
/// 15 s at most. `allowed` is checked again, so a bad caller can never remove
/// the launcher, Homebrew Channel or anything of LG's.
#[cfg(all(feature = "webos", unix))]
pub fn remove(id: &str) -> Value {
    use std::{
        io::Read,
        process::{Command, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    let removal = Removal::new(id);
    if !allowed(id) {
        return removal.reply(&Err("Protected or invalid application identifier".into()));
    }
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        return removal.reply(&Err("Homebrew Channel needs root access".into()));
    }
    let child = Command::new("/usr/bin/luna-send")
        .args([
            "-i",
            "-w",
            "15000",
            "luna://com.webos.appInstallService/remove",
        ])
        .arg(serde_json::json!({"id": id, "subscribe": true}).to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return removal.reply(&Err(error.to_string())),
    };
    // Both pipes feed one channel; None is a pipe that closed.
    type Chunk = (bool, Option<Vec<u8>>);
    fn forward(pipe: Option<impl Read + Send + 'static>, stdout: bool, to: mpsc::Sender<Chunk>) {
        thread::spawn(move || {
            if let Some(mut pipe) = pipe {
                let mut buffer = [0; 4096];
                while let Ok(count @ 1..) = pipe.read(&mut buffer) {
                    if to.send((stdout, Some(buffer[..count].to_vec()))).is_err() {
                        return;
                    }
                }
            }
            let _ = to.send((stdout, None));
        });
    }
    let (sender, receiver) = mpsc::channel();
    forward(child.stdout.take(), true, sender.clone());
    forward(child.stderr.take(), false, sender);
    let mut removal = removal;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut open = 2;
    let outcome = loop {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((stdout, Some(bytes))) => {
                if let Some(outcome) = removal.output(&bytes, stdout) {
                    break outcome;
                }
            }
            Ok((_, None)) => {
                open -= 1;
                if open == 0 {
                    break Err("App removal ended without completion".into());
                }
            }
            Err(_) => break Err("App removal completion timed out".into()),
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    removal.reply(&outcome)
}

/// After a lost or late reply: whether a complete, unfiltered `listApps`
/// answer shows `id` gone. An app missing only from Lounge's own (filtered)
/// list proves nothing, and an incomplete list is an error, not a yes.
pub fn confirms_removal(apps: &Value, id: &str) -> Result<bool, String> {
    let apps = apps
        .as_array()
        .filter(|apps| apps.len() <= 1000)
        .ok_or("Invalid app list while confirming the removal")?;
    let mut present = false;
    for app in apps {
        let listed = app
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_app_id(id))
            .ok_or("Incomplete app list while confirming the removal")?;
        present |= listed == id;
    }
    Ok(!present)
}
