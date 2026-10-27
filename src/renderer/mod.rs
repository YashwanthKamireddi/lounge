//! A small GLES2-compatible GPU painter. Rust owns vector glyphs and the draw list;
//! SDL only supplies the context. No browser, LVGL or Qt runtime.
mod geometry;
mod occlusion;
mod shader;
mod stats;
mod text;
mod textures;

use crate::{
    font::Font,
    raster::{Size, Viewport},
    state::Rect,
    vector,
};
use geometry::{regions, screens};
use gl::types::*;
use serde::Serialize;
use shader::{gl_string, link};
pub use stats::{GraphicsInfo, RenderStats};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use text::load_font;
pub use textures::{Compressed, read_astc};
use textures::{compressed_texture, load_texture, texture};

pub type Color = [f32; 4];
pub const WHITE: Color = [0.96, 0.96, 0.96, 1.];
const GLYPH_GEOMETRY_BUDGET: usize = 8 * 1024 * 1024;
// Every screen holds under 6 MB of text runs; twice that bounds a runaway
// (a layout key changing every frame once held 24.8 MB) on a 1.3 GB TV.
const TEXT_GEOMETRY_BUDGET: usize = 12 * 1024 * 1024;
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
    /// nothing, the second by its colour, `f32` of the way to the second;
    /// then lifted the last colour's alpha of the way to its colour (the
    /// light appearance's frosted material, in the same pass).
    Dissolve(String, String, f32, Color, Color),
    Clip(Option<Rect>),
    EdgeFade(Option<[f32; 4]>),
    /// Rounded fills and pictures drawn while set get a soft edge this many
    /// logical px either side of their outline, in place of the crisp 1 px
    /// one. The TV shows its 1080p frame doubled to 4K, so a large curve
    /// (the pill, menus, avatars) steps visibly with a 1 px edge; text and
    /// tiles keep the crisp one.
    Soft(Option<f32>),
}

struct Texture {
    id: GLuint,
    width: u32,
    height: u32,
    /// What the GPU holds for it: 4 per pixel decoded, 1 per pixel as ASTC
    /// 4x4 (16-byte blocks), so the budget counts compressed wallpapers at
    /// their real size rather than four times it.
    bytes: usize,
    frame: u64,
}
impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.id);
        }
    }
}

/// A vector mesh (None: its file failed to load) and the frame it was last
/// drawn in.
struct VectorEntry {
    mesh: Option<vector::Mesh>,
    frame: u64,
}

