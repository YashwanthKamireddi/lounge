//! tvOS alerts: a glass panel in the middle of the dimmed screen, its title
//! and message centred above a column of full-width buttons.
use super::{
    Assets, Scene,
    shared::{PLATTER, c, text},
};
use crate::{
    i18n,
    renderer::Draw,
    state::{Rect, State, Target},
};

/// The panel's width, and the inset of everything inside it.
const WIDTH: f32 = 760.;
const PAD: f32 = 52.;
/// tvOS's default control height (66 pt minimum, HIG Accessibility).
const BUTTON: f32 = 80.;
const BUTTON_GAP: f32 = 16.;
const RADIUS: f32 = 44.;
/// The large curves get the TV's soft edge (see Draw::Soft).
const SOFT_EDGE: f32 = 0.8;

/// The alert before removing `id`: "Uninstall “Netflix”?", one line on what
/// goes with it, then Uninstall (system red) over Cancel. The buttons are
/// `Target::Alert(0)` and `(1)`; nothing else on screen takes focus.
pub(super) fn uninstall(out: &mut Scene, state: &State, assets: &Assets, id: &str) {
    let title = state
        .inventory
        .apps
        .iter()
        .find(|app| app.id == id)
        .map_or(id, |app| app.title.as_str());
    let buttons = [i18n::t("uninstall"), i18n::t("cancel")];
    alert(
        out,
        state,
        assets,
        &i18n::format("uninstallTitle", &[("app", title)]),
        i18n::t("uninstallMessage"),
        &buttons,
        Some(0),
    );
}

/// An alert: `title` (at most two lines), `message`, then `buttons` top to
/// bottom; `destructive` is the index drawn in red.
fn alert(
    out: &mut Scene,
    state: &State,
    assets: &Assets,
    title: &str,
    message: &str,
    buttons: &[&str],
    destructive: Option<usize>,
) {
    let inner = WIDTH - 2. * PAD;
    let title_style = text(38., 650, 0.).leading(46. / 38.);
    let message_style = text(27., 500, 0.).leading(35. / 27.);
    let title_lines = assets.lines(title, title_style, inner).min(2) as f32;
    let message_lines = assets.lines(message, message_style, inner) as f32;
    let height = PAD
        + title_lines * 46.
        + 14.
        + message_lines * 35.
        + 44.
        + buttons.len() as f32 * (BUTTON + BUTTON_GAP)
        - BUTTON_GAP
        + PAD;
    let panel = Rect::new((1920. - WIDTH) / 2., (1080. - height) / 2., WIDTH, height);
    out.modal = Some(panel);
    // Regular Liquid Glass, as the context menu and banners are, lifted off
    // the dimmed screen by one soft shadow.
    out.shadow(panel, RADIUS, 60., 24., 0.45);
    out.draw.push(Draw::Soft(Some(SOFT_EDGE)));
    out.glass(
        assets,
        state.wallpaper(),
        panel,
        RADIUS,
        out.theme.veil(0.72),
    );
    out.draw.push(Draw::RoundedGradient(
        panel,
        RADIUS,
        c(0xffffff, 0.1),
        c(0xffffff, 0.02),
    ));
    out.draw.push(Draw::Soft(None));
    out.rim(panel, RADIUS, 0.5, 0.08);
    let mut y = panel.y + PAD;
    out.text(
        title,
        Rect::new(panel.x + PAD, y, inner, title_lines * 46.),
        title_style,
        out.theme.ink(1.),
        true,
    );
    y += title_lines * 46. + 14.;
    out.text(
        message,
        Rect::new(panel.x + PAD, y, inner, message_lines * 35.),
        message_style,
        out.theme.ink(0.62),
        true,
    );
    y += message_lines * 35. + 44.;
    let label = text(29., 600, 0.);
    for (index, name) in buttons.iter().enumerate() {
        let target = Target::Alert(index);
        let focus = state.focus_amount(&target);
        let base = Rect::new(panel.x + PAD, y, inner, BUTTON);
        y += BUTTON + BUTTON_GAP;
        // Focused, a button lifts and turns white, as every tvOS control.
        let grow = 0.04 * focus;
        let button = Rect::new(
            base.x - base.w * grow / 2.,
            base.y - base.h * grow / 2.,
            base.w * (1. + grow),
            base.h * (1. + grow),
        );
        let radius = button.h / 2.;
        let first = out.draw.len();
        if focus > 0. {
            out.shadow(button, radius, 24., 8. * focus, 0.3 * focus);
        }
        out.draw.push(Draw::Soft(Some(SOFT_EDGE)));
        out.shape(button, radius, out.theme.wash(0.12 * (1. - focus)));
        out.shape(button, radius, c(0xffffff, 0.97 * focus));
        out.draw.push(Draw::Soft(None));
        let ink = if destructive == Some(index) {
            out.theme.destructive(focus)
        } else {
            let (rest, lit) = (out.theme.ink(1.), c(PLATTER, 1.));
            std::array::from_fn(|i| rest[i] + (lit[i] - rest[i]) * focus)
        };
        out.text(
            *name,
            Rect::new(button.x, button.y + (button.h - 36.) / 2., button.w, 36.),
            label,
            [ink[0], ink[1], ink[2], 1.],
            true,
        );
        if state.pressed.as_ref() == Some(&target) {
            out.fade(first, 1., 0.85);
        }
        out.control(target, base, false);
    }
}
