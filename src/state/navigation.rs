//! Remote directions: moving focus across the home, Settings levels and overlays.
use super::*;

impl State {
    /// Starts a Settings push when the level shown changed (0 the section
    /// list, 1 a section, 2 a sub-page). Called as each frame is composed, so
    /// the push begins on the very frame the new level first draws.
    pub fn track_settings_level(&mut self) -> bool {
        let depth = if self.page != Page::Settings {
            0
        } else if self.subpage.is_some() {
            2
        } else if matches!(self.focus, Target::Sidebar(_)) {
            0
        } else {
            1
        };
        // On the section list, moving between sections is focus, not a new
        // page: only entering or leaving a level pushes.
        let level = (depth, if depth == 0 { 0 } else { self.section });
        let changed = self.page == Page::Settings && level != self.settings_level;
        if changed {
            self.settings_previous = self.settings_level;
            self.settings_ghost = None;
            if !self.preferences.reduced_motion && self.page_since.is_none() {
                let direction = if depth >= self.settings_level.0 {
                    1.
                } else {
                    -1.
                };
                self.settings_since = Some((crate::clock::now(), direction));
            }
            self.dirty = true;
        }
        if self.page != Page::Settings {
            self.settings_ghost = None;
        }
        self.settings_level = level;
        changed && self.settings_since.is_some()
    }

    pub(super) fn section_entry(&self) -> Target {
        match self.section {
            1 => Target::Background(
                BACKGROUNDS
                    .iter()
                    .position(|(id, _)| *id == self.preferences.background)
                    .unwrap_or(3),
            ),
            WEATHER_SECTION => Target::WeatherToggle,
            _ => Target::Setting(0),
        }
    }

    pub(super) fn close_subpage(&mut self) {
        if self.subpage == Some(Subpage::WeatherLocation) {
            self.weather.cancel_search();
        }
        self.panel_started = None;
        self.pointer = false;
        self.focus = match self.subpage.take() {
            Some(Subpage::Clock) => Target::Setting(1),
            Some(Subpage::WeatherLocation) => Target::WeatherLocation,
            Some(Subpage::Temperature) => Target::WeatherTemperature,
            // Its row goes once nothing is hidden; focus takes the row above it.
            Some(Subpage::Hidden) => Target::Setting(if self.hidden_count() > 0 {
                7
            } else if !self.heroes.is_empty() {
                6
            } else if self.tv_home.is_some() {
                10
            } else {
                2
            }),
            Some(Subpage::Avatar) => Target::Setting(8),
            Some(Subpage::Appearance) => Target::Setting(9),
            None => Target::Sidebar(self.section),
        };
        self.scroll = 0.;
        self.dirty = true;
    }

