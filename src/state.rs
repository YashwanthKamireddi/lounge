use crate::i18n;
use crate::model::{App, BACKGROUNDS, InputMode, Inventory, Preferences};
use crate::weather::{self, Unit};
use serde::Serialize;
use std::time::{Duration, Instant};

pub const WEATHER_SECTION: usize = 2;
pub const INPUTS_SECTION: usize = 3;
pub const SECTION_COUNT: usize = 4;
pub const APPS_COLUMNS: usize = 5;
pub const HOME_STRIP_LEFT: f32 = 84.;
pub const HOME_STRIP_RIGHT: f32 = 1836.;
pub const HOME_TILE_WIDTH: f32 = 324.;
pub const HOME_TILE_PITCH: f32 = 354.;
pub const FOCUS_MS: f32 = 260.;
/// tvOS 26 home, five large icons a row (324x194, Apple's 5:3): in Top Shelf
/// mode the dock icons sit at y=780 with the next row peeking at the bottom;
/// scrolling by GRID_SCROLL brings the dock to the top (y=90) for the grid.
pub const DOCK_TOP: f32 = 780.;
/// How far the dock drops below the screen for full-screen Top Shelf.
pub const SHELF_DROP: f32 = 360.;
/// Seconds the Top Shelf chevron bobs after the dock comes back.
pub const SHELF_HINT: f32 = 2.4;
/// How long focus rests on a dock app before its Top Shelf art fades in.
pub const SHELF_REST: f32 = 0.18;
/// Top Shelf art and the dock glass dissolve between apps over this long.
pub const SHELF_FADE: f32 = 0.5;
pub const GRID_SCROLL: f32 = 690.;
pub const DOCK_GAP: f32 = 280.;
/// In the full grid the rows spread: icons at 90/394/698/1002.
pub const GRID_GAP: f32 = 304.;
pub const ROW_PITCH: f32 = 304.;
pub const GRID_LEFT: f32 = 90.;
pub const GRID_STEP: f32 = 354.;
pub const SETTINGS_TILE: &str = "luma.settings";
pub const SEARCH_TILE: &str = "luma.search";
pub const HOME_DASHBOARD: &str = "com.webos.app.homeconnect";
pub const PAGE_MS: f32 = 280.;
pub const SWITCHER_CLOSE_MS: u64 = 320;
/// How long a banner stays up (its last 0.25 s fade it out).
pub const NOTICE_SECONDS: f32 = 5.;
/// Search and the context menu fade out over this long when closed.
pub const FADE_OUT_MS: u64 = 160;
/// After a close, the windows to its right slide into the gap over this long.
pub const SWITCHER_REFLOW_MS: u64 = 460;
/// Back fades and settles the switcher away over this long.
pub const SWITCHER_DISMISS_MS: u64 = 220;

fn page_order(page: Page) -> f32 {
    match page {
        Page::Home => 0.,
        Page::Apps => 1.,
        Page::Settings => 2.,
    }
}

