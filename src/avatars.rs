//! Profile pictures for the status pill: the owner's own pictures, kept in
//! an `avatars` folder beside the preferences. Each is cut square (centred)
//! and scaled once, with a high-quality filter, to the two sizes Luma draws
//! (the pill and the Settings preview); the results are cached beside the
//! originals and only redone when a picture changes.
use image::{ImageReader, imageops::FilterType};
use std::path::{Path, PathBuf};

/// The pill's avatar is drawn 46 logical px wide; 52 covers its largest draw.
const SMALL: u32 = 52;
/// The Settings preview.
const LARGE: u32 = 320;

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
                .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
        })
        .collect();
    files.sort();
    let mut pictures: Vec<Picture> = files.iter().filter_map(|path| prepare(path, &folder.join(".cache"))).collect();
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

/// The cached small and large cuts of `source`, made when missing or older
/// than the picture. None when it cannot be read.
fn prepare(source: &Path, cache: &Path) -> Option<Picture> {
    let file = source.file_name()?.to_str()?.to_owned();
    let stem = source.file_stem()?.to_str()?;
    let small = cache.join(format!("{stem}-{SMALL}.png"));
    let large = cache.join(format!("{stem}-{LARGE}.png"));
    let changed = std::fs::metadata(source).and_then(|meta| meta.modified()).ok();
    let fresh = |path: &Path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .zip(changed)
            .is_some_and(|(made, changed)| made >= changed)
    };
    if !fresh(&small) || !fresh(&large) {
        let image = ImageReader::open(source).ok()?.with_guessed_format().ok()?.decode().ok()?.to_rgba8();
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
        for (size, path) in [(SMALL, &small), (LARGE, &large)] {
            image::imageops::resize(&square, size, size, FilterType::Lanczos3).save(path).ok()?;
        }
    }
    Some(Picture {
        file,
        title: title(stem),
        small: small.to_string_lossy().into_owned(),
        large: large.to_string_lossy().into_owned(),
    })
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
        let directory = std::env::temp_dir().join(format!("luma-avatars-{}", std::process::id()));
        let folder = directory.join("avatars");
        std::fs::create_dir_all(&folder).unwrap();
        image::RgbaImage::from_pixel(90, 60, image::Rgba([200, 30, 40, 255]))
            .save(folder.join("lord_ganesh-1.png"))
            .unwrap();
        std::fs::write(folder.join("notes.txt"), "not a picture").unwrap();
        let pictures = load(&directory);
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0].title, "Lord ganesh 1");
        let small = image::open(&pictures[0].small).unwrap();
        let large = image::open(&pictures[0].large).unwrap();
        assert_eq!((small.width(), small.height()), (SMALL, SMALL));
        assert_eq!((large.width(), large.height()), (LARGE, LARGE));
        // A second load reuses the cache.
        let made = std::fs::metadata(&pictures[0].small).unwrap().modified().unwrap();
        assert_eq!(load(&directory), pictures);
        assert_eq!(std::fs::metadata(&pictures[0].small).unwrap().modified().unwrap(), made);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
