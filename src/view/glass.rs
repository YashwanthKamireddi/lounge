//! Liquid Glass platters and the pre-blurred art behind them.
use super::{Assets, Scene, shared::c};
use crate::{
    renderer::Draw,
    state::{Page, Rect, SHELF_FADE, State},
};

/// A focused row's own piece of Liquid Glass floating over the dimmed screen.
/// Squares round off, wide rows are capsules. Focus (0..1, eased) brightens
/// the glass and swells it within the gap. Returns the drawn rect.
pub(super) fn platter(out: &mut Scene, state: &State, assets: &Assets, rect: Rect, focus: f32) -> Rect {
    let rect = rect.inset(-rect.w.min(rect.h) * 0.05 * focus);
    let radius = if rect.h > 150. { 52. } else { rect.h / 2. };
    out.shadow(rect, radius, 24. + 20. * focus, 10., 0.25 + 0.2 * focus);
    // The glass sees the screen as it is now, dimmed.
    let dim = [0.58, 0.58, 0.6, 1.];
    out.draw.push(Draw::Glass(home_backdrop(state, assets), rect, radius, dim, dim));
    out.shape(rect, radius, c(0xffffff, 0.06 + 0.24 * focus));
    // Light gathers at the top of the glass and along its rim.
    out.draw.push(Draw::RoundedGradient(
        rect,
        radius,
        c(0xffffff, 0.2 + 0.12 * focus),
        c(0xffffff, 0.03),
    ));
    // Liquid Glass's edge: a faint rim all round, and a specular one where
    // light strikes the top that fades away down the sides.
    out.draw.push(Draw::GradientRing(
        rect,
        radius,
        1.5,
        c(0xffffff, 0.62 + 0.3 * focus),
        c(0xffffff, 0.06 + 0.18 * focus),
    ));
    rect
}

/// Top Shelf art for a dock app: (full screen, pre-blurred for glass). Apps
/// without art, and every other page, use the wallpaper.
pub(super) fn shelf_art(state: &State, assets: &Assets, id: Option<&str>) -> (String, String) {
    match id {
        Some(id) if state.page == Page::Apps && state.has_art(id) => (
            assets.path(format!("heroes/{id}.jpg")),
            assets.path(format!("heroes/{id}-glass.png")),
        ),
        _ => {
            let background = &state.preferences.background;
            (
                assets.path(format!("wallpapers/{background}.jpg")),
                assets.path(format!("wallpapers/{background}-home-glass.png")),
            )
        }
    }
}

/// The app Top Shelf is dissolving away from, and how far in (0-1, eased).
pub(super) fn shelf_dissolve(state: &State) -> Option<(Option<&str>, f32)> {
    let (previous, start) = state.shelf_from.as_ref()?;
    let t = (start.elapsed().as_secs_f32() / SHELF_FADE).min(1.);
    Some((previous.as_deref(), t * t * (3. - 2. * t)))
}

/// The pre-blurred image behind home glass: Top Shelf art while it shows,
/// otherwise the wallpaper.
pub(super) fn home_backdrop(state: &State, assets: &Assets) -> String {
    shelf_art(state, assets, state.shelf_app.as_deref()).1
}
