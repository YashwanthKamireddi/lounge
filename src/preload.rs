//! Images decode on a background worker and upload a few per frame, so the
//! drawing thread never stalls on a PNG/JPEG decode. Requests come at startup
//! (wallpaper, tiles, Settings thumbnails) and once the app list is known
//! (each app's brand card).
use crate::{background_priority, branding, model, renderer, state::{self, State}, view::Assets};
use std::{
    collections::VecDeque,
    sync::mpsc::{Receiver, Sender},
};

/// What the background preloader hands back for the drawing thread to upload.
pub enum Preloaded {
    Image(image::RgbaImage),
    /// Read straight from the image's shipped ASTC copy: nothing to decode.
    Compressed(renderer::Compressed),
    Vector(Vec<u8>),
}

/// Starts the decoder: paths sent in come back decoded (or as their ASTC
/// copy when the GPU `astc` takes it); a repeat within seconds is skipped.
pub fn start(astc: bool) -> (Sender<String>, Receiver<(String, Preloaded)>) {
    let (requests, queue) = std::sync::mpsc::channel::<String>();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        background_priority();
        // Startup asks for some images twice before either is uploaded: a
        // path decoded in the last few seconds is skipped. Asked again later
        // (refilling what the GPU freed while an app was in front) it decodes
        // again, here, rather than on the drawing thread mid-scroll.
        let mut recent: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        for path in queue {
            let now = std::time::Instant::now();
            if recent.get(&path).is_some_and(|at| now.duration_since(*at) < std::time::Duration::from_secs(10)) {
                continue;
            }
            if recent.len() > 512 {
                recent.retain(|_, at| now.duration_since(*at) < std::time::Duration::from_secs(10));
            }
            recent.insert(path.clone(), now);
            let loaded = if path.ends_with(".lvec") {
                std::fs::read(&path).ok().map(Preloaded::Vector)
            } else if let Some(compressed) = astc.then(|| renderer::read_astc(&path)).flatten() {
                Some(Preloaded::Compressed(compressed))
            } else {
                image::open(&path).ok().map(|image| Preloaded::Image(image.into_rgba8()))
            };
            if let Some(loaded) = loaded
                && sender.send((path, loaded)).is_err()
            {
                break;
            }
        }
    });
    (requests, receiver)
}

/// Decodes every home image ahead of need, wallpaper first, so no icon pops
/// in late and no slide stalls on a decode.
pub fn request_home(preload: &Sender<String>, state: &State, assets: &Assets) {

    let background = &state.preferences.background;
    for name in [
        format!("wallpapers/{background}.jpg"),
        format!("wallpapers/{background}-home-glass.png"),
        format!("wallpapers/{background}-glass.png"),
    ] {
        let _ = preload.send(assets.path(name));
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
        for id in order.iter().take(state::APPS_COLUMNS).filter(|id| assets.heroes.contains(*id)) {
            let _ = preload.send(assets.path(format!("heroes/{id}.jpg")));
            let _ = preload.send(assets.path(format!("heroes/{id}-glass.png")));
        }
    }
    for id in order.iter().filter(|id| assets.tiles.contains_key(*id)) {
        let _ = preload.send(assets.path(format!("tiles/{id}.png")));
    }
    for (id, _) in model::BACKGROUNDS {
        let _ = preload.send(assets.path(format!("thumbnails/{id}.jpg")));
    }
    // Interface glyphs (menus, Settings).
    if let Ok(entries) = std::fs::read_dir(assets.path("ui-icons")) {
        for entry in entries.flatten() {
            let _ = preload.send(entry.path().to_string_lossy().into_owned());
        }
    }
}

/// Decoded images on their way to the GPU. On the TV one 1080p image takes
/// the GPU tens of ms, so while anything moves only one small image goes up
/// per frame and large ones wait for a still screen (four at once froze
/// ~140 ms).
pub struct Uploads {
    decoded: Receiver<(String, Preloaded)>,
    /// Large images held back while the screen moves.
    held: VecDeque<(String, Preloaded)>,
}

impl Uploads {
    pub fn new(decoded: Receiver<(String, Preloaded)>) -> Self {
        Self { decoded, held: VecDeque::new() }
    }

    /// Uploads this frame's share: one small image while `busy` (anything
    /// animating on screen), else up to four, held ones first.
    pub fn pump(&mut self, renderer: &mut renderer::Renderer, busy: bool) {
        let mut budget = if busy { 1 } else { 4 };
        while budget > 0 {
            let next = if busy { None } else { self.held.pop_front() }
                .or_else(|| self.decoded.try_recv().ok());
            let Some((path, loaded)) = next else {
                break;
            };
            if busy && matches!(&loaded, Preloaded::Image(image) if image.width() * image.height() > 600_000) {
                self.held.push_back((path, loaded));
                continue;
            }
            match loaded {
                Preloaded::Image(image) => renderer.adopt(path, image),
                Preloaded::Compressed(image) => renderer.adopt_compressed(path, &image),
                Preloaded::Vector(bytes) => renderer.adopt_vector(path, &bytes),
            }
            budget -= 1;
        }
    }
}

/// Back from an app: the tiles freed while it was in front
/// decode again in the background, before any scroll reaches
/// them. Only what is missing is queued.
pub fn refill(preload: &Sender<String>, state: &State, assets: &Assets, renderer: &renderer::Renderer) {
    if state.preferences.top_shelf_art {
        for app in state.apps().into_iter().take(state::APPS_COLUMNS) {
            if assets.heroes.contains(&app.id) {
                for name in [format!("heroes/{}.jpg", app.id), format!("heroes/{}-glass.png", app.id)] {
                    let path = assets.path(name);
                    if !renderer.has(&path) {
                        let _ = preload.send(path);
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
                let _ = preload.send(path);
            }
        }
    }
}

/// After the first frame: fetch every app's icon in the
/// background, so no tile shows a placeholder on scrolling.
pub fn prefetch(preload: &Sender<String>, state: &State, assets: &Assets) {
    // Only apps Luma ships no artwork for read their icon from
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
                if std::path::Path::new(&path).is_file() {
                    let _ = preload.send(path);
                }
            }
        }
    }
}
