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
    sync::{Arc, Mutex, PoisonError, mpsc},
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
            for value in [self.large_icon.as_deref(), self.icon.as_deref()]
                .into_iter()
                .flatten()
            {
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
    /// The renderer's texture for it: one per app, so an updated icon
    /// replaces the old texture instead of leaving it behind.
    pub key: String,
    /// The source the pixels came from (its hash): a different one under the
    /// same key is a changed icon, uploaded again.
    pub source: u64,
    /// The card behind the icon: top and bottom of a vertical gradient (equal when flat).
    pub backdrop: [[u8; 3]; 2],
    size: (u32, u32),
    /// The decoded icon (up to 256 KB) until the renderer has uploaded it;
    /// after that only if it cannot be read back from `reload`. The GPU copy
    /// is the one drawn, so holding a CPU copy of every icon behind it only
    /// cost memory.
    pixels: Mutex<Option<RgbaImage>>,
    reload: Option<Reload>,
    /// Corner radius as a share of the drawn height: 0 for icons (drawn
    /// inside their card), a card's own rounding for a picture that is one.
    pub corner: f32,
}

/// Where an icon's pixels can be read back from: its file in the icons cache,
/// and whether they went through decode() (an icon read from that cache) or
/// are the file's exact contents (an icon read from the app and saved there).
struct Reload {
    path: PathBuf,
    decoded: bool,
}

impl Artwork {
    fn new(pixels: RgbaImage, backdrop: [[u8; 3]; 2]) -> Self {
        Self {
            key: String::new(),
            source: 0,
            backdrop,
            size: pixels.dimensions(),
            pixels: Mutex::new(Some(pixels)),
            reload: None,
            corner: 0.,
        }
    }

    /// A picture decoded elsewhere (a switcher snapshot), uploaded under
    /// `key`; a new `source` there replaces the old texture. Its pixels stay
    /// in memory while something holds it, to upload again after a trim.
    pub fn picture(key: String, source: u64, pixels: RgbaImage, corner: f32) -> Arc<Self> {
        Arc::new(Self {
            key,
            source,
            corner,
            ..Self::new(pixels, [[0; 3]; 2])
        })
    }

    /// Hands the pixels to `upload` (the renderer's texture upload), first
    /// reading them back from the icons cache if an earlier upload let them
    /// go (the texture was evicted since). None if they cannot be read.
    pub fn upload<T>(&self, upload: impl FnOnce(&RgbaImage) -> T) -> Option<T> {
        let mut held = self.pixels.lock().unwrap_or_else(PoisonError::into_inner);
        let pixels = match held.take() {
            Some(pixels) => pixels,
            None => self.read_back()?,
        };
        let uploaded = upload(&pixels);
        if self.reload.is_none() {
            *held = Some(pixels);
        }
        Some(uploaded)
    }

    /// The texture already holds these pixels: let the CPU copy go if it can
    /// be read back.
    pub fn uploaded(&self) {
        if self.reload.is_some() {
            self.pixels
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
        }
    }

    /// The decoded pixels this artwork holds in memory.
    fn held_bytes(&self) -> usize {
        self.pixels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map_or(0, |pixels| pixels.as_raw().len())
    }

    /// The same pixels again, from the icons cache.
    fn read_back(&self) -> Option<RgbaImage> {
        let reload = self.reload.as_ref()?;
        let bytes = read_bounded(&reload.path, MAX_FILE)?;
        let pixels = if reload.decoded {
            decode(&bytes)?.pixels.into_inner().ok()??
        } else {
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .ok()?
                .into_rgba8()
        };
        (pixels.dimensions() == self.size).then_some(pixels)
    }

    #[cfg(test)]
    fn pixels(&self) -> RgbaImage {
        self.pixels.lock().unwrap().clone().unwrap()
    }
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
    let pixels = thumbnail(image.into_rgba8());
    let backdrop = backdrop(&pixels);
    Some(Artwork::new(pixels, backdrop))
}