fn eased(start: &Instant) -> f32 {
    let t = (start.elapsed().as_secs_f32() * 1000. / FOCUS_MS).min(1.);
    1. - (1. - t).powi(3)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Page {
    Home,
    Apps,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Target {
    Nav(usize),
    Switch(usize),
    App(String),
    Input(String),
    Browse,
    Search,
    Sidebar(usize),
    Setting(usize),
    Background(usize),
    InputMode(String),
    Choice(usize),
    Retry,
    DismissNotice,
    Option(usize),
    Key(usize),
    WeatherConfigure,
    WeatherToggle,
    WeatherLocation,
    WeatherTemperature,
    WeatherQuery,
    WeatherSearch,
    WeatherClear,
    WeatherResult(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn center(self) -> (f32, f32) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }
    pub fn inset(self, n: f32) -> Self {
        Self::new(self.x + n, self.y + n, self.w - 2. * n, self.h - 2. * n)
    }
}

#[derive(Clone, Debug)]
pub struct Control {
    pub target: Target,
    pub rect: Rect,
    pub scrollable: bool,
}

#[derive(Clone, Debug)]
pub enum Overlay {
    Options { id: String, restore: Target },
    Keyboard,
    SystemKeyboard,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Subpage {
    Clock,
    Input(String),
    WeatherLocation,
    Temperature,
    Hidden,
    Avatar,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum TextField {
    Apps,
    Location,
}

#[derive(Clone, Copy)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, PartialEq)]
pub enum Effect {
    Save,
    Refresh,
    Launch(String),
    /// Ask webOS which apps are open, for the app switcher.
    Running,
    Close(String),
    Quit,
    Weather(weather::Command),
}

pub const KEYS: [&str; 40] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "Space",
    "Delete", "Clear", "Done",
];

pub struct State {
    pub page: Page,
    pub focus: Target,
    pub inventory: Inventory,
    /// The signed-in LG profile for the avatar; None when nobody is.
    pub profile: Option<crate::model::Profile>,
    /// The profile picture shown now (its small cut), in place of the
    /// profile's initial; see `avatars`.
    pub avatar: Option<String>,
    /// The owner's pictures (crate::avatars), and which one shows.
    pub avatars: Vec<crate::avatars::Picture>,
    avatar_index: usize,
    /// When the shown picture last changed, and the one it replaced (it
    /// crossfades out).
    avatar_since: Instant,
    pub avatar_from: Option<(String, Instant)>,
    pub preferences: Preferences,
    pub query: String,
    pub section: usize,
    pub subpage: Option<Subpage>,
    pub scroll: f32,
    pub home_scroll: f32,
    pub overlay: Option<Overlay>,
    pub moving: Option<String>,
    /// When Move began: the app lifts out of the grid from then.
    pub moving_since: Option<Instant>,
    pub loading: bool,
    pub launching: bool,
    pub notice: Option<(String, Instant)>,
    pub error: Option<String>,
    pub dirty: bool,
    pub pointer: bool,
    pub pressed: Option<Target>,
    pub previous_background: Option<(String, Instant)>,
    pub panel_started: Option<Instant>,
    pub weather: weather::Weather,
    pub keyboard_field: TextField,
    /// Outgoing focus target, when it started shrinking, and how focused it was then.
    pub focus_from: Option<(Target, Instant, f32)>,
    focus_since: Option<Instant>,
    last_focus: Target,
    /// Drawn scroll offsets glide toward `scroll`/`home_scroll`; layout and
    /// navigation always use the exact targets.
    pub shown_scroll: f32,
    pub shown_home_scroll: f32,
    shown_page: Page,
    last_tick: Instant,
    /// When the visible page last changed, and the direction travelled (-1/1).
    pub page_since: Option<(Instant, f32)>,
    pub switcher: Option<Switcher>,
    /// Settings' level (list, section, sub-page) last changed, and the side
    /// the new level slides in from (1 deeper, -1 back).
    pub settings_since: Option<(Instant, f32)>,
    /// Move mode just reordered apps: when, and how far each tile was drawn
    /// from its new slot then (pixels). The offsets ease to nothing, so every
    /// tile glides from exactly where it was on screen, even mid-glide.
    pub rearranged: Option<(Instant, Glide)>,
    /// Move changed the home order and it is not on disk yet (it is saved
    /// once Move ends).
    pub unsaved_order: bool,
    /// The Settings page being left, as it was last drawn, fading out.
    pub settings_ghost: Option<Vec<crate::renderer::Draw>>,
    settings_level: (u8, usize),
    /// The level shown before the current one (for the left panel's fade).
    pub settings_previous: (u8, usize),
    /// The context menu just closed (for which app, when): it fades away.
    pub options_closed: Option<(String, Instant)>,
    /// Search just closed: its dark page fades off the home.
    pub search_closed: Option<Instant>,
    /// Full-screen Top Shelf: Up from the dock lowers it off screen.
    pub expanded: bool,
    /// When Top Shelf mode last began; its chevron bobs for a moment.
    pub shelf_since: Option<Instant>,
    /// The app whose Top Shelf art shows; it stays while the pill has focus.
    pub shelf_app: Option<String>,
    /// A dock app focus has just reached, waiting to rest before its Top
    /// Shelf art takes over.
    pub shelf_pending: Option<(String, Instant)>,
    /// The previous Top Shelf app and when the dissolve away from it began.
    pub shelf_from: Option<(Option<String>, Instant)>,
    /// Apps with Top Shelf art, from the asset bundle.
    pub heroes: Vec<String>,
}

/// tvOS-style app switcher over Home: open apps as large cards.
/// How long tiles take to glide to their new slots in Move mode, in ms.
/// Each gliding tile's offset from its slot, in pixels, by app.
pub type Glide = std::collections::HashMap<String, (f32, f32)>;
/// The avatar preference that shows every picture in turn.
pub const AVATAR_ROTATE: &str = "rotate";
/// Seconds each rotating picture shows, and its crossfade.
pub const AVATAR_SECONDS: f32 = 20.;
pub const AVATAR_FADE: f32 = 0.6;
pub const REARRANGE_MS: f32 = 240.;

/// How long a Settings level takes to push in or back, in milliseconds.
pub const SETTINGS_PUSH_MS: f32 = 280.;

pub struct Switcher {
    pub apps: Vec<String>,
    /// Drawn card position, gliding toward the focused index.
    pub shown: f32,
    pub opened: Instant,
    /// Cards sliding away after Up closed their app, with their slot offset.
    pub closing: Vec<(String, Instant, f32)>,
    /// The slot a close emptied and when: windows from it on slide in.
    pub reflow: Option<(usize, Instant)>,
    /// Back pressed: the switcher is fading away over the live home.
    pub dismissed: Option<Instant>,
    restore: Target,
}

impl State {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            page: Page::Apps,
            focus: Target::Nav(0),
            inventory: Inventory::default(),
            profile: None,
            avatar: None,
            avatars: Vec::new(),
            avatar_index: 0,
            avatar_since: Instant::now(),
            avatar_from: None,
            preferences,
            query: String::new(),
            section: 0,
            subpage: None,
            scroll: 0.,
            home_scroll: 0.,
            overlay: None,
            moving: None,
            moving_since: None,
            loading: false,
            launching: false,
            notice: None,
            error: None,
            dirty: true,
            pointer: false,
            pressed: None,
            previous_background: None,
            panel_started: None,
            weather: weather::Weather::default(),
            keyboard_field: TextField::Apps,
            focus_from: None,
            focus_since: None,
            last_focus: Target::Nav(0),
            shown_scroll: 0.,
            shown_home_scroll: 0.,
            shown_page: Page::Apps,
            last_tick: Instant::now(),
            page_since: None,
            switcher: None,
            settings_since: None,
            rearranged: None,
            unsaved_order: false,
            settings_ghost: None,
            settings_previous: (0, 0),
            settings_level: (0, 0),
            options_closed: None,
            search_closed: None,
            expanded: false,
            shelf_since: Some(Instant::now()),
            shelf_app: None,
            shelf_pending: None,
            shelf_from: None,
            heroes: Vec::new(),
        }
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
        self.dirty = true;
    }

    /// Eased 0..1 focus emphasis for a target; continuous across rapid moves.
    pub fn focus_amount(&self, target: &Target) -> f32 {
        if &self.focus == target {
            self.focus_since.as_ref().map_or(1., eased)
        } else if let Some((from, start, level)) = &self.focus_from
            && from == target
        {
            level * (1. - eased(start))
        } else {
            0.
        }
    }

    pub fn track_focus(&mut self) {
        if self.focus != self.last_focus {
            let level = self.focus_since.as_ref().map_or(1., eased);
            let previous = std::mem::replace(&mut self.last_focus, self.focus.clone());
            // Only dock apps own Top Shelf; the grid below keeps the last one.
            // The art changes once focus rests on an app: a sweep along the
            // dock crosses no full-screen dissolves (each costs this TV's GPU
            // its frame budget for the whole fade).
            if let Target::App(id) = &self.focus
                && self.apps().iter().take(APPS_COLUMNS).any(|app| &app.id == id)
                && self.shelf_app.as_ref() != Some(id)
            {
                if self.shelf_app.is_none() || self.preferences.reduced_motion {
                    self.shelf_app = Some(id.clone());
                    self.shelf_pending = None;
                } else {
                    self.shelf_pending = Some((id.clone(), Instant::now()));
                }
            } else {
                self.shelf_pending = None;
            }
            // The first real focus (out of the startup placeholder) appears
            // already settled: webOS is animating Luma's window in, and a
            // tile growing at the same time read as a second opening.
            let first = matches!(previous, Target::Nav(_)) && self.page == Page::Apps;
            let now = (!self.preferences.reduced_motion && !first).then(Instant::now);
            self.focus_from = now.map(|now| (previous, now, level));
            self.focus_since = now;
            self.dirty = true;
        }
        if self
            .shelf_pending
            .as_ref()
            .is_some_and(|(_, since)| since.elapsed().as_secs_f32() >= SHELF_REST)
            && let Some((id, _)) = self.shelf_pending.take()
        {
            let previous = self.shelf_app.replace(id);
            self.shelf_from = Some((previous, Instant::now()));
            self.dirty = true;
        }
        if let Some((_, start)) = &self.shelf_from {
            if start.elapsed().as_secs_f32() >= SHELF_FADE {
                self.shelf_from = None;
            }
            self.dirty = true;
        }
        let done = |start: &Instant| start.elapsed().as_secs_f32() * 1000. >= FOCUS_MS;
        if self.focus_since.is_some() || self.focus_from.is_some() {
            self.dirty = true;
            if self.focus_since.as_ref().is_none_or(done) {
                self.focus_since = None;
            }
            if self
                .focus_from
                .as_ref()
                .is_none_or(|(_, start, _)| done(start))
            {
                self.focus_from = None;
            }
        }
    }

    pub fn glide_scroll(&mut self) {
        let now = Instant::now();
        // An idle home draws no frames, so the gap since the last one can be
        // seconds: capped at a frame or two, the first press after a pause
        // glides like every other instead of jumping straight to its place.
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.04);
        self.last_tick = now;
        if self.page != self.shown_page || self.preferences.reduced_motion {
            if self.page != self.shown_page && !self.preferences.reduced_motion {
                let direction = page_order(self.page) - page_order(self.shown_page);
                self.page_since = Some((now, direction.signum()));
                self.dirty = true;
            }
            self.shown_page = self.page;
            self.shown_scroll = self.scroll;
            self.shown_home_scroll = self.home_scroll;
            return;
        }
        // Critically damped approach: frame-rate independent, never overshoots.
        let k = 1. - (-dt / 0.075).exp();
        if let (Some(switcher), Target::Switch(index)) = (&mut self.switcher, &self.focus) {
            let target = *index as f32;
            if (target - switcher.shown).abs() < 0.002 {
                switcher.shown = target;
            } else {
                switcher.shown += (target - switcher.shown) * k;
                self.dirty = true;
            }
        }
        for (shown, target) in [
            (&mut self.shown_scroll, self.scroll),
            (&mut self.shown_home_scroll, self.home_scroll),
        ] {
            if (target - *shown).abs() < 0.5 {
                *shown = target;
            } else {
                *shown += (target - *shown) * k;
                self.dirty = true;
            }
        }
    }

    pub fn tick(&mut self) {
        self.track_focus();
        self.glide_scroll();
        if let Some(switcher) = &mut self.switcher {
            // Keep presenting frames while the switcher fades in or a card is
            // being dismissed; otherwise the first frame would freeze on screen.
            let before = switcher.closing.len();
            switcher
                .closing
                .retain(|(_, start, _)| start.elapsed() < Duration::from_millis(SWITCHER_CLOSE_MS));
            if switcher
                .reflow
                .is_some_and(|(_, start)| start.elapsed() >= Duration::from_millis(SWITCHER_REFLOW_MS))
            {
                switcher.reflow = None;
            }
            if before != switcher.closing.len()
                || !switcher.closing.is_empty()
                || switcher.reflow.is_some()
                || switcher.dismissed.is_some()
                || switcher.opened.elapsed() < Duration::from_millis(320)
            {
                self.dirty = true;
            }
            if switcher.dismissed.is_some_and(|start| {
                start.elapsed() >= Duration::from_millis(SWITCHER_DISMISS_MS)
            }) && switcher.closing.is_empty()
            {
                self.switcher = None;
            }
        }
        if let Some((start, _)) = self.page_since {
            self.dirty = true;
            if start.elapsed().as_secs_f32() >= PAGE_MS / 1000. {
                self.page_since = None;
            }
        }
        self.track_settings_level();
        // Move's glide and lift are drawn on every frame while they run (only
        // marking the end left the tiles frozen in place, then jumping).
        if let Some((at, _)) = &self.rearranged {
            if at.elapsed().as_secs_f32() * 1000. >= REARRANGE_MS {
                self.rearranged = None;
            }
            self.dirty = true;
        }
        if self.moving.is_some() && self.move_raise() < 1. {
            self.dirty = true;
        }
        if self
            .settings_since
            .is_some_and(|(start, _)| start.elapsed().as_secs_f32() * 1000. >= SETTINGS_PUSH_MS)
        {
            self.settings_since = None;
            self.settings_ghost = None;
            self.dirty = true;
        }
        if self
            .search_closed
            .is_some_and(|start| start.elapsed() >= Duration::from_millis(FADE_OUT_MS))
        {
            self.search_closed = None;
            self.dirty = true;
        }
        if self
            .options_closed
            .as_ref()
            .is_some_and(|(_, start)| start.elapsed() >= Duration::from_millis(FADE_OUT_MS))
        {
            self.options_closed = None;
            self.dirty = true;
        }
        if let Some(start) = self.panel_started {
            self.dirty = true;
            let duration = if self.overlay.is_some() { 120 } else { 150 };
            if self.preferences.reduced_motion || start.elapsed() >= Duration::from_millis(duration)
            {
                self.panel_started = None;
            }
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, start)| start.elapsed() < Duration::from_millis(120))
            && !self.preferences.reduced_motion
        {
            self.dirty = true;
        }
        if let Some((_, start)) = &self.previous_background {
            self.dirty = true;
            if self.preferences.reduced_motion || start.elapsed() >= Duration::from_millis(400) {
                self.previous_background = None;
            }
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, start)| start.elapsed().as_secs_f32() > NOTICE_SECONDS)
        {
            self.notice = None;
            self.dirty = true;
        }
        self.rotate_avatar();
        // One rule for every animation: while anything moves, each frame is
        // drawn. Marking only an animation's end froze it on its first frame
        // and then jumped (Move's glide, the Settings push, menu fades).
        if self.animating() {
            self.dirty = true;
        }
    }

    /// Starts a Settings push when the level shown changed (0 the section
    /// list, 1 a section, 2 a sub-page). Called as each frame is composed, so
    /// the push begins on the very frame the new level first draws.
    pub fn track_settings_level(&mut self) -> bool {
        let depth = if self.page != Page::Settings {
            0
        } else if self.subpage.is_some() {
            2
        } else if matches!(self.focus, Target::Sidebar(_)) {
            0
        } else {
            1
        };
        // On the section list, moving between sections is focus, not a new
        // page: only entering or leaving a level pushes.
        let level = (depth, if depth == 0 { 0 } else { self.section });
        let changed = self.page == Page::Settings && level != self.settings_level;
        if changed {
            self.settings_previous = self.settings_level;
            self.settings_ghost = None;
            if !self.preferences.reduced_motion && self.page_since.is_none() {
                let direction = if depth >= self.settings_level.0 { 1. } else { -1. };
                self.settings_since = Some((Instant::now(), direction));
            }
            self.dirty = true;
        }
        if self.page != Page::Settings {
            self.settings_ghost = None;
        }
        self.settings_level = level;
        changed && self.settings_since.is_some()
    }

    pub fn page_index(&self) -> usize {
        match self.page {
            Page::Home => 0,
            Page::Apps => 1,
            Page::Settings => 2,
        }
    }

    /// Opens the switcher with webOS's open apps, most recently used first.
    pub fn open_switcher(&mut self, running: Vec<String>) {
        // The reply is asynchronous: the viewer may have moved on meanwhile.
        if self.page != Page::Apps || self.overlay.is_some() || self.switcher.is_some() {
            return;
        }
        let known = |id: &String| {
            self.inventory.apps.iter().any(|app| &app.id == id)
                && id != crate::platform::APP_ID
                && id != crate::platform::HOME_ID
        };
        let mut apps: Vec<String> = self
            .preferences
            .recent
            .iter()
            .filter(|id| running.contains(id))
            .cloned()
            .collect();
        for id in running {
            if !apps.contains(&id) {
                apps.push(id);
            }
        }
        apps.retain(known);
        self.dirty = true;
        if apps.is_empty() {
            self.notify(i18n::t("noOpenApps"));
            return;
        }
        let restore = std::mem::replace(&mut self.focus, Target::Switch(0));
        self.switcher = Some(Switcher {
            apps,
            shown: 0.,
            opened: Instant::now(),
            closing: vec![],
            reflow: None,
            dismissed: None,
            restore,
        });
    }

    /// The visible Home favourite at `slot`, opened directly.
    pub fn quick_launch(&mut self, slot: usize) -> Vec<Effect> {
        let favorites = self.preferences.favorites(&self.inventory.apps);
        favorites
            .iter()
            .filter(|id| {
                !self.preferences.hidden.contains(id)
                    && self.inventory.apps.iter().any(|app| &app.id == *id)
            })
            .nth(slot)
            .cloned()
            .map_or_else(Vec::new, |id| vec![Effect::Launch(id)])
    }

    pub fn note_launch(&mut self, id: &str) {
        self.preferences.recent.retain(|item| item != id);
        self.preferences.recent.insert(0, id.to_owned());
        self.preferences.recent.truncate(24);
    }

    pub fn animating(&self) -> bool {
        let switcher_moving = self.switcher.as_ref().is_some_and(|switcher| {
            switcher.opened.elapsed() < Duration::from_millis(300)
                || !switcher.closing.is_empty()
                || switcher.reflow.is_some()
                || switcher.dismissed.is_some()
                || matches!(self.focus, Target::Switch(index) if (switcher.shown - index as f32).abs() > 0.001)
        });
        self.previous_background.is_some()
            || self.panel_started.is_some()
            || switcher_moving
            || self.focus_since.is_some()
            || self.focus_from.is_some()
            || self.page_since.is_some()
            || self.shown_scroll != self.scroll
            || self.shown_home_scroll != self.home_scroll
            || self.shelf_from.is_some()
            || self.shelf_pending.is_some()
            || self.settings_since.is_some()
            || self.rearranged.is_some()
            || (self.moving.is_some() && self.move_raise() < 1.)
            || self.avatar_from.is_some()
            // Keep ticking until a paused-typing search has been sent.
            || self.weather.search_pending()
            || self.options_closed.is_some()
            || self.search_closed.is_some()
            // The chevron only exists with Top Shelf art; never redraw for it otherwise.
            || (!self.preferences.reduced_motion
                && self.preferences.top_shelf_art
                && self
                    .shelf_since
                    .is_some_and(|since| since.elapsed().as_secs_f32() < SHELF_HINT))
            || (!self.preferences.reduced_motion
                && self
                    .notice
                    .as_ref()
                    .is_some_and(|(_, start)| {
                        let age = start.elapsed().as_secs_f32();
                        age < 0.12 || age > NOTICE_SECONDS - 0.25
                    }))
    }

    pub fn open(&mut self, index: usize) {
        self.weather.cancel_search();
        // One tvOS-style home (the app grid) plus Settings; no tab bar.
        self.page = if index == 2 {
            Page::Settings
        } else {
            Page::Apps
        };
        self.scroll = 0.;
        if self.page == Page::Home {
            self.scroll_favorites(0.);
        }
        self.query.clear();
        self.overlay = None;
        self.moving = None;
        self.subpage = None;
        self.panel_started = None;
        self.dirty = true;
        self.focus = self.first_content();
    }

    pub fn apps(&self) -> Vec<&App> {
        let ids = self.preferences.favorites(&self.inventory.apps);
        if self.page == Page::Home {
            return ids
                .iter()
                .filter(|id| !self.preferences.hidden.contains(id))
                .filter_map(|id| self.inventory.apps.iter().find(|app| &app.id == id))
                .collect();
        }
        // The query narrows results only on the search screen; Home always
        // shows every app (the last search is kept for next time, as on tvOS).
        let query = if matches!(
            self.overlay,
            Some(Overlay::Keyboard | Overlay::SystemKeyboard)
        ) {
            self.query.to_lowercase()
        } else {
            String::new()
        };
        let mut apps: Vec<_> = self
            .inventory
            .apps
            .iter()
            // LG's Home Dashboard stays off the grid.
            .filter(|app| app.id != HOME_DASHBOARD)
            .filter(|app| {
                !self.preferences.hidden.contains(&app.id)
                    && app.title.to_lowercase().contains(&query)
            })
            .collect();
        apps.sort_by(|a, b| {
            // Dock favourites first, then Settings, Search and inputs, then A-Z.
            let rank = |app: &App| {
                ids.iter()
                    .position(|id| id == &app.id)
                    .unwrap_or(match app.brand.as_str() {
                        "luma-settings" => usize::MAX - 3,
                        "luma-search" => usize::MAX - 2,
                        "luma-input" => usize::MAX - 1,
                        _ => usize::MAX,
                    })
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        apps
    }

    pub fn scroll_favorites(&mut self, delta: f32) {
        let count = self
            .preferences
            .favorites(&self.inventory.apps)
            .iter()
            .filter(|id| {
                !self.preferences.hidden.contains(id)
                    && self.inventory.apps.iter().any(|app| &app.id == *id)
            })
            .count();
        let content = if count == 0 {
            0.
        } else {
            (count - 1) as f32 * HOME_TILE_PITCH + HOME_TILE_WIDTH
        };
        let max = (content - (HOME_STRIP_RIGHT - HOME_STRIP_LEFT)).max(0.);
        let next = (self.home_scroll + delta).clamp(0., max);
        if next != self.home_scroll {
            self.home_scroll = next;
            self.dirty = true;
        }
    }

    pub fn set_inventory(&mut self, next: Inventory, warning: Option<String>) {
        self.inventory.apps = next.apps;
        if let Some(warning) = warning {
            self.notify(warning);
        } else {
            self.inventory.inputs = next.inputs;
        }
        self.loading = false;
        self.error = None;
        self.dirty = true;
        self.refresh_tiles();
        self.reconcile_focus();
    }

    /// Settings, Search and visible HDMI inputs live in the grid as tiles, like
    /// tvOS's own Settings and Search icons.
    pub fn refresh_tiles(&mut self) {
        self.inventory
            .apps
            .retain(|app| !app.brand.starts_with("luma-"));
        let tile = |id: &str, title: &str, brand: &str| App {
            id: id.into(),
            title: title.into(),
            brand: brand.into(),
            category: "Utilities".into(),
            icon: None,
        };
        let mut tiles = vec![
            tile(SETTINGS_TILE, i18n::t("settings"), "luma-settings"),
            tile(SEARCH_TILE, i18n::t("search"), "luma-search"),
        ];
        for input in &self.inventory.inputs {
            if self.preferences.input_visible(input)
                && !self.inventory.apps.iter().any(|app| app.id == input.id)
            {
                tiles.push(tile(&input.id, &input.port, "luma-input"));
            }
        }
        self.inventory.apps.extend(tiles);
    }

    pub fn reconcile_focus(&mut self) {
        if self.page == Page::Apps && matches!(self.focus, Target::Nav(_)) {
            self.focus = self.first_content();
        }
        if self.page == Page::Home {
            self.scroll_favorites(0.);
        }
        if let Target::App(id) = &self.focus
            && !self.apps().iter().any(|app| &app.id == id)
        {
            self.focus = self.first_content();
        }
        if let Target::Input(id) = &self.focus
            && !self
                .inventory
                .inputs
                .iter()
                .any(|input| &input.id == id && self.preferences.input_visible(input))
        {
            self.focus = self.first_content();
        }
    }

    pub fn first_content(&self) -> Target {
        match self.page {
            Page::Home => self
                .apps()
                .get(((self.home_scroll - 0.01).max(0.) / HOME_TILE_PITCH).ceil() as usize)
                .map(|app| Target::App(app.id.clone()))
                .unwrap_or(if self.error.is_some() {
                    Target::Retry
                } else {
                    Target::Browse
                }),
            Page::Apps => self
                .apps()
                .first()
                .map(|app| Target::App(app.id.clone()))
                .unwrap_or(if self.error.is_some() {
                    Target::Retry
                } else {
                    Target::Browse
                }),
            Page::Settings => Target::Sidebar(0),
        }
    }

    pub fn focus(&mut self, target: Target, controls: &[Control]) {
        self.pointer = false;
        // The tvOS home has no tab bar: moving past the top edge stays put.
        if matches!(target, Target::Nav(_)) {
            return;
        }
        self.focus = target;
        self.reveal_focus(controls);
        if let Target::Sidebar(section) = self.focus {
            if self.subpage == Some(Subpage::WeatherLocation) {
                self.weather.cancel_search();
            }
            self.section = section;
            self.subpage = None;
        }
        self.dirty = true;
    }

    /// Whether `id` shows Top Shelf art: the user has it on and art exists.
    pub fn has_art(&self, id: &str) -> bool {
        self.preferences.top_shelf_art && self.heroes.iter().any(|hero| hero == id)
    }

    /// Remote focus is revealed after layout changes, including hide/reorder.
    /// Pointer paths deliberately do not call this method.
    pub fn reveal_focus(&mut self, controls: &[Control]) -> bool {
        let previous = self.scroll;
        let previous_home = self.home_scroll;
        // tvOS home: the dock row shows Top Shelf mode; any other row slides
        // the grid up so the dock sits at the top, scrolling further as needed.
        if self.page == Page::Apps
            && self.overlay.is_none()
            && let Target::App(id) = &self.focus
            && let Some(index) = self.apps().iter().position(|app| &app.id == id)
        {
            let row = index / APPS_COLUMNS;
            if row > 0 {
                self.expanded = false;
            }
            self.scroll = if row == 0 && self.expanded {
                -SHELF_DROP
            } else if row == 0 {
                if previous > 0. {
                    self.shelf_since = Some(Instant::now());
                }
                0.
            } else {
                let y = 90. + GRID_GAP + (row - 1) as f32 * ROW_PITCH;
                let extra = (self.scroll - GRID_SCROLL)
                    .max(0.)
                    .clamp((y + 210. - 1030.).max(0.), (y - 90.).max(0.));
                GRID_SCROLL + extra
            };
            if self.scroll != previous {
                self.dirty = true;
            }
            return self.scroll != previous;
        }
        if let Some(control) = controls.iter().find(|c| c.target == self.focus) {
            if self.page == Page::Home && matches!(self.focus, Target::App(_)) {
                if control.rect.x < HOME_STRIP_LEFT {
                    self.home_scroll += control.rect.x - HOME_STRIP_LEFT;
                } else if control.rect.x + control.rect.w > HOME_STRIP_RIGHT {
                    self.home_scroll += control.rect.x + control.rect.w - HOME_STRIP_RIGHT;
                }
            }
            if matches!(
                self.focus,
                Target::Nav(_)
                    | Target::Search
                    | Target::Sidebar(_)
            ) {
                self.scroll = 0.;
            } else if control.scrollable {
                let top = if self.page == Page::Apps {
                    DOCK_TOP
                } else {
                    120.8
                };
                if control.rect.y < top {
                    self.scroll = (self.scroll + control.rect.y - top).max(0.);
                }
                let bottom = if self.page == Page::Home && self.moving.is_none() {
                    1041.6
                } else {
                    982.4
                };
                if control.rect.y + control.rect.h > bottom {
                    self.scroll += control.rect.y + control.rect.h - bottom;
                }
            }
        }
        if self.page == Page::Home {
            self.scroll_favorites(0.);
        }
        if self.scroll != previous || self.home_scroll != previous_home {
            self.dirty = true;
        }
        self.scroll != previous || self.home_scroll != previous_home
    }

    /// Hover and pointer-down must not move the control before pointer-up.
    pub fn pointer_focus(&mut self, target: Target) {
        if !self.pointer {
            self.dirty = true;
        }
        self.pointer = true;
        if self.focus == target {
            return;
        }
        if let Target::Sidebar(section) = target {
            if self.subpage == Some(Subpage::WeatherLocation) {
                self.weather.cancel_search();
            }
            self.section = section;
            self.subpage = None;
        }
        self.focus = target;
        self.dirty = true;
    }

    /// Installed apps the user hid, in the order they were hidden.
    pub fn hidden_apps(&self) -> Vec<&App> {
        self.preferences
            .hidden
            .iter()
            .filter_map(|id| self.inventory.apps.iter().find(|app| &app.id == id))
            .collect()
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden_apps().len()
    }

    fn section_entry(&self) -> Target {
        match self.section {
            1 => Target::Background(
                BACKGROUNDS
                    .iter()
                    .position(|(id, _)| *id == self.preferences.background)
                    .unwrap_or(3),
            ),
            WEATHER_SECTION => Target::WeatherToggle,
            INPUTS_SECTION => self
                .inventory
                .inputs
                .first()
                .map(|input| Target::InputMode(input.id.clone()))
                .unwrap_or(Target::Setting(3)),
            _ => Target::Setting(0),
        }
    }

    fn close_subpage(&mut self) {
        if self.subpage == Some(Subpage::WeatherLocation) {
            self.weather.cancel_search();
        }
        self.panel_started = None;
        self.pointer = false;
        self.focus = match self.subpage.take() {
            Some(Subpage::Clock) => Target::Setting(1),
            Some(Subpage::Input(id)) => Target::InputMode(id),
            Some(Subpage::WeatherLocation) => Target::WeatherLocation,
            Some(Subpage::Temperature) => Target::WeatherTemperature,
            // Its row goes once nothing is hidden; focus takes the row below.
            Some(Subpage::Hidden) => Target::Setting(if self.hidden_count() > 0 { 7 } else { 3 }),
            Some(Subpage::Avatar) => Target::Setting(8),
            None => Target::Sidebar(self.section),
        };
        self.scroll = 0.;
        self.dirty = true;
    }

    pub fn direction(&mut self, direction: Direction, controls: &[Control]) -> Vec<Effect> {
        self.pointer = false;
        self.dirty = true;
        if let Some(switcher) = &mut self.switcher
            && switcher.dismissed.is_none()
        {
            let Target::Switch(index) = self.focus else {
                return vec![];
            };
            match direction {
                Direction::Left => self.focus = Target::Switch(index.saturating_sub(1)),
                Direction::Right => {
                    self.focus =
                        Target::Switch((index + 1).min(switcher.apps.len().saturating_sub(1)))
                }
                Direction::Up if index < switcher.apps.len() => {
                    // tvOS: swipe a window up to quit that app.
                    let id = switcher.apps.remove(index);
                    let now = Instant::now();
                    switcher.closing.push((id.clone(), now, index as f32 - switcher.shown));
                    if switcher.apps.is_empty() {
                        // Let the last window finish flying away, then leave.
                        switcher.dismissed = Some(now);
                        self.focus = switcher.restore.clone();
                    } else {
                        // The windows to the right slide into the gap.
                        switcher.reflow = Some((index, now));
                        self.focus = Target::Switch(index.min(switcher.apps.len() - 1));
                    }
                    return vec![Effect::Close(id)];
                }
                Direction::Up => {}
                Direction::Down => {}
            }
            return vec![];
        }
        if matches!(self.overlay, Some(Overlay::SystemKeyboard)) {
            // The TV keyboard owns its arrows until Done/Back dismisses it.
            return vec![];
        }
        if let Some(id) = self.moving.clone() {
            // Moving reflows the others, as tvOS does: Left/Right by one
            // slot, Up/Down by one row. Hidden or missing ids keep their place.
            let mut ids = self.preferences.favorites(&self.inventory.apps);
            let slots: Vec<_> = ids
                .iter()
                .enumerate()
                .filter_map(|(index, id)| {
                    (!self.preferences.hidden.contains(id)
                        && self.inventory.apps.iter().any(|app| &app.id == id))
                    .then_some(index)
                })
                .collect();
            if let Some(index) = slots.iter().position(|slot| ids[*slot] == id) {
                let last = slots.len() - 1;
                let next = match direction {
                    Direction::Left => index.saturating_sub(1),
                    Direction::Right => (index + 1).min(last),
                    Direction::Up => index.saturating_sub(APPS_COLUMNS),
                    Direction::Down => (index + APPS_COLUMNS).min(last),
                };
                if next != index {
                    let mut visible: Vec<String> = slots.iter().map(|slot| ids[*slot].clone()).collect();
                    let before = visible.clone();
                    let moved = visible.remove(index);
                    visible.insert(next, moved);
                    if !self.preferences.reduced_motion {
                        // Each tile starts from where it is drawn now: its
                        // remaining glide plus the change of slot. Quick
                        // presses redirect tiles in flight instead of
                        // snapping them back to a slot first.
                        let mut offsets = self.glide_offsets();
                        for (new, id) in visible.iter().enumerate() {
                            let old = before.iter().position(|item| item == id).unwrap_or(new);
                            let (ox, oy) = self.slot_position(old);
                            let (nx, ny) = self.slot_position(new);
                            let (dx, dy) = offsets.get(id).copied().unwrap_or_default();
                            let offset = (dx + ox - nx, dy + oy - ny);
                            if offset.0.abs() + offset.1.abs() > 0.5 {
                                offsets.insert(id.clone(), offset);
                            } else {
                                offsets.remove(id);
                            }
                        }
                        self.rearranged = Some((Instant::now(), offsets));
                    }
                    for (slot, item) in slots.iter().zip(visible) {
                        ids[*slot] = item;
                    }
                    self.preferences.favorites = Some(ids);
                    self.unsaved_order = true;
                    self.dirty = true;
                    return vec![];
                }
            }
            return vec![];
        }
        if self.overlay.is_none() {
            if self.page == Page::Home
                && let Target::App(id) = &self.focus
            {
                let apps = self.apps();
                if let Some(index) = apps.iter().position(|app| &app.id == id) {
                    match direction {
                        Direction::Left | Direction::Right => {
                            let next = if matches!(direction, Direction::Left) {
                                index.saturating_sub(1)
                            } else {
                                (index + 1).min(apps.len() - 1)
                            };
                            let id = apps[next].id.clone();
                            self.focus(Target::App(id), controls);
                        }
                        Direction::Up => self.focus(Target::Nav(0), controls),
                        Direction::Down => {
                            let x = controls
                                .iter()
                                .find(|control| control.target == self.focus)
                                .map_or(960., |control| control.rect.center().0);
                            let top = controls
                                .iter()
                                .filter(|control| matches!(control.target, Target::Input(_)))
                                .map(|control| control.rect.y)
                                .fold(f32::INFINITY, f32::min);
                            let next = controls
                                .iter()
                                .filter(|control| {
                                    matches!(control.target, Target::Input(_))
                                        && (control.rect.y - top).abs() < 1.
                                })
                                .min_by(|a, b| {
                                    (a.rect.center().0 - x)
                                        .abs()
                                        .total_cmp(&(b.rect.center().0 - x).abs())
                                })
                                .map(|control| control.target.clone());
                            if let Some(next) = next {
                                self.focus(next, controls);
                            }
                        }
                    }
                    return vec![];
                }
            }
            if let Target::Choice(index) = self.focus {
                let count = match self.subpage {
                    Some(Subpage::Clock | Subpage::Temperature) => 2,
                    Some(Subpage::Hidden) => self.hidden_count().max(1),
                    Some(Subpage::Avatar) => self.avatar_choices().len().max(1),
                    _ => 3,
                };
                match direction {
                    Direction::Left => self.close_subpage(),
                    Direction::Up => self.focus(
                        if index == 0 {
                            Target::Nav(2)
                        } else {
                            Target::Choice(index - 1)
                        },
                        controls,
                    ),
                    Direction::Down => self.focus(Target::Choice((index + 1) % count), controls),
                    Direction::Right => {}
                }
                return vec![];
            }
            if let Target::Sidebar(index) = self.focus {
                match direction {
                    Direction::Right => {
                        let entry = self.section_entry();
                        self.focus(entry, controls);
                    }
                    Direction::Up => self.focus(
                        if index == 0 {
                            Target::Nav(2)
                        } else {
                            Target::Sidebar(index - 1)
                        },
                        controls,
                    ),
                    Direction::Down => {
                        self.focus(Target::Sidebar((index + 1) % SECTION_COUNT), controls)
                    }
                    Direction::Left => {}
                }
                return vec![];
            }
            if matches!(
                self.focus,
                Target::Setting(1)
                    | Target::InputMode(_)
                    | Target::WeatherLocation
                    | Target::WeatherTemperature
            ) && matches!(direction, Direction::Right)
            {
                return self.activate();
            }
            if matches!(self.focus, Target::Setting(_) | Target::WeatherToggle)
                && matches!(direction, Direction::Right)
            {
                return vec![];
            }
            if let Target::InputMode(id) = &self.focus
                && matches!(direction, Direction::Up)
                && self
                    .inventory
                    .inputs
                    .first()
                    .is_some_and(|input| &input.id == id)
            {
                self.focus(Target::Sidebar(INPUTS_SECTION), controls);
                return vec![];
            }
            if self.focus == Target::Setting(0) && matches!(direction, Direction::Up) {
                self.focus(Target::Nav(2), controls);
                return vec![];
            }
            if self.focus == Target::WeatherToggle && matches!(direction, Direction::Up) {
                self.focus(Target::Nav(2), controls);
                return vec![];
            }
            if matches!(
                self.focus,
                Target::WeatherQuery | Target::WeatherSearch | Target::WeatherClear
            ) {
                match direction {
                    Direction::Left => {
                        self.close_subpage();
                        return vec![];
                    }
                    Direction::Up => {
                        self.focus(
                            if self.preferences.weather_location.is_some()
                                && self.focus != Target::WeatherClear
                            {
                                Target::WeatherClear
                            } else {
                                Target::Nav(2)
                            },
                            controls,
                        );
                        return vec![];
                    }
                    Direction::Down => {
                        let target = if self.focus == Target::WeatherClear {
                            Some(Target::WeatherQuery)
                        } else if !self.weather.results.is_empty() {
                            Some(Target::WeatherResult(0))
                        } else if self.focus == Target::WeatherQuery {
                            Some(Target::WeatherSearch)
                        } else {
                            None
                        };
                        if let Some(target) = target {
                            self.focus(target, controls);
                        }
                        return vec![];
                    }
                    _ => {}
                }
            }
            if let Target::WeatherResult(index) = self.focus {
                match direction {
                    Direction::Left => self.close_subpage(),
                    Direction::Up => self.focus(
                        if index == 0 {
                            Target::WeatherQuery
                        } else {
                            Target::WeatherResult(index - 1)
                        },
                        controls,
                    ),
                    Direction::Down if index + 1 < self.weather.results.len() => {
                        self.focus(Target::WeatherResult(index + 1), controls)
                    }
                    _ => {}
                }
                return vec![];
            }
            if let Target::Nav(index) = self.focus {
                let next = match direction {
                    Direction::Left if index > 0 => Some(index - 1),
                    Direction::Right if index < 2 => Some(index + 1),
                    _ => None,
                };
                if let Some(next) = next {
                    self.open(next);
                    return vec![];
                }
            }
            if self.page == Page::Apps
                && let Target::App(id) = &self.focus
                && matches!(direction, Direction::Up | Direction::Down)
            {
                let apps = self.apps();
                if let Some(index) = apps.iter().position(|app| &app.id == id) {
                    // A preceding row may be above the visible scroll region.
                    // Its grid index still defines the route; reveal_focus then
                    // scrolls it into view, without letting the fixed nav steal Up.
                    let next = match direction {
                        // tvOS: Up from the dock shows Top Shelf full screen
                        // (Down returns); the time pill above is never focused.
                        // Only apps with Top Shelf art go full screen: a lone
                        // wallpaper without the dock would only confuse.
                        Direction::Up
                            if index < APPS_COLUMNS && (self.expanded || !self.has_art(id)) =>
                        {
                            None
                        }
                        Direction::Up if index < APPS_COLUMNS => {
                            self.expanded = true;
                            self.shelf_since = None;
                            self.reveal_focus(controls);
                            return vec![];
                        }
                        Direction::Down if self.expanded => {
                            self.expanded = false;
                            self.reveal_focus(controls);
                            return vec![];
                        }
                        Direction::Up => Some(Target::App(apps[index - APPS_COLUMNS].id.clone())),
                        Direction::Down
                            if (index / APPS_COLUMNS + 1) * APPS_COLUMNS < apps.len() =>
                        {
                            Some(Target::App(
                                apps[(index + APPS_COLUMNS).min(apps.len() - 1)].id.clone(),
                            ))
                        }
                        _ => None,
                    };
                    if let Some(next) = next {
                        self.focus(next, controls);
                    }
                    return vec![];
                }
            }
            if let Target::Background(index) = self.focus {
                match direction {
                    Direction::Right if index + 1 < BACKGROUNDS.len() => {
                        self.focus(Target::Background(index + 1), controls);
                        return vec![];
                    }
                    Direction::Left if index % 3 != 0 => {
                        self.focus(Target::Background(index - 1), controls);
                        return vec![];
                    }
                    _ => {}
                }
            }
            if matches!(self.focus, Target::Nav(_)) && matches!(direction, Direction::Down) {
                if self.page == Page::Settings {
                    self.subpage = None;
                    self.section = 0;
                }
                self.focus(self.first_content(), controls);
                return vec![];
            }
            if self.page == Page::Settings
                && matches!(direction, Direction::Left)
                && !matches!(
                    self.focus,
                    Target::Nav(_) | Target::Sidebar(_)
                )
            {
                if let Target::Background(index) = self.focus {
                    if index % 3 != 0 { /* left follows gallery order */
                    } else {
                        self.focus(Target::Sidebar(self.section), controls);
                        return vec![];
                    }
                } else {
                    self.focus(Target::Sidebar(self.section), controls);
                    return vec![];
                }
            }
        }
        let Some(current) = controls.iter().find(|c| c.target == self.focus) else {
            self.focus(self.first_content(), controls);
            return vec![];
        };
        let (x, y) = current.rect.center();
        let next = controls
            .iter()
            .filter(|candidate| candidate.target != self.focus)
            // A notification is never a stop for the remote's arrows (the
            // pointer can still click it away).
            .filter(|candidate| candidate.target != Target::DismissNotice)
            .filter(|candidate| {
                self.page != Page::Home
                    || !matches!(candidate.target, Target::App(_))
                    || (candidate.rect.x >= HOME_STRIP_LEFT - 0.1
                        && candidate.rect.x + candidate.rect.w <= HOME_STRIP_RIGHT + 0.1)
            })
            .filter_map(|candidate| {
                // Header controls never steal an Up route from the active tab.
                if self.overlay.is_none()
                    && matches!(direction, Direction::Up)
                    && candidate.rect.y < 150.
                    && candidate.target != Target::Nav(self.page_index())
                {
                    return None;
                }
                let (cx, cy) = candidate.rect.center();
                let (primary, cross, overlap) = match direction {
                    Direction::Up => (
                        y - cy,
                        (x - cx).abs(),
                        (current.rect.w + candidate.rect.w) / 2.,
                    ),
                    Direction::Down => (
                        cy - y,
                        (x - cx).abs(),
                        (current.rect.w + candidate.rect.w) / 2.,
                    ),
                    Direction::Left => (
                        x - cx,
                        (y - cy).abs(),
                        (current.rect.h + candidate.rect.h) / 2.,
                    ),
                    Direction::Right => (
                        cx - x,
                        (y - cy).abs(),
                        (current.rect.h + candidate.rect.h) / 2.,
                    ),
                };
                (primary > 1.).then_some((
                    candidate.target.clone(),
                    primary + cross * 2. + if cross >= overlap { 10000. } else { 0. },
                ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(target, _)| target);
        if let Some(next) = next {
            self.focus(next, controls);
        }
        vec![]
    }

    pub fn options(&mut self) {
        if self.overlay.is_some() || self.moving.is_some() {
            return;
        }
        if let Target::App(id) = &self.focus {
            self.options_closed = None;
            self.overlay = Some(Overlay::Options {
                id: id.clone(),
                restore: self.focus.clone(),
            });
            self.focus = Target::Option(0);
            if !self.preferences.reduced_motion {
                self.panel_started = Some(Instant::now());
            }
            self.dirty = true;
        }
    }

    /// Holding OK on an app starts Move at once, as on tvOS; elsewhere it
    /// opens the options menu.
    pub fn hold(&mut self) {
        if self.page == Page::Apps
            && self.overlay.is_none()
            && self.moving.is_none()
            && let Target::App(id) = self.focus.clone()
        {
            self.start_move(&id);
        } else {
            self.options();
        }
    }

    /// Enter Move for `id`. The home order becomes explicit (exactly as now
    /// shown, so nothing jumps); the dock is simply its first row.
    pub fn start_move(&mut self, id: &str) {
        let order: Vec<String> = self.apps().iter().map(|app| app.id.clone()).collect();
        if !order.iter().any(|item| item == id) {
            return;
        }
        self.preferences.favorites = Some(order);
        self.moving = Some(id.into());
        self.moving_since = (!self.preferences.reduced_motion).then(Instant::now);
        // Say how Move works, once, in the banner.
        self.notify(i18n::t("moveHint"));
        self.focus = Target::App(id.into());
        self.dirty = true;
    }

    pub fn text(&mut self, text: &str) {
        if self.text_input_active() {
            if self.keyboard_field == TextField::Location {
                let mut query = self.weather.query.clone();
                query.extend(
                    text.chars()
                        .filter(|c| !c.is_control())
                        .take(120usize.saturating_sub(query.chars().count())),
                );
                self.weather.edit(query);
                self.dirty = true;
                return;
            }
            self.query.extend(
                text.chars()
                    .filter(|c| !c.is_control())
                    .take(120usize.saturating_sub(self.query.chars().count())),
            );
            self.scroll = 0.;
            self.dirty = true;
        }
    }

    pub fn delete(&mut self) {
        if self.text_input_active() {
            if self.keyboard_field == TextField::Location {
                let mut query = self.weather.query.clone();
                query.pop();
                self.weather.edit(query);
            } else {
                self.query.pop();
            }
            self.dirty = true;
        }
    }

    pub fn text_input_active(&self) -> bool {
        matches!(
            self.overlay,
            Some(Overlay::Keyboard | Overlay::SystemKeyboard)
        )
    }

    pub fn finish_text_input(&mut self) {
        if self.text_input_active() {
            self.overlay = None;
            self.panel_started = None;
            self.focus = self.keyboard_return();
            self.pointer = false;
            self.dirty = true;
        }
    }

    pub fn delete_surrounding_text(&mut self, offset: i32, length: i32) {
        if !self.text_input_active() || length <= 0 {
            return;
        }
        // SDL-webOS forwards the native text model's UTF-8 byte offsets,
        // relative to the insertion cursor (at the end of our query).
        let mut query = self.editing_text().to_owned();
        let mut start =
            (query.len() as i64 + i64::from(offset)).clamp(0, query.len() as i64) as usize;
        let mut end = start.saturating_add(length as usize).min(query.len());
        while !query.is_char_boundary(start) {
            start -= 1;
        }
        while !query.is_char_boundary(end) {
            end += 1;
        }
        query.replace_range(start..end, "");
        if self.keyboard_field == TextField::Location {
            self.weather.edit(query);
        } else {
            self.query = query;
            self.scroll = 0.;
        }
        self.dirty = true;
    }

    pub fn editing_text(&self) -> &str {
        if self.keyboard_field == TextField::Location {
            &self.weather.query
        } else {
            &self.query
        }
    }
    pub fn open_keyboard(&mut self, field: TextField) {
        self.keyboard_field = field;
        self.search_closed = None;
        // App search uses Luma's own tvOS keyboard everywhere; place names may
        // need the TV's full system keyboard.
        if field == TextField::Location
            && cfg!(all(
                feature = "webos",
                target_os = "linux",
                target_arch = "arm"
            ))
        {
            self.overlay = Some(Overlay::SystemKeyboard);
            self.focus = if field == TextField::Location {
                Target::WeatherQuery
            } else {
                Target::Search
            };
            self.panel_started = None;
            self.dirty = true;
            return;
        }
        self.overlay = Some(Overlay::Keyboard);
        self.focus = Target::Key(0);
        if !self.preferences.reduced_motion {
            self.panel_started = Some(Instant::now());
        }
        self.dirty = true;
    }
    fn open_location(&mut self) {
        self.weather.edit(String::new());
        // A fresh page: no answer or message left from the last visit.
        self.weather.cancel_search();
        self.subpage = Some(Subpage::WeatherLocation);
        self.focus = Target::WeatherQuery;
        self.scroll = 0.;
        if !self.preferences.reduced_motion {
            self.panel_started = Some(Instant::now());
        }
        self.dirty = true;
    }
    fn keyboard_return(&self) -> Target {
        if self.keyboard_field == TextField::Location {
            Target::WeatherSearch
        } else {
            Target::App(SEARCH_TILE.into())
        }
    }

    pub fn activate(&mut self) -> Vec<Effect> {
        if matches!(self.overlay, Some(Overlay::Keyboard)) && matches!(self.focus, Target::App(_)) {
            // A live search result was chosen: leave the keyboard and open it.
            self.overlay = None;
            self.panel_started = None;
        }
        if let (Some(switcher), Target::Switch(index)) = (&self.switcher, &self.focus) {
            let id = switcher.apps.get(*index).cloned();
            let restore = switcher.restore.clone();
            self.switcher = None;
            self.focus = restore;
            self.dirty = true;
            return id.map_or_else(Vec::new, |id| vec![Effect::Launch(id)]);
        }
        if self.moving.take().is_some() {
            // The order is saved as Move ends (see `unsaved_order`). A fresh OK
            // release finishes the mode and keeps the selected favorite,
            // rather than launching it.
            self.notice = None;
            self.dirty = true;
            return vec![];
        }
        self.dirty = true;
        match self.focus.clone() {
            Target::Nav(index) => self.open(index),
            Target::Browse => self.open(1),
            Target::Retry => return vec![Effect::Refresh],
            Target::DismissNotice => self.notice = None,
            Target::App(id) if id == SETTINGS_TILE => {
                self.open(2);
                self.focus = Target::Sidebar(0);
                return vec![];
            }
            Target::App(id) if id == SEARCH_TILE => {
                self.open_keyboard(TextField::Apps);
                return vec![];
            }
            Target::App(id) | Target::Input(id) if !self.launching => {
                return vec![Effect::Launch(id)];
            }
            Target::Search => {
                self.open_keyboard(TextField::Apps);
            }
            Target::WeatherConfigure => {
                self.open(2);
                self.section = WEATHER_SECTION;
                self.open_location();
                self.open_keyboard(TextField::Location);
            }
            Target::WeatherToggle => {
                self.preferences.show_weather = !self.preferences.show_weather;
                self.weather.invalidate_forecast();
                return vec![Effect::Save];
            }
            Target::WeatherLocation => self.open_location(),
            Target::WeatherTemperature => {
                self.subpage = Some(Subpage::Temperature);
                self.focus = Target::Choice(usize::from(
                    self.preferences.temperature_unit == Unit::Fahrenheit,
                ));
                if !self.preferences.reduced_motion {
                    self.panel_started = Some(Instant::now());
                }
            }
            Target::WeatherQuery => self.open_keyboard(TextField::Location),
            Target::WeatherSearch => {
                return self
                    .weather
                    .search()
                    .map(|command| vec![Effect::Weather(command)])
                    .unwrap_or_default();
            }
            Target::WeatherClear => {
                self.preferences.weather_location = None;
                self.weather.remove_location();
                self.focus = Target::WeatherSearch;
                return vec![Effect::Save, Effect::Weather(weather::Command::ClearCache)];
            }
            Target::WeatherResult(index) => {
                if let Some(place) = self.weather.results.get(index).cloned() {
                    self.preferences.weather_location = Some(place);
                    self.weather.invalidate_forecast();
                    self.weather.cancel_search();
                    self.focus = Target::WeatherSearch;
                    return vec![Effect::Save];
                }
            }
            Target::Sidebar(index) => {
                self.section = index;
                self.scroll = 0.;
                self.subpage = None;
                self.focus = self.section_entry();
            }
            Target::Setting(index) => {
                match index {
                    0 => self.preferences.show_names = !self.preferences.show_names,
                    1 => {
                        self.subpage = Some(Subpage::Clock);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(Instant::now());
                        }
                        self.focus = Target::Choice(usize::from(self.preferences.hour_24));
                        return vec![];
                    }
                    2 => self.preferences.reduced_motion = !self.preferences.reduced_motion,
                    6 => {
                        self.preferences.top_shelf_art = !self.preferences.top_shelf_art;
                        self.expanded = false;
                    }
                    5 => return vec![Effect::Quit],
                    8 => {
                        self.subpage = Some(Subpage::Avatar);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(Instant::now());
                        }
                        let current = self.avatar_choice();
                        let index = self
                            .avatar_choices()
                            .iter()
                            .position(|choice| Some(choice) == current.as_ref())
                            .unwrap_or(0);
                        self.focus = Target::Choice(index);
                        return vec![];
                    }
                    7 => {
                        self.subpage = Some(Subpage::Hidden);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(Instant::now());
                        }
                        self.focus = Target::Choice(0);
                        return vec![];
                    }
                    _ => return vec![Effect::Refresh],
                }
                return vec![Effect::Save];
            }
            Target::Background(index) => {
                if !self.preferences.reduced_motion
                    && self.preferences.background != BACKGROUNDS[index].0
                {
                    self.previous_background =
                        Some((self.preferences.background.clone(), Instant::now()));
                }
                self.preferences.background = BACKGROUNDS[index].0.into();
                return vec![Effect::Save];
            }
            Target::InputMode(id) => {
                let mode = self
                    .preferences
                    .input_modes
                    .get(&id)
                    .copied()
                    .unwrap_or_default();
                self.subpage = Some(Subpage::Input(id));
                if !self.preferences.reduced_motion {
                    self.panel_started = Some(Instant::now());
                }
                self.focus = Target::Choice(match mode {
                    InputMode::On => 0,
                    InputMode::Off => 1,
                    InputMode::Auto => 2,
                });
            }
            Target::Choice(index) => {
                match self.subpage.clone() {
                    Some(Subpage::Clock) => self.preferences.hour_24 = index == 1,
                    Some(Subpage::Input(id)) => {
                        self.preferences.input_modes.insert(
                            id,
                            [InputMode::On, InputMode::Off, InputMode::Auto][index.min(2)],
                        );
                        self.refresh_tiles();
                    }
                    Some(Subpage::Temperature) => {
                        self.preferences.temperature_unit = if index == 1 {
                            Unit::Fahrenheit
                        } else {
                            Unit::Celsius
                        }
                    }
                    Some(Subpage::Avatar) => {
                        if let Some(choice) = self.avatar_choices().get(index).cloned() {
                            self.preferences.avatar = Some(choice);
                            self.show_avatar(true);
                        }
                    }
                    Some(Subpage::Hidden) => {
                        // Showing an app puts it back where it was; the list
                        // closes once nothing is left in it.
                        let Some(app) = self.hidden_apps().get(index).map(|app| (app.id.clone(), app.title.clone()))
                        else {
                            return vec![];
                        };
                        self.preferences.hidden.retain(|item| item != &app.0);
                        self.notify(i18n::format("appRestored", &[("app", &app.1)]));
                        let left = self.hidden_count();
                        if left > 0 {
                            self.focus = Target::Choice(index.min(left - 1));
                            self.dirty = true;
                            return vec![Effect::Save];
                        }
                    }
                    Some(Subpage::WeatherLocation) => return vec![],
                    None => return vec![],
                }
                self.close_subpage();
                return vec![Effect::Save];
            }
            Target::Option(index) => {
                let Some(Overlay::Options { id, restore }) = self.overlay.take() else {
                    return vec![];
                };
                self.panel_started = None;
                if !self.preferences.reduced_motion {
                    self.options_closed = Some((id.clone(), Instant::now()));
                }
                let old_ids: Vec<_> = self.apps().iter().map(|a| a.id.clone()).collect();
                let old_index = old_ids.iter().position(|item| item == &id).unwrap_or(0);
                let title = self
                    .inventory
                    .apps
                    .iter()
                    .find(|app| app.id == id)
                    .map_or(id.clone(), |app| app.title.clone());
                if index == 0 {
                    // tvOS "Move": any app, anywhere on the home screen.
                    self.start_move(&id);
                    return vec![Effect::Save];
                }
                if index == 1 {
                    if self.preferences.hidden.contains(&id) {
                        self.preferences.hidden.retain(|item| item != &id);
                        self.notify(i18n::format("appRestored", &[("app", &title)]));
                    } else {
                        self.preferences.hidden.push(id.clone());
                        self.notify(i18n::format("appHidden", &[("app", &title)]));
                    }
                }
                self.focus = restore;
                self.pointer = false;
                if let Target::App(id) = &self.focus {
                    let apps = self.apps();
                    if !apps.iter().any(|app| &app.id == id) {
                        self.focus = apps
                            .get(old_index.min(apps.len().saturating_sub(1)))
                            .map(|app| Target::App(app.id.clone()))
                            .unwrap_or_else(|| self.first_content());
                    }
                }
                return vec![Effect::Save];
            }
            Target::Key(index) => match KEYS[index] {
                "Space" => self.text(" "),
                "Delete" => self.delete(),
                "Clear" => {
                    if self.keyboard_field == TextField::Location {
                        self.weather.edit(String::new());
                    } else {
                        self.query.clear();
                    }
                }
                "Done" => {
                    self.overlay = None;
                    self.panel_started = None;
                    self.focus = self.keyboard_return();
                    self.search_closed = (!self.preferences.reduced_motion).then(Instant::now);
                }
                key => self.text(key),
            },
            _ => {}
        }
        vec![]
    }

    /// Where home slot `index` sits relative to the first, in pixels: the dock
    /// is row 0, the grid's rows follow it (their gap opens as the grid rises).
    pub fn slot_position(&self, index: usize) -> (f32, f32) {
        let (row, column) = (index / APPS_COLUMNS, index % APPS_COLUMNS);
        let spread = (self.shown_scroll / GRID_SCROLL).clamp(0., 1.);
        let gap = DOCK_GAP + (GRID_GAP - DOCK_GAP) * spread;
        let y = if row == 0 { 0. } else { gap + (row - 1) as f32 * ROW_PITCH };
        (column as f32 * GRID_STEP, y)
    }

    /// How far each gliding tile is drawn from its slot right now.
    pub fn glide_offsets(&self) -> Glide {
        let Some((at, offsets)) = &self.rearranged else {
            return Default::default();
        };
        let left = 1. - Self::glide_eased(*at);
        offsets
            .iter()
            .map(|(id, (dx, dy))| (id.clone(), (dx * left, dy * left)))
            .collect()
    }

    /// How far the app in Move has lifted out of the grid (0-1): quickly at
    /// first, settling softly, as tvOS raises it.
    pub fn move_raise(&self) -> f32 {
        self.moving_since.map_or(1., |since| {
            let t = (since.elapsed().as_secs_f32() / 0.16).min(1.);
            1. - (1. - t).powi(3)
        })
    }

    /// Progress (0-1) of a Move glide that began at `at`: an ease-out cubic.
    pub fn glide_eased(at: Instant) -> f32 {
        let t = (at.elapsed().as_secs_f32() * 1000. / REARRANGE_MS).min(1.);
        1. - (1. - t).powi(3)
    }

    /// Profile picture choices, as the preference names them: "rotate" (when
    /// there is more than one picture) and then every picture's file.
    pub fn avatar_choices(&self) -> Vec<String> {
        let rotate = (self.avatars.len() > 1).then(|| AVATAR_ROTATE.to_owned());
        rotate.into_iter().chain(self.avatars.iter().map(|picture| picture.file.clone())).collect()
    }

    /// The choice in effect: the preference, or the first picture by default.
    pub fn avatar_choice(&self) -> Option<String> {
        self.preferences
            .avatar
            .clone()
            .or_else(|| self.avatars.first().map(|picture| picture.file.clone()))
    }

    pub fn set_avatars(&mut self, pictures: Vec<crate::avatars::Picture>) {
        self.avatars = pictures;
        self.show_avatar(false);
    }

    /// Shows the chosen picture (or, rotating, the current one), fading from
    /// the last when `fade`.
    fn show_avatar(&mut self, fade: bool) {
        let chosen = self.preferences.avatar.as_deref();
        if chosen != Some(AVATAR_ROTATE) {
            self.avatar_index = chosen
                .and_then(|file| self.avatars.iter().position(|picture| picture.file == file))
                .unwrap_or(0);
        }
        let next = self.avatars.get(self.avatar_index).map(|picture| picture.small.clone());
        if fade && !self.preferences.reduced_motion && next != self.avatar
            && let Some(previous) = self.avatar.clone()
        {
            self.avatar_from = Some((previous, Instant::now()));
        }
        self.avatar = next;
        self.avatar_since = Instant::now();
        self.dirty = true;
    }

    /// Rotating pictures change every AVATAR_SECONDS, crossfading.
    fn rotate_avatar(&mut self) {
        if self.preferences.avatar.as_deref() == Some(AVATAR_ROTATE)
            && self.avatars.len() > 1
            && self.avatar_since.elapsed().as_secs_f32() >= AVATAR_SECONDS
        {
            self.avatar_index = (self.avatar_index + 1) % self.avatars.len();
            self.show_avatar(true);
        }
        if self.avatar_from.as_ref().is_some_and(|(_, since)| since.elapsed().as_secs_f32() >= AVATAR_FADE) {
            self.avatar_from = None;
            self.dirty = true;
        }
    }

    /// The switcher fills the screen: open long enough to have faded in, and
    /// not leaving. Nothing beneath it needs drawing then.
    pub fn switcher_covers(&self) -> bool {
        self.switcher.as_ref().is_some_and(|switcher| {
            switcher.opened.elapsed().as_secs_f32() >= 0.3 && switcher.dismissed.is_none()
        })
    }

    /// The remote's Home key. Pressed with Luma in front it is tvOS's Home:
    /// whatever is open closes and the home screen returns to its top. On the
    /// way back from an app the home stays as it was left, with anything that
    /// was still open closed.
    pub fn home_key(&mut self, in_front: bool) {
        let open = self.page == Page::Settings
            || self.overlay.is_some()
            || self.switcher.is_some()
            || self.moving.is_some()
            || self.expanded;
        if !open && !in_front {
            return;
        }
        if self.moving.take().is_some() {
            self.notice = None;
        }
        self.switcher = None;
        self.expanded = false;
        self.open(1);
    }

    /// The remote's Recent key: the App Switcher, from the home screen only
    /// (inside a panel it would stack one overlay on another).
    pub fn recent(&mut self) -> Vec<Effect> {
        self.dirty = true;
        if self.page == Page::Apps
            && self.overlay.is_none()
            && self.switcher.is_none()
            && self.moving.is_none()
        {
            vec![Effect::Running]
        } else {
            vec![]
        }
    }

    pub fn back(&mut self) -> Vec<Effect> {
        self.pointer = false;
        self.dirty = true;
        if let Some(switcher) = &mut self.switcher
            && switcher.dismissed.is_none()
        {
            // The home is live at once; the switcher fades away over it.
            switcher.dismissed = Some(Instant::now());
            self.focus = switcher.restore.clone();
            return vec![];
        }
        if self.moving.take().is_some() {
            self.notice = None;
            return vec![];
        }
        if let Some(overlay) = self.overlay.take() {
            self.panel_started = None;
            if let Overlay::Options { id, .. } = &overlay
                && !self.preferences.reduced_motion
            {
                self.options_closed = Some((id.clone(), Instant::now()));
            }
            self.focus = match overlay {
                Overlay::Options { restore, .. } => restore,
                Overlay::Keyboard | Overlay::SystemKeyboard => {
                    self.search_closed = (!self.preferences.reduced_motion
                        && matches!(overlay, Overlay::Keyboard))
                    .then(Instant::now);
                    self.keyboard_return()
                }
            };
            return vec![];
        }
        if self.page == Page::Apps && self.expanded {
            // Focus stays on its dock app as Top Shelf lowers back.
            self.expanded = false;
            self.scroll = 0.;
            return vec![];
        }
        if self.page == Page::Settings && self.subpage.is_some() {
            self.close_subpage();
            return vec![];
        }
        if self.page == Page::Settings && !matches!(self.focus, Target::Nav(_)) {
            if matches!(self.focus, Target::Sidebar(_)) {
                self.open(1);
                return vec![];
            } else {
                self.focus = Target::Sidebar(self.section);
            }
            self.scroll = 0.;
            return vec![];
        }
        if self.page == Page::Settings {
            self.open(1);
            return vec![];
        }
        if self.page == Page::Apps && self.focus == Target::Search {
            self.query.clear();
            self.focus = Target::App(SEARCH_TILE.into());
            self.scroll = 0.;
            return vec![];
        }
        // Back on Home opens the app switcher, as on tvOS. Quit lives in Settings.
        vec![Effect::Running]
    }
}
