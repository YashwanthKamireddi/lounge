use crate::{
    sdl_events,
    state::{self, Control, Direction, Effect, Overlay, State, Target},
};
use sdl2::keyboard::Keycode;
use std::time::{Duration, Instant};

pub(crate) struct Press {
    pub(crate) target: Target,
    pub(crate) start: Instant,
    pub(crate) held: bool,
    pub(crate) pointer: bool,
    /// The stretch on screen it began in (`Presence::front`): it acts only if
    /// it ends in the same one.
    pub(crate) front: u64,
}

/// Fastest pace of a held arrow key: about nine moves a second.
const HELD_MOVE: Duration = Duration::from_millis(110);
// SDL-webOS's Back scancode is outside rust-sdl2's standard Scancode enum.
// Keycode retains the raw value: SDL_SCANCODE_TO_KEYCODE(SDL_SCANCODE_WEBOS_BACK).
const WEBOS_BACK_KEYCODE: i32 = (1 << 30) | 482;
/// LG's remote keys as SDL-webOS delivers them: the xkbcommon webOS keysym
/// (xkbcommon-webos-keysyms.h) less its 0x0100_0000 flag. Back was measured
/// on this TV's Magic Remote (0x200003 == XKB_KEY_WEBOS_BACK).
const WEBOS_KEY_EXIT: i32 = 0x0020_0001;
const WEBOS_KEY_BACK: i32 = 0x0020_0003;
const WEBOS_KEY_RECENT: i32 = 0x0020_0005;

/// Remote number keys 1-9 open the matching Home favourite (Quick Access).
fn quick_slot(key: Keycode) -> Option<usize> {
    Some(match key {
        Keycode::Num1 | Keycode::Kp1 => 0,
        Keycode::Num2 | Keycode::Kp2 => 1,
        Keycode::Num3 | Keycode::Kp3 => 2,
        Keycode::Num4 | Keycode::Kp4 => 3,
        Keycode::Num5 | Keycode::Kp5 => 4,
        Keycode::Num6 | Keycode::Kp6 => 5,
        Keycode::Num7 | Keycode::Kp7 => 6,
        Keycode::Num8 | Keycode::Kp8 => 7,
        Keycode::Num9 | Keycode::Kp9 => 8,
        _ => return None,
    })
}

pub(crate) fn pointer_position(window: &sdl2::video::Window, x: i32, y: i32) -> Option<(f32, f32)> {
    let (width, height) = window.size();
    let scale = (width as f32 / 1920.).min(height as f32 / 1080.);
    if scale <= 0. {
        return None;
    }
    Some((
        (x as f32 - (width as f32 - 1920. * scale) / 2.) / scale,
        (y as f32 - (height as f32 - 1080. * scale) / 2.) / scale,
    ))
}

pub(crate) fn hit(
    controls: &[state::Control],
    window: &sdl2::video::Window,
    x: i32,
    y: i32,
    state: &State,
) -> Option<Target> {
    let (x, y) = pointer_position(window, x, y)?;
    controls
        .iter()
        .rev()
        .find(|control| {
            control.rect.contains(x, y)
                && (!control.scrollable
                    || state.page == state::Page::Apps
                    || ((64. ..1856.).contains(&x)
                        && y >= 108.8
                        && y < if state.page == state::Page::Home && state.moving.is_none() {
                            1020.
                        } else {
                            968.
                        }))
        })
        .map(|control| control.target.clone())
}

/// The remote, as Lounge reads it: what each key does, an OK held for the menu,
/// and a held arrow moving at an even pace.
///
/// `front` is `Presence::front`: None while Lounge is not on screen or not yet
/// drawn since it came to the front. Input then does nothing at all (a key let
/// go on Lounge may have gone down on an app, the screen saver or a TV waking
/// up), and a press acts only when it began and ended in the same stretch.
pub(crate) struct Remote {
    /// OK (or a click) held down on a target, until released or cancelled.
    pub(crate) press: Option<Press>,
    /// When a held arrow last moved focus.
    last_move: Instant,
}

impl Remote {
    pub(crate) fn new() -> Self {
        Self {
            press: None,
            last_move: crate::clock::now(),
        }
    }