/// Scales with premultiplied alpha, so the colour of transparent pixels (often black) never
/// darkens antialiased edges. Transparent texels then take their neighbours' colour, which
/// the GPU's linear filter blends in at the logo's edge.
fn thumbnail(mut image: RgbaImage) -> RgbaImage {
    for pixel in image.pixels_mut() {
        let alpha = pixel[3] as u32;
        for channel in 0..3 {
            pixel[channel] = ((pixel[channel] as u32 * alpha + 127) / 255) as u8;
        }
    }
    let mut pixels = image::DynamicImage::ImageRgba8(image)
        .thumbnail(256, 256)
        .into_rgba8();
    for pixel in pixels.pixels_mut() {
        let alpha = pixel[3] as u32;
        for channel in 0..3 {
            if let Some(value) = (pixel[channel] as u32 * 255 + alpha / 2).checked_div(alpha) {
                pixel[channel] = value.min(255) as u8;
            }
        }
    }
    bleed(&mut pixels, 6);
    pixels
}

/// Gives fully transparent pixels the average colour of coloured neighbours, `passes` deep.
fn bleed(pixels: &mut RgbaImage, passes: usize) {
    let (width, height) = pixels.dimensions();
    let mut coloured: Vec<bool> = pixels.pixels().map(|pixel| pixel[3] > 0).collect();
    for _ in 0..passes {
        let source = pixels.clone();
        let known = coloured.clone();
        for y in 0..height {
            for x in 0..width {
                if known[(y * width + x) as usize] {
                    continue;
                }
                let (mut count, mut sum) = (0, [0u32; 3]);
                for ny in y.saturating_sub(1)..(y + 2).min(height) {
                    for nx in x.saturating_sub(1)..(x + 2).min(width) {
                        if known[(ny * width + nx) as usize] {
                            let pixel = source.get_pixel(nx, ny);
                            count += 1;
                            for channel in 0..3 {
                                sum[channel] += pixel[channel] as u32;
                            }
                        }
                    }
                }
                if let Some(average) = sum
                    .map(|total| total.checked_div(count))
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                {
                    let pixel = pixels.get_pixel_mut(x, y);
                    for channel in 0..3 {
                        pixel[channel] = average[channel] as u8;
                    }
                    coloured[(y * width + x) as usize] = true;
                }
            }
        }
    }
}

fn backdrop(image: &RgbaImage) -> [[u8; 3]; 2] {
    // Match the icon's opaque edges rather than letting its central logo choose
    // the card colour. Transparent artwork gets a gradient of its own instead, even
    // when its logo reaches into the edge band: those few opaque pixels would
    // otherwise fill the card with the logo's own colour and hide it.
    let sample = image::imageops::resize(image, 48, 48, image::imageops::FilterType::Triangle);
    let mut bins = HashMap::<[u8; 3], (usize, [usize; 3])>::new();
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
        return logo_backdrop(image);
    }
    bins.into_iter()
        .max_by_key(|(key, (count, _))| (*count, *key))
        .map_or_else(
            || logo_backdrop(image),
            |(_, (count, sum))| {
                let rgb = sum.map(|s| ((s + count / 2) / count) as u8);
                [rgb, rgb]
            },
        )
}

/// Transparent artwork: a gradient in the logo's dominant hue, light behind a dark logo and
/// dark behind a light one, with at least 3:1 contrast against the logo's average luminance.
/// Logos without a clear hue get a neutral slate.
fn logo_backdrop(image: &RgbaImage) -> [[u8; 3]; 2] {
    let (mut weight, mut luminance_sum, mut saturated_weight, mut saturation_sum) =
        (0., 0., 0., 0.);
    let mut hues = [(0f32, 0f32, 0f32); 24];
    for pixel in image.pixels() {
        let alpha = pixel[3] as f32 / 255.;
        if alpha < 0.25 {
            continue;
        }
        let rgb = [pixel[0], pixel[1], pixel[2]];
        weight += alpha;
        luminance_sum += alpha * luminance(rgb);
        let (hue, saturation, value) = hsv(rgb);
        if saturation >= 0.3 && value >= 0.2 {
            let bin = &mut hues[(hue / 15.) as usize % 24];
            let strength = alpha * saturation;
            bin.0 += strength;
            bin.1 += strength * hue.to_radians().cos();
            bin.2 += strength * hue.to_radians().sin();
            saturated_weight += alpha;
            saturation_sum += alpha * saturation;
        }
    }
    let logo = if weight > 0. {
        luminance_sum / weight
    } else {
        1.
    };
    let (hue, saturation) = if weight > 0. && saturated_weight >= 0.15 * weight {
        let (_, x, y) = hues
            .into_iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or_default();
        (
            y.atan2(x).to_degrees().rem_euclid(360.),
            (saturation_sum / saturated_weight).min(0.6),
        )
    } else {
        (220., 0.12)
    };
    let gradient = |top: f32, saturation: f32| {
        // Move both stops away from the logo's lightness until each has enough contrast.
        let step = if top > 0.5 { 0.02 } else { -0.02 };
        let mut top = top;
        loop {
            let stops = [hsl(hue, saturation, top), hsl(hue, saturation, top - 0.1)];
            let worst = stops
                .iter()
                .map(|stop| contrast(luminance(*stop), logo))
                .fold(f32::MAX, f32::min);
            if worst >= 3. || !(0.14..=0.98).contains(&(top + step)) {
                return (worst, stops);
            }
            top += step;
        }
    };
    let light = gradient(0.9, saturation * 0.8);
    let dark = gradient(0.27, saturation);
    if light.0 > dark.0 { light.1 } else { dark.1 }
}

