//! What the renderer reports about the GPU and each frame.
use super::*;

#[derive(Clone, Serialize)]
pub struct GraphicsInfo {
    pub vendor: String,
    pub renderer: String,
    pub version: String,
    pub max_texture_size: u32,
    pub max_viewport: [i32; 2],
    pub fragment_highp: bool,
}

#[derive(Clone, Serialize)]
pub struct RenderStats {
    pub target: Size,
    pub viewport: Viewport,
    pub font_antialiasing: &'static str,
    pub multisample_samples: u32,
    pub max_logical_font_px: f32,
    pub max_physical_font_px: f32,
    pub cached_glyphs: usize,
    pub glyph_tessellations: usize,
    /// Textures uploaded since start (cumulative).
    pub texture_uploads: usize,
    pub glyph_failures: usize,
    pub missing_glyphs: usize,
    pub missing_images: usize,
    pub glyph_geometry_bytes: usize,
    pub cached_text_runs: usize,
    pub text_geometry_bytes: usize,
    pub text_triangles: usize,
    pub artwork_bytes: usize,
    pub vector_meshes: usize,
    pub vector_bytes: usize,
    pub vector_triangles: usize,
    pub high_resolution_wallpaper: bool,
    pub draw_calls: usize,
    pub cpu_submission_ms: f64,
    pub cpu_build_ms: f64,
    /// Pixels this frame paints, in full screens, by kind of draw. The TV's
    /// one-core GPU is bound by exactly this (fill), so it is the number to
    /// cut; a desktop run measures it faithfully.
    pub fill: std::collections::BTreeMap<&'static str, f32>,
    /// The frame with the most fill since start, and what it was made of.
    pub peak_fill: std::collections::BTreeMap<&'static str, f32>,
    /// Opaque tiles masking what lies behind them, and their area (screens).
    pub occluders: usize,
    pub occluded: f32,
}

impl RenderStats {
    pub(super) fn new(target: Size) -> Self {
        Self {
            target,
            viewport: Viewport::new(target),
            font_antialiasing: "vector grayscale coverage",
            multisample_samples: 0,
            max_logical_font_px: 0.,
            max_physical_font_px: 0.,
            cached_glyphs: 0,
            glyph_tessellations: 0,
            texture_uploads: 0,
            glyph_failures: 0,
            missing_glyphs: 0,
            missing_images: 0,
            glyph_geometry_bytes: 0,
            cached_text_runs: 0,
            text_geometry_bytes: 0,
            text_triangles: 0,
            artwork_bytes: 0,
            vector_meshes: 0,
            vector_bytes: 0,
            vector_triangles: 0,
            high_resolution_wallpaper: false,
            draw_calls: 0,
            cpu_submission_ms: 0.,
            cpu_build_ms: 0.,
            fill: Default::default(),
            peak_fill: Default::default(),
            occluders: 0,
            occluded: 0.,
        }
    }
}
