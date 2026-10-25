use crate::state::{self, Direction, Effect, State};
use sdl2::{event::Event, mouse::MouseButton};

pub(crate) const SCRIPT_POINTER: u32 = u32::MAX - 1;

pub(crate) fn script(
    action: &str,
    state: &mut State,
    controls: &[state::Control],
) -> Result<Vec<Effect>, String> {
    Ok(match action {
        "home" => {
            state.open(0);
            vec![]
        }
        "apps" => {
            state.open(1);
            vec![]
        }
        "settings" => {
            state.open(2);
            vec![]
        }
        "up" => state.direction(Direction::Up, controls),
        "down" => state.direction(Direction::Down, controls),
        "left" => state.direction(Direction::Left, controls),
        "right" => state.direction(Direction::Right, controls),
        "enter" => state.activate(),
        "back" => state.back(),
        "menu" => {
            state.options();
            vec![]
        }
        "hold" => {
            state.hold();
            vec![]
        }
        "wait" => vec![],
        action if action.starts_with("type:") => {
            let field = if state.subpage == Some(state::Subpage::WeatherLocation) {
                state::TextField::Location
            } else {
                state::TextField::Apps
            };
            state.open_keyboard(field);
            state.text(&action[5..]);
            vec![]
        }
        "await-search" => vec![],
        "await-weather" => vec![],
        _ => return Err(format!("Unknown scripted action: {action}")),
    })
}

pub(crate) fn scripted_click(
    sdl: &sdl2::Sdl,
    window: &sdl2::video::Window,
    position: &str,
) -> Result<(), String> {
    let coordinates: Vec<f32> = position
        .split(':')
        .map(|value| value.parse::<f32>().map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let [x, y] = coordinates.as_slice() else {
        return Err("click: expects logical x:y coordinates".into());
    };
    if !x.is_finite() || !y.is_finite() {
        return Err("Pointer coordinates must be finite".into());
    }
    let (w, h) = window.size();
    let scale = (w as f32 / 1920.).min(h as f32 / 1080.);
    let x = (x * scale + (w as f32 - 1920. * scale) / 2.).round() as i32;
    let y = (y * scale + (h as f32 - 1080. * scale) / 2.).round() as i32;
    let events = sdl.event()?;
    for event in [
        Event::MouseButtonDown {
            timestamp: 0,
            window_id: window.id(),
            which: SCRIPT_POINTER,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x,
            y,
        },
        Event::MouseButtonUp {
            timestamp: 0,
            window_id: window.id(),
            which: SCRIPT_POINTER,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x,
            y,
        },
    ] {
        events.push_event(event)?;
    }
    Ok(())
}
