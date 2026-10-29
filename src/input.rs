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
        state: &mut State,
        controls: &[Control],
    ) -> Vec<Effect> {
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

    /// OK released: a press that was neither held nor moved off activates.
    pub(crate) fn enter_up(&mut self, state: &mut State) -> Vec<Effect> {
        let mut pending = vec![];
        if let Some(held) = self.press.take()
            && !held.pointer
            && !held.held
            && held.target == state.focus
        {
            pending = state.activate();
            note_launches(&pending, "OK");
        }
        pending
    }
}

/// Records what made Lounge open an app, so an app opening unasked can be traced to its cause.
pub(crate) fn note_launches(effects: &[crate::state::Effect], cause: &str) {
    for effect in effects {
        if let crate::state::Effect::Launch(id) = effect {
            crate::platform::status(&format!("launch {id}: {cause}"));
        }
    }
}
