//! The pre-blurred art behind Liquid Glass.
use super::Assets;
use crate::state::{Page, SHELF_FADE, State};

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
