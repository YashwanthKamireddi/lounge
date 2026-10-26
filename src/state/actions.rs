//! OK, Back, Home, hold and the context menu: what each press does.
use super::*;

impl State {
    pub fn options(&mut self) {
        if self.overlay.is_some() || self.moving.is_some() {
            return;
        }
        if let Target::App(id) = &self.focus {
            self.options_closed = None;
            self.overlay = Some(Overlay::Options {
                id: id.clone(),
                restore: self.focus.clone(),
            });
            self.focus = Target::Option(0);
            if !self.preferences.reduced_motion {
                self.panel_started = Some(Instant::now());
            }
            self.dirty = true;
        }
    }

    /// Holding OK on an app starts Move at once, as on tvOS; elsewhere it
    /// opens the options menu.
    pub fn hold(&mut self) {
        if self.page == Page::Apps
            && self.overlay.is_none()
            && self.moving.is_none()
            && let Target::App(id) = self.focus.clone()
        {
            self.start_move(&id);
        } else {
            self.options();
        }
    }

    /// Enter Move for `id`. The home order becomes explicit (exactly as now
    /// shown, so nothing jumps); the dock is simply its first row.
    pub fn start_move(&mut self, id: &str) {
        let order: Vec<String> = self.apps().iter().map(|app| app.id.clone()).collect();
        if !order.iter().any(|item| item == id) {
            return;
        }
        self.preferences.favorites = Some(order);
        self.moving = Some(id.into());
        self.moving_since = (!self.preferences.reduced_motion).then(Instant::now);
        // Say how Move works, once, in the banner.
        self.notify(i18n::t("moveHint"));
        self.focus = Target::App(id.into());
        self.dirty = true;
    }

