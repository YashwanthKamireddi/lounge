//! Focus: which control is focused, how far its lift has come, and keeping it on screen.
use super::*;

impl State {
    /// Eased 0..1 focus emphasis for a target; continuous across rapid moves.
    pub fn focus_amount(&self, target: &Target) -> f32 {
        if &self.focus == target {
            self.focus_since.as_ref().map_or(1., eased)
        } else if let Some((from, start, level)) = &self.focus_from
            && from == target
        {
            level * (1. - eased(start))
        } else {
            0.
        }
    }

    pub fn track_focus(&mut self) {
        if self.focus != self.last_focus {
            let level = self.focus_since.as_ref().map_or(1., eased);
            let previous = std::mem::replace(&mut self.last_focus, self.focus.clone());
            if let Target::App(id) = &self.focus
                && let Some(slot) = self.apps().iter().position(|app| &app.id == id)
            {
                self.focus_slot = slot;
            }
            // Only dock apps own Top Shelf; the grid below keeps the last one.
            // The art changes once focus rests on an app: a sweep along the
            // dock crosses no full-screen dissolves (each costs this TV's GPU
            // its frame budget for the whole fade).
            if let Target::App(id) = &self.focus
                && self
                    .apps()
                    .iter()
                    .take(APPS_COLUMNS)
                    .any(|app| &app.id == id)
                && self.shelf_app.as_ref() != Some(id)
            {
                if self.shelf_app.is_none() || self.preferences.reduced_motion {
                    self.shelf_app = Some(id.clone());
                    self.shelf_pending = None;
                } else {
                    self.shelf_pending = Some((id.clone(), crate::clock::now()));
                }
            } else {
                self.shelf_pending = None;
            }
            // The first real focus (out of the startup placeholder) appears
            // already settled: webOS is animating Lounge's window in, and a
            // tile growing at the same time read as a second opening.
            let first = matches!(previous, Target::Nav(_)) && self.page == Page::Apps;
            let now = (!self.preferences.reduced_motion && !first).then(crate::clock::now);
            self.focus_from = now.map(|now| (previous, now, level));
            self.focus_since = now;
            self.dirty = true;
        }
        if self
            .shelf_pending
            .as_ref()
            .is_some_and(|(_, since)| crate::clock::since(since).as_secs_f32() >= SHELF_REST)
            && let Some((id, _)) = self.shelf_pending.take()
        {
            let previous = self.shelf_app.replace(id);
            self.shelf_from = Some((previous, crate::clock::now()));
            self.dirty = true;
        }
        if let Some((_, start)) = &self.shelf_from {
            if crate::clock::since(start).as_secs_f32() >= SHELF_FADE {
                self.shelf_from = None;
            }
            self.dirty = true;
        }
        let done = |start: &Instant| crate::clock::since(start).as_secs_f32() * 1000. >= FOCUS_MS;
        if self.focus_since.is_some() || self.focus_from.is_some() {
            self.dirty = true;
            if self.focus_since.as_ref().is_none_or(done) {
                self.focus_since = None;
            }
            if self
                .focus_from
                .as_ref()
                .is_none_or(|(_, start, _)| done(start))
            {
                self.focus_from = None;
            }
        }
    }

    pub fn glide_scroll(&mut self) {
        let now = crate::clock::now();
        // An idle home draws no frames, so the gap since the last one can be
        // seconds: then the first step counts as one frame, and the first press
        // after a pause glides like every other instead of jumping to its
        // place. Every other step takes its real time, so a heavy frame on the
        // TV (40-50 ms while the grid rises) never slows the motion itself.
        let gap = now.duration_since(self.last_tick).as_secs_f32();
        let dt = if gap > 0.1 { 1. / 60. } else { gap };
        self.last_tick = now;
        if self.page != self.shown_page || self.preferences.reduced_motion {
            if self.page != self.shown_page {
                self.left = (self.shown_page, self.shown_scroll);
            }
            if self.page != self.shown_page && !self.preferences.reduced_motion {
                let direction = page_order(self.page) - page_order(self.shown_page);
                self.page_since = Some((now, direction.signum()));
                self.dirty = true;
            }
            self.shown_page = self.page;
            self.shown_scroll = self.scroll;
            self.shown_home_scroll = self.home_scroll;
            self.scroll_velocity = [0.; 2];
            // The switcher's cards jump to the focused one as well; left
            // behind, they never scroll and the frame never settles.
            if let (Some(switcher), Target::Switch(index)) = (&mut self.switcher, &self.focus) {
                switcher.shown = *index as f32;
            }
            return;
        }
        // The switcher's cards: a first-order approach, frame-rate
        // independent, never overshooting.
        let k = 1. - (-dt / 0.075).exp();
        if let (Some(switcher), Target::Switch(index)) = (&mut self.switcher, &self.focus) {
            let target = *index as f32;
            if (target - switcher.shown).abs() < 0.002 {
                switcher.shown = target;
            } else {
                switcher.shown += (target - switcher.shown) * k;
                self.dirty = true;
            }
        }
        // The page itself glides on a critically damped spring: it gathers
        // speed over the first frames and settles without overshoot. A
        // first-order approach (the cards' above) moved a fifth of the way on
        // the very first frame (138 px of a 690 px rise), which read as a
        // jump at the start of every scroll. Solved exactly for each frame's real time,
        // so a heavy TV frame neither slows nor destabilises it, and a new
        // target mid-glide carries on from the current speed.
        let decay = (-SCROLL_SPRING * dt).exp();
        let [velocity, home_velocity] = &mut self.scroll_velocity;
        for (shown, velocity, target) in [
            (&mut self.shown_scroll, velocity, self.scroll),
            (&mut self.shown_home_scroll, home_velocity, self.home_scroll),
        ] {
            let offset = *shown - target;
            let carried = *velocity + SCROLL_SPRING * offset;
            let offset = (offset + carried * dt) * decay;
            *velocity = (*velocity - SCROLL_SPRING * carried * dt) * decay;
            // Settled within a fifth of a pixel and barely moving: the last
            // step is no larger than the ones before it.
            if offset.abs() < 0.2 && velocity.abs() < 12. {
                *shown = target;
                *velocity = 0.;
            } else {
                *shown = target + offset;
                self.dirty = true;
            }
        }
    }

