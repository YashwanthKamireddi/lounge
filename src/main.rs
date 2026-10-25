mod app_artwork;
mod avatars;
mod branding;
mod debug;
mod diagnostics;
mod effects;
mod font;
mod i18n;
mod input;
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
mod luna;
mod crash_guard;
mod damage;
mod model;
mod sdl_events;
mod platform;
mod options;
mod raster;
mod renderer;
#[path = "scroll-preview.rs"]
mod scroll_preview;
mod script;
mod state;
mod storage;
mod vector;
mod view;
mod weather;
#[path = "weather-service.rs"]
mod weather_service;
#[cfg(all(feature = "webos", any(target_os = "linux", test)))]
#[path = "webos-compat.rs"]
mod webos_compat;

#[cfg(test)]
mod tests;

use effects::{effects, save_order};
use input::{
    HELD_MOVE, Press, WEBOS_BACK_KEYCODE, WEBOS_KEY_BACK, WEBOS_KEY_EXIT, WEBOS_KEY_RECENT, hit,
    pointer_position, quick_slot,
};
use options::Options;
use platform::{Platform, Request, Response};
use raster::Size;
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
    mouse::MouseButton,
    video::GLProfile,
};
use state::{Direction, Effect, Overlay, State, Target};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

/// How long an inventory read stays fresh before a return re-reads it.
const INVENTORY_FRESH: Duration = Duration::from_secs(30);
/// What the background preloader hands back for the drawing thread to upload.
enum Preloaded {
    Image(image::RgbaImage),
    /// Read straight from the image's shipped ASTC copy: nothing to decode.
    Compressed(renderer::Compressed),
    Vector(Vec<u8>),
}

fn main() {
    debug::debug_log();
    if let Err(error) = run() {
        eprintln!("Luma native: {error}");
        std::process::exit(1);
    }
}

/// Background workers yield the CPU to the UI thread (Linux `nice`), so a
/// decode or icon read never delays a frame on the TV's small CPU.
pub fn background_priority() {
    #[cfg(feature = "webos")]
    // SAFETY: nice() only adjusts this thread's scheduling priority.
    unsafe {
        libc::nice(10);
    }
}

fn compose(
    state: &mut State,
    assets: &view::Assets,
    now: OffsetDateTime,
    previous: Option<&view::Scene>,
) -> view::Scene {
    state.track_focus();
    state.glide_scroll();
    // A new Settings level pushes from this very frame, out of exactly what
    // the last frame showed.
    if state.track_settings_level()
        && let Some(previous) = previous
        && let Some(range) = previous.settings_range.clone()
    {
        state.settings_ghost = Some(previous.draw[range].to_vec());
    }
    let scene = view::scene(state, assets, now);
    if !state.pointer && state.overlay.is_none() && state.reveal_focus(&scene.controls) {
        view::scene(state, assets, now)
    } else {
        scene
    }
}

fn release_memory(renderer: &mut renderer::Renderer) {
    // Keep what the home needs to come back instantly (its textures, glyphs
    // and decoded icons); free only what it does not show.
    renderer.trim();
    // Hand the freed heap back to the system rather than keeping it resident.
    #[cfg(all(feature = "webos", unix))]
    unsafe {
        libc::malloc_trim(0);
    }
}

fn weather_tick(state: &mut State, service: &weather_service::Service, visible: bool) {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let (command, changed) = state.weather.sync(
        &state.preferences,
        visible && state.page == state::Page::Apps,
        now,
    );
    state.dirty |= changed;
    if let Some(search) = state.weather.due_search() {
        state.dirty = true;
        service.send(search);
    }
    service.generations(state.weather.generations());
    if let Some(command) = command {
        service.send(command);
    }
    while let Ok(response) = service.receiver.try_recv() {
        match response {
            weather_service::Response::Search { id, result } => {
                state.dirty |= state.weather.accept_search(id, result)
            }
            weather_service::Response::Forecast { id, key, result } => {
                let (changed, command) = state.weather.accept_forecast(id, &key, result, now);
                state.dirty |= changed;
                if let Some(command) = command {
                    service.send(command);
                }
            }
        }
    }
}

