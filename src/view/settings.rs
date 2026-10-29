//! Settings: the section list and each section's page.
use super::{
    Assets, MAIN_TOP, Scene,
    shared::{PLATTER, c, detail_style, row_style, shelf_style, text},
};
use crate::{
    i18n,
    model::{Appearance, BACKGROUNDS},
    renderer::Draw,
    state::{
        APPEARANCES, AVATAR_ROTATE, Page, Rect, SETTINGS_PUSH_MS, State, Subpage, Target,
        WEATHER_SECTION,
    },
    weather::Unit,
};

/// tvOS Settings: the list owns the right half; the left shows the section.
const LIST_X: f32 = 860.;
const LIST_W: f32 = 920.;

struct Row<'a> {
    target: Target,
    rect: Rect,
    title: &'a str,
    icon: Option<&'a str>,
    value: Option<&'a str>,
    description: Option<&'a str>,
    selected: bool,
    chevron: bool,
}

/// tvOS list item: a quiet light material at rest, a lifted white plate
/// `lift` of the way in. Focus moving down a list hands the plate over as
/// tvOS does, one row's growing in as the last one's settles back, rather
/// than jumping to the next row in one frame.
fn list_plate(out: &mut Scene, rect: Rect, lift: f32, selected: bool) {
    // tvOS 27 rows are near-capsules, at rest and focused.
    let radius = (rect.h / 2.).min(36.);
    if lift > 0. {
        let plate = Rect::new(
            rect.x - 6. * lift,
            rect.y - 2. * lift,
            rect.w + 12. * lift,
            rect.h + 4. * lift,
        );
        out.shadow(plate, radius + 2. * lift, 31., 21. * lift, 0.3 * lift);
        out.shape(plate, radius + 2. * lift, c(0xffffff, 0.95 * lift));
    } else if selected {
        // tvOS lists are plain text at rest; only a current choice is tinted.
        out.shape(rect, radius, out.theme.wash(0.1));
    }
}

fn row(out: &mut Scene, state: &State, assets: &Assets, row: Row<'_>) {
    let lift = state.focus_amount(&row.target);
    // tvOS Settings: every row sits on a faint capsule over the tinted
    // backdrop; focus turns it solid white.
    let plain = state.page == Page::Settings;
    if plain && lift < 1. {
        out.shape(
            row.rect,
            (row.rect.h / 2.).min(36.),
            out.theme.wash(0.08 * (1. - lift)),
        );
    }
    list_plate(out, row.rect, lift, false);
    let theme = out.theme;
    let ink = |alpha: f32| -> crate::renderer::Color {
        let (rest, lifted) = (theme.ink(alpha), c(PLATTER, alpha));
        std::array::from_fn(|i| rest[i] + (lifted[i] - rest[i]) * lift)
    };
    let mut x = row.rect.x + 30.;
    // tvOS Settings rows carry no glyphs: title, grey value, then a chevron
    // where the row opens a page, or a check on the current choice.
    if let Some(icon) = row.icon.filter(|_| !plain) {
        out.icon(
            assets,
            icon,
            Rect::new(x, row.rect.y + (row.rect.h - 28.) / 2., 28., 28.),
            ink(0.7),
        );
        x += 46.;
    }
    let line = row_style().size * row_style().line_height;
    let title_y = row.rect.y
        + if row.description.is_some() {
            18.
        } else {
            (row.rect.h - line) / 2.
        };
    let title_width = row.rect.w - (x - row.rect.x) - if row.value.is_some() { 260. } else { 70. };
    let title_height = if row.description.is_some() {
        assets.lines(row.title, row_style(), title_width) as f32 * line
    } else {
        line
    };
    out.text(
        row.title,
        Rect::new(x, title_y, title_width, title_height),
        row_style(),
        ink(1.),
        false,
    );
    if let Some(description) = row.description {
        out.text(
            description,
            Rect::new(
                x,
                title_y + title_height + 2.,
                row.rect.w - (x - row.rect.x) - 40.,
                (row.rect.h - 38. - title_height).max(30.),
            ),
            detail_style(),
            ink(0.6),
            false,
        );
    }
    if let Some(value) = row.value {
        out.text(
            value,
            Rect::new(
                row.rect.x + row.rect.w - 250. - if row.chevron { 36. } else { 0. },
                row.rect.y + (row.rect.h - line) / 2.,
                224.,
                line,
            ),
            row_style().right(),
            ink(0.6),
            false,
        );
    }
    if row.chevron || row.selected {
        out.icon(
            assets,
            if row.selected { "check" } else { "right" },
            Rect::new(
                row.rect.x + row.rect.w - 48.,
                row.rect.y + (row.rect.h - 22.) / 2.,
                22.,
                22.,
            ),
            ink(if row.selected { 1. } else { 0.5 }),
        );
    }
    out.control(row.target, row.rect, true);
}

