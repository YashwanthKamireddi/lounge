//! A photo from the phone for the wallpaper or the profile picture: the QR code's overlay and
//! what the session reports (the server itself is `crate::companion`).
use super::*;
use crate::companion::{Event, Kind};

impl State {
    /// The gallery's wallpapers: the bundled ones, then the owner's photo. The tile after them
    /// sends a new photo from the phone.
    pub fn backgrounds(&self) -> Vec<&str> {
        BACKGROUNDS
            .iter()
            .map(|(id, _)| *id)
            .chain(self.photo_wallpaper.as_deref())
            .collect()
    }

    /// Picks the wallpaper `id`: the cross-fade starts once its pictures are on the GPU, from
    /// what shows now; a running fade finishes first.
    pub fn pick_background(&mut self, id: &str) {
        let fading = matches!(self.previous_background, Some((_, Some(_))));
        if self.preferences.background != id && !fading {
            let shown = self.wallpaper().to_owned();
            self.previous_background = Some((shown, None));
        }
        self.preferences.background = id.into();
        self.dirty = true;
    }

    /// Shows the QR code for a photo from the phone; the server starts with it.
    pub fn open_phone(&mut self, kind: Kind) -> Vec<Effect> {
        let restore = self.focus.clone();
        self.overlay = Some(Overlay::Phone { restore });
        self.focus = Target::Alert(0);
        self.phone = None;
        self.panel_started = (!self.preferences.reduced_motion).then(crate::clock::now);
        self.dirty = true;
        vec![Effect::Phone(Some(kind))]
    }

    /// The server is listening at `url`; its QR code is `modules` (`size` × `size`).
    pub fn phone_ready(&mut self, kind: Kind, url: String, size: usize, modules: Vec<bool>) {
        if matches!(self.overlay, Some(Overlay::Phone { .. })) {
            self.phone = Some(Phone {
                kind,
                url,
                size,
                modules,
                step: PhoneStep::Waiting,
            });
            self.dirty = true;
        }
    }

    /// The server could not start (`reason`, a message key): said, and the code closes.
    pub fn phone_unavailable(&mut self, reason: &'static str) {
        let _ = self.close_phone();
        self.notify(i18n::t(reason));
    }

    /// Closes the QR code, if it is open; the server stops with it.
    pub fn close_phone(&mut self) -> Vec<Effect> {
        let Some(Overlay::Phone { restore }) = self.overlay.take() else {
            return vec![];
        };
        self.focus = restore;
        self.phone = None;
        self.panel_started = None;
        self.dirty = true;
        vec![Effect::Phone(None)]
    }

    /// What the phone's session reports. A saved profile picture shows once the loop has read
    /// the avatars folder again.
    pub fn phone_event(&mut self, event: Event) -> Vec<Effect> {
        let Some(phone) = &mut self.phone else {
            return vec![];
        };
        self.dirty = true;
        match event {
            Event::Opened => {
                if phone.step == PhoneStep::Waiting {
                    phone.step = PhoneStep::Opened;
                }
                vec![]
            }
            Event::Receiving => {
                phone.step = PhoneStep::Receiving;
                vec![]
            }
            Event::Failed(reason) => {
                phone.step = PhoneStep::Failed(match reason.as_str() {
                    "uploadStopped" => "phoneUploadStopped",
                    _ => "phoneNotAPhoto",
                });
                vec![]
            }
            Event::Saved(Kind::Wallpaper, id) => {
                self.photo_wallpaper = Some(id.clone());
                self.pick_background(&id);
                let restore = self
                    .backgrounds()
                    .iter()
                    .position(|background| *background == id)
                    .map(Target::Background);
                let mut effects = self.close_phone();
                if let Some(restore) = restore
                    && self.page == Page::Settings
                    && self.section == 1
                {
                    self.focus = restore;
                }
                self.notify(i18n::t("phoneWallpaperSet"));
                effects.push(Effect::Save);
                effects
            }
            Event::Saved(Kind::Avatar, file) => {
                self.preferences.avatar = Some(file);
                let mut effects = self.close_phone();
                self.notify(i18n::t("phoneAvatarSet"));
                effects.push(Effect::Save);
                effects
            }
        }
    }
}
