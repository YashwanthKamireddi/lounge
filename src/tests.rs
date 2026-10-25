use crate::{
    model::{App, Input, InputMode, Preferences, demo_inventory},
    platform::{normalize_apps, normalize_inputs},
    state::{Direction, Effect, Overlay, Page, State, Target, TextField},
    storage, view,
};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

fn ready() -> State {
    let mut state = State::new(Preferences::default());
    state.set_inventory(demo_inventory(), None);
    state
}

fn controls(state: &State) -> Vec<crate::state::Control> {
    let assets = view::Assets {
        root: PathBuf::new(),
        icons: BTreeMap::new(),
        colors: BTreeMap::new(),
        version: "test".into(),
        metrics: BTreeMap::new(),
        tiles: Default::default(),
        heroes: Default::default(),
        artwork: crate::app_artwork::Service::new(false),
    };
    view::scene(state, &assets, time::OffsetDateTime::UNIX_EPOCH).controls
}

#[test]
fn remote_routes_through_the_tvos_home_and_settings() {
    let mut state = ready();
    assert_eq!(state.focus, Target::App("netflix".into()));
    // No tab bar, and the time pill only shows: Up from a dock app without
    // Top Shelf art stays on it.
    state.direction(Direction::Up, &controls(&state));
    assert_eq!(state.focus, Target::App("netflix".into()));
    // With Top Shelf art, Up shows it full screen (Up again stays there);
    // Down restores the dock.
    state.heroes = vec!["netflix".into()];
    state.preferences.top_shelf_art = true;
    state.direction(Direction::Up, &controls(&state));
    assert!(state.expanded && state.scroll < 0.);
    state.direction(Direction::Up, &controls(&state));
    assert!(state.expanded);
    assert_eq!(state.focus, Target::App("netflix".into()));
    state.direction(Direction::Down, &controls(&state));
    assert!(!state.expanded && state.scroll == 0.);
    state.focus = Target::App(crate::state::SETTINGS_TILE.into());
    assert!(state.activate().is_empty());
    assert_eq!(state.page, Page::Settings);
    assert_eq!(state.focus, Target::Sidebar(0));
    state.activate();
    assert_eq!(state.focus, Target::Setting(0));
    assert!(state.back().is_empty());
    assert_eq!(state.focus, Target::Sidebar(0));
    // Up at the top of Settings stays put rather than reaching a removed tab bar.
    state.direction(Direction::Up, &controls(&state));
    assert_eq!(state.focus, Target::Sidebar(0));
    state.back();
    assert_eq!(state.page, Page::Apps);
    // Back on Home opens the app switcher rather than quitting.
    assert_eq!(state.back(), vec![Effect::Running]);
}

#[test]
fn hiding_restores_nearest_app_and_hidden_filter_can_restore_it() {
    let mut state = ready();
    state.open(1);
    state.focus = Target::App("netflix".into());
    state.options();
    state.focus = Target::Option(1);
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert!(state.preferences.hidden.contains(&"netflix".into()));
    assert_eq!(state.focus, Target::App("youtube.leanback.v4".into()));
    assert!(state.apps().iter().all(|app| app.id != "netflix"));
}

#[test]
fn reorder_persists_order_and_keeps_selected_identity() {
    let mut state = ready();
    state.open(1);
    state.focus = Target::App("netflix".into());
    // Holding OK on an app starts Move at once, as on tvOS.
    state.hold();
    assert_eq!(state.moving.as_deref(), Some("netflix"));
    // Arrows write nothing to disk; the order is saved once Move ends.
    assert!(state.direction(Direction::Right, &controls(&state)).is_empty());
    assert!(state.unsaved_order);
    assert_eq!(
        &state.preferences.favorites(&state.inventory.apps)[..2],
        &["youtube.leanback.v4", "netflix"]
    );
    assert_eq!(state.focus, Target::App("netflix".into()));
    assert!(state.back().is_empty());
    assert!(state.moving.is_none());
    assert!(state.unsaved_order, "saving falls to the main loop once Move ends");
}

