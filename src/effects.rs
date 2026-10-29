use crate::{
    i18n,
    platform::{self, Platform, Request},
    scroll_preview,
    state::{Effect, State},
    storage, weather_service,
};

pub(crate) fn effects(
    effects: Vec<Effect>,
    state: &mut State,
    platform: &Platform,
    writer: &storage::Writer,
    weather: &weather_service::Service,
) -> bool {
    for effect in effects {
        match effect {
            Effect::Save => writer.save(&state.preferences),
            Effect::Refresh => {
                if !state.loading {
                    state.loading = true;
                    state.dirty = true;
                    platform.send(Request::Refresh);
                }
            }
            Effect::Running => {
                let cached = platform.running.lock().ok().and_then(|cache| cache.clone());
                match cached {
                    Some(ids) => state.open_switcher(platform::opened_only(ids, platform.webos)),
                    None => {
                        state.switcher_asked = true;
                        platform.send(Request::Running);
                    }
                }
            }
            Effect::Close(id) => platform.send(Request::Close(id)),
            Effect::TvHome(set) => platform.send(Request::TvHome(set)),
            Effect::Uninstall(id) => platform.send(Request::Uninstall(id)),
            Effect::NameInput(device, name, icon) => {
                platform.send(Request::NameInput(device, name, icon))
            }
            Effect::ClockFormat(hour_24) => crate::time_format::set(hour_24),
            Effect::Launch(id) => {
                if scroll_preview::is_placeholder(&id) {
                    state.notify(i18n::t("scrollTestOnly"));
                    continue;
                }
                // A second press while the first launch is on its way is the
                // same press twice: nothing more is asked or recorded.
                if state.launch_busy() {
                    continue;
                }
                state.note_launch(&id);
                // webOS plays its own native launch transition; Lounge adds
                // nothing over it (a second animation read as a flicker).
                if platform.webos {
                    state.launching = true;
                    state.launch_started = Some(crate::clock::now());
                    let title = state
                        .inventory
                        .apps
                        .iter()
                        .find(|a| a.id == id)
                        .map_or(id.clone(), |app| app.title.clone());
                    state.launch_waiting = Some((title, crate::clock::now(), 0));
                    platform.send(Request::Launch(id));
                } else {
                    let title = state
                        .inventory
                        .apps
                        .iter()
                        .find(|a| a.id == id)
                        .map_or(id.as_str(), |app| app.title.as_str());
                    state.notify(i18n::format("desktopPreview", &[("app", title)]));
                }
                // The recents list is written in the background, after the
                // launch request is on its way.
                writer.save(&state.preferences);
            }
            Effect::Quit => {
                save_order(state, writer);
                return false;
            }
            Effect::Weather(command) => weather.send(command),
        }
    }
    if state.moving.is_none() {
        save_order(state, writer);
    }
    true
}

/// Move commits its new order once, when it ends, as tvOS does: writing the
/// preferences on every arrow stalled input on the TV's busy flash storage.
pub(crate) fn save_order(state: &mut State, writer: &storage::Writer) {
    if std::mem::take(&mut state.unsaved_order) {
        writer.save(&state.preferences);
    }
}
