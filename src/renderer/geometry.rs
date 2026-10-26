//! Quad geometry helpers: edge bands and fill accounting.
use super::*;

/// The parts of `rect` a shape actually paints, as fractions of it. A rim (a
/// border-only shape) covers just its edge: four bands along it rather than
/// the whole rectangle, whose middle the shader would only fill with
/// transparent pixels (on the TV's one-core GPU the dock's rim alone was a
/// quarter of a screen per frame). Each band keeps the whole shape's
/// coordinates, so the pixels are the same.
pub(super) fn regions(rect: Rect, radius: f32, border: f32) -> Vec<[f32; 4]> {
    let edge = border + 3.;
    let corner = (radius.max(0.) + edge) / rect.h;
    let side = edge / rect.w;
    if border > 0. && corner < 0.45 && side < 0.2 {
        vec![
            [0., 0., 1., corner],
            [0., 1. - corner, 1., 1.],
            [0., corner, side, 1. - corner],
            [1. - side, corner, 1., 1. - corner],
        ]
    } else {
        vec![[0., 0., 1., 1.]]
    }
}

/// The share of the 1080p screen `rect` covers inside `clip`.
pub(super) fn screens(rect: Rect, clip: Option<Rect>) -> f32 {
    let bound = clip.unwrap_or(Rect::new(0., 0., 1920., 1080.));
    let x = rect.x.max(bound.x).max(0.);
    let y = rect.y.max(bound.y).max(0.);
    let right = (rect.x + rect.w).min(bound.x + bound.w).min(1920.);
    let bottom = (rect.y + rect.h).min(bound.y + bound.h).min(1080.);
    ((right - x).max(0.) * (bottom - y).max(0.)) / (1920. * 1080.)
}

#[cfg(test)]
mod tests {
    use super::{Rect, regions};

    #[test]
    fn a_rim_draws_only_bands_that_cover_its_edge() {
        let dock = Rect::new(50., 740., 1820., 274.);
        let (radius, border) = (72., 1.5);
        let bands = regions(dock, radius, border);
        assert_eq!(bands.len(), 4);
        // Every pixel the rim (and its anti-aliasing) can touch lies in a
        // band: within the corner rounding of the top and bottom, or within
        // border + aa of the left and right sides.
        let inside = |fx: f32, fy: f32| {
            bands
                .iter()
                .any(|[x0, y0, x1, y1]| fx >= *x0 && fx <= *x1 && fy >= *y0 && fy <= *y1)
        };
        let edge = (border + 2.5) / dock.w;
        for fy in [0.0, 0.1, 0.3, 0.5, 0.7, 0.9, 1.0] {
            assert!(
                inside(0.0, fy) && inside(edge, fy) && inside(1.0, fy) && inside(1.0 - edge, fy)
            );
        }
        for fx in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!(
                inside(fx, (radius + border) / dock.h)
                    && inside(fx, 1. - (radius + border) / dock.h)
            );
        }
        // The middle, which the shader would only fill with nothing, is not drawn.
        assert!(!inside(0.5, 0.5));
        let area: f32 = bands
            .iter()
            .map(|[x0, y0, x1, y1]| (x1 - x0) * (y1 - y0))
            .sum();
        assert!(area < 0.65);
    }

    #[test]
    fn fills_and_small_rims_draw_whole() {
        assert_eq!(
            regions(Rect::new(0., 0., 400., 200.), 30., 0.),
            vec![[0., 0., 1., 1.]]
        );
        // A capsule pill's rounding spans its height: no bands to save.
        assert_eq!(
            regions(Rect::new(0., 0., 360., 60.), 30., 1.5),
            vec![[0., 0., 1., 1.]]
        );
    }
}
