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

fn test_assets() -> view::Assets {
    view::Assets {
        root: PathBuf::new(),
        icons: BTreeMap::new(),
        colors: BTreeMap::new(),
        version: "test".into(),
        metrics: BTreeMap::new(),
        tiles: Default::default(),
        heroes: Default::default(),
        artwork: crate::app_artwork::Service::new(false),
    }
}

fn controls(state: &State) -> Vec<crate::state::Control> {
    view::scene(state, &test_assets(), time::OffsetDateTime::UNIX_EPOCH).controls
}

/// The full-screen picture passes a frame draws, in order.
fn backgrounds(state: &State) -> Vec<crate::renderer::Backdrop> {
    view::scene(state, &test_assets(), time::OffsetDateTime::UNIX_EPOCH)
        .draw
        .into_iter()
        .filter_map(|draw| match draw {
            crate::renderer::Draw::Dissolve(backdrop) => Some(backdrop),
            _ => None,
        })
        .collect()
}

fn ui_text(state: &State) -> Vec<String> {
    view::scene(state, &test_assets(), time::OffsetDateTime::UNIX_EPOCH)
        .draw
        .into_iter()
        .filter_map(|draw| match draw {
            crate::renderer::Draw::Text(value, ..) => Some(value),
            _ => None,
        })
        .collect()
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
    // Opening Settings asks the TV where the Home key goes until it has said.
    assert_eq!(state.activate(), vec![Effect::TvHome(None)]);
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
    // Holding OK on an app opens its menu, Move first.
    state.hold();
    assert_eq!(state.focus, Target::Option(0));
    state.activate();
    assert_eq!(state.moving.as_deref(), Some("netflix"));
    // Arrows write nothing to disk; the order is saved once Move ends.
    assert!(
        state
            .direction(Direction::Right, &controls(&state))
            .is_empty()
    );
    assert!(state.unsaved_order);
    assert_eq!(
        &state.preferences.favorites(&state.inventory.apps)[..2],
        &["youtube.leanback.v4", "netflix"]
    );
    assert_eq!(state.focus, Target::App("netflix".into()));
    assert!(state.back().is_empty());
    assert!(state.moving.is_none());
    assert!(
        state.unsaved_order,
        "saving falls to the main loop once Move ends"
    );
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
    // Only Lounge's own Settings/Search and input tiles remain; no TV app is made up.
    assert!(
        state
            .inventory
            .apps
            .iter()
            .all(|app| app.brand.starts_with("lounge-"))
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
fn an_input_is_in_use_by_whichever_report_means_something_on_this_firmware() {
    // A 65UQ801C0SB (webOS 7.6): only HDMI 2 connected, cable sense false on
    // every port (the device on it in standby).
    let uq80 = normalize_inputs(&json!([
        {"appId":"com.webos.app.hdmi1","label":"HDMI 1","connected":false,"hdmiPlugIn":false},
        {"appId":"com.webos.app.hdmi2","label":"HDMI 2","connected":true,"hdmiPlugIn":false},
        {"appId":"com.webos.app.hdmi3","label":"HDMI 3","connected":false,"hdmiPlugIn":false}
    ]))
    .unwrap();
    let connected: Vec<_> = uq80.iter().map(|input| input.connected).collect();
    assert_eq!(connected, [Some(false), Some(true), Some(false)]);
    // Firmware that reports every port connected: the cable sense decides.
    let everywhere = normalize_inputs(&json!([
        {"appId":"com.webos.app.hdmi1","connected":true,"hdmiPlugIn":true},
        {"appId":"com.webos.app.hdmi2","connected":true,"hdmiPlugIn":false}
    ]))
    .unwrap();
    let connected: Vec<_> = everywhere.iter().map(|input| input.connected).collect();
    assert_eq!(connected, [Some(true), Some(false)]);
}

#[test]
fn luma_preferences_keep_settings_and_search_in_place() {
    let mut preferences: Preferences = serde_json::from_value(json!({
        "favorites": ["netflix", "luma.settings", "luma.search", "youtube.leanback.v4"],
        "hidden": ["luma.search"],
    }))
    .unwrap();
    preferences.sanitize();
    assert_eq!(
        preferences.favorites.unwrap(),
        [
            "netflix",
            "lounge.settings",
            "lounge.search",
            "youtube.leanback.v4"
        ]
    );
    assert_eq!(preferences.hidden, ["lounge.search"]);
}

#[test]
fn the_lounge_as_home_switch_flips_once_per_answer_from_the_tv() {
    let mut state = State::new(Preferences::default());
    state.focus = Target::Setting(10);
    // Unknown until the TV has said: nothing to flip.
    assert!(state.activate().is_empty());
    state.tv_home = Some(false);
    assert_eq!(state.activate(), vec![Effect::TvHome(Some(true))]);
    assert!(state.tv_home_busy);
    // A second press while the TV is still changing it does nothing.
    assert!(state.activate().is_empty());
    state.tv_home_busy = false;
    state.tv_home = Some(true);
    assert_eq!(state.activate(), vec![Effect::TvHome(Some(false))]);
}

#[test]
fn an_hdmi_input_moves_like_an_app_and_a_new_one_lands_after_search() {
    let mut state = State::new(Preferences::default());
    state.inventory = crate::model::demo_inventory();
    state.refresh_tiles();
    state.page = Page::Apps;
    let order =
        |state: &State| -> Vec<String> { state.apps().iter().map(|app| app.id.clone()).collect() };
    let at = |state: &State, id: &str| order(state).iter().position(|item| item == id).unwrap();
    let hdmi = "com.webos.app.hdmi1";
    // Unplaced, it follows Search.
    assert_eq!(at(&state, hdmi), at(&state, crate::state::SEARCH_TILE) + 1);
    let before = at(&state, hdmi);
    state.focus = Target::App(hdmi.into());
    state.hold();
    state.activate();
    assert_eq!(
        state.moving.as_deref(),
        Some(hdmi),
        "Move from the menu works on an input"
    );
    state.direction(Direction::Right, &[]);
    assert_eq!(
        at(&state, hdmi),
        before + 1,
        "it moves one place, and stays there"
    );
    state.direction(Direction::Right, &[]);
    assert_eq!(at(&state, hdmi), before + 2);
    state.activate();
    assert!(state.moving.is_none());
    assert_eq!(at(&state, hdmi), before + 2, "dropped where it was put");
    // A device plugged into HDMI 3 later has no place yet: after Search.
    state.inventory.inputs[2].connected = Some(true);
    state.refresh_tiles();
    assert_eq!(
        at(&state, "com.webos.app.hdmi3"),
        at(&state, crate::state::SEARCH_TILE) + 1
    );
    assert_eq!(
        at(&state, hdmi),
        before + 3,
        "the placed input keeps its place"
    );
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
    let directory = std::env::temp_dir().join(format!("lounge-test-{}", std::process::id()));
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
    // Starting up, a damaged file is kept aside (never overwritten) and Lounge
    // starts with defaults rather than not at all.
    let recovered = storage::load_or_recover(&path).unwrap();
    assert_eq!(recovered.background, Preferences::default().background);
    assert!(!path.exists());
    let kept: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("preferences.json.bad-")
        })
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read(kept[0].path()).unwrap(), b"broken");
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
    assert!(
        state
            .apps()
            .iter()
            .all(|app| app.id != "youtube.leanback.v4")
    );
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
    state.focus = Target::Setting(0);
    state.home_key(true);
    assert_eq!(state.page, crate::state::Page::Apps);
    assert!(state.subpage.is_none() && state.overlay.is_none());
    // Home pressed mid-Move ends Move.
    state.focus = Target::App("netflix".into());
    state.hold();
    state.activate();
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
    assert!(
        !controls(&state)
            .iter()
            .any(|c| c.target == Target::Setting(7))
    );
    state.preferences.hidden = vec!["netflix".into(), "youtube.leanback.v4".into()];
    assert!(
        controls(&state)
            .iter()
            .any(|c| c.target == Target::Setting(7))
    );
    state.focus = Target::Setting(7);
    state.activate();
    assert_eq!(state.subpage, Some(crate::state::Subpage::Hidden));
    assert_eq!(state.focus, Target::Choice(0));
    // Showing one keeps the list open on the next.
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert_eq!(
        state.preferences.hidden,
        vec!["youtube.leanback.v4".to_string()]
    );
    assert_eq!(state.focus, Target::Choice(0));
    // Showing the last closes it; focus lands on the row below the gone one.
    assert_eq!(state.activate(), vec![Effect::Save]);
    assert!(state.preferences.hidden.is_empty());
    assert!(state.subpage.is_none());
    assert_eq!(state.focus, Target::Setting(3));
}

