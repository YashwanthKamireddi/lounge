//! A small GLES2-compatible GPU painter. Rust owns vector glyphs and the draw list;
//! SDL only supplies the context. No browser, LVGL or Qt runtime.
use crate::{
    font::Font,
    raster::{Size, Viewport},
    state::Rect,
    vector,
};
use gl::types::*;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

pub type Color = [f32; 4];
pub const WHITE: Color = [0.96, 0.96, 0.96, 1.];
const GLYPH_GEOMETRY_BUDGET: usize = 8 * 1024 * 1024;
const TEXT_GEOMETRY_BUDGET: usize = 32 * 1024 * 1024;
const TEXTURE_BUDGET: usize = 48 * 1024 * 1024;
/// Every texture upload, for profiling frames that stall on one.
static TEXTURE_UPLOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: u16,
    /// CSS letter-spacing in em, not pixels.
    pub tracking: f32,
    pub line_height: f32,
    pub align_right: bool,
}
impl TextStyle {
    pub const fn new(size: f32) -> Self {
        Self {
            size,
            weight: 400,
            tracking: 0.,
            line_height: 1.2,
            align_right: false,
        }
    }
    pub const fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }
    pub const fn tracking(mut self, tracking: f32) -> Self {
        self.tracking = tracking;
        self
    }
    pub const fn leading(mut self, height: f32) -> Self {
        self.line_height = height;
        self
    }
    pub const fn right(mut self) -> Self {
        self.align_right = true;
        self
    }
}

#[derive(Clone)]
pub enum Draw {
    Shape(Rect, f32, f32, Color),
    RoundedGradient(Rect, f32, Color, Color),
    /// A rounded ring whose colour runs from top to bottom: Liquid Glass's
    /// specular rim, bright where light falls on the edge and gone below.
    GradientRing(Rect, f32, f32, Color, Color),
    Shadow(Rect, f32, f32, Color),
    Text(String, Rect, TextStyle, Color, bool),
    Image(String, Rect, f32, bool),
    TintedImage(String, Rect, f32, bool, Color),
    /// A full-screen backdrop texture sampled at `rect`'s own screen position,
    /// tinted from the first colour at the top to the second at the bottom
    /// (a tint above 1 lights the glass in the same pass, no sheen layer).
    Glass(String, Rect, f32, Color, Color),
    AppImage(Arc<crate::app_artwork::Artwork>, Rect, Color),
    /// Two full-screen images mixed in one opaque pass: the first tinted by
    /// nothing, the second by its colour, `f32` of the way to the second.
    Dissolve(String, String, f32, Color),
    Clip(Option<Rect>),
    EdgeFade(Option<[f32; 4]>),
}

struct Texture {
    id: GLuint,
    width: u32,
    height: u32,
    frame: u64,
}
impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.id);
        }
    }
}