fn weather_settings(out: &mut Scene, state: &State, assets: &Assets) {
    let s = state.scroll;
    row(
        out,
        state,
        assets,
        Row {
            target: Target::WeatherToggle,
            rect: Rect::new(LIST_X, 164.8 - s, LIST_W, 72.),
            title: i18n::t("showWidget"),
            icon: Some("eye"),
            value: Some(if state.preferences.show_weather {
                i18n::t("stateOn")
            } else {
                i18n::t("stateOff")
            }),
            description: None,
            selected: false,
            chevron: false,
        },
    );
    let city = state
        .preferences
        .weather_location
        .as_ref()
        .map(|place| place.name.as_str());
    // The place reads as every other row's value: same size, same ink.
    let style = row_style();
    let line = 32.;
    let city_lines = city.map_or(1, |city| assets.lines(city, style, 450.));
    let height = (34. + city_lines as f32 * line).max(72.);
    let rect = Rect::new(LIST_X, 264.8 - s, LIST_W, height);
    row(
        out,
        state,
        assets,
        Row {
            target: Target::WeatherLocation,
            rect,
            title: i18n::t("location"),
            icon: Some("location"),
            // No city yet: say so, as tvOS does, rather than an empty row.
            value: city.is_none().then(|| i18n::t("notSet")),
            description: None,
            selected: false,
            chevron: true,
        },
    );
    if let Some(city) = city {
        out.text(
            city,
            Rect::new(
                rect.x + rect.w - 508.6,
                rect.y + (height - city_lines as f32 * line) / 2.,
                450.,
                city_lines as f32 * line,
            ),
            style.right(),
            if state.focus == Target::WeatherLocation {
                c(0x444444, 1.)
            } else {
                out.theme.ink(0.6)
            },
            false,
        );
    }
    let y = rect.y + height + 12.;
    row(
        out,
        state,
        assets,
        Row {
            target: Target::WeatherTemperature,
            rect: Rect::new(LIST_X, y, LIST_W, 72.),
            title: i18n::t("temperature"),
            icon: Some("thermometer"),
            value: Some(state.preferences.temperature_unit.symbol()),
            description: None,
            selected: false,
            chevron: true,
        },
    );
}

