//! Replies from the platform worker (the TV's services) and what each one
//! changes: the inventory, the profile, closes and launches, and the
//! foreground poll that brings a hidden Lounge back over LG Home.
use crate::{
    i18n,
    platform::{self, Platform, Request, Response},
    scroll_preview,
    state::{self, State, Target},
    view,
};
use std::time::{Duration, Instant};

/// The loop flags replies drive.
pub struct Replies {
    /// Every app's icon has been asked for (once the first frame is up).
    pub prefetched: bool,
    /// Focus still waits for the first inventory to land on content.
    pub initial_focus: bool,
    /// When to ask for the inventory again after a failed load.
    pub inventory_retry: Option<Instant>,
    /// A foreground poll is out (polling only when push events are off).
    pub foreground_pending: bool,
    /// When Lounge last raised itself over LG Home from a poll.
    pub last_redirect: Option<Instant>,
}

impl Replies {
    pub fn new(initial_focus: bool) -> Self {
        Self {
            prefetched: false,
            initial_focus,
            inventory_retry: None,
            foreground_pending: false,
            last_redirect: None,
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
                if self.initial_focus
                    && state.focus == Target::Nav(0)
                    && state.page == state::Page::Apps
                {
                    state.focus = state.first_content();
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
            // opened an app) must not open the switcher behind it.
            Response::Running(ids) if visible && state.overlay.is_none() => {
                state.open_switcher(platform::opened_only(ids, platform.webos))
            }
            Response::Running(_) => {}
            Response::Home(result) => {
                let asked = std::mem::take(&mut state.home_button_busy);
                match result {
                    Ok(on) => {
                        if asked {
                            state.notify(i18n::t(if on {
                                "homeButtonLounge"
                            } else {
                                "homeButtonLg"
                            }));
                        }
                        state.home_button = Some(on);
                    }
                    Err(error) => {
                        eprintln!("Lounge Home button: {error}");
                        if asked {
                            state.notify(i18n::t("homeButtonError"));
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
            Response::Closed(id, result) => {
                if let Err(error) = result {
                    eprintln!("Lounge close {id}: {error}");
                    state.notify(i18n::t("appCloseError"));
                }
            }
            Response::Foreground(id) => {
                self.foreground_pending = false;
                if !visible
                    && id.as_deref() == Some(platform::HOME_ID)
                    && self
                        .last_redirect
                        .is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
                {
                    self.last_redirect = Some(Instant::now());
                    platform.send(Request::Launch(platform::APP_ID.into()));
                }
            }
            Response::Launched(result) => {
                state.launching = false;
                match result {
                    Err(error) => {
                        eprintln!("{error}");
                        state.notify(i18n::t("appOpenError"));
                    }
                    Ok(()) => {
                        state.notice = None;
                        state.dirty = true;
                    }
                }
            }
        }
    }
}
