//! The app switcher.
use super::{Assets, Scene, shared::tab_style, tile::tile_face};
use crate::{
    model::App,
    renderer::{Draw, WHITE},
    state::{Rect, State, Target},
};

/// tvOS app switcher: open apps as large cards; OK opens, Up quits, Back returns.
pub(super) fn switcher(out: &mut Scene, state: &State, assets: &Assets) {
    let Some(switcher) = &state.switcher else {
        return;
    };
    out.controls.clear();
    let ease = |t: f32| 1. - (1. - t.clamp(0., 1.)).powi(3);
    // In over 0.28 s; Back fades it out over the live home, settling back.
    let leave = switcher.dismissed.map_or(0., |start| {
        ease(
            crate::clock::since(start).as_secs_f32() * 1000.
                / crate::state::SWITCHER_DISMISS_MS as f32,
        )
    });
    let appear = ease(crate::clock::since(switcher.opened).as_secs_f32() / 0.28) * (1. - leave);
    // tvOS: open apps as windows in a row over the dimmed, blurred home. While
    // it fades in or out the home cross-fades into the backdrop (in the
    // background's own pass where it can); once it covers the screen the
    // scene draws it as the background itself (see `scene`).
    if !state.switcher_covers() {
        let backdrop = super::blurred(assets, state.wallpaper());
        let veil = super::SWITCHER_VEIL;
        if !out.fade_into(&backdrop, [veil, veil, veil + 0.02, 1.], appear) {
            let shade = [veil, veil, veil + 0.02, appear];
            out.draw.push(Draw::Glass(
                backdrop,
                Rect::new(0., 0., 1920., 1080.),
                0.,
                shade,
                shade,
            ));
        }
    }
    // Each open app is its own card (no frame, no second logo), as upstream
    // Lounge and tvOS show it: the focused one lifted, the rest dimmed back.
    let (w, h, pitch) = (720., 412., 800.);
    let top = 300. + 40. * (1. - appear);
    // After a close the cards right of the gap wait a beat (the thrown one
    // clears first), then glide in with a long, soft settle.
    let reflow = switcher.reflow.map(|(slot, start)| {
        let ms = crate::clock::since(start).as_secs_f32() * 1000.;
        let t = ((ms - 90.) / (crate::state::SWITCHER_REFLOW_MS as f32 - 90.)).clamp(0., 1.);
        (slot, (1. - t).powi(5))
    });
    let window = |offset: f32| {
        let near = 1. - offset.abs().min(1.);
        // Lift follows the glide: the card nearest the centre is largest.
        let scale = 0.9 + 0.1 * near;
        let (cw, ch) = (w * scale, h * scale);
        (
            Rect::new(960. + offset * pitch - cw / 2., top + (h - ch) / 2., cw, ch),
            near,
        )
    };
    // The TV's one GPU core pays for every painted pixel: only the card in
    // front casts a shadow, and the others darken (no extra layer). As on
    // tvOS each card carries its app's name above it, dimmed with the card.
    // An app webOS has a snapshot of shows it as it was left, dissolving in
    // over its card once read; its icon then joins the name above (the card
    // no longer says which app it is).
    let draw = |out: &mut Scene, app: &App, rect: Rect, near: f32, alpha: f32| {
        let first = out.draw.len();
        if near > 0.05 {
            out.shadow(rect, 36., 60., 26., 0.55 * near);
        }
        let shot = state.snapshots.get(&app.id);
        let shown = state
            .snapshots
            .shown(&app.id, state.preferences.reduced_motion);
        if shown < 1. {
            tile_face(out, assets, app, rect, 36.);
        }
        if let Some(shot) = shot {
            out.draw.push(Draw::AppImage(
                shot.picture.clone(),
                rect,
                [1., 1., 1., shown],
            ));
        }
        // Laid out at the card's full width, centred on it, however the
        // card is scaled: one cached text run per app, not one per frame.
        let (icon_w, gap) = (60., 14.);
        let label = Rect::new(rect.x + rect.w / 2. - w / 2., rect.y - 58., w, 36.);
        let shift = (icon_w + gap) / 2. * shown;
        if shown > 0. {
            let title = assets.width(&app.title, tab_style()).min(w - icon_w - gap);
            let icon = Rect::new(
                rect.x + rect.w / 2.
                    - (title + icon_w + gap) / 2. * shown
                    - icon_w / 2. * (1. - shown),
                label.y,
                icon_w,
                36.,
            );
            let start = out.draw.len();
            tile_face(out, assets, app, icon, 8.);
            out.fade(start, shown, 1.);
        }
        out.text(
            &app.title,
            Rect::new(label.x + shift, label.y, label.w, label.h),
            tab_style(),
            WHITE,
            true,
        );
        out.fade(first, alpha, 0.55 + 0.45 * near);
    };
    for (index, id) in switcher.apps.iter().enumerate() {
        let Some(app) = state.inventory.apps.iter().find(|app| &app.id == id) else {
            continue;
        };
        let shift = reflow.map_or(0., |(slot, left)| if index >= slot { left } else { 0. });
        let (rect, near) = window(index as f32 + shift - switcher.shown);
        if rect.x > 1920. || rect.x + rect.w < 0. {
            continue;
        }
        draw(out, app, rect, near, appear);
        if switcher.dismissed.is_none() {
            out.control(Target::Switch(index), rect, false);
        }
    }
    // Up closes an app: its window lifts away and fades, as on tvOS.
    for (id, start, offset) in &switcher.closing {
        let Some(app) = state.inventory.apps.iter().find(|app| &app.id == id) else {
            continue;
        };
        let t = (crate::clock::since(start).as_secs_f32() * 1000.
            / crate::state::SWITCHER_CLOSE_MS as f32)
            .min(1.);
        // Thrown: one motion up and off the screen, accelerating, easing
        // back a touch in size; it only fades once nearly gone.
        let (rect, near) = window(*offset);
        let scale = 1. - 0.05 * t;
        let (w, h) = (rect.w * scale, rect.h * scale);
        let travel = rect.y + rect.h + 80.;
        let thrown = Rect::new(
            rect.x + (rect.w - w) / 2.,
            rect.y - travel * t * t * t,
            w,
            h,
        );
        let fade = 1. - ((t - 0.6) / 0.4).clamp(0., 1.);
        draw(out, app, thrown, near, fade * (1. - leave));
    }
}
