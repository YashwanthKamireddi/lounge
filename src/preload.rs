//! Images decode on a background worker and upload a few per frame, so the
//! drawing thread never stalls on a PNG/JPEG decode. Requests come at startup
//! (wallpaper, tiles, Settings thumbnails) and once the app list is known
//! (each app's brand card).
use crate::{
    background_priority, branding, model, renderer,
    state::{self, State},
    view::Assets,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
    },
};

/// What the background preloader hands back for the drawing thread to upload.
pub enum Preloaded {
    Image(image::RgbaImage),
    /// Read straight from the image's shipped ASTC copy: nothing to decode.
    Compressed(renderer::Compressed),
    Vector(Vec<u8>),
}

/// What the drawing thread asks of the decoder.
enum Request {
    Decode(String),
    /// The GPU let go of images (an app came to the front): any of them asked
    /// for again decodes again, however recently it was decoded.
    Forget,
    /// Nothing to do; wakes a decoder holding images until the GPU's ASTC
    /// support is known.
    Wake,
}

/// The decoder and the drawing thread's side of it. Requests go in from the
/// moment the assets are known, before the window exists, so decoding
/// overlaps SDL, EGL and shader start-up; uploads stay on the drawing thread.
/// Behind an app nothing decodes or reaches the GPU: what was asked for then
/// is remembered and asked for again on return.
pub struct Preload {
    requests: Sender<Request>,
    /// Decoded images; None for a path the decoder passed over while Lounge
    /// was behind an app.
    decoded: Receiver<(String, Option<Preloaded>)>,
    /// Large images held back while the screen moves.
    held: VecDeque<(String, Preloaded)>,
    /// Set while Lounge is behind an app: the decoder passes requests over.
    paused: Arc<AtomicBool>,
    /// Whether the GPU takes ASTC, known once the renderer exists.
    astc: Arc<OnceLock<bool>>,
    /// Paths passed over or dropped behind an app, asked for again on return.
    missed: Vec<String>,
    /// Vector meshes asked for: the renderer never frees one, so each is read
    /// once (an inventory refresh asked for every logo again).
    vectors: HashSet<String>,
}

impl Preload {
    /// Starts the decoder: paths sent in come back decoded (or as their ASTC
    /// copy once the GPU is known to take it); a repeat within seconds is
    /// skipped unless the GPU has let go of images since (see `forget`).
    pub fn start() -> Self {
        let (requests, queue) = std::sync::mpsc::channel();
        let (sender, decoded) = std::sync::mpsc::channel();
        let paused = Arc::new(AtomicBool::new(false));
        let astc = Arc::new(OnceLock::new());
        let worker = (paused.clone(), astc.clone());
        std::thread::spawn(move || {
            background_priority();
            decode(&queue, &sender, &worker.0, &worker.1);
        });
        Self {
            requests,
            decoded,
            held: VecDeque::new(),
            paused,
            astc,
            missed: Vec::new(),
            vectors: HashSet::new(),
        }
    }

    /// The renderer exists: images waiting on whether to read their ASTC copy
    /// or decode go ahead.
    pub fn set_astc(&self, astc: bool) {
        let _ = self.astc.set(astc);
        let _ = self.requests.send(Request::Wake);
    }

    /// Queues `path` for decoding.
    pub fn request(&mut self, path: String) {
        if path.ends_with(".lvec") && !self.vectors.insert(path.clone()) {
            return;
        }
        let _ = self.requests.send(Request::Decode(path));
    }

    /// The GPU let go of images (see `renderer.trim`): asked for again, each
    /// decodes again in the background. Skipped as recent, a refill on a quick
    /// return (an app closed within seconds) left them to load on the drawing
    /// thread mid-scroll, ~140 ms each on the TV.
    pub fn forget(&self) {
        let _ = self.requests.send(Request::Forget);
    }

    /// Uploads this frame's share: one small image while `busy` (anything
    /// animating on screen), else up to four, held ones first. Behind an app
    /// (`in_front` false) nothing is decoded or uploaded: anything already
    /// decoded is dropped, freeing its memory for the app, and asked for
    /// again on return (see `resume`). Uploaded then, it stayed on the GPU for
    /// the whole app session: an adopted texture counts as drawn in the last
    /// frame, which `trim` keeps.
    pub fn pump(&mut self, renderer: &mut renderer::Renderer, busy: bool, in_front: bool) {
        if !in_front {
            self.behind();
            return;
        }
        self.paused.store(false, Ordering::Relaxed);
        let mut budget = if busy { 1 } else { 4 };
        while budget > 0 {
            let Some((path, loaded)) = self.next(busy) else {
                break;
            };
            match loaded {
                Preloaded::Image(image) => renderer.adopt(path, image),
                Preloaded::Compressed(image) => renderer.adopt_compressed(path, &image),
                Preloaded::Vector(bytes) => renderer.adopt_vector(path, &bytes),
            }
            budget -= 1;
        }
    }

