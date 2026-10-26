use crate::raster::Size;
use std::{collections::VecDeque, path::PathBuf};
use time::OffsetDateTime;

#[derive(Default)]
pub(crate) struct Options {
    pub(crate) assets: Option<PathBuf>,
    pub(crate) preferences: Option<PathBuf>,
    pub(crate) screenshot: Option<PathBuf>,
    pub(crate) dump: Option<PathBuf>,
    pub(crate) frames: Option<u64>,
    pub(crate) script: VecDeque<String>,
    pub(crate) fullscreen: bool,
    pub(crate) time: Option<OffsetDateTime>,
    pub(crate) resolution: Option<Size>,
    pub(crate) capture_size: Option<Size>,
    pub(crate) diagnostics: Option<PathBuf>,
    pub(crate) profile_frames: Option<usize>,
    pub(crate) msaa: Option<u8>,
    pub(crate) skip: Vec<String>,
    /// Diagnostic: run the main fragment shaders at medium precision.
    pub(crate) medium_precision: bool,
    pub(crate) weather_test_server: Option<String>,
    pub(crate) test_extra_apps: usize,
    pub(crate) inventory: Option<std::path::PathBuf>,
    /// Started by LG's preloader rather than by someone.
    pub(crate) preload: bool,
}

impl Options {
    pub(crate) fn parse() -> Result<Self, String> {
        let mut result = Self::default();
        let mut args = std::env::args().skip(1).peekable();
        // SAM/jailer puts native launch metadata (e.g. @system_native_app and
        // nid) in argv[1]. It is not a Lounge CLI option. Keep the original argv
        // intact for SDL/libhelpers, which also read the process launch data.
        if cfg!(feature = "webos")
            && args.peek().is_some_and(|argument| {
                serde_json::from_str::<serde_json::Value>(argument)
                    .is_ok_and(|value| value.is_object())
            })
        {
            if let Some(argument) = args.next() {
                let launch = serde_json::from_str::<serde_json::Value>(&argument)
                    .map_err(|error| error.to_string())?;
                result.preload = launch.get("preload").is_some();
            }
            eprintln!("Lounge webOS launch parameters accepted");
        }
        while let Some(arg) = args.next() {
            if arg == "--help" {
                println!(
                    "Lounge native\n\n  --assets PATH       Generated artwork directory\n  --preferences PATH  Isolated preferences file\n  --fullscreen        Fullscreen desktop preview\n  --resolution SIZE   Request 1080p, 4k, or WIDTHxHEIGHT; verify actual drawable\n  --frames N          Exit after N event-loop frames\n  --profile-frames N  Force N presentations (20+) and report pacing after warmup\n  --script ACTIONS    Comma-separated home/apps/settings/up/down/left/right/enter/back/menu/hold/type:TEXT/click:X:Y/wait\n  --time RFC3339      Fix the preview's clock and date\n  --screenshot PATH   Save an offscreen PNG\n  --capture-scale N   Capture at 1x/1080p (default) or 2x/4k\n  --diagnostics PATH  Save actual drawable, GPU, font-cache and timing information\n  --dump-state PATH   Save final UI state as JSON\n\nCompile with --features webos for the TV backend. Desktop builds never call TV services."
                );
                println!(
                    "\n  --test-extra-apps N  Explicit scroll preview with up to 300 non-launchable extra cards; separate preferences"
                );
                std::process::exit(0);
            }
            if arg == "--fullscreen" {
                result.fullscreen = true;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| format!("Missing value for {arg}"))?;
            match arg.as_str() {
                "--assets" => result.assets = Some(value.into()),
                "--preferences" => result.preferences = Some(value.into()),
                "--screenshot" => result.screenshot = Some(value.into()),
                "--dump-state" => result.dump = Some(value.into()),
                "--resolution" => result.resolution = Some(value.parse()?),
                "--capture-scale" => {
                    result.capture_size = Some(match value.as_str() {
                        "1" => Size::HD,
                        "2" => Size::UHD,
                        _ => return Err("--capture-scale must be 1 or 2".into()),
                    })
                }
                "--diagnostics" => result.diagnostics = Some(value.into()),
                "--weather-test-server" => result.weather_test_server = Some(value),
                "--inventory" => result.inventory = Some(value.into()),
                "--test-extra-apps" => {
                    result.test_extra_apps = value
                        .parse::<usize>()
                        .ok()
                        .filter(|count| *count <= 300)
                        .ok_or("--test-extra-apps must be between 0 and 300")?;
                }
                "--skip" => result.skip = value.split(',').map(str::to_owned).collect(),
                "--precision" => {
                    result.medium_precision = match value.as_str() {
                        "medium" => true,
                        "high" => false,
                        _ => return Err("--precision must be medium or high".into()),
                    }
                }
                "--msaa" => {
                    result.msaa = Some(
                        value
                            .parse::<u8>()
                            .ok()
                            .filter(|n| [0, 2, 4].contains(n))
                            .ok_or("--msaa must be 0, 2 or 4")?,
                    )
                }
                "--profile-frames" => {
                    result.profile_frames = Some(
                        value
                            .parse::<usize>()
                            .ok()
                            .filter(|n| (20..=1000000).contains(n))
                            .ok_or("--profile-frames must be between 20 and 1000000")?,
                    )
                }
                "--time" => {
                    result.time = Some(
                        OffsetDateTime::parse(
                            &value,
                            &time::format_description::well_known::Rfc3339,
                        )
                        .map_err(|e| e.to_string())?,
                    )
                }
                "--frames" => {
                    result.frames = Some(
                        value
                            .parse::<u64>()
                            .ok()
                            .filter(|n| *n > 0)
                            .ok_or("--frames must be positive")?,
                    )
                }
                "--script" => result.script = value.split(',').map(str::to_owned).collect(),
                _ => return Err(format!("Unknown option: {arg}")),
            }
        }
        if result.screenshot.is_some() && result.frames.is_none() && result.profile_frames.is_none()
        {
            result.frames = Some(90);
        }
        Ok(result)
    }
}