struct Glyph {
    geometry: Vec<luma_vector_geometry::Vertex>,
    frame: u64,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct TextKey {
    text: String,
    weight: u16,
    // Logical size, tracking, line height, available width and height.
    layout: [u32; 5],
    flags: u8,
}

struct TextRun {
    mesh: Option<vector::Mesh>,
    missing_glyphs: usize,
    failures: usize,
    frame: u64,
}

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
    fn new(target: Size) -> Self {
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

/// The parts of `rect` a shape actually paints, as fractions of it. A rim (a
/// border-only shape) covers just its edge: four bands along it rather than
/// the whole rectangle, whose middle the shader would only fill with
/// transparent pixels (on the TV's one-core GPU the dock's rim alone was a
/// quarter of a screen per frame). Each band keeps the whole shape's
/// coordinates, so the pixels are the same.
fn regions(rect: Rect, radius: f32, border: f32) -> Vec<[f32; 4]> {
    let edge = border + 3.;
    let corner = (radius.max(0.) + edge) / rect.h;
    let side = edge / rect.w;
    if border > 0. && corner < 0.45 && side < 0.2 {
        vec![
            [0., 0., 1., corner],
            [0., 1. - corner, 1., 1.],
            [0., corner, side, 1. - corner],
            [1. - side, corner, 1., 1. - corner],
        ]
    } else {
        vec![[0., 0., 1., 1.]]
    }
}

/// The share of the 1080p screen `rect` covers inside `clip`.
fn screens(rect: Rect, clip: Option<Rect>) -> f32 {
    let bound = clip.unwrap_or(Rect::new(0., 0., 1920., 1080.));
    let x = rect.x.max(bound.x).max(0.);
    let y = rect.y.max(bound.y).max(0.);
    let right = (rect.x + rect.w).min(bound.x + bound.w).min(1920.);
    let bottom = (rect.y + rect.h).min(bound.y + bound.h).min(1080.);
    ((right - x).max(0.) * (bottom - y).max(0.)) / (1920. * 1080.)
}

enum BatchKind {
    Paint {
        texture: GLuint,
        start: usize,
        count: usize,
    },
    Vector {
        path: String,
        rect: Rect,
        tint: Color,
    },
    Text {
        key: TextKey,
        origin: [f32; 2],
        tint: Color,
    },
    Dissolve {
        from: GLuint,
        to: GLuint,
        start: usize,
        amount: f32,
        tint: Color,
    },
}

struct Batch {
    kind: BatchKind,
    clip: Option<Rect>,
    fade: Option<[f32; 4]>,
    /// How many opaque tiles were drawn up to this batch (its own included).
    /// It skips the pixels covered by any later tile (stencil-masked), which
    /// would paint over them anyway.
    rank: u8,
}

/// Uniform locations resolved once at link time; name lookups per draw call
/// are a measurable cost on TV GL drivers.
#[derive(Clone, Copy)]
struct Uniforms {
    aa: GLint,
    texture: GLint,
    edge_fade: GLint,
    fringe: GLint,
    transform: GLint,
    pixel_scale: GLint,
    tint: GLint,
}
impl Uniforms {
    fn of(program: GLuint) -> Self {
        let at = |name: &std::ffi::CStr| unsafe { gl::GetUniformLocation(program, name.as_ptr()) };
        Self {
            aa: at(c"u_aa"),
            texture: at(c"u_texture"),
            edge_fade: at(c"u_edge_fade"),
            fringe: at(c"u_fringe"),
            transform: at(c"u_transform"),
            pixel_scale: at(c"u_pixel_scale"),
            tint: at(c"u_tint"),
        }
    }
}

pub struct Renderer {
    /// The GPU takes ASTC textures (the TV's Mali does; desktop GL may not).
    astc: bool,
    program: GLuint,
    dissolve_program: GLuint,
    dissolve_uniforms: [GLint; 4],
    paint_uniforms: Uniforms,
    vector_uniforms: Uniforms,
    text_uniforms: Uniforms,
    vector_program: GLuint,
    text_program: GLuint,
    buffer: GLuint,
    vao: GLuint,
    white: Texture,
    font_directory: PathBuf,
    fonts: HashMap<u16, Font>,
    failed_fonts: HashSet<u16>,
    glyphs: HashMap<(char, u16), Glyph>,
    text_runs: HashMap<TextKey, TextRun>,
    textures: HashMap<String, Option<Texture>>,
    vectors: HashMap<String, Option<vector::Mesh>>,
    artwork_variants: HashMap<String, String>,
    vertices: Vec<f32>,
    batches: Vec<Batch>,
    clip: Option<Rect>,
    fade: Option<[f32; 4]>,
    /// The rank (see Batch) of the draws being batched now.
    rank: u8,
    /// For each draw of this frame: whether it is an opaque tile.
    occluder_at: Vec<bool>,
    /// The opaque tile interiors of this frame (logical px), marked in the
    /// stencil before anything else draws.
    occluders: Vec<Rect>,
    /// The current target has a stencil buffer (the TV window does; an
    /// offscreen capture does not, and then everything simply draws).
    stencil: bool,
    gles: bool,
    frame: u64,
    stats: RenderStats,
    pub info: GraphicsInfo,
    /// Diagnostic: draw kinds to skip when attributing frame cost on hardware.
    pub skip: Vec<String>,
    /// This frame repaints only this area (logical px); `None` = all.
    pub damage: Option<Rect>,
}

impl Renderer {
    pub fn new(font: &Path, gles: bool, medium_precision: bool) -> Result<Self, String> {
        let directory = font.parent().ok_or("Invalid font path")?;
        let mut fonts = HashMap::new();
        // Open only the common face; the others, and all metrics and
        // outlines, load lazily on first use.
        fonts.insert(400, load_font(directory, 400)?);
        let mut max_texture_size = 0;
        let mut max_viewport = [0; 2];
        let mut precision = 0;
        unsafe {
            gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut max_texture_size);
            gl::GetIntegerv(gl::MAX_VIEWPORT_DIMS, max_viewport.as_mut_ptr());
            if gles && gl::GetShaderPrecisionFormat::is_loaded() {
                let mut range = [0; 2];
                gl::GetShaderPrecisionFormat(
                    gl::FRAGMENT_SHADER,
                    gl::HIGH_FLOAT,
                    range.as_mut_ptr(),
                    &mut precision,
                );
            }
        }
        let fragment_highp = !gles || (precision > 0 && !medium_precision);
        let qualifier = if fragment_highp { "highp" } else { "mediump" };
        let vertex_prefix = if gles {
            format!("#version 100\nprecision highp float;\n#define PAINT_PRECISION {qualifier}\n")
        } else {
            "#version 150\n#define attribute in\n#define varying out\n#define PAINT_PRECISION\n"
                .into()
        };
        let fragment_prefix = if gles {
            format!(
                "#version 100\nprecision {qualifier} float;\n#define PAINT_PRECISION {qualifier}\n"
            )
        } else {
            "#version 150\n#define varying in\n#define texture2D texture\nout vec4 out_color;\n#define gl_FragColor out_color\n#define PAINT_PRECISION\n".into()
        };
        // The dissolve only mixes two texture colours, so medium precision
        // is exact enough, and Mali runs it up to twice as fast: it is the
        // one full-screen pass that moves while scrolling.
        let fragment_prefix_dissolve = if gles {
            "#version 100\nprecision mediump float;\n#define PAINT_PRECISION mediump\n".to_owned()
        } else {
            fragment_prefix.clone()
        };
        let vertex_prefix_dissolve = vertex_prefix.clone();
        let program = link(
            &(vertex_prefix.clone() + include_str!("shaders/paint.vert")),
            &(fragment_prefix.clone() + include_str!("shaders/paint.frag")),
            &[c"a_position", c"a_uv", c"a_color", c"a_shape", c"a_params"],
        )?;
        let vector_program = match link(
            &(vertex_prefix.clone() + include_str!("shaders/vector.vert")),
            &(fragment_prefix.clone() + include_str!("shaders/vector.frag")),
            &[c"a_position", c"a_offset", c"a_color", c"a_coverage"],
        ) {
            Ok(program) => program,
            Err(error) => {
                unsafe { gl::DeleteProgram(program) };
                return Err(error);
            }
        };
        let text_program = match link(
            &(vertex_prefix + include_str!("shaders/text.vert")),
            &(fragment_prefix + include_str!("shaders/vector.frag")),
            &[c"a_position", c"a_offset", c"a_origin", c"a_coverage"],
        ) {
            Ok(program) => program,
            Err(error) => {
                unsafe {
                    gl::DeleteProgram(program);
                    gl::DeleteProgram(vector_program);
                }
                return Err(error);
            }
        };
        let dissolve_program = match link(
            &(vertex_prefix_dissolve + include_str!("shaders/paint.vert")),
            &(fragment_prefix_dissolve + include_str!("shaders/dissolve.frag")),
            &[c"a_position", c"a_uv", c"a_color", c"a_shape", c"a_params"],
        ) {
            Ok(program) => program,
            Err(error) => {
                unsafe {
                    gl::DeleteProgram(program);
                    gl::DeleteProgram(vector_program);
                    gl::DeleteProgram(text_program);
                }
                return Err(error);
            }
        };
        let dissolve_uniforms = [c"u_texture", c"u_texture2", c"u_mix", c"u_tint2"]
            .map(|name| unsafe { gl::GetUniformLocation(dissolve_program, name.as_ptr()) });
        let mut buffer = 0;
        let mut vao = 0;
        unsafe {
            if !gles {
                gl::GenVertexArrays(1, &mut vao);
                gl::BindVertexArray(vao);
            }
            gl::GenBuffers(1, &mut buffer);
            gl::Enable(gl::BLEND);
            // Preserve an opaque destination alpha. Using BlendFunc for both
            // channels makes translucent UI darken again in PNG/window compositing.
            gl::BlendFuncSeparate(
                gl::SRC_ALPHA,
                gl::ONE_MINUS_SRC_ALPHA,
                gl::ONE,
                gl::ONE_MINUS_SRC_ALPHA,
            );
            gl::Disable(gl::DEPTH_TEST);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
        }
        let white = texture(1, 1, &[255; 4]);
        Ok(Self {
            paint_uniforms: Uniforms::of(program),
            dissolve_program,
            dissolve_uniforms,
            vector_uniforms: Uniforms::of(vector_program),
            text_uniforms: Uniforms::of(text_program),
            program,
            vector_program,
            text_program,
            buffer,
            vao,
            white,
            font_directory: directory.into(),
            fonts,
            failed_fonts: HashSet::new(),
            glyphs: HashMap::new(),
            text_runs: HashMap::new(),
            textures: HashMap::new(),
            vectors: HashMap::new(),
            artwork_variants: HashMap::new(),
            astc: gles && gl_string(gl::EXTENSIONS).split(' ').any(|name| name == "GL_KHR_texture_compression_astc_ldr"),
            vertices: Vec::with_capacity(65536),
            batches: Vec::new(),
            clip: None,
            fade: None,
            rank: u8::MAX,
            occluder_at: Vec::new(),
            occluders: Vec::new(),
            stencil: false,
            gles,
            frame: 0,
            stats: RenderStats::new(Size::HD),
            info: GraphicsInfo {
                vendor: gl_string(gl::VENDOR),
                renderer: gl_string(gl::RENDERER),
                version: gl_string(gl::VERSION),
                max_texture_size: max_texture_size.max(0) as u32,
                max_viewport,
                fragment_highp,
            },
            skip: Vec::new(),
            damage: None,
        })
    }

    fn ensure_font(&mut self, id: u16) -> bool {
        if self.fonts.contains_key(&id) {
            return true;
        }
        if self.failed_fonts.contains(&id) {
            return false;
        }
        match load_font(&self.font_directory, id) {
            Ok(font) => {
                self.fonts.insert(id, font);
                true
            }
            Err(error) => {
                eprintln!("{error}");
                self.failed_fonts.insert(id);
                false
            }
        }
    }

    fn glyph(&mut self, c: char, font: u16) -> Result<&[luma_vector_geometry::Vertex], String> {
        let key = (c, font);
        if !self.glyphs.contains_key(&key) {
            let geometry = self.fonts[&font].geometry(c)?;
            self.glyphs.insert(
                key,
                Glyph {
                    geometry,
                    frame: self.frame,
                },
            );
            self.stats.glyph_tessellations += 1;
        }
        let glyph = self.glyphs.get_mut(&key).unwrap();
        glyph.frame = self.frame;
        Ok(&glyph.geometry)
    }

    /// Prepare up to `budget` not-yet-cached glyphs of the printable ASCII
    /// set in the faces the interface uses, so a screen opened for the first
    /// time never tessellates its text mid-frame. Returns whether any work
    /// remains. Meant for idle ticks only.
    pub fn warm_glyphs(&mut self, budget: usize) -> bool {
        // Font ids are the weight, +1000 for display sizes (>= 32pt).
        const FACES: [u16; 6] = [400, 500, 600, 650, 1500, 1650];
        let mut done = 0;
        for font in FACES {
            if !self.ensure_font(font) {
                continue;
            }
            for c in ' '..='~' {
                if self.glyphs.contains_key(&(c, font)) {
                    continue;
                }
                if done == budget {
                    return true;
                }
                if let Ok(geometry) = self.fonts[&font].geometry(c) {
                    self.glyphs.insert((c, font), Glyph { geometry, frame: self.frame });
                } else {
                    self.glyphs.insert((c, font), Glyph { geometry: Vec::new(), frame: self.frame });
                }
                done += 1;
            }
        }
        false
    }

    fn trim_text_geometry(&mut self) {
        let glyph_bytes =
            |glyph: &Glyph| glyph.geometry.len() * size_of::<luma_vector_geometry::Vertex>();
        let mut bytes: usize = self.glyphs.values().map(glyph_bytes).sum();
        if bytes > GLYPH_GEOMETRY_BUDGET || self.glyphs.len() > 4096 {
            let mut old: Vec<_> = self
                .glyphs
                .iter()
                .filter(|(_, glyph)| glyph.frame != self.frame)
                .map(|(key, glyph)| (*key, glyph.frame))
                .collect();
            old.sort_by_key(|(_, frame)| *frame);
            for (key, _) in old {
                if bytes <= GLYPH_GEOMETRY_BUDGET && self.glyphs.len() <= 4096 {
                    break;
                }
                if let Some(glyph) = self.glyphs.remove(&key) {
                    bytes -= glyph_bytes(&glyph);
                }
            }
        }
        let run_bytes = |run: &TextRun| run.mesh.as_ref().map_or(0, |mesh| mesh.bytes);
        let mut bytes: usize = self.text_runs.values().map(run_bytes).sum();
        if bytes > TEXT_GEOMETRY_BUDGET || self.text_runs.len() > 512 {
            let mut old: Vec<_> = self
                .text_runs
                .iter()
                .filter(|(_, run)| run.frame != self.frame)
                .map(|(key, run)| (key.clone(), run.frame))
                .collect();
            old.sort_by_key(|(_, frame)| *frame);
            for (key, _) in old {
                if bytes <= TEXT_GEOMETRY_BUDGET && self.text_runs.len() <= 512 {
                    break;
                }
                if let Some(run) = self.text_runs.remove(&key) {
                    bytes -= run_bytes(&run);
                }
            }
        }
        self.stats.cached_glyphs = self.glyphs.len();
        self.stats.glyph_geometry_bytes = self.glyphs.values().map(glyph_bytes).sum();
        self.stats.cached_text_runs = self.text_runs.len();
        self.stats.text_geometry_bytes = self.text_runs.values().map(run_bytes).sum();
    }

    fn quad(
        &mut self,
        rect: Rect,
        uv: [f32; 4],
        radius: f32,
        border: f32,
        color: Color,
        texture: GLuint,
    ) {
        self.gradient_quad(rect, uv, radius, border, [color, color], texture);
    }

    fn gradient_quad(
        &mut self,
        rect: Rect,
        uv: [f32; 4],
        radius: f32,
        border: f32,
        colors: [Color; 2],
        texture: GLuint,
    ) {
        if rect.w <= 0. || rect.h <= 0. {
            return;
        }
        // Large square-cornered fills (wallpaper, dims, photos) need no edge
        // coverage maths: route them to the shader's plain texture path.
        let radius = if radius <= 0. && border == 0. && rect.w >= 64. && rect.h >= 64. {
            -1.
        } else {
            radius
        };
        let start = self.vertices.len() / 14;
        let regions = regions(rect, radius, border);
        for &[x0, y0, x1, y1] in &regions {
            for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x0, y1), (x1, y0), (x1, y1)] {
                let color: [f32; 4] = std::array::from_fn(|i| colors[0][i] + (colors[1][i] - colors[0][i]) * y);
                self.vertices.extend_from_slice(&[
                    rect.x + x * rect.w,
                    rect.y + y * rect.h,
                    uv[0] + x * (uv[2] - uv[0]),
                    uv[1] + y * (uv[3] - uv[1]),
                    color[0],
                    color[1],
                    color[2],
                    color[3],
                    (x - 0.5) * rect.w,
                    (y - 0.5) * rect.h,
                    rect.w / 2.,
                    rect.h / 2.,
                    radius,
                    border,
                ]);
            }
        }
        let emitted = regions.len() * 6;
        if let Some(batch) = self.batches.last_mut() {
            let same_clip = match (batch.clip, self.clip) {
                (None, None) => true,
                (Some(a), Some(b)) => a.x == b.x && a.y == b.y && a.w == b.w && a.h == b.h,
                _ => false,
            };
            if same_clip
                && batch.fade == self.fade
                && batch.rank == self.rank
                && let BatchKind::Paint {
                    texture: previous,
                    count,
                    ..
                } = &mut batch.kind
                && *previous == texture
            {
                *count += emitted;
                return;
            }
        }
        self.batches.push(Batch {
            kind: BatchKind::Paint {
                texture,
                start,
                count: emitted,
            },
            clip: self.clip,
            fade: self.fade,
            rank: self.rank,
        });
    }

    fn text(&mut self, text: &str, rect: Rect, style: TextStyle, color: Color, centered: bool) {
        // Layout remains logical. Reuse the same run geometry at every density;
        // its shader snaps insertion origins and computes pixel-sized coverage.
        let size_f = style.size;
        let viewport = self.stats.viewport;
        self.stats.max_logical_font_px = self.stats.max_logical_font_px.max(size_f);
        self.stats.max_physical_font_px = self
            .stats
            .max_physical_font_px
            .max(size_f * viewport.text_scale());
        let padded = rect.inset(-size_f);
        if padded.x + padded.w < 0.
            || padded.y + padded.h < 0.
            || padded.x > 1920.
            || padded.y > 1080.
            || self.clip.is_some_and(|clip| {
                padded.x + padded.w < clip.x
                    || padded.y + padded.h < clip.y
                    || padded.x > clip.x + clip.w
                    || padded.y > clip.y + clip.h
            })
        {
            return;
        }
        let origin = [rect.x, rect.y];
        let key = TextKey {
            text: text.into(),
            weight: style.weight,
            layout: [
                size_f.to_bits(),
                style.tracking.to_bits(),
                style.line_height.to_bits(),
                rect.w.to_bits(),
                rect.h.to_bits(),
            ],
            flags: u8::from(style.align_right)
                | (u8::from(centered) << 1)
                | (u8::from(self.stats.multisample_samples > 1) << 2),
        };
        if self.text_runs.contains_key(&key) {
            self.submit_text(key, origin, color);
            return;
        }
        let rect = Rect::new(0., 0., rect.w, rect.h);
        let font_id = style.weight + if style.size >= 32. { 1000 } else { 0 };
        if !self.ensure_font(font_id) {
            self.stats.glyph_failures += text.chars().count();
            return;
        }
        let font = &self.fonts[&font_id];
        let line_height = size_f * style.line_height;
        let baseline = font.horizontal_line_metrics(size_f).map_or(size_f, |m| {
            (line_height - m.ascent + m.descent) / 2. + m.ascent
        });
        let tracking = style.tracking * size_f;
        let advance = |c| {
            if c == '\n' {
                0.
            } else {
                font.metrics(c, size_f).advance_width + tracking
            }
        };
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        let mut width = 0.;
        // Word wrapping, with character fallback for a title wider than the tile.
        for word in text.split_inclusive(char::is_whitespace) {
            let word_width: f32 = word.chars().map(advance).sum();
            if width > 0. && width + word_width > rect.w {
                lines.push(line.trim_end().into());
                line.clear();
                width = 0.;
            }
            for c in word.chars() {
                if c == '\n' {
                    lines.push(line.trim_end().into());
                    line.clear();
                    width = 0.;
                    continue;
                }
                let w = advance(c);
                if width > 0. && width + w > rect.w {
                    lines.push(line.trim_end().into());
                    line.clear();
                    width = 0.;
                }
                line.push(c);
                width += w;
            }
        }
        if !line.is_empty() {
            lines.push(line.trim_end().into());
        }
        let mut placed = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            let top = rect.y + index as f32 * line_height;
            if index > 0 && top + line_height > rect.y + rect.h + 1. {
                break;
            }
            let mut previous = None;
            let width: f32 = line
                .chars()
                .map(|c| {
                    let kern = previous
                        .and_then(|p| font.horizontal_kern(p, c, size_f))
                        .unwrap_or(0.);
                    previous = Some(c);
                    advance(c) + kern
                })
                .sum();
            let mut x = rect.x
                + if style.align_right {
                    rect.w - width
                } else if centered {
                    (rect.w - width) / 2.
                } else {
                    0.
                };
            let mut previous = None;
            for c in line.chars() {
                if let Some(previous) = previous {
                    x += font.horizontal_kern(previous, c, size_f).unwrap_or(0.);
                }
                placed.push((c, x, top + baseline));
                x += advance(c);
                previous = Some(c);
            }
        }
        let mut vertices = Vec::new();
        let mut missing_glyphs = 0;
        let mut failures = 0;
        let multisampled = self.stats.multisample_samples > 1;
        for (c, x, baseline) in placed {
            if self.fonts[&font_id].lookup_glyph_index(c) == 0 && !c.is_whitespace() {
                missing_glyphs += 1;
            }
            match self.glyph(c, font_id) {
                Ok(geometry) => {
                    for triangle in geometry.as_chunks::<3>().0 {
                        // True sample coverage needs only the original fill.
                        // Exclude the boundary fringe, not just its alpha.
                        if multisampled && triangle.iter().any(|vertex| vertex.coverage < 1.) {
                            continue;
                        }
                        for vertex in triangle {
                            vertices.extend_from_slice(&[
                                vertex.position.x * size_f,
                                vertex.position.y * size_f,
                                if multisampled { 0. } else { vertex.offset[0] },
                                if multisampled { 0. } else { vertex.offset[1] },
                                x,
                                baseline,
                                0.,
                                0.,
                                vertex.coverage,
                            ]);
                        }
                    }
                }
                Err(error) => {
                    eprintln!("Luma glyph {c:?}: {error}");
                    failures += 1;
                }
            }
        }
        let mesh = if vertices.is_empty() {
            None
        } else {
            match vector::Mesh::upload(1., 1., &vertices) {
                Ok(mesh) => Some(mesh),
                Err(error) => {
                    eprintln!("Luma text: {error}");
                    failures += 1;
                    None
                }
            }
        };
        self.text_runs.insert(
            key.clone(),
            TextRun {
                mesh,
                missing_glyphs,
                failures,
                frame: self.frame,
            },
        );
        self.submit_text(key, origin, color);
    }

    fn submit_text(&mut self, key: TextKey, origin: [f32; 2], tint: Color) {
        let run = self.text_runs.get_mut(&key).unwrap();
        run.frame = self.frame;
        self.stats.missing_glyphs += run.missing_glyphs;
        self.stats.glyph_failures += run.failures;
        if let Some(mesh) = &run.mesh {
            self.stats.text_triangles += mesh.vertices as usize / 3;
            self.batches.push(Batch {
                kind: BatchKind::Text { key, origin, tint },
                clip: self.clip,
                fade: self.fade,
                rank: self.rank,
            });
        }
    }

    fn image(&mut self, path: &str, mut rect: Rect, radius: f32, contain: bool, tint: Color) {
        if path.ends_with(".lvec") {
            self.vector_image(path, rect, contain, tint);
            return;
        }
        let path = if self.stats.viewport.scale > 1.
            && self.info.max_texture_size >= 3840
            && Path::new(path)
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|p| p == "wallpapers")
        {
            self.artwork_variants
                .entry(path.into())
                .or_insert_with(|| {
                    let original = Path::new(path);
                    let variant = original.with_file_name(format!(
                        "{}@2x.jpg",
                        original.file_stem().unwrap_or_default().to_string_lossy()
                    ));
                    if variant.is_file() {
                        variant.to_string_lossy().into_owned()
                    } else {
                        path.into()
                    }
                })
                .clone()
        } else {
            path.into()
        };
        if path.ends_with("@2x.jpg") {
            self.stats.high_resolution_wallpaper = true;
        }
        let frame = self.frame;
        let astc = self.astc;
        let entry = self
            .textures
            .entry(path.clone())
            .or_insert_with(|| load_texture(astc, &path));
        let Some(texture) = entry else {
            self.stats.missing_images += 1;
            return;
        };
        texture.frame = frame;
        let id = texture.id;
        if contain {
            let scale = (rect.w / texture.width as f32).min(rect.h / texture.height as f32);
            let w = texture.width as f32 * scale;
            let h = texture.height as f32 * scale;
            rect = Rect::new(rect.x + (rect.w - w) / 2., rect.y + (rect.h - h) / 2., w, h);
        }
        self.quad(rect, [0., 0., 1., 1.], radius, 0., tint, id);
    }

    /// A decoded, uploaded image by path (cached), marked used this frame.
    fn loaded(&mut self, path: &str) -> Option<GLuint> {
        let frame = self.frame;
        let astc = self.astc;
        let entry = self
            .textures
            .entry(path.into())
            .or_insert_with(|| load_texture(astc, path));
        let Some(texture) = entry else {
            self.stats.missing_images += 1;
            return None;
        };
        texture.frame = frame;
        Some(texture.id)
    }

    fn dissolve(&mut self, from: &str, to: &str, amount: f32, tint: Color) {
        let (Some(from), Some(to)) = (self.loaded(from), self.loaded(to)) else {
            return;
        };
        let start = self.vertices.len() / 14;
        self.quad(
            Rect::new(0., 0., 1920., 1080.),
            [0., 0., 1., 1.],
            0.,
            0.,
            [1.; 4],
            from,
        );
        // quad() may have merged into an earlier batch; give these six
        // vertices a batch of their own.
        if let Some(batch) = self.batches.last_mut()
            && let BatchKind::Paint { count, .. } = &mut batch.kind
            && *count > 6
        {
            *count -= 6;
        } else {
            self.batches.pop();
        }
        self.batches.push(Batch {
            kind: BatchKind::Dissolve {
                from,
                to,
                start,
                amount,
                tint,
            },
            clip: None,
            fade: None,
            rank: self.rank,
        });
    }

    fn glass(&mut self, path: &str, rect: Rect, radius: f32, tint: [Color; 2]) {
        let frame = self.frame;
        let astc = self.astc;
        let entry = self
            .textures
            .entry(path.into())
            .or_insert_with(|| load_texture(astc, path));
        let Some(texture) = entry else {
            self.stats.missing_images += 1;
            return;
        };
        texture.frame = frame;
        let id = texture.id;
        let uv = [
            rect.x / 1920.,
            rect.y / 1080.,
            (rect.x + rect.w) / 1920.,
            (rect.y + rect.h) / 1080.,
        ];
        self.gradient_quad(rect, uv, radius, 0., tint, id);
    }

    fn vector_image(&mut self, path: &str, mut rect: Rect, contain: bool, tint: Color) {
        if rect.w <= 0. || rect.h <= 0. {
            return;
        }
        let mesh = self.vectors.entry(path.into()).or_insert_with(|| {
            match vector::Mesh::load(Path::new(path)) {
                Ok(mesh) => Some(mesh),
                Err(error) => {
                    eprintln!("{error}");
                    None
                }
            }
        });
        let Some(mesh) = mesh else {
            self.stats.missing_images += 1;
            return;
        };
        if contain {
            let scale = (rect.w / mesh.width).min(rect.h / mesh.height);
            let width = mesh.width * scale;
            let height = mesh.height * scale;
            rect = Rect::new(
                rect.x + (rect.w - width) / 2.,
                rect.y + (rect.h - height) / 2.,
                width,
                height,
            );
        }
        let clip = if let Some(clip) = self.clip {
            let x = rect.x.max(clip.x);
            let y = rect.y.max(clip.y);
            Rect::new(
                x,
                y,
                (rect.x + rect.w).min(clip.x + clip.w).max(x) - x,
                (rect.y + rect.h).min(clip.y + clip.h).max(y) - y,
            )
        } else {
            rect
        };
        if clip.w <= 0. || clip.h <= 0. {
            return;
        }
        self.stats.vector_triangles += mesh.vertices as usize / 3;
        self.batches.push(Batch {
            kind: BatchKind::Vector {
                path: path.into(),
                rect,
                tint,
            },
            clip: Some(clip),
            fade: self.fade,
            rank: self.rank,
        });
    }

    fn app_image(&mut self, image: &crate::app_artwork::Artwork, rect: Rect, tint: Color) {
        let entry = self.textures.entry(image.key.clone()).or_insert_with(|| {
            Some(texture(
                image.pixels.width(),
                image.pixels.height(),
                image.pixels.as_raw(),
            ))
        });
        let Some(texture) = entry else {
            return;
        };
        texture.frame = self.frame;
        let scale = (rect.w / texture.width as f32).min(rect.h / texture.height as f32);
        let w = texture.width as f32 * scale;
        let h = texture.height as f32 * scale;
        let id = texture.id;
        self.quad(
            Rect::new(rect.x + (rect.w - w) / 2., rect.y + (rect.h - h) / 2., w, h),
            [0., 0., 1., 1.],
            0.,
            0.,
            tint,
            id,
        );
    }

    pub fn render(&mut self, commands: &[Draw], width: u32, height: u32) {
        let started = Instant::now();
        // The peak outlives each frame's fresh stats.
        let peak_fill = std::mem::take(&mut self.stats.peak_fill);
        self.stats = RenderStats::new(Size::new(width, height));
        self.stats.peak_fill = peak_fill;
        // Captures can bind a single-sample FBO even when the actual TV window
        // is multisampled, so query the active target rather than assume.
        let mut samples = 0;
        unsafe { gl::GetIntegerv(gl::SAMPLES, &mut samples) };
        self.stats.multisample_samples = samples.max(0) as u32;
        if samples > 1 {
            self.stats.font_antialiasing = "multisample vector coverage";
        }
        self.stencil = self.stencil_bits() > 0;
        self.frame += 1;
        self.vertices.clear();
        self.batches.clear();
        self.clip = None;
        self.fade = None;
        let skip = |kind: &str| self.skip.iter().any(|s| s == kind);
        let (no_text, no_shadow, no_vector, no_image, no_shape, no_glass, no_dissolve) = (
            skip("text"),
            skip("shadow"),
            skip("vector"),
            skip("image"),
            skip("shape"),
            skip("glass"),
            skip("dissolve"),
        );
        let mut fill: std::collections::BTreeMap<&'static str, f32> = Default::default();
        // Opaque tiles in paint order: each draw skips the pixels of every
        // tile painted after it, which cover it anyway.
        self.find_occluders(commands);
        let mut rank = 0u8;
        for (index, command) in commands.iter().enumerate() {
            if self.occluder_at.get(index).copied().unwrap_or(false) {
                rank += 1;
            }
            self.rank = rank;
            let skipped = match command {
                Draw::Text(..) => no_text,
                Draw::Shadow(..) => no_shadow,
                Draw::Image(path, ..) | Draw::TintedImage(path, ..) => {
                    if path.ends_with(".lvec") {
                        no_vector
                    } else {
                        no_image
                    }
                }
                Draw::AppImage(..) => no_image,
                Draw::Glass(..) => no_image || no_glass,
                Draw::Dissolve(..) => no_image || no_glass || no_dissolve,
                Draw::Shape(..) | Draw::RoundedGradient(..) | Draw::GradientRing(..) => no_shape,
                _ => false,
            };
            if skipped {
                continue;
            }
            let painted = match command {
                Draw::Shape(rect, radius, width, _) | Draw::GradientRing(rect, radius, width, ..) if *width > 0. => {
                    for [x0, y0, x1, y1] in regions(*rect, *radius, *width) {
                        let band = Rect::new(rect.x + x0 * rect.w, rect.y + y0 * rect.h, (x1 - x0) * rect.w, (y1 - y0) * rect.h);
                        *fill.entry("shape").or_insert(0.) += screens(band, self.clip);
                    }
                    None
                }
                Draw::Shape(rect, ..) | Draw::RoundedGradient(rect, ..) | Draw::GradientRing(rect, ..) => {
                    Some(("shape", *rect))
                }
                Draw::Shadow(rect, _, blur, _) => Some(("shadow", rect.inset(-*blur))),
                Draw::Text(_, rect, ..) => Some(("text", *rect)),
                Draw::Image(path, rect, ..) | Draw::TintedImage(path, rect, ..) => {
                    Some((if path.ends_with(".lvec") { "vector" } else { "image" }, *rect))
                }
                Draw::AppImage(_, rect, _) => Some(("image", *rect)),
                Draw::Glass(_, rect, ..) => Some(("glass", *rect)),
                Draw::Dissolve(..) => Some(("background", Rect::new(0., 0., 1920., 1080.))),
                Draw::Clip(_) | Draw::EdgeFade(_) => None,
            };
            if let Some((kind, rect)) = painted {
                *fill.entry(kind).or_insert(0.) += screens(rect, self.clip);
            }
            match command {
                Draw::Shape(rect, radius, border, color) => self.quad(
                    *rect,
                    [0., 0., 1., 1.],
                    *radius,
                    *border,
                    *color,
                    self.white.id,
                ),
                Draw::RoundedGradient(rect, radius, top, bottom) => self.gradient_quad(
                    *rect,
                    [0., 0., 1., 1.],
                    *radius,
                    0.,
                    [*top, *bottom],
                    self.white.id,
                ),
                Draw::GradientRing(rect, radius, width, top, bottom) => self.gradient_quad(
                    *rect,
                    [0., 0., 1., 1.],
                    *radius,
                    *width,
                    [*top, *bottom],
                    self.white.id,
                ),
                Draw::Shadow(rect, radius, blur, color) => self.quad(
                    rect.inset(-*blur),
                    [0., 0., 1., 1.],
                    *radius,
                    -*blur,
                    *color,
                    self.white.id,
                ),
                Draw::Text(text, rect, size, color, centered) => {
                    self.text(text, *rect, *size, *color, *centered)
                }
                Draw::Image(path, rect, radius, contain) => {
                    self.image(path, *rect, *radius, *contain, [1.; 4])
                }
                Draw::TintedImage(path, rect, radius, contain, tint) => {
                    self.image(path, *rect, *radius, *contain, *tint)
                }
                Draw::AppImage(image, rect, tint) => self.app_image(image, *rect, *tint),
                Draw::Glass(path, rect, radius, top, bottom) => self.glass(path, *rect, *radius, [*top, *bottom]),
                Draw::Dissolve(from, to, amount, tint) => self.dissolve(from, to, *amount, *tint),
                Draw::Clip(clip) => self.clip = *clip,
                Draw::EdgeFade(fade) => self.fade = *fade,
            }
        }
        self.rank = u8::MAX;
        // The occluders as plain quads (no corner maths), drawn first into
        // the stencil only.
        let occluder_start = self.vertices.len() / 14;
        for rect in &self.occluders {
            for (x, y) in [(0., 0.), (1., 0.), (0., 1.), (0., 1.), (1., 0.), (1., 1.)] {
                self.vertices.extend_from_slice(&[
                    rect.x + x * rect.w,
                    rect.y + y * rect.h,
                    0.,
                    0.,
                    1.,
                    1.,
                    1.,
                    1.,
                    0.,
                    0.,
                    rect.w / 2.,
                    rect.h / 2.,
                    -1.,
                    0.,
                ]);
            }
        }
        let total = |fill: &std::collections::BTreeMap<&'static str, f32>| fill.values().sum::<f32>();
        if total(&fill) > total(&self.stats.peak_fill) {
            self.stats.peak_fill = fill.clone();
        }
        self.stats.fill = fill;
        self.stats.occluders = self.occluders.len();
        self.stats.occluded = self.occluders.iter().map(|rect| screens(*rect, None)).sum();
        self.stats.cpu_build_ms = started.elapsed().as_secs_f64() * 1000.;
        let Viewport {
            x: vx,
            y: vy,
            width: vw,
            height: vh,
            scale,
        } = self.stats.viewport;
        // Logical area to pixels (bottom-left origin) for the scissor.
        let scissor = |area: Rect| {
            (
                vx + (area.x * vw as f32 / 1920.).floor() as i32,
                vy + ((1080. - area.y - area.h) * vh as f32 / 1080.).floor() as i32,
                (area.w * vw as f32 / 1920.).ceil() as i32 + 1,
                (area.h * vh as f32 / 1080.).ceil() as i32 + 1,
            )
        };
        let damage = self.damage;
        unsafe {
            gl::Viewport(0, 0, width as i32, height as i32);
            gl::ClearColor(0.04, 0.04, 0.05, 1.);
            match damage {
                Some(area) => {
                    let (x, y, w, h) = scissor(area);
                    gl::Enable(gl::SCISSOR_TEST);
                    gl::Scissor(x, y, w, h);
                }
                None => gl::Disable(gl::SCISSOR_TEST),
            }
            if self.occluders.is_empty() {
                gl::Clear(gl::COLOR_BUFFER_BIT);
            } else {
                gl::ClearStencil(0);
                gl::Clear(gl::COLOR_BUFFER_BIT | gl::STENCIL_BUFFER_BIT);
            }
            gl::Viewport(vx, vy, vw, vh);
            if self.vao != 0 {
                gl::BindVertexArray(self.vao);
            }
            gl::BindBuffer(gl::ARRAY_BUFFER, self.buffer);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (self.vertices.len() * size_of::<f32>()) as isize,
                self.vertices.as_ptr().cast(),
                gl::DYNAMIC_DRAW,
            );
            gl::UseProgram(self.program);
            // Shape edges blend over 1 logical px: the TV scales its 1080p
            // frame up to the 4K panel, which turned 0.85 px into a visible
            // stair-step on rims and tight curves, and 1.25 px into a blur.
            gl::Uniform1f(self.paint_uniforms.aa, 1.0 / scale.max(0.01));
            gl::Uniform1i(self.paint_uniforms.texture, 0);
            gl::ActiveTexture(gl::TEXTURE0);
            let mut paint_bound = false;
            if !self.occluders.is_empty() {
                self.bind_paint();
                paint_bound = true;
                gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
                gl::Enable(gl::STENCIL_TEST);
                gl::StencilOp(gl::KEEP, gl::KEEP, gl::REPLACE);
                gl::BindTexture(gl::TEXTURE_2D, self.white.id);
                // In paint order, each tile's rank: a pixel ends up holding
                // the rank of the last tile that covers it.
                for index in 0..self.occluders.len() {
                    gl::StencilFunc(gl::ALWAYS, index as i32 + 1, 0xff);
                    gl::DrawArrays(gl::TRIANGLES, (occluder_start + index * 6) as i32, 6);
                }
                gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
                gl::StencilOp(gl::KEEP, gl::KEEP, gl::KEEP);
            }
            let mut current_fade = None;
            for batch in &self.batches {
                if !self.occluders.is_empty() {
                    // Drawn where no later tile covers it: rank >= stencil.
                    if (batch.rank as usize) < self.occluders.len() {
                        gl::Enable(gl::STENCIL_TEST);
                        gl::StencilFunc(gl::GEQUAL, batch.rank as i32, 0xff);
                    } else {
                        gl::Disable(gl::STENCIL_TEST);
                    }
                }
                if current_fade != Some(batch.fade) {
                    // The same cheap alpha mask applies to shapes, textures,
                    // icons and text; the unmasked wallpaper remains beneath.
                    let fade = batch.fade.unwrap_or([0., 0., 0., 0.]);
                    for (program, uniforms) in [
                        (self.program, self.paint_uniforms),
                        (self.vector_program, self.vector_uniforms),
                        (self.text_program, self.text_uniforms),
                    ] {
                        gl::UseProgram(program);
                        gl::Uniform4f(uniforms.edge_fade, fade[0], fade[1], fade[2], fade[3]);
                    }
                    current_fade = Some(batch.fade);
                    paint_bound = false;
                }
                // The batch's own clip, within this frame's repainted area.
                let area = match (batch.clip, damage) {
                    (Some(clip), Some(area)) => {
                        let (x, y) = (clip.x.max(area.x), clip.y.max(area.y));
                        Some(Rect::new(
                            x,
                            y,
                            ((clip.x + clip.w).min(area.x + area.w) - x).max(0.),
                            ((clip.y + clip.h).min(area.y + area.h) - y).max(0.),
                        ))
                    }
                    (clip, area) => clip.or(area),
                };
                if let Some(area) = area {
                    let (x, y, w, h) = if batch.clip.is_some() && damage.is_none() {
                        (
                            vx + (area.x * vw as f32 / 1920.).round() as i32,
                            vy + ((1080. - area.y - area.h) * vh as f32 / 1080.).round() as i32,
                            (area.w * vw as f32 / 1920.).round() as i32,
                            (area.h * vh as f32 / 1080.).round() as i32,
                        )
                    } else {
                        scissor(area)
                    };
                    gl::Enable(gl::SCISSOR_TEST);
                    gl::Scissor(x, y, w, h);
                } else {
                    gl::Disable(gl::SCISSOR_TEST);
                }
                match &batch.kind {
                    BatchKind::Paint {
                        texture,
                        start,
                        count,
                    } => {
                        if !paint_bound {
                            self.bind_paint();
                            paint_bound = true;
                        }
                        gl::BindTexture(gl::TEXTURE_2D, *texture);
                        gl::DrawArrays(gl::TRIANGLES, *start as i32, *count as i32);
                    }
                    BatchKind::Dissolve {
                        from,
                        to,
                        start,
                        amount,
                        tint,
                    } => {
                        // Same vertex layout as paint; the output is opaque, so
                        // blending is off for this one full-screen pass.
                        gl::UseProgram(self.dissolve_program);
                        gl::BindBuffer(gl::ARRAY_BUFFER, self.buffer);
                        let mut offset = 0;
                        for (index, size) in [2, 2, 4, 4, 2].into_iter().enumerate() {
                            gl::EnableVertexAttribArray(index as u32);
                            gl::VertexAttribPointer(
                                index as u32,
                                size,
                                gl::FLOAT,
                                gl::FALSE,
                                56,
                                (offset * size_of::<f32>()) as *const _,
                            );
                            offset += size as usize;
                        }
                        let [texture, texture2, mix, tint2] = self.dissolve_uniforms;
                        gl::Uniform1i(texture, 0);
                        gl::Uniform1i(texture2, 1);
                        gl::Uniform1f(mix, *amount);
                        gl::Uniform4f(tint2, tint[0], tint[1], tint[2], tint[3]);
                        gl::ActiveTexture(gl::TEXTURE1);
                        gl::BindTexture(gl::TEXTURE_2D, *to);
                        gl::ActiveTexture(gl::TEXTURE0);
                        gl::BindTexture(gl::TEXTURE_2D, *from);
                        gl::Disable(gl::BLEND);
                        gl::DrawArrays(gl::TRIANGLES, *start as i32, 6);
                        gl::Enable(gl::BLEND);
                        gl::UseProgram(self.program);
                        paint_bound = false;
                    }
                    BatchKind::Vector { path, rect, tint } => {
                        let Some(mesh) = self.vectors.get(path).and_then(Option::as_ref) else {
                            continue;
                        };
                        self.draw_mesh(
                            self.vector_program,
                            mesh,
                            [rect.w / mesh.width, rect.h / mesh.height, rect.x, rect.y],
                            *tint,
                        );
                        paint_bound = false;
                    }
                    BatchKind::Text { key, origin, tint } => {
                        let Some(mesh) = self.text_runs.get(key).and_then(|run| run.mesh.as_ref())
                        else {
                            continue;
                        };
                        self.draw_mesh(
                            self.text_program,
                            mesh,
                            [1., 1., origin[0], origin[1]],
                            *tint,
                        );
                        paint_bound = false;
                    }
                }
            }
            gl::Disable(gl::SCISSOR_TEST);
        }
        // Keep the current scene, evict old full-resolution wallpapers before memory grows.
        let mut bytes: usize = self
            .textures
            .values()
            .flatten()
            .map(|t| t.width as usize * t.height as usize * 4)
            .sum();
        if bytes > TEXTURE_BUDGET {
            let mut old: Vec<_> = self
                .textures
                .iter()
                .filter_map(|(path, t)| {
                    t.as_ref()
                        .filter(|t| t.frame != self.frame)
                        .map(|t| (path.clone(), t.frame))
                })
                .collect();
            old.sort_by_key(|(_, frame)| *frame);
            for (path, _) in old {
                if bytes <= TEXTURE_BUDGET {
                    break;
                }
                if let Some(Some(t)) = self.textures.remove(&path) {
                    bytes -= t.width as usize * t.height as usize * 4;
                }
            }
        }
        self.trim_text_geometry();
        self.stats.vector_meshes = self.vectors.values().flatten().count();
        self.stats.vector_bytes = self.vectors.values().flatten().map(|mesh| mesh.bytes).sum();
        self.stats.artwork_bytes = bytes + self.stats.vector_bytes;
        self.stats.draw_calls = self.batches.len();
        self.stats.texture_uploads = TEXTURE_UPLOADS.load(std::sync::atomic::Ordering::Relaxed);
        self.stats.cpu_submission_ms = started.elapsed().as_secs_f64() * 1000.;
    }

    fn draw_mesh(&self, program: GLuint, mesh: &vector::Mesh, transform: [f32; 4], tint: Color) {
        let viewport = self.stats.viewport;
        let uniforms = if program == self.text_program {
            self.text_uniforms
        } else {
            self.vector_uniforms
        };
        unsafe {
            gl::UseProgram(program);
            if program == self.text_program {
                gl::Uniform1f(
                    uniforms.fringe,
                    if self.stats.multisample_samples > 1 {
                        0.
                    } else {
                        0.5
                    },
                );
            }
            gl::BindBuffer(gl::ARRAY_BUFFER, mesh.buffer);
            gl::DisableVertexAttribArray(4);
            let mut offset = 0;
            for (index, size) in [2, 2, 4, 1].into_iter().enumerate() {
                gl::EnableVertexAttribArray(index as u32);
                gl::VertexAttribPointer(
                    index as u32,
                    size,
                    gl::FLOAT,
                    gl::FALSE,
                    36,
                    (offset * size_of::<f32>()) as *const _,
                );
                offset += size as usize;
            }
            gl::Uniform4f(
                uniforms.transform,
                transform[0],
                transform[1],
                transform[2],
                transform[3],
            );
            gl::Uniform2f(
                uniforms.pixel_scale,
                viewport.width as f32 / 1920.,
                viewport.height as f32 / 1080.,
            );
            gl::Uniform4f(uniforms.tint, tint[0], tint[1], tint[2], tint[3]);
            gl::DrawArrays(gl::TRIANGLES, 0, mesh.vertices);
        }
    }

    /// Drops every cached texture, mesh and text run (GPU objects free on drop)
    /// so a launched app gets the memory. Everything reloads lazily on return.
    /// While an app runs: keep the home's working set (everything drawn in
    /// the last frame, plus every app tile and icon) so the home returns whole
    /// on its first frame; free the rest and the per-frame buffers.
    /// While another app is in front: keep only what the home showed last
    /// (it comes back instantly) and installed apps' icons (costly to fetch
    /// again). Off-screen tiles go; they are queued again on return.
    pub fn trim(&mut self) {
        let frame = self.frame;
        self.textures.retain(|path, texture| {
            texture.as_ref().is_some_and(|texture| texture.frame == frame)
                || path.starts_with("installed-icon:")
        });
        self.artwork_variants = HashMap::new();
        self.vertices = Vec::new();
        self.batches = Vec::new();
    }

    /// Upload a vector mesh read off the render thread, unless already loaded.
    pub fn adopt_vector(&mut self, path: String, bytes: &[u8]) {
        if let std::collections::hash_map::Entry::Vacant(slot) = self.vectors.entry(path) {
            let mesh = vector::Mesh::from_bytes(Path::new(slot.key()), bytes).ok();
            slot.insert(mesh);
        }
    }

    /// Whether this image is already on the GPU.
    pub fn has(&self, path: &str) -> bool {
        self.textures.get(path).is_some_and(Option::is_some)
    }

    /// Whether ASTC copies are used (the preloader reads them instead of decoding).
    pub fn takes_astc(&self) -> bool {
        self.astc
    }

    /// Upload an image's ASTC copy read off the render thread, unless loaded.
    pub fn adopt_compressed(&mut self, path: String, image: &Compressed) {
        let frame = self.frame;
        self.textures.entry(path).or_insert_with(|| {
            let mut texture = compressed_texture(image);
            texture.frame = frame;
            Some(texture)
        });
    }

    /// Upload an image decoded off the render thread, unless already loaded.
    pub fn adopt(&mut self, path: String, image: image::RgbaImage) {
        let frame = self.frame;
        self.textures.entry(path).or_insert_with(|| {
            let mut texture = texture(image.width(), image.height(), image.as_raw());
            texture.frame = frame;
            Some(texture)
        });
    }

    pub fn stats(&self) -> &RenderStats {
        &self.stats
    }

    /// Selects the paint program with its vertex layout.
    ///
    /// # Safety
    /// Needs this renderer's context current and its vertex buffer filled.
    unsafe fn bind_paint(&self) {
        unsafe {
            gl::UseProgram(self.program);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.buffer);
            let mut offset = 0;
            for (index, size) in [2, 2, 4, 4, 2].into_iter().enumerate() {
                gl::EnableVertexAttribArray(index as u32);
                gl::VertexAttribPointer(
                    index as u32,
                    size,
                    gl::FLOAT,
                    gl::FALSE,
                    56,
                    (offset * size_of::<f32>()) as *const _,
                );
                offset += size as usize;
            }
        }
    }

    /// Stencil bits of the bound framebuffer (0 when it has none).
    fn stencil_bits(&self) -> i32 {
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
                let attachment = if bound == 0 { gl::STENCIL } else { gl::STENCIL_ATTACHMENT };
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
    fn find_occluders(&mut self, commands: &[Draw]) {
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
                Draw::RoundedGradient(rect, radius, top, bottom) if top[3] >= 0.999 && bottom[3] >= 0.999 => {
                    Some((*rect, *radius))
                }
                // Glass at full strength is a solid picture of the backdrop.
                Draw::Glass(_, rect, radius, top, bottom) if top[3] >= 0.999 && bottom[3] >= 0.999 => {
                    Some((*rect, *radius))
                }
                // Tile artwork is full-bleed and opaque (only its corners are cut).
                Draw::Image(path, rect, radius, false) if path.contains("/tiles/") => Some((*rect, *radius)),
                Draw::TintedImage(path, rect, radius, false, tint) if path.contains("/tiles/") && tint[3] >= 0.999 => {
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

    pub fn screenshot(&self, path: &Path, width: u32, height: u32) -> Result<(), String> {
        let mut pixels = vec![0; width as usize * height as usize * 4];
        unsafe {
            gl::ReadPixels(
                0,
                0,
                width as i32,
                height as i32,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        let mut image = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or("Invalid framebuffer dimensions")?;
        image::imageops::flip_vertical_in_place(&mut image);
        image.save(path).map_err(|e| e.to_string())
    }

    /// Capture the authored TV surface, independent of desktop HiDPI/window limits.
    pub fn capture(
        &mut self,
        commands: &[Draw],
        path: &Path,
        size: Size,
    ) -> Result<RenderStats, String> {
        if size.width > self.info.max_texture_size
            || size.height > self.info.max_texture_size
            || size.width > self.info.max_viewport[0] as u32
            || size.height > self.info.max_viewport[1] as u32
        {
            return Err(format!(
                "Capture {}x{} exceeds GPU limits: {}px texture, {:?} viewport",
                size.width, size.height, self.info.max_texture_size, self.info.max_viewport
            ));
        }
        let image = texture(size.width, size.height, &[]);
        let mut framebuffer = 0;
        unsafe {
            gl::GenFramebuffers(1, &mut framebuffer);
            gl::BindFramebuffer(gl::FRAMEBUFFER, framebuffer);
            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                image.id,
                0,
            );
            if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                gl::DeleteFramebuffers(1, &framebuffer);
                return Err("Screenshot framebuffer is incomplete".into());
            }
        }
        // A stencil like the window's, so a capture draws exactly as the TV
        // does (opaque tiles masking what lies behind them). Without one the
        // capture still works; everything simply draws.
        let mut stencil = 0;
        unsafe {
            gl::GenRenderbuffers(1, &mut stencil);
            gl::BindRenderbuffer(gl::RENDERBUFFER, stencil);
            gl::RenderbufferStorage(gl::RENDERBUFFER, gl::STENCIL_INDEX8, size.width as i32, size.height as i32);
            gl::FramebufferRenderbuffer(gl::FRAMEBUFFER, gl::STENCIL_ATTACHMENT, gl::RENDERBUFFER, stencil);
            if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                gl::FramebufferRenderbuffer(gl::FRAMEBUFFER, gl::STENCIL_ATTACHMENT, gl::RENDERBUFFER, 0);
            }
            while gl::GetError() != gl::NO_ERROR {}
        }
        self.render(commands, size.width, size.height);
        let stats = self.stats.clone();
        let result = self.screenshot(path, size.width, size.height);
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::DeleteFramebuffers(1, &framebuffer);
            gl::DeleteRenderbuffers(1, &stencil);
        }
        result.map(|_| stats)
    }
}

