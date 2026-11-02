//! A photo from the phone (see `companion`) made into what Lounge draws, on the TV, the way
//! `tools/assets.mjs` makes the bundled wallpapers: the photo shaded for Home, its blurred
//! glass backdrops, and a thumbnail for the gallery. Every file is written beside the
//! preferences, under a new name each time (the GPU caches pictures by path), and only the
//! newest photo is kept.
use image::{ImageReader, RgbImage, imageops::FilterType};
use std::path::{Path, PathBuf};

/// What the phone's page fits a wallpaper to.
pub const WALLPAPER_SIZE: (u32, u32) = (1920, 1080);
/// What it fits a profile picture to.
pub const AVATAR_SIDE: u32 = 640;
/// Wallpaper ids made from the owner's photos start with this.
pub const PHOTO_PREFIX: &str = "photo-";
/// The blurred backdrops' size and blur, as for the bundled wallpapers.
const GLASS: (u32, u32) = (640, 360);
const GLASS_SIGMA: f32 = 12.;

fn decode(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let mut limits = image::Limits::default();
    // The phone's page sends exactly the size needed; anything far larger is refused rather
    // than allowed to take tens of MB from a TV with little memory to spare.
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    reader.limits(limits);
    reader.decode().map_err(|error| error.to_string())
}

/// Scaled to cover `width` × `height`, centred.
fn cover(image: &image::DynamicImage, width: u32, height: u32) -> RgbImage {
    // What the page sends is already this size: used as is, with no resampling pass.
    if (image.width(), image.height()) == (width, height) {
        return image.to_rgb8();
    }
    image
        .resize_to_fill(width, height, FilterType::Lanczos3)
        .to_rgb8()
}

fn seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// Written whole or not at all: a power cut mid-write leaves no half picture.
fn write(path: &Path, save: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let partial = path.with_extension("partial");
    save(&partial)?;
    std::fs::rename(&partial, path).map_err(|error| error.to_string())
}

fn save_jpeg(image: &RgbImage, path: &Path, quality: u8) -> Result<(), String> {
    write(path, |partial| {
        let file = std::fs::File::create(partial).map_err(|error| error.to_string())?;
        let mut out = std::io::BufWriter::new(file);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .encode_image(image)
            .map_err(|error| error.to_string())
    })
}

fn save_png(image: &RgbImage, path: &Path) -> Result<(), String> {
    // Lossless: JPEG blocks and banding show once a blur is stretched 3×.
    write(path, |partial| {
        image
            .save_with_format(partial, image::ImageFormat::Png)
            .map_err(|error| error.to_string())
    })
}

fn glass(image: &RgbImage) -> RgbImage {
    let small = image::imageops::resize(image, GLASS.0, GLASS.1, FilterType::Triangle);
    image::imageops::blur(&small, GLASS_SIGMA)
}

/// The folder a wallpaper id's pictures are in, for photos.
pub fn wallpapers(storage: &Path) -> PathBuf {
    storage.join("wallpapers")
}

/// Whether `id` names one of the owner's photos (not a bundled wallpaper).
pub fn is_photo(id: &str) -> bool {
    id.strip_prefix(PHOTO_PREFIX)
        .is_some_and(|stamp| !stamp.is_empty() && stamp.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The newest photo wallpaper saved in `storage`, if its pictures are all there.
pub fn saved_wallpaper(storage: &Path) -> Option<String> {
    let folder = wallpapers(storage);
    let mut ids: Vec<String> = std::fs::read_dir(&folder)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".jpg")?;
            is_photo(id).then(|| id.to_owned())
        })
        .filter(|id| {
            ["-glass.png", "-home-glass.png", "-thumb.jpg"]
                .iter()
                .all(|suffix| folder.join(format!("{id}{suffix}")).is_file())
        })
        .collect();
    ids.sort_by_key(|id| id[PHOTO_PREFIX.len()..].parse::<u64>().unwrap_or(0));
    ids.pop()
}

/// Makes a wallpaper of the photo in `bytes`; its id.
pub fn save_wallpaper(bytes: &[u8], storage: &Path) -> Result<String, String> {
    let photo = cover(&decode(bytes)?, WALLPAPER_SIZE.0, WALLPAPER_SIZE.1);
    let folder = wallpapers(storage);
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let mut stamp = seconds();
    while folder.join(format!("{PHOTO_PREFIX}{stamp}.jpg")).exists() {
        stamp += 1;
    }
    let id = format!("{PHOTO_PREFIX}{stamp}");
    let shaded = shade(&photo);
    let thumb = image::imageops::resize(&photo, 480, 270, FilterType::Triangle);
    // The photo itself last: `saved_wallpaper` only finds a wallpaper once all are there.
    save_png(&glass(&photo), &folder.join(format!("{id}-glass.png")))?;
    save_png(
        &glass(&shaded),
        &folder.join(format!("{id}-home-glass.png")),
    )?;
    save_jpeg(&thumb, &folder.join(format!("{id}-thumb.jpg")), 85)?;
    save_jpeg(&shaded, &folder.join(format!("{id}.jpg")), 93)?;
    // Only the newest photo is kept.
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(PHOTO_PREFIX)
                && !name.starts_with(&format!("{id}."))
                && !name.starts_with(&format!("{id}-"))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(id)
}

