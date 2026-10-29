//! App tiles: the card face, its brand mark and its caption.
use super::{
    Assets, FOCUS_SCALE, LABEL_GAP, MAIN_TOP, Scene, TILE_RADIUS,
    shared::{c, text},
};

/// How much further than focus the app in Move rises (a fraction of its size).
const MOVE_LIFT: f32 = 0.03;
use crate::{
    i18n,
    model::App,
    renderer::{Color, Draw, TextStyle, WHITE},
    state::{FADE_OUT_MS, Overlay, Rect, State, Target},
};

pub(super) fn tile(
    out: &mut Scene,
    state: &State,
    assets: &Assets,
    app: &App,
    rect: Rect,
    // The app's name under it, in this ink (white over the photo; the
    // appearance's ink on the search screen), or None for no name.
    names: Option<Color>,
    pinned: bool,
) {
    let caption_height = if names.is_some() {
        caption_height(state, assets, app, rect.w)
    } else {
        0.
    };
    let label_height = if names.is_some() {
        LABEL_GAP + caption_height
    } else {
        0.
    };
    out.control(
        Target::App(app.id.clone()),
        Rect::new(rect.x, rect.y, rect.w, rect.h + label_height),
        true,
    );
    // One pitch of slack: drawn positions glide behind the layout positions.
    if rect.y + rect.h + label_height < MAIN_TOP - 340.
        || rect.y > 1054. + 340.
        || rect.x + rect.w < -340.
        || rect.x > 1920. + 340.
    {
        return;
    }
    let start = out.draw.len();
    let focused = state.focus == Target::App(app.id.clone());
    // The app a context menu acts on stays lifted while the menu is open and
    // while it fades away, then hands back to focus with no dip.
    let held = matches!(&state.overlay, Some(Overlay::Options { id, .. }) if *id == app.id)
        || state.options_closed.as_ref().is_some_and(|(id, closed)| {
            *id == app.id && crate::clock::since(closed).as_millis() < u128::from(FADE_OUT_MS)
        });
    let lift = if held {
        1.
    } else {
        state.focus_amount(&Target::App(app.id.clone()))
    };
    // Move, as tvOS: the app in hand rises further out of the grid, with a
    // deeper shadow. No outline, and the others stay as they are.
    let raise = if state.moving.as_ref() == Some(&app.id) {
        state.move_raise()
    } else {
        0.
    };
    let base = rect;
    let scale = 1. + FOCUS_SCALE * lift + MOVE_LIFT * raise;
    let (cx, cy) = base.center();
    let rect = Rect::new(
        cx - base.w * scale / 2.,
        cy - base.h * scale / 2.,
        base.w * scale,
        base.h * scale,
    );
    let radius = TILE_RADIUS + 8. * lift;
    // Only the focused card casts a shadow (tvOS lift). Resting shadows were
    // nearly invisible yet cost a large blurred quad each on the TV's GPU.
    if lift > 0. || raise > 0. {
        out.shadow(
            rect,
            radius,
            40. + 24. * raise,
            30. * lift + 14. * raise,
            0.35 * lift + 0.15 * raise,
        );
    }
    tile_face(out, assets, app, rect, radius);
    if pinned {
        out.icon(
            assets,
            "heart-filled",
            Rect::new(rect.x + rect.w - 27.2, rect.y + 11.2, 16., 16.),
            c(0xffffff, 0.9),
        );
    }
    if state.pressed.as_ref() == Some(&Target::App(app.id.clone())) {
        out.fade(start, 1., 0.85);
    }
    // The app in hand has Move's instructions where its name goes (overlays::move_hint).
    if let Some([r, g, b, _]) = names.filter(|_| state.moving.as_ref() != Some(&app.id)) {
        out.text(
            fit_caption(assets, &app.title, caption_style(focused), base.w - 8.),
            Rect::new(
                base.x + 4.,
                base.y + base.h + LABEL_GAP,
                base.w - 8.,
                caption_height,
            ),
            caption_style(focused),
            [r, g, b, 0.6 + 0.4 * lift],
            true,
        );
    }
    if held {
        out.lit = Some(start..out.draw.len());
    }
}