fn location_settings(out: &mut Scene, state: &State, assets: &Assets) {
    let mut y = 164.8 - state.scroll;
    if let Some(place) = &state.preferences.weather_location {
        let rect = Rect::new(LIST_X, y, LIST_W, 76.2);
        out.shape(rect, 20., out.theme.well(0.72));
        out.border(rect, 20., 1., out.theme.wash(0.04));
        out.icon(
            assets,
            "location",
            Rect::new(LIST_X + 24.2, y + 29.1, 18., 18.),
            out.theme.ink(0.85),
        );
        out.text(
            &place.name,
            Rect::new(LIST_X + 58.2, y + 8., LIST_W - 200., 32.),
            row_style(),
            out.theme.ink(1.),
            false,
        );
        out.text(
            place.detail(),
            Rect::new(LIST_X + 58.2, y + 40., LIST_W - 200., 30.),
            detail_style(),
            out.theme.ink(0.56),
            false,
        );
        let clear = Rect::new(LIST_X + LIST_W - 64.2, y + 18.1, 40., 40.);
        out.shape(clear, 20., out.theme.well(0.5));
        out.border(clear, 20., 1., out.theme.wash(0.07));
        out.icon(assets, "close", clear.inset(12.), out.theme.ink(1.));
        if state.focus == Target::WeatherClear {
            out.border(clear.inset(-2.), 22., 2., out.theme.ink(0.95));
        }
        out.control(Target::WeatherClear, clear, true);
        y += 96.2;
    }
    let query = Rect::new(LIST_X, y, LIST_W - 76., 64.);
    let focused = state.focus == Target::WeatherQuery;
    out.shape(
        query,
        48.,
        if focused {
            out.theme.pick(c(0x303438, 1.), c(0xffffff, 1.))
        } else {
            out.theme.well(0.72)
        },
    );
    out.border(
        query,
        48.,
        1.,
        out.theme.wash(if focused { 0.70 } else { 0.07 }),
    );
    if focused {
        out.border(query.inset(-3.), 51., 3., out.theme.wash(0.063));
    }
    out.icon(
        assets,
        "search",
        Rect::new(LIST_X + 24.2, y + 21.2, 21.6, 21.6),
        out.theme.ink(0.73),
    );
    out.text(
        if state.weather.query.is_empty() {
            i18n::t("cityPostcode")
        } else {
            &state.weather.query
        },
        Rect::new(LIST_X + 59.4, y + 16., LIST_W - 170., 32.),
        row_style(),
        out.theme.ink(0.73),
        false,
    );
    out.control(Target::WeatherQuery, query, true);
    let search = Rect::new(LIST_X + LIST_W - 64., y, 64., 64.);
    out.shape(search, 32., out.theme.well(0.72));
    out.border(search, 32., 1., out.theme.wash(0.07));
    out.icon(
        assets,
        if state.weather.searching {
            "refresh"
        } else {
            "search"
        },
        search.inset(20.8),
        out.theme.ink(1.),
    );
    if state.focus == Target::WeatherSearch {
        out.border(search.inset(-7.), 39., 2., out.theme.ink(0.95));
    }
    out.control(Target::WeatherSearch, search, true);
    y += 84.;
    if state.weather.searching || state.weather.message.is_some() {
        out.text(
            if state.weather.searching {
                i18n::t("searching")
            } else {
                i18n::t(state.weather.message.as_deref().unwrap_or(""))
            },
            Rect::new(LIST_X + 24.2, y, LIST_W - 48.4, 58.),
            text(16., 400, 0.).leading(1.5),
            out.theme.ink(0.68),
            false,
        );
    }
    for (index, place) in state.weather.results.iter().enumerate() {
        let detail = place.detail();
        let height = (38.
            + assets.lines(&place.name, row_style(), LIST_W - 96.) as f32 * 32.
            + 2.
            + assets.lines(&detail, detail_style(), LIST_W - 86.) as f32 * 30.)
            .max(104.);
        let rect = Rect::new(LIST_X, y, LIST_W, height);
        row(
            out,
            state,
            assets,
            Row {
                target: Target::WeatherResult(index),
                rect,
                title: &place.name,
                icon: Some("location"),
                value: None,
                description: Some(&detail),
                selected: false,
                chevron: false,
            },
        );
        if state
            .preferences
            .weather_location
            .as_ref()
            .is_some_and(|saved| saved.key() == place.key())
        {
            out.icon(
                assets,
                "check",
                Rect::new(LIST_X + LIST_W - 42.6, y + (height - 18.4) / 2., 18.4, 18.4),
                if state.focus == Target::WeatherResult(index) {
                    c(0x444444, 1.)
                } else {
                    out.theme.ink(0.61)
                },
            );
        }
        y += height + 12.;
    }
}

/// An appearance's name in Settings, and the line under Automatic.
fn appearance_label(appearance: Appearance) -> (&'static str, Option<&'static str>) {
    match appearance {
        Appearance::Light => ("appearanceLight", None),
        Appearance::Dark => ("appearanceDark", None),
        Appearance::Auto => ("appearanceAuto", Some("appearanceAutoDetail")),
    }
}

/// Settings' sections and their symbols.
const SECTIONS: [(&str, &str); 3] = [
    ("interface", "settings"),
    ("background", "tv"),
    ("weather", "weather-partly-cloudy"),
];

