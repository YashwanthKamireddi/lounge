use crate::state::{self, State, Target};
use sdl2::keyboard::Keycode;
use std::time::{Duration, Instant};

pub(crate) struct Press {
    pub(crate) target: Target,
    pub(crate) start: Instant,
    pub(crate) held: bool,
    pub(crate) pointer: bool,
}

/// Fastest pace of a held arrow key: about nine moves a second.
pub(crate) const HELD_MOVE: Duration = Duration::from_millis(110);
// SDL-webOS's Back scancode is outside rust-sdl2's standard Scancode enum.
// Keycode retains the raw value: SDL_SCANCODE_TO_KEYCODE(SDL_SCANCODE_WEBOS_BACK).
pub(crate) const WEBOS_BACK_KEYCODE: i32 = (1 << 30) | 482;
/// LG's remote keys as SDL-webOS delivers them: the xkbcommon webOS keysym
/// (xkbcommon-webos-keysyms.h) less its 0x0100_0000 flag. Back was measured
/// on this TV's Magic Remote (0x200003 == XKB_KEY_WEBOS_BACK).
pub(crate) const WEBOS_KEY_EXIT: i32 = 0x0020_0001;
pub(crate) const WEBOS_KEY_BACK: i32 = 0x0020_0003;
pub(crate) const WEBOS_KEY_RECENT: i32 = 0x0020_0005;

/// Remote number keys 1-9 open the matching Home favourite (Quick Access).
pub(crate) fn quick_slot(key: Keycode) -> Option<usize> {
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