    pub fn activate(&mut self) -> Vec<Effect> {
        if matches!(self.overlay, Some(Overlay::Keyboard)) && matches!(self.focus, Target::App(_)) {
            // A live search result was chosen: leave the keyboard and open it.
            self.overlay = None;
            self.panel_started = None;
        }
        if let (Some(switcher), Target::Switch(index)) = (&self.switcher, &self.focus) {
            let id = switcher.apps.get(*index).cloned();
            let restore = switcher.restore.clone();
            self.switcher = None;
            self.focus = restore;
            self.dirty = true;
            return id.map_or_else(Vec::new, |id| vec![Effect::Launch(id)]);
        }
        if self.moving.take().is_some() {
            // The order is saved as Move ends (see `unsaved_order`). A fresh OK
            // release finishes the mode and keeps the selected favorite,
            // rather than launching it.
            self.notice = None;
            self.dirty = true;
            return vec![];
        }
        self.dirty = true;
        match self.focus.clone() {
            Target::Nav(index) => self.open(index),
            Target::Browse => self.open(1),
            Target::Retry => return vec![Effect::Refresh],
            Target::DismissNotice => self.notice = None,
            Target::App(id) if id == SETTINGS_TILE => {
                self.open(2);
                self.focus = Target::Sidebar(0);
                return vec![];
            }
            Target::App(id) if id == SEARCH_TILE => {
                self.open_keyboard(TextField::Apps);
                return vec![];
            }
            Target::App(id) | Target::Input(id) if !self.launching => {
                return vec![Effect::Launch(id)];
            }
            Target::Search => {
                self.open_keyboard(TextField::Apps);
            }
            Target::WeatherConfigure => {
                self.open(2);
                self.section = WEATHER_SECTION;
                self.open_location();
                self.open_keyboard(TextField::Location);
            }
            Target::WeatherToggle => {
                self.preferences.show_weather = !self.preferences.show_weather;
                self.weather.invalidate_forecast();
                return vec![Effect::Save];
            }
            Target::WeatherLocation => self.open_location(),
            Target::WeatherTemperature => {
                self.subpage = Some(Subpage::Temperature);
                self.focus = Target::Choice(usize::from(
                    self.preferences.temperature_unit == Unit::Fahrenheit,
                ));
                if !self.preferences.reduced_motion {
                    self.panel_started = Some(Instant::now());
                }
            }
            Target::WeatherQuery => self.open_keyboard(TextField::Location),
            Target::WeatherSearch => {
                return self
                    .weather
                    .search()
                    .map(|command| vec![Effect::Weather(command)])
                    .unwrap_or_default();
            }
            Target::WeatherClear => {
                self.preferences.weather_location = None;
                self.weather.remove_location();
                self.focus = Target::WeatherSearch;
                return vec![Effect::Save, Effect::Weather(weather::Command::ClearCache)];
            }
            Target::WeatherResult(index) => {
                if let Some(place) = self.weather.results.get(index).cloned() {
                    self.preferences.weather_location = Some(place);
                    self.weather.invalidate_forecast();
                    self.weather.cancel_search();
                    self.focus = Target::WeatherSearch;
                    return vec![Effect::Save];
                }
            }
            Target::Sidebar(index) => {
                self.section = index;
                self.scroll = 0.;
                self.subpage = None;
                self.focus = self.section_entry();
            }
            Target::Setting(index) => {
                match index {
                    0 => self.preferences.show_names = !self.preferences.show_names,
                    1 => {
                        self.subpage = Some(Subpage::Clock);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(Instant::now());
                        }
                        self.focus = Target::Choice(usize::from(self.preferences.hour_24));
                        return vec![];
                    }
                    2 => self.preferences.reduced_motion = !self.preferences.reduced_motion,
                    9 => {
                        self.subpage = Some(Subpage::Appearance);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(Instant::now());
                        }
                        let index = APPEARANCES
                            .iter()
                            .position(|appearance| *appearance == self.preferences.appearance)
                            .unwrap_or(0);
                        self.focus = Target::Choice(index);
                        return vec![];
                    }
                    6 => {
                        self.preferences.top_shelf_art = !self.preferences.top_shelf_art;
                        self.expanded = false;
                    }
                    5 => return vec![Effect::Quit],
                    8 => {
                        self.subpage = Some(Subpage::Avatar);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(Instant::now());
                        }
                        let current = self.avatar_choice();
                        let index = self
                            .avatar_choices()
                            .iter()
                            .position(|choice| Some(choice) == current.as_ref())
                            .unwrap_or(0);
                        self.focus = Target::Choice(index);
                        return vec![];
                    }
                    7 => {
                        self.subpage = Some(Subpage::Hidden);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(Instant::now());
                        }
                        self.focus = Target::Choice(0);
                        return vec![];
                    }
                    _ => return vec![Effect::Refresh],
                }
                return vec![Effect::Save];
            }
            Target::Background(index) => {
                if !self.preferences.reduced_motion
                    && self.preferences.background != BACKGROUNDS[index].0
                {
                    self.previous_background =
                        Some((self.preferences.background.clone(), Instant::now()));
                }
                self.preferences.background = BACKGROUNDS[index].0.into();
                return vec![Effect::Save];
            }
            Target::InputMode(id) => {
                let mode = self
                    .preferences
                    .input_modes
                    .get(&id)
                    .copied()
                    .unwrap_or_default();
                self.subpage = Some(Subpage::Input(id));
                if !self.preferences.reduced_motion {
                    self.panel_started = Some(Instant::now());
                }
                self.focus = Target::Choice(match mode {
                    InputMode::On => 0,
                    InputMode::Off => 1,
                    InputMode::Auto => 2,
                });
            }
            Target::Choice(index) => {
                match self.subpage.clone() {
                    Some(Subpage::Clock) => self.preferences.hour_24 = index == 1,
                    Some(Subpage::Appearance) => {
                        self.preferences.appearance = APPEARANCES[index.min(APPEARANCES.len() - 1)];
                    }
                    Some(Subpage::Input(id)) => {
                        self.preferences.input_modes.insert(
                            id,
                            [InputMode::On, InputMode::Off, InputMode::Auto][index.min(2)],
                        );
                        self.refresh_tiles();
                    }
                    Some(Subpage::Temperature) => {
                        self.preferences.temperature_unit = if index == 1 {
                            Unit::Fahrenheit
                        } else {
                            Unit::Celsius
                        }
                    }
                    Some(Subpage::Avatar) => {
                        if let Some(choice) = self.avatar_choices().get(index).cloned() {
                            self.preferences.avatar = Some(choice);
                            self.show_avatar(true);
                        }
                    }
                    Some(Subpage::Hidden) => {
                        // Showing an app puts it back where it was; the list
                        // closes once nothing is left in it.
                        let Some(app) = self.hidden_apps().get(index).map(|app| (app.id.clone(), app.title.clone()))
                        else {
                            return vec![];
                        };
                        self.preferences.hidden.retain(|item| item != &app.0);
                        self.notify(i18n::format("appRestored", &[("app", &app.1)]));
                        let left = self.hidden_count();
                        if left > 0 {
                            self.focus = Target::Choice(index.min(left - 1));
                            self.dirty = true;
                            return vec![Effect::Save];
                        }
                    }
                    Some(Subpage::WeatherLocation) => return vec![],
                    None => return vec![],
                }
                self.close_subpage();
                return vec![Effect::Save];
            }
            Target::Option(index) => {
                let Some(Overlay::Options { id, restore }) = self.overlay.take() else {
                    return vec![];
                };
                self.panel_started = None;
                if !self.preferences.reduced_motion {
                    self.options_closed = Some((id.clone(), Instant::now()));
                }
                let old_ids: Vec<_> = self.apps().iter().map(|a| a.id.clone()).collect();
                let old_index = old_ids.iter().position(|item| item == &id).unwrap_or(0);
                let title = self
                    .inventory
                    .apps
                    .iter()
                    .find(|app| app.id == id)
                    .map_or(id.clone(), |app| app.title.clone());
                if index == 0 {
                    // tvOS "Move": any app, anywhere on the home screen.
                    self.start_move(&id);
                    return vec![Effect::Save];
                }
                if index == 1 {
                    if self.preferences.hidden.contains(&id) {
                        self.preferences.hidden.retain(|item| item != &id);
                        self.notify(i18n::format("appRestored", &[("app", &title)]));
                    } else {
                        self.preferences.hidden.push(id.clone());
                        self.notify(i18n::format("appHidden", &[("app", &title)]));
                    }
                }
                self.focus = restore;
                self.pointer = false;
                if let Target::App(id) = &self.focus {
                    let apps = self.apps();
                    if !apps.iter().any(|app| &app.id == id) {
                        self.focus = apps
                            .get(old_index.min(apps.len().saturating_sub(1)))
                            .map(|app| Target::App(app.id.clone()))
                            .unwrap_or_else(|| self.first_content());
                    }
                }
                return vec![Effect::Save];
            }
            Target::Key(index) => match KEYS[index] {
                "Space" => self.text(" "),
                "Delete" => self.delete(),
                "Clear" => {
                    if self.keyboard_field == TextField::Location {
                        self.weather.edit(String::new());
                    } else {
                        self.query.clear();
                    }
                }
                "Done" => {
                    self.overlay = None;
                    self.panel_started = None;
                    self.focus = self.keyboard_return();
                    self.search_closed = (!self.preferences.reduced_motion).then(Instant::now);
                }
                key => self.text(key),
            },
            _ => {}
        }
        vec![]
    }

    /// The remote's Home key. Pressed with Lounge in front it is tvOS's Home:
    /// whatever is open closes and the home screen returns to its top. On the
    /// way back from an app the home stays as it was left, with anything that
    /// was still open closed.
    pub fn home_key(&mut self, in_front: bool) {
        let open = self.page == Page::Settings
            || self.overlay.is_some()
            || self.switcher.is_some()
            || self.moving.is_some()
            || self.expanded;
        if !open && !in_front {
            return;
        }
        if self.moving.take().is_some() {
            self.notice = None;
        }
        self.switcher = None;
        self.expanded = false;
        self.open(1);
    }

    /// The remote's Recent key: the App Switcher, from the home screen only
    /// (inside a panel it would stack one overlay on another).
    pub fn recent(&mut self) -> Vec<Effect> {
        self.dirty = true;
        if self.page == Page::Apps
            && self.overlay.is_none()
            && self.switcher.is_none()
            && self.moving.is_none()
        {
            vec![Effect::Running]
        } else {
            vec![]
        }
    }

    pub fn back(&mut self) -> Vec<Effect> {
        self.pointer = false;
        self.dirty = true;
        if let Some(switcher) = &mut self.switcher
            && switcher.dismissed.is_none()
        {
            // The home is live at once; the switcher fades away over it.
            switcher.dismissed = Some(Instant::now());
            self.focus = switcher.restore.clone();
            return vec![];
        }
        if self.moving.take().is_some() {
            self.notice = None;
            return vec![];
        }
        if let Some(overlay) = self.overlay.take() {
            self.panel_started = None;
            if let Overlay::Options { id, .. } = &overlay
                && !self.preferences.reduced_motion
            {
                self.options_closed = Some((id.clone(), Instant::now()));
            }
            self.focus = match overlay {
                Overlay::Options { restore, .. } => restore,
                Overlay::Keyboard | Overlay::SystemKeyboard => {
                    self.search_closed = (!self.preferences.reduced_motion
                        && matches!(overlay, Overlay::Keyboard))
                    .then(Instant::now);
                    self.keyboard_return()
                }
            };
            return vec![];
        }
        if self.page == Page::Apps && self.expanded {
            // Focus stays on its dock app as Top Shelf lowers back.
            self.expanded = false;
            self.scroll = 0.;
            return vec![];
        }
        if self.page == Page::Settings && self.subpage.is_some() {
            self.close_subpage();
            return vec![];
        }
        if self.page == Page::Settings && !matches!(self.focus, Target::Nav(_)) {
            if matches!(self.focus, Target::Sidebar(_)) {
                self.open(1);
                return vec![];
            } else {
                self.focus = Target::Sidebar(self.section);
            }
            self.scroll = 0.;
            return vec![];
        }
        if self.page == Page::Settings {
            self.open(1);
            return vec![];
        }
        if self.page == Page::Apps && self.focus == Target::Search {
            self.query.clear();
            self.focus = Target::App(SEARCH_TILE.into());
            self.scroll = 0.;
            return vec![];
        }
        // Back on Home opens the app switcher, as on tvOS. Quit lives in Settings.
        vec![Effect::Running]
    }
}
