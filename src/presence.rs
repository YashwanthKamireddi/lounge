//! Whether Lounge is on screen, and the moments that decide what an event
//! means: a window shown again is a return from an app (refill, re-read the
//! inventory), and a Home launch well after Lounge came to the front is the
//! Home key pressed on Lounge itself.
use std::time::{Duration, Instant};

/// How long after coming to the front a Home launch still belongs to that
/// return rather than being Home pressed on Lounge.
const SETTLE: Duration = Duration::from_millis(800);
/// How long after a raise was asked for the hidden loop answers quickly.
const RAISING: Duration = Duration::from_secs(2);

pub struct Presence {
    /// On screen (not hidden or minimised).
    pub visible: bool,
    /// Left since last shown (hidden, or focus went to an app).
    away: bool,
    /// When Lounge last came back to the front.
    shown_at: Instant,
    /// Until when a raise is in progress (see `raising`).
    raising_until: Instant,
}

impl Presence {
    pub fn new() -> Self {
        let now = crate::clock::now();
        Self {
            visible: true,
            away: false,
            shown_at: now,
            raising_until: now,
        }
    }

    /// The window was hidden or minimised.
    pub fn hide(&mut self) {
        self.visible = false;
        self.away = true;
    }

    /// Focus left Lounge. Only an app Lounge is launching makes this a leave
    /// (some firmware sends no Hidden for it): a system panel over Lounge
    /// (LG's quick settings, input list) takes focus without hiding it, and
    /// closing the panel is not a return from an app.
    pub fn leave(&mut self, launching: bool) {
        self.away |= launching;
    }

    /// The window was shown, restored or focused. One return fires all three
    /// events: true once, on the edge back from hidden or away.
    pub fn show(&mut self) -> bool {
        let returned = std::mem::take(&mut self.away) || !self.visible;
        self.visible = true;
        if returned {
            self.shown_at = crate::clock::now();
        }
        returned
    }

    /// On screen and not left for an app: what Lounge draws can be seen.
    /// Otherwise nothing is composed, decoded or uploaded (an app is in
    /// front; some firmware sends no Hidden for one Lounge launched).
    pub fn in_front(&self) -> bool {
        self.visible && !self.away
    }

    /// Something is bringing Lounge back: answer webOS quickly for a while.
    pub fn raise(&mut self) {
        self.raising_until = crate::clock::now() + RAISING;
    }

    pub fn raising(&self) -> bool {
        crate::clock::now() < self.raising_until
    }

    /// On screen long enough that a Home launch is the Home key pressed on
    /// Lounge, not the launch that brought it back.
    pub fn settled(&self) -> bool {
        self.visible && crate::clock::since(self.shown_at) > SETTLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_return_is_counted_once_and_settles_after_a_moment() {
        let mut presence = Presence::new();
        // Shown, Restored and FocusGained all arrive for one return.
        presence.hide();
        assert!(!presence.visible);
        assert!(presence.show(), "back from hidden: a return");
        assert!(
            !presence.show(),
            "the same return's next event is not another"
        );
        assert!(
            !presence.settled(),
            "just back: a Home launch now belongs to the return"
        );
        std::thread::sleep(SETTLE + Duration::from_millis(20));
        assert!(
            presence.settled(),
            "in front a while: Home is pressed on Lounge"
        );
        // Focus lost to a system panel (verified on the TV with quick
        // settings: FocusLost, FocusGained, never Hidden): not a return.
        presence.leave(false);
        assert!(!presence.show(), "a closed system panel is not a return");
        assert!(presence.settled(), "and Home stays Home pressed on Lounge");
        // Focus lost to an app Lounge launched (no Hidden on some firmware).
        presence.leave(true);
        assert!(presence.show());
        // Raising answers quickly only for a while.
        assert!(presence.in_front());
        assert!(!presence.raising());
        presence.raise();
        assert!(presence.raising());
    }

    #[test]
    fn only_a_window_in_front_does_any_work() {
        let mut presence = Presence::new();
        assert!(presence.in_front(), "a new Lounge is on screen");
        // Hidden behind an app.
        presence.hide();
        assert!(!presence.in_front());
        presence.show();
        assert!(presence.in_front(), "back: draws again at once");
        // An app Lounge launched took focus, with no Hidden (some firmware):
        // still nominally visible, but behind the app.
        presence.leave(true);
        assert!(presence.visible && !presence.in_front());
        presence.show();
        assert!(presence.in_front());
        // A system panel over Lounge leaves it in front: the home still shows
        // around the panel.
        presence.leave(false);
        assert!(presence.in_front());
    }
}
