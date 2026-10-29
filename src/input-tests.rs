//! The remote mashed: thousands of random key, pointer, lifecycle and TV-reply
//! sequences against the real input, state and view code, on the stepped
//! clock. Whatever happens, nothing panics, focus stays on something drawn, a
//! launch only ever comes from a press that began and ended on a Lounge drawn
//! in front, and Back always gets back out.
use super::{HOLD, Remote};
use crate::{
    model::{Preferences, demo_inventory},
    presence::Presence,
    state::{Effect, Overlay, Page, State, Target},
    view,
};
use sdl2::keyboard::Keycode;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

/// xorshift64*: a fixed seed per run, so a failure replays exactly.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

fn assets() -> view::Assets {
    view::Assets {
        root: PathBuf::new(),
        icons: BTreeMap::new(),
        colors: BTreeMap::new(),
        version: "test".into(),
        metrics: BTreeMap::new(),
        tiles: Default::default(),
        heroes: Default::default(),
        artwork: crate::app_artwork::Service::new(false),
    }
}

/// What the harness knows about the press in progress, independently of
/// `Remote`: the stretch on screen OK (or the click) went down in, and what
/// was focused and drawn then.
#[derive(Clone)]
struct Down {
    front: u64,
    target: Target,
}

struct Harness {
    state: State,
    remote: Remote,
    presence: Presence,
    assets: view::Assets,
    scene: view::Scene,
    random: Random,
    /// Launch replies still on their way, with how many steps until each lands.
    replies: Vec<u32>,
    ok: Option<Down>,
    click: Option<Down>,
    launches: usize,
    /// The latest steps, shown when a check fails.
    trail: Vec<String>,
    started: std::time::Instant,
}

const KEYS: [Keycode; 14] = [
    Keycode::Up,
    Keycode::Down,
    Keycode::Left,
    Keycode::Right,
    Keycode::Up,
    Keycode::Down,
    Keycode::Left,
    Keycode::Right,
    Keycode::Escape,
    Keycode::M,
    Keycode::Num1,
    Keycode::Num2,
    Keycode::F5,
    Keycode::Backspace,
];

impl Harness {
    fn new(seed: u64) -> Self {
        let mut state = State::new(Preferences::default());
        state.set_inventory(demo_inventory(), None);
        let assets = assets();
        let scene = crate::compose(&mut state, &assets, time::OffsetDateTime::UNIX_EPOCH, None);
        let mut harness = Self {
            state,
            remote: Remote::new(),
            presence: Presence::new(),
            assets,
            scene,
            random: Random(seed | 1),
            replies: vec![],
            ok: None,
            click: None,
            launches: 0,
            trail: vec![],
            started: crate::clock::now(),
        };
        harness.frame();
        harness
    }

    /// One loop pass's frame: composed and, on screen, presented.
    fn frame(&mut self) {
        crate::clock::step(Duration::from_millis(16));
        self.state.tick();
        self.scene = crate::compose(
            &mut self.state,
            &self.assets,
            time::OffsetDateTime::UNIX_EPOCH,
            Some(&self.scene),
        );
        self.presence.presented(self.ticks());
        self.state.dirty = false;
    }

    /// The stepped clock as SDL ticks (input is stamped with these).
    fn ticks(&self) -> u32 {
        crate::clock::since(self.started).as_millis() as u32
    }

    /// Drawn now, or a switcher card being glided to (focused before it is
    /// in view).
    fn drawn(&self, target: &Target) -> bool {
        let card = match (target, &self.state.switcher) {
            (Target::Switch(index), Some(switcher)) => *index < switcher.apps.len(),
            _ => false,
        };
        card || self
            .scene
            .controls
            .iter()
            .any(|control| &control.target == target)
    }