#[test]
fn search_is_modal_filters_live_and_leaves_home_unfiltered() {
    let mut state = ready();
    state.open(1);
    state.focus = Target::App(crate::state::SEARCH_TILE.into());
    state.activate();
    assert!(
        controls(&state)
            .iter()
            // Keys plus live result cards; Back dismisses, as on tvOS.
            .all(|c| matches!(c.target, Target::Key(_) | Target::App(_)))
    );
    state.text("netflix");
    state.delete();
    state.text("x");
    assert_eq!(state.apps().len(), 1);
    state.back();
    // The last search is kept for next time; Home shows every app again and
    // focus returns to the Search tile.
    assert_eq!(state.query, "netflix");
    assert_eq!(state.focus, Target::App(crate::state::SEARCH_TILE.into()));
    assert!(state.apps().len() > 1);
    assert_eq!(state.page, Page::Apps);
}

#[test]
fn partial_refresh_preserves_valid_inputs_and_never_invents_tv_apps() {
    let mut state = ready();
    state.set_inventory(
        crate::model::Inventory::default(),
        Some("Inputs unavailable".into()),
    );
    // Only Luma's own Settings/Search and input tiles remain; no TV app is made up.
    assert!(
        state
            .inventory
            .apps
            .iter()
            .all(|app| app.brand.starts_with("luma-"))
    );
    assert_eq!(state.inventory.inputs.len(), 3);
}

#[test]
fn inventory_filters_system_apps_duplicates_and_unsafe_identifiers() {
    let apps = normalize_apps(&json!([
        {"id":"netflix","title":" Netflix\n"}, {"id":"netflix","title":"Duplicate"},
        {"id":"bad;command","title":"Bad"}, {"id":"com.webos.app.hidden","title":"Internal"},
        {"id":"com.webos.app.livetv","visible":false,"title":"Live TV"},
        {"id":"com.luma.launcher.native","visible":true},
        {"id":"com.webos.app.hdmi1","visible":true},
        {"id":"hidden","visible":"false"}
    ]))
    .unwrap();
    assert_eq!(apps.len(), 2);
    assert_eq!(
        apps.iter().find(|a| a.id == "netflix").unwrap().title,
        "Netflix"
    );
    assert!(normalize_apps(&json!({"apps":[]})).is_err());
    let inputs = normalize_inputs(&json!([
        {"appId":"com.webos.app.hdmi1","id":"HDMI_1","port":1,"connected":true},
        {"appId":"com.webos.app.hdmi2","id":"HDMI_4"},
        {"appId":"com.webos.app.hdmi3"}, {"appId":"com.webos.app.hdmi3"},
        {"appId":"com.webos.app.hdmi4","label":"Console"}
    ]))
    .unwrap();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[1].connected, None);
}

