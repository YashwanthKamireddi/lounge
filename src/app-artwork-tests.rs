use super::*;
use image::{DynamicImage, ImageFormat, Rgba};
use serde_json::json;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

fn encoded(format: ImageFormat, width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(color)));
    let image = if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(image.into_rgb8())
    } else {
        image
    };
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn poll_until(service: &Service, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        service.poll();
        if done() {
            break;
        }
        assert!(Instant::now() < deadline, "artwork worker did not finish");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn inventory_metadata_resolves_absolute_relative_and_manifest_fallback_paths() {
    let metadata = json!({"id":"sample", "title":"Regional App", "icon":"regional.png",
        "largeIcon":"large.png", "folderPath":format!("{}/sample", ROOTS[1])});
    let apps = crate::platform::normalize_apps(&json!([metadata.clone()])).unwrap();
    assert_eq!(apps[0].icon, Some(Source::new("sample", &metadata)));
    let source = Source::new(
        "sample",
        &json!({
            "folderPath": format!("{}/actual-package-directory", ROOTS[1]),
            "largeIcon": "resources/hu/large.png", "icon": "icon.png"
        }),
    );
    let dirs = source.directories();
    assert_eq!(
        dirs[0],
        Path::new(ROOTS[1]).join("actual-package-directory")
    );
    assert_eq!(
        icon_path(&dirs[0], source.large_icon.as_deref().unwrap()),
        Some(dirs[0].join("resources/hu/large.png"))
    );
    let absolute = format!("file://{}/sample/icon.png", ROOTS[2]);
    assert_eq!(
        icon_path(&dirs[0], &absolute),
        Some(Path::new(ROOTS[2]).join("sample/icon.png"))
    );
    for bad in [
        "../../etc/passwd",
        "/etc/passwd",
        "https://example.com/icon.png",
        "icon\0.png",
    ] {
        assert!(icon_path(&dirs[0], bad).is_none());
    }
    let source = Source::new("sample", &json!({"folderPath":"/etc"}));
    assert_eq!(source.directories().len(), 3);
    assert_eq!(source.directories()[0], Path::new(ROOTS[0]).join("sample"));
}

#[test]
fn supported_icons_decode_to_bounded_pixels_and_runtime_card_colours() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
        let artwork = decode(&encoded(format, 400, 200, [48, 80, 112, 255])).unwrap();
        assert_eq!(artwork.pixels.dimensions(), (256, 128));
        for (actual, expected) in artwork.color.into_iter().zip([48u8, 80, 112]) {
            assert!(actual.abs_diff(expected) <= 2);
        }
    }
    let transparent = decode(&encoded(ImageFormat::Png, 32, 32, [255, 0, 0, 0])).unwrap();
    assert_eq!(transparent.color, [20; 3]);
    assert!(decode(b"not an image").is_none());
    assert!(decode(&encoded(ImageFormat::Png, 2049, 1, [0; 4])).is_none());
}

#[test]
fn local_reads_fall_back_from_a_corrupt_large_icon_to_manifest_artwork() {
    let directory = Path::new(ROOTS[1]).join("sample");
    let manifest = json!({"id":"sample", "largeIcon":"corrupt.png", "icon":"local/icon.webp"});
    let files = HashMap::from([
        (
            directory.join("appinfo.json"),
            serde_json::to_vec(&manifest).unwrap(),
        ),
        (directory.join("corrupt.png"), b"broken".to_vec()),
        (
            directory.join("local/icon.webp"),
            encoded(ImageFormat::WebP, 32, 32, [8, 16, 24, 255]),
        ),
    ]);
    let source = Source::new("sample", &json!({"folderPath": directory}));
    let mut reads = Vec::new();
    let image = load_local(&source, |path, limit| {
        reads.push(path.to_owned());
        files
            .get(path)
            .filter(|bytes| bytes.len() <= limit)
            .cloned()
    })
    .unwrap();
    assert_eq!(image.pixels.get_pixel(0, 0).0, [8, 16, 24, 255]);
    assert_eq!(reads[0], directory.join("appinfo.json"));
    assert!(reads.contains(&directory.join("corrupt.png")));
}

