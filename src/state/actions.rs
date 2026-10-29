//! OK, Back, Home, hold and the context menu: what each press does.
use super::*;

impl State {
    pub fn options(&mut self) {
        if self.overlay.is_some() || self.moving.is_some() {
            return;
        }
        if let Target::App(id) = &self.focus {
            // The pills anchor to the tile, so a full-screen Top Shelf lowers.
            if self.expanded {
                self.expanded = false;
                self.scroll = 0.;
            }
            self.options_closed = None;
            self.overlay = Some(Overlay::Options {
                id: id.clone(),
                restore: self.focus.clone(),
            });
            self.focus = Target::Option(0);
            if !self.preferences.reduced_motion {
                self.panel_started = Some(crate::clock::now());
            }
            self.dirty = true;
        }
    }

    /// Holding OK on an app opens its menu: Move (focused, so Move is hold then OK), Hide and
    /// Uninstall. The LG remote has no Menu key, so a hold that went straight to Move left Hide
    /// and Uninstall unreachable. In a menu or Move it opens nothing new; on anything else a
    /// hold is OK.
    pub fn hold(&mut self) -> Vec<Effect> {
        if matches!(self.focus, Target::App(_)) {
            self.options();
            return vec![];
        }
        // Anywhere else a slow press is still a press: a Settings row, a
        // key, a menu pill or a switcher card does what OK does.
        self.activate()
    }

