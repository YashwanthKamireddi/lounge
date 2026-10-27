//! What floats over the pages: the status pill, hint footer, context menu,
//! search keyboard and notifications.
use super::{
    Assets, Scene,
    glass::home_backdrop,
    shared::{PLATTER, c, detail_style, row_style, text},
    tile::tile,
};
use crate::{
    i18n,
    renderer::{Draw, WHITE},
    state::{GRID_SCROLL, KEYS, Page, Rect, SEARCH_TILE, State, Target, TextField},
    weather,
};
use time::OffsetDateTime;

/// The status pill's scale over tvOS's 25 px menu-bar text: large enough to
/// read across a room, in the same proportions.
const PILL_SCALE: f32 = 1.3;
pub(super) const PILL_HEIGHT: f32 = 60. * PILL_SCALE;
/// The soft edge (logical px either side) of the large glass curves: the
/// pill, its avatar, menus and banners (see Draw::Soft).
const SOFT_EDGE: f32 = 0.8;

pub(super) fn header(out: &mut Scene, state: &State, assets: &Assets, now: OffsetDateTime) {
    // tvOS: no tab bar, one glass capsule at the top right: time, the local
    // weather and the profile's avatar. It only shows; nothing here is focused.
    let clock = i18n::clock(now, state.hour_24);
    let k = PILL_SCALE;
    let style = text(25. * k, 600, 0.);
    let reading = state
        .preferences
        .weather_location
        .as_ref()
        .filter(|_| state.preferences.show_weather)
        .and_then(|place| state.weather.reading(place));
    let temperature = reading.map(|reading| {
        (
            weather::format_temperature(reading.temperature, state.preferences.temperature_unit),
            format!("weather-{}", weather::condition(reading).icon),
        )
    });
    // The date leads, set exactly like the time: one line, one voice.
    let date = i18n::short_date(now);
    let date_style = style;
    let date_width = assets.width(&date, date_style) + 14. * k;
    let clock_width = assets.width(&clock, style);
    let extra = temperature.as_ref().map_or(0., |(text, _)| {
        (18. + 30. + 8.) * k + assets.width(text, style)
    });
    // The avatar: a chosen picture, else the signed-in profile's initial.
    let avatar = state.avatar.is_some() || state.profile.is_some();
    let width =
        40. * k + date_width + clock_width + extra + if avatar { (14. + 50.) * k } else { 0. };
    let pill = Rect::new(1836. - width, 49., width, PILL_HEIGHT);
    // Text, icon and avatar sit on the capsule's centre line.
    let line = |height: f32| pill.y + (pill.h - height) / 2.;
    let first = out.draw.len();
    let fade = pill_shown(state);
    // The capsule is glass over whatever is behind it.
    // Clear glass with white ink in both appearances (over the photo).
    let ink = WHITE;
    out.shadow(pill, pill.h / 2., 48., 4., 0.2);
    out.draw.push(Draw::Soft(Some(SOFT_EDGE)));
    out.glass_over(
        home_backdrop(state, assets),
        pill,
        pill.h / 2.,
        c(PLATTER, 0.34),
    );
    // One thin specular rim, as Apple's glass has: bright where light falls,
    // faint below. (Two stacked rims read as a soft band once the TV
    // doubles every pixel to 4K.)
    out.draw.push(Draw::GradientRing(
        pill,
        pill.h / 2.,
        1.5,
        c(0xffffff, 0.55),
        c(0xffffff, 0.06),
    ));
    let size = 46. * k;
    let circle = Rect::new(
        pill.x + pill.w - 30. * k - size / 2.,
        line(size),
        size,
        size,
    );
    if let Some(picture) = &state.avatar {
        // A new picture (rotating, or just chosen) fades in over the last.
        let fade = state.avatar_from.as_ref().map(|(previous, since)| {
            let t = (crate::clock::since(since).as_secs_f32() / crate::state::AVATAR_FADE).min(1.);
            (previous, t * t * (3. - 2. * t))
        });
        if let Some((previous, t)) = fade {
            out.draw.push(Draw::TintedImage(
                previous.clone(),
                circle,
                size / 2.,
                false,
                [1., 1., 1., 1. - t],
            ));
        }
        let alpha = fade.map_or(1., |(_, t)| t);
        out.draw.push(Draw::TintedImage(
            picture.clone(),
            circle,
            size / 2.,
            false,
            [1., 1., 1., alpha],
        ));
    } else if let Some(profile) = &state.profile {
        // tvOS ends the menu bar with the profile's avatar: here the signed-in
        // LG profile's initial on its colour.
        let circle = circle.inset(-1.);
        out.shape(circle, circle.w / 2., c(profile.color, 1.));
        out.text(
            profile.initial(),
            Rect::new(circle.x, line(30. * k), circle.w, 30. * k),
            style,
            c(0xffffff, 1.),
            true,
        );
    }
    out.draw.push(Draw::Soft(None));
    let mut x = pill.x + 20. * k;
    out.text(
        date,
        Rect::new(x, line(30. * k), date_width, 30. * k),
        date_style,
        ink,
        false,
    );
    x += date_width;
    out.text(
        clock,
        Rect::new(x, line(30. * k), clock_width + 2., 30. * k),
        style,
        ink,
        false,
    );
    x += clock_width + 18. * k;
    if let Some((text, icon)) = temperature {
        out.icon(
            assets,
            &icon,
            Rect::new(x, line(30. * k) - k, 30. * k, 30. * k),
            [ink[0], ink[1], ink[2], 0.9],
        );
        x += 38. * k;
        out.text(
            text,
            Rect::new(x, line(30. * k), 160., 30. * k),
            style,
            ink,
            false,
        );
    }
    out.fade(first, fade, 1.);
}