fn load_font(directory: &Path, id: u16) -> Result<Font, String> {
    let name = if id >= 1000 {
        format!("font-display-{}.ttf", id - 1000)
    } else {
        format!("font-{id}.ttf")
    };
    let path = directory.join(&name);
    let started = Instant::now();
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("Font {}: {e}. Regenerate native assets.", path.display()))?;
    let font = Font::from_bytes(bytes).map_err(|e| format!("Font {}: {e}", path.display()))?;
    if cfg!(feature = "webos") {
        eprintln!(
            "Luma font: {name} loaded in {} ms",
            started.elapsed().as_millis()
        );
    }
    Ok(font)
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.buffer);
            gl::DeleteProgram(self.program);
            gl::DeleteProgram(self.vector_program);
            gl::DeleteProgram(self.text_program);
            if self.vao != 0 {
                gl::DeleteVertexArrays(1, &self.vao);
            }
        }
    }
}

/// GL_COMPRESSED_RGBA_ASTC_4x4_KHR.
const ASTC_4X4: u32 = 0x93B0;

/// An image's GPU-compressed copy, shipped beside it as `<path>.astc` (ASTC
/// 4x4, visually lossless): a quarter of the memory and of what the GPU reads
/// per frame, and no decoding at all.
pub struct Compressed {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Reads `<path>.astc`, accepting only a 4x4 2D file whose data matches its
/// header exactly.
pub fn read_astc(path: &str) -> Option<Compressed> {
    let bytes = std::fs::read(format!("{path}.astc")).ok()?;
    let header = bytes.get(..16)?;
    if header[..4] != [0x13, 0xab, 0xa1, 0x5c] || header[4..7] != [4, 4, 1] {
        return None;
    }
    let size = |at: usize| u32::from(header[at]) | u32::from(header[at + 1]) << 8 | u32::from(header[at + 2]) << 16;
    let (width, height, depth) = (size(7), size(10), size(13));
    let blocks = width.div_ceil(4) as usize * height.div_ceil(4) as usize;
    if depth != 1 || width == 0 || height == 0 || bytes.len() != 16 + blocks * 16 {
        return None;
    }
    Some(Compressed {
        width,
        height,
        data: bytes[16..].to_vec(),
    })
}

fn compressed_texture(image: &Compressed) -> Texture {
    TEXTURE_UPLOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut id = 0;
    unsafe {
        gl::GenTextures(1, &mut id);
        gl::BindTexture(gl::TEXTURE_2D, id);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::CompressedTexImage2D(
            gl::TEXTURE_2D,
            0,
            ASTC_4X4,
            image.width as i32,
            image.height as i32,
            0,
            image.data.len() as i32,
            image.data.as_ptr().cast(),
        );
    }
    Texture {
        id,
        width: image.width,
        height: image.height,
        frame: 0,
    }
}

/// An image's texture: its ASTC copy where the GPU takes ASTC, otherwise the
/// decoded original.
fn load_texture(astc: bool, path: &str) -> Option<Texture> {
    if astc && let Some(image) = read_astc(path) {
        return Some(compressed_texture(&image));
    }
    match image::open(path) {
        Ok(image) => {
            let image = image.into_rgba8();
            Some(texture(image.width(), image.height(), image.as_raw()))
        }
        Err(error) => {
            eprintln!("Artwork {path}: {error}");
            None
        }
    }
}

fn texture(width: u32, height: u32, pixels: &[u8]) -> Texture {
    TEXTURE_UPLOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut id = 0;
    unsafe {
        gl::GenTextures(1, &mut id);
        gl::BindTexture(gl::TEXTURE_2D, id);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as i32,
            width as i32,
            height as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            if pixels.is_empty() {
                std::ptr::null()
            } else {
                pixels.as_ptr().cast()
            },
        );
    }
    Texture {
        id,
        width,
        height,
        frame: 0,
    }
}