#[test]
fn icon_collector_visibility_cases_match_the_launcher_inventory() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../tests/fixtures/app-visibility.json")).unwrap();
    let inventory: Vec<_> = cases.iter().map(|case| case["app"].clone()).collect();
    let actual: std::collections::BTreeSet<_> = normalize_apps(&json!(inventory))
        .unwrap()
        .into_iter()
        .map(|app| app.id)
        .collect();
    let expected: std::collections::BTreeSet<_> = cases
        .iter()
        .filter(|case| case["include"] == true)
        .map(|case| case["app"]["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn auto_input_visibility_requires_a_positive_connection_report() {
    let mut preferences = Preferences::default();
    let mut input = Input {
        id: "com.webos.app.hdmi1".into(),
        title: "Console".into(),
        port: "HDMI 1".into(),
        connected: None,
    };
    assert!(!preferences.input_visible(&input));
    input.connected = Some(true);
    assert!(preferences.input_visible(&input));
    preferences
        .input_modes
        .insert(input.id.clone(), InputMode::Off);
    assert!(!preferences.input_visible(&input));
    preferences
        .input_modes
        .insert(input.id.clone(), InputMode::On);
    input.connected = Some(false);
    assert!(preferences.input_visible(&input));
}

#[test]
fn preferences_round_trip_and_corrupt_data_is_not_silently_overwritten() {
    let directory = std::env::temp_dir().join(format!("luma-native-test-{}", std::process::id()));
    let path = directory.join("preferences.json");
    let mut preferences = Preferences {
        favorites: Some(vec![]),
        background: "forest".into(),
        ..Preferences::default()
    };
    preferences
        .input_modes
        .insert("com.webos.app.hdmi1".into(), InputMode::Off);
    storage::save(&path, &preferences).unwrap();
    let loaded = storage::load(&path).unwrap();
    assert_eq!(loaded.favorites, Some(vec![]));
    assert_eq!(loaded.background, "forest");
    assert_eq!(loaded.input_modes["com.webos.app.hdmi1"], InputMode::Off);
    std::fs::write(&path, "broken").unwrap();
    assert!(storage::load(&path).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gallery_and_grid_navigation_scroll_to_keep_focus_visible() {
    let mut state = ready();
    state.open(2);
    state.section = 1;
    state.focus = Target::Background(0);
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.focus, Target::Background(3));
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.focus, Target::Background(6));
    assert_eq!(
        state.scroll, 0.,
        "the web-sized gallery fits all three rows"
    );
    state.direction(Direction::Left, &controls(&state));
    assert_eq!(state.focus, Target::Sidebar(1));
    state.open(1);
    let last = state.apps().last().unwrap().id.clone();
    state.focus(Target::App(last), &controls(&state));
    assert!(
        state.scroll > 0.,
        "library focus includes its wrapped caption"
    );
}

#[test]
fn cancelling_options_preserves_identity_and_does_not_save() {
    let mut state = ready();
    state.focus = Target::App("netflix".into());
    state.options();
    assert!(matches!(state.overlay, Some(Overlay::Options { .. })));
    // tvOS context menus have no close button: Back cancels.
    assert!(state.back().is_empty());
    assert_eq!(state.focus, Target::App("netflix".into()));
    assert!(state.overlay.is_none());
}

#[test]
fn pointer_focus_does_not_scroll_the_clicked_control() {
    let mut state = ready();
    state.open(1);
    state.scroll = 150.;
    state.pointer_focus(Target::App("netflix".into()));
    assert_eq!(state.scroll, 150.);
    // Remote focus on the dock row scrolls the grid back to the top.
    state.focus(Target::App("netflix".into()), &controls(&state));
    assert_eq!(state.scroll, 0.);
}

#[test]
fn gallery_right_crosses_rows_in_gallery_order() {
    let mut state = ready();
    state.open(2);
    state.section = 1;
    state.focus = Target::Background(2);
    state.direction(Direction::Right, &controls(&state));
    assert_eq!(state.focus, Target::Background(3));
}

#[test]
fn reordering_skips_hidden_and_unavailable_favorite_slots() {
    let mut state = ready();
    state.preferences.favorites = Some(vec![
        "netflix".into(),
        "missing".into(),
        "youtube.leanback.v4".into(),
        "amazon".into(),
    ]);
    state.preferences.hidden.push("youtube.leanback.v4".into());
    state.open(1);
    state.focus = Target::App("netflix".into());
    // Move from the options menu; the order becomes exactly what is shown.
    state.options();
    state.focus = Target::Option(0);
    state.activate();
    state.direction(Direction::Right, &controls(&state));
    let order = state.preferences.favorites.clone().unwrap();
    assert_eq!(&order[..2], &["amazon", "netflix"]);
    assert!(!order.contains(&"missing".into()));
    assert!(state.apps().iter().all(|app| app.id != "youtube.leanback.v4"));
    assert_eq!(state.focus, Target::App("netflix".into()));
}

#[test]
fn home_key_closes_settings_and_keeps_focus_on_return() {
    let mut state = ready();
    // Back from an app with nothing open: the home stays as it was left.
    state.open(1);
    state.focus = Target::App("netflix".into());
    state.home_key(false);
    assert_eq!(state.focus, Target::App("netflix".into()));
    // Home pressed inside Settings: Settings closes onto the home screen.
    state.open(2);
    state.focus = Target::Setting(1);
    state.home_key(true);
    assert_eq!(state.page, crate::state::Page::Apps);
    assert!(state.subpage.is_none() && state.overlay.is_none());
    // Home pressed mid-Move ends Move.
    state.focus = Target::App("netflix".into());
    state.hold();
    assert!(state.moving.is_some());
    state.home_key(true);
    assert!(state.moving.is_none());
}

#[test]
fn hidden_apps_come_back_from_general_settings() {
    let mut state = ready();
    state.open(2);
    state.section = 0;
    state.focus = Target::Setting(0);
    assert!(!controls(&state).iter().any(|c| c.target == Target::Setting(7)));
    state.preferences.hidden = vec!["netflix".into(), "youtube.leanback.v4".into()];
    assert!(controls(&state).iter().any(|c| c.target == Target::Setting(7)));
    state.focus = Target::Setting(7);
    state.activate();
    assert_eq!(state.subpage, Some(crate::state::Subpage::Hidden));
    assert_eq!(state.focus, Target::Choice(0));
    // Showing one keeps the list open on the next.
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert_eq!(state.preferences.hidden, vec!["youtube.leanback.v4".to_string()]);
    assert_eq!(state.focus, Target::Choice(0));
    // Showing the last closes it; focus lands on the row below the gone one.
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert!(state.preferences.hidden.is_empty());
    assert!(state.subpage.is_none());
    assert_eq!(state.focus, Target::Setting(3));
}

#[test]
fn clock_format_subpage_saves_and_restores_its_parent_row() {
    let mut state = ready();
    state.open(2);
    state.focus = Target::Setting(1);
    assert!(
        state
            .direction(Direction::Right, &controls(&state))
            .is_empty()
    );
    assert_eq!(state.subpage, Some(crate::state::Subpage::Clock));
    assert_eq!(state.focus, Target::Choice(0));
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert!(state.preferences.hour_24);
    assert!(state.subpage.is_none());
    assert_eq!(state.focus, Target::Setting(1));
    state.activate();
    state.direction(Direction::Left, &controls(&state));
    assert_eq!(state.focus, Target::Setting(1));
    assert!(state.subpage.is_none());
}

#[test]
fn input_visibility_has_explicit_choices_and_returns_to_the_same_input() {
    let mut state = ready();
    state.open(2);
    state.section = crate::state::INPUTS_SECTION;
    let id = "com.webos.app.hdmi1".to_string();
    state.focus = Target::InputMode(id.clone());
    state.activate();
    assert_eq!(state.focus, Target::Choice(2));
    state.direction(Direction::Up, &controls(&state));
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert_eq!(state.preferences.input_modes[&id], InputMode::Off);
    assert_eq!(state.focus, Target::InputMode(id.clone()));
    assert!(!state.preferences.input_visible(&state.inventory.inputs[0]));
    state.activate();
    state.back();
    assert_eq!(state.focus, Target::InputMode(id));
    assert!(state.subpage.is_none());
}

#[test]
fn background_entry_restores_the_applied_background() {
    let mut state = ready();
    state.open(2);
    state.preferences.background = "forest".into();
    state.focus = Target::Sidebar(1);
    state.section = 1;
    state.direction(Direction::Right, &controls(&state));
    assert_eq!(state.focus, Target::Background(6));
    state.back();
    state.activate();
    assert_eq!(state.focus, Target::Background(6));
}

#[test]
fn a_notice_does_not_escape_modal_focus_and_closing_restores_the_app() {
    let mut state = ready();
    state.open(1);
    state.notify("Saved");
    state.focus = Target::App("netflix".into());
    state.options();
    assert!(
        controls(&state)
            .iter()
            .all(|c| matches!(c.target, Target::Option(_)))
    );
    // Move and Hide: Hide is the last row.
    state.focus = Target::Option(1);
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.focus, Target::Option(1));
    state.back();
    assert_eq!(state.focus, Target::App("netflix".into()));
}