pub(super) fn settings(out: &mut Scene, state: &State, assets: &Assets) {
    // tvOS Settings: one list at a time on the right. At the top level it
    // lists the sections; inside one, that section's rows.
    let root = matches!(state.focus, Target::Sidebar(_));
    // A page opened from a row is titled with that row's name, as on tvOS.
    let title = match (&state.subpage, root) {
        (_, true) => i18n::t("settings"),
        (Some(Subpage::Clock), _) => i18n::t("clockFormat"),
        (Some(Subpage::Appearance), _) => i18n::t("appearance"),
        (Some(Subpage::Hidden), _) => i18n::t("hiddenApps"),
        (Some(Subpage::Avatar), _) => i18n::t("profilePicture"),
        (Some(Subpage::WeatherLocation), _) => i18n::t("location"),
        (Some(Subpage::Temperature), _) => i18n::t("temperature"),
        (None, false) => i18n::t(SECTIONS[state.section.min(SECTIONS.len() - 1)].0),
    };
    // tvOS push: the page being left slides away and fades as the new one
    // slides in from the side it comes from and fades up, together, so no
    // frame is ever blank or dims what was already on screen.
    // Opacity is staggered as Apple does: the old page is gone by half way
    // and the new one comes up from 15% to 65%, so the two titles never sit
    // over each other at half strength, yet together they never fall below
    // about 60% (a later start left the screen nearly empty for a moment).
    // Movement keeps one smooth ease.
    let push = state.settings_since.map(|(since, direction)| {
        let t = (crate::clock::since(since).as_secs_f32() * 1000. / SETTINGS_PUSH_MS).min(1.);
        (t, 1. - (1. - t).powi(5), direction)
    });
    let smooth = |from: f32, to: f32, t: f32| {
        let x = ((t - from) / (to - from)).clamp(0., 1.);
        x * x * (3. - 2. * x)
    };
    // The left half, as tvOS 27: what the page is about, over a grey line
    // saying what it is for. It stays put while the list pushes: it only
    // changes between the section list and a section, and then the new one
    // fades in over the old, never dipping through the backdrop.
    let last = SECTIONS.len() - 1;
    let shown = (!root).then_some(state.section.min(last));
    let (depth, section) = state.settings_previous;
    let before = (depth != 0).then_some(section.min(last));
    out.draw.push(Draw::Clip(None));
    match push {
        // Different panels cross: the old one goes as the new one comes.
        // The new one comes up over the old, which goes only once it is
        // mostly there.
        // Two frosted cards in the same place are one card: it stays whole and
        // only the glyphs and words cross (two half-faded cards dipped).
        Some((t, _, _)) if state.settings_ghost.is_some() && before != shown => {
            let shared = glyph_card(state, before) && glyph_card(state, shown);
            if shared {
                about_card(out);
            }
            let start = out.draw.len();
            about(out, state, assets, before, !shared);
            out.fade(start, 1. - smooth(0.35, 0.9, t), 1.);
            let start = out.draw.len();
            about(out, state, assets, shown, !shared);
            out.fade(start, smooth(0.1, 0.7, t), 1.);
        }
        _ => about(out, state, assets, shown, true),
    }
    if let (Some((t, eased, direction)), Some(ghost)) = (push, &state.settings_ghost) {
        let start = out.draw.len();
        out.draw.extend(ghost.iter().cloned());
        out.translate(start, -direction * 48. * eased, 0.);
        out.fade(start, 1. - smooth(0., 0.5, t), 1.);
    }
    // The title and the list push together; only they make up the page a
    // later push fades out.
    let page = out.draw.len();
    out.draw.push(Draw::Clip(None));
    // tvOS titles Settings in a large, muted grey, not white.
    out.text(
        title,
        Rect::new(0., 52., 1920., 46.),
        shelf_style(),
        out.theme.ink(0.6),
        true,
    );
    out.draw.push(Draw::Clip(Some(Rect::new(
        0.,
        MAIN_TOP,
        1920.,
        968. - MAIN_TOP,
    ))));
    let list = out.draw.len();
    settings_list(out, state, assets);
    out.settings_range = Some(page..out.draw.len());
    if let Some((t, eased, direction)) = push {
        out.translate(list, direction * 48. * (1. - eased), 0.);
        let alpha = if state.settings_ghost.is_some() {
            smooth(0.15, 0.65, t)
        } else {
            1.
        };
        out.fade(page, alpha, 1.);
    }
}

/// The left panel for the section list (`None`) or a section. Background
/// shows the picture itself; the others a frosted card with a solid glyph
/// (solid, so overlapping strokes never double up as darker patches).
/// Whether `section`'s left panel is the frosted card with a glyph (not the
/// wallpaper picture or a profile picture).
fn glyph_card(state: &State, section: Option<usize>) -> bool {
    section != Some(1) && !(section == Some(0) && state.subpage == Some(Subpage::Avatar))
}

