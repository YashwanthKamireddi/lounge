//! The window and its GL context: GLES 2 on the TV, GL 3.2 on the desktop,
//! with the multisampling each can afford.
use crate::{options::Options, raster::Size};
use sdl2::{
    Sdl, VideoSubsystem,
    video::{GLContext, GLProfile, Window},
};

/// Opens Lounge's window, `requested` large, and makes its GL context current. `startup_stage`
/// logs each step's time on the TV.
pub fn open(
    sdl: &Sdl,
    options: &Options,
    webos: bool,
    requested: Size,
    startup_stage: &dyn Fn(&str),
) -> Result<(VideoSubsystem, Window, GLContext), String> {
    let drivers: Vec<_> = sdl2::video::drivers().collect();
    if webos && !drivers.contains(&"wayland") {
        return Err(format!(
            "SDL has no Wayland backend (drivers: {}). Rebuild the package with the SDL-webOS runtime.",
            drivers.join(", ")
        ));
    }
    let video = sdl.video().map_err(|error| {
        format!(
            "SDL video initialization: {error}; drivers: {}; XDG_RUNTIME_DIR={:?}; WAYLAND_DISPLAY={:?}",
            drivers.join(", "),
            std::env::var_os("XDG_RUNTIME_DIR"),
            std::env::var_os("WAYLAND_DISPLAY"),
        )
    })?;
    eprintln!("Lounge SDL video driver: {}", video.current_video_driver());
    startup_stage("SDL video");
    let attributes = video.gl_attr();
    if webos {
        attributes.set_context_profile(GLProfile::GLES);
        attributes.set_context_version(2, 0);
    } else {
        attributes.set_context_profile(GLProfile::Core);
        attributes.set_context_version(3, 2);
    }
    attributes.set_double_buffer(true);
    attributes.set_alpha_size(8);
    attributes.set_depth_size(0);
    // Stencil: opaque tiles mask out what lies behind them (renderer::find_occluders).
    attributes.set_stencil_size(8);
    let create_window = || {
        let mut builder = video.window("Lounge", requested.width, requested.height);
        builder.opengl().allow_highdpi().position_centered();
        if options.fullscreen || (webos && options.resolution.is_none()) {
            builder.fullscreen_desktop();
        } else if webos {
            builder.borderless();
        } else {
            builder.resizable();
        }
        let window = builder.build().map_err(|e| e.to_string())?;
        let context = window.gl_create_context()?;
        Ok::<_, String>((window, context))
    };
    let mut opened = None;
    let mut failures = Vec::new();
    // Shapes and text already blend their own edges (coverage in the shader),
    // so the TV draws without multisampling: on its one-core Mali, 4x MSAA
    // turned ~3 dropped frames per 1000 into ~14 while scrolling (3 runs of
    // 1000, tools/tv-bench.sh). The desktop keeps 4x; --msaa overrides both.
    let sample_choices: &[u8] = match options.msaa {
        None if webos => &[0],
        Some(4) | None => &[4, 2, 0],
        Some(2) => &[2, 0],
        Some(_) => &[0],
    };
    for &samples in sample_choices {
        attributes.set_multisample_buffers(u8::from(samples > 0));
        attributes.set_multisample_samples(samples);
        match create_window() {
            Ok(window) => {
                opened = Some(window);
                break;
            }
            Err(error) => failures.push(format!("{samples}x MSAA: {error}")),
        }
    }
    let (window, _context) = opened.ok_or_else(|| failures.join("; "))?;
    gl::load_with(|name| video.gl_get_proc_address(name) as *const _);
    if !webos {
        unsafe { gl::Enable(gl::MULTISAMPLE) };
    }
    let mut samples = 0;
    unsafe { gl::GetIntegerv(gl::SAMPLES, &mut samples) };
    eprintln!("Lounge GL multisampling: {samples} samples");
    startup_stage("window and GL context");
    Ok((video, window, _context))
}