#[test]
fn long_captions_stay_within_two_lines_and_clear_the_next_row() {
    let mut state = ready();
    state.open(1);
    // The first app of the grid's first row, and the one below it.
    let columns = crate::state::APPS_COLUMNS;
    let id = state.apps()[columns].id.clone();
    state.inventory.apps.iter_mut().find(|app| app.id == id).unwrap().title = "A very long installed application title that needs more than two lines to remain fully readable".into();
    state.focus = Target::App(id.clone());
    let targets = controls(&state);
    let title = targets
        .iter()
        .find(|c| c.target == Target::App(id.clone()))
        .unwrap();
    let next_id = state.apps()[2 * columns].id.clone();
    let next = targets
        .iter()
        .find(|c| c.target == Target::App(next_id.clone()))
        .unwrap();
    // The tile face (194) plus a caption of at most two lines.
    assert!(title.rect.h > 194. && title.rect.h <= 194. + 26. + 60.);
    assert!(next.rect.y > title.rect.y + title.rect.h);
}

#[test]
fn reduced_motion_disables_panel_wallpaper_and_notification_animation() {
    let mut state = ready();
    state.preferences.reduced_motion = true;
    state.open(2);
    state.focus = Target::Setting(1);
    state.activate();
    assert!(!state.animating());
    state.back();
    state.focus = Target::Background(6);
    state.activate();
    state.notify("Saved");
    assert!(!state.animating());
}