struct Glyph {
    geometry: Vec<lounge_vector_geometry::Vertex>,
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
        lift: Color,
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
    embolden: GLint,
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
            embolden: at(c"u_embolden"),
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
    /// The dissolve's two-image mix, and its one-image variant for a mix of
    /// exactly 0 or 1 (see dissolve.frag).
    dissolve_programs: [GLuint; 2],
    dissolve_uniforms: [[GLint; 5]; 2],
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
    /// Which artwork each installed-icon texture was uploaded from (see
    /// Artwork::source), so a changed icon replaces its texture in place.
    installed_icons: HashMap<String, u64>,
    vectors: HashMap<String, VectorEntry>,
    artwork_variants: HashMap<String, String>,
    vertices: Vec<f32>,
    batches: Vec<Batch>,
    clip: Option<Rect>,
    fade: Option<[f32; 4]>,
    /// The soft edge set by Draw::Soft, for the draws being built now.
    soft: f32,
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
        let began = Instant::now();
        let mut fonts = HashMap::new();
        // Open only the common face; the others, and all metrics and
        // outlines, load lazily on first use.
        fonts.insert(400, load_font(directory, 400)?);
        let font_ms = began.elapsed().as_millis();
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
        let mut dissolve_programs = [0; 2];
        for (index, define) in ["", "#define DISSOLVE_SINGLE\n"].into_iter().enumerate() {
            dissolve_programs[index] = match link(
                &(vertex_prefix_dissolve.clone() + include_str!("shaders/paint.vert")),
                &(fragment_prefix_dissolve.clone()
                    + define
                    + include_str!("shaders/dissolve.frag")),
                &[c"a_position", c"a_uv", c"a_color", c"a_shape", c"a_params"],
            ) {
                Ok(program) => program,
                Err(error) => {
                    unsafe {
                        for program in [program, vector_program, text_program]
                            .into_iter()
                            .chain(dissolve_programs.into_iter().filter(|&p| p != 0))
                        {
                            gl::DeleteProgram(program);
                        }
                    }
                    return Err(error);
                }
            };
        }
        if cfg!(feature = "webos") {
            crate::platform::status(&format!(
                "startup: renderer font {} ms, shaders and setup {} ms",
                font_ms,
                began.elapsed().as_millis() - font_ms
            ));
        }
        // The one-image variant has no u_texture2/u_mix: their location is
        // -1, which GL ignores when set.
        let dissolve_uniforms = dissolve_programs.map(|program| {
            [c"u_texture", c"u_texture2", c"u_mix", c"u_tint2", c"u_lift"]
                .map(|name| unsafe { gl::GetUniformLocation(program, name.as_ptr()) })
        });
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
            dissolve_programs,
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
            installed_icons: HashMap::new(),
            vectors: HashMap::new(),
            artwork_variants: HashMap::new(),
            astc: gles
                && gl_string(gl::EXTENSIONS)
                    .split(' ')
                    .any(|name| name == "GL_KHR_texture_compression_astc_ldr"),
            vertices: Vec::with_capacity(65536),
            batches: Vec::new(),
            clip: None,
            fade: None,
            soft: 0.,
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
        // A soft edge (Draw::Soft) on a rounded fill or picture: the quad
        // grows by it and the shader's blur path, which shrinks it back,
        // ramps coverage across the same outline.
        let (rect, uv, border) = if self.soft > 0. && radius > 0. && border == 0. {
            let s = self.soft;
            let (du, dv) = ((uv[2] - uv[0]) / rect.w * s, (uv[3] - uv[1]) / rect.h * s);
            (
                rect.inset(-s),
                [uv[0] - du, uv[1] - dv, uv[2] + du, uv[3] + dv],
                -s,
            )
        } else {
            (rect, uv, border)
        };
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
                let color: [f32; 4] =
                    std::array::from_fn(|i| colors[0][i] + (colors[1][i] - colors[0][i]) * y);
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

