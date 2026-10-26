//! Open-Meteo data and request scheduling. API/cache temperatures remain Celsius;
//! presentation units never change the request or invalidate a cached reading.
use crate::model::Preferences;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const REFRESH_SECONDS: i64 = 15 * 60;
pub const MAX_CACHE_AGE: i64 = 6 * 60 * 60;
/// After a failed fetch: retry in a minute (a Wi-Fi blip recovers fast),
/// doubling up to half an hour while the TV stays offline.
const RETRY_FIRST: i64 = 60;
const RETRY_MAX: i64 = 30 * 60;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Location {
    pub name: String,
    pub region: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}
impl Location {
    pub fn parse(value: &Value) -> Option<Self> {
        let latitude = value.get("latitude")?.as_f64()?;
        let longitude = value.get("longitude")?.as_f64()?;
        let name = capitalised(label(value.get("name")));
        if name.is_empty()
            || !latitude.is_finite()
            || !longitude.is_finite()
            || latitude.abs() > 90.
            || longitude.abs() > 180.
        {
            return None;
        }
        let region = match value.get("region") {
            Some(region) => label(Some(region)),
            // Open-Meteo: the district before the state, so two places of
            // one name in one state still read apart.
            None => area(&label(value.get("admin2")), &label(value.get("admin1")), &name),
        };
        Some(Self {
            name,
            region,
            country: label(value.get("country")),
            latitude,
            longitude,
        })
    }
    pub fn key(&self) -> String {
        format!(
            "{:.4},{:.4}",
            if self.latitude == 0. {
                0.
            } else {
                self.latitude
            },
            if self.longitude == 0. {
                0.
            } else {
                self.longitude
            }
        )
    }
    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        for part in [&self.region, &self.country] {
            if !part.is_empty() && part != &self.name && !parts.contains(&part.as_str()) {
                parts.push(part.as_str());
            }
        }
        parts.join(", ")
    }
}
/// "Visakhapatnam, Andhra Pradesh": a place's district, then its state. The
/// district is left out when empty or only repeating the place's name or
/// state (a state named like its place is hidden later, by `detail`).
fn area(district: &str, state: &str, name: &str) -> String {
    let district = district.trim_end_matches(" District").trim();
    if district.is_empty() || district.eq_ignore_ascii_case(name) || district.eq_ignore_ascii_case(state) {
        return state.to_owned();
    }
    if state.is_empty() { district.to_owned() } else { format!("{district}, {state}") }
}