    /// The next image to upload, if any may go up now.
    fn next(&mut self, busy: bool) -> Option<(String, Preloaded)> {
        loop {
            let (path, loaded) = match if busy { None } else { self.held.pop_front() } {
                Some(held) => held,
                None => match self.decoded.try_recv().ok()? {
                    (path, Some(loaded)) => (path, loaded),
                    // Passed over just as Lounge came back: ask again now.
                    (path, None) => {
                        let _ = self.requests.send(Request::Decode(path));
                        continue;
                    }
                },
            };
            if busy
                && matches!(&loaded, Preloaded::Image(image) if image.width() * image.height() > 600_000)
            {
                self.held.push_back((path, loaded));
                continue;
            }
            return Some((path, loaded));
        }
    }

    /// Behind an app: the decoder passes requests over, and everything
    /// decoded or held is let go and remembered.
    pub fn behind(&mut self) {
        self.paused.store(true, Ordering::Relaxed);
        let held = std::mem::take(&mut self.held)
            .into_iter()
            .map(|(path, _)| path);
        let decoded = self.decoded.try_iter().map(|(path, _)| path);
        for path in held.chain(decoded).collect::<Vec<_>>() {
            if !self.missed.contains(&path) {
                self.missed.push(path);
            }
        }
    }

    /// Back in front: what was let go behind the app and is still not on the
    /// GPU (`has`) is asked for again.
    pub fn resume(&mut self, has: impl Fn(&str) -> bool) {
        // Before the first tick in front lifts it: a refill right after this
        // decodes rather than being passed over.
        self.paused.store(false, Ordering::Relaxed);
        for path in std::mem::take(&mut self.missed) {
            if path.ends_with(".lvec") || !has(&path) {
                let _ = self.requests.send(Request::Decode(path));
            }
        }
    }
}

/// The decoder's loop, until Lounge stops.
fn decode(
    queue: &Receiver<Request>,
    sender: &Sender<(String, Option<Preloaded>)>,
    paused: &AtomicBool,
    astc: &OnceLock<bool>,
) {
    let window = std::time::Duration::from_secs(10);
    // Startup asks for some images twice before either is uploaded: a path
    // decoded in the last few seconds is skipped, until `forget`.
    let mut recent: HashMap<String, std::time::Instant> = HashMap::new();
    // Images with an ASTC copy, asked for before the GPU is known: which form
    // to load waits for it, while images without one decode meanwhile.
    let mut waiting: VecDeque<String> = VecDeque::new();
    loop {
        if let Some(&astc) = astc.get() {
            while let Some(path) = waiting.pop_front() {
                if !deliver(sender, &mut recent, paused, path, |path| load(path, astc)) {
                    return;
                }
            }
        }
        let Ok(request) = queue.recv() else {
            return;
        };
        let path = match request {
            Request::Decode(path) => path,
            Request::Forget => {
                recent.clear();
                continue;
            }
            Request::Wake => continue,
        };
        if paused.load(Ordering::Relaxed) {
            if sender.send((path, None)).is_err() {
                return;
            }
            continue;
        }
        let now = std::time::Instant::now();
        if recent
            .get(&path)
            .is_some_and(|at| now.duration_since(*at) < window)
        {
            continue;
        }
        if recent.len() > 512 {
            recent.retain(|_, at| now.duration_since(*at) < window);
        }
        recent.insert(path.clone(), now);
        let known = astc.get().copied();
        if known.is_none()
            && !path.ends_with(".lvec")
            && std::path::Path::new(&format!("{path}.astc")).is_file()
        {
            waiting.push_back(path);
            continue;
        }
        // Without an ASTC copy the image decodes the same either way.
        let astc = known.unwrap_or(false);
        if !deliver(sender, &mut recent, paused, path, |path| load(path, astc)) {
            return;
        }
    }
}

/// Loads `path` and sends it, or passes it over while Lounge is behind an
/// app. False once the drawing thread is gone.
fn deliver(
    sender: &Sender<(String, Option<Preloaded>)>,
    recent: &mut HashMap<String, std::time::Instant>,
    paused: &AtomicBool,
    path: String,
    load: impl Fn(&str) -> Option<Preloaded>,
) -> bool {
    if paused.load(Ordering::Relaxed) {
        recent.remove(&path);
        return sender.send((path, None)).is_ok();
    }
    match load(&path) {
        Some(loaded) => sender.send((path, Some(loaded))).is_ok(),
        // Unreadable: nothing to send, and not asked for again.
        None => true,
    }
}