/// The card face alone, shared by grid tiles and the launch splash.
pub(super) fn tile_face(out: &mut Scene, assets: &Assets, app: &App, rect: Rect, radius: f32) {
    if assets.tiles.contains_key(&app.id) {
        out.image(
            assets.path(format!("tiles/{}.png", app.id)),
            rect,
            radius,
            false,
        );
        // No outline: tvOS icons have none, and a border pass shades the whole tile.
    } else if app.brand == "lounge-input" {
        // An input: the system tiles' grey with the HDMI glyph over its
        // port's name (the glyph alone cannot tell HDMI 1 from HDMI 2).
        out.draw.push(Draw::RoundedGradient(
            rect,
            radius,
            c(0x9a9aa0, 1.),
            c(0x6e6e74, 1.),
        ));
        let size = rect.h * 0.4;
        out.icon(
            assets,
            "input-hdmi",
            Rect::new(
                rect.x + (rect.w - size) / 2.,
                rect.y + rect.h * 0.13,
                size,
                size,
            ),
            WHITE,
        );
        // Laid out once, for a resting card, and drawn at the card's scale:
        // a focus lift or the switcher's glide grows it smoothly with the
        // card, where re-laying it out stepped it through half-pixel sizes.
        let scale = rect.h / super::FACE_HEIGHT;
        // Whole pixels: the layout is the run's cache key, and the width
        // unscaled again varies in its last bits from frame to frame.
        let (width, height) = ((rect.w / scale).round(), super::FACE_HEIGHT);
        out.text(
            &app.title,
            Rect::new(rect.x, rect.y + rect.h * 0.56, width, height * 0.24),
            text(height * 0.17, 650, 0.).scaled(scale),
            WHITE,
            true,
        );
    } else {
        let logo = crate::branding::for_key(&app.brand);
        // Artwork Lounge ships always wins: it is there from the first frame,
        // never swapped later. Only an app Lounge has no artwork for reads its
        // icon from the TV (once; it is cached on disk after).
        let installed = logo.is_none().then(|| assets.icons.get(&app.id)).flatten();
        let artwork = (logo.is_none() && installed.is_none())
            .then(|| {
                app.icon
                    .as_ref()
                    .and_then(|source| assets.artwork.get(source))
            })
            .flatten();
        if let Some([top, bottom]) = artwork.as_ref().map(|image| image.backdrop).or_else(|| {
            installed
                .and_then(|_| assets.colors.get(&app.id))
                .map(|rgb| [*rgb; 2])
        }) {
            let color = |rgb: [u8; 3]| rgb.map(|c| c as f32 / 255.);
            let [top, bottom] = [color(top), color(bottom)];
            if top == bottom {
                out.shape(rect, radius, [top[0], top[1], top[2], 1.]);
            } else {
                out.draw.push(Draw::RoundedGradient(
                    rect,
                    radius,
                    [top[0], top[1], top[2], 1.],
                    [bottom[0], bottom[1], bottom[2], 1.],
                ));
            }
        } else if let Some(icon) = match app.brand.as_str() {
            "lounge-settings" => Some("gear"),
            "lounge-search" => Some("search"),
            _ => None,
        } {
            // tvOS system tiles: light grey with a large white glyph.
            out.draw.push(Draw::RoundedGradient(
                rect,
                radius,
                c(0x9a9aa0, 1.),
                c(0x6e6e74, 1.),
            ));
            let size = rect.h * 0.46;
            out.icon(
                assets,
                icon,
                Rect::new(
                    rect.x + (rect.w - size) / 2.,
                    rect.y + (rect.h - size) / 2.,
                    size,
                    size,
                ),
                WHITE,
            );
            return;
        } else if app.brand == "generic" {
            // tvOS-style flat tile for apps without their own artwork.
            out.shape(rect, radius, c(0x2c2c2e, 1.));
        } else {
            out.image(
                assets.path(format!("surfaces/{}.png", app.brand)),
                rect,
                radius,
                false,
            );
        }
        // No outline: tvOS icons have none, and a border pass shades the whole tile.
        if let Some(brand) = logo {
            let (ratio, h) = brand.artwork_size();
            out.image(
                assets.path(format!("logos/{}.lvec", app.brand)),
                Rect::new(
                    rect.x + rect.w * (1. - ratio) / 2.,
                    rect.y + (rect.h - h) / 2.,
                    rect.w * ratio,
                    h,
                ),
                0.,
                true,
            );
        } else if let Some(image) = artwork {
            out.draw.push(Draw::AppImage(
                image,
                Rect::new(
                    rect.x + rect.w * 0.16,
                    rect.y + rect.h * 0.16,
                    rect.w * 0.68,
                    rect.h * 0.68,
                ),
                [1.; 4],
            ));
        } else if let Some(icon) = installed {
            out.image(
                assets.path(icon),
                Rect::new(
                    rect.x + rect.w * 0.16,
                    rect.y + rect.h * 0.16,
                    rect.w * 0.68,
                    rect.h * 0.68,
                ),
                0.,
                true,
            );
        } else {
            brand_mark(out, assets, app, rect);
        }
    }
}