fn gl_string(name: GLenum) -> String {
    unsafe {
        let value = gl::GetString(name);
        if value.is_null() {
            "unknown".into()
        } else {
            CStr::from_ptr(value.cast()).to_string_lossy().into_owned()
        }
    }
}

fn compile(kind: GLenum, source: &str) -> Result<GLuint, String> {
    let source = CString::new(source).map_err(|e| e.to_string())?;
    unsafe {
        let shader = gl::CreateShader(kind);
        gl::ShaderSource(shader, 1, &source.as_ptr(), std::ptr::null());
        gl::CompileShader(shader);
        let mut success = 0;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
        if success == 0 {
            let mut log = vec![0u8; 4096];
            let mut len = 0;
            gl::GetShaderInfoLog(shader, log.len() as i32, &mut len, log.as_mut_ptr().cast());
            gl::DeleteShader(shader);
            return Err(format!(
                "Shader compilation: {}",
                String::from_utf8_lossy(&log[..len as usize])
            ));
        }
        Ok(shader)
    }
}

fn link(vertex: &str, fragment: &str, attributes: &[&CStr]) -> Result<GLuint, String> {
    let vertex = compile(gl::VERTEX_SHADER, vertex)?;
    let fragment = match compile(gl::FRAGMENT_SHADER, fragment) {
        Ok(fragment) => fragment,
        Err(error) => {
            unsafe {
                gl::DeleteShader(vertex);
            }
            return Err(error);
        }
    };
    unsafe {
        let program = gl::CreateProgram();
        gl::AttachShader(program, vertex);
        gl::AttachShader(program, fragment);
        for (index, name) in attributes.iter().enumerate() {
            gl::BindAttribLocation(program, index as u32, name.as_ptr());
        }
        gl::LinkProgram(program);
        gl::DeleteShader(vertex);
        gl::DeleteShader(fragment);
        let mut success = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);
        if success == 0 {
            let mut log = vec![0u8; 4096];
            let mut len = 0;
            gl::GetProgramInfoLog(program, log.len() as i32, &mut len, log.as_mut_ptr().cast());
            gl::DeleteProgram(program);
            return Err(format!(
                "Shader linking: {}",
                String::from_utf8_lossy(&log[..len as usize])
            ));
        }
        Ok(program)
    }
}