/// A name stored all in lower case ("sagar nagar") reads "Sagar Nagar";
/// any other casing is the place's own and stays.
fn capitalised(name: String) -> String {
    if name.chars().any(char::is_uppercase) || !name.chars().any(char::is_lowercase) {
        return name;
    }
    name.split(' ')
        .map(|word| {
            let mut letters = word.chars();
            letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Results that are one place to the weather: the same spot, or the same
/// name (any case) within 3 km, as OpenStreetMap lists a neighbourhood, its housing area and
/// its roads. The first, most relevant, stays.
fn distinct(places: impl Iterator<Item = Location>) -> Vec<Location> {
    let mut kept: Vec<Location> = Vec::new();
    for place in places {
        let same = kept.iter().any(|other| {
            let north = (place.latitude - other.latitude) * 111.;
            let east = (place.longitude - other.longitude) * 111. * place.latitude.to_radians().cos();
            other.key() == place.key() || (other.name.eq_ignore_ascii_case(&place.name) && north.hypot(east) < 3.)
        });
        if !same {
            kept.push(place);
        }
        if kept.len() == 6 {
            break;
        }
    }
    kept
}

fn label(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect()
}
pub fn deserialize_location<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Location>, D::Error> {
    Ok(Location::parse(&Value::deserialize(deserializer)?))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Celsius,
    Fahrenheit,
}
impl Unit {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
        }
    }
}
pub fn deserialize_unit<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Unit, D::Error> {
    Ok(
        if Value::deserialize(deserializer)?.as_str() == Some("fahrenheit") {
            Unit::Fahrenheit
        } else {
            Unit::Celsius
        },
    )
}
pub fn deserialize_enabled<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(Value::deserialize(deserializer)? != Value::Bool(false))
}
pub fn format_temperature(celsius: f64, unit: Unit) -> String {
    let value = if unit == Unit::Fahrenheit {
        celsius * 9. / 5. + 32.
    } else {
        celsius
    };
    // Match JavaScript Math.round, including negative half-degree ties.
    format!("{}{}", (value + 0.5).floor() as i32, unit.symbol())
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Reading {
    pub temperature: f64,
    pub code: u8,
    pub is_day: bool,
}
impl Reading {
    pub fn valid(self) -> bool {
        self.temperature.is_finite()
            && (-100.0..=70.0).contains(&self.temperature)
            && self.code <= 99
    }
}
pub fn parse_reading(value: &Value) -> Result<Reading, String> {
    let current = &value["current"];
    let temperature = current["temperature_2m"].as_f64();
    let code = current["weather_code"]
        .as_f64()
        .filter(|n| n.fract() == 0. && (0.0..=99.0).contains(n));
    let day = current["is_day"].as_f64().filter(|n| *n == 0. || *n == 1.);
    if let (Some(temperature), Some(code), Some(day)) = (temperature, code, day) {
        let reading = Reading {
            temperature,
            code: code as u8,
            is_day: day == 1.,
        };
        if reading.valid() {
            return Ok(reading);
        }
    }
    Err("The weather service returned an unreadable reading.".into())
}
pub fn parse_locations(value: &Value) -> Result<Vec<Location>, String> {
    if !value.is_object()
        || value["error"] == true
        || value.get("results").is_some_and(|v| !v.is_array())
    {
        return Err("The location service returned an unreadable response.".into());
    }
    Ok(distinct(
        value
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(20)
            .filter_map(Location::parse),
    ))
}

/// OpenStreetMap's Nominatim answer, for what Open-Meteo does not know
/// (India's PIN codes, older names such as Bangalore). A postcode reads as
/// the code and the district it covers: "530045 · Visakhapatnam".
pub fn parse_places(value: &Value) -> Result<Vec<Location>, String> {
    let list = value
        .as_array()
        .ok_or("The location service returned an unreadable response.")?;
    Ok(distinct(
        list.iter()
        .take(20)
        // Roads and paths share their neighbourhood's name; only places count.
        .filter(|place| !matches!(place.get("category").and_then(Value::as_str), Some("highway" | "railway")))
        .filter(|place| !matches!(place.get("addresstype").and_then(Value::as_str), Some("road" | "highway")))
        .filter_map(|place| {
            let number = |key: &str| place.get(key)?.as_str()?.parse::<f64>().ok();
            let field = |key: &str| label(place.get("address").and_then(|address| address.get(key)));
            let mut name = capitalised(label(place.get("name")));
            let postcode = place.get("addresstype").and_then(Value::as_str) == Some("postcode");
            if postcode {
                let area = ["city", "town", "state_district", "county"]
                    .into_iter()
                    .map(field)
                    .find(|area| !area.is_empty())
                    .unwrap_or_default();
                let area = area.trim_end_matches(" District").trim();
                if !area.is_empty() {
                    name = format!("{name} · {area}");
                }
            }
            Location::parse(&serde_json::json!({
                "name": name,
                "latitude": number("lat")?,
                "longitude": number("lon")?,
                // A postcode already names its district.
                "region": if postcode {
                    field("state")
                } else {
                    let district = ["city", "town", "state_district", "county"]
                        .into_iter()
                        .map(field)
                        .find(|area| !area.is_empty())
                        .unwrap_or_default();
                    area(&district, &field("state"), &name)
                },
                "country": field("country"),
            }))
        }),
    ))
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Cache {
    pub key: String,
    pub updated_at: i64,
    pub reading: Reading,
}
impl Cache {
    pub fn age(&self, now: i64) -> Option<i64> {
        now.checked_sub(self.updated_at)
            .filter(|age| (0..=MAX_CACHE_AGE).contains(age) && self.reading.valid())
    }
}

pub struct Condition {
    pub key: &'static str,
    pub icon: &'static str,
}
pub fn condition(reading: Reading) -> Condition {
    let (key, icon) = match reading.code {
        0 if reading.is_day => ("clearSkies", "sun"),
        0 => ("clearNight", "moon"),
        1 => ("mostlyClear", "partly-cloudy"),
        2 => ("partlyCloudy", "partly-cloudy"),
        3 => ("overcast", "cloud"),
        45 | 48 => ("fog", "fog"),
        51 | 53 | 55 | 56 | 57 => ("drizzle", "drizzle"),
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => ("rain", "rain"),
        71 | 73 | 75 | 77 | 85 | 86 => ("snow", "snow"),
        95 | 96 | 99 => ("thunderstorms", "storm"),
        _ => ("currentWeather", "cloud"),
    };
    Condition { key, icon }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Search {
        id: u64,
        query: String,
    },
    Forecast {
        id: u64,
        location: Location,
    },
    Persist(Cache),
    ClearCache,
}

#[derive(Default, Serialize)]
pub struct Weather {
    pub query: String,
    pub results: Vec<Location>,
    pub searching: bool,
    pub message: Option<String>,
    pub cache: Option<Cache>,
    pub failed: bool,
    pub loading: bool,
    #[serde(skip)]
    search_id: u64,
    #[serde(skip)]
    forecast_id: u64,
    #[serde(skip)]
    active_key: Option<String>,
    #[serde(skip)]
    pending: Option<(u64, String)>,
    #[serde(skip)]
    next_fetch: i64,
    #[serde(skip)]
    active_home: bool,
    /// Consecutive failed fetches, for the retry backoff.
    #[serde(skip)]
    failures: u32,
    /// When the query was last typed into: a search follows once typing
    /// pauses, as tvOS searches while you type.
    #[serde(skip)]
    typed_at: Option<std::time::Instant>,
}

/// How long typing must pause before the search runs.
pub const SEARCH_PAUSE: std::time::Duration = std::time::Duration::from_millis(350);
impl Weather {
    pub fn generations(&self) -> (u64, u64) {
        (self.search_id, self.forecast_id)
    }
    pub fn fetching(&self) -> bool {
        self.pending.is_some()
    }
    pub fn edit(&mut self, query: String) {
        let query: String = query
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        if query == self.query {
            return;
        }
        self.query = query;
        // Whatever is in flight is for older text: drop its answer, but keep
        // the results on screen until the new ones arrive, so the list never
        // flashes empty between keystrokes.
        self.search_id += 1;
        self.searching = false;
        self.message = None;
        if self.query.trim().chars().count() >= 2 {
            self.typed_at = Some(std::time::Instant::now());
        } else {
            self.typed_at = None;
            self.results.clear();
        }
    }
    /// The search typing asks for, once it has paused.
    pub fn due_search(&mut self) -> Option<Command> {
        if self.typed_at.is_some_and(|at| at.elapsed() >= SEARCH_PAUSE) {
            self.typed_at = None;
            return self.search();
        }
        None
    }
    /// A search is waiting on a pause in typing (the loop keeps ticking).
    pub fn search_pending(&self) -> bool {
        self.typed_at.is_some()
    }
    pub fn cancel_search(&mut self) {
        self.search_id += 1;
        self.results.clear();
        self.searching = false;
        self.message = None;
    }
    pub fn search(&mut self) -> Option<Command> {
        if self.searching {
            return None;
        }
        self.typed_at = None;
        self.search_id += 1;
        self.message = None;
        let query = self.query.trim().to_owned();
        if query.chars().count() < 2 {
            self.results.clear();
            self.message = Some("enterCity".into());
            return None;
        }
        self.searching = true;
        Some(Command::Search {
            id: self.search_id,
            query,
        })
    }
    pub fn accept_search(&mut self, id: u64, result: Result<Vec<Location>, String>) -> bool {
        if id != self.search_id || !self.searching {
            return false;
        }
        self.searching = false;
        match result {
            Ok(places) => {
                self.results = places;
                if self.results.is_empty() {
                    self.message = Some("noLocations".into());
                }
            }
            Err(_) => self.message = Some("searchUnavailable".into()),
        }
        true
    }
    pub fn invalidate_forecast(&mut self) {
        self.forecast_id += 1;
        self.pending = None;
        self.active_key = None;
        self.next_fetch = 0;
        self.loading = false;
        self.failed = false;
        self.failures = 0;
    }
    pub fn remove_location(&mut self) {
        self.cancel_search();
        self.invalidate_forecast();
        self.cache = None;
    }
    pub fn reading(&self, location: &Location) -> Option<Reading> {
        self.cache
            .as_ref()
            .filter(|cache| cache.key == location.key())
            .map(|cache| cache.reading)
    }
    pub fn sync(
        &mut self,
        preferences: &Preferences,
        home_visible: bool,
        now: i64,
    ) -> (Option<Command>, bool) {
        let before = (
            self.loading,
            self.failed,
            self.cache.as_ref().map(|c| (c.updated_at, c.reading)),
        );
        if self
            .cache
            .as_ref()
            .is_some_and(|cache| cache.age(now).is_none())
        {
            self.cache = None;
        }
        let key = preferences.weather_location.as_ref().map(Location::key);
        if self.active_key != key {
            self.invalidate_forecast();
            self.active_key = key.clone();
        }
        let active = preferences.show_weather && home_visible && key.is_some();
        if active && !self.active_home {
            self.next_fetch = 0;
        }
        self.active_home = active;
        let mut command = None;
        if !preferences.show_weather || !home_visible || key.is_none() {
            if self.pending.take().is_some() {
                self.forecast_id += 1;
            }
            self.loading = false;
        } else if let Some(cache) = self.cache.as_ref().filter(|c| {
            Some(&c.key) == key.as_ref() && c.age(now).is_some_and(|age| age < REFRESH_SECONDS)
        }) {
            self.loading = false;
            self.failed = false;
            self.next_fetch = cache.updated_at + REFRESH_SECONDS;
        } else if self.pending.is_none() && now >= self.next_fetch {
            self.forecast_id += 1;
            self.pending = key.clone().map(|key| (self.forecast_id, key));
            self.loading = self
                .cache
                .as_ref()
                .is_none_or(|cache| Some(&cache.key) != key.as_ref());
            command = preferences
                .weather_location
                .clone()
                .map(|location| Command::Forecast {
                    id: self.forecast_id,
                    location,
                });
        }
        if active && self.pending.is_some() {
            self.loading = self
                .cache
                .as_ref()
                .is_none_or(|cache| Some(&cache.key) != key.as_ref());
        }
        let changed = before
            != (
                self.loading,
                self.failed,
                self.cache.as_ref().map(|c| (c.updated_at, c.reading)),
            );
        (command, changed)
    }
    pub fn accept_forecast(
        &mut self,
        id: u64,
        key: &str,
        result: Result<Reading, String>,
        now: i64,
    ) -> (bool, Option<Command>) {
        if self
            .pending
            .as_ref()
            .is_none_or(|(pending, location)| *pending != id || location != key)
        {
            return (false, None);
        }
        self.pending = None;
        self.loading = false;
        match result {
            Ok(reading) if reading.valid() => {
                let cache = Cache {
                    key: key.into(),
                    updated_at: now,
                    reading,
                };
                self.cache = Some(cache.clone());
                self.failed = false;
                self.failures = 0;
                self.next_fetch = now + REFRESH_SECONDS;
                (true, Some(Command::Persist(cache)))
            }
            _ => {
                self.failed = true;
                let backoff = RETRY_FIRST << self.failures.min(5);
                self.failures = self.failures.saturating_add(1);
                self.next_fetch = now + backoff.min(RETRY_MAX);
                if self
                    .cache
                    .as_ref()
                    .is_some_and(|cache| cache.age(now).is_none())
                {
                    self.cache = None;
                }
                (true, None)
            }
        }
    }
}

#[cfg(test)]
#[path = "weather-tests.rs"]
mod tests;