pub(super) fn caption_height(state: &State, assets: &Assets, app: &App, width: f32) -> f32 {
    let style = caption_style(state.focus == Target::App(app.id.clone()));
    let lines = assets
        .lines(
            &fit_caption(assets, &app.title, style, width - 8.),
            style,
            width - 8.,
        )
        .clamp(1, 2);
    lines as f32 * style.size * style.line_height
}

/// tvOS keeps names to two lines, ending a longer one with an ellipsis, so
/// labels never reach the next row.
fn fit_caption(assets: &Assets, title: &str, style: TextStyle, width: f32) -> String {
    if assets.lines(title, style, width) <= 2 {
        return title.to_owned();
    }
    let mut chars: Vec<char> = title.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate = format!("{}…", chars.iter().collect::<String>().trim_end());
        if assets.lines(&candidate, style, width) <= 2 {
            return candidate;
        }
    }
    "…".into()
}

/// tvOS Caption 2: SF Pro Medium 23/30.
fn caption_style(focused: bool) -> TextStyle {
    text(23., if focused { 600 } else { 500 }, 0.).leading(30. / 23.)
}

fn brand_mark(out: &mut Scene, assets: &Assets, app: &App, rect: Rect) {
    let (cx, cy) = rect.center();
    match app.brand.as_str() {
        "plex" => {
            out.text(
                "ple",
                Rect::new(cx - 63., cy - 35.2, 130., 70.4),
                text(70.4, 650, -0.08).leading(1.),
                WHITE,
                false,
            );
            out.text(
                "x",
                Rect::new(cx + 20., cy - 35.2, 60., 70.4),
                text(70.4, 800, -0.08).leading(1.),
                c(0xe6aa25, 1.),
                false,
            );
        }
        "spotify" => {
            out.icon(
                assets,
                "spotify",
                Rect::new(cx - 77., cy - 20., 40., 40.),
                c(0x5ae08c, 1.),
            );
            out.text(
                "Spotify",
                Rect::new(cx - 29., cy - 14.4, 120., 36.),
                text(28.8, 650, -0.04).leading(1.),
                c(0x5ae08c, 1.),
                false,
            );
        }
        "kodi" => {
            out.icon(
                assets,
                "kodi",
                Rect::new(cx - 76., cy - 22.5, 45., 45.),
                WHITE,
            );
            out.text(
                "kodi",
                Rect::new(cx - 18., cy - 24.8, 130., 60.),
                text(49.6, 350, -0.04).leading(1.),
                WHITE,
                false,
            );
        }
        brand => {
            let (icon, label) = match brand {
                "live-tv" => ("tv", i18n::t("liveTV")),
                "homebrew" => ("coffee", i18n::t("homebrew")),
                "browser" => ("globe", i18n::t("browser")),
                "moonlight" => ("moonlight", "Moonlight"),
                "gaming" => ("gaming", app.title.as_str()),
                _ => ("generic", app.title.as_str()),
            };
            let font = if brand == "generic" {
                text(18.4, 500, -0.035).leading(1.35)
            } else {
                text(24., 500, -0.035).leading(1.2)
            };
            let w = (assets.width(label, font) + 54.4).min(rect.w - 32.);
            let label_w = rect.w - 54.4 - 32.;
            let label_h =
                (assets.lines(label, font, label_w) as f32 * font.size * font.line_height)
                    .min(rect.h - 32.);
            out.icon(
                assets,
                icon,
                Rect::new(cx - w / 2., cy - 20.8, 41.6, 41.6),
                WHITE,
            );
            out.text(
                label,
                Rect::new(cx - w / 2. + 54.4, cy - label_h / 2., label_w, label_h),
                font,
                WHITE,
                false,
            );
        }
    }
}