    pub fn direction(&mut self, direction: Direction, controls: &[Control]) -> Vec<Effect> {
        self.pointer = false;
        self.dirty = true;
        if let Some(switcher) = &mut self.switcher
            && switcher.dismissed.is_none()
        {
            let Target::Switch(index) = self.focus else {
                return vec![];
            };
            match direction {
                Direction::Left => self.focus = Target::Switch(index.saturating_sub(1)),
                Direction::Right => {
                    self.focus =
                        Target::Switch((index + 1).min(switcher.apps.len().saturating_sub(1)))
                }
                Direction::Up if index < switcher.apps.len() => {
                    // tvOS: swipe a window up to quit that app.
                    let id = switcher.apps.remove(index);
                    let now = crate::clock::now();
                    switcher
                        .closing
                        .push((id.clone(), now, index as f32 - switcher.shown));
                    if switcher.apps.is_empty() {
                        // Let the last window finish flying away, then leave.
                        switcher.dismissed = Some(now);
                        self.focus = switcher.restore.clone();
                    } else {
                        // The windows to the right slide into the gap.
                        switcher.reflow = Some((index, now));
                        self.focus = Target::Switch(index.min(switcher.apps.len() - 1));
                    }
                    return vec![Effect::Close(id)];
                }
                Direction::Up => {}
                Direction::Down => {}
            }
            return vec![];
        }
        if matches!(self.overlay, Some(Overlay::SystemKeyboard)) {
            // The TV keyboard owns its arrows until Done/Back dismisses it.
            return vec![];
        }
        if let Some(id) = self.moving.clone() {
            // Moving reflows the others, as tvOS does: Left/Right by one
            // slot, Up/Down by one row. Hidden or missing ids keep their place.
            let mut ids = self.preferences.favorites(&self.inventory.apps);
            let slots: Vec<_> = ids
                .iter()
                .enumerate()
                .filter_map(|(index, id)| {
                    (!self.preferences.hidden.contains(id)
                        && self.inventory.apps.iter().any(|app| &app.id == id))
                    .then_some(index)
                })
                .collect();
            if let Some(index) = slots.iter().position(|slot| ids[*slot] == id) {
                let last = slots.len() - 1;
                let next = match direction {
                    Direction::Left => index.saturating_sub(1),
                    Direction::Right => (index + 1).min(last),
                    Direction::Up => index.saturating_sub(APPS_COLUMNS),
                    Direction::Down => (index + APPS_COLUMNS).min(last),
                };
                if next != index {
                    let mut visible: Vec<String> =
                        slots.iter().map(|slot| ids[*slot].clone()).collect();
                    let before = visible.clone();
                    let moved = visible.remove(index);
                    visible.insert(next, moved);
                    if !self.preferences.reduced_motion {
                        // Each tile starts from where it is drawn now: its
                        // remaining glide plus the change of slot. Quick
                        // presses redirect tiles in flight instead of
                        // snapping them back to a slot first.
                        let mut offsets = self.glide_offsets();
                        for (new, id) in visible.iter().enumerate() {
                            let old = before.iter().position(|item| item == id).unwrap_or(new);
                            let (ox, oy) = self.slot_position(old);
                            let (nx, ny) = self.slot_position(new);
                            let (dx, dy) = offsets.get(id).copied().unwrap_or_default();
                            let offset = (dx + ox - nx, dy + oy - ny);
                            if offset.0.abs() + offset.1.abs() > 0.5 {
                                offsets.insert(id.clone(), offset);
                            } else {
                                offsets.remove(id);
                            }
                        }
                        self.rearranged = Some((crate::clock::now(), offsets));
                    }
                    for (slot, item) in slots.iter().zip(visible) {
                        ids[*slot] = item;
                    }
                    self.preferences.favorites = Some(ids);
                    self.unsaved_order = true;
                    self.dirty = true;
                    return vec![];
                }
            }
            return vec![];
        }
        if self.overlay.is_none()
            && matches!(self.focus, Target::InputsButton | Target::SearchButton)
        {
            // Down goes back to the dock app Up came from; Left and Right cross the bar.
            match direction {
                Direction::Down => self.leave_bar(),
                Direction::Left if self.focus == Target::InputsButton => {
                    self.focus(Target::SearchButton, controls)
                }
                Direction::Right
                    if self.focus == Target::SearchButton && !self.input_choices().is_empty() =>
                {
                    self.focus(Target::InputsButton, controls)
                }
                _ => {}
            }
            return vec![];
        }
        if self.overlay.is_none() {
            if self.page == Page::Home
                && let Target::App(id) = &self.focus
            {
                let apps = self.apps();
                if let Some(index) = apps.iter().position(|app| &app.id == id) {
                    match direction {
                        Direction::Left | Direction::Right => {
                            let next = if matches!(direction, Direction::Left) {
                                index.saturating_sub(1)
                            } else {
                                (index + 1).min(apps.len() - 1)
                            };
                            let id = apps[next].id.clone();
                            self.focus(Target::App(id), controls);
                        }
                        Direction::Up => self.focus(Target::Nav(0), controls),
                        Direction::Down => {
                            let x = controls
                                .iter()
                                .find(|control| control.target == self.focus)
                                .map_or(960., |control| control.rect.center().0);
                            let top = controls
                                .iter()
                                .filter(|control| matches!(control.target, Target::Input(_)))
                                .map(|control| control.rect.y)
                                .fold(f32::INFINITY, f32::min);
                            let next = controls
                                .iter()
                                .filter(|control| {
                                    matches!(control.target, Target::Input(_))
                                        && (control.rect.y - top).abs() < 1.
                                })
                                .min_by(|a, b| {
                                    (a.rect.center().0 - x)
                                        .abs()
                                        .total_cmp(&(b.rect.center().0 - x).abs())
                                })
                                .map(|control| control.target.clone());
                            if let Some(next) = next {
                                self.focus(next, controls);
                            }
                        }
                    }
                    return vec![];
                }
            }
            if let Target::Choice(index) = self.focus {
                let count = match self.subpage {
                    Some(Subpage::Clock | Subpage::Temperature) => 2,
                    Some(Subpage::Appearance) => APPEARANCES.len(),
                    Some(Subpage::Hidden) => self.hidden_count().max(1),
                    Some(Subpage::Avatar) => self.avatar_choices().len().max(1),
                    _ => 3,
                };
                match direction {
                    Direction::Left => self.close_subpage(),
                    Direction::Up => self.focus(
                        if index == 0 {
                            Target::Nav(2)
                        } else {
                            Target::Choice(index - 1)
                        },
                        controls,
                    ),
                    Direction::Down => self.focus(Target::Choice((index + 1) % count), controls),
                    Direction::Right => {}
                }
                return vec![];
            }
            if let Target::Sidebar(index) = self.focus {
                match direction {
                    Direction::Right => {
                        let entry = self.section_entry();
                        self.focus(entry, controls);
                    }
                    Direction::Up => self.focus(
                        if index == 0 {
                            Target::Nav(2)
                        } else {
                            Target::Sidebar(index - 1)
                        },
                        controls,
                    ),
                    Direction::Down => {
                        self.focus(Target::Sidebar((index + 1) % SECTION_COUNT), controls)
                    }
                    Direction::Left => {}
                }
                return vec![];
            }
            // Every row with a chevron opens its page on Right, as on tvOS.
            if matches!(
                self.focus,
                Target::Setting(1 | 7 | 8 | 9)
                    | Target::WeatherLocation
                    | Target::WeatherTemperature
            ) && matches!(direction, Direction::Right)
            {
                return self.activate();
            }
            if matches!(self.focus, Target::Setting(_) | Target::WeatherToggle)
                && matches!(direction, Direction::Right)
            {
                return vec![];
            }
            if self.focus == Target::Setting(0) && matches!(direction, Direction::Up) {
                self.focus(Target::Nav(2), controls);
                return vec![];
            }
            if self.focus == Target::WeatherToggle && matches!(direction, Direction::Up) {
                self.focus(Target::Nav(2), controls);
                return vec![];
            }
            if matches!(
                self.focus,
                Target::WeatherQuery | Target::WeatherSearch | Target::WeatherClear
            ) {
                match direction {
                    Direction::Left => {
                        self.close_subpage();
                        return vec![];
                    }
                    Direction::Up => {
                        self.focus(
                            if self.preferences.weather_location.is_some()
                                && self.focus != Target::WeatherClear
                            {
                                Target::WeatherClear
                            } else {
                                Target::Nav(2)
                            },
                            controls,
                        );
                        return vec![];
                    }
                    Direction::Down => {
                        let target = if self.focus == Target::WeatherClear {
                            Some(Target::WeatherQuery)
                        } else if !self.weather.results.is_empty() {
                            Some(Target::WeatherResult(0))
                        } else if self.focus == Target::WeatherQuery {
                            Some(Target::WeatherSearch)
                        } else {
                            None
                        };
                        if let Some(target) = target {
                            self.focus(target, controls);
                        }
                        return vec![];
                    }
                    _ => {}
                }
            }
            if let Target::WeatherResult(index) = self.focus {
                match direction {
                    Direction::Left => self.close_subpage(),
                    Direction::Up => self.focus(
                        if index == 0 {
                            Target::WeatherQuery
                        } else {
                            Target::WeatherResult(index - 1)
                        },
                        controls,
                    ),
                    Direction::Down if index + 1 < self.weather.results.len() => {
                        self.focus(Target::WeatherResult(index + 1), controls)
                    }
                    _ => {}
                }
                return vec![];
            }
            if let Target::Nav(index) = self.focus {
                let next = match direction {
                    Direction::Left if index > 0 => Some(index - 1),
                    Direction::Right if index < 2 => Some(index + 1),
                    _ => None,
                };
                if let Some(next) = next {
                    self.open(next);
                    return vec![];
                }
            }
            if self.page == Page::Apps
                && let Target::App(id) = &self.focus
                && matches!(direction, Direction::Up | Direction::Down)
            {
                let apps = self.apps();
                if let Some(index) = apps.iter().position(|app| &app.id == id) {
                    // A preceding row may be above the visible scroll region.
                    // Its grid index still defines the route; reveal_focus then
                    // scrolls it into view, without letting the fixed nav steal Up.
                    let next = match direction {
                        // Up from the dock: Top Shelf full screen first for an app with art
                        // (Down returns), then the Inputs button beside the time pill. Only
                        // apps with Top Shelf art go full screen: a lone wallpaper without
                        // the dock would only confuse.
                        Direction::Up
                            if index < APPS_COLUMNS && (self.expanded || !self.has_art(id)) =>
                        {
                            // The bar's button above the tile's half of the screen: Search over
                            // the left, Inputs over the right.
                            let x = controls
                                .iter()
                                .find(|control| control.target == self.focus)
                                .map_or(0., |control| control.rect.center().0);
                            self.bar_return = Some(self.focus.clone());
                            Some(if x >= 960. && !self.input_choices().is_empty() {
                                Target::InputsButton
                            } else {
                                Target::SearchButton
                            })
                        }
                        Direction::Up if index < APPS_COLUMNS => {
                            self.expanded = true;
                            self.shelf_since = None;
                            self.reveal_focus(controls);
                            return vec![];
                        }
                        Direction::Down if self.expanded => {
                            self.expanded = false;
                            self.reveal_focus(controls);
                            return vec![];
                        }
                        Direction::Up => Some(Target::App(apps[index - APPS_COLUMNS].id.clone())),
                        Direction::Down
                            if (index / APPS_COLUMNS + 1) * APPS_COLUMNS < apps.len() =>
                        {
                            Some(Target::App(
                                apps[(index + APPS_COLUMNS).min(apps.len() - 1)].id.clone(),
                            ))
                        }
                        _ => None,
                    };
                    if let Some(next) = next {
                        self.focus(next, controls);
                    }
                    return vec![];
                }
            }
            if let Target::Background(index) = self.focus {
                match direction {
                    Direction::Right if index + 1 < BACKGROUNDS.len() => {
                        self.focus(Target::Background(index + 1), controls);
                        return vec![];
                    }
                    Direction::Left if index % 3 != 0 => {
                        self.focus(Target::Background(index - 1), controls);
                        return vec![];
                    }
                    _ => {}
                }
            }
            if matches!(self.focus, Target::Nav(_)) && matches!(direction, Direction::Down) {
                if self.page == Page::Settings {
                    self.subpage = None;
                    self.section = 0;
                }
                self.focus(self.first_content(), controls);
                return vec![];
            }
            if self.page == Page::Settings
                && matches!(direction, Direction::Left)
                && !matches!(self.focus, Target::Nav(_) | Target::Sidebar(_))
            {
                if let Target::Background(index) = self.focus {
                    if index % 3 != 0 { /* left follows gallery order */
                    } else {
                        self.focus(Target::Sidebar(self.section), controls);
                        return vec![];
                    }
                } else {
                    self.focus(Target::Sidebar(self.section), controls);
                    return vec![];
                }
            }
        }
        let Some(current) = controls.iter().find(|c| c.target == self.focus) else {
            self.focus(self.first_content(), controls);
            return vec![];
        };
        let (x, y) = current.rect.center();
        let next = controls
            .iter()
            .filter(|candidate| candidate.target != self.focus)
            // A notification is never a stop for the remote's arrows (the
            // pointer can still click it away).
            .filter(|candidate| candidate.target != Target::DismissNotice)
            .filter(|candidate| {
                self.page != Page::Home
                    || !matches!(candidate.target, Target::App(_))
                    || (candidate.rect.x >= HOME_STRIP_LEFT - 0.1
                        && candidate.rect.x + candidate.rect.w <= HOME_STRIP_RIGHT + 0.1)
            })
            .filter_map(|candidate| {
                // Header controls never steal an Up route from the active tab.
                if self.overlay.is_none()
                    && matches!(direction, Direction::Up)
                    && candidate.rect.y < 150.
                    && candidate.target != Target::Nav(self.page_index())
                {
                    return None;
                }
                let (cx, cy) = candidate.rect.center();
                let (primary, cross, overlap) = match direction {
                    Direction::Up => (
                        y - cy,
                        (x - cx).abs(),
                        (current.rect.w + candidate.rect.w) / 2.,
                    ),
                    Direction::Down => (
                        cy - y,
                        (x - cx).abs(),
                        (current.rect.w + candidate.rect.w) / 2.,
                    ),
                    Direction::Left => (
                        x - cx,
                        (y - cy).abs(),
                        (current.rect.h + candidate.rect.h) / 2.,
                    ),
                    Direction::Right => (
                        cx - x,
                        (y - cy).abs(),
                        (current.rect.h + candidate.rect.h) / 2.,
                    ),
                };
                (primary > 1.).then_some((
                    candidate.target.clone(),
                    primary + cross * 2. + if cross >= overlap { 10000. } else { 0. },
                ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(target, _)| target);
        if let Some(next) = next {
            self.focus(next, controls);
        }
        vec![]
    }
}