#[cfg(test)]
mod tests {
    use super::{Rect, regions};

    #[test]
    fn a_rim_draws_only_bands_that_cover_its_edge() {
        let dock = Rect::new(50., 740., 1820., 274.);
        let (radius, border) = (72., 1.5);
        let bands = regions(dock, radius, border);
        assert_eq!(bands.len(), 4);
        // Every pixel the rim (and its anti-aliasing) can touch lies in a
        // band: within the corner rounding of the top and bottom, or within
        // border + aa of the left and right sides.
        let inside = |fx: f32, fy: f32| {
            bands.iter().any(|[x0, y0, x1, y1]| fx >= *x0 && fx <= *x1 && fy >= *y0 && fy <= *y1)
        };
        let edge = (border + 2.5) / dock.w;
        for fy in [0.0, 0.1, 0.3, 0.5, 0.7, 0.9, 1.0] {
            assert!(inside(0.0, fy) && inside(edge, fy) && inside(1.0, fy) && inside(1.0 - edge, fy));
        }
        for fx in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!(inside(fx, (radius + border) / dock.h) && inside(fx, 1. - (radius + border) / dock.h));
        }
        // The middle, which the shader would only fill with nothing, is not drawn.
        assert!(!inside(0.5, 0.5));
        let area: f32 = bands.iter().map(|[x0, y0, x1, y1]| (x1 - x0) * (y1 - y0)).sum();
        assert!(area < 0.65);
    }

    #[test]
    fn fills_and_small_rims_draw_whole() {
        assert_eq!(regions(Rect::new(0., 0., 400., 200.), 30., 0.), vec![[0., 0., 1., 1.]]);
        // A capsule pill's rounding spans its height: no bands to save.
        assert_eq!(regions(Rect::new(0., 0., 360., 60.), 30., 1.5), vec![[0., 0., 1., 1.]]);
    }
}
