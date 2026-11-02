//! Replies from the platform worker (the TV's services) and what each one
//! changes: the inventory, the profile, closes, launches and Lounge as Home.
use crate::{
    i18n,
    platform::{self, Platform, Response},
    scroll_preview,
    state::{self, State, Target},
    view,
};
use std::time::Instant;

/// The loop flags replies drive.
pub struct Replies {
    /// Every app's icon has been asked for (once the first frame is up).
    pub prefetched: bool,
    /// Focus still waits for the first inventory to land on content.
    pub initial_focus: bool,
    /// When to ask for the inventory again after a failed load.
    pub inventory_retry: Option<Instant>,
}

impl Replies {
    pub fn new(initial_focus: bool) -> Self {
        Self {
            prefetched: false,
            initial_focus,
            inventory_retry: None,
        }
    }

    /// Applies one reply. `visible`: Lounge is on screen.
    pub fn apply(
        &mut self,
        response: Response,
        state: &mut State,
        platform: &Platform,
        assets: &view::Assets,
        visible: bool,
        test_extra_apps: usize,
    ) {
        match response {
            Response::Inventory(Ok((mut inventory, warning))) => {
                assets.artwork.refresh();
                // Every app's icon is fetched in the background once the
                // first frame is up (see below), never competing with it.
                self.prefetched = false;
                scroll_preview::append(&mut inventory, test_extra_apps);
                state.set_inventory(inventory, warning);
                if self.initial_focus && state.page == state::Page::Apps {
                    let first = state.first_content();
                    if state.focus == Target::Nav(0) {
                        state.focus = first;
                    } else if let Some(state::Overlay::OfferHome { restore }) = &mut state.overlay
                        && *restore == Target::Nav(0)
                    {
                        // The Make Home question came first: closing it lands on the first app.
                        *restore = first;
                    }
                }
                self.initial_focus = false;
            }
            Response::Profile(profile) => {
                if state.profile != profile {
                    state.profile = profile;
                    state.dirty = true;
                }
            }
            Response::Inventory(Err(error)) => {
                if let Some(delay) = state.inventory_failed(error) {
                    eprintln!(
                        "Lounge TV inventory: retry {} in {} s",
                        state.inventory_failures,
                        delay.as_secs()
                    );
                    self.inventory_retry = Some(Instant::now() + delay);
                } else if self.initial_focus {
                    state.focus = state.first_content();
                    self.initial_focus = false;
                }
            }
            // A late reply (the bus was slow and the owner has since
            // pressed something else, or opened an app) must not open the
            // switcher over what they moved on to.
            Response::Running(ids)
                if visible
                    && std::mem::take(&mut state.switcher_asked)
                    && state.overlay.is_none() =>
            {
                state.open_switcher(platform::opened_only(ids, platform.webos))
            }
            Response::Running(_) => {}
            Response::TvHome(result) => {
                state.tv_home_busy = false;
                // Running as the TV's Home is proof of it, whatever a reply says: the switch
                // must then always be there to turn it off.
                let home_now = crate::platform::is_home();
                match result {
                    Ok(on) => {
                        let on = on || home_now;
                        let wanted = state.tv_home_wanted.take();
                        state.tv_home_rechecking = false;
                        if let Some(wanted) = wanted {
                            state.notify(i18n::t(match (on == wanted, on) {
                                (true, true) => "tvHomeLounge",
                                (true, false) => "tvHomeLg",
                                (false, true) => "tvHomeStuck",
                                (false, false) => "tvHomeError",
                            }));
                        }
                        state.tv_home = Some(on);
                        state.tv_home_blocked = None;
                        // A TV that can have Lounge as its Home, and does not yet: asked once,
                        // once the screen has settled (see `State::offer_home`).
                        state.home_offer_due =
                            wanted.is_none() && !on && !state.preferences.home_offered;
                    }
                    Err(error) => {
                        eprintln!("Lounge as Home: {error}");
                        let block = state::home_block(&error);
                        if state.tv_home_wanted.is_some() && !state.tv_home_rechecking {
                            // The change may well have happened (the reply can be lost while
                            // Home switches over): ask the TV how things stand before saying.
                            state.tv_home_rechecking = true;
                            state.tv_home_busy = true;
                            state.queued.push(state::Effect::TvHome(None));
                        } else {
                            // Said when asked: after a change, or the row pressed while the
                            // TV could not be read.
                            let wanted = state.tv_home_wanted.take();
                            if std::mem::take(&mut state.tv_home_rechecking) {
                                state.notify(i18n::t(if home_now && wanted == Some(false) {
                                    "tvHomeStuck"
                                } else {
                                    block
                                }));
                            }
                            if home_now {
                                state.tv_home = Some(true);
                                state.tv_home_blocked = None;
                            } else if state.tv_home.is_none() {
                                state.tv_home_blocked = Some(block);
                            }
                        }
                    }
                }
                state.dirty = true;
            }
            // What the removal asks for next (save, re-read the apps) runs
            // with the loop's own effects.
            Response::Uninstalled(id, result) => {
                let pending = state.uninstalled(&id, result);
                state.queued.extend(pending);
            }
            // A name the TV refused: read its inputs again, so the panel shows LG's own.
            Response::InputNamed(Err(error)) => {
                eprintln!("Lounge input name: {error}");
                state.queued.push(state::Effect::Refresh);
            }
            Response::InputNamed(Ok(())) => {}
            Response::Closed(id, result) => {
                if let Err(error) = result {
                    eprintln!("Lounge close {id}: {error}");
                    state.notify(i18n::t("appCloseError"));
                }
            }
            Response::Launched(result) => {
                state.launching = false;
                match result {
                    Err(error) => {
                        eprintln!("{error}");
                        state.launch_waiting = None;
                        state.notify(i18n::t("appOpenError"));
                    }
                    Ok(()) => {
                        // webOS took the request; the app may still be on its way, and then
                        // "Opening …" stays until it arrives (State::launch_arrived).
                        if state
                            .launch_waiting
                            .as_ref()
                            .is_none_or(|(_, _, said)| *said == 0)
                        {
                            state.notice = None;
                        }
                        state.dirty = true;
                    }
                }
            }
        }
    }
}
