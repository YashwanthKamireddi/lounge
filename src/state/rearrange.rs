//! Move: where tiles glide while the owner rearranges the grid.
use super::*;

impl State {
    /// Where home slot `index` sits relative to the first, in pixels: the dock
    /// is row 0, any rows above it come next, then the grid's rows (their gap
    /// opens as the grid rises).
    pub fn slot_position(&self, index: usize) -> (f32, f32) {
        let (row, column) = (index / APPS_COLUMNS, index % APPS_COLUMNS);
        let spread = (self.shown_scroll / GRID_SCROLL).clamp(0., 1.);
        let gap = DOCK_GAP + (GRID_GAP - DOCK_GAP) * spread;
        let above = self.rows_above(self.apps().len());
        (column as f32 * GRID_STEP, row_offset(row, above, gap))
    }

    /// How many of the rows after the dock's are drawn above it, for `count` apps: what
    /// Settings asks for, as far as there are apps to fill them.
    pub fn rows_above(&self, count: usize) -> usize {
        self.preferences
            .rows_above_dock
            .min(MAX_ROWS_ABOVE)
            .min(count.saturating_sub(1) / APPS_COLUMNS)
    }

    /// After apps leave the grid (hidden, uninstalled), the ones behind them glide into the gap
    /// from where they are drawn, the way a Move reflows, instead of jumping there. `before` is
    /// the order as it was shown.
    pub fn glide_from(&mut self, before: &[String]) {
        if self.preferences.reduced_motion {
            return;
        }
        let now: Vec<String> = self.apps().iter().map(|app| app.id.clone()).collect();
        if let Some((slot, id)) = before.iter().enumerate().find(|(_, id)| !now.contains(id)) {
            self.leaving = Some((id.clone(), slot, crate::clock::now()));
        }
        let mut offsets = self.glide_offsets();
        self.entering.clear();
        for (new, id) in now.iter().enumerate() {
            let Some(old) = before.iter().position(|item| item == id) else {
                continue;
            };
            let ((ox, oy), (nx, ny)) = (self.slot_position(old), self.slot_position(new));
            let (dx, dy) = offsets.get(id).copied().unwrap_or_default();
            // A tile that changes rows would cut diagonally across the others: it slides in
            // from just beyond its new slot instead, fading in, while its old row closes up.
            let offset = if old / APPS_COLUMNS == new / APPS_COLUMNS {
                (dx + ox - nx, dy + oy - ny)
            } else {
                self.entering.push(id.clone());
                (GRID_STEP * 0.3, 0.)
            };
            let app = id;
            if offset.0.abs() + offset.1.abs() > 0.5 {
                offsets.insert(app.clone(), offset);
            } else {
                offsets.remove(app);
            }
        }
        if !offsets.is_empty() {
            // The gap closes a beat after the app starts to leave (as tvOS's), so nothing slides
            // over it while it is still shrinking away.
            let wait = if self.leaving.is_some() { 90 } else { 0 };
            self.rearranged = Some((
                crate::clock::now() + std::time::Duration::from_millis(wait),
                offsets,
            ));
            self.dirty = true;
        }
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

/// Where row `row` sits relative to the dock, with `above` rows over it: rows 1 to `above`
/// stack upward from the dock, top to bottom in order; the grid's rows follow `gap` below it.
pub fn row_offset(row: usize, above: usize, gap: f32) -> f32 {
    if row == 0 {
        0.
    } else if row <= above {
        -((above - row + 1) as f32) * ROW_PITCH
    } else {
        gap + (row - above - 1) as f32 * ROW_PITCH
    }
}

/// The slot one row up or down from `index` as the rows are drawn (those above the dock, the
/// dock, then the grid), in the same column or the last app; None past the top or bottom.
pub fn vertical_neighbour(index: usize, count: usize, above: usize, up: bool) -> Option<usize> {
    let (row, column) = (index / APPS_COLUMNS, index % APPS_COLUMNS);
    let rows = count.div_ceil(APPS_COLUMNS);
    // Rows by where they are drawn, top first, and back.
    let drawn = |row: usize| match row {
        0 => above,
        row if row <= above => row - 1,
        row => row,
    };
    let row_at = |position: usize| match position {
        position if position < above => position + 1,
        position if position == above => 0,
        position => position,
    };
    let position = if up {
        drawn(row).checked_sub(1)?
    } else {
        drawn(row) + 1
    };
    (position < rows).then(|| (row_at(position) * APPS_COLUMNS + column).min(count - 1))
}