fn keycap(out: &mut Scene, label: &str, x: f32, y: f32, w: f32) {
    out.border(Rect::new(x, y, w, 30.), 7., 1.5, c(0xffffff, 0.3));
    out.text(
        label,
        Rect::new(x, y + 6., w, 20.),
        text(16., 600, 0.),
        c(0xffffff, 0.7),
        true,
    );
}

pub(super) fn footer(out: &mut Scene, state: &State, assets: &Assets) {
    // tvOS shows no hint bars: not on the home screen, in Move, or Settings.
    if matches!(state.page, Page::Home | Page::Apps | Page::Settings) {
        return;
    }
    // Discoverability hints for beginners, at tvOS caption size and inside the
    // 60pt bottom safe area that overscanning TVs can crop.
    let style = detail_style();
    let color = c(0xffffff, 0.5);
    let y = 984.;
    let mut x = 84.;
    let mut hint = |out: &mut Scene, key: &str, key_width: f32, label: &str| {
        keycap(out, key, x, y, key_width);
        x += key_width + 12.;
        out.text(label, Rect::new(x, y, 400., 30.), style, color, false);
        x += assets.width(label, style) + 36.;
    };
    if state.moving.is_some() {
        hint(out, "← →", 56., i18n::t("move"));
        hint(out, "OK", 40., i18n::t("done"));
    } else if state.page == Page::Settings {
        hint(out, "OK", 40., i18n::t("select"));
        hint(out, "Back", 62., i18n::t("homeLabel"));
        out.text(
            format!("v{}", assets.version.trim()),
            Rect::new(1636., y, 200., 30.),
            style.right(),
            c(0xffffff, 0.4),
            false,
        );
    } else {
        hint(out, "OK", 40., i18n::t("open"));
        // The translated hint already names the gesture ("Hold OK for options").
        out.text(
            i18n::t("options"),
            Rect::new(x, y, 600., 30.),
            style,
            color,
            false,
        );
    }
}