    /// What the main loop does with effects, as the TV build runs them, with
    /// every launch checked against the press that asked for it.
    fn apply(&mut self, effects: Vec<Effect>, from: Option<Down>, what: &str) {
        for effect in effects {
            match effect {
                Effect::Launch(id) => {
                    let down = from
                        .clone()
                        .unwrap_or_else(|| panic!("{what} launched {id} with no press behind it"));
                    assert_eq!(
                        self.presence.front(self.ticks()),
                        Some(down.front),
                        "{what} launched {id}, but the press did not begin and end on the same stretch on screen"
                    );
                    match &down.target {
                        Target::App(app) | Target::Input(app) | Target::InputPick(app) => {
                            assert_eq!(app, &id)
                        }
                        Target::Switch(_) => {}
                        other => panic!("{what} launched {id} from {other:?}"),
                    }
                    self.launches += 1;
                    if !self.state.launch_busy() {
                        self.state.note_launch(&id);
                        self.state.launching = true;
                        self.state.launch_started = Some(crate::clock::now());
                        self.replies.push(1 + self.random.below(40) as u32);
                    }
                }
                Effect::Running => {
                    let mut open: Vec<String> = self
                        .state
                        .inventory
                        .apps
                        .iter()
                        .filter(|app| !app.brand.starts_with("lounge-"))
                        .map(|app| app.id.clone())
                        .collect();
                    let keep = self.random.below(open.len().min(5) + 1);
                    open.truncate(keep);
                    self.state.open_switcher(open);
                }
                Effect::Refresh => {
                    // The TV answers with the apps it has: now and then one
                    // went away meanwhile (the focused one, for all we know).
                    let mut inventory = demo_inventory();
                    if self.random.chance(30) {
                        let gone = self.random.below(inventory.apps.len());
                        inventory.apps.remove(gone);
                    }
                    self.state.set_inventory(inventory, None);
                }
                Effect::Uninstall(id) => {
                    let result = if self.random.chance(50) {
                        Ok(())
                    } else {
                        Err("refused".into())
                    };
                    let next = self.state.uninstalled(&id, result);
                    self.state.queued.extend(next);
                }
                Effect::TvHome(_) => {
                    self.state.tv_home_busy = false;
                    self.state.tv_home = Some(self.random.chance(50));
                }
                Effect::Save
                | Effect::Close(_)
                | Effect::Quit
                | Effect::Weather(_)
                | Effect::ClockFormat(_) => {}
            }
        }
        if self.state.moving.is_none() {
            self.state.unsaved_order = false;
        }
        self.remote.settle(&mut self.state);
    }