fn luminance(rgb: [u8; 3]) -> f32 {
    let linear = |c: u8| {
        let c = c as f32 / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Hue in degrees, saturation and value in 0..1.
fn hsv(rgb: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.);
    let max = r.max(g).max(b);
    let range = max - r.min(g).min(b);
    let hue = if range == 0. {
        0.
    } else if max == r {
        60. * ((g - b) / range).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / range + 2.)
    } else {
        60. * ((r - g) / range + 4.)
    };
    (hue, if max == 0. { 0. } else { range / max }, max)
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> [u8; 3] {
    let chroma = (1. - (2. * lightness - 1.).abs()) * saturation;
    let x = chroma * (1. - ((hue / 60.).rem_euclid(2.) - 1.).abs());
    let (r, g, b) = match (hue / 60.) as u32 {
        0 => (chroma, x, 0.),
        1 => (x, chroma, 0.),
        2 => (0., chroma, x),
        3 => (0., x, chroma),
        4 => (x, 0., chroma),
        _ => (chroma, 0., x),
    };
    let m = lightness - chroma / 2.;
    [r, g, b].map(|c| ((c + m).clamp(0., 1.) * 255.).round() as u8)
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
            eprintln!("Lounge installed icon {}: {error}", source.id);
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

/// Decoded icons persist in Lounge's own storage, named by their source, so
/// after the first read every icon loads from local disk: no elevated read,
/// no wait, across launches and reboots.
fn disk_path(directory: &Path, source: &Source) -> PathBuf {
    directory.join(format!("{}-{:016x}.png", source.id, source_hash(source)))
}

fn source_hash(source: &Source) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
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
    if let Some(mut artwork) = read_bounded(&path, MAX_FILE).and_then(|bytes| decode(&bytes)) {
        artwork.reload = Some(Reload {
            path,
            decoded: true,
        });
        return Some(artwork);
    }
    let mut artwork = load(source)?;
    // Written whole then renamed, so a power cut never leaves a torn file.
    let partial = path.with_extension("part");
    let saved = std::fs::create_dir_all(directory).is_ok()
        && artwork
            .pixels
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|pixels| {
                pixels
                    .save_with_format(&partial, image::ImageFormat::Png)
                    .is_ok()
            })
        && std::fs::rename(&partial, &path).is_ok();
    if saved {
        artwork.reload = Some(Reload {
            path,
            decoded: false,
        });
    }
    Some(artwork)
}

impl Service {
    pub fn new(enabled: bool) -> Self {
        let directory = enabled
            .then(|| crate::storage::default_path(true).ok())
            .flatten()
            .and_then(|preferences| Some(preferences.parent()?.join("icons")));
        Self::with_loader(enabled, move |source| {
            load_cached(directory.as_deref(), source)
        })
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
                        image.key = format!("installed-icon:{}", request.source.id);
                        image.source = source_hash(&request.source);
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
            .map(|i| i.held_bytes())
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
                bytes -= image.held_bytes();
            }
        }
        changed
    }
}

#[cfg(test)]
#[path = "app-artwork-tests.rs"]
mod tests;
