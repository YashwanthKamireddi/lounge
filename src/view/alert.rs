//! tvOS alerts: a glass panel in the middle of the dimmed screen, its title
//! and message centred above a column of full-width buttons.
use super::{
    Assets, Scene,
    shared::{PLATTER, c, text},
};
use crate::{
    i18n,
    renderer::{Draw, WHITE},
    state::{PhoneStep, Rect, State, Target},
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
        Alert {
            title: &i18n::format("uninstallTitle", &[("app", title)]),
            message: i18n::t("uninstallMessage"),
            buttons: &buttons,
            destructive: Some(0),
            figure: None,
        },
    );
}

/// Asked once on a TV that can have it: "Make Lounge your TV's Home?", what that means and
/// that it can be undone, then Make Home over Not Now.
pub(super) fn offer_home(out: &mut Scene, state: &State, assets: &Assets) {
    let buttons = [i18n::t("offerHomeYes"), i18n::t("notNow")];
    alert(
        out,
        state,
        assets,
        Alert {
            title: i18n::t("offerHomeTitle"),
            message: i18n::t("offerHomeMessage"),
            buttons: &buttons,
            destructive: None,
            figure: None,
        },
    );
}

/// The QR code for a photo from the phone: what to do, the code (with its address under it,
/// for typing), how far it got, then Done.
pub(super) fn phone(out: &mut Scene, state: &State, assets: &Assets) {
    let kind = state.phone.as_ref().map(|phone| phone.kind);
    let title = match kind {
        Some(crate::companion::Kind::Avatar) => i18n::t("phoneAvatarTvTitle"),
        _ => i18n::t("phoneWallpaperTvTitle"),
    };
    let message = match state.phone.as_ref().map(|phone| &phone.step) {
        None => i18n::t("phoneStarting"),
        Some(PhoneStep::Waiting) => i18n::t("phoneScan"),
        Some(PhoneStep::Opened) => i18n::t("phoneOpened"),
        Some(PhoneStep::Receiving) => i18n::t("phoneReceiving"),
        Some(PhoneStep::Failed(reason)) => i18n::t(reason),
    };
    let buttons = [i18n::t("done")];
    let draw = |out: &mut Scene, area: Rect| {
        let Some(phone) = &state.phone else { return };
        // A white card with a quiet zone of two modules, the modules in near-black runs.
        let card = Rect::new(area.x + (area.w - QR) / 2., area.y, QR, QR);
        out.shape(card, 24., c(0xffffff, 1.));
        // Whole pixels per module, so neighbours meet without a seam a camera reads as white;
        // each run also reaches a little into the next row for the same reason.
        let module = (QR / (phone.size + 4) as f32).floor();
        let side = module * phone.size as f32;
        let origin = (
            (card.x + (QR - side) / 2.).round(),
            (card.y + (QR - side) / 2.).round(),
        );
        // Runs both across and down: where two dark modules meet, one run covers the join, so
        // no hairline seam (which a camera reads as white) is left between them.
        let dark = |x: usize, y: usize| phone.modules[y * phone.size + x];
        let ink = c(0x111111, 1.);
        for line in 0..phone.size {
            for across in [true, false] {
                let at = |step: usize| if across { (step, line) } else { (line, step) };
                let mut step = 0;
                while step < phone.size {
                    let (x, y) = at(step);
                    if !dark(x, y) {
                        step += 1;
                        continue;
                    }
                    let run = (step..phone.size)
                        .take_while(|next| {
                            let (x, y) = at(*next);
                            dark(x, y)
                        })
                        .count();
                    let (w, h) = if across { (run, 1) } else { (1, run) };
                    out.shape(
                        Rect::new(
                            origin.0 + x as f32 * module,
                            origin.1 + y as f32 * module,
                            w as f32 * module,
                            h as f32 * module,
                        ),
                        0.,
                        ink,
                    );
                    step += run;
                }
            }
        }
        out.text(
            &phone.url,
            Rect::new(area.x, card.y + QR + 18., area.w, 32.),
            text(24., 500, 0.),
            c(0xffffff, 0.6),
            true,
        );
    };
    alert(
        out,
        state,
        assets,
        Alert {
            title,
            message,
            buttons: &buttons,
            destructive: None,
            figure: Some((QR + 18. + 32. + 8., &draw)),
        },
    );
}

/// The QR card's side.
const QR: f32 = 340.;

/// Drawn under an alert's message, in the height given.
type Figure<'a> = (f32, &'a dyn Fn(&mut Scene, Rect));

/// An alert's content: `title` (at most two lines), `message`, an optional `figure`, then
/// `buttons` top to bottom; `destructive` is the index drawn in red.
struct Alert<'a> {
    title: &'a str,
    message: &'a str,
    buttons: &'a [&'a str],
    destructive: Option<usize>,
    figure: Option<Figure<'a>>,
}

fn alert(out: &mut Scene, state: &State, assets: &Assets, content: Alert) {
    let Alert {
        title,
        message,
        buttons,
        destructive,
        figure,
    } = content;
    let figure_height = figure.map_or(0., |(height, _)| height + 36.);
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
        + figure_height
        + buttons.len() as f32 * (BUTTON + BUTTON_GAP)
        - BUTTON_GAP
        + PAD;
    let panel = Rect::new((1920. - WIDTH) / 2., (1080. - height) / 2., WIDTH, height);
    out.modal = Some(panel);
    // The same clear Liquid Glass as the dock, the pills and the menu (the wallpaper, blurred,
    // through it; white ink in both appearances), lifted off the dimmed screen by one shadow.
    out.shadow(panel, RADIUS, 60., 24., 0.45);
    out.draw.push(Draw::Soft(Some(SOFT_EDGE)));
    out.glass_over(
        super::glass::home_backdrop(state, assets),
        panel,
        RADIUS,
        c(PLATTER, 0.34),
    );
    out.draw.push(Draw::Soft(None));
    out.rim(panel, RADIUS, 0.55, 0.06);
    let mut y = panel.y + PAD;
    out.text(
        title,
        Rect::new(panel.x + PAD, y, inner, title_lines * 46.),
        title_style,
        WHITE,
        true,
    );
    y += title_lines * 46. + 14.;
    out.text(
        message,
        Rect::new(panel.x + PAD, y, inner, message_lines * 35.),
        message_style,
        c(0xffffff, 0.68),
        true,
    );
    y += message_lines * 35. + 44.;
    if let Some((height, draw)) = figure {
        draw(out, Rect::new(panel.x + PAD, y, inner, height));
        y += figure_height;
    }
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
        // At rest a button is a lighter pane of the same glass with its rim; focused it turns
        // white, as every focused control does.
        out.shape(button, radius, c(0xffffff, 0.16 * (1. - focus)));
        out.shape(button, radius, c(0xffffff, 0.97 * focus));
        out.draw.push(Draw::Soft(None));
        out.rim(button, radius, 0.45 * (1. - focus), 0.05 * (1. - focus));
        let ink: [f32; 4] = if destructive == Some(index) {
            // Red that reads on the dark glass at rest, and the system red once focused.
            let (rest, lit) = (c(0xff6961, 1.), c(0xff3b30, 1.));
            std::array::from_fn(|i| rest[i] + (lit[i] - rest[i]) * focus)
        } else {
            let (rest, lit) = (WHITE, c(PLATTER, 1.));
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
