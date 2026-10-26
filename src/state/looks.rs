//! Profile pictures and the Light/Dark appearance.
use super::*;

impl State {
    /// Profile picture choices, as the preference names them: "rotate" (when
    /// there is more than one picture) and then every picture's file.
    pub fn avatar_choices(&self) -> Vec<String> {
        let rotate = (self.avatars.len() > 1).then(|| AVATAR_ROTATE.to_owned());
        rotate
            .into_iter()
            .chain(self.avatars.iter().map(|picture| picture.file.clone()))
            .collect()
    }

    /// The choice in effect: the preference, or the first picture by default.
    pub fn avatar_choice(&self) -> Option<String> {
        self.preferences
            .avatar
            .clone()
            .or_else(|| self.avatars.first().map(|picture| picture.file.clone()))
    }

    pub fn set_avatars(&mut self, pictures: Vec<crate::avatars::Picture>) {
        self.avatars = pictures;
        self.show_avatar(false);
    }

    /// Shows the chosen picture (or, rotating, the current one), fading from
    /// the last when `fade`.
    pub(super) fn show_avatar(&mut self, fade: bool) {
        let chosen = self.preferences.avatar.as_deref();
        if chosen != Some(AVATAR_ROTATE) {
            self.avatar_index = chosen
                .and_then(|file| self.avatars.iter().position(|picture| picture.file == file))
                .unwrap_or(0);
        }
        let next = self
            .avatars
            .get(self.avatar_index)
            .map(|picture| picture.small.clone());
        if fade
            && !self.preferences.reduced_motion
            && next != self.avatar
            && let Some(previous) = self.avatar.clone()
        {
            self.avatar_from = Some((previous, Instant::now()));
        }
        self.avatar = next;
        self.avatar_since = Instant::now();
        self.dirty = true;
    }

    /// Whether the appearance wanted now is light: Auto follows the clock.
    pub(super) fn wants_light(&self) -> bool {
        match self.preferences.appearance {
            Appearance::Light => true,
            Appearance::Dark => false,
            Appearance::Auto => (LIGHT_FROM..DARK_FROM).contains(&self.hour),
        }
    }

    /// How light the screen is (0 dark, 1 light), cross-fading after a change.
    pub fn light(&self) -> f32 {
        let target = if self.light_shown.unwrap_or_else(|| self.wants_light()) {
            1.
        } else {
            0.
        };
        match self.appearance_from {
            Some((from, start)) => {
                let t = (start.elapsed().as_secs_f32() / APPEARANCE_FADE).min(1.);
                from + (target - from) * t * t * (3. - 2. * t)
            }
            None => target,
        }
    }

    /// A change of appearance (chosen, or Auto passing 06:00 or 19:00)
    /// cross-fades from wherever the screen is; the first frame just shows it.
    pub(super) fn track_appearance(&mut self) {
        let wanted = self.wants_light();
        if self.light_shown.is_some_and(|shown| shown != wanted) && !self.preferences.reduced_motion
        {
            self.appearance_from = Some((self.light(), Instant::now()));
        }
        if self.light_shown != Some(wanted) {
            self.light_shown = Some(wanted);
            self.dirty = true;
        }
        if self
            .appearance_from
            .is_some_and(|(_, start)| start.elapsed().as_secs_f32() >= APPEARANCE_FADE)
        {
            self.appearance_from = None;
            self.dirty = true;
        }
    }

    /// Rotating pictures change every AVATAR_SECONDS, crossfading.
    pub(super) fn rotate_avatar(&mut self) {
        if self.preferences.avatar.as_deref() == Some(AVATAR_ROTATE)
            && self.avatars.len() > 1
            && self.avatar_since.elapsed().as_secs_f32() >= AVATAR_SECONDS
        {
            self.avatar_index = (self.avatar_index + 1) % self.avatars.len();
            self.show_avatar(true);
        }
        if self
            .avatar_from
            .as_ref()
            .is_some_and(|(_, since)| since.elapsed().as_secs_f32() >= AVATAR_FADE)
        {
            self.avatar_from = None;
            self.dirty = true;
        }
    }
}