    fn dissolve(&mut self, from: &str, to: &str, amount: f32, tint: Color, lift: Color) {
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
                lift,
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
        let entry = self
            .vectors
            .entry(path.into())
            .or_insert_with(|| VectorEntry {
                mesh: match vector::Mesh::load(Path::new(path)) {
                    Ok(mesh) => Some(mesh),
                    Err(error) => {
                        eprintln!("{error}");
                        None
                    }
                },
                frame: 0,
            });
        entry.frame = self.frame;
        let Some(mesh) = &entry.mesh else {
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
        // One texture per app: an updated icon (a new source) replaces the
        // old one rather than leaving it on the GPU under a key nothing uses.
        let current = self.installed_icons.get(&image.key) == Some(&image.source)
            && self.textures.get(&image.key).is_some_and(Option::is_some);
        if current {
            image.uploaded();
        } else {
            let Some(uploaded) =
                image.upload(|pixels| texture(pixels.width(), pixels.height(), pixels.as_raw()))
            else {
                // Its pixels could not be read back: the card keeps its
                // backdrop colour, as for an app with no artwork.
                return;
            };
            self.textures.insert(image.key.clone(), Some(uploaded));
            self.installed_icons.insert(image.key.clone(), image.source);
        }
        let Some(Some(texture)) = self.textures.get_mut(&image.key) else {
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
        self.soft = 0.;
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
                Draw::Shape(rect, radius, width, _)
                | Draw::GradientRing(rect, radius, width, ..)
                    if *width > 0. =>
                {
                    for [x0, y0, x1, y1] in regions(*rect, *radius, *width) {
                        let band = Rect::new(
                            rect.x + x0 * rect.w,
                            rect.y + y0 * rect.h,
                            (x1 - x0) * rect.w,
                            (y1 - y0) * rect.h,
                        );
                        *fill.entry("shape").or_insert(0.) += screens(band, self.clip);
                    }
                    None
                }
                Draw::Shape(rect, ..)
                | Draw::RoundedGradient(rect, ..)
                | Draw::GradientRing(rect, ..) => Some(("shape", *rect)),
                Draw::Shadow(rect, _, blur, _) => Some(("shadow", rect.inset(-*blur))),
                Draw::Text(_, rect, ..) => Some(("text", *rect)),
                Draw::Image(path, rect, ..) | Draw::TintedImage(path, rect, ..) => Some((
                    if path.ends_with(".lvec") {
                        "vector"
                    } else {
                        "image"
                    },
                    *rect,
                )),
                Draw::AppImage(_, rect, _) => Some(("image", *rect)),
                Draw::Glass(_, rect, ..) => Some(("glass", *rect)),
                Draw::Dissolve(..) => Some(("background", Rect::new(0., 0., 1920., 1080.))),
                Draw::Clip(_) | Draw::EdgeFade(_) | Draw::Soft(_) => None,
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
                Draw::Glass(path, rect, radius, top, bottom) => {
                    self.glass(path, *rect, *radius, [*top, *bottom])
                }
                Draw::Dissolve(from, to, amount, tint, lift) => {
                    self.dissolve(from, to, *amount, *tint, *lift)
                }
                Draw::Clip(clip) => self.clip = *clip,
                Draw::EdgeFade(fade) => self.fade = *fade,
                Draw::Soft(soft) => self.soft = soft.unwrap_or(0.),
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
        let total =
            |fill: &std::collections::BTreeMap<&'static str, f32>| fill.values().sum::<f32>();
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
                        lift,
                    } => {
                        // At a mix of exactly 0 or 1 only one image shows (Home
                        // and Settings at rest): the one-sample variant draws
                        // it with that image's tint at half the texture fetches
                        // of this full-screen pass. Recorded frames matched the
                        // two-image pass bit for bit, except the light Settings
                        // push, where GL's mix(a, b, 1) rounds up to 1/255 off
                        // b on a few hundred edge pixels; this draws b exactly.
                        let single = *amount == 0. || *amount == 1.;
                        let (from, tint) = if *amount == 0. {
                            (from, &[1.; 4])
                        } else if single {
                            (to, tint)
                        } else {
                            (from, tint)
                        };
                        // Same vertex layout as paint; the output is opaque, so
                        // blending is off for this one full-screen pass.
                        gl::UseProgram(self.dissolve_programs[usize::from(single)]);
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
                        let [texture, texture2, mix, tint2, lift_uniform] =
                            self.dissolve_uniforms[usize::from(single)];
                        gl::Uniform1i(texture, 0);
                        gl::Uniform1i(texture2, 1);
                        gl::Uniform1f(mix, *amount);
                        gl::Uniform4f(tint2, tint[0], tint[1], tint[2], tint[3]);
                        gl::Uniform4f(lift_uniform, lift[0], lift[1], lift[2], lift[3]);
                        if !single {
                            gl::ActiveTexture(gl::TEXTURE1);
                            gl::BindTexture(gl::TEXTURE_2D, *to);
                            gl::ActiveTexture(gl::TEXTURE0);
                        }
                        gl::BindTexture(gl::TEXTURE_2D, *from);
                        gl::Disable(gl::BLEND);
                        gl::DrawArrays(gl::TRIANGLES, *start as i32, 6);
                        gl::Enable(gl::BLEND);
                        gl::UseProgram(self.program);
                        paint_bound = false;
                    }
                    BatchKind::Vector { path, rect, tint } => {
                        let Some(mesh) = self.vectors.get(path).and_then(|v| v.mesh.as_ref())
                        else {
                            continue;
                        };
                        self.draw_mesh(
                            self.vector_program,
                            mesh,
                            [rect.w / mesh.width, rect.h / mesh.height, rect.x, rect.y],
                            *tint,
                            0.,
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
                            text::embolden(
                                f32::from_bits(key.layout[0]) * self.stats.viewport.height as f32
                                    / 1080.,
                            ),
                        );
                        paint_bound = false;
                    }
                }
            }
            gl::Disable(gl::SCISSOR_TEST);
        }
        // Keep the current scene, evict old full-resolution wallpapers before memory grows.
        let mut bytes: usize = self.textures.values().flatten().map(|t| t.bytes).sum();
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
                    bytes -= t.bytes;
                }
            }
        }
        self.trim_text_geometry();
        let meshes = || self.vectors.values().filter_map(|v| v.mesh.as_ref());
        self.stats.vector_meshes = meshes().count();
        self.stats.vector_bytes = meshes().map(|mesh| mesh.bytes).sum();
        self.stats.artwork_bytes = bytes + self.stats.vector_bytes;
        self.stats.draw_calls = self.batches.len();
        self.stats.texture_uploads = TEXTURE_UPLOADS.load(std::sync::atomic::Ordering::Relaxed);
        self.stats.cpu_submission_ms = started.elapsed().as_secs_f64() * 1000.;
    }

    fn draw_mesh(
        &self,
        program: GLuint,
        mesh: &vector::Mesh,
        transform: [f32; 4],
        tint: Color,
        embolden: f32,
    ) {
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
                gl::Uniform1f(uniforms.embolden, embolden);
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
    /// Text runs and vector meshes follow the textures' rule: those drawn in
    /// the last frame stay, the rest rebuild on demand (text runs measured
    /// 0.30 MB on Home but 5.99 MB after a Settings subpage; vector meshes
    /// 4.7 MB growing to 9.4 MB, never evicted before). The glyph outlines
    /// all go (5.7 MB of heap for 586 glyphs): the kept runs are already
    /// built from them, and a new run tessellates the few it needs.
    pub fn trim(&mut self) {
        let frame = self.frame;
        self.textures.retain(|path, texture| {
            texture
                .as_ref()
                .is_some_and(|texture| texture.frame == frame)
                || path.starts_with("installed-icon:")
        });
        self.installed_icons
            .retain(|key, _| self.textures.contains_key(key));
        trim_geometry(
            &mut self.text_runs,
            &mut self.vectors,
            &mut self.glyphs,
            frame,
        );
        self.textures.shrink_to_fit();
        self.installed_icons.shrink_to_fit();
        self.artwork_variants = HashMap::new();
        self.vertices = Vec::new();
        self.batches = Vec::new();
    }

    /// Upload a vector mesh read off the render thread, unless already loaded.
    pub fn adopt_vector(&mut self, path: String, bytes: &[u8]) {
        if let std::collections::hash_map::Entry::Vacant(slot) = self.vectors.entry(path) {
            let mesh = vector::Mesh::from_bytes(Path::new(slot.key()), bytes).ok();
            // Counts as drawn in the last frame, like adopted textures, so a
            // trim before its first draw keeps it.
            slot.insert(VectorEntry {
                mesh,
                frame: self.frame,
            });
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
        let frame = self.adopted_frame();
        self.textures.entry(path).or_insert_with(|| {
            let mut texture = compressed_texture(image);
            texture.frame = frame;
            Some(texture)
        });
    }

    /// An upload counts as drawn the frame before the last, so `trim` frees
    /// a preloaded image no frame has shown yet (it is queued again on
    /// return) while the budget still evicts it after older ones.
    fn adopted_frame(&self) -> u64 {
        self.frame.saturating_sub(1)
    }

    /// Upload an image decoded off the render thread, unless already loaded.
    pub fn adopt(&mut self, path: String, image: image::RgbaImage) {
        let frame = self.adopted_frame();
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

    /// The bound framebuffer as RGBA rows from the top.
    fn read_pixels(&self, width: u32, height: u32) -> Vec<u8> {
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
        let row = width as usize * 4;
        pixels.chunks_exact(row).rev().flatten().copied().collect()
    }

    /// The authored TV surface rendered off-screen at `size`, as RGBA rows
    /// from the top.
    pub fn capture_pixels(
        &mut self,
        commands: &[Draw],
        size: Size,
    ) -> Result<(Vec<u8>, RenderStats), String> {
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
            gl::RenderbufferStorage(
                gl::RENDERBUFFER,
                gl::STENCIL_INDEX8,
                size.width as i32,
                size.height as i32,
            );
            gl::FramebufferRenderbuffer(
                gl::FRAMEBUFFER,
                gl::STENCIL_ATTACHMENT,
                gl::RENDERBUFFER,
                stencil,
            );
            if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                gl::FramebufferRenderbuffer(
                    gl::FRAMEBUFFER,
                    gl::STENCIL_ATTACHMENT,
                    gl::RENDERBUFFER,
                    0,
                );
            }
            while gl::GetError() != gl::NO_ERROR {}
        }
        self.render(commands, size.width, size.height);
        let stats = self.stats.clone();
        let pixels = self.read_pixels(size.width, size.height);
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::DeleteFramebuffers(1, &framebuffer);
            gl::DeleteRenderbuffers(1, &stencil);
        }
        Ok((pixels, stats))
    }

    /// Capture the authored TV surface, independent of desktop HiDPI/window limits.
    pub fn capture(
        &mut self,
        commands: &[Draw],
        path: &Path,
        size: Size,
    ) -> Result<RenderStats, String> {
        let (pixels, stats) = self.capture_pixels(commands, size)?;
        image::RgbaImage::from_raw(size.width, size.height, pixels)
            .ok_or("Invalid framebuffer dimensions")?
            .save(path)
            .map_err(|e| e.to_string())?;
        Ok(stats)
    }
}

