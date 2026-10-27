//! The launcher's state: what is on screen, what is focused, and what each
//! remote press does. The behaviour is split by concern into the modules
//! below; this module holds the types, the per-frame tick and the app list.
mod actions;
mod focus;
mod looks;
mod navigation;
mod rearrange;
mod switcher;
mod text;

use crate::i18n;
use crate::model::{App, Appearance, BACKGROUNDS, InputMode, Inventory, Preferences};
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
pub const SETTINGS_TILE: &str = "lounge.settings";
pub const SEARCH_TILE: &str = "lounge.search";
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
    let t = (crate::clock::since(start).as_secs_f32() * 1000. / FOCUS_MS).min(1.);
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
    /// An alert's buttons, top to bottom (the action, then Cancel).
    Alert(usize),
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
    Options {
        id: String,
        restore: Target,
    },
    Keyboard,
    SystemKeyboard,
    /// tvOS's alert before removing an app: Uninstall over Cancel, Cancel
    /// focused. Back is Cancel.
    Uninstall {
        id: String,
        restore: Target,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Subpage {
    Clock,
    Input(String),
    WeatherLocation,
    Temperature,
    Hidden,
    Avatar,
    Appearance,
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
    /// Give the remote's Home key to Lounge (true) or back to LG, or (None)
    /// read which it is.
    HomeButton(Option<bool>),
    /// Remove an installed app from the TV.
    Uninstall(String),
    /// Set the TV's clock to 24-hour (true) or 12-hour.
    ClockFormat(bool),
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
    /// Whether the remote's Home key opens Lounge; None until the TV has
    /// said (and always on the desktop, which has no Home key to set).
    pub home_button: Option<bool>,
    /// A Home key change is on its way to the TV.
    pub home_button_busy: bool,
    /// Where the focused tile sits in the grid, so focus can go to the tile
    /// that takes its place if it disappears.
    pub focus_slot: usize,
    avatar_index: usize,
    /// When the shown picture last changed, and the one it replaced (it
    /// crossfades out).
    avatar_since: Instant,
    pub avatar_from: Option<(String, Instant)>,
    /// The hour on the TV's clock, for the automatic appearance.
    pub hour: u8,
    /// The appearance on screen (None until the first frame settles it), and
    /// the light share it is cross-fading from.
    light_shown: Option<bool>,
    appearance_from: Option<(f32, Instant)>,
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
    /// Inventory loads that failed in a row while no apps were shown yet.
    pub inventory_failures: u32,
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
    /// Apps on their way off the TV: gone from the grid at once, back only
    /// if webOS refuses (see `uninstalled`).
    pub uninstalling: Vec<String>,
    /// The switcher's window snapshots by app (see `snapshots`).
    pub snapshots: crate::snapshots::Shots,
    /// Open apps whose snapshots the switcher wants read, once it opens.
    pub snapshots_wanted: Option<Vec<String>>,
    /// The TV's own clock format, 24-hour when true: one source of truth
    /// for every clock on the TV, which Settings › General › Clock format sets.
    pub hour_24: bool,
    /// Effects a TV reply asked for, run by the loop after replies.
    pub queued: Vec<Effect>,
}

/// tvOS-style app switcher over Home: open apps as large cards.
/// How long tiles take to glide to their new slots in Move mode, in ms.
/// Each gliding tile's offset from its slot, in pixels, by app.
pub type Glide = std::collections::HashMap<String, (f32, f32)>;
/// Automatic inventory retries before the error shows (with LG's 6.5 s
/// request timeout, about 50 s in all).
pub const INVENTORY_RETRIES: u32 = 5;
pub const INVENTORY_RETRY_DELAY: Duration = Duration::from_secs(2);
/// The appearance choices, in Settings' order (as tvOS lists them).
pub const APPEARANCES: [Appearance; 3] = [Appearance::Light, Appearance::Dark, Appearance::Auto];
/// The avatar preference that shows every picture in turn.
pub const AVATAR_ROTATE: &str = "rotate";
/// Seconds each rotating picture shows, and its crossfade.
pub const AVATAR_SECONDS: f32 = 20.;
pub const AVATAR_FADE: f32 = 0.6;
/// Seconds a change of appearance cross-fades over.
pub const APPEARANCE_FADE: f32 = 0.5;
/// Automatic appearance is light from this hour until DARK_FROM.
pub const LIGHT_FROM: u8 = 6;
pub const DARK_FROM: u8 = 19;
pub const REARRANGE_MS: f32 = 240.;

/// How long an overlay (context menu, search) takes to open, in milliseconds.
pub const OVERLAY_OPEN_MS: u64 = 220;
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
            home_button: None,
            home_button_busy: false,
            focus_slot: 0,
            avatar_index: 0,
            avatar_since: crate::clock::now(),
            avatar_from: None,
            hour: 12,
            light_shown: None,
            appearance_from: None,
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
            inventory_failures: 0,
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
            last_tick: crate::clock::now(),
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
            shelf_since: Some(crate::clock::now()),
            shelf_app: None,
            shelf_pending: None,
            shelf_from: None,
            heroes: Vec::new(),
            uninstalling: Vec::new(),
            snapshots: Default::default(),
            snapshots_wanted: None,
            hour_24: false,
            queued: Vec::new(),
        }
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), crate::clock::now()));
        self.dirty = true;
    }

    pub fn tick(&mut self) {
        self.track_focus();
        self.glide_scroll();
        if let Some(switcher) = &mut self.switcher {
            // Keep presenting frames while the switcher fades in or a card is
            // being dismissed; otherwise the first frame would freeze on screen.
            let before = switcher.closing.len();
            switcher.closing.retain(|(_, start, _)| {
                crate::clock::since(start) < Duration::from_millis(SWITCHER_CLOSE_MS)
            });
            if switcher.reflow.is_some_and(|(_, start)| {
                crate::clock::since(start) >= Duration::from_millis(SWITCHER_REFLOW_MS)
            }) {
                switcher.reflow = None;
            }
            if before != switcher.closing.len()
                || !switcher.closing.is_empty()
                || switcher.reflow.is_some()
                || switcher.dismissed.is_some()
                || crate::clock::since(switcher.opened) < Duration::from_millis(320)
            {
                self.dirty = true;
            }
            if switcher.dismissed.is_some_and(|start| {
                crate::clock::since(start) >= Duration::from_millis(SWITCHER_DISMISS_MS)
            }) && switcher.closing.is_empty()
            {
                self.switcher = None;
            }
        }
        if let Some((start, _)) = self.page_since {
            self.dirty = true;
            if crate::clock::since(start).as_secs_f32() >= PAGE_MS / 1000. {
                self.page_since = None;
            }
        }
        self.track_settings_level();
        // Move's glide and lift are drawn on every frame while they run (only
        // marking the end left the tiles frozen in place, then jumping).
        if let Some((at, _)) = &self.rearranged {
            if crate::clock::since(at).as_secs_f32() * 1000. >= REARRANGE_MS {
                self.rearranged = None;
            }
            self.dirty = true;
        }
        if self.moving.is_some() && self.move_raise() < 1. {
            self.dirty = true;
        }
        if self.settings_since.is_some_and(|(start, _)| {
            crate::clock::since(start).as_secs_f32() * 1000. >= SETTINGS_PUSH_MS
        }) {
            self.settings_since = None;
            self.settings_ghost = None;
            self.dirty = true;
        }
        if self
            .search_closed
            .is_some_and(|start| crate::clock::since(start) >= Duration::from_millis(FADE_OUT_MS))
        {
            self.search_closed = None;
            self.dirty = true;
        }
        if self.options_closed.as_ref().is_some_and(|(_, start)| {
            crate::clock::since(start) >= Duration::from_millis(FADE_OUT_MS)
        }) {
            self.options_closed = None;
            self.dirty = true;
        }
        if let Some(start) = self.panel_started {
            self.dirty = true;
            let duration = if self.overlay.is_some() {
                OVERLAY_OPEN_MS
            } else {
                150
            };
            if self.preferences.reduced_motion
                || crate::clock::since(start) >= Duration::from_millis(duration)
            {
                self.panel_started = None;
            }
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, start)| crate::clock::since(start) < Duration::from_millis(120))
            && !self.preferences.reduced_motion
        {
            self.dirty = true;
        }
        if let Some((_, start)) = &self.previous_background {
            self.dirty = true;
            if self.preferences.reduced_motion
                || crate::clock::since(start) >= Duration::from_millis(400)
            {
                self.previous_background = None;
            }
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, start)| crate::clock::since(start).as_secs_f32() > NOTICE_SECONDS)
        {
            self.notice = None;
            self.dirty = true;
        }
        self.rotate_avatar();
        self.track_appearance();
        // One rule for every animation: while anything moves, each frame is
        // drawn. Marking only an animation's end froze it on its first frame
        // and then jumped (Move's glide, the Settings push, menu fades).
        if self.animating() {
            self.dirty = true;
        }
    }

    pub fn page_index(&self) -> usize {
        match self.page {
            Page::Home => 0,
            Page::Apps => 1,
            Page::Settings => 2,
        }
    }

    pub fn animating(&self) -> bool {
        let switcher_moving = self.switcher.as_ref().is_some_and(|switcher| {
            crate::clock::since(switcher.opened) < Duration::from_millis(300)
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
            || self.appearance_from.is_some()
            || (self.switcher.is_some() && self.snapshots.fading())
            // Keep ticking until a paused-typing search has been sent.
            || self.weather.search_pending()
            || self.options_closed.is_some()
            || self.search_closed.is_some()
            // The chevron only exists with Top Shelf art; never redraw for it otherwise.
            || (!self.preferences.reduced_motion
                && self.preferences.top_shelf_art
                && self
                    .shelf_since
                    .is_some_and(|since| crate::clock::since(since).as_secs_f32() < SHELF_HINT))
            || (!self.preferences.reduced_motion
                && self
                    .notice
                    .as_ref()
                    .is_some_and(|(_, start)| {
                        let age = crate::clock::since(start).as_secs_f32();
                        !(0.12..=NOTICE_SECONDS - 0.25).contains(&age)
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
                .filter(|id| {
                    !self.preferences.hidden.contains(id) && !self.uninstalling.contains(id)
                })
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
                    && !self.uninstalling.contains(&app.id)
                    && app.title.to_lowercase().contains(&query)
            })
            .collect();
        // Dock favourites first, then Settings, Search and inputs, then A-Z.
        let rank = |app: &App| {
            ids.iter()
                .position(|id| id == &app.id)
                .unwrap_or(match app.brand.as_str() {
                    "lounge-settings" => usize::MAX - 3,
                    "lounge-search" => usize::MAX - 2,
                    "lounge-input" => usize::MAX - 1,
                    _ => usize::MAX,
                })
        };
        // An input the owner has not placed follows Search, so a newly
        // plugged-in device appears in one known place, never at the end of
        // the grid. One that has a place in the saved order (moved there, or
        // shown there when Move began) keeps it like any other app.
        let search = apps
            .iter()
            .find(|app| app.id == SEARCH_TILE)
            .map_or(usize::MAX - 2, |app| rank(app));
        let place = |app: &App| {
            if app.brand == "lounge-input" && !ids.contains(&app.id) {
                // During Move a new device waits at the end: Move works on
                // the saved order, where it has no place yet.
                if self.moving.is_some() {
                    (usize::MAX, 1)
                } else {
                    (search, 1)
                }
            } else {
                (rank(app), 0)
            }
        };
        apps.sort_by(|a, b| {
            place(a).cmp(&place(b)).then_with(|| {
                // Inputs in port order (HDMI 1, 2, …), as LG lists them;
                // everything else A-Z.
                if a.brand == "lounge-input" && b.brand == "lounge-input" {
                    let port = |app: &App| {
                        app.id
                            .trim_start_matches(|c: char| !c.is_ascii_digit())
                            .parse::<u8>()
                            .unwrap_or(u8::MAX)
                    };
                    port(a).cmp(&port(b))
                } else {
                    a.title.to_lowercase().cmp(&b.title.to_lowercase())
                }
            })
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
        self.inventory_failures = 0;
        self.dirty = true;
        self.refresh_tiles();
        self.reconcile_focus();
    }

    /// A failed inventory load. Right after a cold boot, or when the TV is
    /// short of memory, the Homebrew service that reads the inventory can
    /// take longer than one request allows (Lounge now starts early, from LG's
    /// preloader). While no apps are shown yet the screen keeps loading and
    /// the caller retries after the returned delay; the error and its Retry
    /// button only appear once those retries are used up (upstream 945598a).
    pub fn inventory_failed(&mut self, error: String) -> Option<Duration> {
        self.dirty = true;
        // Lounge's own tiles (Settings, Search) are always there; apps are not.
        let no_apps = self
            .inventory
            .apps
            .iter()
            .all(|app| app.brand.starts_with("lounge-"));
        if no_apps && self.inventory_failures < INVENTORY_RETRIES {
            self.inventory_failures += 1;
            return Some(INVENTORY_RETRY_DELAY);
        }
        self.inventory_failures = 0;
        self.loading = false;
        self.error = Some(error);
        None
    }

    /// Settings, Search and visible HDMI inputs live in the grid as tiles, like
    /// tvOS's own Settings and Search icons.
    pub fn refresh_tiles(&mut self) {
        self.inventory
            .apps
            .retain(|app| !app.brand.starts_with("lounge-"));
        let tile = |id: &str, title: &str, brand: &str| App {
            id: id.into(),
            title: title.into(),
            brand: brand.into(),
            category: "Utilities".into(),
            icon: None,
            removable: false,
        };
        let mut tiles = vec![
            tile(SETTINGS_TILE, i18n::t("settings"), "lounge-settings"),
            tile(SEARCH_TILE, i18n::t("search"), "lounge-search"),
        ];
        for input in &self.inventory.inputs {
            if self.preferences.input_visible(input)
                && !self.inventory.apps.iter().any(|app| app.id == input.id)
            {
                // The name the owner gave the input on the TV, else its port.
                tiles.push(tile(&input.id, &input.title, "lounge-input"));
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
            // The tile that now has its place, as when an app is hidden;
            // the top only if the grid is empty.
            let apps = self.apps();
            self.focus = apps
                .get(self.focus_slot.min(apps.len().saturating_sub(1)))
                .map(|app| Target::App(app.id.clone()))
                .unwrap_or_else(|| self.first_content());
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

    /// Whether `id` shows Top Shelf art: the user has it on and art exists.
    pub fn has_art(&self, id: &str) -> bool {
        self.preferences.top_shelf_art && self.heroes.iter().any(|hero| hero == id)
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
}
