//! The home page (greeting, weather, favourites, inputs) and the apps page
//! (Top Shelf dock and grid).
use super::{
    Assets, DOCK_RADIUS, FACE_HEIGHT, GRID_PITCH, GRID_VEIL, LABEL_GAP, Scene, TILE_RADIUS,
    glass::{home_backdrop, shelf_art, shelf_dissolve},
    shared::{c, detail_style, input_icon, input_status, row_style, shelf_style, text},
    tile::{caption_height, tile},
};
use crate::{
    i18n,
    renderer::{Color, Draw, TextStyle, WHITE},
    state::{
        APPS_COLUMNS, DOCK_GAP, DOCK_TOP, GRID_GAP, GRID_LEFT, GRID_SCROLL, GRID_STEP,
        HOME_TILE_PITCH, HOME_TILE_WIDTH, ROW_PITCH, Rect, SHELF_HINT, State, Target,
    },
    weather,
};
use time::OffsetDateTime;

pub(super) fn home(out: &mut Scene, state: &State, assets: &Assets, now: OffsetDateTime) {
    let s = state.scroll;
    let summary = state
        .preferences
        .show_weather
        .then(|| weather_summary_layout(state, assets));
    let shift = summary
        .as_ref()
        .map_or(0., |summary| (summary.height + 18.4) / 2.);
    let date = i18n::date(now);
    out.text(
        date,
        Rect::new(84., 214. - shift - s, 1200., 30.),
        text(23., 600, 0.06),
        c(0xf5f5f5, 0.72),
        false,
    );
    let greeting = i18n::greeting(now.hour());
    out.text(
        greeting,
        Rect::new(84., 265. - shift - s, 1500., 95.),
        text(90.4, 500, -0.065).leading(1.05),
        WHITE,
        false,
    );
    if let Some(summary) = summary {
        weather_summary(out, state, assets, summary, 265. - shift + 94.92 + 18.4 - s);
    }
    out.text(
        i18n::t("favorites"),
        Rect::new(84., 466. - s, 1000., 46.),
        shelf_style(),
        WHITE,
        false,
    );
    if state.moving.is_some() {
        out.text(
            i18n::t("moveApps"),
            Rect::new(1536., 474. - s, 300., 30.),
            text(23., 500, 0.),
            c(0xdedede, 1.),
            true,
        );
    }
    let apps = state.apps();
    let names = state.preferences.show_names;
    let mut grid_height = FACE_HEIGHT;
    if state.loading && state.inventory.apps.is_empty() {
        for index in 0..6 {
            out.shape(
                Rect::new(
                    84. + index as f32 * GRID_PITCH,
                    544.8 - s,
                    HOME_TILE_WIDTH,
                    FACE_HEIGHT,
                ),
                TILE_RADIUS,
                c(0xffffff, 0.06),
            );
        }
    } else if apps.is_empty() {
        let error = state.error.is_some() && state.inventory.apps.is_empty();
        empty(
            out,
            state,
            assets,
            EmptyState {
                rect: Rect::new(84., 533.6 - s, 1752., 258.7),
                icon: if error { "alert" } else { "heart" },
                title: if error {
                    i18n::t("aLittleHelp")
                } else {
                    i18n::t("makeFavorites")
                },
                hint: if error {
                    state.error.as_deref().unwrap_or("")
                } else {
                    i18n::t("pinHint")
                },
                action: Some(if error { Target::Retry } else { Target::Browse }),
            },
        );
        grid_height = 247.5;
    }
    if !apps.is_empty() {
        grid_height = FACE_HEIGHT
            + if names {
                LABEL_GAP
                    + apps
                        .iter()
                        .map(|app| caption_height(state, assets, app, HOME_TILE_WIDTH))
                        .fold(49.28, f32::max)
            } else {
                0.
            };
    }
    let strip = out.draw.len();
    for (index, app) in apps.iter().enumerate() {
        tile(
            out,
            state,
            assets,
            app,
            Rect::new(
                84. + index as f32 * HOME_TILE_PITCH - state.home_scroll,
                544.8 - s,
                HOME_TILE_WIDTH,
                FACE_HEIGHT,
            ),
            names.then_some(WHITE),
            false,
        );
    }
    out.translate(strip, state.home_scroll - state.shown_home_scroll, 0.);
    let input_y = 544.8 + grid_height + 41.6 - s;
    let inputs: Vec<_> = state
        .inventory
        .inputs
        .iter()
        .filter(|input| state.preferences.input_visible(input))
        .collect();
    if !inputs.is_empty() {
        out.text(
            i18n::t("inputsHeading"),
            Rect::new(84., input_y - 4., 1000., 46.),
            shelf_style(),
            WHITE,
            false,
        );
    }
    for (index, input) in inputs.iter().enumerate() {
        let base = Rect::new(
            84. + (index % 3) as f32 * 591.45,
            input_y + 62. + (index / 3) as f32 * 146.,
            569.1,
            120.,
        );
        let lift = state.focus_amount(&Target::Input(input.id.clone()));
        let grow = 1. + 0.06 * lift;
        let (cx, cy) = base.center();
        let rect = Rect::new(
            cx - base.w * grow / 2.,
            cy - base.h * grow / 2.,
            base.w * grow,
            base.h * grow,
        );
        let radius = 24. + 4. * lift;
        out.shadow(
            rect,
            radius,
            5. + 45. * lift,
            6. + 24. * lift,
            0.08 + 0.32 * lift,
        );
        // Resting cards use tvOS's light "regular" material; focus deepens it.
        out.shape(rect, radius, c(0xffffff, 0.1 * (1. - lift)));
        out.shape(rect, radius, c(0x2c2c2e, 0.92 * lift));
        out.border(rect, radius, 1., c(0xffffff, 0.08));
        let art = Rect::new(rect.x + 24., rect.y + (rect.h - 64.) / 2., 64., 64.);
        out.draw.push(Draw::RoundedGradient(
            art,
            12.8,
            c(0xd6d6d6, 0.24),
            c(0xa0a0a0, 0.10),
        ));
        let icon = input_icon(input);
        let size = if icon == "apple-tv" { 48. } else { 40. };
        out.icon(
            assets,
            &format!("input-{icon}"),
            Rect::new(
                art.x + (64. - size) / 2.,
                art.y + (64. - size) / 2.,
                size,
                size,
            ),
            c(0xdedede, 1.),
        );
        out.text(
            &input.port,
            Rect::new(rect.x + 108., rect.y + rect.h / 2. - 34., 440., 36.),
            text(29., 600, 0.).leading(36. / 29.),
            WHITE,
            false,
        );
        out.text(
            input_status(input),
            Rect::new(rect.x + 108., rect.y + rect.h / 2. + 4., 440., 30.),
            text(23., 500, 0.).leading(30. / 23.),
            c(0xffffff, 0.6),
            false,
        );
        out.control(Target::Input(input.id.clone()), base, true);
    }
}

