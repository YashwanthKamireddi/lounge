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
    /// This frame's appearance (Light, Dark, or between while it changes).
    pub(super) theme: super::theme::Theme,
}
impl Scene {
    pub(super) fn new(theme: super::theme::Theme) -> Self {
        Self {
            theme,
            draw: Vec::new(),
            controls: Vec::new(),
            modal: None,
            settings_range: None,
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
    pub(super) fn translate(&mut self, from: usize, dx: f32, dy: f32) {
        self.shift(from, dx, dy, false);
    }
    /// As `translate`, also moving the clips drawn from `from`: for a piece
    /// that owns its clips (the context menu masks its words to its
    /// highlight). A page's clips stay put when it scrolls.
    pub(super) fn translate_with_clips(&mut self, from: usize, dx: f32, dy: f32) {
        self.shift(from, dx, dy, true);
    }
    fn shift(&mut self, from: usize, dx: f32, dy: f32, clips: bool) {
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
                Draw::Clip(Some(rect)) if clips => {
                    rect.x += dx;
                    rect.y += dy;
                }
                // Full-screen opaque backdrops neither move nor fade with content.
                Draw::Clip(_) | Draw::EdgeFade(_) | Draw::Soft(_) | Draw::Dissolve(..) => {}
            }
        }
    }
    /// tvOS Liquid Glass: the blurred wallpaper behind `rect` under a light
    /// veil. Cheap: one texture sample, no runtime blur.
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
    /// Glass over a specific pre-blurred backdrop (Top Shelf art, say).
    pub(super) fn glass_over(&mut self, backdrop: String, rect: Rect, radius: f32, veil: Color) {
        self.draw
            .push(Draw::Glass(backdrop, rect, radius, [1.; 4], [1.; 4]));
        self.shape(rect, radius, veil);
        // No rim here: each glass piece draws its own single specular rim
        // (two stacked rims read as a soft band once the TV scales to 4K).
    }
    pub(super) fn fade(&mut self, from: usize, alpha: f32, brightness: f32) {
        self.recolour(from..self.draw.len(), alpha, brightness);
    }
    /// Darkens everything drawn so far to `brightness`, as a black veil of
    /// 1 - brightness over it would, but by recolouring those draws: no
    /// full-screen layer for the TV's GPU to paint. Only a lifted backdrop
    /// (light Settings) caught mid-dissolve between two pictures cannot be
    /// recoloured whole; then the veil is drawn after all.
    pub(super) fn dim(&mut self, brightness: f32) {
        if brightness >= 1. {
            return;
        }
        let mut whole = true;
        for draw in &mut self.draw {
            if let Draw::Dissolve(from, to, amount, tint, lift) = draw {
                if lift[3] <= 0. {
                    // A lift toward black by 1 - brightness is exactly that
                    // darkening, of whatever the pass shows (two pictures
                    // mid-dissolve included).
                    *lift = [0., 0., 0., 1. - brightness];
                    continue;
                }
                if *amount <= 0. {
                    // Only the first picture shows: draw it as the second.
                    *to = from.clone();
                }
                if from == to || *amount <= 0. || *amount >= 1. {
                    // One picture: drawn fully as the tinted second image.
                    *amount = 1.;
                    for channel in 0..3 {
                        tint[channel] *= brightness;
                        lift[channel] *= brightness;
                    }
                } else {
                    whole = false;
                }
            }
        }
        if whole {
            self.recolour(0..self.draw.len(), 1., brightness);
        } else {
            self.shape(Rect::new(0., 0., 1920., 1080.), 0., c(0, 1. - brightness));
        }
    }
    /// Lays a full-screen `veil` over the background, as a layer of that
    /// colour would (straight alpha), but inside the background's own pass:
    /// the pass's lift toward a colour composes with the veil exactly. Only
    /// while the background is all that is drawn; otherwise the veil is
    /// drawn as a layer after all.
    pub(super) fn veil(&mut self, veil: Color) {
        if veil[3] <= 0. {
            return;
        }
        if let [Draw::Dissolve(.., lift)] = self.draw.as_mut_slice() {
            // mix(mix(x, L, l), V, v) = mix(x, L', 1 - (1 - l)(1 - v)).
            let alpha = 1. - (1. - lift[3]) * (1. - veil[3]);
            for channel in 0..3 {
                lift[channel] =
                    ((1. - veil[3]) * lift[3] * lift[channel] + veil[3] * veil[channel]) / alpha;
            }
            lift[3] = alpha;
        } else {
            self.shape(Rect::new(0., 0., 1920., 1080.), 0., veil);
        }
    }
    /// Cross-fades everything drawn so far `amount` of the way to `backdrop`
    /// tinted `shade`, as that picture laid over it at `amount` would look,
    /// but within the existing background pass (one full-screen layer less
    /// for the TV's GPU): the background mixes into the backdrop and the
    /// content above it fades away. False, drawing nothing, when the
    /// background is itself mid-dissolve; the caller then lays the backdrop
    /// over as a layer.
    pub(super) fn fade_into(&mut self, backdrop: &str, shade: Color, amount: f32) -> bool {
        let Some(Draw::Dissolve(_, to, mix, tint, lift)) = self
            .draw
            .iter_mut()
            .find(|draw| matches!(draw, Draw::Dissolve(..)))
        else {
            return false;
        };
        let blend = |a: Color, b: Color| -> Color {
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * amount)
        };
        if *mix >= 1. && to == backdrop {
            // Already that picture (the grid's backdrop): only its tint moves.
            *tint = blend(*tint, shade);
            lift[3] *= 1. - amount;
        } else if *mix <= 0. && lift[3] <= 0. {
            // The untinted first picture (the home's photo) mixes into it.
            *to = backdrop.to_owned();
            *mix = amount;
            *tint = shade;
        } else {
            return false;
        }
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
                Draw::Glass(_, _, _, top, bottom) => {
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

    fn scene(background: Draw) -> Scene {
        let mut out = Scene::new(super::super::theme::Theme::new(0.));
        out.draw.push(background);
        out.shape(Rect::new(0., 0., 100., 100.), 0., [1., 1., 1., 1.]);
        out
    }

    #[test]
    fn the_switcher_backdrop_fades_in_within_the_background_pass() {
        let shade = [0.38, 0.38, 0.4, 1.];
        // The home's photo mixes into the backdrop; content fades with it.
        let mut out = scene(Draw::Dissolve(
            "photo".into(),
            "photo".into(),
            0.,
            [1.; 4],
            [0.; 4],
        ));
        assert!(out.fade_into("glass", shade, 0.25));
        assert!(
            matches!(&out.draw[0], Draw::Dissolve(from, to, mix, tint, _)
            if from == "photo" && to == "glass" && *mix == 0.25 && *tint == shade)
        );
        assert!(matches!(out.draw[1], Draw::Shape(_, _, _, color) if color[3] == 0.75));
        // The grid's backdrop is already that picture: only its tint moves.
        let mut out = scene(Draw::Dissolve(
            "glass".into(),
            "glass".into(),
            1.,
            [0.62; 4],
            [0.; 4],
        ));
        assert!(out.fade_into("glass", shade, 1.));
        assert!(matches!(&out.draw[0], Draw::Dissolve(_, _, _, tint, _) if *tint == shade));
        // Mid-dissolve between two pictures: left alone for a layer instead.
        let mut out = scene(Draw::Dissolve(
            "a".into(),
            "b".into(),
            0.5,
            [1.; 4],
            [0.; 4],
        ));
        assert!(!out.fade_into("glass", shade, 0.5));
        assert!(matches!(out.draw[1], Draw::Shape(_, _, _, color) if color[3] == 1.));
    }

    #[test]
    fn dimming_a_background_mid_dissolve_adds_no_layer() {
        let mut out = scene(Draw::Dissolve(
            "a".into(),
            "b".into(),
            0.5,
            [1.; 4],
            [0.; 4],
        ));
        out.dim(0.65);
        assert_eq!(out.draw.len(), 2, "no veil drawn");
        assert!(matches!(&out.draw[0], Draw::Dissolve(_, _, _, _, lift)
            if lift[..3] == [0.; 3] && (lift[3] - 0.35).abs() < 1e-6));
        assert!(
            matches!(out.draw[1], Draw::Shape(_, _, _, color) if (color[0] - 0.65).abs() < 1e-6)
        );
    }
    #[test]
    fn a_backdrop_veil_folds_into_the_background_pass_exactly() {
        // What the dissolve shader shows, then a straight-alpha layer over it.
        let pass = |x: f32, lift: Color, channel: usize| x + (lift[channel] - x) * lift[3];
        let layer =
            |below: f32, veil: Color, channel: usize| below + (veil[channel] - below) * veil[3];
        for (lift, veil) in [
            ([0.; 4], [0., 0., 0., 0.3]),
            ([0.; 4], [0.96, 0.96, 0.97, 0.45]),
            ([0.96, 0.96, 0.97, 0.7], [0.96, 0.96, 0.97, 0.2]),
            ([0., 0., 0., 0.35], [0.9, 0.8, 0.7, 0.5]),
        ] {
            let mut out = Scene::new(super::super::theme::Theme::new(0.));
            out.draw
                .push(Draw::Dissolve("a".into(), "b".into(), 0.5, [1.; 4], lift));
            out.veil(veil);
            assert_eq!(out.draw.len(), 1, "no layer drawn");
            let Draw::Dissolve(.., folded) = out.draw[0] else {
                unreachable!()
            };
            for x in [0., 0.25, 0.6, 1.] {
                for channel in 0..3 {
                    let expected = layer(pass(x, lift, channel), veil, channel);
                    assert!((pass(x, folded, channel) - expected).abs() < 1e-5);
                }
            }
        }
        // With content already drawn the veil would cover it too: a layer.
        let mut out = scene(Draw::Dissolve("a".into(), "a".into(), 1., [1.; 4], [0.; 4]));
        out.veil([0., 0., 0., 0.4]);
        assert_eq!(out.draw.len(), 3);
    }
}
