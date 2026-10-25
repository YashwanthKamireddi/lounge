//! Installed-app artwork is discovered and decoded on its own worker. The UI
//! requests visible cards only; it uploads finished pixels on the SDL thread.
use crate::model::valid_app_id;
use image::{ImageReader, Limits, RgbaImage};
use serde::Serialize;
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

const ROOTS: [&str; 3] = [
    "/media/developer/apps/usr/palm/applications",
    "/media/cryptofs/apps/usr/palm/applications",
    "/usr/palm/applications",
];
pub const MAX_FILE: usize = 512 * 1024;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const CACHE_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Source {
    pub id: String,
    directory: Option<PathBuf>,
    large_icon: Option<String>,
    icon: Option<String>,
    /// The installed version: an update changes the cache key, so the icon
    /// (in memory and on disk) is read again rather than kept stale.
    version: Option<String>,
}

impl Source {
    pub fn new(id: &str, metadata: &Value) -> Self {
        let text = |key| {
            metadata
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
        };
        Self {
            id: id.into(),
            directory: text("folderPath").map(PathBuf::from).filter(|p| {
                p.components().all(|c| !matches!(c, Component::ParentDir))
                    && ROOTS.iter().any(|root| p.parent() == Some(Path::new(root)))
            }),
            large_icon: text("largeIcon").map(str::to_owned),
            icon: text("icon").map(str::to_owned),
            version: text("version").map(str::to_owned),
        }
    }

    /// Icon files webOS's app list names for this app (large first), each
    /// resolved and checked to lie inside an application folder.
    pub fn listed_icon_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for directory in self.directories() {
            for value in [self.large_icon.as_deref(), self.icon.as_deref()].into_iter().flatten() {
                if let Some(path) = icon_path(&directory, value)
                    && !paths.contains(&path)
                {
                    paths.push(path);
                }
            }
        }
        paths
    }

    fn directories(&self) -> Vec<PathBuf> {
        let mut directories: Vec<_> = self.directory.iter().cloned().collect();
        for root in ROOTS {
            let directory = Path::new(root).join(&self.id);
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
        directories
    }
}

pub struct Artwork {
    pub key: String,
    pub pixels: RgbaImage,
    pub color: [u8; 3],
}

fn read_bounded(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

fn icon_path(directory: &Path, value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    let value = value.strip_prefix("file://").unwrap_or(value);
    if value.contains("://") {
        return None;
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return None;
    }
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        directory.join(path)
    };
    ROOTS
        .iter()
        .any(|root| path.starts_with(root))
        .then_some(path)
}

fn decode(bytes: &[u8]) -> Option<Artwork> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let pixels = image.thumbnail(256, 256).into_rgba8();
    let color = background(&pixels);
    Some(Artwork {
        key: String::new(),
        pixels,
        color,
    })
}

fn background(image: &RgbaImage) -> [u8; 3] {
    // Match the icon's opaque edges rather than letting its central logo choose
    // the card colour. Transparent artwork gets a neutral dark background.
    let sample = image::imageops::resize(image, 48, 48, image::imageops::FilterType::Triangle);
    let mut bins = HashMap::<[u8; 3], (usize, [usize; 3])>::new();
    // Transparent artwork keeps the neutral card even when its logo reaches
    // into the edge band: those few opaque pixels would otherwise fill the
    // card with the logo's own colour and hide it.
    let (mut edge, mut opaque) = (0, 0);
    for (x, y, pixel) in sample.enumerate_pixels() {
        if (6..42).contains(&x) && (6..42).contains(&y) {
            continue;
        }
        edge += 1;
        if pixel[3] < 230 {
            continue;
        }
        opaque += 1;
        let bin = bins
            .entry([pixel[0] >> 4, pixel[1] >> 4, pixel[2] >> 4])
            .or_default();
        bin.0 += 1;
        for channel in 0..3 {
            bin.1[channel] += pixel[channel] as usize;
        }
    }
    if opaque * 2 < edge {
        return [20; 3];
    }
    bins.into_iter()
        .max_by_key(|(key, (count, _))| (*count, *key))
        .map_or([20; 3], |(_, (count, sum))| {
            sum.map(|s| ((s + count / 2) / count) as u8)
        })
}

