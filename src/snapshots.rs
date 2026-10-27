//! The switcher's cards show each open app as it was left, as tvOS does.
//! webOS keeps that picture itself: LG's recents service (served by the
//! surface manager) names a JPEG per app it took when the app was last left
//! (`/tmp/snapshots/<appId>/…`). Reading, decoding and scaling happen on
//! this module's worker; the drawing thread only uploads the result. An app
//! with no snapshot (none taken since the TV started) keeps its card.
use crate::{app_artwork::Artwork, model::valid_app_id, state::State};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Instant,
};

/// The switcher card's size (and the snapshot's, once scaled): one texel
/// per logical pixel at the focused size.
pub const CARD: (u32, u32) = (720, 412);
/// The card's corner radius as a share of its height.
pub const CORNER: f32 = 36. / 412.;
/// Seconds a snapshot takes to dissolve in over the card it replaces.
pub const FADE: f32 = 0.3;
/// The largest snapshot file read (webOS writes card-sized JPEGs; a full
/// frame at high quality stays well below this).
const MAX_FILE: usize = 900 * 1024;
const RECENTS: &str = "luna://com.webos.service.recent/listRecents";

/// A snapshot ready to draw: its picture, the file's stamp (modified time
/// and size, so an unchanged file is never read twice), and when it arrived.
pub struct Shot {
    pub picture: Arc<Artwork>,
    pub stamp: String,
    pub since: Instant,
}

/// The snapshots the switcher holds, by app. Only the open apps' are kept
/// (memory: one card-sized picture each).
#[derive(Default)]
pub struct Shots(HashMap<String, Shot>);

impl Shots {
    pub fn get(&self, id: &str) -> Option<&Shot> {
        self.0.get(id)
    }

    pub fn remove(&mut self, id: &str) {
        self.0.remove(id);
    }

    /// A snapshot is still dissolving in.
    pub fn fading(&self) -> bool {
        self.0
            .values()
            .any(|shot| crate::clock::since(shot.since).as_secs_f32() < FADE)
    }

    /// How far `id`'s snapshot has dissolved in (0 none, 1 whole).
    pub fn shown(&self, id: &str, reduced_motion: bool) -> f32 {
        self.0.get(id).map_or(0., |shot| {
            if reduced_motion {
                return 1.;
            }
            let t = (crate::clock::since(shot.since).as_secs_f32() / FADE).min(1.);
            t * t * (3. - 2. * t)
        })
    }

    /// The stamps known for `apps`, for the worker to skip unchanged files.
    fn stamps(&self, apps: &[String]) -> Vec<(String, Option<String>)> {
        apps.iter()
            .map(|id| (id.clone(), self.0.get(id).map(|shot| shot.stamp.clone())))
            .collect()
    }

    /// Keeps only `apps`' snapshots.
    fn keep(&mut self, apps: &[String]) {
        self.0.retain(|id, _| apps.contains(id));
    }

    #[cfg(test)]
    pub fn insert_for_test(&mut self, id: &str, picture: Arc<Artwork>) {
        self.insert(id.into(), picture, String::new());
    }

    fn insert(&mut self, id: String, picture: Arc<Artwork>, stamp: String) {
        self.0.insert(
            id,
            Shot {
                picture,
                stamp,
                since: crate::clock::now(),
            },
        );
    }
}

/// What the worker finds for one app.
enum Found {
    /// A new or changed picture, and its stamp.
    Picture(String, Arc<Artwork>),
    /// webOS has no snapshot of it (or it could not be read).
    Gone,
    /// The known picture is still current.
    Same,
}

/// The worker that reads snapshots, and the drawing thread's side of it.
pub struct Service {
    requests: mpsc::Sender<Vec<(String, Option<String>)>>,
    found: mpsc::Receiver<(String, Found)>,
}

impl Service {
    /// `webos`: read LG's recents; otherwise the desktop preview shows
    /// sample pictures from `assets` (labelled demo data, never on a TV).
    pub fn new(webos: bool, assets: PathBuf) -> Self {
        let (requests, queue) = mpsc::channel::<Vec<(String, Option<String>)>>();
        let (sender, found) = mpsc::channel();
        std::thread::spawn(move || {
            crate::background_priority();
            // Asked once: a TV whose compositor never captures (a 65UQ801C0SB,
            // webOS 7.6) is not asked for recents on every open.
            let mut captures = None;
            while let Ok(mut wanted) = queue.recv() {
                // Opened again meanwhile: only the latest list matters.
                while let Ok(newer) = queue.try_recv() {
                    wanted = newer;
                }
                let captures = *captures.get_or_insert_with(|| !webos || compositor_captures());
                let listed = if !captures {
                    Vec::new()
                } else if webos {
                    recents().unwrap_or_else(|error| {
                        eprintln!("Lounge snapshots: {error}");
                        Vec::new()
                    })
                } else {
                    demo(&assets)
                };
                for (id, known) in wanted {
                    let found = match listed.iter().find(|(app, _)| *app == id) {
                        Some((_, path)) => read(webos, &id, path, known.as_deref()),
                        None => Found::Gone,
                    };
                    if sender.send((id, found)).is_err() {
                        return;
                    }
                }
            }
        });
        Self { requests, found }
    }

