//! Colours, type styles and input descriptions every screen uses.
use crate::{
    i18n,
    model::Input,
    renderer::{Color, TextStyle},
};

/// tvOS tab bar label: SF Pro Bold 29/36.
pub(super) fn tab_style() -> TextStyle {
    text(29., 650, 0.).leading(36. / 29.)
}

/// tvOS glass platter; Luma has no backdrop blur, so it is slightly more opaque.
pub(super) const PLATTER: u32 = 0x1d1d1d;

pub(super) fn c(rgb: u32, alpha: f32) -> Color {
    [
        ((rgb >> 16) & 255) as f32 / 255.,
        ((rgb >> 8) & 255) as f32 / 255.,
        (rgb & 255) as f32 / 255.,
        alpha,
    ]
}
pub(super) fn text(size: f32, weight: u16, tracking: f32) -> TextStyle {
    TextStyle::new(size).weight(weight).tracking(tracking)
}

/// tvOS shelf heading ("Title - Secondary"): SF Pro Bold 38/46.
pub(super) fn shelf_style() -> TextStyle {
    text(38., 650, 0.).leading(46. / 38.)
}

/// tvOS list row label: SF Pro Medium 25/32.
pub(super) fn row_style() -> TextStyle {
    text(25., 500, 0.).leading(32. / 25.)
}

/// tvOS secondary line: SF Pro Medium 23/30.
pub(super) fn detail_style() -> TextStyle {
    text(23., 500, 0.).leading(30. / 23.)
}

pub(super) fn input_icon(input: &Input) -> &'static str {
    let name = input.title.to_lowercase().replace(' ', "");
    if name.contains("playstation") || ["ps1", "ps2", "ps3", "ps4", "ps5"].contains(&name.as_str())
    {
        "playstation"
    } else if name.contains("appletv") {
        "apple-tv"
    } else if name.contains("usb") {
        "usb"
    } else if ["xbox", "switch", "console"]
        .iter()
        .any(|part| name.contains(part))
    {
        "console"
    } else if ["pc", "computer", "laptop"]
        .iter()
        .any(|part| name.contains(part))
    {
        "computer"
    } else if ["stream", "fire", "roku"]
        .iter()
        .any(|part| name.contains(part))
    {
        "streamer"
    } else {
        "hdmi"
    }
}
pub(super) fn input_status(input: &Input) -> String {
    match input.connected {
        Some(false) => i18n::t("noDevice").into(),
        None => i18n::t("externalInput").into(),
        Some(true) if input.title == input.port => i18n::t("connected").into(),
        Some(true) => format!("{} · {}", input.title, i18n::t("connected")),
    }
}