fn load(source: &Source) -> Option<Artwork> {
    if !valid_app_id(&source.id) {
        return None;
    }
    if let Some(image) = load_local(source, read_bounded) {
        return Some(image);
    }
    // Some jailed installations cannot read other applications' files. Use the
    // same native-authenticated elevated service as inventory, on this worker.
    match crate::platform::read_app_icons(source) {
        Ok(icons) => icons.iter().find_map(|bytes| decode(bytes)),
        Err(error) => {
            eprintln!("Luma installed icon {}: {error}", source.id);
            None
        }
    }
}

fn load_local(
    source: &Source,
    mut read: impl FnMut(&Path, usize) -> Option<Vec<u8>>,
) -> Option<Artwork> {
    for directory in source.directories() {
        let manifest = read(&directory.join("appinfo.json"), 64 * 1024)
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(|value| value.get("id").and_then(Value::as_str) == Some(&source.id));
        let manifest_icon = |key| {
            manifest
                .as_ref()
                .and_then(|m| m.get(key))
                .and_then(Value::as_str)
        };
        for icon in [
            source.large_icon.as_deref(),
            manifest_icon("largeIcon"),
            source.icon.as_deref(),
            manifest_icon("icon"),
            Some("icon.png"),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(artwork) = icon_path(&directory, icon)
                .and_then(|path| read(&path, MAX_FILE))
                .and_then(|bytes| decode(&bytes))
            {
                return Some(artwork);
            }
        }
    }
    None
}

struct Request {
    generation: u64,
    token: u64,
    source: Source,
}
struct Reply {
    request: Request,
    image: Option<Arc<Artwork>>,
}
struct Entry {
    token: u64,
    used: u64,
    // None: pending; Some(None): the read failed, retried at `retry_at`.
    image: Option<Option<Arc<Artwork>>>,
    failures: u32,
    retry_at: Option<std::time::Instant>,
}

/// A failed read (the elevated service timing out under load, say) is
/// retried by itself: 15 s, then doubling to 10 minutes. A success goes to
/// disk, so it never needs reading again.
fn retry_delay(failures: u32) -> std::time::Duration {
    std::time::Duration::from_secs((15u64 << (failures.clamp(1, 7) - 1)).min(600))
}
#[derive(Default)]
struct Cache {
    generation: u64,
    clock: u64,
    entries: HashMap<Source, Entry>,
}

pub struct Service {
    sender: Option<mpsc::SyncSender<Request>>,
    receiver: mpsc::Receiver<Reply>,
    cache: RefCell<Cache>,
}

/// Decoded icons persist in Luma's own storage, named by their source, so
/// after the first read every icon loads from local disk: no elevated read,
/// no wait, across launches and reboots.
fn disk_path(directory: &Path, source: &Source) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    directory.join(format!("{}-{:016x}.png", source.id, hasher.finish()))
}

fn load_cached(directory: Option<&Path>, source: &Source) -> Option<Artwork> {
    // The id names a file: only validated ids (no separators) ever reach it.
    if !valid_app_id(&source.id) {
        return None;
    }
    let Some(directory) = directory else {
        return load(source);
    };
    let path = disk_path(directory, source);
    if let Some(artwork) = read_bounded(&path, MAX_FILE).and_then(|bytes| decode(&bytes)) {
        return Some(artwork);
    }
    let artwork = load(source)?;
    // Written whole then renamed, so a power cut never leaves a torn file.
    let partial = path.with_extension("part");
    if std::fs::create_dir_all(directory).is_ok()
        && artwork.pixels.save_with_format(&partial, image::ImageFormat::Png).is_ok()
    {
        let _ = std::fs::rename(&partial, &path);
    }
    Some(artwork)
}

impl Service {
    pub fn new(enabled: bool) -> Self {
        let directory = enabled
            .then(|| crate::storage::default_path(true).ok())
            .flatten()
            .and_then(|preferences| Some(preferences.parent()?.join("icons")));
        Self::with_loader(enabled, move |source| load_cached(directory.as_deref(), source))
    }