/// The actions of the app in hand, as tvOS 26 controls: small glass
/// capsules against the lifted tile (above a dock tile, below one in the
/// grid), centred on it. No panel and no title: the lit tile says which app
/// they act on. The focused capsule turns white with dark text. Back closes
/// them.
pub(super) fn options(
    out: &mut Scene,
    state: &State,
    assets: &Assets,
    id: &str,
    anchor: Option<Rect>,
) {
    const H: f32 = 58.;
    const GAP: f32 = 14.;
    const PAD: f32 = 24.;
    const ICON: f32 = 24.;
    const SPACE: f32 = 10.;
    let hidden = state.preferences.hidden.iter().any(|item| item == id);
    let mut actions = vec![(i18n::t("move"), "grid-bold")];
    // Settings is never hidden: the way back to hidden apps is inside it.
    if id != crate::state::SETTINGS_TILE {
        actions.push((
            i18n::t(if hidden { "showApp" } else { "hideApp" }),
            if hidden { "eye-bold" } else { "eye-off-bold" },
        ));
    }
    // Destructive, so last and in red (HIG context menus); only where webOS
    // lets the owner remove the app, never dimmed elsewhere.
    let destructive = state.can_uninstall(id).then_some(actions.len());
    if destructive.is_some() {
        actions.push((i18n::t("uninstall"), "trash-bold"));
    }
    let style = text(24., 600, 0.);
    let widths: Vec<f32> = actions
        .iter()
        .map(|(label, _)| PAD + ICON + SPACE + assets.width(label, style) + PAD)
        .collect();
    let total = widths.iter().sum::<f32>() + GAP * (widths.len() - 1) as f32;
    let (centre, y) = match anchor {
        Some(tile) => {
            // Clear of the tile's focus lift, and of the dock's glass edge.
            let overhang = tile.h * super::FOCUS_SCALE / 2.;
            let below = tile.y + tile.h + overhang + 22.;
            (
                tile.x + tile.w / 2.,
                if below + H <= 1020. {
                    below
                } else {
                    tile.y - overhang - 44. - H
                },
            )
        }
        None => (960., 540. - H / 2.),
    };
    let mut x = (centre - total / 2.).clamp(80., 1840. - total);
    out.modal = Some(Rect::new(x, y, total, H));
    let backdrop = home_backdrop(state, assets);
    let (dark, light) = (c(PLATTER, 1.), out.theme.ink(1.));
    for (index, ((label, icon), width)) in actions.iter().zip(&widths).enumerate() {
        let focus = state.focus_amount(&Target::Option(index));
        let base = Rect::new(x, y, *width, H);
        x += width + GAP;
        // The focused capsule lifts a little, as a focused control does.
        let grow = 0.06 * focus;
        let pill = Rect::new(
            base.x - base.w * grow / 2.,
            base.y - H * grow / 2.,
            base.w * (1. + grow),
            H * (1. + grow),
        );
        let radius = pill.h / 2.;
        out.shadow(
            pill,
            radius,
            24. + 8. * focus,
            8. + 6. * focus,
            0.28 + 0.12 * focus,
        );
        out.draw.push(Draw::Soft(Some(SOFT_EDGE)));
        let (top, bottom) = out.theme.lens();
        out.draw
            .push(Draw::Glass(backdrop.clone(), pill, radius, top, bottom));
        out.shape(pill, radius, out.theme.veil(0.34));
        out.shape(pill, radius, c(0xffffff, 0.97 * focus));
        out.draw.push(Draw::Soft(None));
        let (rim_top, rim_bottom) = out.theme.edge(0.5 * (1. - focus), 0.1 * (1. - focus));
        out.draw
            .push(Draw::GradientRing(pill, radius, 1.5, rim_top, rim_bottom));
        let ink: [f32; 4] = if destructive == Some(index) {
            out.theme.destructive(focus)
        } else {
            let ink: [f32; 4] = std::array::from_fn(|i| light[i] + (dark[i] - light[i]) * focus);
            [ink[0], ink[1], ink[2], 1.]
        };
        out.icon(
            assets,
            icon,
            Rect::new(pill.x + PAD, pill.y + (pill.h - ICON) / 2., ICON, ICON),
            ink,
        );
        out.text(
            *label,
            Rect::new(
                pill.x + PAD + ICON + SPACE,
                pill.y + (pill.h - 30.) / 2.,
                *width,
                30.,
            ),
            style,
            ink,
            false,
        );
        out.control(Target::Option(index), base, false);
    }
}