/// A `.lvec` mesh's bytes, an image's ASTC copy when the GPU takes it, or the
/// decoded image.
fn load(path: &str, astc: bool) -> Option<Preloaded> {
    if path.ends_with(".lvec") {
        std::fs::read(path).ok().map(Preloaded::Vector)
    } else if let Some(compressed) = astc.then(|| renderer::read_astc(path)).flatten() {
        Some(Preloaded::Compressed(compressed))
    } else {
        image::open(path)
            .ok()
            .map(|image| Preloaded::Image(image.into_rgba8()))
    }
}

/// Decodes every home image ahead of need, wallpaper first, so no icon pops
/// in late and no slide stalls on a decode.
pub fn request_home(preload: &mut Preload, state: &State, assets: &Assets) {
    let background = &state.preferences.background;
    for name in [
        format!("wallpapers/{background}.jpg"),
        format!("wallpapers/{background}-home-glass.png"),
        format!("wallpapers/{background}-glass.png"),
    ] {
        preload.request(assets.path(name));
    }
    let order: Vec<String> = state
        .preferences
        .favorites(&state.inventory.apps)
        .into_iter()
        .chain(assets.tiles.keys().cloned())
        .collect();
    // Top Shelf art for the dock apps: decoded on first focus it froze the
    // screen ~140 ms per app on the TV.
    if state.preferences.top_shelf_art {
        for id in order
            .iter()
            .take(state::APPS_COLUMNS)
            .filter(|id| assets.heroes.contains(*id))
        {
            preload.request(assets.path(format!("heroes/{id}.jpg")));
            preload.request(assets.path(format!("heroes/{id}-glass.png")));
        }
    }
    for id in order.iter().filter(|id| assets.tiles.contains_key(*id)) {
        preload.request(assets.path(format!("tiles/{id}.png")));
    }
    for (id, _) in model::BACKGROUNDS {
        preload.request(assets.path(format!("thumbnails/{id}.jpg")));
    }
    // Interface glyphs (menus, Settings).
    if let Ok(entries) = std::fs::read_dir(assets.path("ui-icons")) {
        for entry in entries.flatten() {
            preload.request(entry.path().to_string_lossy().into_owned());
        }
    }
}

/// Back from an app: the tiles freed while it was in front
/// decode again in the background, before any scroll reaches
/// them. Only what is missing is queued.
pub fn refill(
    preload: &mut Preload,
    state: &State,
    assets: &Assets,
    renderer: &renderer::Renderer,
) {
    if state.preferences.top_shelf_art {
        for app in state.apps().into_iter().take(state::APPS_COLUMNS) {
            if assets.heroes.contains(&app.id) {
                for name in [
                    format!("heroes/{}.jpg", app.id),
                    format!("heroes/{}-glass.png", app.id),
                ] {
                    let path = assets.path(name);
                    if !renderer.has(&path) {
                        preload.request(path);
                    }
                }
            }
        }
    }
    for app in &state.inventory.apps {
        let names = if assets.tiles.contains_key(&app.id) {
            vec![format!("tiles/{}.png", app.id)]
        } else if app.brand != "generic" {
            vec![format!("surfaces/{}.png", app.brand)]
        } else {
            vec![]
        };
        for name in names {
            let path = assets.path(name);
            if !renderer.has(&path) && std::path::Path::new(&path).is_file() {
                preload.request(path);
            }
        }
    }
}

