//! Repaint only what changed. On the TV's single-core GPU a full 1080p
//! repaint costs ~38 ms, so a frame where only focus moved redraws just the
//! area that differs from the frame the buffer last held
//! (EGL_KHR_partial_update with its buffer age). Anything uncertain repaints
//! everything, so the worst case is today's full repaint.
#![cfg_attr(
    not(all(feature = "webos", target_os = "linux", target_arch = "arm")),
    allow(dead_code)
)]
use crate::{renderer::Draw, state::Rect};
use std::collections::VecDeque;

const SCREEN: Rect = Rect { x: 0., y: 0., w: 1920., h: 1080. };
/// Past this share of the screen a full repaint costs no more.
const FULL_SHARE: f32 = 0.55;

/// The area two frames' draw lists differ in, or `None` for everything.
pub fn diff(previous: &[Draw], current: &[Draw]) -> Option<Rect> {
    let mut area: Option<Rect> = None;
    let mut add = |rect: Option<Rect>| -> bool {
        let Some(rect) = rect else {
            return false;
        };
        area = Some(area.map_or(rect, |area| union(area, rect)));
        true
    };
    for index in 0..previous.len().max(current.len()) {
        let (before, after) = (previous.get(index), current.get(index));
        if let (Some(a), Some(b)) = (before, after)
            && same(a, b)
        {
            continue;
        }
        for draw in [before, after].into_iter().flatten() {
            if !add(bounds(draw)) {
                return None;
            }
        }
    }
    // Nothing differs: an empty area (nothing to repaint).
    let Some(area) = area else {
        return Some(Rect::default());
    };
    let area = intersect(area, SCREEN);
    (area.w * area.h <= FULL_SHARE * SCREEN.w * SCREEN.h).then_some(area)
}

/// What a draw can touch on screen; `None` when it may touch anything.
fn bounds(draw: &Draw) -> Option<Rect> {
    let grow = |rect: &Rect, by: f32| Rect::new(rect.x - by, rect.y - by, rect.w + 2. * by, rect.h + 2. * by);
    match draw {
        Draw::Shape(rect, ..) | Draw::RoundedGradient(rect, ..) | Draw::GradientRing(rect, ..) => Some(grow(rect, 3.)),
        Draw::Shadow(rect, _, blur, _) => Some(grow(rect, blur + 3.)),
        Draw::Text(_, rect, ..) => Some(grow(rect, 6.)),
        Draw::Image(_, rect, ..)
        | Draw::TintedImage(_, rect, ..)
        | Draw::AppImage(_, rect, _)
        | Draw::Glass(_, rect, ..) => Some(grow(rect, 3.)),
        // Full-screen, or changes how everything after it is drawn.
        Draw::Dissolve(..) | Draw::Clip(_) | Draw::EdgeFade(_) => None,
    }
}

fn same(a: &Draw, b: &Draw) -> bool {
    use Draw::*;
    match (a, b) {
        (Shape(r1, a1, b1, c1), Shape(r2, a2, b2, c2)) => r1 == r2 && a1 == a2 && b1 == b2 && c1 == c2,
        (RoundedGradient(r1, a1, c1, d1), RoundedGradient(r2, a2, c2, d2)) => r1 == r2 && a1 == a2 && c1 == c2 && d1 == d2,
        (GradientRing(r1, a1, w1, c1, d1), GradientRing(r2, a2, w2, c2, d2)) => {
            r1 == r2 && a1 == a2 && w1 == w2 && c1 == c2 && d1 == d2
        }
        (Shadow(r1, a1, b1, c1), Shadow(r2, a2, b2, c2)) => r1 == r2 && a1 == a2 && b1 == b2 && c1 == c2,
        (Text(t1, r1, s1, c1, m1), Text(t2, r2, s2, c2, m2)) => t1 == t2 && r1 == r2 && s1 == s2 && c1 == c2 && m1 == m2,
        (Image(p1, r1, a1, k1), Image(p2, r2, a2, k2)) => p1 == p2 && r1 == r2 && a1 == a2 && k1 == k2,
        (TintedImage(p1, r1, a1, k1, c1), TintedImage(p2, r2, a2, k2, c2)) => {
            p1 == p2 && r1 == r2 && a1 == a2 && k1 == k2 && c1 == c2
        }
        (Glass(p1, r1, a1, c1, d1), Glass(p2, r2, a2, c2, d2)) => {
            p1 == p2 && r1 == r2 && a1 == a2 && c1 == c2 && d1 == d2
        }
        (AppImage(i1, r1, c1), AppImage(i2, r2, c2)) => std::sync::Arc::ptr_eq(i1, i2) && r1 == r2 && c1 == c2,
        (Dissolve(p1, q1, m1, c1), Dissolve(p2, q2, m2, c2)) => p1 == p2 && q1 == q2 && m1 == m2 && c1 == c2,
        (Clip(a), Clip(b)) => a == b,
        (EdgeFade(a), EdgeFade(b)) => a == b,
        _ => false,
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    // An empty area adds nothing (it must not stretch the box to its corner).
    if a.w <= 0. || a.h <= 0. {
        return b;
    }
    if b.w <= 0. || b.h <= 0. {
        return a;
    }
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect::new(x, y, (a.x + a.w).max(b.x + b.w) - x, (a.y + a.h).max(b.y + b.h) - y)
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    Rect::new(x, y, ((a.x + a.w).min(b.x + b.w) - x).max(0.), ((a.y + a.h).min(b.y + b.h) - y).max(0.))
}

/// Each presented frame's damage, newest first, to cover a buffer that is
/// several frames old.
#[derive(Default)]
pub struct Tracker {
    history: VecDeque<Option<Rect>>,
}

impl Tracker {
    /// What to repaint now, given this frame's change and the buffer's age
    /// (frames since it was last drawn; 0 = unknown). `None` = everything.
    pub fn region(&self, change: Option<Rect>, age: i32) -> Option<Rect> {
        if age < 1 || age as usize > self.history.len() + 1 {
            return None;
        }
        let mut area = change?;
        for past in self.history.iter().take(age as usize - 1) {
            area = union(area, (*past)?);
        }
        (area.w * area.h <= FULL_SHARE * SCREEN.w * SCREEN.h).then_some(area)
    }

    pub fn presented(&mut self, change: Option<Rect>) {
        self.history.push_front(change);
        self.history.truncate(4);
    }

}

/// The TV's EGL surface, when it supports partial updates.
#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
pub struct Egl {
    display: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
    set_damage: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *const i32, i32) -> u32,
}