#[test]
fn weather_location_keyboard_back_and_sidebar_routes_restore_focus() {
    let mut state = ready();
    state.open(2);
    state.focus = Target::Sidebar(crate::state::WEATHER_SECTION);
    state.section = crate::state::WEATHER_SECTION;
    state.activate();
    assert_eq!(state.focus, Target::WeatherToggle);
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.focus, Target::WeatherLocation);
    state.activate();
    assert_eq!(state.focus, Target::WeatherQuery);
    state.activate();
    state.text("Vienna");
    state.back();
    assert_eq!(state.focus, Target::WeatherSearch);
    assert_eq!(state.weather.query, "Vienna");
    assert_eq!(state.subpage, Some(crate::state::Subpage::WeatherLocation));
    state.direction(Direction::Left, &controls(&state));
    assert_eq!(state.focus, Target::WeatherLocation);
    assert!(state.subpage.is_none());
}

#[test]
fn weather_units_save_without_replacing_celsius_cache_and_hide_keeps_city() {
    let mut state = ready();
    state.open(2);
    state.section = crate::state::WEATHER_SECTION;
    let place = crate::weather::Location {
        name: "Vienna".into(),
        region: "Vienna".into(),
        country: "Austria".into(),
        latitude: 48.2085,
        longitude: 16.3721,
    };
    state.preferences.weather_location = Some(place.clone());
    state.weather.cache = Some(crate::weather::Cache {
        key: place.key(),
        updated_at: 1000,
        reading: crate::weather::Reading {
            temperature: 14.2,
            code: 2,
            is_day: true,
        },
    });
    state.focus = Target::WeatherTemperature;
    state.activate();
    assert_eq!(state.focus, Target::Choice(0));
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert_eq!(
        state.preferences.temperature_unit,
        crate::weather::Unit::Fahrenheit
    );
    assert_eq!(
        state.weather.cache.as_ref().unwrap().reading.temperature,
        14.2
    );
    assert_eq!(state.focus, Target::WeatherTemperature);
    state.focus = Target::WeatherToggle;
    state.activate();
    assert!(!state.preferences.show_weather);
    assert_eq!(state.preferences.weather_location, Some(place));
}

#[test]
fn removing_weather_location_clears_cache_and_cancels_pending_results() {
    let mut state = ready();
    state.open(2);
    state.section = crate::state::WEATHER_SECTION;
    state.focus = Target::WeatherLocation;
    state.activate();
    state.focus = Target::WeatherClear;
    let effects = state.activate();
    assert_eq!(
        effects,
        vec![
            Effect::Save,
            Effect::Weather(crate::weather::Command::ClearCache)
        ]
    );
    assert!(state.weather.cache.is_none());
    assert!(state.preferences.weather_location.is_none());
    assert_eq!(state.focus, Target::WeatherSearch);
}

#[test]
fn stored_language_preferences_load_without_losing_other_choices() {
    let preferences: Preferences =
        serde_json::from_value(json!({"language":"de","favorites":["netflix"],"show_names":true}))
            .unwrap();
    assert!(preferences.show_names);
    assert_eq!(preferences.favorites, Some(vec!["netflix".into()]));
}