    /// The loop's share: asks for the snapshots the switcher wants and hands
    /// those found to the state. Behind an app nothing is asked for.
    pub fn pump(&self, state: &mut State) {
        if let Some(apps) = state.snapshots_wanted.take() {
            state.snapshots.keep(&apps);
            let _ = self.requests.send(state.snapshots.stamps(&apps));
        }
        while let Ok((id, found)) = self.found.try_recv() {
            match found {
                Found::Picture(stamp, picture) => {
                    state.snapshots.insert(id, picture, stamp);
                    state.dirty = true;
                }
                Found::Gone => {
                    state.snapshots.remove(&id);
                    state.dirty = true;
                }
                Found::Same => {}
            }
        }
    }
}

/// Whether the compositor takes app snapshots: LG turns it off on some
/// models (configd `com.webos.surfacemanager.disableCapture`, readable by
/// Lounge's own role). Unknown counts as yes.
fn compositor_captures() -> bool {
    let captures = crate::platform::bus(
        "luna://com.webos.service.config/getConfigs",
        serde_json::json!({"configNames": [DISABLE_CAPTURE]}),
    )
    .map_or(true, |reply| !capture_disabled(&reply));
    crate::platform::status(&format!(
        "switcher snapshots: {}",
        if captures {
            "read from LG's recents"
        } else {
            "this TV takes none (disableCapture)"
        }
    ));
    captures
}

const DISABLE_CAPTURE: &str = "com.webos.surfacemanager.disableCapture";

/// A `getConfigs` reply says the compositor's capture is off.
pub fn capture_disabled(reply: &Value) -> bool {
    reply
        .get("configs")
        .and_then(|configs| configs.get(DISABLE_CAPTURE))
        .and_then(Value::as_bool)
        == Some(true)
}

/// LG's recent apps and their snapshot files, newest first.
fn recents() -> Result<Vec<(String, String)>, String> {
    // Lounge's own bus role first; where the hub refuses it, the same read
    // through Homebrew Channel's elevated exec.
    let reply = crate::platform::bus(RECENTS, serde_json::json!({})).or_else(|error| {
        eprintln!("Lounge snapshots: bus refused ({error}); asking as root");
        crate::platform::elevated(
            &format!(
                "if [ \"$(id -u)\" = 0 ]; then /usr/bin/luna-send -n 1 -w 3000 {RECENTS} '{{}}'; else echo '{{\"returnValue\":false,\"errorText\":\"Homebrew Channel needs root access\"}}'; fi"
            ),
            "recentsAppList",
        )
        .map(|list| serde_json::json!({ "recentsAppList": list }))
    })?;
    Ok(parse(&reply))
}

/// The apps in a `listRecents` reply with a snapshot file Lounge will read:
/// only a plain JPEG or PNG path inside that app's own snapshot folder.
pub fn parse(reply: &Value) -> Vec<(String, String)> {
    reply
        .get("recentsAppList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(|entry| {
            let id = entry.get("appId")?.as_str().filter(|id| valid_app_id(id))?;
            let path = entry.get("cardSnapShotFilePath")?.as_str()?;
            safe_path(id, path).then(|| (id.to_owned(), path.to_owned()))
        })
        .collect()
}

/// Whether `path` is a picture in `/tmp/snapshots/<id>/`, spelled only with
/// characters that are safe to hand to the elevated shell.
fn safe_path(id: &str, path: &str) -> bool {
    let Some(name) = path.strip_prefix(&format!("/tmp/snapshots/{id}/")) else {
        return false;
    };
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    !name.is_empty()
        && name.len() <= 200
        && !name.contains('/')
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && matches!(extension.as_deref(), Some("jpg" | "jpeg" | "png"))
}