#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetCurrentDisplay() -> *mut std::ffi::c_void;
    fn eglGetCurrentSurface(read_draw: i32) -> *mut std::ffi::c_void;
    fn eglQueryString(display: *mut std::ffi::c_void, name: i32) -> *const std::ffi::c_char;
    fn eglQuerySurface(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void, attribute: i32, value: *mut i32) -> u32;
    fn eglGetProcAddress(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
}

#[cfg(all(feature = "webos", target_os = "linux", target_arch = "arm"))]
impl Egl {
    const EXTENSIONS: i32 = 0x3055;
    const DRAW: i32 = 0x3059;
    const BUFFER_AGE: i32 = 0x313D;

    /// The current context's surface, if EGL_KHR_partial_update is offered.
    pub fn current() -> Option<Self> {
        // SAFETY: plain EGL queries on the context SDL made current on this
        // thread; every pointer is checked before use.
        unsafe {
            let display = eglGetCurrentDisplay();
            let surface = eglGetCurrentSurface(Self::DRAW);
            if display.is_null() || surface.is_null() {
                return None;
            }
            let extensions = eglQueryString(display, Self::EXTENSIONS);
            if extensions.is_null()
                || !std::ffi::CStr::from_ptr(extensions)
                    .to_string_lossy()
                    .split(' ')
                    .any(|name| name == "EGL_KHR_partial_update")
            {
                return None;
            }
            let address = eglGetProcAddress(c"eglSetDamageRegionKHR".as_ptr());
            if address.is_null() {
                return None;
            }
            Some(Self {
                display,
                surface,
                set_damage: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *const i32, i32) -> u32,
                >(address),
            })
        }
    }

    /// Frames since the back buffer was last drawn; 0 when unknown.
    pub fn buffer_age(&self) -> i32 {
        let mut age = 0;
        // SAFETY: display and surface came from the current context.
        if unsafe { eglQuerySurface(self.display, self.surface, Self::BUFFER_AGE, &mut age) } == 0 {
            return 0;
        }
        age
    }

    /// Declares the area this frame repaints (the whole surface for `None`),
    /// in the surface's pixels with a bottom-left origin. False on refusal.
    pub fn declare(&self, area: Option<Rect>, width: u32, height: u32) -> bool {
        let Some(area) = area else {
            // SAFETY: an empty list declares the whole surface.
            return unsafe { (self.set_damage)(self.display, self.surface, std::ptr::null(), 0) } != 0;
        };
        let (sx, sy) = (width as f32 / 1920., height as f32 / 1080.);
        let x = (area.x * sx).floor() as i32;
        let w = ((area.x + area.w) * sx).ceil() as i32 - x;
        let top = (area.y * sy).floor() as i32;
        let h = ((area.y + area.h) * sy).ceil() as i32 - top;
        let rect = [x, height as i32 - top - h, w, h];
        // SAFETY: one rectangle of four ints, as the extension specifies.
        unsafe { (self.set_damage)(self.display, self.surface, rect.as_ptr(), 1) != 0 }
    }
}