/// Saves the photo in `bytes` as a profile picture; its file name in the avatars folder.
pub fn save_avatar(bytes: &[u8], storage: &Path) -> Result<String, String> {
    let image = decode(bytes)?;
    let side = image.width().min(image.height()).min(AVATAR_SIDE);
    let square = cover(&image, side, side);
    let folder = storage.join("avatars");
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let mut stamp = seconds();
    let prefix = crate::avatars::PHONE_PREFIX;
    while folder.join(format!("{prefix}{stamp}.jpg")).exists() {
        stamp += 1;
    }
    let name = format!("{prefix}{stamp}.jpg");
    save_jpeg(&square, &folder.join(&name), 92)?;
    Ok(name)
}

/// The Home shading the bundled wallpapers get (tools/assets.mjs): the photo at 92 % over
/// #17191c, a veil from the left, and the vertical shade that keeps the dock readable.
/// Blended in sRGB, as the SVG that makes the bundled ones is.
fn shade(photo: &RgbImage) -> RgbImage {
    const BASE: [f32; 3] = [0x17 as f32, 0x19 as f32, 0x1c as f32];
    const LEFT: ([f32; 3], f32) = ([0x11 as f32, 0x14 as f32, 0x17 as f32], 0.42);
    const STOPS: [(f32, u32, f32); 6] = [
        (0., 0x0d1013, 0.24),
        (0.19, 0x0e1115, 0.08),
        (0.35, 0x0f1114, 0.35),
        (0.63, 0x111317, 0.86),
        (0.85, 0x17191d, 0.92157),
        (1., 0x17191d, 0.96078),
    ];
    let rgb = |hex: u32| {
        [
            (hex >> 16) as f32,
            ((hex >> 8) & 0xff) as f32,
            (hex & 0xff) as f32,
        ]
    };
    let (width, height) = photo.dimensions();
    // The vertical shade, one colour and opacity per row.
    let rows: Vec<([f32; 3], f32)> = (0..height)
        .map(|y| {
            let t = (y as f32 + 0.5) / height as f32;
            let next = STOPS
                .iter()
                .position(|stop| stop.0 >= t)
                .unwrap_or(STOPS.len() - 1);
            let (a, b) = (STOPS[next.saturating_sub(1)], STOPS[next]);
            let f = if b.0 > a.0 {
                (t - a.0) / (b.0 - a.0)
            } else {
                0.
            };
            let (ca, cb) = (rgb(a.1), rgb(b.1));
            (
                [0, 1, 2].map(|i| ca[i] + (cb[i] - ca[i]) * f),
                a.2 + (b.2 - a.2) * f,
            )
        })
        .collect();
    let mut out = RgbImage::new(width, height);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let source = photo.get_pixel(x, y).0;
        let left = (LEFT.1 * (1. - (x as f32 + 0.5) / (0.75 * width as f32))).max(0.);
        let (shade, opacity) = rows[y as usize];
        pixel.0 = [0, 1, 2].map(|i| {
            let mut value = source[i] as f32 * 0.92 + BASE[i] * 0.08;
            value += (LEFT.0[i] - value) * left;
            value += (shade[i] - value) * opacity;
            value.round().clamp(0., 255.) as u8
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let image = RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .encode_image(&image)
            .unwrap();
        bytes
    }

    #[test]
    fn a_phone_photo_becomes_a_wallpaper_and_only_the_newest_is_kept() {
        let storage = std::env::temp_dir().join(format!("lounge-photo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&storage);
        let first = save_wallpaper(&jpeg(1600, 1200), &storage).unwrap();
        assert!(is_photo(&first));
        assert_eq!(saved_wallpaper(&storage).as_deref(), Some(first.as_str()));
        let folder = wallpapers(&storage);
        let shaded = image::open(folder.join(format!("{first}.jpg"))).unwrap();
        assert_eq!((shaded.width(), shaded.height()), WALLPAPER_SIZE);
        let glass = image::open(folder.join(format!("{first}-glass.png"))).unwrap();
        assert_eq!((glass.width(), glass.height()), GLASS);
        let second = save_wallpaper(&jpeg(800, 600), &storage).unwrap();
        assert_ne!(first, second);
        assert_eq!(saved_wallpaper(&storage).as_deref(), Some(second.as_str()));
        assert!(
            !folder.join(format!("{first}.jpg")).exists(),
            "the older photo is gone"
        );
        assert!(save_wallpaper(b"not a picture", &storage).is_err());
        let avatar = save_avatar(&jpeg(900, 1200), &storage).unwrap();
        let picture = image::open(storage.join("avatars").join(avatar)).unwrap();
        assert_eq!(picture.width(), picture.height());
        let _ = std::fs::remove_dir_all(&storage);
    }

    #[test]
    fn the_shade_darkens_toward_the_dock() {
        let white = RgbImage::from_pixel(64, 64, image::Rgb([255, 255, 255]));
        let shaded = shade(&white);
        let top = shaded.get_pixel(63, 2).0[0];
        let bottom = shaded.get_pixel(63, 61).0[0];
        assert!(top > 150 && bottom < 50, "top {top}, bottom {bottom}");
    }

    #[test]
    fn only_photo_ids_are_photos() {
        assert!(is_photo("photo-1730000000"));
        assert!(!is_photo("photo-"));
        assert!(!is_photo("photo-../x"));
        assert!(!is_photo("dunes"));
    }
}