struct WeatherSummary {
    height: f32,
    width: f32,
    icon: String,
    parts: Vec<(String, TextStyle, Color, Rect)>,
}
type WeatherPart = (String, TextStyle, Color, f32, f32);
fn weather_summary_layout(state: &State, assets: &Assets) -> WeatherSummary {
    let base = text(23.2, 400, -0.025).leading(1.5);
    let mut parts = Vec::new();
    let mut icon = "location".to_owned();
    if let Some(place) = &state.preferences.weather_location {
        parts.push((place.name.clone(), base.weight(450), c(0xf5f5f5, 0.72)));
        parts.push(("·".into(), base, c(0xf5f5f5, 0.324)));
        if let Some(reading) = state.weather.reading(place) {
            let condition = weather::condition(reading);
            icon = format!("weather-{}", condition.icon);
            parts.push((
                weather::format_temperature(
                    reading.temperature,
                    state.preferences.temperature_unit,
                ),
                base.weight(550),
                c(0xf5f5f5, 0.91),
            ));
            parts.push((i18n::t(condition.key).into(), base, c(0xf5f5f5, 0.58)));
            if state.weather.failed {
                parts.push((
                    i18n::t("lastReading").into(),
                    detail_style(),
                    c(0xf2d5a5, 0.65),
                ));
            }
        } else {
            parts.push((
                if state.weather.loading {
                    i18n::t("loadingWeather")
                } else {
                    i18n::t("weatherUnavailable")
                }
                .into(),
                base,
                c(0xf5f5f5, 0.58),
            ));
        }
    } else {
        parts.push((
            i18n::t("setWeatherLocation").into(),
            base,
            c(0xf5f5f5, 0.72),
        ));
    }
    let mut rows: Vec<Vec<WeatherPart>> = vec![Vec::new()];
    let mut used = 0.;
    for (label, style, color) in parts {
        let measured_width = assets.width(&label, style);
        let width = measured_width.min(1714.4);
        // Use the same extra pixel as the rendered text rectangle. Counting
        // lines at the exact advance width can round into a phantom second
        // line even though the renderer displays the label on one line.
        let text_width = width + 1.;
        let lines = if measured_width <= text_width && !label.contains('\n') {
            1
        } else {
            assets.lines(&label, style, text_width)
        };
        let height = lines as f32 * style.size * style.line_height;
        if used > 0. && used + 11.2 + width > 1714.4 {
            rows.push(Vec::new());
            used = 0.;
        }
        rows.last_mut()
            .unwrap()
            .push((label, style, color, width, height));
        used += width + if used > 0. { 11.2 } else { 0. };
    }
    let mut positioned = Vec::new();
    let mut top = 1.6;
    let mut right: f32 = 0.;
    for row in rows {
        let height = row.iter().map(|part| part.4).fold(34.8, f32::max);
        let mut x = 121.6;
        for (label, style, color, width, item_height) in row {
            positioned.push((
                label,
                style,
                color,
                Rect::new(
                    x,
                    top + (height - item_height) / 2.,
                    width + 1.,
                    item_height,
                ),
            ));
            x += width + 11.2;
        }
        right = right.max(x - 11.2);
        top += height + 11.2;
    }
    WeatherSummary {
        height: top - 11.2 + 1.6,
        width: right - 84.,
        icon,
        parts: positioned,
    }
}
fn weather_summary(
    out: &mut Scene,
    state: &State,
    assets: &Assets,
    summary: WeatherSummary,
    y: f32,
) {
    let label_focus = summary.parts.first().map(|(_, style, _, rect)| {
        // The label's layout line box includes leading above and below the
        // text. Keep the focus ring snug to the label rather than the whole
        // weather row, which also contains the location icon.
        let leading = style.size * (style.line_height - 1.).max(0.);
        Rect::new(
            rect.x,
            y + rect.y + leading / 2.,
            rect.w,
            (rect.h - leading).max(style.size),
        )
    });
    out.icon(
        assets,
        &summary.icon,
        Rect::new(84., y + 5.8, 26.4, 26.4),
        c(0xdedede, 0.83),
    );
    for (label, style, color, rect) in summary.parts {
        out.text(
            label,
            Rect::new(rect.x, rect.y + y, rect.w, rect.h),
            style,
            color,
            false,
        );
    }
    if state.preferences.weather_location.is_none() {
        let rect = Rect::new(84., y, summary.width, summary.height);
        if state.focus == Target::WeatherConfigure
            && !state.pointer
            && let Some(label) = label_focus
        {
            out.border(label.inset(-5.), 8.6, 2., c(0xf0f0f0, 0.95));
        }
        out.control(Target::WeatherConfigure, rect, true);
    }
}

