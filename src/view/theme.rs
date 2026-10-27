//! Appearance: the few colours Light and Dark differ in, for Settings, menus,
//! glass and the time pill. The home's photo, tiles and dock look the same in
//! both, as on tvOS. A theme is one number (how light), so a change
//! cross-fades by mixing these colours, and it adds no layer to any frame.
use super::shared::{PLATTER, c};
use crate::renderer::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// 0 dark, 1 light.
    light: f32,
}

/// Apple's light frosted material: a cool, near-white grey.
const FROST: [f32; 3] = [0.95, 0.95, 0.97];

impl Theme {
    pub fn new(light: f32) -> Self {
        Self {
            light: light.clamp(0., 1.),
        }
    }

    fn mix(self, dark: f32, light: f32) -> f32 {
        dark + (light - dark) * self.light
    }

    /// Between two colours chosen for each appearance.
    pub fn pick(self, dark: Color, light: Color) -> Color {
        std::array::from_fn(|i| self.mix(dark[i], light[i]))
    }

    /// Text and glyphs: near-white on dark, near-black on light.
    pub fn ink(self, alpha: f32) -> Color {
        let v = self.mix(0.96, 0.1);
        [v, v, v, alpha]
    }

    /// A faint fill or hairline: light on the dark backdrop, a soft shade on
    /// the light one.
    pub fn wash(self, alpha: f32) -> Color {
        let v = self.mix(1., 0.);
        [v, v, v, alpha * self.mix(1., 0.6)]
    }

    /// A well behind a field (the location search, a saved place).
    pub fn well(self, alpha: f32) -> Color {
        let [r, g, b, _] = c(0x171c20, 1.);
        [
            self.mix(r, 1.),
            self.mix(g, 1.),
            self.mix(b, 1.),
            self.mix(alpha, 0.75),
        ]
    }

    /// The veil laid over glass menus and banners: dark smoke, or on light
    /// the bright frosted glass of a light menu (dark text reads on it over
    /// any wallpaper).
    pub fn veil(self, alpha: f32) -> Color {
        let [r, g, b, _] = c(PLATTER, 1.);
        [
            self.mix(r, FROST[0]),
            self.mix(g, FROST[1]),
            self.mix(b, FROST[2]),
            self.mix(alpha, 0.92),
        ]
    }

    /// The light along a glass panel's edge, top then bottom: on dark glass
    /// a white glow that fades down; on light glass, where white cannot show,
    /// a highlight at the top and a faint shade at the bottom.
    pub fn edge(self, top: f32, bottom: f32) -> (Color, Color) {
        let shade = self.mix(1., 0.);
        (
            [1., 1., 1., (top * self.mix(1., 1.6)).min(1.)],
            [shade, shade, shade, self.mix(bottom, top * 0.25)],
        )
    }

    /// Regular Liquid Glass light: the pre-blurred backdrop brightened where
    /// light enters at the top, as the glass bends and concentrates it. Dark
    /// glass lenses it; light glass is mostly veil, so it stays even.
    pub fn lens(self) -> (Color, Color) {
        let top = self.mix(1.3, 1.);
        ([top, top, top * 1.02, 1.], [1.; 4])
    }

    /// Settings' backdrop over the blurred wallpaper: the multiply for a dark
    /// veil of `shade`, and the lift toward frost (its alpha 0 on dark).
    pub fn backdrop(self, shade: f32) -> (Color, Color) {
        let v = self.mix(shade, 1.);
        (
            [v, v, self.mix(shade + 0.02, 1.), 1.],
            [FROST[0], FROST[1], FROST[2], 0.7 * self.light],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_matches_the_colours_it_replaced() {
        let dark = Theme::new(0.);
        assert_eq!(dark.ink(1.), crate::renderer::WHITE);
        assert_eq!(dark.wash(0.08), [1., 1., 1., 0.08]);
        assert_eq!(dark.veil(0.34), super::super::shared::c(PLATTER, 0.34));
        assert_eq!(dark.backdrop(0.32).1[3], 0.);
        assert_eq!(dark.well(0.72), c(0x171c20, 0.72));
    }

    #[test]
    fn light_reads_dark_on_frost() {
        let light = Theme::new(1.);
        assert!(light.ink(1.)[0] < 0.2);
        assert_eq!(light.backdrop(0.32).0, [1., 1., 1., 1.]);
        assert!(light.backdrop(0.32).1[3] > 0.5);
    }
}
