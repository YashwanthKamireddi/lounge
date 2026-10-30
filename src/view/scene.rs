//! The draw list a frame is built into, and its drawing primitives.
use super::{Assets, shared::c};
use crate::{
    renderer::{Color, Draw, TextStyle},
    state::{Control, Rect, Target},
};

pub struct Scene {
    pub draw: Vec<Draw>,
    pub controls: Vec<Control>,
    pub modal: Option<Rect>,
    /// The Settings page's own draws, kept so the next level can cross-fade
    /// out of exactly what was on screen.
    pub settings_range: Option<std::ops::Range<usize>>,
    /// The page shown and its draws (all but the background and overlays),
    /// kept so the next page can cross-fade out of exactly what was on screen.
    pub page: Option<(crate::state::Page, std::ops::Range<usize>)>,
    /// The search page's draws while it shows, kept so closing it fades out
    /// exactly what was on screen.
    pub search: Option<std::ops::Range<usize>>,
    /// This frame's appearance (Light, Dark, or between while it changes).
    pub(super) theme: super::theme::Theme,
    /// The draws of the tile a context menu acts on: dimming leaves them
    /// lit, so that app stands out while the rest of the screen falls back.
    pub(super) lit: Option<std::ops::Range<usize>>,
}
impl Scene {
    pub(super) fn new(theme: super::theme::Theme) -> Self {
        Self {
            theme,
            draw: Vec::new(),
            controls: Vec::new(),
            modal: None,
            settings_range: None,
            page: None,
            search: None,
            lit: None,
        }
    }
    pub(super) fn shape(&mut self, rect: Rect, radius: f32, fill: Color) {
        self.draw.push(Draw::Shape(rect, radius, 0., fill));
    }
    pub(super) fn border(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        self.draw.push(Draw::Shape(rect, radius, width, color));
    }
    pub(super) fn text(
        &mut self,
        text: impl Into<String>,
        rect: Rect,
        style: TextStyle,
        color: Color,
        center: bool,
    ) {
        self.draw
            .push(Draw::Text(text.into(), rect, style, color, center));
    }
    pub(super) fn image(&mut self, path: String, rect: Rect, radius: f32, contain: bool) {
        self.draw.push(Draw::Image(path, rect, radius, contain));
    }
    pub(super) fn icon(&mut self, assets: &Assets, name: &str, rect: Rect, color: Color) {
        self.draw.push(Draw::TintedImage(
            assets.path(format!("ui-icons/{name}.lvec")),
            rect,
            0.,
            true,
            color,
        ));
    }
    pub(super) fn control(&mut self, target: Target, rect: Rect, scrollable: bool) {
        self.controls.push(Control {
            target,
            rect,
            scrollable,
        });
    }
    pub(super) fn ring(&mut self, rect: Rect, radius: f32) {
        self.border(rect.inset(-6.9), radius + 6.9, 3.7, c(0xffffff, 0.10));
        self.border(rect.inset(-3.2), radius + 3.2, 3.2, c(0xf0f0f0, 1.));
    }
    pub(super) fn shadow(&mut self, rect: Rect, radius: f32, blur: f32, y: f32, alpha: f32) {
        self.draw.push(Draw::Shadow(
            Rect::new(rect.x, rect.y + y, rect.w, rect.h),
            radius,
            blur,
            c(0, alpha),
        ));
    }
    /// Lays what an earlier frame drew (a page or search being left) over
    /// this one, moved `dx` and faded to `alpha`; what it set for the draws
    /// after it (clip, edge fade, soft edge) ends with it.
    pub(super) fn ghost(&mut self, draws: &[Draw], dx: f32, alpha: f32) {
        if alpha <= 0. {
            return;
        }
        let start = self.draw.len();
        self.draw.extend(draws.iter().cloned());
        self.translate(start, dx, 0.);
        self.fade(start, alpha, 1.);
        self.draw
            .extend([Draw::Clip(None), Draw::EdgeFade(None), Draw::Soft(None)]);
    }
    /// Moves everything drawn from `from`; its clips stay put (a page scrolling under them).
    pub(super) fn translate(&mut self, from: usize, dx: f32, dy: f32) {
        if dx == 0. && dy == 0. {
            return;
        }
        for draw in &mut self.draw[from..] {
            match draw {
                Draw::Shape(rect, ..)
                | Draw::RoundedGradient(rect, ..)
                | Draw::GradientRing(rect, ..)
                | Draw::Shadow(rect, ..)
                | Draw::Text(_, rect, ..)
                | Draw::Image(_, rect, ..)
                | Draw::TintedImage(_, rect, ..)
                | Draw::AppImage(_, rect, _)
                | Draw::Glass(_, rect, ..) => {
                    rect.x += dx;
                    rect.y += dy;
                }
                // Full-screen opaque backdrops neither move nor fade with content.
                Draw::Clip(_) | Draw::EdgeFade(_) | Draw::Soft(_) | Draw::Dissolve(..) => {}
            }
        }
    }
    /// Scales everything drawn from `from` by `factor` about (`cx`, `cy`), corners, borders and
    /// shadows with it, and its own clips; text keeps its layout and is drawn scaled
    /// (`TextStyle::scale`), so a piece can shrink into the place it came from in one motion.
    pub(super) fn scale_about(&mut self, from: usize, cx: f32, cy: f32, factor: f32) {
        if (factor - 1.).abs() < f32::EPSILON {
            return;
        }
        let place = |rect: &mut Rect| {
            *rect = Rect::new(
                cx + (rect.x - cx) * factor,
                cy + (rect.y - cy) * factor,
                rect.w * factor,
                rect.h * factor,
            );
        };
        for draw in &mut self.draw[from..] {
            match draw {
                Draw::Shape(rect, radius, width, _)
                | Draw::GradientRing(rect, radius, width, ..) => {
                    place(rect);
                    *radius *= factor;
                    *width *= factor;
                }
                Draw::Shadow(rect, radius, blur, _) => {
                    place(rect);
                    *radius *= factor;
                    *blur *= factor;
                }
                Draw::RoundedGradient(rect, radius, ..)
                | Draw::Image(_, rect, radius, _)
                | Draw::TintedImage(_, rect, radius, ..)
                | Draw::Glass(_, rect, radius, ..) => {
                    place(rect);
                    *radius *= factor;
                }
                // Laid out in its own box, drawn scaled about the box's top left.
                Draw::Text(_, rect, style, ..) => {
                    rect.x = cx + (rect.x - cx) * factor;
                    rect.y = cy + (rect.y - cy) * factor;
                    style.scale *= factor;
                }
                Draw::AppImage(_, rect, _) | Draw::Clip(Some(rect)) => place(rect),
                Draw::Clip(None) | Draw::EdgeFade(_) | Draw::Soft(_) | Draw::Dissolve(..) => {}
            }
        }
    }
    /// tvOS Liquid Glass: the blurred wallpaper behind `rect` under a light
    /// veil. Cheap: no runtime blur, a lens only along its rim.
    pub(super) fn glass(
        &mut self,
        assets: &Assets,
        background: &str,
        rect: Rect,
        radius: f32,
        veil: Color,
    ) {
        self.glass_over(
            assets.path(format!("wallpapers/{background}-glass.png")),
            rect,
            radius,
            veil,
        );
    }
    /// Glass over a specific pre-blurred backdrop (Top Shelf art, say): the
    /// backdrop through the glass's lens (glass.frag), in the appearance's
    /// light, under `veil`. Every glass piece is this one material.
    pub(super) fn glass_over(&mut self, backdrop: String, rect: Rect, radius: f32, veil: Color) {
        let (top, bottom) = self.theme.lens();
        self.draw
            .push(Draw::Glass(backdrop, rect, radius, top, bottom, None));
        if veil[3] > 0. {
            self.shape(rect, radius, veil);
        }
    }
    /// Glass's one thin specular rim, `top` to `bottom` strong in the
    /// appearance's light (two stacked rims read as a soft band once the TV
    /// scales to 4K).
    pub(super) fn rim(&mut self, rect: Rect, radius: f32, top: f32, bottom: f32) {
        let (top, bottom) = self.theme.edge(top, bottom);
        self.draw
            .push(Draw::GradientRing(rect, radius, 1.5, top, bottom));
    }
    pub(super) fn fade(&mut self, from: usize, alpha: f32, brightness: f32) {
        self.recolour(from..self.draw.len(), alpha, brightness);
    }
    /// Darkens everything drawn so far to `brightness`, as a black veil of
    /// 1 - brightness over it would, but by recolouring those draws: no
    /// full-screen layer for the TV's GPU to paint. The background pass
    /// darkens through its tints and lift, which is exact for any mix.
    pub(super) fn dim(&mut self, brightness: f32) {
        if brightness >= 1. {
            return;
        }
        for draw in &mut self.draw {
            if let Draw::Dissolve(backdrop) = draw {
                for colour in backdrop.tint.iter_mut().chain([&mut backdrop.lift]) {
                    for channel in &mut colour[..3] {
                        *channel *= brightness;
                    }
                }
            }
        }
        let lit = self.lit.clone().unwrap_or(0..0);
        self.recolour(0..lit.start, 1., brightness);
        self.recolour(lit.end..self.draw.len(), 1., brightness);
    }
    /// Cross-fades everything drawn so far `amount` of the way to `backdrop`
    /// tinted `shade`, as that picture laid over it at `amount` would look,
    /// but within the existing background pass (one full-screen layer less
    /// for the TV's GPU): the background mixes into the backdrop and the
    /// content above it fades away. False, drawing nothing, when the
    /// background is itself mid-dissolve between two pictures; the caller
    /// then lays the backdrop over as a layer.
    pub(super) fn fade_into(&mut self, backdrop: &str, shade: Color, amount: f32) -> bool {
        let passes = self
            .draw
            .iter()
            .filter(|draw| matches!(draw, Draw::Dissolve(..)));
        if passes.count() != 1 {
            return false;
        }
        let Some(Draw::Dissolve(pass)) = self
            .draw
            .iter_mut()
            .find(|draw| matches!(draw, Draw::Dissolve(..)))
        else {
            return false;
        };
        let Some((picture, tint)) = pass.single().map(|(path, tint)| (path.to_owned(), tint))
        else {
            return false;
        };
        if pass.opacity < 1. {
            return false;
        }
        pass.from = picture;
        pass.to = backdrop.to_owned();
        pass.mix = amount;
        pass.tint = [tint, shade];
        pass.lift[3] *= 1. - amount;
        self.recolour(0..self.draw.len(), 1. - amount, 1.);
        true
    }
    fn recolour(&mut self, range: std::ops::Range<usize>, alpha: f32, brightness: f32) {
        for draw in &mut self.draw[range] {
            let apply = |color: &mut Color| {
                for channel in &mut color[..3] {
                    *channel *= brightness;
                }
                color[3] *= alpha;
            };
            match draw {
                Draw::Shape(_, _, _, color)
                | Draw::Shadow(_, _, _, color)
                | Draw::Text(_, _, _, color, _)
                | Draw::TintedImage(_, _, _, _, color)
                | Draw::AppImage(_, _, color) => apply(color),
                Draw::Glass(_, _, _, top, bottom, _) => {
                    apply(top);
                    apply(bottom);
                }
                Draw::RoundedGradient(_, _, a, b) | Draw::GradientRing(_, _, _, a, b) => {
                    apply(a);
                    apply(b);
                }
                Draw::Image(path, rect, radius, contain) => {
                    *draw = Draw::TintedImage(
                        path.clone(),
                        *rect,
                        *radius,
                        *contain,
                        [brightness, brightness, brightness, alpha],
                    );
                }
                // Full-screen opaque backdrops neither move nor fade with content.
                Draw::Clip(_) | Draw::EdgeFade(_) | Draw::Soft(_) | Draw::Dissolve(..) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::Backdrop;

    fn scene(background: Backdrop) -> Scene {
        let mut out = Scene::new(super::super::theme::Theme::new(0.));
        out.draw.push(Draw::Dissolve(background));
        out.shape(Rect::new(0., 0., 100., 100.), 0., [1., 1., 1., 1.]);
        out
    }
    fn pass(out: &Scene) -> &Backdrop {
        match &out.draw[0] {
            Draw::Dissolve(backdrop) => backdrop,
            _ => unreachable!(),
        }
    }
    /// What the dissolve shader shows for one channel of two texels.
    fn shown(pass: &Backdrop, a: f32, b: f32, channel: usize) -> f32 {
        let x = a * pass.tint[0][channel]
            + (b * pass.tint[1][channel] - a * pass.tint[0][channel]) * pass.mix;
        x + (pass.lift[channel] - x) * pass.lift[3]
    }

    #[test]
    fn the_switcher_backdrop_fades_in_within_the_background_pass() {
        let shade = [0.38, 0.38, 0.4, 1.];
        // The home's photo mixes into the backdrop; content fades with it.
        let mut out = scene(Backdrop::picture("photo", [1.; 4]));
        assert!(out.fade_into("glass", shade, 0.25));
        let p = pass(&out);
        assert!(p.from == "photo" && p.to == "glass" && p.mix == 0.25 && p.tint[1] == shade);
        assert!(matches!(out.draw[1], Draw::Shape(_, _, _, color) if color[3] == 0.75));
        // The grid's backdrop is already that picture: only its tint moves.
        let mut out = scene(Backdrop::picture("glass", [0.62; 4]));
        assert!(out.fade_into("glass", shade, 1.));
        assert_eq!(pass(&out).single(), Some(("glass", shade)));
        // Mid-dissolve between two pictures: left alone for a layer instead.
        let mut out = scene(Backdrop::between("a", "b", 0.5));
        assert!(!out.fade_into("glass", shade, 0.5));
        assert!(matches!(out.draw[1], Draw::Shape(_, _, _, color) if color[3] == 1.));
    }

    #[test]
    fn dimming_any_background_is_exact_and_adds_no_layer() {
        for background in [
            Backdrop::between("a", "b", 0.5),
            Backdrop {
                tint: [[0.9, 0.8, 0.7, 1.], [0.3, 0.3, 0.32, 1.]],
                lift: [0.96, 0.96, 0.97, 0.7],
                ..Backdrop::between("a", "b", 0.3)
            },
        ] {
            let before = background.clone();
            let mut out = scene(background);
            out.dim(0.65);
            assert_eq!(out.draw.len(), 2, "no veil drawn");
            for (a, b) in [(0., 1.), (0.4, 0.9), (1., 0.2)] {
                for channel in 0..3 {
                    let expected = 0.65 * shown(&before, a, b, channel);
                    assert!((shown(pass(&out), a, b, channel) - expected).abs() < 1e-5);
                }
            }
            assert!(
                matches!(out.draw[1], Draw::Shape(_, _, _, color) if (color[0] - 0.65).abs() < 1e-6)
            );
        }
    }
    #[test]
    fn the_tile_a_menu_acts_on_stays_lit_while_the_screen_dims() {
        // Background, then the held tile's face, then another tile.
        for background in [
            Backdrop::picture("a", [1.; 4]),
            Backdrop {
                lift: [0.9, 0.9, 0.9, 0.5],
                ..Backdrop::between("a", "b", 0.5)
            },
        ] {
            let mut out = scene(background);
            out.shape(Rect::new(0., 0., 10., 10.), 0., [1., 1., 1., 1.]);
            out.lit = Some(1..2);
            out.dim(0.65);
            let colour = |draw: &Draw| match draw {
                Draw::Shape(_, _, _, color) => color[0],
                _ => unreachable!(),
            };
            assert_eq!(out.draw.len(), 3, "no veil, no second tile");
            assert_eq!(colour(&out.draw[1]), 1., "the held tile keeps its light");
            assert!((colour(&out.draw[2]) - 0.65).abs() < 1e-6, "the rest dims");
        }
    }
}