    /// A key went down: the effects it asks for.
    pub(crate) fn key_down(
        &mut self,
        key: Keycode,
        repeat: bool,
        webos: bool,
        front: Option<u64>,
        state: &mut State,
        controls: &[Control],
    ) -> Vec<Effect> {
        // Whatever is pressed next supersedes a switcher still being asked for.
        state.switcher_asked = false;
        let Some(front) = front else {
            self.press = None;
            // Drawn again at once, so the next press counts.
            state.dirty = true;
            if !repeat
                && (matches!(key, Keycode::Return | Keycode::KpEnter) || quick_slot(key).is_some())
            {
                crate::platform::status(&format!("{key} before Lounge was on screen: ignored"));
            }
            return vec![];
        };
        let mut pending = vec![];
        match key {
            // A held arrow moves at a steady cadence: repeats that
            // arrive faster are dropped, so focus glides evenly instead
            // of jumping when two land in one frame.
            Keycode::Up | Keycode::Down | Keycode::Left | Keycode::Right
                if repeat && crate::clock::since(self.last_move) < HELD_MOVE => {}
            Keycode::Up | Keycode::Down | Keycode::Left | Keycode::Right => {
                self.last_move = crate::clock::now();
                let direction = match key {
                    Keycode::Up => Direction::Up,
                    Keycode::Down => Direction::Down,
                    Keycode::Left => Direction::Left,
                    _ => Direction::Right,
                };
                pending = state.direction(direction, controls);
            }
            Keycode::Return | Keycode::KpEnter
                if matches!(state.overlay, Some(Overlay::SystemKeyboard)) =>
            {
                self.press = None;
                state.finish_text_input();
            }
            Keycode::Return | Keycode::KpEnter if !repeat => {
                self.press = Some(Press {
                    target: state.focus.clone(),
                    start: crate::clock::now(),
                    held: false,
                    pointer: false,
                    front,
                });
            }
            Keycode::Escape | Keycode::AcBack if !repeat => {
                self.press = None;
                pending = state.back();
            }
            Keycode::Backspace if state.text_input_active() => state.delete(),
            Keycode::Backspace if !repeat => {
                self.press = None;
                pending = state.back();
            }
            Keycode::Clear if matches!(state.overlay, Some(Overlay::SystemKeyboard)) => {
                let length = state.editing_text().len() as i32;
                state.delete_surrounding_text(-length, length);
            }
            Keycode::M | Keycode::Menu if !repeat && state.overlay.is_none() => {
                self.press = None;
                state.options();
            }
            Keycode::F5 if !repeat => pending.push(Effect::Refresh),
            key if !repeat
                && state.overlay.is_none()
                && state.switcher.is_none()
                && state.moving.is_none()
                && !state.text_input_active()
                && quick_slot(key).is_some() =>
            {
                self.press = None;
                pending = state.quick_launch(quick_slot(key).unwrap_or(0));
                note_launches(&pending, &format!("number key {key}"));
            }
            key if !repeat && webos && i32::from(key) == WEBOS_KEY_RECENT => {
                self.press = None;
                pending = state.recent();
            }
            key if !repeat
                && (i32::from(key) == 461
                    || (webos
                        && [
                            WEBOS_BACK_KEYCODE,
                            WEBOS_KEY_BACK,
                            WEBOS_KEY_EXIT,
                            sdl_events::WEBOS_EXIT_KEYCODE,
                        ]
                        .contains(&i32::from(key)))) =>
            {
                self.press = None;
                pending = state.back();
            }
            _ => {}
        }
        pending
    }

    /// A key came up: OK released activates a press that was neither held
    /// nor moved off, and began on this same stretch on screen.
    pub(crate) fn key_up(
        &mut self,
        key: Keycode,
        front: Option<u64>,
        state: &mut State,
    ) -> Vec<Effect> {
        if !matches!(key, Keycode::Return | Keycode::KpEnter) {
            return vec![];
        }
        let mut pending = vec![];
        if let Some(held) = self.press.take()
            && !held.pointer
            && !held.held
            && front == Some(held.front)
            && held.target == state.focus
        {
            pending = state.activate();
            note_launches(&pending, &held.describe("OK"));
        }
        pending
    }