/// Reads and decodes `path` for `id`, unless its stamp matches `known`.
/// Lounge reads it directly where its sandbox allows; otherwise as root
/// through Homebrew Channel (one bounded hex dump of that one file).
fn read(webos: bool, id: &str, path: &str, known: Option<&str>) -> Found {
    let direct = std::fs::metadata(path).ok().filter(|meta| meta.is_file());
    let (stamp, bytes) = match direct {
        Some(meta) => {
            let stamp = stamp_of(&meta);
            if known == Some(stamp.as_str()) {
                return Found::Same;
            }
            match std::fs::read(path) {
                Ok(bytes) if bytes.len() <= MAX_FILE => (stamp, bytes),
                _ if webos => match elevated_read(path, known) {
                    Ok(read) => read,
                    Err(error) => {
                        eprintln!("Lounge snapshot {id}: {error}");
                        return Found::Gone;
                    }
                },
                _ => return Found::Gone,
            }
        }
        None if webos => match elevated_read(path, known) {
            Ok(read) => read,
            Err(error) => {
                eprintln!("Lounge snapshot {id}: {error}");
                return Found::Gone;
            }
        },
        None => return Found::Gone,
    };
    if bytes.is_empty() {
        return Found::Same;
    }
    match card(&bytes) {
        Some(pixels) => {
            let source = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                (path, &stamp).hash(&mut hasher);
                hasher.finish()
            };
            Found::Picture(
                stamp,
                Artwork::picture(format!("snapshot:{id}"), source, pixels, CORNER),
            )
        }
        None => Found::Gone,
    }
}

/// A file's modified time and size: changed whenever webOS writes a new one.
fn stamp_of(meta: &std::fs::Metadata) -> String {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_secs());
    format!("{modified}-{}", meta.len())
}

/// The file's stamp and bytes, read as root; no bytes when the stamp is
/// still `known`. `path` passed `safe_path`.
fn elevated_read(path: &str, known: Option<&str>) -> Result<(String, Vec<u8>), String> {
    let known = known.filter(|stamp| stamp.bytes().all(|b| b.is_ascii_digit() || b == b'-'));
    let command = format!(
        "[ -f {path} ] || {{ echo '{{\"returnValue\":false,\"errorText\":\"No snapshot\"}}'; exit 0; }}\n\
         S=$(date -r {path} +%s)-$(wc -c < {path})\n\
         if [ \"$S\" = '{known}' ]; then printf '{{\"returnValue\":true,\"snapshot\":{{\"stamp\":\"%s\",\"hex\":\"\"}}}}' \"$S\"; exit 0; fi\n\
         [ $(wc -c < {path}) -le {MAX_FILE} ] || {{ echo '{{\"returnValue\":false,\"errorText\":\"Snapshot too large\"}}'; exit 0; }}\n\
         printf '{{\"returnValue\":true,\"snapshot\":{{\"stamp\":\"%s\",\"hex\":\"' \"$S\"; od -An -v -tx1 {path} | tr -d ' \\n'; printf '\"}}}}'",
        known = known.unwrap_or("none"),
    );
    let snapshot = crate::platform::elevated(&command, "snapshot")?;
    let stamp = snapshot
        .get("stamp")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let hex = snapshot
        .get("hex")
        .and_then(Value::as_str)
        .ok_or("Invalid snapshot reply")?;
    Ok((stamp, decode_hex(hex)?))
}

fn decode_hex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) || hex.len() / 2 > MAX_FILE {
        return Err("Snapshot reply exceeds its size limit".into());
    }
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b: u8| (b as char).to_digit(16).ok_or("Invalid snapshot encoding");
            Ok((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
        })
        .collect()
}

/// The snapshot as the card shows it: decoded within bounds, cropped from
/// its centre to the card's shape and scaled to the card's size.
pub fn card(bytes: &[u8]) -> Option<image::RgbaImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(3840);
    limits.max_image_height = Some(2160);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    if width < 16 || height < 16 {
        return None;
    }
    let (cw, ch) = CARD;
    // The largest centred region with the card's shape.
    let (w, h) = if u64::from(width) * u64::from(ch) > u64::from(height) * u64::from(cw) {
        (
            (u64::from(height) * u64::from(cw) / u64::from(ch)) as u32,
            height,
        )
    } else {
        (
            width,
            (u64::from(width) * u64::from(ch) / u64::from(cw)) as u32,
        )
    };
    let cropped = image::imageops::crop_imm(&image, (width - w) / 2, (height - h) / 2, w, h);
    Some(image::imageops::resize(
        &*cropped,
        cw,
        ch,
        image::imageops::FilterType::Triangle,
    ))
}

/// The desktop preview's stand-ins: wallpaper photos as the open sample
/// apps' snapshots (a TV always shows its real ones, or none).
fn demo(assets: &Path) -> Vec<(String, String)> {
    [
        ("netflix", "summit"),
        ("youtube.leanback.v4", "yosemite"),
        ("amazon", "northern-lights"),
    ]
    .into_iter()
    .map(|(id, picture)| {
        (
            id.to_owned(),
            assets
                .join(format!("wallpapers/{picture}.jpg"))
                .to_string_lossy()
                .into_owned(),
        )
    })
    .collect()
}