    fn step(&mut self) {
        // Each loop pass ticks (and composes, in front) before reading input.
        self.state.tick();
        if self.state.dirty && self.presence.in_front() {
            self.scene = crate::compose(
                &mut self.state,
                &self.assets,
                time::OffsetDateTime::UNIX_EPOCH,
                Some(&self.scene),
            );
        }
        // Some input is read well after SDL stamped it (the loop was busy
        // in a swap): it counts by its stamp.
        let stamped = if self.random.chance(15) {
            self.ticks().saturating_sub(self.random.below(600) as u32)
        } else {
            self.ticks()
        };
        let front = self.presence.front(stamped);
        let roll = self.random.below(100);
        let before = format!(
            "{roll}: focus {:?}, overlay {}, switcher {}, front {:?}",
            self.state.focus,
            self.state.overlay.is_some(),
            self.state.switcher.is_some(),
            front,
        );
        if self.trail.len() >= 24 {
            self.trail.remove(0);
        }
        self.trail.push(before);
        match roll {
            // Arrows, Back, menu, number keys, refresh: down, sometimes held.
            0..=29 => {
                let key = KEYS[self.random.below(KEYS.len())];
                let repeat = self.random.chance(20);
                let quick = matches!(key, Keycode::Num1 | Keycode::Num2);
                let down = front.map(|front| Down {
                    front,
                    target: self.state.focus.clone(),
                });
                let effects = self.remote.key_down(
                    key,
                    repeat,
                    true,
                    front,
                    &mut self.state,
                    &self.scene.controls,
                );
                if quick && !effects.is_empty() {
                    let Some(Effect::Launch(id)) = effects.first().map(|effect| match effect {
                        Effect::Launch(id) => Effect::Launch(id.clone()),
                        _ => Effect::Save,
                    }) else {
                        panic!("a number key asked for {effects:?}");
                    };
                    let down = down.expect("a number key acted on a Lounge not in front");
                    self.apply(
                        effects,
                        Some(Down {
                            front: down.front,
                            target: Target::App(id.clone()),
                        }),
                        "number key",
                    );
                } else {
                    self.apply(effects, None, "key");
                }
            }
            30..=41 => {
                let repeat = self.random.chance(15);
                let effects = self.remote.key_down(
                    Keycode::Return,
                    repeat,
                    true,
                    front,
                    &mut self.state,
                    &self.scene.controls,
                );
                if !repeat {
                    self.click = None;
                    self.ok = front.filter(|_| self.remote.press.is_some()).map(|front| {
                        assert!(
                            self.drawn(&self.state.focus) || self.scene.controls.is_empty(),
                            "OK went down on {:?}, which is not drawn",
                            self.state.focus
                        );
                        Down {
                            front,
                            target: self.state.focus.clone(),
                        }
                    });
                }
                self.apply(effects, None, "OK down");
            }
            42..=53 => {
                let down = self.ok.take();
                let effects = self.remote.key_up(Keycode::Return, front, &mut self.state);
                self.apply(effects, down, "OK up");
            }
            // The Magic Remote: hover, click, right-click, release.
            54..=59 => {
                let hovered = self.pick_control();
                self.remote.pointer_moved(hovered, front, &mut self.state);
                self.apply(vec![], None, "pointer move");
            }
            60..=64 => {
                let target = self.pick_control();
                let right = self.random.chance(15);
                self.remote
                    .pointer_down(target, right, front, &mut self.state);
                self.ok = None;
                self.click = self
                    .remote
                    .press
                    .as_ref()
                    .filter(|press| press.pointer)
                    .map(|press| Down {
                        front: press.front,
                        target: press.target.clone(),
                    });
                self.apply(vec![], None, "pointer down");
            }
            65..=69 => {
                let target = if self.random.chance(70) {
                    self.click.as_ref().map(|down| down.target.clone())
                } else {
                    self.pick_control()
                };
                let outside = self.random.chance(20);
                let down = self.click.take();
                let effects = self
                    .remote
                    .pointer_up(target, outside, front, &mut self.state);
                self.apply(effects, down, "click");
            }
            // Time passes: holds come due, animations run, replies land.
            70..=79 => {
                let ms =
                    [16, 16, 50, 120, 300, HOLD.as_millis() as u64 + 20, 900][self.random.below(7)];
                crate::clock::step(Duration::from_millis(ms));
                let down = self.ok.clone().or_else(|| self.click.clone());
                let effects = self.remote.hold_due(front, &mut self.state);
                self.apply(effects, down, "hold");
            }
            80..=84 => {
                for reply in &mut self.replies {
                    *reply = reply.saturating_sub(1);
                }
                if let Some(index) = self.replies.iter().position(|left| *left == 0) {
                    self.replies.remove(index);
                    self.state.launching = false;
                    if self.random.chance(20) {
                        self.state.notify("could not open");
                    }
                }
                let queued = std::mem::take(&mut self.state.queued);
                self.apply(queued, None, "queued");
            }
            // Lounge goes and comes: an app takes over, Home brings it back,
            // the screen saver or a system panel takes focus, sam relaunches.
            85..=88 => {
                self.remote.press = None;
                self.presence.hide();
            }
            89..=91 => {
                if self.presence.show() {
                    self.remote.press = None;
                }
                self.state.dirty = true;
            }
            92..=93 => {
                self.remote.press = None;
                self.presence.leave(self.state.launching);
            }
            94..=96 => {
                self.remote.press = None;
                let settled = self.presence.settled();
                self.presence.raise();
                self.state.home_key(settled);
                self.state.dirty = true;
            }
            _ => {}
        }
        self.remote.settle(&mut self.state);
        // The loop composes again after each event while in front, so the
        // next event hits what this one left on screen.
        if self.state.dirty && self.presence.in_front() {
            self.scene = crate::compose(
                &mut self.state,
                &self.assets,
                time::OffsetDateTime::UNIX_EPOCH,
                Some(&self.scene),
            );
        }
        match &self.remote.press {
            None => {
                self.ok = None;
                self.click = None;
            }
            Some(press) if press.pointer => self.ok = None,
            Some(_) => self.click = None,
        }
        // Most passes end with a frame; some events arrive between frames.
        if self.random.chance(70) {
            self.frame();
            self.check();
        }
    }