pub(super) fn library(out: &mut Scene, state: &State, assets: &Assets) {
    // tvOS home: the dock platter (1820x274, r72) holds
    // the first row; the grid follows. Dock icons carry no labels; grid icons
    // show their name only when focused.
    let s = state.scroll;
    let apps = state.apps();
    if apps.is_empty() {
        let title = if state.loading {
            i18n::t("findingApps")
        } else {
            i18n::t("roomForNew")
        };
        let hint = state
            .error
            .clone()
            .unwrap_or_else(|| i18n::t("noCategoryApps").into());
        empty(
            out,
            state,
            assets,
            EmptyState {
                rect: Rect::new(84., 600., 1752., 304.),
                icon: "grid",
                title,
                hint: &hint,
                action: state.error.as_ref().map(|_| Target::Retry),
            },
        );
        return;
    }
    let dock_y = DOCK_TOP - s;
    // Under the fold the first grid row sits 230 below the dock; as the grid
    // rises it eases out to 260, following the drawn (gliding) scroll.
    let spread = (state.shown_scroll / GRID_SCROLL).clamp(0., 1.);
    let gap = DOCK_GAP + (GRID_GAP - DOCK_GAP) * spread;
    // Liquid Glass platter: exactly what is behind it, blurred, lifted a
    // touch, with a top sheen and a bright rim for the edge. The blur
    // dissolves with Top Shelf.
    let platter = Rect::new(50., dock_y - 40., 1820., FACE_HEIGHT + 80.);
    let backdrop = home_backdrop(state, assets);
    // tvOS 26 dock: clear dark glass with a whisper of light at the top. The
    // light is the glass's own tint rising above 1 toward the top (a white
    // 9%-to-2% sheen on typical glass), so it costs no layer of its own:
    // a quarter of the screen less to paint on every scrolling frame.
    // Clear glass in both appearances, as Liquid Glass is.
    let glass = |out: &mut Scene, path: String, [r, g, b]: [f32; 3], alpha: f32| {
        let lit = |k: f32| [r * k, g * k, b * k, alpha];
        out.draw.push(Draw::Glass(
            path,
            platter,
            DOCK_RADIUS,
            lit(1.19),
            lit(1.045),
        ));
    };
    if spread < 1. {
        if let Some((previous, t)) = shelf_dissolve(state) {
            let (_, previous) = shelf_art(state, assets, previous);
            glass(out, previous.clone(), [1.; 3], 1.);
            if previous != backdrop {
                glass(out, backdrop, [1.; 3], t);
            }
        } else {
            glass(out, backdrop, [1.; 3], 1.);
        }
    }
    // Over the grid the platter blurs the grid's backdrop, blending in with
    // the drawn scroll exactly as the screen behind it does.
    if spread > 0. {
        glass(
            out,
            assets.path(format!(
                "wallpapers/{}-glass.png",
                state.preferences.background
            )),
            [GRID_VEIL, GRID_VEIL, GRID_VEIL + 0.02],
            spread,
        );
    }
    // One thin specular rim (no flat outline).
    out.draw.push(Draw::GradientRing(
        platter,
        DOCK_RADIUS,
        1.5,
        c(0xffffff, 0.5),
        c(0xffffff, 0.06),
    ));
    let slot = |index: usize| {
        let (row, column) = (index / APPS_COLUMNS, index % APPS_COLUMNS);
        let y = if row == 0 {
            dock_y
        } else {
            dock_y + gap + (row - 1) as f32 * ROW_PITCH
        };
        (GRID_LEFT + column as f32 * GRID_STEP, y)
    };
    // Move mode: a reorder glides every displaced tile (dock and grid alike)
    // from its old slot to its new one, easing out, as tvOS rearranges.
    let glide = state.glide_offsets();
    for (index, app) in apps.iter().enumerate() {
        let row = index / APPS_COLUMNS;
        let (mut x, mut y) = slot(index);
        if let Some((dx, dy)) = glide.get(&app.id) {
            x += dx;
            y += dy;
        }
        let focused = state.focus == Target::App(app.id.clone());
        tile(
            out,
            state,
            assets,
            app,
            Rect::new(x, y, HOME_TILE_WIDTH, FACE_HEIGHT),
            (row > 0 && (state.preferences.show_names || focused)).then_some(WHITE),
            false,
        );
    }
    shelf_chevron(out, state, assets, dock_y);
}

