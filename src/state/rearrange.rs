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
