//! Bundled English message catalog and date/clock formatting.
use std::{collections::BTreeMap, sync::OnceLock};
use time::OffsetDateTime;

fn messages() -> &'static BTreeMap<String, String> {
    static CATALOG: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/messages.json"))
            .expect("Bundled messages must be valid")
    })
}

pub fn t(key: &str) -> &str {
    messages().get(key).map_or(key, String::as_str)
}
pub fn format(key: &str, values: &[(&str, &str)]) -> String {
    let mut value = t(key).to_owned();
    for (name, replacement) in values {
        value = value.replace(&format!("{{{name}}}"), replacement);
    }
    value
}
pub fn wallpaper(id: &str) -> &str {
    t(match id {
        "dunes" => "monumentValley",
        "san-francisco" => "sanFrancisco",
        "northern-lights" => "northernLights",
        _ => id,
    })
}
pub fn greeting(hour: u8) -> &'static str {
    t(match hour {
        0..=4 => "greetingNight",
        5..=11 => "morning",
        12..=17 => "afternoon",
        _ => "evening",
    })
}
/// The menu bar's short date: "Sat 25 Oct".
pub fn short_date(now: OffsetDateTime) -> String {
    format!(
        "{} {} {}",
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
            [now.weekday().number_days_from_monday() as usize],
        now.day(),
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
        ][now.month() as usize - 1]
    )
}
pub fn date(now: OffsetDateTime) -> String {
    format!("{}, {} {}", now.weekday(), now.month(), now.day()).to_uppercase()
}
pub fn clock(now: OffsetDateTime, hour24: bool) -> String {
    if hour24 {
        return format!("{:02}:{:02}", now.hour(), now.minute());
    }
    let hour = match now.hour() % 12 {
        0 => 12,
        value => value,
    };
    format!(
        "{hour}:{:02} {}",
        now.minute(),
        if now.hour() >= 12 { "PM" } else { "AM" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_message_is_empty() {
        for (key, message) in messages() {
            assert!(!message.is_empty(), "Empty message {key}");
        }
    }
    #[test]
    fn formatting_keeps_user_provided_names_intact() {
        assert_eq!(
            format("favoriteAdded", &[("app", "Netflix")]),
            "Netflix added to favorites."
        );
    }
    #[test]
    fn clock_and_date_use_english_formatting() {
        let now = OffsetDateTime::parse(
            "2026-10-21T20:41:00+02:00",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        assert_eq!(date(now), "WEDNESDAY, OCTOBER 21");
        assert_eq!(short_date(now), "Wed 21 Oct");
        assert_eq!(clock(now, false), "8:41 PM");
        assert_eq!(clock(now, true), "20:41");
    }
}
