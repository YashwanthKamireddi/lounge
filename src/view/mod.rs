//! Screen geometry and styling mirror the web UI's 1920×1080 / 16px-rem layout.
//!
//! The draw-list builder, by screen: `home` (the home page and the apps
//! dock and grid), `settings`, `switcher`, `overlays` (the status pill,
//! context menu, search keyboard, notifications and hints), with the pieces
//! they share in `scene` (the draw list), `tile`, `glass` and `shared`.
mod alert;
mod glass;
mod home;
mod overlays;
mod scene;
mod settings;
mod shared;
mod switcher;
mod theme;
mod tile;

pub use scene::Scene;

use crate::{
    i18n,
    renderer::{Backdrop, Color, Draw, TextStyle, WHITE},
    state::{
        FADE_OUT_MS, GRID_SCROLL, HOME_TILE_PITCH, Overlay, PAGE_MS, Page, Rect, State, Target,
    },
};
use glass::{shelf_art, shelf_dissolve};
use home::{home, library};
use overlays::{footer, header, inputs_panel, keyboard, move_hint, options, toast};
use settings::settings;
use shared::{c, detail_style};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use switcher::switcher;
use time::OffsetDateTime;

const MAIN_TOP: f32 = 108.8;
const FACE_HEIGHT: f32 = 194.;
const GRID_PITCH: f32 = HOME_TILE_PITCH;
// Apple's app icon shape (250x150 at r30) at tvOS 26's five-a-row size:
// 324x194 at r39; focused it grows 1.14x, staying clear of its neighbours
// (the 30 px gap).
const TILE_RADIUS: f32 = 39.;
const FOCUS_SCALE: f32 = 0.14;
const DOCK_RADIUS: f32 = 72.;
const LABEL_GAP: f32 = 26.;
/// Settings' backdrop: the blurred wallpaper at this brightness (home's is
/// 0.62), measured from tvOS 27 Settings over the same wallpaper.
const SETTINGS_VEIL: f32 = 0.32;
/// How much the screen dims beneath a context menu or opening search.
const MENU_DIM: f32 = 0.35;
/// The grid's backdrop: the blurred wallpaper dimmed to this (dark appearance).
const GRID_VEIL: f32 = 0.62;

