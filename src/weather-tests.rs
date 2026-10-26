use super::*;
use serde_json::json;

fn place() -> Location {
    Location {
        name: "Vienna".into(),
        region: "Vienna".into(),
        country: "Austria".into(),
        latitude: 48.2085,
        longitude: 16.3721,
    }
}
fn reading() -> Reading {
    Reading {
        temperature: 14.2,
        code: 2,
        is_day: true,
    }
}
fn preferences() -> Preferences {
    Preferences {
        weather_location: Some(place()),
        ..Preferences::default()
    }
}
fn cache(updated_at: i64) -> Cache {
    Cache {
        key: place().key(),
        updated_at,
        reading: reading(),
    }
}

#[test]
fn search_results_are_validated_normalized_and_deduplicated() {
    let result = parse_locations(&json!({"results":[
        {"name":"  Vienna\n", "admin1":"Vienna", "country":"Austria", "latitude":48.2085,"longitude":16.3721},
        {"name":"Duplicate", "latitude":48.2085,"longitude":16.3721},
        {"name":"Bad", "latitude":91,"longitude":16}, {"name":"", "latitude":0,"longitude":0}
    ]})).unwrap();
    assert_eq!(result, vec![place()]);
    assert_eq!(result[0].detail(), "Austria");
    assert!(parse_locations(&json!({})).unwrap().is_empty());
    assert!(parse_locations(&json!({"results":"wrong"})).is_err());
    assert!(parse_locations(&json!({"error":true})).is_err());
}

#[test]
fn temperatures_convert_the_unrounded_reading_and_use_web_rounding() {
    assert_eq!(format_temperature(14.2, Unit::Fahrenheit), "58°F");
    assert_eq!(format_temperature(14.2, Unit::Celsius), "14°C");
    assert_eq!(format_temperature(-0.5, Unit::Celsius), "0°C");
    assert_eq!(format_temperature(-5.5, Unit::Celsius), "-5°C");
}

#[test]
fn api_readings_and_conditions_cover_invalid_data_and_night() {
    let good = json!({"current":{"temperature_2m":14.2,"weather_code":2.0,"is_day":1.0}});
    assert_eq!(parse_reading(&good).unwrap(), reading());
    for value in [
        json!({}),
        json!({"current":{"temperature_2m":90,"weather_code":2,"is_day":1}}),
        json!({"current":{"temperature_2m":14,"weather_code":2.5,"is_day":1}}),
        json!({"current":{"temperature_2m":14,"weather_code":2,"is_day":true}}),
    ] {
        assert!(parse_reading(&value).is_err());
    }
    for (code, day, icon) in [
        (0, false, "moon"),
        (0, true, "sun"),
        (45, true, "fog"),
        (56, true, "drizzle"),
        (81, true, "rain"),
        (85, true, "snow"),
        (99, true, "storm"),
    ] {
        assert_eq!(
            condition(Reading {
                code,
                is_day: day,
                ..reading()
            })
            .icon,
            icon
        );
    }
}

#[test]
fn cache_reuse_and_units_do_not_trigger_another_request() {
    let mut weather = Weather {
        cache: Some(cache(1000)),
        ..Weather::default()
    };
    let mut preferences = preferences();
    assert!(weather.sync(&preferences, true, 1100).0.is_none());
    preferences.temperature_unit = Unit::Fahrenheit;
    assert!(weather.sync(&preferences, true, 1200).0.is_none());
    assert_eq!(weather.reading(&place()).unwrap().temperature, 14.2);
    assert!(matches!(
        weather.sync(&preferences, true, 1900).0,
        Some(Command::Forecast { .. })
    ));
}

#[test]
fn no_requests_without_a_location_enabled_widget_and_visible_home() {
    let mut weather = Weather::default();
    let mut preferences = preferences();
    preferences.show_weather = false;
    assert!(weather.sync(&preferences, true, 1000).0.is_none());
    assert!(preferences.weather_location.is_some());
    preferences.show_weather = true;
    assert!(weather.sync(&preferences, false, 1000).0.is_none());
    preferences.weather_location = None;
    assert!(weather.sync(&preferences, true, 1000).0.is_none());
}

