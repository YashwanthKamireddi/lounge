//! Typing: the search and location fields and their keyboards.
use super::*;

impl State {
    pub fn text(&mut self, text: &str) {
        if self.text_input_active() {
            if self.keyboard_field == TextField::Location {
                let mut query = self.weather.query.clone();
                query.extend(
                    text.chars()
                        .filter(|c| !c.is_control())
                        .take(120usize.saturating_sub(query.chars().count())),
                );
                self.weather.edit(query);
                self.dirty = true;
                return;
            }
            self.query.extend(
                text.chars()
                    .filter(|c| !c.is_control())
                    .take(120usize.saturating_sub(self.query.chars().count())),
            );
            self.scroll = 0.;
            self.dirty = true;
        }
    }

    pub fn delete(&mut self) {
        if self.text_input_active() {
            if self.keyboard_field == TextField::Location {
                let mut query = self.weather.query.clone();
                query.pop();
                self.weather.edit(query);
            } else {
                self.query.pop();
            }
            self.dirty = true;
        }
    }

    pub fn text_input_active(&self) -> bool {
        matches!(
            self.overlay,
            Some(Overlay::Keyboard | Overlay::SystemKeyboard)
        )
    }

    pub fn finish_text_input(&mut self) {
        if self.text_input_active() {
            self.overlay = None;
            self.panel_started = None;
            self.focus = self.keyboard_return();
            self.pointer = false;
            self.dirty = true;
        }
    }

    pub fn delete_surrounding_text(&mut self, offset: i32, length: i32) {
        if !self.text_input_active() || length <= 0 {
            return;
        }
        // SDL-webOS forwards the native text model's UTF-8 byte offsets,
        // relative to the insertion cursor (at the end of our query).
        let mut query = self.editing_text().to_owned();
        let mut start =
            (query.len() as i64 + i64::from(offset)).clamp(0, query.len() as i64) as usize;
        let mut end = start.saturating_add(length as usize).min(query.len());
        while !query.is_char_boundary(start) {
            start -= 1;
        }
        while !query.is_char_boundary(end) {
            end += 1;
        }
        query.replace_range(start..end, "");
        if self.keyboard_field == TextField::Location {
            self.weather.edit(query);
        } else {
            self.query = query;
            self.scroll = 0.;
        }
        self.dirty = true;
    }

    pub fn editing_text(&self) -> &str {
        if self.keyboard_field == TextField::Location {
            &self.weather.query
        } else {
            &self.query
        }
    }

    pub fn open_keyboard(&mut self, field: TextField) {
        self.keyboard_field = field;
        self.search_closed = None;
        // App search uses Luma's own tvOS keyboard everywhere; place names may
        // need the TV's full system keyboard.
        if field == TextField::Location
            && cfg!(all(
                feature = "webos",
                target_os = "linux",
                target_arch = "arm"
            ))
        {
            self.overlay = Some(Overlay::SystemKeyboard);
            self.focus = if field == TextField::Location {
                Target::WeatherQuery
            } else {
                Target::Search
            };
            self.panel_started = None;
            self.dirty = true;
            return;
        }
        self.overlay = Some(Overlay::Keyboard);
        self.focus = Target::Key(0);
        if !self.preferences.reduced_motion {
            self.panel_started = Some(Instant::now());
        }
        self.dirty = true;
    }

    pub(super) fn open_location(&mut self) {
        self.weather.edit(String::new());
        // A fresh page: no answer or message left from the last visit.
        self.weather.cancel_search();
        self.subpage = Some(Subpage::WeatherLocation);
        self.focus = Target::WeatherQuery;
        self.scroll = 0.;
        if !self.preferences.reduced_motion {
            self.panel_started = Some(Instant::now());
        }
        self.dirty = true;
    }

    pub(super) fn keyboard_return(&self) -> Target {
        if self.keyboard_field == TextField::Location {
            Target::WeatherSearch
        } else {
            Target::App(SEARCH_TILE.into())
        }
    }
}