pub(super) fn keyboard(out: &mut Scene, state: &State, assets: &Assets) {
    // tvOS search: large query, one linear keyboard row, live results below.
    let location = state.keyboard_field == TextField::Location;
    let screen = Rect::new(0., 0., 1920., 1080.);
    out.modal = Some(screen);
    // Apple's Linear Keyboard template: #2b2b2d, a 56pt glyph at (104,164),
    // the Bold 57 query from x187, keys on one row, results below a rule.
    // The light appearance's keyboard is Apple's light grey.
    out.shape(screen, 0., out.theme.pick(c(0x2b2b2d, 1.), c(0xf2f2f7, 1.)));
    out.icon(
        assets,
        "search",
        Rect::new(104., 169., 46., 46.),
        out.theme.ink(0.5),
    );
    let typed = state.editing_text();
    let query_style = text(57., 650, 0.).leading(66. / 57.);
    if typed.is_empty() {
        let hint = if location {
            i18n::t("cityPostcode")
        } else {
            i18n::t("search")
        };
        out.text(
            hint,
            Rect::new(187., 159., 1500., 66.),
            query_style,
            out.theme.ink(0.3),
            false,
        );
    } else {
        out.text(
            typed,
            Rect::new(187., 159., 1500., 66.),
            query_style,
            out.theme.ink(1.),
            false,
        );
    }
    let caret_x = 183.
        + if typed.is_empty() {
            0.
        } else {
            assets.width(typed, query_style) + 6.
        };
    out.shape(Rect::new(caret_x, 166., 4., 54.), 2., c(0x0a84ff, 1.));
    let pitch = 1752. / KEYS.len() as f32;
    for (index, key) in KEYS.iter().enumerate() {
        let rect = Rect::new(84. + index as f32 * pitch, 262., pitch, 66.);
        let focused = state.focus == Target::Key(index);
        if focused {
            let plate = Rect::new(rect.x - 8., rect.y - 6., rect.w + 16., rect.h + 12.);
            out.shadow(plate, 14., 20., 12., 0.45);
            out.shape(plate, 14., WHITE);
        }
        let ink = if focused {
            c(PLATTER, 1.)
        } else {
            out.theme.ink(0.62)
        };
        let icon = match *key {
            "Space" => Some("space"),
            "Delete" => Some("delete"),
            "Clear" => Some("close"),
            "Done" => Some("check"),
            _ => None,
        };
        if let Some(icon) = icon {
            out.icon(
                assets,
                icon,
                Rect::new(rect.x + (rect.w - 30.) / 2., rect.y + 18., 30., 30.),
                ink,
            );
        } else {
            out.text(
                key.to_lowercase(),
                Rect::new(rect.x, rect.y + 6., rect.w, 52.),
                text(40., if focused { 650 } else { 500 }, 0.).leading(1.3),
                ink,
                true,
            );
        }
        out.control(Target::Key(index), rect, false);
    }
    out.shape(Rect::new(80., 371., 1760., 1.), 0., out.theme.wash(0.3));
    if !location {
        // Search never offers itself as a result.
        let apps: Vec<_> = state
            .apps()
            .into_iter()
            .filter(|app| app.id != SEARCH_TILE)
            .collect();
        if apps.is_empty() && !typed.is_empty() {
            out.text(
                i18n::format("noMatchesHint", &[("query", typed)]),
                Rect::new(80., 410., 1760., 32.),
                row_style(),
                out.theme.ink(0.5),
                false,
            );
            return;
        }
        // "Top Results": two rows of four large lockups (410 wide, 450 pitch).
        out.text(
            i18n::t("topResults"),
            Rect::new(80., 390., 1760., 38.),
            text(31., 650, 0.),
            out.theme.ink(0.55),
            false,
        );
        let ink = out.theme.ink(1.);
        for (index, app) in apps.iter().take(8).enumerate() {
            let (row, column) = (index / 4, index % 4);
            tile(
                out,
                state,
                assets,
                app,
                Rect::new(
                    80. + column as f32 * 450.,
                    486. + row as f32 * 300.,
                    410.,
                    231.,
                ),
                Some(ink),
                false,
            );
        }
    }
}