    /// Enter Move for `id`. The home order becomes explicit (exactly as now
    /// shown, so nothing jumps); the dock is simply its first row.
    pub fn start_move(&mut self, id: &str) {
        // Move works on the grid, so a full-screen Top Shelf lowers first.
        if self.expanded {
            self.expanded = false;
            self.scroll = 0.;
        }
        let mut order: Vec<String> = self.apps().iter().map(|app| app.id.clone()).collect();
        if !order.iter().any(|item| item == id) {
            return;
        }
        // Hidden apps keep their places in the order, so one shown again
        // comes back where it was (Move's arrows skip over them).
        for (index, item) in self
            .preferences
            .favorites(&self.inventory.apps)
            .iter()
            .enumerate()
        {
            if self.preferences.hidden.contains(item) && !order.contains(item) {
                order.insert(index.min(order.len()), item.clone());
            }
        }
        self.preferences.favorites = Some(order);
        self.moving = Some(id.into());
        self.moving_since = (!self.preferences.reduced_motion).then(crate::clock::now);
        // How Move works is said beside the app in hand (view::overlays::move_hint): a banner
        // covered the dock's last tiles, where apps are moved to.
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
                // Settings opens on General, and Right enters General: the
                // section is reset with the highlight, not left from last time.
                self.section = 0;
                self.focus = Target::Sidebar(0);
                // The start-up read can time out on a busy cold boot; asking
                // again here keeps the Lounge as Home row from staying hidden.
                if self.tv_home.is_none() && !self.tv_home_busy {
                    return vec![Effect::TvHome(None)];
                }
                return vec![];
            }
            Target::SearchButton => {
                self.open_keyboard(TextField::Apps);
                return vec![];
            }
            Target::App(id) | Target::Input(id) if !self.launch_busy() => {
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
                    self.panel_started = Some(crate::clock::now());
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
                            self.panel_started = Some(crate::clock::now());
                        }
                        self.focus = Target::Choice(usize::from(self.hour_24));
                        return vec![];
                    }
                    2 => self.preferences.reduced_motion = !self.preferences.reduced_motion,
                    9 => {
                        self.subpage = Some(Subpage::Appearance);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(crate::clock::now());
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
                    10 => {
                        if let Some(on) = self.tv_home
                            && !self.tv_home_busy
                        {
                            self.tv_home_busy = true;
                            self.dirty = true;
                            return vec![Effect::TvHome(Some(!on))];
                        }
                        return vec![];
                    }
                    8 => {
                        self.subpage = Some(Subpage::Avatar);
                        if !self.preferences.reduced_motion {
                            self.panel_started = Some(crate::clock::now());
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
                            self.panel_started = Some(crate::clock::now());
                        }
                        self.focus = Target::Choice(0);
                        return vec![];
                    }
                    _ => return vec![Effect::Refresh],
                }
                return vec![Effect::Save];
            }
            Target::Background(index) => {
                // The cross-fade starts once the new pictures are on the GPU,
                // from what shows now; a running fade finishes first.
                let fading = matches!(self.previous_background, Some((_, Some(_))));
                if self.preferences.background != BACKGROUNDS[index].0 && !fading {
                    let shown = self.wallpaper().to_owned();
                    self.previous_background = Some((shown, None));
                }
                self.preferences.background = BACKGROUNDS[index].0.into();
                return vec![Effect::Save];
            }
            Target::Choice(index) => {
                match self.subpage.clone() {
                    // The TV's own setting, not a Lounge preference: every
                    // clock on the TV changes with it.
                    Some(Subpage::Clock) => {
                        self.hour_24 = index == 1;
                        self.close_subpage();
                        return vec![Effect::ClockFormat(self.hour_24)];
                    }
                    Some(Subpage::Appearance) => {
                        self.preferences.appearance = APPEARANCES[index.min(APPEARANCES.len() - 1)];
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
                        let Some(app) = self
                            .hidden_apps()
                            .get(index)
                            .map(|app| (app.id.clone(), app.title.clone()))
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
            // Uninstall asks first, in an alert over the dimmed home (the
            // menu gives way to it: never two menus at once).
            Target::Option(2) if matches!(&self.overlay, Some(Overlay::Options { id, .. }) if self.can_uninstall(id)) =>
            {
                let Some(Overlay::Options { id, restore }) = self.overlay.take() else {
                    return vec![];
                };
                self.overlay = Some(Overlay::Uninstall { id, restore });
                // Cancel is where focus lands: OK pressed twice in a hurry
                // never removes an app.
                self.focus = Target::Alert(1);
                self.pointer = false;
                self.pressed = None;
                self.panel_started = (!self.preferences.reduced_motion).then(crate::clock::now);
                return vec![];
            }
            Target::Alert(0) => {
                let Some(Overlay::Uninstall { id, restore }) = self.overlay.take() else {
                    return vec![];
                };
                self.panel_started = None;
                self.focus = restore;
                if !self.can_uninstall(&id) {
                    return vec![];
                }
                // The tile goes at once, as on tvOS, and the ones behind it glide into its
                // place; focus takes the tile that moves there.
                let before: Vec<String> = self.apps().iter().map(|app| app.id.clone()).collect();
                let slot = before.iter().position(|item| *item == id);
                self.uninstalling.push(id.clone());
                self.glide_from(&before);
                if let Some(slot) = slot {
                    let apps = self.apps();
                    self.focus = apps
                        .get(slot.min(apps.len().saturating_sub(1)))
                        .map(|app| Target::App(app.id.clone()))
                        .unwrap_or_else(|| self.first_content());
                }
                return vec![Effect::Uninstall(id)];
            }
            Target::Alert(_) => return self.back(),
            Target::InputsButton => {
                let first = self.input_choices().into_iter().next();
                if let Some((id, ..)) = first {
                    self.overlay = Some(Overlay::Inputs);
                    self.focus = Target::InputPick(id);
                    self.panel_started = (!self.preferences.reduced_motion).then(crate::clock::now);
                    self.dirty = true;
                }
                return vec![];
            }
            Target::InputPick(id) => {
                self.overlay = None;
                self.panel_started = None;
                self.leave_bar();
                self.note_launch(&id);
                return vec![Effect::Launch(id)];
            }
            Target::Option(index) => {
                let Some(Overlay::Options { id, restore }) = self.overlay.take() else {
                    return vec![];
                };
                self.panel_started = None;
                if !self.preferences.reduced_motion {
                    self.options_closed = Some((id.clone(), crate::clock::now()));
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
                    if self.moving.is_some() {
                        return vec![Effect::Save];
                    }
                    // The app went while its menu was open (uninstalled,
                    // unplugged): focus goes back to a tile, as for Hide.
                }
                if index == 1 && id != SETTINGS_TILE {
                    if self.preferences.hidden.contains(&id) {
                        self.preferences.hidden.retain(|item| item != &id);
                        self.notify(i18n::format("appRestored", &[("app", &title)]));
                    } else {
                        self.preferences.hidden.push(id.clone());
                        self.notify(i18n::format("appHidden", &[("app", &title)]));
                    }
                    self.glide_from(&old_ids);
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
                    self.search_closed = (!self.preferences.reduced_motion).then(crate::clock::now);
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
            switcher.dismissed = Some(crate::clock::now());
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
                self.options_closed = Some((id.clone(), crate::clock::now()));
            }
            self.focus = match overlay {
                Overlay::Options { restore, .. } | Overlay::Uninstall { restore, .. } => restore,
                Overlay::Inputs => Target::InputsButton,
                Overlay::Keyboard | Overlay::SystemKeyboard => {
                    self.search_closed = (!self.preferences.reduced_motion
                        && matches!(overlay, Overlay::Keyboard))
                    .then(crate::clock::now);
                    self.keyboard_return()
                }
            };
            return vec![];
        }
        if matches!(self.focus, Target::InputsButton | Target::SearchButton) {
            self.leave_bar();
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
            self.focus = Target::SearchButton;
            self.scroll = 0.;
            return vec![];
        }
        // Back on Home opens the app switcher, as on tvOS. Quit lives in Settings.
        vec![Effect::Running]
    }
}

impl State {
    /// Whether the context menu offers Uninstall for `id`: an installed app
    /// webOS lets the owner remove (never an input, Lounge's own tiles, LG's
    /// apps, Homebrew Channel or Lounge), not already on its way off.
    pub fn can_uninstall(&self, id: &str) -> bool {
        crate::uninstall::allowed(id)
            && !self.uninstalling.iter().any(|item| item == id)
            && self
                .inventory
                .apps
                .iter()
                .any(|app| app.id == id && app.removable && !app.brand.starts_with("lounge-"))
    }

    /// webOS answered a removal. Gone: it leaves every list Lounge keeps,
    /// and the app list is read again. Refused: its tile comes back where it
    /// was and a banner says so; nothing else changed.
    pub fn uninstalled(&mut self, id: &str, result: Result<(), String>) -> Vec<Effect> {
        let Some(index) = self.uninstalling.iter().position(|item| item == id) else {
            return vec![];
        };
        self.uninstalling.remove(index);
        self.dirty = true;
        let title = self
            .inventory
            .apps
            .iter()
            .find(|app| app.id == id)
            .map_or_else(|| id.to_owned(), |app| app.title.clone());
        if let Err(error) = result {
            eprintln!("Lounge uninstall {id}: {error}");
            self.notify(i18n::format("uninstallError", &[("app", &title)]));
            return vec![];
        }
        self.inventory.apps.retain(|app| app.id != id);
        if let Some(favorites) = &mut self.preferences.favorites {
            favorites.retain(|item| item != id);
        }
        self.preferences.hidden.retain(|item| item != id);
        self.preferences.recent.retain(|item| item != id);
        self.snapshots.remove(id);
        self.reconcile_focus();
        vec![Effect::Save, Effect::Refresh]
    }

    /// What the Inputs panel lists, in order: Live TV, then every input, plugged-in ones first
    /// (port order kept within each), as (app id, name, whether a device is plugged in).
    pub fn input_choices(&self) -> Vec<(String, String, Option<bool>)> {
        let live = self
            .inventory
            .apps
            .iter()
            .filter(|app| app.id == crate::model::LIVE_TV)
            .map(|app| (app.id.clone(), app.title.clone(), None));
        let inputs = self
            .inventory
            .inputs
            .iter()
            .map(|input| (input.id.clone(), input.title.clone(), input.connected));
        let (plugged, rest): (Vec<_>, Vec<_>) =
            inputs.partition(|(_, _, connected)| *connected == Some(true));
        live.chain(plugged).chain(rest).collect()
    }

    /// From the Search or Inputs button back to the dock app Up came from.
    pub(super) fn leave_bar(&mut self) {
        self.focus = self
            .bar_return
            .take()
            .filter(|target| matches!(target, Target::App(id) if self.apps().iter().any(|app| &app.id == id)))
            .unwrap_or_else(|| self.first_content());
        self.dirty = true;
    }
}