/// Keeps the text runs and vector meshes drawn in `frame`, drops every glyph
/// outline, and hands the emptied tables' memory back.
fn trim_geometry(
    text_runs: &mut HashMap<TextKey, TextRun>,
    vectors: &mut HashMap<String, VectorEntry>,
    glyphs: &mut HashMap<(char, u16), Glyph>,
    frame: u64,
) {
    text_runs.retain(|_, run| run.frame == frame);
    text_runs.shrink_to_fit();
    vectors.retain(|_, entry| entry.frame == frame);
    vectors.shrink_to_fit();
    *glyphs = HashMap::new();
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.buffer);
            gl::DeleteProgram(self.program);
            for program in self.dissolve_programs {
                gl::DeleteProgram(program);
            }
            gl::DeleteProgram(self.vector_program);
            gl::DeleteProgram(self.text_program);
            if self.vao != 0 {
                gl::DeleteVertexArrays(1, &self.vao);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(frame: u64) -> TextRun {
        TextRun {
            mesh: None,
            missing_glyphs: 0,
            failures: 0,
            frame,
        }
    }

    fn key(text: &str) -> TextKey {
        TextKey {
            text: text.into(),
            weight: 400,
            layout: [0; 5],
            flags: 0,
        }
    }

    #[test]
    fn trim_keeps_what_the_last_frame_drew_and_drops_the_rest() {
        let mut runs = HashMap::from([(key("Home"), run(9)), (key("Settings"), run(4))]);
        let mut vectors = HashMap::from([
            (
                "home.lvec".to_owned(),
                VectorEntry {
                    mesh: None,
                    frame: 9,
                },
            ),
            (
                "wifi.lvec".to_owned(),
                VectorEntry {
                    mesh: None,
                    frame: 3,
                },
            ),
        ]);
        let mut glyphs = HashMap::from([(
            ('H', 400),
            Glyph {
                geometry: Vec::new(),
                frame: 9,
            },
        )]);
        trim_geometry(&mut runs, &mut vectors, &mut glyphs, 9);
        assert!(runs.contains_key(&key("Home")));
        assert!(!runs.contains_key(&key("Settings")));
        assert!(vectors.contains_key("home.lvec"));
        assert!(!vectors.contains_key("wifi.lvec"));
        // Even a glyph drawn last frame goes: its run is already built.
        assert!(glyphs.is_empty());
    }
}