/// tvOS "Swipe up for fullscreen": a compact chevron centred above the dock
/// while a dock app with Top Shelf art has focus (only those go full screen). It bobs a few times when
/// Top Shelf mode begins, then rests (an idle home renders no frames).
fn shelf_chevron(out: &mut Scene, state: &State, assets: &Assets, dock_y: f32) {
    let Target::App(id) = &state.focus else {
        return;
    };
    let in_dock = state
        .apps()
        .iter()
        .take(APPS_COLUMNS)
        .any(|app| &app.id == id);
    if !in_dock || !state.has_art(id) || state.expanded {
        return;
    }
    let alpha = (1. - state.shown_scroll.abs() / 60.).clamp(0., 1.);
    if alpha <= 0. {
        return;
    }
    let lift = state
        .shelf_since
        .filter(|_| !state.preferences.reduced_motion)
        .map_or(0., |since| {
            let t = crate::clock::since(since).as_secs_f32();
            if t >= SHELF_HINT {
                0.
            } else {
                // Rise and settle: 0 → -8 → 0 every 0.8 s.
                -8. * (t / 0.8 * std::f32::consts::PI).sin().abs()
            }
        });
    out.icon(
        assets,
        "chevron-up",
        Rect::new(928., dock_y - 104. + lift, 64., 64.),
        c(0xffffff, 0.8 * alpha),
    );
}

