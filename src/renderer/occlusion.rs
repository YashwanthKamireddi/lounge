//! Ranked stencil occlusion: opaque tiles mask what lies behind them.
use super::*;

impl Renderer {
    /// Stencil bits of the bound framebuffer (0 when it has none).
    pub(super) fn stencil_bits(&self) -> i32 {
        let mut bits = 0;
        // SAFETY: plain state queries on the current context; any error they
        // raise (an enum this profile lacks) is cleared before returning.
        unsafe {
            if self.gles {
                // GL_STENCIL_BITS: GLES 2 has it; the core bindings do not.
                gl::GetIntegerv(0x0D57, &mut bits);
            } else {
                let mut bound = 0;
                gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut bound);
                let attachment = if bound == 0 {
                    gl::STENCIL
                } else {
                    gl::STENCIL_ATTACHMENT
                };
                let mut kind = gl::NONE as i32;
                gl::GetFramebufferAttachmentParameteriv(
                    gl::FRAMEBUFFER,
                    attachment,
                    gl::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE,
                    &mut kind,
                );
                if kind != gl::NONE as i32 {
                    gl::GetFramebufferAttachmentParameteriv(
                        gl::FRAMEBUFFER,
                        attachment,
                        gl::FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE,
                        &mut bits,
                    );
                }
            }
            while gl::GetError() != gl::NO_ERROR {}
        }
        bits.max(0)
    }

    /// Collects this frame's opaque tile interiors into `occluders`, in paint
    /// order, marking which draws they are in `occluder_at`.
    /// Only fills drawn fully opaque count: a tile's solid colour or its
    /// full-bleed artwork, inside any clip, never under an edge fade.
    pub(super) fn find_occluders(&mut self, commands: &[Draw]) {
        self.occluders.clear();
        self.occluder_at.clear();
        self.occluder_at.resize(commands.len(), false);
        if !self.stencil || self.skip.iter().any(|kind| kind == "occlusion") {
            return;
        }
        let (mut clip, mut fade) = (None::<Rect>, false);
        for (index, command) in commands.iter().enumerate() {
            let solid = match command {
                Draw::Clip(area) => {
                    clip = *area;
                    None
                }
                Draw::EdgeFade(area) => {
                    fade = area.is_some();
                    None
                }
                _ if fade => None,
                Draw::Shape(rect, radius, 0., color) if color[3] >= 0.999 => Some((*rect, *radius)),
                Draw::RoundedGradient(rect, radius, top, bottom)
                    if top[3] >= 0.999 && bottom[3] >= 0.999 =>
                {
                    Some((*rect, *radius))
                }
                // Glass at full strength is a solid picture of the backdrop.
                Draw::Glass(_, rect, radius, top, bottom, _)
                    if top[3] >= 0.999 && bottom[3] >= 0.999 =>
                {
                    Some((*rect, *radius))
                }
                // Tile artwork is full-bleed and opaque (only its corners are
                // cut), and so is any photo stretched over its whole rect.
                Draw::Image(path, rect, radius, false) if self.opaque_picture(path) => {
                    Some((*rect, *radius))
                }
                Draw::TintedImage(path, rect, radius, false, tint)
                    if tint[3] >= 0.999 && self.opaque_picture(path) =>
                {
                    Some((*rect, *radius))
                }
                _ => None,
            };
            let Some((rect, radius)) = solid else {
                continue;
            };
            // Inside a rounded rectangle, the rectangle inset by 0.3 of the
            // radius (plus the edge's blend) is entirely solid.
            let mut inner = rect.inset(radius.max(0.) * 0.3 + 2.);
            if let Some(clip) = clip {
                let (x, y) = (inner.x.max(clip.x), inner.y.max(clip.y));
                inner = Rect::new(
                    x,
                    y,
                    (inner.x + inner.w).min(clip.x + clip.w) - x,
                    (inner.y + inner.h).min(clip.y + clip.h) - y,
                );
            }
            // Small pieces save little and only cost a stencil write; the
            // stencil counts 254 tiles at most.
            if inner.w >= 48. && inner.h >= 48. && self.occluders.len() < 254 {
                self.occluders.push(inner);
                self.occluder_at[index] = true;
            }
        }
    }

    /// Artwork that covers every pixel of its rect: tile art, and JPEG
    /// photos (wallpaper previews, thumbnails, heroes; JPEG has no alpha).
    /// A photo counts only once it is decoded: an occluder whose picture
    /// failed to load would leave a hole down to the clear colour. On the
    /// Background page this lets the preview frame and the visible
    /// thumbnails mask their own shadows and the wallpaper beneath them.
    fn opaque_picture(&self, path: &str) -> bool {
        if path.contains("/tiles/") {
            return true;
        }
        if !path.ends_with(".jpg") {
            return false;
        }
        let path = self.artwork_variants.get(path).map_or(path, String::as_str);
        matches!(self.textures.get(path), Some(Some(_)))
    }
}