    fn with_loader(
        enabled: bool,
        loader: impl Fn(&Source) -> Option<Artwork> + Send + 'static,
    ) -> Self {
        // Room for every installed app: the whole list is prefetched at once.
        let (sender, requests) = mpsc::sync_channel::<Request>(CACHE_ENTRIES);
        let (responses, receiver) = mpsc::sync_channel(1);
        if enabled {
            thread::spawn(move || {
                crate::background_priority();
                while let Ok(request) = requests.recv() {
                    let image = loader(&request.source).map(|mut image| {
                        image.key = format!("installed-icon:{}", request.token);
                        Arc::new(image)
                    });
                    if responses.send(Reply { request, image }).is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            sender: enabled.then_some(sender),
            receiver,
            cache: RefCell::default(),
        }
    }

    pub fn get(&self, source: &Source) -> Option<Arc<Artwork>> {
        let sender = self.sender.as_ref()?;
        let mut cache = self.cache.borrow_mut();
        cache.clock += 1;
        let clock = cache.clock;
        if let Some(entry) = cache.entries.get_mut(source) {
            entry.used = clock;
            return entry.image.clone().flatten();
        }
        let request = Request {
            generation: cache.generation,
            token: clock,
            source: source.clone(),
        };
        // A full queue is retried when the next completed request redraws the
        // scene. Neither scrolling nor a slow Homebrew service can block input.
        if sender.try_send(request).is_ok() {
            cache.entries.insert(
                source.clone(),
                Entry {
                    token: clock,
                    used: clock,
                    image: None,
                    failures: 0,
                    retry_at: None,
                },
            );
        }
        None
    }

    /// Drop every decoded icon (tests force fresh reads with it).
    #[cfg(test)]
    pub fn reset(&self) {
        let mut cache = self.cache.borrow_mut();
        cache.generation += 1;
        cache.entries.clear();
    }

    /// After an inventory refresh: keep every icon already shown, so nothing
    /// flashes back to a placeholder; only failed or pending reads retry.
    pub fn refresh(&self) {
        let mut cache = self.cache.borrow_mut();
        cache.generation += 1;
        cache
            .entries
            .retain(|_, entry| matches!(entry.image, Some(Some(_))));
    }

    pub fn poll(&self) -> bool {
        let mut changed = false;
        let mut cache = self.cache.borrow_mut();
        while let Ok(reply) = self.receiver.try_recv() {
            changed = true;
            if reply.request.generation == cache.generation
                && let Some(entry) = cache.entries.get_mut(&reply.request.source)
                && entry.token == reply.request.token
            {
                if reply.image.is_none() {
                    entry.failures += 1;
                    entry.retry_at = Some(std::time::Instant::now() + retry_delay(entry.failures));
                }
                entry.image = Some(reply.image);
            }
        }
        // Re-request failed reads whose retry time has come.
        if let Some(sender) = &self.sender {
            let now = std::time::Instant::now();
            let generation = cache.generation;
            let due: Vec<Source> = cache
                .entries
                .iter()
                .filter(|(_, entry)| {
                    matches!(entry.image, Some(None)) && entry.retry_at.is_some_and(|at| at <= now)
                })
                .map(|(source, _)| source.clone())
                .collect();
            for source in due {
                cache.clock += 1;
                let token = cache.clock;
                let request = Request {
                    generation,
                    token,
                    source: source.clone(),
                };
                if sender.try_send(request).is_ok()
                    && let Some(entry) = cache.entries.get_mut(&source)
                {
                    entry.token = token;
                    entry.image = None;
                    entry.retry_at = None;
                }
            }
        }
        if !changed {
            return false;
        }
        let mut bytes: usize = cache
            .entries
            .values()
            .filter_map(|e| e.image.as_ref()?.as_ref())
            .map(|i| i.pixels.as_raw().len())
            .sum();
        let mut old: Vec<_> = cache
            .entries
            .iter()
            .filter(|(_, e)| e.image.is_some())
            .map(|(s, e)| (s.clone(), e.used))
            .collect();
        old.sort_by_key(|(_, used)| *used);
        for (source, _) in old {
            if bytes <= CACHE_BYTES && cache.entries.len() <= CACHE_ENTRIES {
                break;
            }
            if let Some(entry) = cache.entries.remove(&source)
                && let Some(Some(image)) = entry.image
            {
                bytes -= image.pixels.as_raw().len();
            }
        }
        changed
    }
}

#[cfg(test)]
#[path = "app-artwork-tests.rs"]
mod tests;