#[test]
fn unrelated_app_ids_and_titles_keep_their_installed_artwork() {
    for (id, title) in [
        ("com.tvweb.dashboard", "Glasshouse"),
        ("com.example.ardent", "Notes"),
        ("com.example.3ard", "Notes"),
        ("com.example.ard3", "Notes"),
        ("com.example.plexus", "Notes"),
        ("com.example.media", "Standard dashboard"),
        ("com.example.media", "netflixer"),
        ("com.example.media", "preNetflix"),
        ("amazon.alexa.view", "Amazon Alexa"),
        ("com.example.spotify", "Spotify tools"),
        ("com.example.media", "Plex"),
        ("com.example.browser", "Web Browser"),
        ("com.example.livetv", "Live TV"),
    ] {
        let app = App::new(id, title);
        assert_eq!(
            (app.brand.as_str(), app.category.as_str()),
            ("generic", "Utilities"),
            "An unrelated application must not be rebranded: {id} / {title}"
        );
    }
}

#[test]
fn curated_branding_does_not_use_title_words_or_partial_id_matches() {
    for (id, title) in [
        ("com.example.ard_more", "Notes"),
        ("com.example.more-ard", "Notes"),
        ("com.example.ARD", "Notes"),
        ("com.example.media", "ARD"),
        ("com.example.media", "(ARD)"),
        ("com.example.media", "ARD미디어테크"),
        ("com.example.media", "아에르데ARD"),
        ("com.example.media", "아에르데ARD미디어테크"),
        ("com.example.dashboard", "ARD"),
        ("com.example.media", "standard ardent ARD"),
    ] {
        let app = App::new(id, title);
        assert_eq!(
            (app.brand.as_str(), app.category.as_str()),
            ("generic", "Utilities"),
            "Only an exact app ID may select curated artwork: {id} / {title}"
        );
    }
}

#[test]
fn known_app_ids_preserve_the_curated_brands_and_categories() {
    let apps = demo_inventory().apps;
    let expected = [
        ("com.webos.app.livetv", "live-tv", "Streaming"),
        ("netflix", "netflix", "Streaming"),
        ("youtube.leanback.v4", "youtube", "Streaming"),
        ("com.apple.appletv", "apple-tv", "Streaming"),
        ("com.disney.disneyplus-prod", "disney-plus", "Streaming"),
        ("amazon", "prime-video", "Streaming"),
        ("plex", "plex", "Streaming"),
        ("spotify-beehive", "spotify", "Music"),
        ("org.jellyfin.webos", "jellyfin", "Streaming"),
        ("org.xbmc.kodi", "kodi", "Streaming"),
        ("com.limelight.webos", "moonlight", "Gaming"),
        ("org.webosbrew.hbchannel", "homebrew", "Utilities"),
        ("com.webos.app.browser", "browser", "Utilities"),
        ("com.arte.tv", "arte", "Streaming"),
        ("de.zdf.app.zdfm3", "zdf", "Streaming"),
        ("ard.mediathek", "das-erste", "Streaming"),
    ];
    for (id, brand, category) in expected {
        let app = apps.iter().find(|app| app.id == id).unwrap();
        assert_eq!(
            (app.brand.as_str(), app.category.as_str()),
            (brand, category),
            "Known app {id} must retain its curated brand and category"
        );
    }
}

#[test]
fn number_keys_open_the_matching_visible_favourite() {
    let mut state = ready();
    let favourites: Vec<String> = state
        .preferences
        .favorites(&state.inventory.apps)
        .into_iter()
        .filter(|id| state.inventory.apps.iter().any(|app| &app.id == id))
        .collect();
    assert_eq!(
        state.quick_launch(0),
        vec![Effect::Launch(favourites[0].clone())]
    );
    state.preferences.hidden.push(favourites[0].clone());
    assert_eq!(
        state.quick_launch(0),
        vec![Effect::Launch(favourites[1].clone())]
    );
    assert!(state.quick_launch(500).is_empty());
}