/// How far a notification is in (0..1), on its arrive and leave curves.
fn notice_shown(state: &State) -> f32 {
    let Some((_, opened)) = &state.notice else {
        return 0.;
    };
    if state.preferences.reduced_motion {
        return 1.;
    }
    let age = crate::clock::since(opened).as_secs_f32();
    let arrive = 1. - (1. - (age / 0.34).min(1.)).powi(4);
    let leave = ((crate::state::NOTICE_SECONDS - age) / 0.28).clamp(0., 1.);
    arrive.min(leave * leave * (3. - 2. * leave))
}

/// How much of the status pill shows (0-1): on the home the time belongs to
/// Top Shelf and fades as the grid rises; Settings has none.
fn pill_shown(state: &State) -> f32 {
    match state.page {
        Page::Apps => 1. - (state.shown_scroll / (GRID_SCROLL * 0.5)).clamp(0., 1.),
        Page::Settings => 0.,
        Page::Home => 1.,
    }
}

pub(super) fn toast(out: &mut Scene, state: &State, assets: &Assets, notice: &str) {
    // A tvOS 26 banner: a Liquid Glass card under the menu bar with the
    // app's icon and its words, no close button (it leaves on its own, and
    // a click anywhere on it dismisses). It slides in from the screen edge.
    let start = out.draw.len();
    let style = text(24., 500, 0.).leading(31. / 24.);
    let icon = 52.;
    // One line when it fits; otherwise two balanced lines, never a lone
    // word left hanging on the last.
    let natural = assets.width(notice, style) + 4.;
    let text_width = if natural <= 760. {
        natural.max(200.)
    } else {
        (natural / 2. + 60.).min(760.)
    };
    let lines = assets.lines(notice, style, text_width) as f32;
    let width = 22. + icon + 18. + text_width + 30.;
    let height = (lines * 31. + 40.).max(icon + 32.);
    // Top right, right-aligned with the status pill and just beneath it, so
    // the time is never covered; where no pill shows it takes the top.
    let rect = Rect::new(
        1836. - width,
        49. + (PILL_HEIGHT + 16.) * pill_shown(state),
        width,
        height,
    );
    let focused = state.focus == Target::DismissNotice;
    out.shadow(rect, 32., 44., 12., 0.32);
    out.draw.push(Draw::Soft(Some(SOFT_EDGE)));
    out.glass(
        assets,
        &state.preferences.background,
        rect,
        32.,
        out.theme.veil(0.6 - 0.12 * f32::from(u8::from(focused))),
    );
    out.draw.push(Draw::RoundedGradient(
        rect,
        32.,
        c(0xffffff, 0.12),
        c(0xffffff, 0.02),
    ));
    out.draw.push(Draw::Soft(None));
    out.draw.push(Draw::GradientRing(
        rect,
        32.,
        1.5,
        c(0xffffff, 0.5),
        c(0xffffff, if focused { 0.3 } else { 0.06 }),
    ));
    let badge = Rect::new(rect.x + 22., rect.y + (height - icon) / 2., icon, icon);
    out.shape(badge, 13., c(0x25272d, 1.));
    out.image(assets.path("app-icon.png"), badge, 13., false);
    out.text(
        notice,
        Rect::new(
            badge.x + icon + 18.,
            rect.y + (height - lines * 31.) / 2.,
            text_width,
            lines * 31.,
        ),
        style,
        out.theme.ink(1.),
        false,
    );
    if state.overlay.is_none() {
        out.control(Target::DismissNotice, rect, false);
    }
    // In: drops 16pt into place and fades up over 0.34 s, easing out.
    // Out, at the end of its time: lifts back and fades over 0.28 s.
    if !state.preferences.reduced_motion {
        let shown = notice_shown(state);
        out.translate(start, 0., -16. * (1. - shown));
        out.fade(start, shown, 1.);
    }
}
