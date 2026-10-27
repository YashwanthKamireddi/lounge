//! Which apps the context menu may offer to uninstall, and the elevated
//! command that removes one. The rules are by exact identity and install
//! location, never by title: a renamed app must not slip past them.
use crate::{
    model::valid_app_id,
    platform::{APP_ID, HOME_ID},
};
use serde_json::Value;

/// Apps never removed from Lounge, whatever webOS says about them: Lounge
/// itself (and the launchers it replaced), LG's Home, and Homebrew Channel
/// (removing it would take Lounge's elevated access with it).
const PROTECTED: [&str; 7] = [
    APP_ID,
    HOME_ID,
    "com.ecklf.luma",
    "com.luma.native",
    "com.luma.launcher",
    "com.luma.launcher.native",
    "org.webosbrew.hbchannel",
];

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

/// The shell command Homebrew Channel's exec runs to remove `id`: the
/// embedded helper (`tv/uninstall-app.cjs`), which checks the same rule again.
pub fn command(id: &str) -> Result<String, String> {
    if !allowed(id) {
        return Err("This app is protected from removal".into());
    }
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    Ok(format!(
        "/usr/bin/node -e {} {}",
        quote(include_str!("../tv/uninstall-app.cjs")),
        quote(id)
    ))
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
