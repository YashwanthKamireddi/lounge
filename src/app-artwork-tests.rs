use super::*;
use image::{DynamicImage, ImageFormat, Rgba};
use serde_json::json;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

fn png(image: RgbaImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

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
        for stop in artwork.backdrop {
            for (actual, expected) in stop.into_iter().zip([48u8, 80, 112]) {
                assert!(actual.abs_diff(expected) <= 2);
            }
        }
    }
    // Nothing visible at all: the neutral slate backdrop.
    let transparent = decode(&encoded(ImageFormat::Png, 32, 32, [255, 0, 0, 0])).unwrap();
    assert!(
        transparent
            .backdrop
            .iter()
            .all(|stop| luminance(*stop) < 0.2)
    );
    assert!(decode(b"not an image").is_none());
    assert!(decode(&encoded(ImageFormat::Png, 2049, 1, [0; 4])).is_none());
}

fn luminance(rgb: [u8; 3]) -> f32 {
    let linear = |c: u8| {
        let c = c as f32 / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}
fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
/// A logo of one colour on a transparent background: a centred block plus text-like bars.
fn logo(rgb: [u8; 3]) -> RgbaImage {
    RgbaImage::from_fn(200, 200, |x, y| {
        let block = (60..140).contains(&x) && (40..110).contains(&y);
        let bars = (30..170).contains(&x) && (130..160).contains(&y) && x % 12 < 7;
        if block || bars {
            Rgba([rgb[0], rgb[1], rgb[2], 255])
        } else {
            Rgba([0; 4])
        }
    })
}

#[test]
fn transparent_logos_get_a_contrasting_gradient_in_their_own_hue() {
    for (rgb, light_backdrop) in [
        ([255, 255, 255], false),
        ([235, 235, 240], false),
        ([20, 20, 24], true),
        ([200, 30, 40], true),
        ([30, 80, 200], true),
        ([250, 210, 40], false),
        ([40, 200, 120], false),
    ] {
        let backdrop = decode(&png(logo(rgb))).unwrap().backdrop;
        for stop in backdrop {
            assert!(
                contrast(stop, rgb) >= 3.,
                "{rgb:?} on {stop:?}: contrast {}",
                contrast(stop, rgb)
            );
            assert_eq!(luminance(stop) > 0.3, light_backdrop, "{rgb:?} on {stop:?}");
        }
        assert_ne!(
            backdrop[0], backdrop[1],
            "{rgb:?}: a gradient, not a flat fill"
        );
        assert!(
            luminance(backdrop[0]) > luminance(backdrop[1]),
            "{rgb:?}: lighter at the top"
        );
        let saturated = rgb.iter().max().unwrap() - rgb.iter().min().unwrap() > 100;
        if saturated {
            // The backdrop leans to the logo's strongest channel.
            let strongest = (0..3).max_by_key(|&i| rgb[i]).unwrap();
            for stop in backdrop {
                let lean = (0..3).max_by_key(|&i| stop[i]).unwrap();
                assert_eq!(lean, strongest, "{rgb:?} on {stop:?}");
            }
        }
    }
}

#[test]
fn transparent_edges_keep_the_logo_colour_when_scaled() {
    // White on fully transparent black, as many PNG exporters write it: scaling must not
    // pull the transparent black into the antialiased edge, and the GPU's linear filter
    // must find the logo's colour in the transparent texels next to it.
    let image = RgbaImage::from_fn(600, 600, |x, y| {
        if (150..450).contains(&x) && (250..350).contains(&y) {
            Rgba([255, 255, 255, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let pixels = decode(&png(image)).unwrap().pixels;
    for pixel in pixels.pixels().filter(|pixel| pixel[3] > 0) {
        assert!(pixel[0].min(pixel[1]).min(pixel[2]) >= 245, "{pixel:?}");
    }
    let beside = pixels.get_pixel(128, 103);
    assert_eq!(beside[3], 0);
    assert!(beside[0] >= 245, "transparent neighbour {beside:?}");
}

#[test]
fn transparent_logos_reaching_the_edges_keep_the_neutral_card() {
    // Like Litefin's 130px icon: a ring logo on a transparent background whose
    // stroke reaches into the sampled edge band.
    let ring = RgbaImage::from_fn(130, 130, |x, y| {
        let distance = (x as f32 - 64.5).hypot(y as f32 - 64.5);
        if (52.0..62.0).contains(&distance) {
            Rgba([150, 100, 210, 255])
        } else {
            Rgba([0; 4])
        }
    });
    let backdrop = decode(&png(ring)).unwrap().backdrop;
    for stop in backdrop {
        assert!(contrast(stop, [150, 100, 210]) >= 3., "{stop:?}");
    }
    // Opaque artwork with transparent rounded corners still takes its edge colour.
    let rounded = RgbaImage::from_fn(130, 130, |x, y| {
        let inset = |v: u32| v.min(129 - v) as f32;
        let (dx, dy) = (24. - inset(x), 24. - inset(y));
        if dx > 0. && dy > 0. && dx.hypot(dy) > 24. {
            Rgba([0; 4])
        } else {
            Rgba([40, 90, 160, 255])
        }
    });
    for stop in decode(&png(rounded)).unwrap().backdrop {
        for (actual, expected) in stop.into_iter().zip([40u8, 90, 160]) {
            assert!(actual.abs_diff(expected) <= 2);
        }
    }
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
fn hundreds_of_apps_stay_within_the_cache_bounds_across_refreshes() {
    let service = Service::with_loader(true, |_| {
        Some(Artwork {
            key: String::new(),
            pixels: RgbaImage::new(256, 256),
            backdrop: [[20; 3]; 2],
        })
    });
    for number in 0..300 {
        if number % 100 == 0 {
            service.refresh();
        }
        let source = Source::new(&format!("app-{number}"), &json!({"version": "1"}));
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
    assert!(cache.entries.len() <= CACHE_ENTRIES);
}

#[test]
fn decoded_cache_evicts_old_icons_when_scrolling_through_many_apps() {
    let service = Service::with_loader(true, |_| {
        Some(Artwork {
            key: String::new(),
            pixels: RgbaImage::new(256, 256),
            backdrop: [[20; 3]; 2],
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
    assert_eq!(image.backdrop, [[24, 40, 56]; 2]);
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

    let directory = std::env::temp_dir().join(format!("lounge-icons-{}", std::process::id()));
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
    let directory = std::env::temp_dir().join(format!("lounge-icons-guard-{}", std::process::id()));
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
    let directory = Path::new("/tmp/lounge-icons");
    let old = Source::new("sample", &json!({"version": "1.0.0"}));
    let new = Source::new("sample", &json!({"version": "1.1.0"}));
    assert_ne!(old, new);
    assert_ne!(disk_path(directory, &old), disk_path(directory, &new));
}

