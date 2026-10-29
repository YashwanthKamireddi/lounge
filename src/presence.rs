//! Whether Lounge is on screen, and the moments that decide what an event
//! means: a window shown again is a return from an app (refill, re-read the
//! inventory), a Home launch well after Lounge came to the front is the
//! Home key pressed on Lounge itself, and only a press made on a Lounge that
//! has been drawn since it came to the front is the viewer's (`front`).
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
    /// When (SDL ticks, the clock SDL stamps input with) the first frame
    /// since Lounge last came to the front, or got focus back, was presented:
    /// what the viewer presses after that is what they can see.
    seen_at: Option<u32>,
    /// Counts every time Lounge went away or came back, so a press is tied
    /// to the stretch on screen it began in (see `front`).
    generation: u64,
}

impl Presence {
    pub fn new() -> Self {
        let now = crate::clock::now();
        Self {
            visible: true,
            away: false,
            shown_at: now,
            raising_until: now,
            seen_at: None,
            generation: 0,
        }
    }

    /// Whatever was on screen may no longer be what the viewer sees: input
    /// waits for the next presented frame.
    fn unseen(&mut self) {
        self.seen_at = None;
        self.generation += 1;
    }

    /// The window was hidden or minimised.
    pub fn hide(&mut self) {
        self.visible = false;
        self.away = true;
        self.unseen();
    }

    /// Focus left Lounge. Only an app Lounge is launching makes this a leave
    /// (some firmware sends no Hidden for it): a system panel over Lounge
    /// (LG's quick settings, input list) takes focus without hiding it, and
    /// closing the panel is not a return from an app.
    pub fn leave(&mut self, launching: bool) {
        self.away |= launching;
        // Whatever took focus (an app, the screen saver, a system panel)
        // has the remote now; a press reaching Lounge meanwhile was not
        // made on it.
        self.unseen();
    }

    /// The window was shown, restored or focused. One return fires all three
    /// events: true once, on the edge back from hidden or away.
    pub fn show(&mut self) -> bool {
        let returned = std::mem::take(&mut self.away) || !self.visible;
        self.visible = true;
        if returned {
            self.shown_at = crate::clock::now();
            self.unseen();
        }
        returned
    }

    /// A frame reached the screen at `ticks` (SDL ticks, once the swap returned).
    pub fn presented(&mut self, ticks: u32) {
        if self.visible && self.seen_at.is_none() {
            self.seen_at = Some(ticks);
        }
    }

    /// Some(stretch) for input SDL received at `ticks` (its event timestamp)
    /// while Lounge was in front and already drawn there: it then acts on
    /// what the viewer saw (and on the scene the loop keeps composed while in
    /// front). The event's own time decides, not when the loop reads it: the
    /// first swap after a return can block for half a second, and a key that
    /// went down during it was pressed on whatever was on screen before.
    /// A press keeps the stretch it began in and acts only if it ends in the
    /// same. Keys reach Lounge after a FocusLost only once it has focus
    /// again, which `show` sees.
    pub fn front(&self, ticks: u32) -> Option<u64> {
        // SDL ticks wrap after 49 days; Lounge as Home can run that long.
        let after = |seen: u32| ticks.wrapping_sub(seen) < u32::MAX / 2;
        (self.in_front() && self.seen_at.is_some_and(after)).then_some(self.generation)
    }

    /// On screen and not left for an app: what Lounge draws can be seen.
    /// Otherwise nothing is composed, decoded or uploaded (an app is in
    /// front; some firmware sends no Hidden for one Lounge launched).
    pub fn in_front(&self) -> bool {
        self.visible && !self.away
    }

    /// Something is bringing Lounge back (Home, sam's relaunch, a wake from
    /// standby): answer webOS quickly for a while. A press already under way
    /// belongs to what was in front before.
    pub fn raise(&mut self) {
        self.raising_until = crate::clock::now() + RAISING;
        self.unseen();
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

    #[test]
    fn input_counts_only_on_a_lounge_drawn_since_it_came_to_the_front() {
        let mut presence = Presence::new();
        assert_eq!(presence.front(10), None, "nothing drawn yet");
        presence.presented(100);
        let first = presence.front(100).expect("drawn and in front");
        assert_eq!(
            presence.front(99),
            None,
            "a key that went down before the frame was up, read after it"
        );
        // Every way of leaving or coming back waits for a new frame, and
        // starts a new stretch: a press from before never ends in it.
        for leave in [
            Presence::hide as fn(&mut Presence),
            |presence| presence.leave(false),
            |presence| presence.leave(true),
            Presence::raise,
        ] {
            leave(&mut presence);
            presence.show();
            assert_eq!(presence.front(200), None);
            presence.presented(300);
            assert_eq!(presence.front(250), None);
            assert!(presence.front(300).is_some_and(|stretch| stretch != first));
        }
        // SDL's ticks wrap after 49 days: a frame just before, a key just after.
        presence.hide();
        presence.show();
        presence.presented(u32::MAX - 5);
        assert!(presence.front(3).is_some());
        assert_eq!(presence.front(u32::MAX - 6), None);
        // Hidden, a frame (there is none) changes nothing.
        presence.hide();
        presence.presented(400);
        assert_eq!(presence.front(400), None);
    }
}
