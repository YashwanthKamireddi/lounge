mod app_artwork;
mod avatars;
mod branding;
mod clock;
mod crash_guard;
mod damage;
mod debug;
mod diagnostics;
mod effects;
mod font;
mod i18n;
mod input;
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
mod luna;
mod model;
mod options;
mod platform;
mod preload;
mod presence;
mod raster;
mod renderer;
mod replies;
mod script;
#[path = "scroll-preview.rs"]
mod scroll_preview;
mod sdl_events;
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
mod window;

#[cfg(test)]
mod tests;

use effects::{effects, save_order};
use input::{Press, hit, pointer_position};
use options::Options;
use platform::{Platform, Request};
use raster::Size;
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
    mouse::MouseButton,
};
use state::{Effect, Overlay, State, Target};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

/// How long an inventory read stays fresh before a return re-reads it.
const INVENTORY_FRESH: Duration = Duration::from_secs(30);
fn main() {
    debug::debug_log();
    // stderr goes nowhere on the TV: how a run ends is kept in the status log.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        platform::status(&format!("panicked: {info}"));
        default_hook(info);
    }));
    if let Err(error) = run() {
        eprintln!("Lounge native: {error}");
        platform::status(&format!("stopped: {error}"));
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
    let options = Options::parse()?;
    let webos = cfg!(feature = "webos");
    if webos {
        platform::status_reset();
    }
    // On the TV each stage goes to the status log: how long a cold start
    // takes (at power-on Lounge always starts cold) is read there.
    let startup_stage = |stage: &str| {
        if webos {
            platform::status(&format!(
                "startup: {stage} at {} ms",
                startup.elapsed().as_millis()
            ));
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
        eprintln!("Lounge webOS surface: {surface_id}");
        sdl2::hint::set("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
        // SDL-webOS answers tvpower's screen saver requests with
        // `ack: !suspend_screensaver`, and SDL suspends the screen saver
        // unless this allows it: LG's screen saver never started while Lounge
        // was in front. A launcher has no video to keep on screen (upstream
        // Lounge b1dcbcf).
        sdl2::hint::set("SDL_VIDEO_ALLOW_SCREENSAVER", "1");
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
        storage::load_or_recover(&default_preferences)?
    } else {
        storage::load_or_recover(&preferences_path)?
    };
    let mut state = State::new(initial_preferences);
    state.heroes = assets.heroes.clone();
    // The owner's profile pictures, prepared once (crate::avatars).
    let storage = preferences_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    state.set_avatars(avatars::load(&storage));
    if options.test_extra_apps > 0 {
        eprintln!(
            "Lounge scroll preview: explicit {}-app expansion; preferences {}",
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
    let writer = storage::Writer::new(preferences_path.clone());
    effects(
        vec![Effect::Refresh],
        &mut state,
        &platform,
        &writer,
        &weather_service,
    );
    startup_stage("preferences and workers");

    let sdl = sdl2::init()?;
    let requested = options.resolution.unwrap_or(if webos {
        Size::HD
    } else {
        Size::new(1280, 720)
    });
    let (video, window, _context) = window::open(&sdl, &options, webos, requested, &startup_stage)?;
    if let Err(error) = video.gl_set_swap_interval(1) {
        eprintln!("VSync: {error}");
    }
    let mut renderer = renderer::Renderer::new(
        &assets.root.join("font.ttf"),
        webos,
        options.medium_precision,
    )?;
    // Decode every home image off the render thread at startup, wallpaper
    // first, so no icon pops in late and no slide stalls on a PNG decode.
    // Images decode on a background worker and upload a few per frame, so
    // the drawing thread never stalls on a PNG/JPEG decode. Requests come at
    // startup (wallpaper, tiles, Settings thumbnails) and once the app list
    // is known (each app's brand card).
    let (preload, preloaded) = preload::start(renderer.takes_astc());
    let uploads = preload::Uploads::new(preloaded);
    preload::request_home(&preload, &state, &assets);
    renderer.skip = options.skip.clone();
    startup_stage("renderer");
    let profile = diagnostics::Profile::default();
    let presented = 0usize;
    let captured = None;
    let events = sdl.event_pump()?;
    let remote = input::Remote::new();
    let last_minute = None;
    let frame = 0u64;
    let presence = presence::Presence::new();
    // LG's own Home coming to the front (an app was closed) is answered by
    // bringing Lounge back, so the stock launcher never lingers. Push events when the hub allows them; otherwise poll while hidden.
    let watching = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // When the watcher raises Lounge it posts this event: a hidden Lounge sleeps
    // long, and webOS shows its window only once Lounge answers, so the event
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
    // The Home key, seen as Lounge's own launch (see `watch_launches`).
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
    // Crash guard: if Lounge crashed twice within two minutes, this run does
    // not pull itself back over LG Home, so a broken build can never trap the
    // TV in a relaunch loop. A clean exit clears the record.
    let crash_guard = default_preferences.with_file_name("running.marker");
    let safe_mode = webos && crash_guard::begin(&crash_guard);
    let crash_guard_clear =
        webos.then(|| Instant::now() + Duration::from_secs(crash_guard::HEALTHY_SECONDS));
    if safe_mode {
        eprintln!("Lounge safe mode: two recent crashes; not taking over Home this run");
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
            Err(error) => {
                platform::status(&format!("launch events: {error}; Home only raises Lounge"))
            }
        }
        match platform::watch_running(platform.running.clone()) {
            Ok(()) => platform::status("running events: on"),
            Err(error) => {
                platform::status(&format!("running events: {error}; asking on Back instead"))
            }
        }
        if !safe_mode {
            match platform::watch_power() {
                Ok(()) => platform::status("power events (wake to Lounge): on"),
                Err(error) => platform::status(&format!("power events: {error}")),
            }
        }
        if safe_mode {
            platform::status("safe mode: two recent crashes");
        }
    }
    // Developer flag: log every input event (stderr goes to /dev/null otherwise).
    let log_events = webos && std::path::Path::new("/tmp/lounge-events").exists();
    let frame_log =
        diagnostics::FrameLog::new(webos && std::path::Path::new("/tmp/lounge-debug").exists());
    let partial = damage::Partial::new();
    // Back from an app: queue what the trim freed, after the first frame.
    let rewarm = false;
    let glyphs_warming = true;
    let last_refresh = Instant::now();
    let foreground_poll = Instant::now();
    let running = true;
    let queued_event = None;
    let text_active = false;
    // Replies from the TV services and the loop flags they drive.
    let replies = replies::Replies::new(options.script.is_empty());
    let scripted_input = !options.script.is_empty();
    let start = options
        .time
        .unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(local_offset));
    state.hour = start.hour();
    let scene = compose(&mut state, &assets, start, None);
    // A recording runs on a stepped clock: every frame is one 60th of a
    // second of the capture, however long it took to render.
    let record = match &options.record {
        Some(path) => {
            clock::record();
            Some(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        None => None,
    };
    Shell {
        startup,
        options,
        webos,
        local_offset,
        assets,
        state,
        weather_service,
        platform,
        writer,
        sdl,
        requested,
        video,
        window,
        _context,
        renderer,
        preload,
        uploads,
        profile,
        presented,
        captured,
        events,
        remote,
        last_minute,
        frame,
        presence,
        watching,
        wake_type,
        home_type,
        crash_guard,
        crash_guard_clear,
        log_events,
        frame_log,
        partial,
        rewarm,
        glyphs_warming,
        last_refresh,
        foreground_poll,
        running,
        queued_event,
        text_active,
        replies,
        scripted_input,
        scene,
        record,
        script_resume: None,
    }
    .run()
}

/// The running launcher: everything its event loop owns, from the window
/// and renderer to the loop's own counters and flags. `run` sets it up;
/// `Shell::run` turns events into frames until Lounge quits.
struct Shell {
    startup: Instant,
    options: Options,
    webos: bool,
    local_offset: time::UtcOffset,
    assets: view::Assets,
    state: State,
    weather_service: weather_service::Service,
    platform: Platform,
    writer: storage::Writer,
    requested: Size,
    renderer: renderer::Renderer,
    preload: std::sync::mpsc::Sender<String>,
    uploads: preload::Uploads,
    profile: diagnostics::Profile,
    presented: usize,
    captured: Option<renderer::RenderStats>,
    events: sdl2::EventPump,
    remote: input::Remote,
    last_minute: Option<i64>,
    frame: u64,
    presence: presence::Presence,
    watching: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake_type: u32,
    home_type: u32,
    crash_guard: std::path::PathBuf,
    crash_guard_clear: Option<Instant>,
    log_events: bool,
    frame_log: diagnostics::FrameLog,
    partial: damage::Partial,
    rewarm: bool,
    glyphs_warming: bool,
    last_refresh: Instant,
    foreground_poll: Instant,
    running: bool,
    queued_event: Option<Event>,
    text_active: bool,
    replies: replies::Replies,
    scripted_input: bool,
    scene: view::Scene,
    /// A recording's raw frames (`--record`), and when a scripted
    /// `pause:SECONDS` ends.
    record: Option<std::fs::File>,
    script_resume: Option<Instant>,
    // Last, in this order: fields drop top to bottom, and the renderer frees
    // GL objects as it drops, so the GL context, its window, the video
    // subsystem and SDL itself must outlive it.
    _context: sdl2::video::GLContext,
    window: sdl2::video::Window,
    video: sdl2::VideoSubsystem,
    sdl: sdl2::Sdl,
}

impl Shell {
    /// A startup stage, timed in the TV's status log.
    fn stage(&self, stage: &str) {
        if self.webos {
            platform::status(&format!(
                "startup: {stage} at {} ms",
                self.startup.elapsed().as_millis()
            ));
        }
    }

    /// The event loop: input, replies and timers in, frames out, until Lounge
    /// quits; then everything is saved and a scripted run's report written.
    fn run(mut self) -> Result<(), String> {
        while self.running {
            let now = self.tick();
            self.input(now);
            self.follow_up()?;
            let final_frame = self.final_frame();
            self.present(now, final_frame)?;
            self.frame += 1;
            if final_frame {
                self.running = false;
            }
            self.wait();
        }
        self.finish()
    }

    /// Background results, the clock, uploads, TV-service replies and
    /// timers, ahead of input; returns the time this pass is drawn for.
    fn tick(&mut self) -> OffsetDateTime {
        // A save that failed in the background is reported as before.
        while let Ok(error) = self.writer.errors.try_recv() {
            eprintln!("{error}");
            self.state.notify(i18n::t("savedError"));
        }
        let now = self
            .options
            .time
            .unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(self.local_offset));
        if self.last_minute != Some(now.unix_timestamp() / 60) {
            self.last_minute = Some(now.unix_timestamp() / 60);
            self.state.hour = now.hour();
            self.state.dirty = true;
        }
        self.state.tick();
        // Upload decoded images a few per tick (idle or not), so each is on
        // the GPU before it is first drawn and no frame pays for a batch.
        // Idle and on screen: warm a few text glyphs ahead of need.
        if self.glyphs_warming
            && self.presence.visible
            && !self.state.dirty
            && !self.state.animating()
        {
            self.glyphs_warming = self.renderer.warm_glyphs(3);
        }
        self.uploads.pump(
            &mut self.renderer,
            self.presence.visible && self.state.animating(),
        );
        self.state.dirty |= self.assets.artwork.poll();
        while let Ok(response) = self.platform.receiver.try_recv() {
            self.replies.apply(
                response,
                &mut self.state,
                &self.platform,
                &self.assets,
                self.presence.visible,
                self.options.test_extra_apps,
            );
        }
        if self.webos
            && !self.watching.load(std::sync::atomic::Ordering::Relaxed)
            && !self.presence.visible
            && !self.replies.foreground_pending
            && self.foreground_poll.elapsed() >= Duration::from_millis(250)
        {
            self.replies.foreground_pending = true;
            self.foreground_poll = Instant::now();
            self.platform.send(Request::Foreground);
        }
        weather_tick(
            &mut self.state,
            &self.weather_service,
            self.presence.visible,
        );
        // Rebuild hit regions before input whenever inventory/layout changed.
        if self.state.dirty {
            self.scene = compose(&mut self.state, &self.assets, now, Some(&self.scene));
        }
        now
    }

    /// Every event waiting, in arrival order.
    fn input(&mut self, now: OffsetDateTime) {
        // Raw SDL events are decoded through sdl_events: rust-sdl2's own
        // conversion transmutes TV-specific scancodes (undefined behaviour).
        let mut pending_events = Vec::new();
        while let Some(event) = sdl_events::poll(&mut self.events, self.webos) {
            pending_events.push(event);
        }
        for event in self.queued_event.take().into_iter().chain(pending_events) {
            self.event(event, now);
        }
    }

    /// One event: remote, pointer, window and TV-service wake-ups.
    fn event(&mut self, event: Event, now: OffsetDateTime) {
        // Window-entry pointer events can arrive after a slow first upload.
        // Bounded scripted captures own input until their final frame; only
        // their explicitly injected SDL clicks may alter pointer focus.
        let script_owns_input = self.scripted_input
            && (self.options.frames.is_some()
                || self.options.profile_frames.is_some()
                || !self.options.script.is_empty());
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
            return;
        }
        let mut pending = vec![];
        // Debug log only (stderr is /dev/null unless /tmp/lounge-debug): which
        // key codes the TV remote really sends.
        if self.webos && self.log_events {
            if !matches!(event, Event::MouseMotion { .. }) {
                eprintln!("Lounge event: {event:?}");
            }
        } else if self.webos
            && let Event::KeyDown {
                keycode,
                scancode,
                repeat: false,
                ..
            } = &event
        {
            eprintln!(
                "Lounge key at {} ms: {keycode:?} ({:?}) scancode {scancode:?}",
                self.startup.elapsed().as_millis(),
                keycode.map(i32::from)
            );
        }
        match event {
            // The TV turning off asks every app to quit: Lounge stays,
            // hidden, so it is there to take the screen at power-on.
            Event::Quit { .. }
                if self.webos && platform::STANDBY.load(std::sync::atomic::Ordering::Relaxed) =>
            {
                platform::status("quit asked while turning off: staying resident for power-on");
                // Healthy until the TV turned off: webOS replaces this
                // process at power-on, which must not count as a crash.
                crash_guard::end(&self.crash_guard);
                self.crash_guard_clear = None;
            }
            Event::Quit { .. } => {
                if self.webos {
                    platform::status("quit asked: exiting");
                }
                self.running = false;
            }
            Event::User { type_, .. } if type_ == self.wake_type => {
                self.presence.raise();
            }
            Event::User { type_, .. } if type_ == self.home_type => {
                // The launch that brings Lounge back from an app lands just
                // after its window shows: only a launch well after that is
                // Home pressed on Lounge itself.
                self.remote.press = None;
                // webOS's relaunch reaches SDL just after this event: hidden,
                // the loop would sleep up to 2 s before pumping it.
                self.presence.raise();
                let in_front = self.presence.settled();
                platform::status(if in_front {
                    "home key: back to the top"
                } else {
                    "home key: returned from an app"
                });
                self.state.home_key(in_front);
            }
            Event::Window {
                window_id,
                win_event,
                ..
            } if window_id == self.window.id() => match win_event {
                WindowEvent::Hidden | WindowEvent::Minimized => {
                    self.presence.hide();
                    save_order(&mut self.state, &self.writer);
                    release_memory(&mut self.renderer);
                    self.remote.press = None;
                    if matches!(self.state.overlay, Some(Overlay::SystemKeyboard)) {
                        self.state.finish_text_input();
                    }
                    self.profile.pause();
                }
                WindowEvent::FocusLost => {
                    self.remote.press = None;
                    self.presence.leave(self.state.launching);
                    if self.state.launching {
                        release_memory(&mut self.renderer);
                    }
                }
                WindowEvent::Shown | WindowEvent::Restored | WindowEvent::FocusGained => {
                    // One return fires all three events: act once, on the
                    // hidden-to-shown edge.
                    let returned = self.presence.show();
                    if returned {
                        self.rewarm = true;
                        // Every buffer is stale after being hidden.
                        self.partial.invalidate();
                    }
                    self.state.dirty = true;
                    if returned {
                        // Catch installs and input changes, at most every
                        // 30 s rather than on every return.
                        if self.webos && self.last_refresh.elapsed() >= INVENTORY_FRESH {
                            self.last_refresh = Instant::now();
                            pending.push(Effect::Refresh);
                        }
                    }
                }
                WindowEvent::Resized(..) | WindowEvent::SizeChanged(..) | WindowEvent::Exposed => {
                    self.state.dirty = true
                }
                _ => {}
            },
            Event::TextInput { text, .. } => self.state.text(&text),
            Event::TextEditing {
                text,
                start,
                length,
                ..
            } if text.is_empty() && matches!(self.state.overlay, Some(Overlay::SystemKeyboard)) => {
                self.state.delete_surrounding_text(start, length);
            }
            Event::KeyDown {
                keycode: Some(key),
                repeat,
                ..
            } => {
                pending = self.remote.key_down(
                    key,
                    repeat,
                    self.webos,
                    &mut self.state,
                    &self.scene.controls,
                )
            }
            Event::KeyUp {
                keycode: Some(Keycode::Return | Keycode::KpEnter),
                ..
            } => pending = self.remote.enter_up(&mut self.state),
            Event::MouseMotion { x, y, .. }
                if self.state.moving.is_none()
                    && !matches!(self.state.overlay, Some(Overlay::SystemKeyboard)) =>
            {
                let hovered = hit(&self.scene.controls, &self.window, x, y, &self.state);
                if let Some(target) = hovered.clone() {
                    self.state.pointer_focus(target);
                }
                if self
                    .remote
                    .press
                    .as_ref()
                    .is_some_and(|p| p.pointer && hovered.as_ref() != Some(&p.target))
                {
                    self.remote.press = None;
                }
            }
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                if let Some(target) = hit(&self.scene.controls, &self.window, x, y, &self.state)
                    && self.state.moving.is_none()
                {
                    if matches!(self.state.overlay, Some(Overlay::SystemKeyboard)) {
                        self.state.finish_text_input();
                    }
                    // Pointer-down must not normalize scroll and move the clicked control.
                    self.state.pointer_focus(target);
                    if mouse_btn == MouseButton::Right {
                        self.state.options();
                        self.remote.press = None;
                    } else if mouse_btn == MouseButton::Left {
                        self.remote.press = Some(Press {
                            target: self.state.focus.clone(),
                            start: Instant::now(),
                            held: false,
                            pointer: true,
                        });
                    }
                } else if mouse_btn == MouseButton::Left {
                    self.remote.press = None;
                }
            }
            Event::MouseButtonUp {
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            } => match self.remote.press.take() {
                Some(held)
                    if held.pointer
                        && !held.held
                        && hit(&self.scene.controls, &self.window, x, y, &self.state).as_ref()
                            == Some(&held.target) =>
                {
                    pending = self.state.activate()
                }
                None if self.scene.modal.is_some_and(|rect| {
                    pointer_position(&self.window, x, y).is_some_and(|(x, y)| !rect.contains(x, y))
                }) =>
                {
                    pending = self.state.back()
                }
                _ => {}
            },
            Event::MouseWheel {
                x,
                y,
                mouse_x,
                mouse_y,
                ..
            } if self.state.overlay.is_none() => {
                let over_favorites = self.state.page == state::Page::Home
                    && pointer_position(&self.window, mouse_x, mouse_y).is_some_and(|(x, y)| {
                        (64. ..1856.).contains(&x)
                            && self.scene.controls.iter().any(|control| {
                                matches!(control.target, Target::App(_))
                                    && control.rect.contains(x, y)
                            })
                    });
                if self.state.page == state::Page::Home && (x != 0 || over_favorites) {
                    self.state.scroll_favorites(
                        (if x != 0 { x } else { -y }) as f32 * state::HOME_TILE_PITCH,
                    );
                    self.remote.press = None;
                } else {
                    let max = self
                        .scene
                        .controls
                        .iter()
                        .filter(|c| c.scrollable)
                        .map(|c| {
                            c.rect.y + c.rect.h + self.state.scroll
                                - if self.state.page == state::Page::Home {
                                    1041.6
                                } else {
                                    982.4
                                }
                        })
                        .fold(0., f32::max);
                    self.state.scroll = (self.state.scroll - y as f32 * 90.).clamp(0., max.max(0.));
                }
                self.state.pointer = true;
                self.state.dirty = true;
            }
            _ => {}
        }
        self.running &= effects(
            pending,
            &mut self.state,
            &self.platform,
            &self.writer,
            &self.weather_service,
        );
        if self
            .remote
            .press
            .as_ref()
            .is_some_and(|p| !p.held && p.target != self.state.focus)
        {
            self.remote.press = None;
        }
        let pressed = self
            .remote
            .press
            .as_ref()
            .filter(|p| !p.held)
            .map(|p| p.target.clone());
        if self.state.pressed != pressed {
            self.state.pressed = pressed;
            self.state.dirty = true;
        }
        // SDL may deliver several navigation events in a single poll.
        if self.state.dirty {
            self.scene = compose(&mut self.state, &self.assets, now, Some(&self.scene));
        }
    }

    /// After input: OK-holds, scripted steps, the crash record, inventory
    /// retries, weather, and the on-screen keyboard.
    fn follow_up(&mut self) -> Result<(), String> {
        if let Some(held) = &mut self.remote.press
            && !held.held
            && held.start.elapsed() >= Duration::from_millis(500)
        {
            held.held = true;
            self.state.pressed = None;
            self.state.hold();
        }
        if !self.state.loading
            && self.frame.is_multiple_of(3)
            && self.script_resume.is_none_or(|at| clock::now() >= at)
            && !self.options.script.front().is_some_and(|action| {
                (action == "await-search"
                    && (self.state.weather.searching || self.state.weather.search_pending()))
                    || (action == "await-weather" && self.state.weather.fetching())
            })
            && let Some(action) = self.options.script.pop_front()
        {
            if let Some(seconds) = action.strip_prefix("pause:") {
                let seconds: f32 = seconds
                    .parse()
                    .map_err(|_| format!("Invalid pause: {action}"))?;
                self.script_resume = Some(clock::now() + Duration::from_secs_f32(seconds));
            } else if let Some(position) = action.strip_prefix("click:") {
                script::scripted_click(&self.sdl, &self.window, position)?;
            } else {
                let pending = script::script(&action, &mut self.state, &self.scene.controls)?;
                self.running &= effects(
                    pending,
                    &mut self.state,
                    &self.platform,
                    &self.writer,
                    &self.weather_service,
                );
            }
        }
        if self
            .crash_guard_clear
            .is_some_and(|at| Instant::now() >= at)
        {
            self.crash_guard_clear = None;
            crash_guard::end(&self.crash_guard);
        }
        // state.loading stays set meanwhile, so focus and relaunch refreshes
        // add no requests of their own.
        if self
            .replies
            .inventory_retry
            .is_some_and(|at| Instant::now() >= at)
        {
            self.replies.inventory_retry = None;
            self.platform.send(Request::Refresh);
        }
        weather_tick(
            &mut self.state,
            &self.weather_service,
            self.presence.visible,
        );
        // SDL-webOS disables text input when its own panel is dismissed, even
        // if it did not forward a Back/Return key to the application.
        if self.text_active
            && matches!(self.state.overlay, Some(Overlay::SystemKeyboard))
            && !self.video.text_input().is_active()
        {
            self.remote.press = None;
            self.state.finish_text_input();
        }
        // On the TV, text input summons LG's on-screen keyboard: only when that
        // keyboard is the one asked for, never over Lounge's own (two keyboards
        // at once). On the desktop it carries a real keyboard's typing to both.
        let keyboard = if self.webos {
            matches!(self.state.overlay, Some(Overlay::SystemKeyboard))
        } else {
            self.state.text_input_active()
        };
        if keyboard != self.text_active {
            if keyboard {
                self.video.text_input().start();
            } else {
                self.video.text_input().stop();
            }
            self.text_active = keyboard;
        }
        Ok(())
    }

    /// A profiling run ends after its frames, or after 90 s whatever
    /// happens: hidden behind another app it presents nothing, and must
    /// never linger beside the real Lounge.
    fn final_frame(&self) -> bool {
        self.options
            .frames
            .is_some_and(|limit| self.frame + 1 >= limit)
            || (self.presence.visible
                && self
                    .options
                    .profile_frames
                    .is_some_and(|limit| self.presented + 1 >= limit))
            || (self.options.profile_frames.is_some()
                && self.startup.elapsed() > Duration::from_secs(90))
            || (self.record.is_some()
                && self.options.script.is_empty()
                && self.script_resume.is_none_or(|at| clock::now() >= at))
    }

    /// Draws and presents a frame when anything changed.
    fn present(&mut self, now: OffsetDateTime, final_frame: bool) -> Result<(), String> {
        if ((self.state.dirty || self.options.profile_frames.is_some() || self.record.is_some())
            && self.presence.visible)
            || final_frame
        {
            let composing = Instant::now();
            self.scene = compose(&mut self.state, &self.assets, now, Some(&self.scene));
            let compose_ms = composing.elapsed().as_secs_f64() * 1000.;
            let (width, height) = self.window.drawable_size();
            // A capture renders off-screen at its own size: taken even when
            // the window reports no drawable yet (a headless compositor can,
            // for a frame), so a scripted run always has its screenshot.
            if final_frame && let Some(path) = &self.options.screenshot {
                self.renderer.damage = None;
                self.captured = Some(self.renderer.capture(
                    &self.scene.draw,
                    path,
                    self.options.capture_size.unwrap_or(Size::HD),
                )?);
            }
            if let Some(record) = &mut self.record {
                use std::io::Write;
                self.renderer.damage = None;
                let (pixels, _) = self.renderer.capture_pixels(
                    &self.scene.draw,
                    self.options.capture_size.unwrap_or(Size::HD),
                )?;
                record
                    .write_all(&pixels)
                    .map_err(|e| format!("Recording: {e}"))?;
                clock::step(Duration::from_secs(1) / 60);
            }
            if width > 0 && height > 0 {
                self.renderer.damage = None;
                if self.rewarm && self.presented > 0 {
                    self.rewarm = false;
                    preload::refill(&self.preload, &self.state, &self.assets, &self.renderer);
                }
                if !self.replies.prefetched && self.presented > 0 {
                    self.replies.prefetched = true;
                    preload::prefetch(&self.preload, &self.state, &self.assets);
                }
                // Repaint only what changed since the buffer was last drawn
                // (EGL partial update); anything uncertain repaints it all.
                self.renderer.damage = self.partial.region(&self.scene.draw, width, height);
                self.renderer.render(&self.scene.draw, width, height);
                self.window.gl_swap_window();
                self.partial.drawn(&self.scene.draw);
                if self.presented == 0 {
                    self.stage("first frame");
                    if self.webos {
                        let stats = self.renderer.stats();
                        eprintln!(
                            "Lounge vector text: {} glyphs, {} text runs, {} KiB glyph geometry, {} KiB GPU text",
                            stats.cached_glyphs,
                            stats.cached_text_runs,
                            stats.glyph_geometry_bytes / 1024,
                            stats.text_geometry_bytes / 1024,
                        );
                    }
                }
                self.presented += 1;
                self.frame_log.presented(
                    &self.state,
                    self.renderer.stats(),
                    self.options
                        .profile_frames
                        .is_some()
                        .then_some(&mut self.profile),
                    diagnostics::Frame {
                        number: self.presented,
                        compose_ms,
                        exclude: final_frame && self.options.screenshot.is_some(),
                        width,
                        height,
                        uptime: self.startup.elapsed(),
                        verbose: self.options.diagnostics.is_some()
                            || self.options.resolution.is_some(),
                    },
                );
            }
            self.state.dirty = false;
        }
        Ok(())
    }

    /// Block until input instead of polling at frame rate on idle screens.
    /// VSync paces animation; holds and scripted checks need short timed wakes.
    fn wait(&mut self) {
        let timeout = if self.record.is_some() {
            0
        } else if !self.presence.visible {
            // Hidden behind an app: push events need no polling, so sleep long,
            // except while being raised (see `wake`): then answer webOS fast.
            if self.presence.raising() {
                16
            } else if self.watching.load(std::sync::atomic::Ordering::Relaxed) {
                2000
            } else {
                250
            }
        } else if self.state.animating() || self.options.profile_frames.is_some() {
            0
        } else if self.remote.press.is_some() || self.options.frames.is_some() {
            8
        } else {
            100
        };
        self.queued_event = sdl_events::wait(&mut self.events, timeout, self.webos);
    }

    /// Saves, stops text input and writes a scripted run's reports.
    fn finish(mut self) -> Result<(), String> {
        save_order(&mut self.state, &self.writer);
        // Every save queued so far reaches the disk before Lounge exits.
        self.writer.finish();
        crash_guard::end(&self.crash_guard);
        if self.text_active {
            self.video.text_input().stop();
        }
        if !self.options.script.is_empty() {
            return Err("Frame budget ended before the input script completed".into());
        }
        if let Some(path) = self.options.dump {
            let dump = serde_json::json!({ "page": self.state.page, "focus": self.state.focus, "query": self.state.query,
                "preferences": self.state.preferences, "apps": self.state.apps().iter().map(|a| &a.id).collect::<Vec<_>>(),
                "error": self.state.error, "scroll": self.state.scroll, "home_scroll": self.state.home_scroll, "subpage": self.state.subpage, "modal": self.state.overlay.is_some(), "weather": self.state.weather,
                "inventory": self.state.inventory.apps,
                "ui_text": self.scene.draw.iter().filter_map(|draw| match draw { renderer::Draw::Text(value, ..) => Some(value), _ => None }).collect::<Vec<_>>() });
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&dump).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(path) = self.options.diagnostics {
            let (width, height) = self.window.size();
            let (dw, dh) = self.window.drawable_size();
            let display_mode = self.window.display_index().ok().and_then(|index| self.video.current_display_mode(index).ok())
                .map(|mode| serde_json::json!({ "width": mode.w, "height": mode.h, "refresh_hz": mode.refresh_rate }));
            let report = serde_json::json!({
                "logical_layout": Size::HD, "requested_window": self.requested,
                "window": Size::new(width, height), "drawable": Size::new(dw, dh),
                "display_mode": display_mode,
                "drawable_is_4k": Size::new(dw, dh).is_4k(), "presented_frames": self.presented,
                "graphics": self.renderer.info, "last_presented": self.renderer.stats(), "capture": self.captured,
                "profile": self.options.profile_frames.map(|_| self.profile.summary()),
            });
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
