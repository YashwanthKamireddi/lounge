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
mod snapshots;
mod state;
mod storage;
mod time_format;
mod uninstall;
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
use input::{hit, pointer_position};
use options::Options;
use platform::{Platform, Request};
use raster::Size;
use sdl2::{
    event::{Event, WindowEvent},
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
/// The code of the wake-up that says webOS's list of apps changed.
const APPS_CHANGED: i32 = 1;
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
    // A new page comes in from this very frame, out of exactly what the last
    // frame showed.
    if let Some(previous) = previous
        && let Some((page, range)) = &previous.page
        && *page != state.page
    {
        state.page_ghost = state
            .page_since
            .map(|_| previous.draw[range.clone()].to_vec());
    }
    // Search closing fades out exactly what it showed last.
    if let Some(previous) = previous
        && let Some(range) = &previous.search
        && state.search_closed.is_some()
        && !matches!(state.overlay, Some(Overlay::Keyboard))
    {
        state.search_ghost = Some(previous.draw[range.clone()].to_vec());
    }
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

/// SDL's clock, the one it stamps input events with.
fn ticks() -> u32 {
    // SAFETY: SDL is initialised before the loop that calls this.
    unsafe { sdl2::sys::SDL_GetTicks() }
}

fn release_memory(renderer: &mut renderer::Renderer, preload: &mut preload::Preload) {
    // Keep what the home needs to come back instantly (its textures, glyphs
    // and decoded icons); free only what it does not show.
    renderer.trim();
    // Nothing more decodes or goes up behind the app, and what the trim freed
    // decodes again when it is asked for on return.
    preload.behind();
    preload.forget();
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

/// Lounge's start as the TV's Home, or a normal start that opens that Home instead. Some(result)
/// ends this run before any window exists.
/// As Home: mounts the updated executable in LG Home's place and becomes it (the same
/// process, so webOS sees nothing change). Only at start, before a window or LS2 connection
/// exists. Returns only if that failed.
fn restart_updated_home() {
    match platform::refresh_home_mount() {
        Ok(()) => {
            use std::os::unix::process::CommandExt;
            platform::status("home: a package update replaced Lounge; starting the new one");
            let error = std::process::Command::new(platform::LG_HOME_EXE)
                .args(std::env::args_os().skip(1))
                .env("LOUNGE_HOME_RESTARTED", "1")
                .exec();
            platform::status(&format!("home: could not start the new Lounge: {error}"));
        }
        Err(error) => platform::status(&format!("home: renewing the mount failed: {error}")),
    }
}

fn home_start(options: &Options) -> Option<Result<(), String>> {
    if !platform::is_home() {
        return platform::redirect_to_home().then_some(Ok(()));
    }
    platform::status(&format!(
        "as the TV's Home (uid {}), launch: {}",
        platform::user_id(),
        options.launch.as_deref().unwrap_or("none")
    ));
    // A package update replaced the executable the mount holds: mount the new one and start
    // again from it, once (the same process, so webOS sees nothing change).
    if platform::home_mount_stale() && std::env::var_os("LOUNGE_HOME_RESTARTED").is_none() {
        restart_updated_home();
    }
    if crash_guard::begin(std::path::Path::new(crash_guard::HOME_MARKER)) {
        platform::status("home: started 3 times in a minute without coming up; LG Home is back");
        if let Err(error) = platform::restore_lg_home() {
            platform::status(&format!("home: restoring LG Home failed: {error}"));
        }
        return Some(Ok(()));
    }
    // sam's own background start of Home (at boot, say) never takes the screen from an
    // input or app: Home starts when it is asked for.
    if options.background && !platform::home_in_front() {
        platform::status("home: started in the background with something else in front; exiting");
        crash_guard::end(std::path::Path::new(crash_guard::HOME_MARKER));
        return Some(Ok(()));
    }
    None
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
        if let Some(home) = home_start(&options) {
            return home;
        }
        let surface_id = platform::own_id();
        // SDL-webOS reads APPID for its shell surface and getenv (not a hint)
        // for WMCLASS. Set these before video init and before starting workers.
        // SAFETY: startup is still single-threaded here.
        unsafe {
            std::env::set_var("APPID", surface_id);
            std::env::set_var("SDL_VIDEO_WAYLAND_WMCLASS", surface_id);
        }
        eprintln!("Lounge webOS surface: {surface_id}");
        sdl2::hint::set("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
        // A long Back on the Magic Remote sends the separate Exit key. Unclaimed, the
        // compositor handles it by closing the app in front, which is Lounge itself: Home
        // then started cold, its place lost. Claimed, it reaches Lounge as Back.
        sdl2::hint::set("SDL_WEBOS_ACCESS_POLICY_KEYS_EXIT", "true");
        // SDL-webOS answers tvpower's screen saver requests with
        // `ack: !suspend_screensaver`, and SDL suspends the screen saver
        // unless this allows it: LG's screen saver never started while Lounge
        // was in front. A launcher has no video to keep on screen (upstream
        // Lounge b1dcbcf).
        sdl2::hint::set("SDL_VIDEO_ALLOW_SCREENSAVER", "1");
        // As Home, Home pressed on Lounge comes to Lounge as a key, as it came to LG's Home.
        if platform::is_home() {
            sdl2::hint::set("SDL_WEBOS_ACCESS_POLICY_KEYS_HOME", "true");
            // Lounge registers with sam itself (`platform::register_home`).
            sdl2::hint::set("SDL_WEBOS_REGISTER_APP", "0");
        }
    }
    // Resolve the local offset before artwork, HTTP or platform workers start.
    let local_offset = OffsetDateTime::now_local()
        .map(|now| now.offset())
        .unwrap_or(time::UtcOffset::UTC);
    let root = match options.assets.clone() {
        Some(root) => root,
        None if webos => platform::install_dir()?.join("assets"),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".build/assets"),
    };
    let assets = view::Assets::load(root, webos)?;
    // The image decoder starts now, so the home's images decode while SDL,
    // EGL and the shaders start (uploads still wait for the renderer).
    let mut preload = preload::Preload::start();
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
    preload::request_home(&mut preload, &state, &assets);
    // The owner's profile pictures, prepared once (crate::avatars). A changed
    // picture is decoded (up to 4096²), scaled and blurred: on a worker, off
    // the way to the first frame; the picture shows when ready. Captures and
    // scripted runs wait for it, so every frame they record is the same.
    let storage = preferences_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let avatars = if options.frames.is_some()
        || options.record.is_some()
        || options.screenshot.is_some()
        || !options.script.is_empty()
    {
        state.set_avatars(avatars::load(&storage));
        None
    } else {
        Some(avatars::load_in_background(storage))
    };
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
    // The switcher's snapshots and the TV's clock format, off this thread.
    let snapshots = snapshots::Service::new(webos, assets.root.clone());
    time_format::watch();
    time_format::watch_time();
    let writer = storage::Writer::new(preferences_path.clone());
    effects(
        if webos {
            vec![Effect::Refresh, Effect::TvHome(None)]
        } else {
            vec![Effect::Refresh]
        },
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
    // The home's images have been decoding since the assets loaded; those
    // with an ASTC copy waited for this answer.
    preload.set_astc(renderer.takes_astc());
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
    // sam's lifecycle events for Lounge as Home (see `platform::register_home`).
    let mut lifecycle = None;
    // The Home key, seen as Lounge's own launch (see `watch_launches`).
    // SAFETY: registering an SDL user event type has no preconditions.
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
    // webOS changed its list of apps (an install, update or removal): read it again now.
    let apps_sender = sdl.event()?.event_sender();
    let apps_changed: platform::Wake = std::sync::Arc::new(move || {
        let _ = apps_sender.push_event(Event::User {
            timestamp: 0,
            window_id: 0,
            type_: home_type,
            code: APPS_CHANGED,
            data1: std::ptr::null_mut(),
            data2: std::ptr::null_mut(),
        });
    });
    if webos && platform::is_home() {
        // SAFETY: as above.
        let lifecycle_type = unsafe { sdl.event()?.register_event()? };
        let sender = sdl.event()?.event_sender();
        let registered = platform::register_home(move |event| {
            let _ = sender.push_event(Event::User {
                timestamp: 0,
                window_id: 0,
                type_: lifecycle_type,
                code: match event {
                    platform::Lifecycle::Relaunch => 1,
                    platform::Lifecycle::Close => 2,
                },
                data1: std::ptr::null_mut(),
                data2: std::ptr::null_mut(),
            });
        });
        match registered {
            Ok(()) => {
                lifecycle = Some(lifecycle_type);
                platform::status("home lifecycle (registerApp): on");
            }
            Err(error) => platform::status(&format!("home lifecycle: {error}")),
        }
    }
    if webos {
        match platform::watch_opened() {
            Ok(()) => platform::status("foreground events (opened apps): on"),
            Err(error) => platform::status(&format!("foreground events: {error}")),
        }
        match platform::watch_launches(home) {
            Ok(()) => platform::status("launch events (Home key): on"),
            Err(error) => platform::status(&format!("launch events: {error}")),
        }
        match platform::watch_apps(apps_changed) {
            Ok(()) => platform::status("app list events: on"),
            Err(error) => platform::status(&format!("app list events: {error}")),
        }
        match platform::watch_running(platform.running.clone()) {
            Ok(()) => platform::status("running events: on"),
            Err(error) => {
                platform::status(&format!("running events: {error}; asking on Back instead"))
            }
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
        snapshots,
        writer,
        sdl,
        requested,
        video,
        window,
        _context,
        renderer,
        preload,
        avatars,
        profile,
        presented,
        captured,
        events,
        remote,
        last_minute,
        frame,
        presence,
        home_type,
        lifecycle,
        healthy_at: None,
        log_events,
        frame_log,
        partial,
        rewarm,
        glyphs_warming,
        last_refresh,
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
    snapshots: snapshots::Service,
    writer: storage::Writer,
    requested: Size,
    renderer: renderer::Renderer,
    preload: preload::Preload,
    /// The profile pictures while their worker prepares them.
    avatars: Option<std::sync::mpsc::Receiver<Vec<avatars::Picture>>>,
    profile: diagnostics::Profile,
    presented: usize,
    captured: Option<renderer::RenderStats>,
    events: sdl2::EventPump,
    remote: input::Remote,
    last_minute: Option<i64>,
    frame: u64,
    presence: presence::Presence,
    home_type: u32,
    /// The SDL event type sam's lifecycle events arrive as, as Home.
    lifecycle: Option<u32>,
    /// As Home: when this run counts as healthy (see `crash_guard`), once its first frame is up.
    healthy_at: Option<Instant>,
    log_events: bool,
    frame_log: diagnostics::FrameLog,
    partial: damage::Partial,
    rewarm: bool,
    glyphs_warming: bool,
    last_refresh: Instant,
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
        // The TV set its clock (network back after power-on): the time appears at once.
        let clock_set = time_format::time_valid();
        if self.state.clock_set != clock_set {
            self.state.clock_set = clock_set;
            self.state.dirty = true;
        }
        if let Some(pictures) = &self.avatars {
            match pictures.try_recv() {
                Ok(pictures) => {
                    self.state.set_avatars(pictures);
                    self.avatars = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.avatars = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        // Behind an app nothing is seen: the profile picture's rotation waits
        // (each turn started a fade that kept the hidden loop composing).
        let in_front = self.presence.in_front();
        if !in_front {
            self.state.hold_avatar();
        }
        self.state.tick();
        // Upload decoded images a few per tick (idle or not), so each is on
        // the GPU before it is first drawn and no frame pays for a batch;
        // behind an app, none (see `Preload::pump`).
        // Idle and on screen: warm a few text glyphs ahead of need.
        if self.glyphs_warming
            && self.presence.visible
            && !self.state.dirty
            && !self.state.animating()
        {
            self.glyphs_warming = self.renderer.warm_glyphs(3);
        }
        self.preload
            .pump(&mut self.renderer, self.state.animating(), in_front);
        preload::wallpaper(
            &mut self.preload,
            &mut self.state,
            &self.assets,
            &self.renderer,
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
        self.extras();
        weather_tick(
            &mut self.state,
            &self.weather_service,
            self.presence.visible,
        );
        // Rebuild hit regions before input whenever inventory/layout changed.
        // Behind an app nothing is drawn, so nothing is composed either: a
        // minute passing (or a reply) left `dirty` set, as only a drawn frame
        // clears it, and every 2 s wake rebuilt the whole scene. The return
        // marks the screen dirty and composes it for that moment's clock.
        if self.state.dirty && in_front {
            self.scene = compose(&mut self.state, &self.assets, now, Some(&self.scene));
        }
        now
    }

    /// Snapshots for the switcher, the TV's clock format, and the effects TV
    /// replies queued (an uninstall's save and app list re-read).
    fn extras(&mut self) {
        self.snapshots.pump(&mut self.state);
        if let Some(hour_24) = time_format::hour_24()
            && hour_24 != self.state.hour_24
        {
            self.state.hour_24 = hour_24;
            self.state.dirty = true;
        }
        if time_format::refused() {
            self.state.notify(i18n::t("clockFormatError"));
        }
        let queued = std::mem::take(&mut self.state.queued);
        if !queued.is_empty() {
            self.running &= effects(
                queued,
                &mut self.state,
                &self.platform,
                &self.writer,
                &self.weather_service,
            );
        }
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
            Event::Quit { .. } => {
                if self.webos {
                    platform::status("quit asked: exiting");
                }
                self.running = false;
            }
            // As Home, the compositor hands Lounge the Home key while it is in front (it
            // launches nothing then): tvOS's Home, back to the top.
            Event::KeyDown {
                keycode: Some(key),
                repeat,
                ..
            } if i32::from(key) == sdl_events::WEBOS_HOME_KEYCODE => {
                if !repeat {
                    self.remote.press = None;
                    platform::status("home key: back to the top");
                    self.state.home_key(true);
                }
            }
            Event::KeyUp {
                keycode: Some(key), ..
            } if i32::from(key) == sdl_events::WEBOS_HOME_KEYCODE => {}
            Event::User { type_, code, .. } if Some(type_) == self.lifecycle => {
                if code == 2 {
                    platform::status("home: closed by webOS");
                    self.running = false;
                } else {
                    // Home opened while this run is resident: to the front at once.
                    // SAFETY: the window outlives the event loop.
                    unsafe { sdl2::sys::SDL_RaiseWindow(self.window.raw()) };
                    self.remote.press = None;
                    self.presence.raise();
                    self.state.dirty = true;
                }
            }
            Event::User { type_, code, .. } if type_ == self.home_type && code == APPS_CHANGED => {
                if !self.state.loading {
                    self.last_refresh = Instant::now();
                    pending.push(Effect::Refresh);
                }
            }
            Event::User { type_, .. } if type_ == self.home_type => {
                // The launch that brings Lounge back from an app lands just
                // after its window shows: only a launch well after that is
                // Home pressed on Lounge itself.
                self.remote.press = None;
                // webOS's relaunch reaches SDL just after this event: hidden,
                // the loop would sleep up to 2 s before pumping it.
                self.presence.raise();
                self.state.dirty = true;
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
                    // Resident as Home, an update only takes effect on a restart. Behind the
                    // app Lounge mounts the new version and exits, and the next Home press
                    // starts it. Restarting in place instead brought the new window over the
                    // app, and the old LS2 connection, kept across exec, held Home's service
                    // name so the lifecycle registration failed (seen 2026-10-29).
                    if platform::is_home() && platform::executable_replaced() {
                        match platform::refresh_home_mount() {
                            Ok(()) => {
                                platform::status(
                                    "home: a package update replaced Lounge; it starts on the next Home press",
                                );
                                self.running = false;
                            }
                            Err(error) => platform::status(&format!(
                                "home: renewing the mount failed: {error}"
                            )),
                        }
                    }
                    release_memory(&mut self.renderer, &mut self.preload);
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
                        release_memory(&mut self.renderer, &mut self.preload);
                    }
                }
                WindowEvent::Shown | WindowEvent::Restored | WindowEvent::FocusGained => {
                    // One return fires all three events: act once, on the
                    // hidden-to-shown edge.
                    let returned = self.presence.show();
                    if returned {
                        self.remote.press = None;
                        self.rewarm = true;
                        // The trim behind the app dropped the glyph outlines.
                        self.glyphs_warming = true;
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
                timestamp,
                keycode: Some(key),
                repeat,
                ..
            } => {
                pending = self.remote.key_down(
                    key,
                    repeat,
                    self.webos,
                    self.presence.front(timestamp),
                    &mut self.state,
                    &self.scene.controls,
                )
            }
            Event::KeyUp {
                timestamp,
                keycode: Some(key),
                ..
            } => {
                pending = self
                    .remote
                    .key_up(key, self.presence.front(timestamp), &mut self.state)
            }
            Event::MouseMotion {
                timestamp, x, y, ..
            } => {
                let hovered = hit(&self.scene.controls, &self.window, x, y, &self.state);
                self.remote
                    .pointer_moved(hovered, self.presence.front(timestamp), &mut self.state);
            }
            Event::MouseButtonDown {
                timestamp,
                mouse_btn: mouse_btn @ (MouseButton::Left | MouseButton::Right),
                x,
                y,
                ..
            } => {
                let target = hit(&self.scene.controls, &self.window, x, y, &self.state);
                self.remote.pointer_down(
                    target,
                    mouse_btn == MouseButton::Right,
                    self.presence.front(timestamp),
                    &mut self.state,
                );
            }
            Event::MouseButtonUp {
                timestamp,
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            } => {
                let target = hit(&self.scene.controls, &self.window, x, y, &self.state);
                let outside_modal = self.scene.modal.is_some_and(|rect| {
                    pointer_position(&self.window, x, y).is_some_and(|(x, y)| !rect.contains(x, y))
                });
                pending = self.remote.pointer_up(
                    target,
                    outside_modal,
                    self.presence.front(timestamp),
                    &mut self.state,
                );
            }
            Event::MouseWheel {
                timestamp, x, y, ..
            } if self.state.overlay.is_none()
                && self.presence.front(timestamp).is_some()
                && !self.state.pointer =>
            {
                // Pointer hidden (arrow-key use): the wheel moves focus a step at a time as the
                // arrows do, as on LG's own Home, so focus never scrolls out of sight.
                let direction = match (x.signum(), y.signum()) {
                    (1, _) => Some(state::Direction::Right),
                    (-1, _) => Some(state::Direction::Left),
                    (_, 1) => Some(state::Direction::Up),
                    (_, -1) => Some(state::Direction::Down),
                    _ => None,
                };
                if let Some(direction) = direction {
                    self.remote.press = None;
                    pending = self.state.direction(direction, &self.scene.controls);
                }
            }
            Event::MouseWheel {
                timestamp,
                x,
                y,
                mouse_x,
                mouse_y,
                ..
            } if self.state.overlay.is_none() && self.presence.front(timestamp).is_some() => {
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
        self.remote.settle(&mut self.state);
        // SDL may deliver several navigation events in a single poll.
        if self.state.dirty && self.presence.in_front() {
            self.scene = compose(&mut self.state, &self.assets, now, Some(&self.scene));
        }
    }

    /// After input: OK-holds, scripted steps, the crash record, inventory
    /// retries, weather, and the on-screen keyboard.
    fn follow_up(&mut self) -> Result<(), String> {
        let pending = self
            .remote
            .hold_due(self.presence.front(ticks()), &mut self.state);
        if !pending.is_empty() {
            self.running &= effects(
                pending,
                &mut self.state,
                &self.platform,
                &self.writer,
                &self.weather_service,
            );
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
        if self.healthy_at.is_some_and(|at| Instant::now() >= at) {
            self.healthy_at = None;
            crash_guard::end(std::path::Path::new(crash_guard::HOME_MARKER));
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
                if self.presented > 0 && self.presence.in_front() {
                    // What was let go behind an app goes again.
                    self.preload.resume(|path| self.renderer.has(path));
                }
                if self.rewarm && self.presented > 0 {
                    self.rewarm = false;
                    preload::refill(&mut self.preload, &self.state, &self.assets, &self.renderer);
                }
                if !self.replies.prefetched && self.presented > 0 {
                    self.replies.prefetched = true;
                    preload::prefetch(&mut self.preload, &self.state, &self.assets, &self.renderer);
                }
                // Repaint only what changed since the buffer was last drawn
                // (EGL partial update); anything uncertain repaints it all.
                self.renderer.damage = self.partial.region(&self.scene.draw, width, height);
                self.renderer.render(&self.scene.draw, width, height);
                self.window.gl_swap_window();
                self.presence.presented(ticks());
                self.partial.drawn(&self.scene.draw);
                if self.presented == 0 {
                    self.stage("first frame");
                    if platform::is_home() {
                        self.healthy_at = Some(
                            Instant::now()
                                + Duration::from_secs(crash_guard::HEALTHY_AFTER_FRAME_SECONDS),
                        );
                    }
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
            if self.presence.raising() { 16 } else { 2000 }
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
        if platform::is_home() {
            crash_guard::end(std::path::Path::new(crash_guard::HOME_MARKER));
        }
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