#[test]
fn refresh_discards_old_worker_results_and_reloads_changed_artwork() {
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = calls.clone();
    let (started, received) = mpsc::channel();
    let ui_thread = thread::current().id();
    let service = Service::with_loader(true, move |_| {
        assert_ne!(thread::current().id(), ui_thread);
        let number = worker_calls.fetch_add(1, Ordering::SeqCst) + 1;
        started.send(number).unwrap();
        decode(&encoded(ImageFormat::Png, 2, 2, [number as u8, 0, 0, 255]))
    });
    let source = Source::new("sample", &json!({}));
    assert!(service.get(&source).is_none());
    assert_eq!(received.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
    service.reset();
    assert!(service.get(&source).is_none());
    poll_until(&service, || service.get(&source).is_some());
    let image = service.get(&source).unwrap();
    assert_eq!(image.pixels.get_pixel(0, 0)[0], 2);
    assert!(Arc::ptr_eq(&image, &service.get(&source).unwrap()));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn unavailable_icons_are_not_retried_on_every_redraw() {
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = calls.clone();
    let service = Service::with_loader(true, move |_| {
        worker_calls.fetch_add(1, Ordering::SeqCst);
        None
    });
    let source = Source::new("sample", &json!({}));
    assert!(service.get(&source).is_none());
    poll_until(&service, || {
        service.cache.borrow().entries[&source].image.is_some()
    });
    for _ in 0..10 {
        assert!(service.get(&source).is_none());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    service.reset();
    assert!(service.get(&source).is_none());
    poll_until(&service, || {
        service.cache.borrow().entries[&source].image.is_some()
    });
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn decoded_cache_evicts_old_icons_when_scrolling_through_many_apps() {
    let service = Service::with_loader(true, |_| {
        Some(Artwork {
            key: String::new(),
            pixels: RgbaImage::new(256, 256),
            color: [20; 3],
        })
    });
    for number in 0..80 {
        let source = Source::new(&format!("app-{number}"), &json!({}));
        service.get(&source);
        poll_until(&service, || {
            service.cache.borrow().entries[&source].image.is_some()
        });
    }
    let cache = service.cache.borrow();
    let bytes: usize = cache
        .entries
        .values()
        .filter_map(|entry| entry.image.as_ref()?.as_ref())
        .map(|image| image.pixels.as_raw().len())
        .sum();
    assert!(bytes <= CACHE_BYTES);
    assert!(
        !cache
            .entries
            .contains_key(&Source::new("app-0", &json!({})))
    );
    assert!(
        cache
            .entries
            .contains_key(&Source::new("app-79", &json!({})))
    );
}

#[test]
fn visible_cards_switch_from_fallback_to_installed_artwork_without_an_icon_map() {
    let assets = crate::view::Assets {
        root: PathBuf::new(),
        icons: Default::default(),
        colors: Default::default(),
        version: "test".into(),
        metrics: Default::default(),
        tiles: Default::default(),
        heroes: Default::default(),
        artwork: Service::with_loader(true, |_| {
            decode(&encoded(ImageFormat::Png, 32, 32, [24, 40, 56, 255]))
        }),
    };
    let apps =
        crate::platform::normalize_apps(&json!([{"id":"regional.app", "title":"Regional App"}]))
            .unwrap();
    let mut state = crate::state::State::new(Default::default());
    state.set_inventory(
        crate::model::Inventory {
            apps,
            inputs: vec![],
        },
        None,
    );
    let scene = || {
        crate::view::scene(&state, &assets, time::OffsetDateTime::UNIX_EPOCH)
    };
    assert!(
        !scene()
            .draw
            .iter()
            .any(|draw| matches!(draw, crate::renderer::Draw::AppImage(..)))
    );
    poll_until(&assets.artwork, || {
        scene()
            .draw
            .iter()
            .any(|draw| matches!(draw, crate::renderer::Draw::AppImage(..)))
    });
    let image = scene()
        .draw
        .into_iter()
        .find_map(|draw| match draw {
            crate::renderer::Draw::AppImage(image, _, _) => Some(image),
            _ => None,
        })
        .unwrap();
    assert_eq!(image.color, [24, 40, 56]);
}

#[test]
fn refresh_keeps_shown_icons_and_disk_cache_serves_them_without_a_read() {
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = calls.clone();
    let service = Service::with_loader(true, move |_| {
        worker_calls.fetch_add(1, Ordering::SeqCst);
        decode(&encoded(ImageFormat::Png, 2, 2, [9, 0, 0, 255]))
    });
    let source = Source::new("sample", &json!({}));
    assert!(service.get(&source).is_none());
    poll_until(&service, || service.get(&source).is_some());
    // An inventory refresh must not drop an icon already on screen.
    service.refresh();
    assert!(service.get(&source).is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let directory = std::env::temp_dir().join(format!("luma-icons-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let path = disk_path(&directory, &source);
    let artwork = decode(&encoded(ImageFormat::Png, 2, 2, [7, 0, 0, 255])).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    artwork.pixels.save_with_format(&path, ImageFormat::Png).unwrap();
    let cached = load_cached(Some(&directory), &source).unwrap();
    assert_eq!(cached.pixels.get_pixel(0, 0)[0], 7);
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn disk_cache_refuses_ids_that_could_leave_its_folder() {
    let directory = std::env::temp_dir().join(format!("luma-icons-guard-{}", std::process::id()));
    let source = Source::new("../escape", &json!({}));
    assert!(load_cached(Some(&directory), &source).is_none());
    assert!(!directory.exists());
}

#[test]
fn failed_reads_retry_by_themselves_with_growing_gaps() {
    assert_eq!(retry_delay(1), Duration::from_secs(15));
    assert_eq!(retry_delay(2), Duration::from_secs(30));
    assert_eq!(retry_delay(9), Duration::from_secs(600));
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = calls.clone();
    let service = Service::with_loader(true, move |_| {
        // Fail the first read (a service timeout), succeed after.
        if worker_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            None
        } else {
            decode(&encoded(ImageFormat::Png, 2, 2, [5, 0, 0, 255]))
        }
    });
    let source = Source::new("sample", &json!({}));
    assert!(service.get(&source).is_none());
    poll_until(&service, || service.cache.borrow().entries[&source].image.is_some());
    // Bring the retry forward rather than waiting 15 s in a test.
    service.cache.borrow_mut().entries.get_mut(&source).unwrap().retry_at =
        Some(std::time::Instant::now());
    poll_until(&service, || service.get(&source).is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn an_app_update_gets_a_fresh_icon_cache_key() {
    let directory = Path::new("/tmp/luma-icons");
    let old = Source::new("sample", &json!({"version": "1.0.0"}));
    let new = Source::new("sample", &json!({"version": "1.1.0"}));
    assert_ne!(old, new);
    assert_ne!(disk_path(directory, &old), disk_path(directory, &new));
}

#[test]
fn transparent_icons_whose_logo_touches_the_edge_keep_the_neutral_card() {
    let mut image = image::RgbaImage::new(48, 48);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        // A red logo across the middle that reaches the left and right edges.
        *pixel = if (20..28).contains(&y) {
            image::Rgba([220, 20, 20, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        };
        let _ = x;
    }
    assert_eq!(background(&image), [20; 3]);
}