#[test]
fn move_never_starts_on_a_hidden_app() {
    let mut state = ready();
    state.open(1);
    state.preferences.hidden.push("org.jellyfin.webos".into());
    let before = state.preferences.favorites.clone();
    state.start_move("org.jellyfin.webos");
    assert!(state.moving.is_none());
    assert_eq!(state.preferences.favorites, before);
}

#[test]
fn switcher_closes_every_app_safely_and_back_returns_home_at_once() {
    let mut state = ready();
    state.open(1);
    let home = state.focus.clone();
    state.open_switcher(vec!["netflix".into(), "youtube.leanback.v4".into()]);
    assert_eq!(state.focus, Target::Switch(0));
    // Close both: the second close empties the list without panicking.
    assert_eq!(state.direction(Direction::Up, &[]), vec![Effect::Close("netflix".into())]);
    assert!(state.switcher.as_ref().unwrap().reflow.is_some());
    assert_eq!(
        state.direction(Direction::Up, &[]),
        vec![Effect::Close("youtube.leanback.v4".into())]
    );
    // The last window keeps flying while the home already has focus.
    assert_eq!(state.focus, home);
    assert!(state.switcher.as_ref().unwrap().dismissed.is_some());
    // Keys now reach the home, not the fading switcher.
    state.direction(Direction::Right, &controls(&state));
    assert_ne!(state.focus, Target::Switch(0));

    let mut state = ready();
    state.open(1);
    let home = state.focus.clone();
    state.open_switcher(vec!["netflix".into()]);
    assert!(state.back().is_empty());
    assert_eq!(state.focus, home);
    assert!(state.switcher.as_ref().unwrap().dismissed.is_some());
}

#[test]
fn recent_key_opens_the_switcher_only_from_the_bare_home() {
    let mut state = ready();
    state.open(1);
    assert_eq!(state.recent(), vec![Effect::Running]);
    state.open_keyboard(TextField::Apps);
    assert!(state.recent().is_empty());
    state.back();
    state.open(2);
    assert!(state.recent().is_empty());
}

#[test]
fn the_first_focus_on_launch_appears_settled_without_animating() {
    let mut state = ready();
    state.open(1);
    state.track_focus();
    assert_eq!(state.focus_amount(&state.focus.clone()), 1.);
    assert!(!state.animating());
}


#[test]
fn moving_between_settings_sections_does_not_replay_the_push() {
    let mut state = ready();
    state.open(2);
    state.tick();
    state.page_since = None;
    state.settings_since = None;
    let controls = controls(&state);
    state.direction(Direction::Down, &controls);
    state.tick();
    assert_ne!(state.focus, Target::Sidebar(0));
    assert!(state.settings_since.is_none(), "moving between sections replayed the push");
    // Entering a section still pushes.
    state.activate();
    state.tick();
    assert!(state.settings_since.is_some());
}

#[test]
fn moving_an_app_glides_the_tiles_it_displaces() {
    let mut state = ready();
    state.open(1);
    let apps: Vec<String> = state.apps().iter().map(|app| app.id.clone()).collect();
    let (first, second) = (apps[0].clone(), apps[1].clone());
    state.start_move(&first);
    let shown = controls(&state);
    state.direction(Direction::Right, &shown);
    let (_, from) = state.rearranged.as_ref().expect("a reorder glides");
    // The moved app starts one slot to the left of its new place, its
    // neighbour one to the right; nothing else moved.
    let step = crate::state::GRID_STEP;
    assert_eq!(from.get(&first), Some(&(-step, 0.)));
    assert_eq!(from.get(&second), Some(&(step, 0.)));
    assert_eq!(from.len(), 2);
    // A second press at once carries on from where the tile is drawn: it is
    // still a whole slot back, and now two from its newest place.
    let shown = controls(&state);
    state.direction(Direction::Right, &shown);
    let offset = state.glide_offsets()[&first];
    assert!((offset.0 + 2. * step).abs() < 20., "{offset:?}");
    // Reduced Motion: tiles move at once.
    let mut state = ready();
    state.preferences.reduced_motion = true;
    state.open(1);
    state.start_move(&first);
    let shown = controls(&state);
    state.direction(Direction::Right, &shown);
    assert!(state.rearranged.is_none());
}

