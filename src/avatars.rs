//! Profile pictures for the status pill: the owner's own pictures, kept in
//! an `avatars` folder beside the preferences. Each is cut square (centred),
//! fitted inside the round frame (see `fit_circle`) and scaled once, with a
//! high-quality filter, to the two sizes Lounge draws (the pill and the
//! Settings preview); the results are cached beside the originals and only
//! redone when a picture changes.
use image::{ImageReader, RgbaImage, imageops::FilterType};
use std::path::{Path, PathBuf};

/// The pill's avatar is drawn 60 logical px wide; 64 covers its largest draw.
const SMALL: u32 = 64;
/// The Settings preview.
const LARGE: u32 = 320;
/// The picture's share of the round frame's width: small enough that the
/// whole square (a figure reaching its edges: crown, knees, feet) sits
/// inside the circle, floating in the middle, clipped nowhere.
const INSET: f32 = 0.74;
/// Names the cached cuts; a change in how they are made changes it, so old
/// cuts are made again (and cleared away).
const CUT: &str = "fit4";

#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    /// The file's name in the folder: what the preference remembers.
    pub file: String,
    /// The name Settings shows ("lord-ganesh" → "Lord ganesh").
    pub title: String,
    pub small: String,
    pub large: String,
}

/// The pictures in `<directory>/avatars`, A-Z. A lone `avatar.jpg` beside the
/// preferences (how a picture was chosen before this folder) still counts.
pub fn load(directory: &Path) -> Vec<Picture> {
    let folder = directory.join("avatars");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "jpg" | "jpeg" | "png" | "webp"
                    )
                })
        })
        .collect();
    files.sort();
    let cache = folder.join(".cache");
    let mut pictures: Vec<Picture> = files
        .iter()
        .filter_map(|path| prepare(path, &cache))
        .collect();
    prune(&cache, &pictures);
    let legacy = directory.join("avatar.jpg");
    if pictures.is_empty() && legacy.is_file() {
        let path = legacy.to_string_lossy().into_owned();
        pictures.push(Picture {
            file: "avatar.jpg".into(),
            title: "Avatar".into(),
            small: path.clone(),
            large: path,
        });
    }
    pictures
}

/// `load` on a worker, the pictures sent once ready. A changed picture is
/// decoded (up to 4096², 80 MB), scaled and blurred: kept off start-up's way
/// to the first frame.
pub fn load_in_background(directory: PathBuf) -> std::sync::mpsc::Receiver<Vec<Picture>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        crate::background_priority();
        let _ = sender.send(load(&directory));
    });
    receiver
}

/// The cached small and large cuts of `source`, made when missing or older
/// than the picture. None when it cannot be read.
fn prepare(source: &Path, cache: &Path) -> Option<Picture> {
    let file = source.file_name()?.to_str()?.to_owned();
    let stem = source.file_stem()?.to_str()?;
    let small = cache.join(format!("{stem}-{CUT}-{SMALL}.png"));
    let large = cache.join(format!("{stem}-{CUT}-{LARGE}.png"));
    let changed = std::fs::metadata(source)
        .and_then(|meta| meta.modified())
        .ok();
    let fresh = |path: &Path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .zip(changed)
            .is_some_and(|(made, changed)| made >= changed)
    };
    if !fresh(&small) || !fresh(&large) {
        // A phone photo can be 12+ megapixels: bounded, so a stray huge file
        // is skipped rather than holding tens of MB at startup.
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(80 * 1024 * 1024);
        let mut reader = ImageReader::open(source).ok()?.with_guessed_format().ok()?;
        reader.limits(limits);
        let image = reader.decode().ok()?.to_rgba8();
        let side = image.width().min(image.height());
        let square = image::imageops::crop_imm(
            &image,
            (image.width() - side) / 2,
            (image.height() - side) / 2,
            side,
            side,
        )
        .to_image();
        std::fs::create_dir_all(cache).ok()?;
        crate::storage::give_to_folder_owner(cache).ok()?;
        for (size, path) in [(SMALL, &small), (LARGE, &large)] {
            fit_circle(&square, size).save(path).ok()?;
            crate::storage::give_to_folder_owner(path).ok()?;
        }
    }
    Some(Picture {
        file,
        title: title(stem),
        small: small.to_string_lossy().into_owned(),
        large: large.to_string_lossy().into_owned(),
    })
}