struct EmptyState<'a> {
    rect: Rect,
    icon: &'a str,
    title: &'a str,
    hint: &'a str,
    action: Option<Target>,
}
fn empty(out: &mut Scene, state: &State, assets: &Assets, content: EmptyState<'_>) {
    let EmptyState {
        rect,
        icon,
        title,
        hint,
        action,
    } = content;
    out.shape(rect, 17.6, c(0xffffff, 0.02));
    out.border(rect, 17.6, 1., c(0xffffff, 0.05));
    let center = rect.x + rect.w / 2.;
    let top = rect.y
        + if action.is_some() {
            33.
        } else {
            (rect.h - 123.2) / 2.
        };
    out.icon(
        assets,
        icon,
        Rect::new(center - 17.6, top, 35.2, 35.2),
        c(0xd8d8d8, 0.60),
    );
    out.text(
        title,
        Rect::new(rect.x + 32., top + 52., rect.w - 64., 31.),
        text(24., 550, -0.035),
        WHITE,
        true,
    );
    out.text(
        hint,
        Rect::new(center - 352., top + 97.6, 704., 64.),
        text(16., 400, 0.).leading(1.6),
        c(0xf5f5f5, 0.72),
        true,
    );
    if let Some(target) = action {
        let label = i18n::t(if target == Target::Retry {
            "retry"
        } else {
            "exploreApps"
        });
        let width = assets.width(label, row_style()) + 80.;
        let button = Rect::new(center - width / 2., top + 140., width, 56.);
        out.shape(button, 11.2, c(0xededed, 1.));
        if state.focus == target {
            out.border(button.inset(-7.), 18.2, 2., WHITE);
        }
        out.text(
            label,
            Rect::new(button.x + 20.8, button.y + 12., button.w - 80., 32.),
            row_style(),
            c(0x242424, 1.),
            false,
        );
        out.icon(
            assets,
            "arrow",
            Rect::new(button.x + button.w - 38.4, button.y + 14.6, 17.6, 17.6),
            c(0x242424, 1.),
        );
        out.control(target, button, true);
    }
}