pub struct Assets {
    pub root: PathBuf,
    pub icons: BTreeMap<String, String>,
    pub colors: BTreeMap<String, [u8; 3]>,
    pub version: String,
    pub metrics: BTreeMap<String, BTreeMap<char, f32>>,
    pub artwork: crate::app_artwork::Service,
    pub tiles: BTreeMap<String, [u8; 3]>,
    pub heroes: Vec<String>,
}
impl Assets {
    pub fn load(root: PathBuf, webos: bool) -> Result<Self, String> {
        let load = |file: &str| {
            std::fs::read(root.join(file))
                .map_err(|e| format!("Native assets are missing: {e}. Run pnpm assets."))
        };
        // Collected artwork is a desktop preview fixture, never a prerequisite
        // for displaying an app installed on the television.
        let icons = if webos {
            BTreeMap::new()
        } else {
            serde_json::from_slice(&load("icons.json")?).map_err(|e| e.to_string())?
        };
        let colors = if webos {
            BTreeMap::new()
        } else {
            serde_json::from_slice(&load("colors.json")?).map_err(|e| e.to_string())?
        };
        let version = String::from_utf8(load("version.txt")?).map_err(|e| e.to_string())?;
        let metrics =
            serde_json::from_slice(&load("font-metrics.json")?).map_err(|e| e.to_string())?;
        let tiles = std::fs::read(root.join("tiles.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let heroes = std::fs::read(root.join("heroes.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Ok(Self {
            heroes,
            root,
            icons,
            colors,
            version,
            metrics,
            artwork: crate::app_artwork::Service::new(webos),
            tiles,
        })
    }
    pub fn path(&self, path: impl AsRef<Path>) -> String {
        self.root.join(path).to_string_lossy().into_owned()
    }

    fn width(&self, value: &str, style: TextStyle) -> f32 {
        let key = format!(
            "{}{}",
            if style.size >= 32. { "display-" } else { "" },
            style.weight
        );
        let advances = self.metrics.get(&key);
        value
            .chars()
            .map(|c| {
                (advances
                    .and_then(|a| a.get(&c))
                    .copied()
                    .unwrap_or(if c == ' ' { 0.28 } else { 0.53 })
                    + style.tracking)
                    * style.size
            })
            .sum()
    }

    fn lines(&self, value: &str, style: TextStyle, width: f32) -> usize {
        let key = format!(
            "{}{}",
            if style.size >= 32. { "display-" } else { "" },
            style.weight
        );
        let advances = self.metrics.get(&key);
        let advance = |c: char| {
            (advances
                .and_then(|a| a.get(&c))
                .copied()
                .unwrap_or(if c == ' ' { 0.28 } else { 0.53 })
                + style.tracking)
                * style.size
        };
        let mut lines = 1;
        let mut used = 0.;
        for word in value.split_inclusive(char::is_whitespace) {
            let w: f32 = word.chars().map(advance).sum();
            if used > 0. && used + w > width {
                lines += 1;
                used = 0.;
            }
            for c in word.chars() {
                if c == '\n' {
                    lines += 1;
                    used = 0.;
                    continue;
                }
                let w = advance(c);
                if used > 0. && used + w > width {
                    lines += 1;
                    used = 0.;
                }
                used += w;
            }
        }
        lines
    }
}

/// The switcher's backdrop: the blurred wallpaper, darkened in its own tint.
const SWITCHER_VEIL: f32 = 0.38;
/// The grid's backdrop tint, the same clear, dimmed glass in both appearances.
const GRID_TINT: Color = [GRID_VEIL, GRID_VEIL, GRID_VEIL + 0.02, 1.];

/// A wallpaper's blurred backdrop (Settings, the grid, the switcher).
fn blurred(assets: &Assets, wallpaper: &str) -> String {
    assets.path(format!("wallpapers/{wallpaper}-glass.png"))
}

/// The apps page's background with the grid `grid` of the way up (0 Top
/// Shelf, 1 the grid). tvOS: the photo behind Top Shelf (the dock app's art,
/// or the wallpaper for apps without any) gives way to a blurred, dimmed
/// backdrop as the grid slides up.
fn apps_backdrop(state: &State, assets: &Assets, grid: f32) -> Backdrop {
    let wallpaper = state.wallpaper();
    let backdrop = blurred(assets, wallpaper);
    // A wallpaper changing as this page comes back (picked in Settings just
    // before): the old one's picture fades into the new one's.
    let change = state.wallpaper_change();
    if grid >= 1. {
        return Backdrop {
            tint: [GRID_TINT; 2],
            ..match change {
                Some((previous, t)) => Backdrop::between(blurred(assets, previous), backdrop, t),
                None => Backdrop::picture(backdrop, [1.; 4]),
            }
        };
    }
    let (current, current_glass) = shelf_art(state, assets, state.shelf_app.as_deref());
    if grid > 0. {
        // Scrolling into the grid the picture blurs, then dims, as tvOS's
        // does, continuously both ways: first the photo eases into its
        // blurred self, then that into the dimmed backdrop. (A jump from
        // blurred straight to sharp read as the wallpaper popping in.)
        const BLUR: f32 = 0.35;
        let smooth = |t: f32| t * t * (3. - 2. * t);
        return if grid < BLUR {
            Backdrop::between(current, current_glass, smooth(grid / BLUR))
        } else {
            Backdrop {
                tint: [[1.; 4], GRID_TINT],
                ..Backdrop::between(current_glass, backdrop, smooth((grid - BLUR) / (1. - BLUR)))
            }
        };
    }
    let photo = |id: &str| assets.path(format!("wallpapers/{id}.jpg"));
    match (shelf_dissolve(state), change) {
        (Some((previous, t)), _) => {
            Backdrop::between(shelf_art(state, assets, previous).0, current, t)
        }
        (None, Some((previous, t))) if current == photo(wallpaper) => {
            Backdrop::between(photo(previous), current, t)
        }
        _ => Backdrop::picture(current, [1.; 4]),
    }
}

/// Settings' background. tvOS Settings is not black: it is the home's
/// blurred wallpaper, dimmed further, so it carries the wallpaper's colour;
/// in the light appearance it frosts over instead, in the same pass. A
/// wallpaper picked here cross-fades in under exactly that tint.
fn settings_backdrop(out: &Scene, state: &State, assets: &Assets) -> Backdrop {
    let (tint, lift) = out.theme.backdrop(SETTINGS_VEIL);
    let wallpaper = blurred(assets, state.wallpaper());
    Backdrop {
        tint: [tint; 2],
        lift,
        ..match state.wallpaper_change() {
            Some((previous, t)) => Backdrop::between(blurred(assets, previous), wallpaper, t),
            None => Backdrop::picture(wallpaper, [1.; 4]),
        }
    }
}

/// The background `t` of the way from one backdrop to another: in one pass
/// where each shows a single picture (every page change at rest), else the
/// new one laid over the old for the moment they cross.
fn cross(out: &mut Scene, from: Backdrop, to: Backdrop, t: f32) {
    if t >= 1. {
        out.draw.push(Draw::Dissolve(to));
        return;
    }
    match (from.single(), to.single()) {
        (Some((a, a_tint)), Some((b, b_tint))) => {
            // Only Settings' light frost lifts; it comes and goes with its page.
            let colour = if to.lift[3] > 0. { to.lift } else { from.lift };
            let mut lift = colour;
            lift[3] = from.lift[3] + (to.lift[3] - from.lift[3]) * t;
            let pass = Backdrop {
                tint: [a_tint, b_tint],
                lift,
                ..Backdrop::between(a, b, t)
            };
            out.draw.push(Draw::Dissolve(pass));
        }
        _ => {
            out.draw.push(Draw::Dissolve(from));
            out.draw.push(Draw::Dissolve(Backdrop { opacity: t, ..to }));
        }
    }
}

pub fn scene(state: &State, assets: &Assets, now: OffsetDateTime) -> Scene {
    let mut out = Scene::new(theme::Theme::new(state.light()));
    // Search, once fully in, is an opaque page: nothing beneath it shows.
    let search_covers =
        matches!(state.overlay, Some(Overlay::Keyboard)) && state.panel_started.is_none();
    if search_covers {
        // No background: the search page covers it.
    } else if state.switcher_covers() {
        // The switcher's dimmed backdrop is then the whole background: one
        // opaque pass instead of a picture with a full-screen glass over it.
        let shade = [SWITCHER_VEIL, SWITCHER_VEIL, SWITCHER_VEIL + 0.02, 1.];
        out.draw.push(Draw::Dissolve(Backdrop::picture(
            blurred(assets, state.wallpaper()),
            shade,
        )));
    } else {
        // Every background is one opaque pass mixing at most two pictures
        // (Draw::Dissolve): two stacked 1080p layers miss the TV's 20 ms
        // frame. A page change dissolves the background the page left
        // showed into the new page's, as its content dissolves.
        let grid = |page: Page, scroll: f32| match page {
            Page::Apps => (scroll / GRID_SCROLL).clamp(0., 1.),
            Page::Settings => 1.,
            Page::Home => 0.,
        };
        let backdrop = |out: &Scene, page: Page, scroll: f32| match page {
            Page::Settings => settings_backdrop(out, state, assets),
            _ => apps_backdrop(state, assets, grid(page, scroll)),
        };
        let shown = backdrop(&out, state.page, state.shown_scroll);
        let (left, left_scroll) = state.left;
        match state.page_since {
            Some((start, _)) if left != state.page => {
                let t = (crate::clock::since(start).as_secs_f32() * 1000. / PAGE_MS).min(1.);
                let from = backdrop(&out, left, left_scroll);
                cross(&mut out, from, shown, 1. - (1. - t).powi(3));
            }
            _ => out.draw.push(Draw::Dissolve(shown)),
        }
        // Keep all Top Shelf art and the wallpaper decoded and resident (a 1px
        // draw each) so no dissolve ever waits on a JPEG decode.
        if state.page == Page::Apps {
            let resident = assets
                .heroes
                .iter()
                .filter(|_| state.preferences.top_shelf_art)
                .flat_map(|id| [format!("heroes/{id}.jpg"), format!("heroes/{id}-glass.png")])
                .map(|name| assets.path(name))
                .chain(crate::preload::wallpaper_pictures(
                    assets,
                    state.wallpaper(),
                ));
            for path in resident {
                out.image(path, Rect::new(0., 0., 1., 1.), 0., false);
            }
        }
    }
    let main_bottom = if state.page == Page::Home && state.moving.is_none() {
        1020.
    } else if state.page == Page::Apps {
        1080.
    } else {
        968.
    };
    let main_top = if state.page == Page::Apps {
        0.
    } else {
        MAIN_TOP
    };
    // A page change: the page left drifts away and fades as the new one
    // drifts in, so no frame drops what was on screen all at once.
    let page_t = state.page_since.map(|(start, direction)| {
        let t = (crate::clock::since(start).as_secs_f32() * 1000. / PAGE_MS).min(1.);
        (t, 1. - (1. - t).powi(3), direction)
    });
    if let (Some((t, eased, direction)), Some(ghost)) = (page_t, &state.page_ghost) {
        // Gone by half way, as a Settings push's page is.
        let x = (t / 0.5).min(1.);
        out.ghost(ghost, -direction * 40. * eased, 1. - x * x * (3. - 2. * x));
    }
    let page_start = out.draw.len();
    out.draw.push(Draw::Clip(Some(Rect::new(
        0.,
        main_top,
        1920.,
        main_bottom - main_top,
    ))));
    // Fully covering screens (search, the switcher once shown) skip the page
    // beneath: drawing it would only spend the TV's small GPU budget.
    let covered = search_covers || state.switcher_covers();
    if covered {
        out.draw.push(Draw::EdgeFade(None));
        out.draw.push(Draw::Clip(None));
    } else {
        let content = out.draw.len();
        match state.page {
            Page::Home => home(&mut out, state, assets, now),
            Page::Apps => library(&mut out, state, assets),
            Page::Settings => settings(&mut out, state, assets),
        }
        out.translate(content, 0., state.scroll - state.shown_scroll);
        // tvOS tab switch: the new page dissolves in, drifting from the travel direction.
        if let Some((_, eased, direction)) = page_t {
            out.translate(content, direction * 40. * (1. - eased), 0.);
        }
        if let Some(error) = &state.error
            && !state.inventory.apps.is_empty()
            && !out
                .controls
                .iter()
                .any(|control| control.target == Target::Retry)
        {
            let rect = Rect::new(84., main_bottom - 66., 1752., 54.);
            out.shape(rect, 11.2, c(0xad7c3e, 0.07));
            out.border(rect, 11.2, 1., c(0xedc596, 0.19));
            out.icon(
                assets,
                "alert",
                Rect::new(104., rect.y + 18., 18., 18.),
                c(0xf3ddbd, 1.),
            );
            out.text(
                error,
                Rect::new(138., rect.y + 12., 1560., 30.),
                detail_style(),
                c(0xf3ddbd, 1.),
                false,
            );
            let retry = Rect::new(1720., rect.y + 8., 96., 38.);
            out.text(
                i18n::t("retry"),
                Rect::new(retry.x, retry.y + 4., retry.w, 30.),
                detail_style(),
                c(0xf3ddbd, 1.),
                true,
            );
            if state.focus == Target::Retry {
                out.border(retry, 6.4, 2., WHITE);
            }
            out.control(Target::Retry, retry, false);
        }
        out.draw.push(Draw::EdgeFade(None));
        out.draw.push(Draw::Clip(None));
        // The time pill belongs to the home screen, as on tvOS.
        if state.page != Page::Settings {
            header(&mut out, state, assets, now);
        }
        footer(&mut out, state, assets);
        // The pill fades with its page: it came back whole on the first frame.
        if let Some((_, eased, _)) = page_t {
            out.fade(content, eased, 1.);
        }
        out.page = Some((state.page, page_start..out.draw.len()));
    }
    // Closing: search fades off the home instead of cutting, its query and
    // results with it (they vanished on the first frame, leaving an empty
    // page to fade).
    if state.overlay.is_none()
        && let Some(closed) = state.search_closed
    {
        let t = (crate::clock::since(closed).as_secs_f32() * 1000. / FADE_OUT_MS as f32).min(1.);
        let alpha = 1. - t * (2. - t);
        match &state.search_ghost {
            Some(ghost) => out.ghost(ghost, 0., alpha),
            // The search page's own colour, so closing it never flashes
            // dark in the light appearance.
            None => out.shape(
                Rect::new(0., 0., 1920., 1080.),
                0.,
                out.theme.pick(c(0x2b2b2d, alpha), c(0xf2f2f7, alpha)),
            ),
        }
    }
    // Closing: the context menu shrinks back into its tile as it fades, the
    // reverse of how it rose. It eases out, as the opening does: an ease-in
    // ended at full speed and the menu stopped dead on its last frame.
    if state.overlay.is_none()
        && let Some((id, closed)) = &state.options_closed
    {
        let t = (crate::clock::since(closed).as_secs_f32() * 1000. / FADE_OUT_MS as f32).min(1.);
        let gone = 1. - (1. - t).powi(3);
        let anchor = out
            .controls
            .iter()
            .find(|control| control.target == Target::App(id.clone()))
            .map(|control| control.rect);
        // The screen lifts back as the menu goes, on the same curve.
        out.dim(1. - MENU_DIM * (1. - gone));
        let start = out.draw.len();
        let controls = out.controls.len();
        options(&mut out, state, assets, id, anchor);
        out.controls.truncate(controls);
        out.modal = None;
        // tvOS: the menu shrinks back into the app it came from, fading as it goes.
        if let Some(tile) = anchor {
            let (cx, cy) = tile.center();
            out.scale_about(start, cx, cy, 1. - 0.18 * gone);
        }
        out.fade(start, 1. - gone, 1.);
    }
    if let Some(
        overlay @ (Overlay::Options { .. }
        | Overlay::Keyboard
        | Overlay::Uninstall { .. }
        | Overlay::Inputs),
    ) = &state.overlay
    {
        // A context menu opens beside the tile it acts on, as on tvOS.
        let anchor = match overlay {
            Overlay::Options { id, .. } => out
                .controls
                .iter()
                .find(|control| control.target == Target::App(id.clone()))
                .map(|control| control.rect),
            _ => None,
        };
        out.controls.clear();
        // Opening eases out: the screen dims as the menu rises into place.
        let opening = state.panel_started.map(|opened| {
            let t = (crate::clock::since(opened).as_secs_f32() * 1000.
                / crate::state::OVERLAY_OPEN_MS as f32)
                .min(1.);
            1. - (1. - t).powi(3)
        });
        let eased = opening.unwrap_or(1.);
        if !search_covers {
            out.dim(1. - MENU_DIM * eased);
        }
        let panel = out.draw.len();
        match overlay {
            Overlay::Options { id, .. } => options(&mut out, state, assets, id, anchor),
            Overlay::Keyboard => {
                keyboard(&mut out, state, assets);
                out.search = Some(panel..out.draw.len());
            }
            Overlay::Uninstall { id, .. } => alert::uninstall(&mut out, state, assets, id),
            Overlay::Inputs => inputs_panel(&mut out, state, assets),
            Overlay::SystemKeyboard => {}
        }
        if opening.is_some() {
            // tvOS: the menu grows out of the app it acts on.
            if let (Overlay::Options { .. }, Some(tile)) = (overlay, anchor) {
                let (cx, cy) = tile.center();
                out.scale_about(panel, cx, cy, 0.82 + 0.18 * eased);
            }
            out.fade(panel, eased, 1.);
        }
    }
    switcher(&mut out, state, assets);
    move_hint(&mut out, state, assets);
    if let Some((notice, _)) = &state.notice {
        toast(&mut out, state, assets, notice);
    }
    out
}