    fn pick_control(&mut self) -> Option<Target> {
        if self.scene.controls.is_empty() || self.random.chance(10) {
            return None;
        }
        let index = self.random.below(self.scene.controls.len());
        Some(self.scene.controls[index].target.clone())
    }

    /// After a frame: focus is on something the frame drew, and a press shows
    /// pressed only while it is down.
    fn check(&self) {
        if self.presence.visible && !self.scene.controls.is_empty() {
            assert!(
                self.drawn(&self.state.focus),
                "focus {:?} is on nothing drawn (page {:?}, overlay {:?}, switcher {}); scroll {} pointer {} expanded {} controls {:?} {:#?}",
                self.state.focus,
                self.state.page,
                self.state.overlay,
                self.state.switcher.is_some(),
                self.state.scroll,
                self.state.pointer,
                self.state.expanded,
                self.scene
                    .controls
                    .iter()
                    .map(|c| format!("{:?}", c.target))
                    .collect::<Vec<_>>(),
                self.trail
            );
        }
        if self.remote.press.is_none() {
            assert_eq!(self.state.pressed, None, "pressed with no press");
        }
    }

    /// Back, as often as it takes, then time for every fade: Lounge is back
    /// on its home with nothing open and nothing left moving.
    fn back_out(&mut self) {
        self.presence.show();
        self.frame();
        let front = self.presence.front(self.ticks());
        for _ in 0..12 {
            let effects = self.remote.key_down(
                Keycode::Escape,
                false,
                true,
                front,
                &mut self.state,
                &self.scene.controls,
            );
            // Back on the home opens the switcher; it is not pressed again here.
            let effects: Vec<_> = effects
                .into_iter()
                .filter(|effect| *effect != Effect::Running)
                .collect();
            self.apply(effects, None, "back");
            crate::clock::step(Duration::from_millis(400));
            self.frame();
        }
        for _ in 0..40 {
            crate::clock::step(Duration::from_millis(100));
            self.frame();
        }
        let state = &self.state;
        assert!(
            state.overlay.is_none(),
            "an overlay stayed: {:?}",
            state.overlay
        );
        assert!(state.switcher.is_none(), "the switcher stayed");
        assert!(state.moving.is_none(), "Move stayed");
        assert_eq!(state.page, Page::Apps, "Back did not reach the home");
        assert!(state.subpage.is_none());
        assert!(!state.animating(), "still animating after every fade");
        self.check();
    }
}