/// `square` at `size`, fitted whole inside the round frame: the picture
/// takes INSET of the width, centred. The margin is the picture's own edge pixels carried outward and
/// softened, so a sky or a wall simply continues; the picture's edges feather
/// into it over a wide band, so no ring or seam shows.
fn fit_circle(square: &RgbaImage, size: u32) -> RgbaImage {
    let inner = ((size as f32 * INSET).round() as u32).min(size);
    let picture = image::imageops::resize(square, inner, inner, FilterType::Lanczos3);
    let (left, top) = ((size - inner) / 2, (size - inner) / 2);
    let clamp = |value: u32, start: u32| value.saturating_sub(start).min(inner - 1);
    let extended = RgbaImage::from_fn(size, size, |x, y| {
        *picture.get_pixel(clamp(x, left), clamp(y, top))
    });
    let mut out = image::imageops::blur(&extended, size as f32 / 16.);
    let feather = (inner as f32 / 6.).max(1.);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let (Some(px), Some(py)) = (x.checked_sub(left), y.checked_sub(top)) else {
            continue;
        };
        if px >= inner || py >= inner {
            continue;
        }
        let edge = px.min(py).min(inner - 1 - px).min(inner - 1 - py) as f32;
        let t = (edge / feather).min(1.);
        let t = t * t * (3. - 2. * t);
        let sharp = picture.get_pixel(px, py).0;
        for (channel, sharp) in pixel.0.iter_mut().zip(sharp) {
            *channel = (f32::from(*channel) * (1. - t) + f32::from(sharp) * t).round() as u8;
        }
    }
    out
}

/// Removes cached cuts no picture uses any more (a removed picture, or cuts
/// made an older way), so the cache never grows.
fn prune(cache: &Path, pictures: &[Picture]) {
    let keep: Vec<&str> = pictures
        .iter()
        .flat_map(|picture| [picture.small.as_str(), picture.large.as_str()])
        .collect();
    for entry in std::fs::read_dir(cache).into_iter().flatten().flatten() {
        let path = entry.path();
        if !keep.contains(&path.to_string_lossy().as_ref()) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// "ganesh_smile-2" → "Ganesh smile 2".
fn title(stem: &str) -> String {
    let words = stem.replace(['-', '_'], " ");
    let mut letters = words.trim().chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_cut_square_cached_and_named() {
        let directory = std::env::temp_dir().join(format!("lounge-avatars-{}", std::process::id()));
        let folder = directory.join("avatars");
        std::fs::create_dir_all(&folder).unwrap();
        image::RgbaImage::from_pixel(90, 60, image::Rgba([200, 30, 40, 255]))
            .save(folder.join("lord_ganesh-1.png"))
            .unwrap();
        std::fs::write(folder.join("notes.txt"), "not a picture").unwrap();
        // A cut made an older way is cleared once the picture is cut again.
        std::fs::create_dir_all(folder.join(".cache")).unwrap();
        std::fs::write(folder.join(".cache/lord_ganesh-1-52.png"), "stale").unwrap();
        let pictures = load(&directory);
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0].title, "Lord ganesh 1");
        let small = image::open(&pictures[0].small).unwrap();
        let large = image::open(&pictures[0].large).unwrap();
        assert_eq!((small.width(), small.height()), (SMALL, SMALL));
        assert_eq!((large.width(), large.height()), (LARGE, LARGE));
        assert!(!folder.join(".cache/lord_ganesh-1-52.png").exists());
        // The picture sits inset: the frame's top edge is margin (the
        // picture's own colour, blurred), never cut through the picture.
        let large = large.to_rgba8();
        assert_eq!(large.get_pixel(LARGE / 2, 2).0, [200, 30, 40, 255]);
        // A second load reuses the cache.
        let made = std::fs::metadata(&pictures[0].small)
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(load(&directory), pictures);
        // The worker finds the same pictures.
        let worker = load_in_background(directory.clone());
        assert_eq!(
            worker.recv_timeout(std::time::Duration::from_secs(10)),
            Ok(pictures.clone())
        );
        assert_eq!(
            std::fs::metadata(&pictures[0].small)
                .unwrap()
                .modified()
                .unwrap(),
            made
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