    pub fn focus(&mut self, target: Target, controls: &[Control]) {
        self.pointer = false;
        // The tvOS home has no tab bar: moving past the top edge stays put.
        if matches!(target, Target::Nav(_)) {
            return;
        }
        self.focus = target;
        self.reveal_focus(controls);
        if let Target::Sidebar(section) = self.focus {
            if self.subpage == Some(Subpage::WeatherLocation) {
                self.weather.cancel_search();
            }
            self.section = section;
            self.subpage = None;
        }
        self.dirty = true;
    }

    /// Remote focus is revealed after layout changes, including hide/reorder.
    /// Pointer paths deliberately do not call this method.
    pub fn reveal_focus(&mut self, controls: &[Control]) -> bool {
        let previous = self.scroll;
        let previous_home = self.home_scroll;
        // tvOS home: the dock row shows Top Shelf mode; any other row slides
        // the grid up so the dock sits at the top, scrolling further as needed.
        if self.page == Page::Apps
            && self.overlay.is_none()
            && let Target::App(id) = &self.focus
            && let Some(index) = self.apps().iter().position(|app| &app.id == id)
        {
            let row = index / APPS_COLUMNS;
            if row > 0 {
                self.expanded = false;
            }
            self.scroll = if row == 0 && self.expanded {
                -SHELF_DROP
            } else if row == 0 {
                if previous > 0. {
                    self.shelf_since = Some(crate::clock::now());
                }
                0.
            } else {
                let y = 90. + GRID_GAP + (row - 1) as f32 * ROW_PITCH;
                let extra = (self.scroll - GRID_SCROLL)
                    .max(0.)
                    .clamp((y + 210. - 1030.).max(0.), (y - 90.).max(0.));
                GRID_SCROLL + extra
            };
            if self.scroll != previous {
                self.dirty = true;
            }
            return self.scroll != previous;
        }
        if let Some(control) = controls.iter().find(|c| c.target == self.focus) {
            if self.page == Page::Home && matches!(self.focus, Target::App(_)) {
                if control.rect.x < HOME_STRIP_LEFT {
                    self.home_scroll += control.rect.x - HOME_STRIP_LEFT;
                } else if control.rect.x + control.rect.w > HOME_STRIP_RIGHT {
                    self.home_scroll += control.rect.x + control.rect.w - HOME_STRIP_RIGHT;
                }
            }
            if matches!(
                self.focus,
                Target::Nav(_) | Target::Search | Target::Sidebar(_)
            ) {
                self.scroll = 0.;
            } else if control.scrollable {
                let top = if self.page == Page::Apps {
                    DOCK_TOP
                } else {
                    120.8
                };
                if control.rect.y < top {
                    self.scroll = (self.scroll + control.rect.y - top).max(0.);
                }
                let bottom = if self.page == Page::Home && self.moving.is_none() {
                    1041.6
                } else {
                    982.4
                };
                if control.rect.y + control.rect.h > bottom {
                    self.scroll += control.rect.y + control.rect.h - bottom;
                }
            }
        }
        if self.page == Page::Home {
            self.scroll_favorites(0.);
        }
        if self.scroll != previous || self.home_scroll != previous_home {
            self.dirty = true;
        }
        self.scroll != previous || self.home_scroll != previous_home
    }

    /// Hover and pointer-down must not move the control before pointer-up.
    pub fn pointer_focus(&mut self, target: Target) {
        if !self.pointer {
            self.dirty = true;
        }
        self.pointer = true;
        if self.focus == target {
            return;
        }
        if let Target::Sidebar(section) = target {
            if self.subpage == Some(Subpage::WeatherLocation) {
                self.weather.cancel_search();
            }
            self.section = section;
            self.subpage = None;
        }
        self.focus = target;
        self.dirty = true;
    }
}