#[test]
fn the_clock_format_is_the_tvs_own_setting() {
    // LG's own value decides: "HH24" or "HH12" from the system service.
    assert_eq!(
        crate::time_format::parse(&json!({"returnValue": true, "timeFormat": "HH24"})),
        Some(true)
    );
    assert_eq!(
        crate::time_format::parse(&json!({"timeFormat": "HH12"})),
        Some(false)
    );
    assert_eq!(
        crate::time_format::parse(&json!({"timeFormat": "HHMM"})),
        None
    );
    assert_eq!(
        crate::time_format::parse(&json!({"returnValue": false})),
        None
    );
    assert_eq!(
        crate::time_format::payload(true),
        json!({"timeFormat": "HH24"})
    );
    // A preferences file from before loads whole; its old Lounge-only
    // choice is no longer kept.
    let preferences: Preferences = serde_json::from_value(json!({
        "background": "forest",
        "hour_24": true,
    }))
    .unwrap();
    assert_eq!(preferences.background, "forest");
    assert!(
        !serde_json::to_string(&preferences)
            .unwrap()
            .contains("hour_24")
    );
    // Clock format opens on what the TV has, and a choice sets the TV (not
    // Lounge's preferences file), returning to its row.
    let mut state = ready();
    state.open(2);
    state.focus = Target::Setting(1);
    state.direction(Direction::Right, &controls(&state));
    assert_eq!(state.subpage, Some(crate::state::Subpage::Clock));
    assert_eq!(state.focus, Target::Choice(0));
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.activate(), vec![Effect::ClockFormat(true)]);
    assert!(state.hour_24);
    assert!(state.subpage.is_none());
    assert_eq!(state.focus, Target::Setting(1));
    assert!(ui_text(&state).iter().any(|line| line == "24-hour"));
    state.activate();
    assert_eq!(state.focus, Target::Choice(1));
    state.direction(Direction::Left, &controls(&state));
    assert_eq!(state.focus, Target::Setting(1));
    // The pill shows the TV's format.
    state.open(1);
    let assets = test_assets();
    let one_pm = time::OffsetDateTime::UNIX_EPOCH + time::Duration::hours(13);
    let shown = |state: &State| {
        view::scene(state, &assets, one_pm)
            .draw
            .into_iter()
            .filter_map(|draw| match draw {
                crate::renderer::Draw::Text(value, ..) => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(shown(&state).iter().any(|line| line == "13:00"));
    state.hour_24 = false;
    assert!(shown(&state).iter().any(|line| line == "1:00 PM"));
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
    // Move, Hide, Uninstall: one row of pills, nothing below them.
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
fn a_new_wallpaper_waits_for_its_pictures_then_fades_in_under_settings_tint() {
    crate::clock::record();
    let mut state = ready();
    state.open(2);
    state.section = 1;
    state.focus = Target::Background(6);
    state.glide_scroll();
    state.tick();
    // Past every opening animation, Top Shelf's chevron hint included.
    crate::clock::step(std::time::Duration::from_secs(3));
    state.tick();
    let old = state.preferences.background.clone();
    let [rest] = backgrounds(&state).try_into().unwrap();
    state.activate();
    // Until the new pictures are on the GPU the old wallpaper stays, whole,
    // and nothing animates (or asks for the new pictures on the drawing thread).
    assert_eq!(state.wallpaper(), old);
    assert!(!state.animating());
    assert_eq!(backgrounds(&state), std::slice::from_ref(&rest));
    state.wallpaper_ready();
    assert!(state.animating());
    let fade = std::time::Duration::from_secs_f32(crate::state::WALLPAPER_FADE);
    crate::clock::step(fade / 2);
    let [middle] = backgrounds(&state).try_into().unwrap();
    // One pass from the old blurred picture to the new, both in exactly the
    // resting tint and lift: no colour steps at either end.
    assert_eq!(middle.from, rest.from);
    assert!(middle.to.contains(&state.preferences.background));
    assert!(middle.mix > 0.4 && middle.mix < 0.6);
    assert_eq!(middle.tint, rest.tint);
    assert_eq!(middle.lift, rest.lift);
    crate::clock::step(fade);
    state.tick();
    assert!(state.previous_background.is_none());
    let [end] = backgrounds(&state).try_into().unwrap();
    assert_eq!(
        end.single().map(|(_, tint)| tint),
        rest.single().map(|(_, tint)| tint)
    );
    assert!(end.from.contains(&state.preferences.background));
}

#[test]
fn a_wallpaper_picked_mid_fade_waits_and_then_fades_in_from_where_that_one_ended() {
    crate::clock::record();
    let mut state = ready();
    state.open(2);
    state.section = 1;
    state.focus = Target::Background(6);
    state.activate();
    let first = state.preferences.background.clone();
    state.wallpaper_ready();
    let fade = std::time::Duration::from_secs_f32(crate::state::WALLPAPER_FADE);
    crate::clock::step(fade / 2);
    let before = backgrounds(&state).pop().unwrap();
    // Another pick mid-way: the running fade carries on unchanged.
    state.focus = Target::Background(7);
    state.activate();
    let second = state.preferences.background.clone();
    assert_ne!(first, second);
    let after = backgrounds(&state).pop().unwrap();
    assert_eq!((&after.from, &after.to), (&before.from, &before.to));
    assert!((after.mix - before.mix).abs() < 1e-6);
    // At its end the screen rests on the first pick, whole, until the second
    // pick's pictures are ready, then fades from it to the second.
    crate::clock::step(fade);
    state.tick();
    assert_eq!(state.wallpaper(), first);
    assert!(state.wallpaper_change().is_none());
    state.wallpaper_ready();
    crate::clock::step(fade / 2);
    // Every pass on screen (the page may still be opening) fades the same way.
    for next in backgrounds(&state) {
        assert!(next.from.contains(&first) && next.to.contains(&second));
    }
    crate::clock::step(fade);
    state.tick();
    assert!(state.previous_background.is_none() && state.fading_to.is_none());
    assert_eq!(state.wallpaper(), second);
}

#[test]
fn leaving_settings_dissolves_its_backdrop_into_the_photo_in_one_pass() {
    crate::clock::record();
    let mut state = ready();
    state.open(2);
    state.glide_scroll();
    crate::clock::step(std::time::Duration::from_secs(1));
    state.tick();
    let [settings] = backgrounds(&state).try_into().unwrap();
    state.open(1);
    state.glide_scroll();
    // The first frame home is exactly the Settings backdrop on screen: no
    // cut to the sharp photo, and no second full-screen layer.
    let [first] = backgrounds(&state).try_into().unwrap();
    assert_eq!(first.single(), settings.single());
    crate::clock::step(std::time::Duration::from_millis(100));
    let [middle] = backgrounds(&state).try_into().unwrap();
    assert_eq!(middle.from, settings.from);
    assert!(middle.to.ends_with(".jpg"));
    assert!(middle.mix > 0. && middle.mix < 1.);
    crate::clock::step(std::time::Duration::from_secs(1));
    state.tick();
    let [home] = backgrounds(&state).try_into().unwrap();
    assert_eq!(home.single(), Some((middle.to.as_str(), [1.; 4])));
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
    assert_eq!(
        state.direction(Direction::Up, &[]),
        vec![Effect::Close("netflix".into())]
    );
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
    assert!(
        state.settings_since.is_none(),
        "moving between sections replayed the push"
    );
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
    let dock: Vec<String> = state
        .apps()
        .iter()
        .take(3)
        .map(|app| app.id.clone())
        .collect();
    state.focus = Target::App(dock[0].clone());
    state.track_focus();
    assert_eq!(
        state.shelf_app.as_deref(),
        Some(dock[0].as_str()),
        "the first app shows at once"
    );
    // A quick sweep: the art does not change while passing.
    state.focus = Target::App(dock[1].clone());
    state.track_focus();
    state.focus = Target::App(dock[2].clone());
    state.track_focus();
    assert_eq!(state.shelf_app.as_deref(), Some(dock[0].as_str()));
    assert!(state.shelf_from.is_none());
    // Resting: the art of the app focus stopped on fades in.
    std::thread::sleep(std::time::Duration::from_secs_f32(
        crate::state::SHELF_REST + 0.02,
    ));
    state.track_focus();
    assert_eq!(state.shelf_app.as_deref(), Some(dock[2].as_str()));
    assert!(state.shelf_from.is_some());
}

#[test]
fn a_draw_inserted_mid_list_repaints_only_itself() {
    use crate::{damage, renderer::Draw, state::Rect};
    let tile = |x: f32| Draw::Shape(Rect::new(x, 100., 200., 100.), 30., 0., [1.; 4]);
    let row: Vec<Draw> = (0..8).map(|i| tile(i as f32 * 220.)).collect();
    // A focus ring appears around the second tile: every later index
    // shifts, but only the ring's own area changed.
    let mut ringed = row.clone();
    ringed.insert(
        2,
        Draw::Shape(Rect::new(210., 90., 220., 120.), 34., 2., [1.; 4]),
    );
    let area = damage::diff(&row, &ringed).expect("a small change repaints a part");
    assert!(
        area.x >= 200. && area.x + area.w <= 440.,
        "repaints {area:?}"
    );
    // And the ring going away again repaints only where it was.
    let area = damage::diff(&ringed, &row).expect("a small change repaints a part");
    assert!(
        area.x >= 200. && area.x + area.w <= 440.,
        "repaints {area:?}"
    );
}

#[test]
fn partial_repaint_covers_exactly_what_changed() {
    use crate::{damage, renderer::Draw, state::Rect};
    let tile = |x: f32| Draw::Shape(Rect::new(x, 100., 200., 100.), 30., 0., [1.; 4]);
    let before = vec![tile(0.), tile(300.)];
    // Nothing changed: no repaint area at all is still a valid (empty) area.
    assert!(
        damage::diff(&before, &before).is_some_and(|r| r.w * r.h == 0.),
        "nothing changed: nothing to repaint"
    );
    // One tile moved: the area covers its old and new place, not the other.
    let after = vec![tile(0.), tile(320.)];
    let area = damage::diff(&before, &after).expect("a small change repaints a part");
    assert!(area.x <= 300. && area.x + area.w >= 520.);
    assert!(area.x > 200., "the unchanged tile is left alone");
    // A full-screen dissolve repaints everything.
    let mut fading = after.clone();
    fading.push(Draw::Dissolve(crate::renderer::Backdrop::between(
        "a", "b", 0.5,
    )));
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
    assert!(
        tracker.region(small, 2).is_none(),
        "a full frame in between repaints all"
    );
    // An unchanged frame still names one pixel inside the screen, never an
    // empty (off-surface) area: Mali presented black for that.
    let still = damage::Tracker::default()
        .region(damage::diff(&before, &before), 1)
        .expect("partial");
    assert!(still.w >= 1. && still.h >= 1. && still.x >= 0. && still.y >= 0.);
    assert!(still.x + still.w <= 1920. && still.y + still.h <= 1080.);
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

/// Where the grid's glide up to 690 px is after a first 16 ms frame, then
/// `steps` frames of `each` ms.
fn glide(steps: u32, each: u64) -> Vec<f32> {
    crate::clock::record();
    let mut state = ready();
    state.open(1);
    state.glide_scroll();
    state.scroll = 690.;
    let mut shown = Vec::new();
    for frame in 0..=steps {
        let ms = if frame == 0 { 16 } else { each };
        crate::clock::step(std::time::Duration::from_millis(ms));
        state.glide_scroll();
        shown.push(state.shown_scroll);
    }
    shown
}

#[test]
fn a_heavy_frame_advances_the_glide_by_its_real_time() {
    // A 50 ms frame (the TV while the grid rises) lands where five 10 ms
    // frames would: the motion keeps its speed however heavy the frames.
    let heavy = *glide(1, 50).last().unwrap();
    let light = *glide(5, 10).last().unwrap();
    assert!(
        (heavy - light).abs() < 0.5,
        "one 50 ms frame reached {heavy:.1}, five 10 ms frames {light:.1}"
    );
}

#[test]
fn a_scroll_gathers_speed_and_settles_without_overshoot() {
    let shown = glide(60, 16);
    let steps: Vec<f32> = std::iter::once(shown[0])
        .chain(shown.windows(2).map(|pair| pair[1] - pair[0]))
        .collect();
    assert!(
        steps[0] < steps[1] && steps[1] < steps[2],
        "eases in, never jumps on its first frame: {steps:?}"
    );
    assert!(steps.iter().all(|step| *step >= 0.), "never turns back");
    assert!(shown.iter().all(|y| *y <= 690.), "never overshoots");
    assert_eq!(*shown.last().unwrap(), 690., "settles exactly");
}

#[test]
fn background_writer_keeps_the_newest_preferences_and_finishes_every_save() {
    let directory = std::env::temp_dir().join(format!("lounge-writer-{}", std::process::id()));
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

#[test]
fn automatic_appearance_follows_the_clock_and_cross_fades() {
    let mut state = ready();
    assert_eq!(state.light(), 0., "dark by default");
    state.preferences.appearance = crate::model::Appearance::Auto;
    state.hour = 21;
    state.tick();
    assert_eq!(state.light(), 0., "evening is dark");
    // 06:00 arrives while Lounge is on screen: it fades to light, not cuts.
    state.hour = 6;
    state.tick();
    assert!(state.animating());
    assert!(state.light() < 0.5);
    std::thread::sleep(std::time::Duration::from_secs_f32(
        crate::state::APPEARANCE_FADE + 0.05,
    ));
    state.tick();
    assert_eq!(state.light(), 1.);
    // Reduced motion switches at once.
    state.preferences.reduced_motion = true;
    state.hour = 19;
    state.tick();
    assert_eq!(state.light(), 0.);
}

#[test]
fn a_plugged_in_input_appears_right_after_search_whatever_the_saved_order() {
    let mut state = ready();
    // A saved order (from Move) that lists every app on the grid.
    let order: Vec<String> = state.apps().iter().map(|app| app.id.clone()).collect();
    state.preferences.favorites = Some(order);
    // A console is plugged into HDMI 1.
    state.inventory.inputs[0].connected = Some(true);
    state.refresh_tiles();
    let ids: Vec<&str> = state.apps().iter().map(|app| app.id.as_str()).collect();
    let search = ids
        .iter()
        .position(|id| *id == crate::state::SEARCH_TILE)
        .unwrap();
    assert_eq!(ids[search + 1], state.inventory.inputs[0].id);
}

#[test]
fn a_slow_tv_service_at_boot_is_retried_before_any_error_shows() {
    let mut state = State::new(Preferences::default());
    state.loading = true;
    // Only Lounge's own tiles: no apps yet.
    state.refresh_tiles();
    for attempt in 1..=crate::state::INVENTORY_RETRIES {
        assert_eq!(
            state.inventory_failed("timeout".into()),
            Some(crate::state::INVENTORY_RETRY_DELAY)
        );
        assert_eq!(state.inventory_failures, attempt);
        assert!(
            state.loading && state.error.is_none(),
            "still loading, no error yet"
        );
    }
    assert_eq!(state.inventory_failed("timeout".into()), None);
    assert!(
        !state.loading && state.error.is_some(),
        "retries used up: the error shows"
    );
    // With apps already shown, a failed refresh reports at once.
    let mut state = ready();
    assert_eq!(state.inventory_failed("timeout".into()), None);
    assert!(state.error.is_some());
}

// Regressions found by walking every remote path (2026-10-27).

fn order(state: &State) -> Vec<String> {
    state.apps().iter().map(|app| app.id.clone()).collect()
}

#[test]
fn settings_can_be_moved_but_never_hidden() {
    use crate::state::SETTINGS_TILE;
    let mut state = ready();
    state.focus = Target::App(SETTINGS_TILE.into());
    state.options();
    state.focus = Target::Option(1);
    state.activate();
    assert!(
        !state
            .preferences
            .hidden
            .iter()
            .any(|id| id == SETTINGS_TILE)
    );
    assert!(order(&state).iter().any(|id| id == SETTINGS_TILE));
    // Anyone an earlier version locked out gets Settings back on load.
    let mut preferences: Preferences =
        serde_json::from_value(json!({"hidden": [SETTINGS_TILE, "plex"]})).unwrap();
    preferences.sanitize();
    assert_eq!(preferences.hidden, ["plex"]);
}

#[test]
fn settings_opens_on_general_and_right_enters_general() {
    use crate::state::SETTINGS_TILE;
    let mut state = ready();
    state.focus = Target::App(SETTINGS_TILE.into());
    state.activate();
    state.focus = Target::Sidebar(2);
    state.activate();
    state.back();
    state.back();
    state.focus = Target::App(SETTINGS_TILE.into());
    state.activate();
    assert_eq!(state.focus, Target::Sidebar(0));
    let controls = controls(&state);
    state.direction(Direction::Right, &controls);
    assert_ne!(
        state.focus,
        Target::WeatherToggle,
        "Right enters General, not last time's Weather"
    );
}

#[test]
fn number_keys_follow_the_grid_and_never_open_lounge_tiles() {
    use crate::state::{SEARCH_TILE, SETTINGS_TILE};
    let mut state = ready();
    let first = order(&state)[0].clone();
    state.focus = Target::App(first);
    state.hold();
    state.activate();
    state.activate();
    let launchable: Vec<String> = order(&state)
        .into_iter()
        .filter(|id| id != SETTINGS_TILE && id != SEARCH_TILE)
        .collect();
    for (slot, id) in launchable.iter().take(9).enumerate() {
        assert_eq!(state.quick_launch(slot), vec![Effect::Launch(id.clone())]);
    }
}

#[test]
fn a_hidden_app_keeps_its_place_through_a_move() {
    let mut state = ready();
    let before = order(&state);
    state.preferences.favorites = Some(before.clone());
    let hidden = before[2].clone();
    state.preferences.hidden.push(hidden.clone());
    state.focus = Target::App(before[0].clone());
    state.hold();
    state.activate();
    state.direction(Direction::Right, &[]);
    state.activate();
    state.preferences.hidden.clear();
    assert_eq!(order(&state).iter().position(|id| id == &hidden), Some(2));
}

#[test]
fn right_opens_every_settings_row_with_a_chevron() {
    let mut state = ready();
    state.page = Page::Settings;
    state.focus = Target::Setting(9);
    state.direction(Direction::Right, &[]);
    assert_eq!(state.subpage, Some(crate::state::Subpage::Appearance));
}

#[test]
fn move_on_an_app_that_has_gone_leaves_focus_on_a_tile() {
    let mut state = ready();
    state.focus = Target::App("plex".into());
    state.options();
    let mut inventory = demo_inventory();
    inventory.apps.retain(|app| app.id != "plex");
    state.set_inventory(inventory, None);
    state.focus = Target::Option(0);
    state.activate();
    assert!(state.overlay.is_none() && state.moving.is_none());
    assert!(matches!(&state.focus, Target::App(id) if id != "plex"));
}

#[test]
fn hiding_an_input_turns_it_off_in_settings_inputs() {
    let mut state = ready();
    let hdmi = "com.webos.app.hdmi1".to_string();
    state.focus = Target::App(hdmi.clone());
    state.options();
    state.focus = Target::Option(1);
    state.activate();
    assert_eq!(
        state.preferences.input_modes.get(&hdmi),
        Some(&InputMode::Off)
    );
    assert!(!state.preferences.hidden.contains(&hdmi));
    assert!(!order(&state).contains(&hdmi));
    // On in Settings › Inputs brings it back: one switch, no hidden copy.
    state
        .preferences
        .input_modes
        .insert(hdmi.clone(), InputMode::On);
    state.refresh_tiles();
    assert!(order(&state).contains(&hdmi));
    // An input an earlier version hid from its tile is Off instead.
    let mut preferences: Preferences =
        serde_json::from_value(json!({"hidden": ["com.webos.app.hdmi2", "plex"]})).unwrap();
    preferences.sanitize();
    assert_eq!(preferences.hidden, ["plex"]);
    assert_eq!(
        preferences.input_modes.get("com.webos.app.hdmi2"),
        Some(&InputMode::Off)
    );
}

#[test]
fn holding_ok_on_full_screen_top_shelf_lowers_it_first() {
    let mut state = ready();
    state.heroes = vec!["netflix".into()];
    state.preferences.top_shelf_art = true;
    state.direction(Direction::Up, &controls(&state));
    assert!(state.expanded);
    state.hold();
    state.activate();
    assert!(state.moving.is_some());
    assert!(!state.expanded, "Move is on the visible grid");
}

#[test]
fn a_device_plugged_in_during_move_waits_at_the_end() {
    let mut state = ready();
    let first = order(&state)[0].clone();
    state.focus = Target::App(first.clone());
    state.hold();
    state.activate();
    state.inventory.inputs[2].connected = Some(true);
    state.refresh_tiles();
    assert_eq!(
        order(&state).last().map(String::as_str),
        Some("com.webos.app.hdmi3")
    );
    state.direction(Direction::Down, &[]);
    assert_eq!(
        order(&state).iter().position(|id| id == &first),
        Some(crate::state::APPS_COLUMNS)
    );
}

#[test]
fn reduced_motion_switcher_jumps_to_the_card_and_settles() {
    let mut state = ready();
    state.preferences.reduced_motion = true;
    state.tick();
    state.open_switcher(vec![
        "netflix".into(),
        "plex".into(),
        "amazon".into(),
        "spotify-beehive".into(),
    ]);
    state.direction(Direction::Right, &[]);
    state.direction(Direction::Right, &[]);
    for _ in 0..40 {
        state.tick();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let shown = state.switcher.as_ref().unwrap().shown;
    assert_eq!(Target::Switch(shown as usize), state.focus);
    assert!(!state.animating(), "nothing left to draw: no frames burned");
}

#[test]
fn a_slow_press_off_app_tiles_is_still_ok() {
    let mut state = ready();
    state.page = Page::Settings;
    state.focus = Target::Setting(9);
    state.hold();
    assert_eq!(state.subpage, Some(crate::state::Subpage::Appearance));
}

#[test]
fn an_unknown_preference_value_keeps_every_other_preference() {
    let preferences: Preferences = serde_json::from_value(json!({
        "background": "forest",
        "appearance": "Sepia",
        "input_modes": {"com.webos.app.hdmi1": "Bogus", "com.webos.app.hdmi2": "Off"},
    }))
    .unwrap();
    assert_eq!(preferences.background, "forest");
    assert_eq!(preferences.appearance, crate::model::Appearance::default());
    assert_eq!(preferences.input_modes.len(), 1);
    assert_eq!(
        preferences.input_modes.get("com.webos.app.hdmi2"),
        Some(&InputMode::Off)
    );
}

#[test]
fn focus_goes_to_the_tile_that_takes_a_vanished_tiles_place() {
    let mut state = ready();
    state.page = Page::Apps;
    let tiles = order(&state);
    state.focus = Target::App(tiles[7].clone());
    state.track_focus();
    let mut inventory = demo_inventory();
    inventory.apps.retain(|app| app.id != tiles[7]);
    state.set_inventory(inventory, None);
    assert_eq!(state.focus, Target::App(order(&state)[7].clone()));
}

// Lounge as the TV's Home (tv/home.cjs mounts its executable over LG Home's).

#[test]
fn only_lg_home_executable_path_makes_lounge_the_home() {
    use crate::platform::{LG_HOME_EXE, is_home_path};
    assert!(is_home_path(std::path::Path::new(LG_HOME_EXE)));
    for path in [
        "/media/developer/apps/usr/palm/applications/io.github.yashwanthkamireddi.lounge/lounge",
        "/usr/bin/com.webos.app.home.real",
        "/usr/bin/lounge",
    ] {
        assert!(!is_home_path(std::path::Path::new(path)));
    }
    // A test run is never the TV's Home.
    assert_eq!(crate::platform::own_id(), crate::platform::APP_ID);
}

#[test]
fn sam_background_starts_never_count_as_someone_opening_home() {
    use crate::options::background_launch;
    use serde_json::json;
    // What sam passed LG's Home at boot on the TV (its CRIU image's argv).
    assert!(background_launch(&json!({
        "event": "launch", "reason": "boot", "appId": "com.webos.app.home",
        "parameters": {"boot": true}, "interfaceVersion": 2,
        "interfaceMethod": "registerApp", "@system_native_app": true
    })));
    assert!(background_launch(
        &json!({"preload": "partial", "parameters": {}})
    ));
    assert!(background_launch(&json!({"reason": "preload"})));
    for opened in [
        json!({"event": "launch", "reason": "undefined", "appId": "com.webos.app.home", "parameters": {}}),
        json!({"event": "launch", "appId": "com.webos.app.home", "parameters": {"boot": false}}),
        json!({"nid": "io.github.yashwanthkamireddi.lounge"}),
    ] {
        assert!(!background_launch(&opened), "{opened}");
    }
}

#[test]
fn the_switcher_never_lists_lounge_or_home_whichever_id_lounge_runs_under() {
    let mut state = ready();
    for id in [crate::platform::APP_ID, crate::platform::HOME_ID] {
        state
            .inventory
            .apps
            .push(crate::model::App::new(id, "Lounge"));
    }
    state.open_switcher(vec![
        crate::platform::HOME_ID.into(),
        "netflix".into(),
        crate::platform::APP_ID.into(),
    ]);
    let switcher = state.switcher.as_ref().expect("netflix is open");
    assert_eq!(switcher.apps, ["netflix"]);
}

#[test]
fn sam_relaunches_and_closes_lounge_as_home_and_a_lost_registration_closes_it() {
    use crate::platform::Lifecycle;
    use serde_json::json;
    assert_eq!(
        Lifecycle::from_reply(&json!({"returnValue": true, "event": "registered"})),
        None
    );
    assert_eq!(
        Lifecycle::from_reply(
            &json!({"returnValue": true, "event": "relaunch", "reason": "hotKey"})
        ),
        Some(Lifecycle::Relaunch)
    );
    assert_eq!(
        Lifecycle::from_reply(&json!({"event": "close"})),
        Some(Lifecycle::Close)
    );
    assert_eq!(
        Lifecycle::from_reply(&json!({"returnValue": false, "errorText": "sam restarted"})),
        Some(Lifecycle::Close)
    );
}

#[test]
fn recents_name_only_snapshots_inside_each_apps_own_folder() {
    let reply = json!({"returnValue": true, "recentsAppList": [
        {"appId": "netflix", "cardSnapShotFilePath": "/tmp/snapshots/netflix/card_1.jpg"},
        {"appId": "youtube.leanback.v4", "cardSnapShotFilePath": ""},
        {"appId": "amazon", "cardSnapShotFilePath": "/tmp/snapshots/netflix/card.jpg"},
        {"appId": "hotstar", "cardSnapShotFilePath": "/tmp/snapshots/hotstar/../../etc/shadow"},
        {"appId": "plex", "cardSnapShotFilePath": "/tmp/snapshots/plex/card'$(reboot).jpg"},
        {"appId": "spotify-beehive", "cardSnapShotFilePath": "/tmp/snapshots/spotify-beehive/card.bmp"},
        {"appId": "bad id", "cardSnapShotFilePath": "/tmp/snapshots/bad id/card.jpg"},
        {"appId": "org.jellyfin.webos", "cardSnapShotFilePath": "/tmp/snapshots/org.jellyfin.webos/RECENTS.PNG"},
    ]});
    assert_eq!(
        crate::snapshots::parse(&reply),
        vec![
            (
                "netflix".to_string(),
                "/tmp/snapshots/netflix/card_1.jpg".to_string()
            ),
            (
                "org.jellyfin.webos".to_string(),
                "/tmp/snapshots/org.jellyfin.webos/RECENTS.PNG".to_string()
            ),
        ]
    );
    assert!(crate::snapshots::parse(&json!({"returnValue": false})).is_empty());
    // As the 65UQ801C0SB (webOS 7.6) answers: its compositor takes none.
    assert!(crate::snapshots::capture_disabled(&json!({
        "configs": {"tv.hw.supportNMRM": false, "com.webos.surfacemanager.disableCapture": true},
        "returnValue": true
    })));
    assert!(!crate::snapshots::capture_disabled(
        &json!({"configs": {}, "returnValue": true})
    ));
}

#[test]
fn a_snapshot_is_cropped_from_its_centre_to_the_cards_shape() {
    // A full 16:9 frame, red at the edges that the card's shape crops away.
    let mut frame = image::RgbaImage::from_pixel(1920, 1080, image::Rgba([0, 0, 255, 255]));
    for y in 0..1080 {
        for x in (0..8).chain(1912..1920) {
            frame.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
        }
    }
    let mut bytes = Vec::new();
    frame
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let card = crate::snapshots::card(&bytes).unwrap();
    assert_eq!(card.dimensions(), crate::snapshots::CARD);
    assert!(card.pixels().all(|pixel| pixel[0] < 40 && pixel[2] > 200));
    assert!(crate::snapshots::card(b"not a picture").is_none());
}

#[test]
fn switcher_cards_show_snapshots_and_ask_for_them_as_they_open() {
    let mut state = ready();
    state.preferences.reduced_motion = true;
    state.open(1);
    state.open_switcher(vec!["netflix".into(), "youtube.leanback.v4".into()]);
    assert_eq!(
        state.snapshots_wanted,
        Some(vec![
            "netflix".to_string(),
            "youtube.leanback.v4".to_string()
        ])
    );
    let pictures = |state: &State| {
        view::scene(state, &test_assets(), time::OffsetDateTime::UNIX_EPOCH)
            .draw
            .iter()
            .filter(|draw| matches!(draw, crate::renderer::Draw::AppImage(image, ..) if image.key.starts_with("snapshot:")))
            .count()
    };
    assert_eq!(pictures(&state), 0, "no snapshot read yet: the cards stay");
    state.snapshots.insert_for_test(
        "netflix",
        crate::app_artwork::Artwork::picture(
            "snapshot:netflix".into(),
            1,
            image::RgbaImage::new(8, 8),
            crate::snapshots::CORNER,
        ),
    );
    assert_eq!(pictures(&state), 1);
    assert_eq!(state.snapshots.shown("netflix", true), 1.);
    assert_eq!(state.snapshots.shown("youtube.leanback.v4", true), 0.);
}

#[test]
fn quitting_ends_a_web_app_and_falls_back_to_closing_it() {
    let command = crate::platform::close_command("youtube.leanback.v4");
    let kill = command
        .find("luna://com.palm.webappmanager/killApp '{\"appId\":\"youtube.leanback.v4\"}'")
        .expect("WebAppMgr ends the page");
    let close = command
        .find("luna://com.webos.applicationManager/closeByAppId '{\"id\":\"youtube.leanback.v4\"}'")
        .expect("anything else is closed as before");
    assert!(kill < close);
    // Only a successful kill skips the close.
    assert!(command.contains("*'\"returnValue\":true'*"));
    assert!(command.contains("exit 0;;"));
}

#[test]
fn only_apps_the_owner_installed_can_be_uninstalled() {
    use crate::uninstall::{allowed, removable};
    for id in [
        crate::platform::APP_ID,
        crate::platform::HOME_ID,
        "com.ecklf.luma",
        "org.webosbrew.hbchannel",
        "org.webosbrew.hbchannel.service",
        "com.webos.app.browser",
        "com.webos.app.hdmi1",
        "com.lge.app.foo",
        "com.palm.app.settings",
        crate::state::SETTINGS_TILE,
        crate::state::SEARCH_TILE,
        "invalid';reboot",
        "",
    ] {
        assert!(!allowed(id), "{id} must be protected");
        assert!(crate::uninstall::command(id).is_err());
    }
    assert!(allowed("netflix") && allowed("com.disney.disneyplus-prod"));
    let store = "/media/cryptofs/apps/usr/palm/applications/netflix";
    assert!(removable(&json!({"id": "netflix", "folderPath": store})));
    assert!(removable(
        &json!({"id": "netflix", "removable": true, "folderPath": "/usr/palm/applications/netflix"})
    ));
    // webOS's own word is final; a system image app without one is not.
    assert!(!removable(
        &json!({"id": "netflix", "removable": false, "folderPath": store})
    ));
    assert!(!removable(
        &json!({"id": "netflix", "systemApp": true, "folderPath": store})
    ));
    assert!(!removable(
        &json!({"id": "netflix", "folderPath": "/usr/palm/applications/netflix"})
    ));
    assert!(!removable(
        &json!({"id": "netflix", "folderPath": "/media/cryptofs/apps/../../usr/palm"})
    ));
    assert!(!removable(
        &json!({"id": "com.webos.app.browser", "removable": true})
    ));
    let apps = normalize_apps(&json!([
        {"id": "netflix", "title": "Netflix", "folderPath": store},
        {"id": "com.webos.app.livetv", "title": "Live TV", "visible": true, "removable": true},
    ]))
    .unwrap();
    assert!(
        apps.iter()
            .find(|app| app.id == "netflix")
            .unwrap()
            .removable
    );
    assert!(
        !apps
            .iter()
            .find(|app| app.id == "com.webos.app.livetv")
            .unwrap()
            .removable
    );
    // A removal is only confirmed by a complete list without the app.
    assert_eq!(
        crate::uninstall::confirms_removal(&json!([{"id": "plex"}]), "netflix"),
        Ok(true)
    );
    assert_eq!(
        crate::uninstall::confirms_removal(&json!([{"id": "netflix"}]), "netflix"),
        Ok(false)
    );
    assert!(crate::uninstall::confirms_removal(&json!([{"title": "?"}]), "netflix").is_err());
    let command = crate::uninstall::command("netflix").unwrap();
    assert!(command.starts_with("/usr/bin/node -e '") && command.ends_with(" 'netflix'"));
    assert!(command.contains("com.webos.appInstallService/remove"));
}

/// Opens the context menu on `id` in the grid.
fn menu_for(state: &mut State, id: &str) -> Vec<Target> {
    state.open(1);
    state.focus = Target::App(id.into());
    state.options();
    controls(state).into_iter().map(|c| c.target).collect()
}

#[test]
fn uninstall_is_offered_last_and_only_where_webos_allows_it() {
    let mut state = ready();
    // Netflix, from the store: Move, Hide, Uninstall.
    let targets = menu_for(&mut state, "netflix");
    assert_eq!(
        targets,
        vec![Target::Option(0), Target::Option(1), Target::Option(2)]
    );
    assert!(ui_text(&state).iter().any(|line| line == "Uninstall"));
    // LG's own apps, Homebrew Channel, inputs and Lounge's tiles: no Uninstall.
    for id in [
        "com.webos.app.livetv",
        "org.webosbrew.hbchannel",
        "com.webos.app.hdmi1",
        crate::state::SEARCH_TILE,
        crate::state::SETTINGS_TILE,
    ] {
        state.back();
        let targets = menu_for(&mut state, id);
        assert!(!targets.contains(&Target::Option(2)), "{id}");
        state.focus = Target::Option(2);
        state.activate();
        assert!(
            !matches!(state.overlay, Some(Overlay::Uninstall { .. })),
            "{id}"
        );
    }
}

#[test]
fn the_uninstall_alert_opens_on_cancel_and_back_leaves_everything_as_it_was() {
    let mut state = ready();
    menu_for(&mut state, "netflix");
    state.focus = Target::Option(2);
    assert!(state.activate().is_empty());
    assert!(matches!(&state.overlay, Some(Overlay::Uninstall { id, .. }) if id == "netflix"));
    // Cancel is focused, and only the alert's two buttons take focus.
    assert_eq!(state.focus, Target::Alert(1));
    let targets: Vec<_> = controls(&state).into_iter().map(|c| c.target).collect();
    assert_eq!(targets, vec![Target::Alert(0), Target::Alert(1)]);
    let text = ui_text(&state);
    assert!(text.iter().any(|line| line == "Uninstall “Netflix”?"));
    assert!(text.iter().any(|line| line == "Cancel"));
    // Up reaches Uninstall, Down returns to Cancel.
    state.direction(Direction::Up, &controls(&state));
    assert_eq!(state.focus, Target::Alert(0));
    state.direction(Direction::Down, &controls(&state));
    assert_eq!(state.focus, Target::Alert(1));
    // Cancel (OK) and Back both close it onto the tile, removing nothing.
    assert!(state.activate().is_empty());
    assert!(state.overlay.is_none());
    assert_eq!(state.focus, Target::App("netflix".into()));
    menu_for(&mut state, "netflix");
    state.focus = Target::Option(2);
    state.activate();
    assert!(state.back().is_empty());
    assert!(state.overlay.is_none());
    assert_eq!(state.focus, Target::App("netflix".into()));
    assert!(state.uninstalling.is_empty());
    assert!(state.apps().iter().any(|app| app.id == "netflix"));
}

#[test]
fn an_uninstall_removes_the_tile_at_once_and_only_success_forgets_it() {
    let mut state = ready();
    state.preferences.favorites = Some(vec!["netflix".into(), "youtube.leanback.v4".into()]);
    state.preferences.recent = vec!["netflix".into()];
    let before: Vec<String> = {
        state.open(1);
        state.apps().iter().map(|app| app.id.clone()).collect()
    };
    let slot = before.iter().position(|id| id == "netflix").unwrap();
    menu_for(&mut state, "netflix");
    state.focus = Target::Option(2);
    state.activate();
    state.focus = Target::Alert(0);
    assert_eq!(state.activate(), vec![Effect::Uninstall("netflix".into())]);
    // Gone from the grid; focus on the tile that took its place.
    assert!(state.overlay.is_none());
    assert!(!state.apps().iter().any(|app| app.id == "netflix"));
    assert_eq!(state.focus, Target::App(before[slot + 1].clone()));
    // Asked once only: it is no longer offered while on its way.
    assert!(!state.can_uninstall("netflix"));
    // A reply for anything else changes nothing.
    assert!(state.uninstalled("plex", Ok(())).is_empty());
    // Refused: the tile comes back, every preference as it was.
    assert!(
        state
            .uninstalled("netflix", Err("denied".into()))
            .is_empty()
    );
    assert!(state.apps().iter().any(|app| app.id == "netflix"));
    assert_eq!(state.preferences.recent, vec!["netflix".to_string()]);
    assert!(state.notice.as_ref().unwrap().0.contains("Netflix"));
    // Removed: Lounge forgets it and reads the app list again.
    state.uninstalling.push("netflix".into());
    assert_eq!(
        state.uninstalled("netflix", Ok(())),
        vec![Effect::Save, Effect::Refresh]
    );
    assert!(!state.inventory.apps.iter().any(|app| app.id == "netflix"));
    assert_eq!(
        state.preferences.favorites,
        Some(vec!["youtube.leanback.v4".to_string()])
    );
    assert!(state.preferences.recent.is_empty());
    assert!(state.uninstalling.is_empty());
}

#[test]
fn the_time_pill_leaves_out_the_date_and_time_until_the_tv_has_set_its_clock() {
    let mut state = ready();
    let clock = crate::i18n::clock(time::OffsetDateTime::UNIX_EPOCH, state.hour_24);
    assert!(ui_text(&state).contains(&clock));
    state.clock_set = false;
    let text = ui_text(&state);
    assert!(!text.contains(&clock), "an unset clock shows no time");
    assert!(
        !text.contains(&crate::i18n::short_date(time::OffsetDateTime::UNIX_EPOCH)),
        "nor the date"
    );
}

#[test]
fn a_launch_webos_has_not_answered_yet_never_swallows_a_later_press() {
    crate::clock::record();
    let mut state = ready();
    state.launching = true;
    state.launch_started = Some(crate::clock::now());
    // The same press twice in a moment opens nothing twice.
    assert!(state.launch_busy());
    state.focus = Target::App("netflix".into());
    assert_eq!(state.activate(), vec![]);
    // webOS still has not answered 2 s later: the next press opens its app.
    crate::clock::step(std::time::Duration::from_secs(2));
    assert!(!state.launch_busy());
    assert_eq!(state.activate(), vec![Effect::Launch("netflix".into())]);
}

#[test]
fn an_app_whose_last_launch_never_showed_starts_afresh() {
    use crate::platform::Fronts;
    use std::time::{Duration, Instant};
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut fronts = Fronts::default();
    // Until the foreground events are on, nothing is known: never a restart.
    assert!(!fronts.launching("youtube.leanback.v4", at(0)));
    assert!(!fronts.launching("youtube.leanback.v4", at(10)));
    fronts.came_to_front("com.webos.app.home", at(20));
    // Opened, shown, opened again: a plain relaunch (it resumes).
    assert!(!fronts.launching("netflix", at(100)));
    fronts.came_to_front("netflix", at(900));
    assert!(!fronts.launching("netflix", at(5000)));
    // YouTube on a slow network: launched, its page never came up, and Home
    // was pressed over the splash. The next press restarts it.
    assert!(!fronts.launching("youtube.leanback.v4", at(6000)));
    fronts.came_to_front("com.webos.app.home", at(30_000));
    assert!(fronts.launching("youtube.leanback.v4", at(31_000)));
    // That fresh start showed: the press after it resumes again.
    fronts.came_to_front("youtube.leanback.v4", at(33_000));
    assert!(!fronts.launching("youtube.leanback.v4", at(40_000)));
    // LG's overlays never take the front; they are never "stalled".
    assert!(!fronts.launching("com.palm.app.settings", at(41_000)));
    assert!(!fronts.launching("com.palm.app.settings", at(42_000)));
    // The events stopping again makes everything unknown once more.
    fronts.live = false;
    assert!(!fronts.launching("hotstar", at(43_000)));
    assert!(!fronts.launching("hotstar", at(44_000)));
}

#[test]
fn a_late_answer_to_back_opens_no_switcher_over_what_came_after() {
    use crate::platform::{Platform, Response};
    use crate::replies::Replies;
    let platform = Platform::new(false, None);
    let assets = test_assets();
    let mut replies = Replies::new(false);
    let mut remote = crate::input::Remote::new();
    let mut state = ready();
    let open = vec!["netflix".to_owned(), "youtube.leanback.v4".to_owned()];
    // Back with webOS's open apps not yet known: asked, then answered.
    state.switcher_asked = true;
    replies.apply(
        Response::Running(open.clone()),
        &mut state,
        &platform,
        &assets,
        true,
        0,
    );
    assert!(state.switcher.is_some(), "the answer to Back opens it");
    state.switcher = None;
    state.focus = state.first_content();
    // Asked, but the owner pressed on before the busy TV answered.
    state.switcher_asked = true;
    let controls = controls(&state);
    remote.key_down(
        sdl2::keyboard::Keycode::Right,
        false,
        false,
        Some(0),
        &mut state,
        &controls,
    );
    replies.apply(
        Response::Running(open.clone()),
        &mut state,
        &platform,
        &assets,
        true,
        0,
    );
    assert!(
        state.switcher.is_none(),
        "a late answer opened the switcher"
    );
    // An answer nobody asked for (a second reply) opens nothing either.
    replies.apply(
        Response::Running(open),
        &mut state,
        &platform,
        &assets,
        true,
        0,
    );
    assert!(state.switcher.is_none());
}