/// After the first frame: fetch every app's icon in the
/// background, so no tile shows a placeholder on scrolling. Runs again after
/// every inventory refresh: only what is not loaded yet is queued (each
/// refresh re-decoded every brand card's surface).
pub fn prefetch(
    preload: &mut Preload,
    state: &State,
    assets: &Assets,
    renderer: &renderer::Renderer,
) {
    // Only apps Lounge ships no artwork for read their icon from
    // the TV; every other tile is complete from the first frame.
    for source in state
        .inventory
        .apps
        .iter()
        .filter(|app| {
            !assets.tiles.contains_key(&app.id)
                && branding::for_key(&app.brand).is_none()
                && !assets.icons.contains_key(&app.id)
        })
        .filter_map(|app| app.icon.as_ref())
    {
        let _ = assets.artwork.get(source);
    }
    // Brand cards for apps without their own tile art.
    for app in &state.inventory.apps {
        if !assets.tiles.contains_key(&app.id) && app.brand != "generic" {
            for name in [
                format!("surfaces/{}.png", app.brand),
                format!("logos/{}.lvec", app.brand),
            ] {
                let path = assets.path(name);
                // Logos are meshes the renderer never frees: `request` reads
                // each once.
                if !renderer.has(&path) && std::path::Path::new(&path).is_file() {
                    preload.request(path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn folder(name: &str) -> std::path::PathBuf {
        let folder =
            std::env::temp_dir().join(format!("lounge-preload-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn picture(folder: &std::path::Path, name: &str) -> String {
        let path = folder.join(name);
        image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        path.to_string_lossy().into_owned()
    }

    /// The next decoded result, waiting up to a second for the worker.
    fn arrival(preload: &mut Preload) -> Option<(String, Option<Preloaded>)> {
        preload.decoded.recv_timeout(Duration::from_secs(1)).ok()
    }

    #[test]
    fn a_refill_after_a_trim_decodes_again_at_once() {
        let folder = folder("forget");
        let tile = picture(&folder, "tile.png");
        let mut preload = Preload::start();
        preload.set_astc(false);
        preload.request(tile.clone());
        assert!(
            matches!(arrival(&mut preload), Some((path, Some(Preloaded::Image(_)))) if path == tile)
        );
        // Asked twice at startup: the repeat is skipped.
        preload.request(tile.clone());
        assert!(arrival(&mut preload).is_none());
        // The GPU let it go; a return seconds later asks again and gets it.
        preload.forget();
        preload.request(tile.clone());
        assert!(matches!(arrival(&mut preload), Some((path, Some(_))) if path == tile));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn behind_an_app_nothing_decodes_and_the_return_asks_again() {
        let folder = folder("behind");
        let tile = picture(&folder, "tile.png");
        let thumbnail = picture(&folder, "thumbnail.png");
        let mut preload = Preload::start();
        preload.set_astc(false);
        // Decoded just before an app came to the front: let go, remembered.
        preload.request(tile.clone());
        let decoded = arrival(&mut preload).unwrap();
        preload.held.push_back((decoded.0, decoded.1.unwrap()));
        preload.behind();
        assert!(preload.held.is_empty());
        // Asked for behind the app: passed over, not decoded.
        preload.request(thumbnail.clone());
        let (path, loaded) = arrival(&mut preload).unwrap();
        assert!(loaded.is_none() && path == thumbnail);
        preload.missed.push(path);
        // Back in front: only what is still missing from the GPU goes again.
        preload.resume(|path| path == tile);
        assert!(preload.missed.is_empty());
        assert!(matches!(arrival(&mut preload), Some((path, Some(_))) if path == thumbnail));
        assert!(arrival(&mut preload).is_none(), "the tile was on the GPU");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn images_decode_before_the_gpu_is_known_and_astc_copies_wait_for_it() {
        let folder = folder("astc");
        let tile = picture(&folder, "tile.png");
        let wallpaper = picture(&folder, "wallpaper.png");
        // A 4×4 ASTC copy: one 16-byte block.
        let mut astc = vec![0x13, 0xab, 0xa1, 0x5c, 4, 4, 1, 4, 0, 0, 4, 0, 0, 1, 0, 0];
        astc.extend([0; 16]);
        std::fs::write(format!("{wallpaper}.astc"), astc).unwrap();
        let mut preload = Preload::start();
        preload.request(wallpaper.clone());
        preload.request(tile.clone());
        // No window yet: the tile decodes; the wallpaper waits for the answer.
        assert!(
            matches!(arrival(&mut preload), Some((path, Some(Preloaded::Image(_)))) if path == tile)
        );
        assert!(arrival(&mut preload).is_none());
        preload.set_astc(true);
        assert!(matches!(
            arrival(&mut preload),
            Some((path, Some(Preloaded::Compressed(_)))) if path == wallpaper
        ));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_logo_mesh_is_read_once() {
        let folder = folder("logo");
        let logo = folder.join("brand.lvec").to_string_lossy().into_owned();
        std::fs::write(&logo, b"mesh").unwrap();
        let mut preload = Preload::start();
        preload.set_astc(false);
        preload.request(logo.clone());
        assert!(matches!(
            arrival(&mut preload),
            Some((_, Some(Preloaded::Vector(_))))
        ));
        // An inventory refresh asks again (and the worker's memory was cleared).
        preload.forget();
        preload.request(logo);
        assert!(arrival(&mut preload).is_none());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
