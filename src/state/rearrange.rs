//! Move: where tiles glide while the owner rearranges the grid.
use super::*;

impl State {
    /// Where home slot `index` sits relative to the first, in pixels: the dock
    /// is row 0, the grid's rows follow it (their gap opens as the grid rises).
    pub fn slot_position(&self, index: usize) -> (f32, f32) {
        let (row, column) = (index / APPS_COLUMNS, index % APPS_COLUMNS);
        let spread = (self.shown_scroll / GRID_SCROLL).clamp(0., 1.);
        let gap = DOCK_GAP + (GRID_GAP - DOCK_GAP) * spread;
        let y = if row == 0 {
            0.
        } else {
            gap + (row - 1) as f32 * ROW_PITCH
        };
        (column as f32 * GRID_STEP, y)
    }

    /// How far each gliding tile is drawn from its slot right now.
    pub fn glide_offsets(&self) -> Glide {
        let Some((at, offsets)) = &self.rearranged else {
            return Default::default();
        };
        let left = 1. - Self::glide_eased(*at);
        offsets
            .iter()
            .map(|(id, (dx, dy))| (id.clone(), (dx * left, dy * left)))
            .collect()
    }

    /// How far the app in Move has lifted out of the grid (0-1): quickly at
    /// first, settling softly, as tvOS raises it.
    pub fn move_raise(&self) -> f32 {
        self.moving_since.map_or(1., |since| {
            let t = (crate::clock::since(since).as_secs_f32() / 0.16).min(1.);
            1. - (1. - t).powi(3)
        })
    }

    /// Progress (0-1) of a Move glide that began at `at`: an ease-out cubic.
    pub fn glide_eased(at: Instant) -> f32 {
        let t = (crate::clock::since(at).as_secs_f32() * 1000. / REARRANGE_MS).min(1.);
        1. - (1. - t).powi(3)
    }
}