#[test]
fn weather_failure_keeps_valid_cache_labels_it_saved_and_retries_later() {
    let mut weather = Weather {
        cache: Some(cache(1000)),
        ..Weather::default()
    };
    let preferences = preferences();
    let Some(Command::Forecast { id, location }) = weather.sync(&preferences, true, 2000).0 else {
        panic!("Expected stale-cache refresh");
    };
    weather.accept_forecast(id, &location.key(), Err("offline".into()), 2000);
    assert!(weather.failed);
    assert!(weather.reading(&place()).is_some());
    // Retries back off: a minute after the first failure, then doubling.
    assert!(weather.sync(&preferences, true, 2059).0.is_none());
    let Some(Command::Forecast { id, location }) = weather.sync(&preferences, true, 2060).0 else {
        panic!("Expected the first retry after a minute");
    };
    weather.accept_forecast(id, &location.key(), Err("offline".into()), 2060);
    assert!(weather.sync(&preferences, true, 2179).0.is_none());
    assert!(weather.sync(&preferences, true, 2180).0.is_some());
    assert!(cache(1000).age(999).is_none());
    assert!(cache(1000).age(1001 + MAX_CACHE_AGE).is_none());
    weather.sync(&preferences, true, 1001 + MAX_CACHE_AGE);
    assert!(weather.cache.is_none());
}

#[test]
fn removed_disabled_or_changed_locations_reject_in_flight_results() {
    let mut weather = Weather::default();
    let mut preferences = preferences();
    let Some(Command::Forecast { id, location }) = weather.sync(&preferences, true, 1000).0 else {
        panic!();
    };
    weather.remove_location();
    preferences.weather_location = None;
    assert_eq!(
        weather.accept_forecast(id, &location.key(), Ok(reading()), 1001),
        (false, None)
    );
    assert!(weather.cache.is_none());
    preferences.weather_location = Some(place());
    let Some(Command::Forecast { id, location }) = weather.sync(&preferences, true, 1002).0 else {
        panic!();
    };
    preferences.show_weather = false;
    weather.sync(&preferences, true, 1003);
    assert_eq!(
        weather.accept_forecast(id, &location.key(), Ok(reading()), 1004),
        (false, None)
    );
    preferences.show_weather = true;
    let Some(Command::Forecast {
        id: old,
        location: old_location,
    }) = weather.sync(&preferences, true, 1005).0
    else {
        panic!();
    };
    preferences.weather_location.as_mut().unwrap().latitude += 1.;
    let Some(Command::Forecast {
        id: new,
        location: new_location,
    }) = weather.sync(&preferences, true, 1006).0
    else {
        panic!();
    };
    assert_eq!(
        weather.accept_forecast(old, &old_location.key(), Ok(reading()), 1007),
        (false, None)
    );
    assert!(
        weather
            .accept_forecast(new, &new_location.key(), Ok(reading()), 1007)
            .0
    );
    assert_eq!(weather.cache.as_ref().unwrap().key, new_location.key());
}

#[test]
fn returning_home_retries_stale_weather_without_waiting_for_the_old_retry_timer() {
    let mut weather = Weather {
        cache: Some(cache(1000)),
        ..Weather::default()
    };
    let preferences = preferences();
    let Some(Command::Forecast { id, location }) = weather.sync(&preferences, true, 2000).0 else {
        panic!();
    };
    weather.accept_forecast(id, &location.key(), Err("offline".into()), 2000);
    assert!(weather.sync(&preferences, false, 2010).0.is_none());
    assert!(weather.sync(&preferences, true, 2020).0.is_some());
}

#[test]
fn editing_query_invalidates_old_results_and_short_queries_do_not_search() {
    let mut weather = Weather::default();
    weather.edit("V".into());
    assert!(weather.search().is_none());
    assert!(weather.message.is_some());
    weather.edit("Vienna".into());
    let Some(Command::Search { id, .. }) = weather.search() else {
        panic!();
    };
    assert!(weather.search().is_none());
    weather.edit("Paris".into());
    assert!(!weather.accept_search(id, Ok(vec![place()])));
    assert!(weather.results.is_empty());
}

#[test]
fn malformed_weather_preferences_do_not_lose_existing_favorites() {
    let preferences: Preferences = serde_json::from_value(json!({
        "favorites":["netflix"], "weather_location":{"name":"Invalid","latitude":"oops","longitude":0},
        "temperature_unit":"kelvin", "show_weather":"off"
    })).unwrap();
    assert_eq!(preferences.favorites, Some(vec!["netflix".into()]));
    assert!(preferences.weather_location.is_none());
    assert_eq!(preferences.temperature_unit, Unit::Celsius);
    assert!(preferences.show_weather);
    let old: Preferences = serde_json::from_value(json!({"background":"forest"})).unwrap();
    assert!(old.show_weather);
    assert!(old.weather_location.is_none());
}

