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
}
impl Scene {
    pub(super) fn new() -> Self {
        Self {
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
                Draw::Clip(_) | Draw::EdgeFade(_) | Draw::Dissolve(..) => {}
            }
        }
    }
    /// tvOS Liquid Glass: the blurred wallpaper behind `rect` under a light
    /// veil. Cheap: one texture sample, no runtime blur.
    pub(super) fn glass(&mut self, assets: &Assets, background: &str, rect: Rect, radius: f32, veil: Color) {
        self.glass_over(
            assets.path(format!("wallpapers/{background}-glass.png")),
            rect,
            radius,
            veil,
        );
    }
    /// Glass over a specific pre-blurred backdrop (Top Shelf art, say).
    pub(super) fn glass_over(&mut self, backdrop: String, rect: Rect, radius: f32, veil: Color) {
        self.draw.push(Draw::Glass(backdrop, rect, radius, [1.; 4], [1.; 4]));
        self.shape(rect, radius, veil);
        // No rim here: each glass piece draws its own single specular rim
        // (two stacked rims read as a soft band once the TV scales to 4K).
    }
    pub(super) fn fade(&mut self, from: usize, alpha: f32, brightness: f32) {
        for draw in &mut self.draw[from..] {
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
                Draw::Clip(_) | Draw::EdgeFade(_) | Draw::Dissolve(..) => {}
            }
        }
    }
}
