//! The tvOS app switcher: open apps as cards, and quick launches.
use super::*;

impl State {
    /// Opens the switcher with webOS's open apps, most recently used first.
    pub fn open_switcher(&mut self, running: Vec<String>) {
        // The reply is asynchronous: the viewer may have moved on meanwhile.
        if self.page != Page::Apps || self.overlay.is_some() || self.switcher.is_some() {
            return;
        }
        let known = |id: &String| {
            self.inventory.apps.iter().any(|app| &app.id == id)
                && id != crate::platform::APP_ID
                && id != crate::platform::HOME_ID
        };
        let mut apps: Vec<String> = self
            .preferences
            .recent
            .iter()
            .filter(|id| running.contains(id))
            .cloned()
            .collect();
        for id in running {
            if !apps.contains(&id) {
                apps.push(id);
            }
        }
        apps.retain(known);
        self.dirty = true;
        if apps.is_empty() {
            self.notify(i18n::t("noOpenApps"));
            return;
        }
        let restore = std::mem::replace(&mut self.focus, Target::Switch(0));
        self.switcher = Some(Switcher {
            apps,
            shown: 0.,
            opened: crate::clock::now(),
            closing: vec![],
            reflow: None,
            dismissed: None,
            restore,
        });
    }

    /// The visible Home favourite at `slot`, opened directly.
    /// The remote's number keys open the apps in the order the grid shows
    /// them (hidden ones and LG's Home Dashboard never count). Lounge's own
    /// Settings and Search tiles are not apps webOS can open, so they are
    /// skipped rather than launched.
    pub fn quick_launch(&mut self, slot: usize) -> Vec<Effect> {
        self.apps()
            .into_iter()
            .filter(|app| app.id != SETTINGS_TILE && app.id != SEARCH_TILE)
            .nth(slot)
            .map_or_else(Vec::new, |app| vec![Effect::Launch(app.id.clone())])
    }

    pub fn note_launch(&mut self, id: &str) {
        self.preferences.recent.retain(|item| item != id);
        self.preferences.recent.insert(0, id.to_owned());
        self.preferences.recent.truncate(24);
    }

    /// The switcher fills the screen: open long enough to have faded in, and
    /// not leaving. Nothing beneath it needs drawing then.
    pub fn switcher_covers(&self) -> bool {
        self.switcher.as_ref().is_some_and(|switcher| {
            crate::clock::since(switcher.opened).as_secs_f32() >= 0.3
                && switcher.dismissed.is_none()
        })
    }
}