#[test]
fn mashing_the_remote_never_launches_unasked_or_gets_stuck() {
    crate::clock::record();
    let mut launches = 0;
    // LOUNGE_STRESS_RUNS raises the count for a deeper run.
    let runs = std::env::var("LOUNGE_STRESS_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(64u64);
    for seed in 1..=runs {
        let mut harness = Harness::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for _ in 0..250 {
            harness.step();
        }
        harness.back_out();
        launches += harness.launches;
    }
    // The run must actually have opened apps, or it proved nothing.
    assert!(launches > 50, "only {launches} launches in the whole run");
}

#[test]
fn a_press_that_began_before_lounge_was_drawn_launches_nothing() {
    crate::clock::record();
    let mut harness = Harness::new(7);
    let focus = harness.state.focus.clone();
    assert!(matches!(focus, Target::App(_)));
    // Waking from standby, or Home over an app: OK goes down as Lounge comes
    // back and is let go once it shows.
    harness.presence.hide();
    harness.presence.show();
    let front = harness.presence.front(harness.ticks());
    assert_eq!(front, None, "not drawn since it came back");
    let down = harness.remote.key_down(
        Keycode::Return,
        false,
        true,
        front,
        &mut harness.state,
        &harness.scene.controls,
    );
    assert!(down.is_empty());
    harness.frame();
    let up = harness.remote.key_up(
        Keycode::Return,
        harness.presence.front(harness.ticks()),
        &mut harness.state,
    );
    assert!(up.is_empty(), "the release alone opened {up:?}");
    // A number key and a click land the same way.
    harness.presence.raise();
    let front = harness.presence.front(harness.ticks());
    let quick = harness.remote.key_down(
        Keycode::Num1,
        false,
        true,
        front,
        &mut harness.state,
        &harness.scene.controls,
    );
    assert!(quick.is_empty());
    harness
        .remote
        .pointer_down(Some(focus.clone()), false, front, &mut harness.state);
    harness.frame();
    let click = harness.remote.pointer_up(
        Some(focus.clone()),
        false,
        harness.presence.front(harness.ticks()),
        &mut harness.state,
    );
    assert!(
        click.is_empty(),
        "the click's release alone opened {click:?}"
    );
    // OK down on Lounge, then focus taken by the screen saver before it is
    // let go: nothing, and nothing when it ends after Lounge is back either.
    let front = harness.presence.front(harness.ticks());
    harness.remote.key_down(
        Keycode::Return,
        false,
        true,
        front,
        &mut harness.state,
        &harness.scene.controls,
    );
    harness.presence.leave(false);
    harness.presence.show();
    harness.frame();
    let up = harness.remote.key_up(
        Keycode::Return,
        harness.presence.front(harness.ticks()),
        &mut harness.state,
    );
    assert!(up.is_empty(), "a press across a focus change opened {up:?}");
    // A press made wholly on a drawn Lounge opens the app.
    let front = harness.presence.front(harness.ticks());
    harness.remote.key_down(
        Keycode::Return,
        false,
        true,
        front,
        &mut harness.state,
        &harness.scene.controls,
    );
    let up = harness.remote.key_up(
        Keycode::Return,
        harness.presence.front(harness.ticks()),
        &mut harness.state,
    );
    let Target::App(id) = focus else {
        unreachable!()
    };
    assert_eq!(up, vec![Effect::Launch(id)]);
}

#[test]
fn a_key_stamped_before_the_first_frame_after_home_does_nothing() {
    // Home over an app, and OK goes down before Lounge has drawn a frame of
    // its home again; the loop reads the key only after that frame (it was
    // busy presenting it). The key was pressed on what was on screen before,
    // so it is no press on the home: not a launch, and not a hold into Move.
    crate::clock::record();
    let mut harness = Harness::new(5);
    let focus = harness.state.focus.clone();
    harness.presence.hide();
    harness.presence.raise();
    harness.presence.show();
    crate::clock::step(Duration::from_millis(140));
    let pressed_at = harness.ticks();
    crate::clock::step(Duration::from_millis(500));
    harness.frame();
    let down = harness.remote.key_down(
        Keycode::Return,
        false,
        true,
        harness.presence.front(pressed_at),
        &mut harness.state,
        &harness.scene.controls,
    );
    assert!(down.is_empty());
    assert!(
        harness.remote.press.is_none(),
        "a press from before the frame"
    );
    crate::clock::step(HOLD + Duration::from_millis(100));
    let held = harness
        .remote
        .hold_due(harness.presence.front(harness.ticks()), &mut harness.state);
    assert!(held.is_empty());
    assert!(harness.state.moving.is_none(), "Move started from it");
    let up = harness.remote.key_up(
        Keycode::Return,
        harness.presence.front(harness.ticks()),
        &mut harness.state,
    );
    assert!(up.is_empty(), "opened {up:?}");
    assert_eq!(harness.state.focus, focus);
}

#[test]
fn a_press_held_through_a_return_is_not_a_hold_either() {
    crate::clock::record();
    let mut harness = Harness::new(11);
    harness
        .state
        .open_switcher(vec!["netflix".into(), "youtube.leanback.v4".into()]);
    harness.frame();
    let front = harness.presence.front(harness.ticks());
    harness.remote.key_down(
        Keycode::Return,
        false,
        true,
        front,
        &mut harness.state,
        &harness.scene.controls,
    );
    // Holding OK on a switcher card opens it, but not once Lounge went away.
    harness.presence.raise();
    crate::clock::step(HOLD + Duration::from_millis(50));
    let effects = harness
        .remote
        .hold_due(harness.presence.front(harness.ticks()), &mut harness.state);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(harness.remote.press.is_none());
    assert!(!matches!(
        harness.state.overlay,
        Some(Overlay::Options { .. })
    ));
}