    /// The pointer moved over `hovered` (None: nothing there).
    pub(crate) fn pointer_moved(
        &mut self,
        hovered: Option<Target>,
        front: Option<u64>,
        state: &mut State,
    ) {
        if front.is_none() || state.moving.is_some() {
            return;
        }
        if matches!(state.overlay, Some(Overlay::SystemKeyboard)) {
            return;
        }
        if let Some(target) = hovered.clone() {
            state.pointer_focus(target);
        }
        if self
            .press
            .as_ref()
            .is_some_and(|press| press.pointer && hovered.as_ref() != Some(&press.target))
        {
            self.press = None;
        }
    }

    /// A pointer button went down over `target`.
    pub(crate) fn pointer_down(
        &mut self,
        target: Option<Target>,
        right: bool,
        front: Option<u64>,
        state: &mut State,
    ) {
        state.switcher_asked = false;
        let Some(front) = front else {
            self.press = None;
            state.dirty = true;
            crate::platform::status("click before Lounge was on screen: ignored");
            return;
        };
        match target {
            Some(target) if state.moving.is_none() => {
                if matches!(state.overlay, Some(Overlay::SystemKeyboard)) {
                    state.finish_text_input();
                }
                // Pointer-down must not normalize scroll and move the clicked control.
                state.pointer_focus(target);
                self.press = None;
                if right {
                    state.options();
                } else {
                    self.press = Some(Press {
                        target: state.focus.clone(),
                        start: crate::clock::now(),
                        held: false,
                        pointer: true,
                        front,
                    });
                }
            }
            _ => self.press = None,
        }
    }

    /// The left button came up over `target`; `outside_modal`: the pointer is
    /// outside an open alert or menu, which a click there dismisses.
    pub(crate) fn pointer_up(
        &mut self,
        target: Option<Target>,
        outside_modal: bool,
        front: Option<u64>,
        state: &mut State,
    ) -> Vec<Effect> {
        let Some(front) = front else {
            self.press = None;
            return vec![];
        };
        match self.press.take() {
            Some(held)
                if held.pointer
                    && !held.held
                    && held.front == front
                    && target.as_ref() == Some(&held.target)
                    && held.target == state.focus =>
            {
                let pending = state.activate();
                note_launches(&pending, &held.describe("pointer click"));
                pending
            }
            None if outside_modal => state.back(),
            _ => vec![],
        }
    }

    /// Called every loop pass: a press still down after `HOLD` becomes a hold
    /// (the menu, Move, or OK for a row). Only within its stretch on screen.
    pub(crate) fn hold_due(&mut self, front: Option<u64>, state: &mut State) -> Vec<Effect> {
        let Some(held) = &mut self.press else {
            return vec![];
        };
        if front != Some(held.front) {
            self.press = None;
            return vec![];
        }
        if held.held || crate::clock::since(held.start) < HOLD {
            return vec![];
        }
        held.held = true;
        let cause = held.describe("OK held");
        state.pressed = None;
        let pending = state.hold();
        note_launches(&pending, &cause);
        pending
    }

    /// After each event: a press whose target lost focus is over, and the
    /// pressed look follows the press.
    pub(crate) fn settle(&mut self, state: &mut State) {
        if self
            .press
            .as_ref()
            .is_some_and(|press| !press.held && press.target != state.focus)
        {
            self.press = None;
        }
        let pressed = self
            .press
            .as_ref()
            .filter(|press| !press.held)
            .map(|press| press.target.clone());
        if state.pressed != pressed {
            state.pressed = pressed;
            state.dirty = true;
        }
    }
}

impl Press {
    /// What a launch from this press is put down to in the status log.
    fn describe(&self, how: &str) -> String {
        format!(
            "{how} on {:?}, down {} ms",
            self.target,
            crate::clock::since(self.start).as_millis()
        )
    }
}

/// How long OK (or a click) stays down before it is a hold.
pub(crate) const HOLD: Duration = Duration::from_millis(500);

/// Records what made Lounge open an app, so an app opening unasked can be traced to its cause.
pub(crate) fn note_launches(effects: &[crate::state::Effect], cause: &str) {
    for effect in effects {
        if let crate::state::Effect::Launch(id) = effect {
            crate::platform::status(&format!("launch {id}: {cause}"));
        }
    }
}

#[cfg(test)]
#[path = "input-tests.rs"]
mod tests;