#[test]
fn top_shelf_art_waits_for_focus_to_rest_on_a_dock_app() {
    let mut state = ready();
    state.open(1);
    let dock: Vec<String> = state.apps().iter().take(3).map(|app| app.id.clone()).collect();
    state.focus = Target::App(dock[0].clone());
    state.track_focus();
    assert_eq!(state.shelf_app.as_deref(), Some(dock[0].as_str()), "the first app shows at once");
    // A quick sweep: the art does not change while passing.
    state.focus = Target::App(dock[1].clone());
    state.track_focus();
    state.focus = Target::App(dock[2].clone());
    state.track_focus();
    assert_eq!(state.shelf_app.as_deref(), Some(dock[0].as_str()));
    assert!(state.shelf_from.is_none());
    // Resting: the art of the app focus stopped on fades in.
    std::thread::sleep(std::time::Duration::from_secs_f32(crate::state::SHELF_REST + 0.02));
    state.track_focus();
    assert_eq!(state.shelf_app.as_deref(), Some(dock[2].as_str()));
    assert!(state.shelf_from.is_some());
}

#[test]
fn partial_repaint_covers_exactly_what_changed() {
    use crate::{damage, renderer::Draw, state::Rect};
    let tile = |x: f32| Draw::Shape(Rect::new(x, 100., 200., 100.), 30., 0., [1.; 4]);
    let before = vec![tile(0.), tile(300.)];
    // Nothing changed: no repaint area at all is still a valid (empty) area.
    assert!(damage::diff(&before, &before).is_some_and(|r| r.w * r.h == 0.), "nothing changed: nothing to repaint");
    // One tile moved: the area covers its old and new place, not the other.
    let after = vec![tile(0.), tile(320.)];
    let area = damage::diff(&before, &after).expect("a small change repaints a part");
    assert!(area.x <= 300. && area.x + area.w >= 520.);
    assert!(area.x > 200., "the unchanged tile is left alone");
    // A full-screen dissolve repaints everything.
    let mut fading = after.clone();
    fading.push(Draw::Dissolve("a".into(), "b".into(), 0.5, [1.; 4]));
    assert!(damage::diff(&after, &fading).is_none());
    // Buffer age: an unknown or too-old buffer repaints everything; an older
    // one also covers the frames since it was drawn.
    let mut tracker = damage::Tracker::default();
    let small = Some(Rect::new(0., 0., 100., 100.));
    assert!(tracker.region(small, 0).is_none());
    assert!(tracker.region(small, 2).is_none(), "no history yet");
    tracker.presented(Some(Rect::new(500., 500., 100., 100.)));
    let both = tracker.region(small, 2).expect("history covers it");
    assert!(both.x <= 0. && both.x + both.w >= 600.);
    tracker.presented(None);
    assert!(tracker.region(small, 2).is_none(), "a full frame in between repaints all");
}

#[test]
fn the_first_scroll_after_a_pause_glides_rather_than_jumps() {
    let mut state = ready();
    state.open(1);
    state.glide_scroll();
    // Idle: no frames are drawn for a while.
    std::thread::sleep(std::time::Duration::from_millis(120));
    state.scroll = 690.;
    state.glide_scroll();
    assert!(
        state.shown_scroll > 0. && state.shown_scroll < 690. * 0.6,
        "first step went to {}",
        state.shown_scroll
    );
}

#[test]
fn background_writer_keeps_the_newest_preferences_and_finishes_every_save() {
    let directory = std::env::temp_dir().join(format!("luma-writer-{}", std::process::id()));
    let path = directory.join("preferences.json");
    let mut writer = crate::storage::Writer::new(path.clone());
    for background in ["slate", "midnight", "forest"] {
        writer.save(&Preferences {
            background: background.into(),
            ..Preferences::default()
        });
    }
    writer.finish();
    assert_eq!(crate::storage::load(&path).unwrap().background, "forest");
    assert!(writer.errors.try_recv().is_err());
    std::fs::remove_dir_all(&directory).unwrap();
}