#[test]
fn typing_searches_on_its_own_once_typing_pauses() {
    let mut weather = Weather::default();
    weather.edit("G".into());
    assert!(!weather.search_pending(), "one letter never searches");
    weather.edit("Gajuwaka".into());
    assert!(weather.search_pending());
    assert!(weather.due_search().is_none(), "not while still typing");
    std::thread::sleep(crate::weather::SEARCH_PAUSE + std::time::Duration::from_millis(20));
    let Some(Command::Search { id, query, .. }) = weather.due_search() else {
        panic!("a paused query searches");
    };
    assert_eq!(query, "Gajuwaka");
    assert!(weather.searching && !weather.search_pending());
    assert!(weather.accept_search(id, Ok(vec![])));
    // Typing more keeps what is shown until the next answer arrives.
    weather.results = vec![];
    weather.edit("Gajuwaka ".into());
    weather.edit("Gajuwak".into());
    assert!(weather.search_pending() && !weather.searching);
}

#[test]
fn openstreetmap_places_name_postcodes_by_their_district() {
    let places = parse_places(&json!([
        {
            "name": "530045", "lat": "17.7865", "lon": "83.3695", "addresstype": "postcode",
            "address": {"postcode": "530045", "state_district": "Visakhapatnam District",
                        "state": "Andhra Pradesh", "country": "India"}
        },
        {
            "name": "Bengaluru", "lat": "12.9767", "lon": "77.5900", "addresstype": "city",
            "address": {"city": "Bengaluru", "state": "Karnataka", "country": "India"}
        },
        {"name": "Nowhere", "lat": "not a number", "lon": "1"}
    ]))
    .unwrap();
    assert_eq!(places.len(), 2);
    assert_eq!(places[0].name, "530045 · Visakhapatnam");
    assert_eq!(places[0].detail(), "Andhra Pradesh, India");
    assert_eq!(places[1].name, "Bengaluru");
    assert!(parse_places(&json!({"error": "x"})).is_err());
}

#[test]
fn one_place_listed_many_times_shows_once_with_its_district() {
    // OpenStreetMap's answer for "sagar nagar": one neighbourhood in
    // Visakhapatnam as a place, a housing area and two roads, and another
    // Sagar Nagar in Kerala.
    let address = json!({"county": "Visakhapatnam Rural Mandal", "state_district": "Visakhapatnam District",
                         "state": "Andhra Pradesh", "country": "India"});
    let places = parse_places(&json!([
        {"name": "Sagar Nagar", "lat": "17.7671", "lon": "83.3580", "category": "place",
         "addresstype": "neighbourhood", "address": address},
        {"name": "Sagar nagar", "lat": "17.7615", "lon": "83.3573", "category": "landuse",
         "addresstype": "residential", "address": address},
        {"name": "sagar nagar", "lat": "11.6617", "lon": "76.2487", "category": "landuse",
         "addresstype": "residential",
         "address": {"town": "Sultan Bathery", "state_district": "Wayanad", "state": "Kerala", "country": "India"}},
        {"name": "Sagar Nagar", "lat": "17.7645", "lon": "83.3613", "category": "highway",
         "addresstype": "road", "address": address},
    ]))
    .unwrap();
    let shown: Vec<_> = places.iter().map(|place| (place.name.as_str(), place.detail())).collect();
    assert_eq!(
        shown,
        [
            ("Sagar Nagar", "Visakhapatnam, Andhra Pradesh, India".to_owned()),
            ("Sagar Nagar", "Sultan Bathery, Kerala, India".to_owned()),
        ]
    );
    // Open-Meteo's district reads the same way.
    let open_meteo = parse_locations(&json!({"results": [
        {"name": "Gajuwaka", "admin1": "Andhra Pradesh", "admin2": "Visakhapatnam",
         "country": "India", "latitude": 17.7, "longitude": 83.2}
    ]}))
    .unwrap();
    assert_eq!(open_meteo[0].detail(), "Visakhapatnam, Andhra Pradesh, India");
}

#[test]
fn an_empty_search_message_does_not_outlive_its_page() {
    let mut weather = Weather::default();
    assert!(weather.search().is_none());
    assert_eq!(weather.message.as_deref(), Some("enterCity"));
    // Typing nothing new is no edit, so it keeps the message; reopening
    // the page cancels the search, which clears it.
    weather.edit(String::new());
    assert!(weather.message.is_some());
    weather.cancel_search();
    assert!(weather.message.is_none() && weather.results.is_empty());
}