const ABOUT_CARD: Rect = Rect::new(190., 250., 480., 480.);

fn about_card(out: &mut Scene) {
    out.shape(ABOUT_CARD, 110., out.theme.wash(0.2));
}

fn about(out: &mut Scene, state: &State, assets: &Assets, section: Option<usize>, card: bool) {
    // Choosing a profile picture: the focused one, large and round.
    if section == Some(0) && state.subpage == Some(Subpage::Avatar) {
        let choices = state.avatar_choices();
        let focused = match state.focus {
            Target::Choice(index) => choices.get(index),
            _ => None,
        };
        let picture = focused
            .and_then(|choice| state.avatars.iter().find(|picture| &picture.file == choice))
            .or_else(|| {
                state
                    .avatars
                    .iter()
                    .find(|picture| Some(&picture.small) == state.avatar.as_ref())
            })
            .or(state.avatars.first());
        if let Some(picture) = picture {
            let circle = Rect::new(230., 250., 400., 400.);
            out.shadow(circle, 200., 40., 18., 0.35);
            out.draw.push(Draw::Soft(Some(0.8)));
            out.image(picture.large.clone(), circle, 200., false);
            out.draw.push(Draw::Soft(None));
        }
        out.text(
            i18n::t("aboutAvatar"),
            Rect::new(110., 780., 640., 120.),
            detail_style(),
            out.theme.ink(0.62),
            true,
        );
        return;
    }
    let (about, glyph) = match section {
        Some(0) if state.subpage == Some(Subpage::Hidden) => ("aboutHidden", "gear"),
        None => ("aboutSettings", "tv-bold"),
        Some(section) => [
            (
                if state.heroes.is_empty() {
                    "aboutInterfaceNoArt"
                } else {
                    "aboutInterface"
                },
                "gear",
            ),
            ("aboutBackground", ""),
            ("aboutWeather", "weather-partly-cloudy"),
        ][section],
    };
    if glyph.is_empty() {
        let frame = Rect::new(110., 310., 640., 360.);
        out.shadow(frame, 36., 40., 18., 0.35);
        let photo = |id: &str| assets.path(format!("wallpapers/{id}.jpg"));
        // A wallpaper just picked fades in over the last, with the backdrop.
        match state.wallpaper_change() {
            Some((previous, t)) => {
                out.image(photo(previous), frame, 36., false);
                if t > 0. {
                    out.draw.push(Draw::TintedImage(
                        photo(state.wallpaper()),
                        frame,
                        36.,
                        false,
                        [1., 1., 1., t],
                    ));
                }
            }
            None => out.image(photo(state.wallpaper()), frame, 36., false),
        }
    } else {
        if card {
            about_card(out);
        }
        out.icon(
            assets,
            glyph,
            ABOUT_CARD.inset(130.),
            [0.15, 0.16, 0.19, 1.],
        );
    }
    out.text(
        i18n::t(about),
        Rect::new(110., 780., 640., 120.),
        detail_style(),
        out.theme.ink(0.62),
        true,
    );
}