fn run() -> Result<(), String> {
    let startup = Instant::now();
    let mut options = Options::parse()?;
    let webos = cfg!(feature = "webos");
    let startup_stage = |stage: &str| {
        if webos {
            eprintln!(
                "Luma startup: {stage} after {} ms",
                startup.elapsed().as_millis()
            );
        }
    };
    if !webos && std::path::Path::new("/etc/starfish-release").exists() {
        return Err("This desktop build cannot run on a TV. Build with --features webos.".into());
    }
    if webos {
        let surface_id = platform::APP_ID;
        // SDL-webOS reads APPID for its shell surface and getenv (not a hint)
        // for WMCLASS. Set these before video init and before starting workers.
        // SAFETY: startup is still single-threaded here.
        unsafe {
            std::env::set_var("APPID", surface_id);
            std::env::set_var("SDL_VIDEO_WAYLAND_WMCLASS", surface_id);
        }
        eprintln!("Luma webOS surface: {surface_id}");
        sdl2::hint::set("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
        if options.preload {
            if !platform::preload_may_show() {
                platform::status("preload at boot: an app was in front; stood aside");
                return Ok(());
            }
            platform::status("preload at boot: LG Home in front; showing");
        }
    }
    // Resolve the local offset before artwork, HTTP or platform workers start.
    let local_offset = OffsetDateTime::now_local()
        .map(|now| now.offset())
        .unwrap_or(time::UtcOffset::UTC);
    let root = options.assets.clone().unwrap_or_else(|| {
        if webos {
            std::env::current_exe()
                .unwrap_or_default()
                .with_file_name("assets")
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".build/assets")
        }
    });
    let assets = view::Assets::load(root, webos)?;
    startup_stage("asset metadata");
    let default_preferences = options
        .preferences
        .clone()
        .map(Ok)
        .unwrap_or_else(|| storage::default_path(webos))?;
    let preferences_path = if options.test_extra_apps > 0 && options.preferences.is_none() {
        default_preferences.with_file_name("scroll-preview-preferences.json")
    } else {
        default_preferences.clone()
    };
    // Start the preview with the user's current choices, but keep test favorites
    // and hidden placeholders out of their normal preferences file.
    let initial_preferences = if options.test_extra_apps > 0
        && options.preferences.is_none()
        && !preferences_path.exists()
    {
        storage::load(&default_preferences)?
    } else {
        storage::load(&preferences_path)?
    };
    let mut state = State::new(initial_preferences);
    state.heroes = assets.heroes.clone();
    // The owner's profile pictures, prepared once (crate::avatars).
    let storage = preferences_path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    state.set_avatars(avatars::load(&storage));
    if options.test_extra_apps > 0 {
        eprintln!(
            "Luma scroll preview: explicit {}-app expansion; preferences {}",
            options.test_extra_apps,
            preferences_path.display()
        );
    }
    if webos && options.weather_test_server.is_some() {
        return Err("Weather test servers are only available in desktop builds".into());
    }
    let weather_cache = weather_service::cache_path(&preferences_path);
    state.weather.cache = weather_service::load_cache(&weather_cache);
    let weather_service = weather_service::Service::new(
        weather_service::Client::new(options.weather_test_server.as_deref())?,
        weather_cache,
    );
    let platform = Platform::new(webos, options.inventory.clone());
    let mut writer = storage::Writer::new(preferences_path.clone());
    effects(
        vec![Effect::Refresh],
        &mut state,
        &platform,
        &writer,
        &weather_service,
    );
    startup_stage("preferences and workers");

    let sdl = sdl2::init()?;
    let drivers: Vec<_> = sdl2::video::drivers().collect();
    if webos && !drivers.contains(&"wayland") {
        return Err(format!(
            "SDL has no Wayland backend (drivers: {}). Rebuild the package with the SDL-webOS runtime.",
            drivers.join(", ")
        ));
    }
    let video = sdl.video().map_err(|error| {
        format!(
            "SDL video initialization: {error}; drivers: {}; XDG_RUNTIME_DIR={:?}; WAYLAND_DISPLAY={:?}",
            drivers.join(", "),
            std::env::var_os("XDG_RUNTIME_DIR"),
            std::env::var_os("WAYLAND_DISPLAY"),
        )
    })?;
    eprintln!("Luma SDL video driver: {}", video.current_video_driver());
    startup_stage("SDL video");
    let attributes = video.gl_attr();
    if webos {
        attributes.set_context_profile(GLProfile::GLES);
        attributes.set_context_version(2, 0);
    } else {
        attributes.set_context_profile(GLProfile::Core);
        attributes.set_context_version(3, 2);
    }
    attributes.set_double_buffer(true);
    attributes.set_alpha_size(8);
    attributes.set_depth_size(0);
    // Stencil: opaque tiles mask out what lies behind them (renderer::find_occluders).
    attributes.set_stencil_size(8);
    let requested = options.resolution.unwrap_or(if webos {
        Size::HD
    } else {
        Size::new(1280, 720)
    });
    let create_window = || {
        let mut builder = video.window("Luma", requested.width, requested.height);
        builder.opengl().allow_highdpi().position_centered();
        if options.fullscreen || (webos && options.resolution.is_none()) {
            builder.fullscreen_desktop();
        } else if webos {
            builder.borderless();
        } else {
            builder.resizable();
        }
        let window = builder.build().map_err(|e| e.to_string())?;
        let context = window.gl_create_context()?;
        Ok::<_, String>((window, context))
    };
    let mut opened = None;
    let mut failures = Vec::new();
    // Shapes and text already blend their own edges (coverage in the shader),
    // so the TV draws without multisampling: on its one-core Mali, 4x MSAA
    // turned ~3 dropped frames per 1000 into ~14 while scrolling (3 runs of
    // 1000, tools/tv-bench.sh). The desktop keeps 4x; --msaa overrides both.
    let sample_choices: &[u8] = match options.msaa {
        None if webos => &[0],
        Some(4) | None => &[4, 2, 0],
        Some(2) => &[2, 0],
        Some(_) => &[0],
    };
    for &samples in sample_choices {
        attributes.set_multisample_buffers(u8::from(samples > 0));
        attributes.set_multisample_samples(samples);
        match create_window() {
            Ok(window) => {
                opened = Some(window);
                break;
            }
            Err(error) => failures.push(format!("{samples}x MSAA: {error}")),
        }
    }
    let (window, _context) = opened.ok_or_else(|| failures.join("; "))?;
    gl::load_with(|name| video.gl_get_proc_address(name) as *const _);
    if !webos {
        unsafe { gl::Enable(gl::MULTISAMPLE) };
    }
    let mut samples = 0;
    unsafe { gl::GetIntegerv(gl::SAMPLES, &mut samples) };
    eprintln!("Luma GL multisampling: {samples} samples");
    startup_stage("window and GL context");
    if let Err(error) = video.gl_set_swap_interval(1) {
        eprintln!("VSync: {error}");
    }
    let mut renderer =
        renderer::Renderer::new(&assets.root.join("font.ttf"), webos, options.medium_precision)?;
    // Decode every home image off the render thread at startup, wallpaper
    // first, so no icon pops in late and no slide stalls on a PNG decode.
    // Images decode on a background worker and upload a few per frame, so
    // the drawing thread never stalls on a PNG/JPEG decode. Requests come at
    // startup (wallpaper, tiles, Settings thumbnails) and once the app list
    // is known (each app's brand card).
    let astc = renderer.takes_astc();
    let (preload, preloaded) = {
        let (requests, queue) = std::sync::mpsc::channel::<String>();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            background_priority();
            let mut seen = std::collections::HashSet::new();
            for path in queue {
                if !seen.insert(path.clone()) {
                    continue;
                }
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
    };
    {
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
    renderer.skip = options.skip.clone();
    startup_stage("renderer");
    let mut last_counts = (0usize, 0usize);
    let mut profile = diagnostics::Profile::default();
    let mut presented = 0usize;
    let mut captured = None;
    let mut last_drawable = None;
    let mut events = sdl.event_pump()?;
    let mut press: Option<Press> = None;
    let mut last_minute = None;
    let mut frame = 0u64;
    let mut visible = true;
    let mut away = false;
    // LG's own Home coming to the front (an app was closed) is answered by
    // bringing Luma back, so the stock launcher never lingers. Push events when the hub allows them; otherwise poll while hidden.
    let watching = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // When the watcher raises Luma it posts this event: a hidden Luma sleeps
    // long, and webOS shows its window only once Luma answers, so the event
    // wakes the loop at once and keeps it brisk while the window comes back.
    // SAFETY: registering an SDL user event type has no preconditions.
    let wake_type = unsafe { sdl.event()?.register_event()? };
    let wake_sender = sdl.event()?.event_sender();
    let wake: platform::Wake = std::sync::Arc::new(move || {
        let _ = wake_sender.push_event(Event::User {
            timestamp: 0,
            window_id: 0,
            type_: wake_type,
            code: 0,
            data1: std::ptr::null_mut(),
            data2: std::ptr::null_mut(),
        });
    });
    // The Home key, seen as Luma's own launch (see `watch_launches`).
    // SAFETY: as above.
    let home_type = unsafe { sdl.event()?.register_event()? };
    let home_sender = sdl.event()?.event_sender();
    let home: platform::Wake = std::sync::Arc::new(move || {
        let _ = home_sender.push_event(Event::User {
            timestamp: 0,
            window_id: 0,
            type_: home_type,
            code: 0,
            data1: std::ptr::null_mut(),
            data2: std::ptr::null_mut(),
        });
    });
    let mut returning_until = Instant::now();
    let mut shown_at = Instant::now();
    // Crash guard: if Luma crashed twice within two minutes, this run does
    // not pull itself back over LG Home, so a broken build can never trap the
    // TV in a relaunch loop. A clean exit clears the record.
    let crash_guard = default_preferences.with_file_name("running.marker");
    let safe_mode = webos && crash_guard::begin(&crash_guard);
    if safe_mode {
        eprintln!("Luma safe mode: two recent crashes; not taking over Home this run");
    }
    if webos {
        platform::status_reset();
    }
    if webos && !safe_mode {
        match platform::watch_foreground(watching.clone(), wake.clone()) {
            Ok(()) => {
                watching.store(true, std::sync::atomic::Ordering::Relaxed);
                platform::status("foreground events: on");
            }
            Err(error) => platform::status(&format!("foreground events: {error}; polling instead")),
        }
    }
    if webos {
        match platform::watch_launches(home, !safe_mode) {
            Ok(()) => platform::status("launch events (Home key): on"),
            Err(error) => platform::status(&format!("launch events: {error}; Home only raises Luma")),
        }
        match platform::watch_running(platform.running.clone()) {
            Ok(()) => platform::status("running events: on"),
            Err(error) => platform::status(&format!("running events: {error}; asking on Back instead")),
        }
        if !safe_mode {
            match platform::watch_power() {
                Ok(()) => platform::status("power events (wake to Luma): on"),
                Err(error) => platform::status(&format!("power events: {error}")),
            }
        }
        if safe_mode {
            platform::status("safe mode: two recent crashes");
        }
    }
    // Developer flag: log every input event (stderr goes to /dev/null otherwise).
    let log_events = webos && std::path::Path::new("/tmp/luma-events").exists();
    let slow_log = webos && std::path::Path::new("/tmp/luma-debug").exists();
    let mut last_faults = debug::major_faults();
    let mut last_drawn: Vec<renderer::Draw> = Vec::new();
    #[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
    let egl = damage::Egl::current();
    #[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
    let mut damage_tracker = damage::Tracker::default();
    #[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
    eprintln!("Luma partial repaint: {}", if egl.is_some() { "on" } else { "off" });
    let mut last_presented = Instant::now();
    let mut was_animating = false;
    let mut last_move = Instant::now();
    let mut prefetched = false;
    let mut held_uploads: std::collections::VecDeque<(String, Preloaded)> = Default::default();
    // Back from an app: queue what the trim freed, after the first frame.
    let mut rewarm = false;
    let mut glyphs_warming = true;
    let mut last_refresh = Instant::now();
    let mut foreground_poll = Instant::now();
    let mut foreground_pending = false;
    let mut last_redirect: Option<Instant> = None;
    let mut running = true;
    let mut queued_event = None;
    let mut text_active = false;
    let mut initial_focus = options.script.is_empty();
    let scripted_input = !options.script.is_empty();
    let mut scene = compose(
        &mut state,
        &assets,
        options.time.unwrap_or_else(OffsetDateTime::now_utc),
        None,
    );
    while running {
        // A save that failed in the background is reported as before.
        while let Ok(error) = writer.errors.try_recv() {
            eprintln!("{error}");
            state.notify(i18n::t("savedError"));
        }
        let now = options
            .time
            .unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(local_offset));
        if last_minute != Some(now.unix_timestamp() / 60) {
            last_minute = Some(now.unix_timestamp() / 60);
            state.dirty = true;
        }
        state.tick();
        // Upload decoded images a few per tick (idle or not), so each is on
        // the GPU before it is first drawn and no frame pays for a batch.
        // Idle and on screen: warm a few text glyphs ahead of need.
        if glyphs_warming && visible && !state.dirty && !state.animating() {
            glyphs_warming = renderer.warm_glyphs(3);
        }
        // GPU uploads: on the TV one 1080p image takes the GPU tens of ms, so
        // while anything moves only one small image goes up per frame and
        // large ones wait for a still screen (four at once froze ~140 ms).
        let busy = visible && state.animating();
        let mut budget = if busy { 1 } else { 4 };
        while budget > 0 {
            let next = if busy { None } else { held_uploads.pop_front() }
                .or_else(|| preloaded.try_recv().ok());
            let Some((path, loaded)) = next else {
                break;
            };
            if busy && matches!(&loaded, Preloaded::Image(image) if image.width() * image.height() > 600_000) {
                held_uploads.push_back((path, loaded));
                continue;
            }
            match loaded {
                Preloaded::Image(image) => renderer.adopt(path, image),
                Preloaded::Compressed(image) => renderer.adopt_compressed(path, &image),
                Preloaded::Vector(bytes) => renderer.adopt_vector(path, &bytes),
            }
            budget -= 1;
        }
        state.dirty |= assets.artwork.poll();
        while let Ok(response) = platform.receiver.try_recv() {
            match response {
                Response::Inventory(Ok((mut inventory, warning))) => {
                    assets.artwork.refresh();
                    // Every app's icon is fetched in the background once the
                    // first frame is up (see below), never competing with it.
                    prefetched = false;
                    scroll_preview::append(&mut inventory, options.test_extra_apps);
                    state.set_inventory(inventory, warning);
                    if initial_focus
                        && state.focus == Target::Nav(0)
                        && state.page == state::Page::Apps
                    {
                        state.focus = state.first_content();
                    }
                    initial_focus = false;
                }
                Response::Profile(profile) => {
                    if state.profile != profile {
                        state.profile = profile;
                        state.dirty = true;
                    }
                }
                Response::Inventory(Err(error)) => {
                    state.loading = false;
                    state.error = Some(error);
                    state.dirty = true;
                    if initial_focus {
                        state.focus = state.first_content();
                        initial_focus = false;
                    }
                }
                Response::Running(ids) => state.open_switcher(platform::opened_only(ids, platform.webos)),
                Response::Closed(id, result) => {
                    if let Err(error) = result {
                        eprintln!("Luma close {id}: {error}");
                        state.notify(i18n::t("appCloseError"));
                    }
                }
                Response::Foreground(id) => {
                    foreground_pending = false;
                    if !visible
                        && id.as_deref() == Some(platform::HOME_ID)
                        && last_redirect.is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
                    {
                        last_redirect = Some(Instant::now());
                        platform.send(Request::Launch(platform::APP_ID.into()));
                    }
                }
                Response::Launched(result) => {
                    state.launching = false;
                    match result {
                        Err(error) => {
                            eprintln!("{error}");
                            state.notify(i18n::t("appOpenError"));
                        }
                        Ok(()) => {
                            state.notice = None;
                            state.dirty = true;
                        }
                    }
                }
            }
        }
        if webos
            && !watching.load(std::sync::atomic::Ordering::Relaxed)
            && !visible
            && !foreground_pending
            && foreground_poll.elapsed() >= Duration::from_millis(250)
        {
            foreground_pending = true;
            foreground_poll = Instant::now();
            platform.send(Request::Foreground);
        }
        weather_tick(&mut state, &weather_service, visible);
        // Rebuild hit regions before input whenever inventory/layout changed.
        if state.dirty {
            scene = compose(&mut state, &assets, now, Some(&scene));
        }
        // Raw SDL events are decoded through sdl_events: rust-sdl2's own
        // conversion transmutes TV-specific scancodes (undefined behaviour).
        let mut pending_events = Vec::new();
        while let Some(event) = sdl_events::poll(&mut events, webos) {
            pending_events.push(event);
        }
        for event in queued_event.take().into_iter().chain(pending_events) {
            // Window-entry pointer events can arrive after a slow first upload.
            // Bounded scripted captures own input until their final frame; only
            // their explicitly injected SDL clicks may alter pointer focus.
            let script_owns_input = scripted_input
                && (options.frames.is_some()
                    || options.profile_frames.is_some()
                    || !options.script.is_empty());
            let external_input = match &event {
                Event::MouseButtonDown { which, .. } | Event::MouseButtonUp { which, .. } => {
                    *which != script::SCRIPT_POINTER
                }
                Event::MouseMotion { .. }
                | Event::MouseWheel { .. }
                | Event::KeyDown { .. }
                | Event::KeyUp { .. }
                | Event::TextEditing { .. }
                | Event::TextInput { .. } => true,
                _ => false,
            };
            if script_owns_input && external_input {
                continue;
            }
            let mut pending = vec![];
            // Debug log only (stderr is /dev/null unless /tmp/luma-debug): which
            // key codes the TV remote really sends.
            if webos && log_events {
                if !matches!(event, Event::MouseMotion { .. }) {
                    eprintln!("Luma event: {event:?}");
                }
            } else if webos && let Event::KeyDown { keycode, scancode, repeat: false, .. } = &event {
                eprintln!(
                    "Luma key at {} ms: {keycode:?} ({:?}) scancode {scancode:?}",
                    startup.elapsed().as_millis(),
                    keycode.map(i32::from)
                );
            }
            match event {
                Event::Quit { .. } => running = false,
                Event::User { type_, .. } if type_ == wake_type => {
                    returning_until = Instant::now() + Duration::from_secs(2);
                }
                Event::User { type_, .. } if type_ == home_type => {
                    // The launch that brings Luma back from an app lands just
                    // after its window shows: only a launch well after that is
                    // Home pressed on Luma itself.
                    press = None;
                    let in_front = visible && shown_at.elapsed() > Duration::from_millis(800);
                    platform::status(if in_front { "home key: back to the top" } else { "home key: returned from an app" });
                    state.home_key(in_front);
                }
                Event::Window {
                    window_id,
                    win_event,
                    ..
                } if window_id == window.id() => match win_event {
                    WindowEvent::Hidden | WindowEvent::Minimized => {
                        visible = false;
                        save_order(&mut state, &writer);
                        away = true;
                        release_memory(&mut renderer);
                        press = None;
                        if matches!(state.overlay, Some(Overlay::SystemKeyboard)) {
                            state.finish_text_input();
                        }
                        profile.pause();
                    }
                    WindowEvent::FocusLost => {
                        press = None;
                        away = true;
                        if state.launching {
                            release_memory(&mut renderer);
                        }
                    }
                    WindowEvent::Shown | WindowEvent::Restored | WindowEvent::FocusGained => {
                        // One return fires all three events: act once, on the
                        // hidden-to-shown edge.
                        let returned = std::mem::take(&mut away) || !visible;
                        visible = true;
                        if returned {
                            shown_at = Instant::now();
                            rewarm = true;
                            // Every buffer is stale after being hidden.
                            last_drawn.clear();
                        }
                        state.dirty = true;
                        if returned {
                            // Catch installs and input changes, at most every
                            // 30 s rather than on every return.
                            if webos && last_refresh.elapsed() >= INVENTORY_FRESH {
                                last_refresh = Instant::now();
                                pending.push(Effect::Refresh);
                            }
                        }
                    }
                    WindowEvent::Resized(..)
                    | WindowEvent::SizeChanged(..)
                    | WindowEvent::Exposed => state.dirty = true,
                    _ => {}
                },
                Event::TextInput { text, .. } => state.text(&text),
                Event::TextEditing {
                    text,
                    start,
                    length,
                    ..
                } if text.is_empty() && matches!(state.overlay, Some(Overlay::SystemKeyboard)) => {
                    state.delete_surrounding_text(start, length);
                }
                Event::KeyDown {
                    keycode: Some(key),
                    repeat,
                    ..
                } => match key {
                    // A held arrow moves at a steady cadence: repeats that
                    // arrive faster are dropped, so focus glides evenly instead
                    // of jumping when two land in one frame.
                    Keycode::Up | Keycode::Down | Keycode::Left | Keycode::Right
                        if repeat && last_move.elapsed() < HELD_MOVE => {}
                    Keycode::Up | Keycode::Down | Keycode::Left | Keycode::Right => {
                        last_move = Instant::now();
                        let direction = match key {
                            Keycode::Up => Direction::Up,
                            Keycode::Down => Direction::Down,
                            Keycode::Left => Direction::Left,
                            _ => Direction::Right,
                        };
                        pending = state.direction(direction, &scene.controls);
                    }
                    Keycode::Return | Keycode::KpEnter
                        if matches!(state.overlay, Some(Overlay::SystemKeyboard)) =>
                    {
                        press = None;
                        state.finish_text_input();
                    }
                    Keycode::Return | Keycode::KpEnter if !repeat => {
                        press = Some(Press {
                            target: state.focus.clone(),
                            start: Instant::now(),
                            held: false,
                            pointer: false,
                        });
                    }
                    Keycode::Escape | Keycode::AcBack if !repeat => {
                        press = None;
                        pending = state.back();
                    }
                    Keycode::Backspace if state.text_input_active() => state.delete(),
                    Keycode::Backspace if !repeat => {
                        press = None;
                        pending = state.back();
                    }
                    Keycode::Clear if matches!(state.overlay, Some(Overlay::SystemKeyboard)) => {
                        let length = state.editing_text().len() as i32;
                        state.delete_surrounding_text(-length, length);
                    }
                    Keycode::M | Keycode::Menu if !repeat && state.overlay.is_none() => {
                        press = None;
                        state.options();
                    }
                    Keycode::F5 if !repeat => pending.push(Effect::Refresh),
                    key if !repeat
                        && state.overlay.is_none()
                        && state.switcher.is_none()
                        && !state.text_input_active()
                        && quick_slot(key).is_some() =>
                    {
                        press = None;
                        pending = state.quick_launch(quick_slot(key).unwrap_or(0));
                    }
                    key if !repeat && webos && i32::from(key) == WEBOS_KEY_RECENT => {
                        press = None;
                        pending = state.recent();
                    }
                    key if !repeat
                        && (i32::from(key) == 461
                            || (webos
                                && [WEBOS_BACK_KEYCODE, WEBOS_KEY_BACK, WEBOS_KEY_EXIT, sdl_events::WEBOS_EXIT_KEYCODE]
                                    .contains(&i32::from(key)))) =>
                    {
                        press = None;
                        pending = state.back();
                    }
                    _ => {}
                },
                Event::KeyUp {
                    keycode: Some(Keycode::Return | Keycode::KpEnter),
                    ..
                } => {
                    if let Some(held) = press.take()
                        && !held.pointer
                        && !held.held
                        && held.target == state.focus
                    {
                        pending = state.activate();
                    }
                }
                Event::MouseMotion { x, y, .. }
                    if state.moving.is_none()
                        && !matches!(state.overlay, Some(Overlay::SystemKeyboard)) =>
                {
                    let hovered = hit(&scene.controls, &window, x, y, &state);
                    if let Some(target) = hovered.clone() {
                        state.pointer_focus(target);
                    }
                    if press
                        .as_ref()
                        .is_some_and(|p| p.pointer && hovered.as_ref() != Some(&p.target))
                    {
                        press = None;
                    }
                }
                Event::MouseButtonDown {
                    mouse_btn, x, y, ..
                } => {
                    if let Some(target) = hit(&scene.controls, &window, x, y, &state)
                        && state.moving.is_none()
                    {
                        if matches!(state.overlay, Some(Overlay::SystemKeyboard)) {
                            state.finish_text_input();
                        }
                        // Pointer-down must not normalize scroll and move the clicked control.
                        state.pointer_focus(target);
                        if mouse_btn == MouseButton::Right {
                            state.options();
                            press = None;
                        } else if mouse_btn == MouseButton::Left {
                            press = Some(Press {
                                target: state.focus.clone(),
                                start: Instant::now(),
                                held: false,
                                pointer: true,
                            });
                        }
                    } else if mouse_btn == MouseButton::Left {
                        press = None;
                    }
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => match press.take() {
                    Some(held)
                        if held.pointer
                            && !held.held
                            && hit(&scene.controls, &window, x, y, &state).as_ref()
                                == Some(&held.target) =>
                    {
                        pending = state.activate()
                    }
                    None if scene.modal.is_some_and(|rect| {
                        pointer_position(&window, x, y).is_some_and(|(x, y)| !rect.contains(x, y))
                    }) =>
                    {
                        pending = state.back()
                    }
                    _ => {}
                },
                Event::MouseWheel {
                    x,
                    y,
                    mouse_x,
                    mouse_y,
                    ..
                } if state.overlay.is_none() => {
                    let over_favorites = state.page == state::Page::Home
                        && pointer_position(&window, mouse_x, mouse_y).is_some_and(|(x, y)| {
                            (64. ..1856.).contains(&x)
                                && scene.controls.iter().any(|control| {
                                    matches!(control.target, Target::App(_))
                                        && control.rect.contains(x, y)
                                })
                        });
                    if state.page == state::Page::Home && (x != 0 || over_favorites) {
                        state.scroll_favorites(
                            (if x != 0 { x } else { -y }) as f32 * state::HOME_TILE_PITCH,
                        );
                        press = None;
                    } else {
                        let max = scene
                            .controls
                            .iter()
                            .filter(|c| c.scrollable)
                            .map(|c| {
                                c.rect.y + c.rect.h + state.scroll
                                    - if state.page == state::Page::Home {
                                        1041.6
                                    } else {
                                        982.4
                                    }
                            })
                            .fold(0., f32::max);
                        state.scroll = (state.scroll - y as f32 * 90.).clamp(0., max.max(0.));
                    }
                    state.pointer = true;
                    state.dirty = true;
                }
                _ => {}
            }
            running &= effects(
                pending,
                &mut state,
                &platform,
                &writer,
                &weather_service,
            );
            if press
                .as_ref()
                .is_some_and(|p| !p.held && p.target != state.focus)
            {
                press = None;
            }
            let pressed = press.as_ref().filter(|p| !p.held).map(|p| p.target.clone());
            if state.pressed != pressed {
                state.pressed = pressed;
                state.dirty = true;
            }
            // SDL may deliver several navigation events in a single poll.
            if state.dirty {
                scene = compose(&mut state, &assets, now, Some(&scene));
            }
        }
        if let Some(held) = &mut press
            && !held.held
            && held.start.elapsed() >= Duration::from_millis(500)
        {
            held.held = true;
            state.pressed = None;
            state.hold();
        }
        if !state.loading
            && frame.is_multiple_of(3)
            && !options.script.front().is_some_and(|action| {
                (action == "await-search"
                    && (state.weather.searching || state.weather.search_pending()))
                    || (action == "await-weather" && state.weather.fetching())
            })
            && let Some(action) = options.script.pop_front()
        {
            if let Some(position) = action.strip_prefix("click:") {
                script::scripted_click(&sdl, &window, position)?;
            } else {
                let pending = script::script(&action, &mut state, &scene.controls)?;
                running &= effects(
                    pending,
                    &mut state,
                    &platform,
                    &writer,
                    &weather_service,
                );
            }
        }
        weather_tick(&mut state, &weather_service, visible);
        // SDL-webOS disables text input when its own panel is dismissed, even
        // if it did not forward a Back/Return key to the application.
        if text_active
            && matches!(state.overlay, Some(Overlay::SystemKeyboard))
            && !video.text_input().is_active()
        {
            press = None;
            state.finish_text_input();
        }
        // On the TV, text input summons LG's on-screen keyboard: only when that
        // keyboard is the one asked for, never over Luma's own (two keyboards
        // at once). On the desktop it carries a real keyboard's typing to both.
        let keyboard = if webos {
            matches!(state.overlay, Some(Overlay::SystemKeyboard))
        } else {
            state.text_input_active()
        };
        if keyboard != text_active {
            if keyboard {
                video.text_input().start();
            } else {
                video.text_input().stop();
            }
            text_active = keyboard;
        }
        // A profiling run ends after its frames, or after 90 s whatever
        // happens: hidden behind another app it presents nothing, and must
        // never linger beside the real Luma.
        let final_frame = options.frames.is_some_and(|limit| frame + 1 >= limit)
            || (visible
                && options
                    .profile_frames
                    .is_some_and(|limit| presented + 1 >= limit))
            || (options.profile_frames.is_some() && startup.elapsed() > Duration::from_secs(90));
        if ((state.dirty || options.profile_frames.is_some()) && visible) || final_frame {
            let composing = Instant::now();
            scene = compose(&mut state, &assets, now, Some(&scene));
            let compose_ms = composing.elapsed().as_secs_f64() * 1000.;
            let (width, height) = window.drawable_size();
            if width > 0 && height > 0 {
                renderer.damage = None;
                if final_frame && let Some(path) = &options.screenshot {
                    captured = Some(renderer.capture(
                        &scene.draw,
                        path,
                        options.capture_size.unwrap_or(Size::HD),
                    )?);
                }
                // Back from an app: the tiles freed while it was in front
                // decode again in the background, before any scroll reaches
                // them. Only what is missing is queued.
                if rewarm && presented > 0 {
                    rewarm = false;
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
                // After the first frame: fetch every app's icon in the
                // background, so no tile shows a placeholder on scrolling.
                if !prefetched && presented > 0 {
                    prefetched = true;
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
                // Repaint only what changed since the buffer was last drawn
                // (EGL partial update); anything uncertain repaints it all.
                #[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
                {
                    let mut region = None;
                    if let Some(egl) = &egl {
                        let change = if last_drawn.is_empty() {
                            None
                        } else {
                            damage::diff(&last_drawn, &scene.draw)
                        };
                        region = damage_tracker.region(change, egl.buffer_age());
                        if !egl.declare(region, width, height) {
                            region = None;
                        }
                        damage_tracker.presented(change);
                    }
                    renderer.damage = region;
                }
                renderer.render(&scene.draw, width, height);
                window.gl_swap_window();
                last_drawn.clone_from(&scene.draw);
                if presented == 0 {
                    startup_stage("first frame");
                    if webos {
                        let stats = renderer.stats();
                        eprintln!(
                            "Luma vector text: {} glyphs, {} text runs, {} KiB glyph geometry, {} KiB GPU text",
                            stats.cached_glyphs,
                            stats.cached_text_runs,
                            stats.glyph_geometry_bytes / 1024,
                            stats.text_geometry_bytes / 1024,
                        );
                    }
                }
                presented += 1;
                // TV debug log: every frame that misses the panel's refresh.
                if slow_log {
                    let now = Instant::now();
                    let interval = now.duration_since(last_presented).as_secs_f64() * 1000.;
                    // The first frame after an idle gap is not slow: Luma sat
                    // still (correctly) until the next key arrived.
                    // In Move every frame is logged, with the memory pages read
                    // back from swap since the last one, to tell a drawing
                    // stall from a paging one.
                    let slow = interval > 30. && interval < 1000. && state.animating() && was_animating;
                    if slow || state.moving.is_some() {
                        let stats = renderer.stats();
                        let faults = debug::major_faults();
                        eprintln!(
                            "Luma {} at {} ms: {interval:.1} ms (compose {compose_ms:.1}, cpu {:.1}, build {:.1}, draws {}, uploads {}, page faults +{}) page {:?} overlay {}",
                            if slow { "slow frame" } else { "move frame" },
                            startup.elapsed().as_millis(),
                            stats.cpu_submission_ms,
                            stats.cpu_build_ms,
                            stats.draw_calls,
                            stats.texture_uploads,
                            faults.saturating_sub(last_faults),
                            state.page,
                            state.overlay.is_some(),
                        );
                        last_faults = faults;
                    }
                    last_presented = now;
                    was_animating = state.animating();
                }
                if options.profile_frames.is_some() {
                    profile.record_split(
                        Instant::now(),
                        renderer.stats().cpu_submission_ms,
                        renderer.stats().cpu_build_ms,
                        compose_ms,
                        final_frame && options.screenshot.is_some(),
                    );
                    let stats = renderer.stats();
                    let (uploads, tessellations) = (stats.texture_uploads, stats.glyph_tessellations);
                    if let Some(interval) = profile.last_interval()
                        && interval > 30.
                    {
                        profile.note_slow(format!(
                            "frame {presented}: {interval:.1} ms, cpu {:.1}, uploads +{}, glyphs +{}, draws {}, scroll {:.0}/{:.0}, focus {:?}",
                            stats.cpu_submission_ms,
                            uploads - last_counts.0,
                            tessellations - last_counts.1,
                            stats.draw_calls,
                            state.scroll,
                            state.shown_scroll,
                            state.focus,
                        ));
                    }
                    last_counts = (uploads, tessellations);
                }
                let drawable = Size::new(width, height);
                if last_drawable != Some(drawable) {
                    if options.diagnostics.is_some() || options.resolution.is_some() || slow_log {
                        eprintln!(
                            "Luma drawable: {}x{}, density {:.3}x, 4K buffer: {}",
                            width,
                            height,
                            drawable.scale(),
                            drawable.is_4k()
                        );
                    }
                    last_drawable = Some(drawable);
                }
            }
            state.dirty = false;
        }
        frame += 1;
        if final_frame {
            running = false;
        }
        // Block until input instead of polling at frame rate on idle screens.
        // VSync paces animation; holds and scripted checks need short timed wakes.
        let timeout = if !visible {
            // Hidden behind an app: push events need no polling, so sleep long,
            // except while being raised (see `wake`): then answer webOS fast.
            if Instant::now() < returning_until {
                16
            } else if watching.load(std::sync::atomic::Ordering::Relaxed) {
                2000
            } else {
                250
            }
        } else if state.animating()
            || options.profile_frames.is_some()
        {
            0
        } else if press.is_some() || options.frames.is_some() {
            8
        } else {
            100
        };
        queued_event = sdl_events::wait(&mut events, timeout, webos);
    }
    save_order(&mut state, &writer);
    // Every save queued so far reaches the disk before Luma exits.
    writer.finish();
    crash_guard::end(&crash_guard);
    if text_active {
        video.text_input().stop();
    }
    if !options.script.is_empty() {
        return Err("Frame budget ended before the input script completed".into());
    }
    if let Some(path) = options.dump {
        let state = serde_json::json!({ "page": state.page, "focus": state.focus, "query": state.query,
            "preferences": state.preferences, "apps": state.apps().iter().map(|a| &a.id).collect::<Vec<_>>(),
            "error": state.error, "scroll": state.scroll, "home_scroll": state.home_scroll, "subpage": state.subpage, "modal": state.overlay.is_some(), "weather": state.weather,
            "inventory": state.inventory.apps,
            "ui_text": scene.draw.iter().filter_map(|draw| match draw { renderer::Draw::Text(value, ..) => Some(value), _ => None }).collect::<Vec<_>>() });
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(path) = options.diagnostics {
        let (width, height) = window.size();
        let (dw, dh) = window.drawable_size();
        let display_mode = window.display_index().ok().and_then(|index| video.current_display_mode(index).ok())
            .map(|mode| serde_json::json!({ "width": mode.w, "height": mode.h, "refresh_hz": mode.refresh_rate }));
        let report = serde_json::json!({
            "logical_layout": Size::HD, "requested_window": requested,
            "window": Size::new(width, height), "drawable": Size::new(dw, dh),
            "display_mode": display_mode,
            "drawable_is_4k": Size::new(dw, dh).is_4k(), "presented_frames": presented,
            "graphics": renderer.info, "last_presented": renderer.stats(), "capture": captured,
            "profile": options.profile_frames.map(|_| profile.summary()),
        });
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
