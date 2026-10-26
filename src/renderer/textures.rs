//! Texture uploads: decoded images and their ASTC-compressed copies.
use super::*;

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
    let size = |at: usize| {
        u32::from(header[at]) | u32::from(header[at + 1]) << 8 | u32::from(header[at + 2]) << 16
    };
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

pub(super) fn compressed_texture(image: &Compressed) -> Texture {
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
pub(super) fn load_texture(astc: bool, path: &str) -> Option<Texture> {
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

pub(super) fn texture(width: u32, height: u32, pixels: &[u8]) -> Texture {
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