fn settings_list(out: &mut Scene, state: &State, assets: &Assets) {
    let s = state.scroll;
    let root = matches!(state.focus, Target::Sidebar(_));
    if root {
        for (index, (key, icon)) in SECTIONS.into_iter().enumerate() {
            row(
                out,
                state,
                assets,
                Row {
                    target: Target::Sidebar(index),
                    rect: Rect::new(LIST_X, 164.8 + index as f32 * 84. - s, LIST_W, 72.),
                    title: i18n::t(key),
                    icon: Some(icon),
                    value: None,
                    description: None,
                    selected: false,
                    chevron: true,
                },
            );
        }
        return;
    }
    if let Some(subpage) = &state.subpage {
        let start = out.draw.len();
        if *subpage == Subpage::WeatherLocation {
            location_settings(out, state, assets);
            if let Some(opened) = state.panel_started {
                out.fade(
                    start,
                    (crate::clock::since(opened).as_secs_f32() / 0.15).min(1.),
                    1.,
                );
            }
            return;
        }
        let options: Vec<(&str, Option<&str>, bool)> = match subpage {
            Subpage::Appearance => APPEARANCES
                .iter()
                .map(|appearance| {
                    let (title, description) = appearance_label(*appearance);
                    (
                        i18n::t(title),
                        description.map(i18n::t),
                        state.preferences.appearance == *appearance,
                    )
                })
                .collect(),
            Subpage::Clock => vec![
                (i18n::t("hour12"), None, !state.hour_24),
                (i18n::t("hour24"), None, state.hour_24),
            ],
            Subpage::Temperature => vec![
                (
                    i18n::t("celsius"),
                    None,
                    state.preferences.temperature_unit == Unit::Celsius,
                ),
                (
                    i18n::t("fahrenheit"),
                    None,
                    state.preferences.temperature_unit == Unit::Fahrenheit,
                ),
            ],
            Subpage::Hidden => state
                .hidden_apps()
                .into_iter()
                .map(|app| (app.title.as_str(), None, false))
                .collect(),
            Subpage::Avatar => {
                let chosen = state.avatar_choice();
                state
                    .avatar_choices()
                    .iter()
                    .map(|choice| {
                        let title = if choice == AVATAR_ROTATE {
                            i18n::t("rotateAll")
                        } else {
                            state
                                .avatars
                                .iter()
                                .find(|picture| &picture.file == choice)
                                .map_or("", |picture| picture.title.as_str())
                        };
                        (title, None, chosen.as_ref() == Some(choice))
                    })
                    .collect()
            }
            Subpage::WeatherLocation => unreachable!(),
        };
        for (index, (title, description, selected)) in options.into_iter().enumerate() {
            let h = if description.is_some() { 90.7 } else { 72. };
            row(
                out,
                state,
                assets,
                Row {
                    target: Target::Choice(index),
                    rect: Rect::new(LIST_X, 164.8 + index as f32 * (h + 12.) - s, LIST_W, h),
                    title,
                    icon: None,
                    value: None,
                    description,
                    selected,
                    chevron: false,
                },
            );
        }
        if let Some(opened) = state.panel_started {
            out.fade(
                start,
                (crate::clock::since(opened).as_secs_f32() / 0.15).min(1.),
                1.,
            );
        }
        return;
    }
    match state.section {
        0 => {
            let mut shown = 0;
            for (position, (index, key, icon, value, chevron)) in [
                (
                    0,
                    "showAppNames",
                    "type",
                    if state.preferences.show_names {
                        i18n::t("stateOn")
                    } else {
                        i18n::t("stateOff")
                    },
                    false,
                ),
                (
                    1,
                    "clockFormat",
                    "clock",
                    if state.hour_24 {
                        i18n::t("hour24")
                    } else {
                        i18n::t("hour12")
                    },
                    true,
                ),
                (
                    9,
                    "appearance",
                    "moon",
                    i18n::t(appearance_label(state.preferences.appearance).0),
                    true,
                ),
                (
                    2,
                    "reducedMotion",
                    "motion",
                    if state.preferences.reduced_motion {
                        i18n::t("stateOn")
                    } else {
                        i18n::t("stateOff")
                    },
                    false,
                ),
                (
                    11,
                    "focusOutline",
                    "eye-bold",
                    if state.preferences.focus_outline {
                        i18n::t("stateOn")
                    } else {
                        i18n::t("stateOff")
                    },
                    false,
                ),
                (
                    12,
                    "largeNames",
                    "type",
                    if state.preferences.large_names {
                        i18n::t("stateOn")
                    } else {
                        i18n::t("stateOff")
                    },
                    false,
                ),
                (
                    10,
                    "tvHome",
                    "tv",
                    if state.tv_home_busy {
                        "…"
                    } else if state.tv_home == Some(true) {
                        i18n::t("stateOn")
                    } else {
                        i18n::t("stateOff")
                    },
                    false,
                ),
                (
                    6,
                    "topShelfArt",
                    "sparkles",
                    if state.preferences.top_shelf_art {
                        i18n::t("stateOn")
                    } else {
                        i18n::t("stateOff")
                    },
                    false,
                ),
            ]
            .into_iter()
            // Top Shelf art is pictures the owner adds; with none installed
            // the switch would change nothing, so it is not offered.
            .filter(|(index, ..)| *index != 6 || !state.heroes.is_empty())
            // Home is set on the TV, once it has said which it is.
            .filter(|(index, ..)| *index != 10 || state.tv_home.is_some())
            .enumerate()
            {
                shown += 1;
                row(
                    out,
                    state,
                    assets,
                    Row {
                        target: Target::Setting(index),
                        rect: Rect::new(LIST_X, 164.8 + position as f32 * 84. - s, LIST_W, 72.),
                        title: i18n::t(key),
                        icon: Some(icon),
                        value: Some(value),
                        description: None,
                        selected: false,
                        chevron,
                    },
                );
            }
            // The rows after the switches start below the last one shown.
            let mut below = (shown as f32 - 5.) * 84.;
            // The profile picture, when the owner keeps any.
            if !state.avatars.is_empty() {
                let shown = match state.preferences.avatar.as_deref() {
                    Some(AVATAR_ROTATE) => i18n::t("rotateAll"),
                    chosen => state
                        .avatars
                        .iter()
                        .find(|picture| Some(picture.file.as_str()) == chosen)
                        .or(state.avatars.first())
                        .map_or("", |picture| picture.title.as_str()),
                };
                row(
                    out,
                    state,
                    assets,
                    Row {
                        target: Target::Setting(8),
                        rect: Rect::new(LIST_X, 600.8 + below - s, LIST_W, 72.),
                        title: i18n::t("profilePicture"),
                        icon: None,
                        value: Some(shown),
                        description: None,
                        selected: false,
                        chevron: true,
                    },
                );
                below += 84.;
            }
            // Hidden apps come back from here; the row shows only while any are.
            let hidden = state.hidden_count();
            if hidden > 0 {
                let count = hidden.to_string();
                row(
                    out,
                    state,
                    assets,
                    Row {
                        target: Target::Setting(7),
                        rect: Rect::new(LIST_X, 600.8 + below - s, LIST_W, 72.),
                        title: i18n::t("hiddenApps"),
                        icon: Some("eye-off-bold"),
                        value: Some(&count),
                        description: None,
                        selected: false,
                        chevron: true,
                    },
                );
                below += 84.;
            }
            // No refresh row: installs, removals and plugged-in devices update the grid by
            // themselves (platform::watch_apps, watch_inputs). As the TV's Home there is no Quit
            // either: it left the screen empty until Home was pressed again.
            if !crate::platform::is_home() {
                row(
                    out,
                    state,
                    assets,
                    Row {
                        target: Target::Setting(5),
                        rect: Rect::new(LIST_X, 600.8 + below - s, LIST_W, 72.),
                        title: i18n::t("quitApp"),
                        icon: Some("power"),
                        value: None,
                        description: None,
                        selected: false,
                        chevron: false,
                    },
                );
            }
        }
        1 => {
            for (index, (id, _title)) in BACKGROUNDS.iter().enumerate() {
                let w = (LIST_W - 40.) / 3.;
                let rect = Rect::new(
                    LIST_X + (index % 3) as f32 * (w + 20.),
                    164.8 + (index / 3) as f32 * (w * 9. / 16. + 62.) - s,
                    w,
                    w * 9. / 16.,
                );
                out.shadow(rect, 16., 15., 3., 0.13);
                out.image(
                    assets.path(format!("thumbnails/{id}.jpg")),
                    rect,
                    16.,
                    false,
                );
                out.border(rect, 16., 1., out.theme.wash(0.086));
                if state.preferences.background == *id {
                    out.border(
                        rect.inset(-2.),
                        18.,
                        2.,
                        out.theme.pick(c(0xdedede, 1.), c(0x1d1d1f, 1.)),
                    );
                    out.border(rect.inset(-7.), 23., 3., out.theme.wash(0.1));
                    let check = Rect::new(rect.x + rect.w - 28., rect.y + rect.h - 28., 21.6, 21.6);
                    out.shape(check, 10.8, c(0xf3f3f3, 1.));
                    out.icon(assets, "check", check.inset(3.6), c(0x333333, 1.));
                }
                if state.focus == Target::Background(index) {
                    out.ring(rect, 16.);
                }
                out.text(
                    i18n::wallpaper(id),
                    Rect::new(rect.x + 4., rect.y + rect.h + 12., rect.w - 8., 30.),
                    detail_style(),
                    out.theme.ink(0.87),
                    false,
                );
                out.control(
                    Target::Background(index),
                    Rect::new(rect.x, rect.y, rect.w, rect.h + 37.8),
                    true,
                );
            }
        }
        WEATHER_SECTION => weather_settings(out, state, assets),
        _ => {}
    }
}
