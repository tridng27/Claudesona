// Bends her image on a grid so parts move separately without cutting layers.
// Coordinates: (u, v) in 0..1 over the image, v pointing down. Output is relative to her feet,
// in units of her height (y negative = up). Region bounds assume all poses share one framing.
use eframe::egui::{Vec2, vec2};
use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pose {
    Idle = 0,
    Working = 1,
    Waiting = 2,
    Poke = 3,
    Music = 4,
    Excited = 5, // "Play with me": game in progress
    Smug = 6,    // she won (you lost)
    Pouty = 7,   // you won
    Cheer = 8,   // you're typing a prompt: she cheers you on
    Held = 9,    // you're dragging her by her sweater collar
    Dizzy = 10,  // held, and shaken around too much
    Coffee = 11, // you gave her coffee from the menu: she sips it, whatever else is going on
    Done = 12,     // work confirmed finished: a brief smug smirk
    Oops = 13,     // a tool call failed: she sobs a little
    Dazed = 14,    // Claude compacted its context: spiral eyes, standing
    Reading = 15,  // working: Read, Grep, Glob (magnifying glass)
    Editing = 16,  // working: Edit, Write (hammer)
    Testing = 17,  // working: running tests (bug net)
    Shipping = 18, // working: git commit / push (books on her head)
    Web = 19,      // working: WebSearch, WebFetch (hugging "www")
    Sleep = 20,    // 10 min with nothing going on: asleep, sitting
}

/// Where you hold her while dragging: the top of her bunched-up sweater in held.png, as (u, v) on her canvas.
pub const COLLAR: (f32, f32) = (0.566, 0.03);

struct Style {
    speed: f32,  // cycles per 3s
    sway: f32,   // whole-body bend, degrees at the top of her head
    head: f32,   // head tilt, degrees
    hair: f32,   // hair-tip swing, height units
    breath: f32, // stretch of the upper body
    hop: f32,    // bounce height
    hand: f32,   // raised-hand wave, degrees (music pose only)
}

fn style(pose: Pose) -> Style {
    match pose {
        Pose::Idle => Style { speed: 1.0, sway: 2.0, head: 3.0, hair: 0.012, breath: 0.010, hop: 0.0, hand: 0.0 },
        Pose::Working => Style { speed: 2.2, sway: 0.6, head: 1.5, hair: 0.008, breath: 0.006, hop: 0.010, hand: 0.0 },
        Pose::Waiting => Style { speed: 1.5, sway: 2.5, head: 4.0, hair: 0.015, breath: 0.012, hop: 0.018, hand: 0.0 },
        Pose::Music => Style { speed: 0.8, sway: 2.5, head: 3.5, hair: 0.014, breath: 0.010, hop: 0.0, hand: 7.0 },
        Pose::Excited => Style { speed: 1.8, sway: 2.0, head: 4.0, hair: 0.015, breath: 0.012, hop: 0.016, hand: 0.0 },
        Pose::Smug => Style { speed: 0.9, sway: 2.5, head: 5.0, hair: 0.012, breath: 0.010, hop: 0.0, hand: 0.0 },
        Pose::Pouty => Style { speed: 0.7, sway: 1.0, head: 2.0, hair: 0.008, breath: 0.006, hop: 0.0, hand: 0.0 },
        Pose::Cheer => Style { speed: 1.9, sway: 1.5, head: 3.5, hair: 0.015, breath: 0.012, hop: 0.022, hand: 0.0 },
        Pose::Held => Style { speed: 1.0, sway: 2.5, head: 0.0, hair: 0.005, breath: 0.0, hop: 0.0, hand: 0.0 },
        Pose::Coffee => Style { speed: 0.7, sway: 1.5, head: 2.0, hair: 0.010, breath: 0.012, hop: 0.0, hand: 0.0 },
        Pose::Dizzy => Style { speed: 0.8, sway: 3.5, head: 0.0, hair: 0.005, breath: 0.0, hop: 0.0, hand: 0.0 },
        Pose::Poke => Style { speed: 1.0, sway: 0.0, head: 0.0, hair: 0.010, breath: 0.0, hop: 0.0, hand: 0.0 },
        Pose::Done => Style { speed: 1.2, sway: 2.0, head: 4.0, hair: 0.014, breath: 0.010, hop: 0.0, hand: 0.0 },
        Pose::Oops => Style { speed: 0.7, sway: 1.0, head: 1.5, hair: 0.008, breath: 0.014, hop: 0.0, hand: 0.0 },
        Pose::Dazed => Style { speed: 0.8, sway: 3.5, head: 4.0, hair: 0.012, breath: 0.006, hop: 0.0, hand: 0.0 },
        Pose::Reading => Style { speed: 1.2, sway: 1.0, head: 1.0, hair: 0.010, breath: 0.008, hop: 0.0, hand: 0.0 },
        Pose::Editing => Style { speed: 2.2, sway: 0.6, head: 1.5, hair: 0.010, breath: 0.006, hop: 0.010, hand: 0.0 },
        Pose::Testing => Style { speed: 1.6, sway: 1.5, head: 2.5, hair: 0.012, breath: 0.010, hop: 0.008, hand: 0.0 },
        Pose::Shipping => Style { speed: 1.0, sway: 1.5, head: 1.0, hair: 0.008, breath: 0.004, hop: 0.0, hand: 0.0 },
        Pose::Web => Style { speed: 1.0, sway: 2.0, head: 2.5, hair: 0.012, breath: 0.010, hop: 0.0, hand: 0.0 },
        // Sitting: the hair and head regions assume a standing figure, so only her breathing moves.
        Pose::Sleep => Style { speed: 0.5, sway: 0.0, head: 0.0, hair: 0.0, breath: 0.012, hop: 0.0, hand: 0.0 },
    }
}

/// 0 at `e0`, 1 at `e1` (either order), smooth in between.
fn ramp(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rotate(p: Vec2, pivot: Vec2, deg: f32) -> Vec2 {
    let (s, c) = deg.to_radians().sin_cos();
    let d = p - pivot;
    pivot + vec2(d.x * c - d.y * s, d.x * s + d.y * c)
}

/// Where image point (u, v) is drawn at time `t`. `poke_t` = seconds since the last poke.
/// `swing` = (angle in degrees, angular speed in rad/s) of her pendulum while held.
pub fn warp(u: f32, v: f32, t: f32, pose: Pose, poke_t: f32, aspect: f32, swing: Vec2) -> Vec2 {
    let st = style(pose);
    let s = t * st.speed * TAU / 3.0;
    let mut p = vec2((u - 0.5) * aspect, v - 1.0);

    // Breathing: upper body stretches up a little; feet stay put.
    p.y *= 1.0 + st.breath * (0.5 + 0.5 * (2.0 * s).sin());

    // Hair tips (outer sides, chin to shoulder height) trail behind the head.
    let side = (u - 0.5).abs();
    let held = matches!(pose, Pose::Held | Pose::Dizzy);
    let hair = if held {
        // held.png's face sits off-centre: only the hair beside it, above her sleeves.
        (ramp(0.27, 0.20, u) + ramp(0.58, 0.64, u) * ramp(0.72, 0.68, u)) * ramp(0.14, 0.22, v) * ramp(0.41, 0.34, v)
    } else {
        ramp(0.12, 0.30, side) * ramp(0.05, 0.18, v) * ramp(0.50, 0.36, v)
    };
    p.x += (st.hair * (s - 0.9).sin() - 0.003 * swing.y.clamp(-3.0, 3.0)) * hair; // hair trails her swing

    // Skirt hem swings a touch behind the body.
    let skirt = if held { 0.0 } else { ramp(0.52, 0.62, v) * ramp(0.72, 0.64, v) }; // held: her hands are at hem height
    p.x += st.hair * 0.5 * (s - 0.5).sin() * skirt;

    // Music pose: her raised hand (viewer's left, wrist to fingertips) waves around the elbow, twice per sway.
    let hand = ramp(0.31, 0.20, u) * ramp(0.30, 0.33, v) * ramp(0.50, 0.46, v);
    p = rotate(p, vec2((0.30 - 0.5) * aspect, 0.43 - 1.0), st.hand * (2.0 * s).sin() * hand);

    // Head tilts around the neck.
    let neck = vec2(0.0, 0.38 - 1.0);
    let head = ramp(0.44, 0.34, v);
    p = rotate(p, neck, st.head * (s + 0.4).sin() * head);

    if held {
        // Held up by the sweater: she swings from it (the physics is in main.rs). Her straight leg (viewer's left) dangles; the bent one kicks from the knee.
        let collar = vec2((COLLAR.0 - 0.5) * aspect, COLLAR.1 - 1.0);
        // Dizzy: her legs go floppy instead of kicking: slower, looser, wobbling unevenly and trailing her swing.
        let dizzy = pose == Pose::Dizzy;
        let (amp, rate) = if dizzy { (1.5, 1.0) } else { (1.0, 2.0) };
        let wobble = |phase: f32| (rate * s + phase).sin() + if dizzy { 0.35 * (1.7 * s + phase).sin() } else { 0.0 };
        let trail = if dizzy { -0.008 * swing.y.clamp(-3.0, 3.0) } else { 0.0 };
        p.x += (amp * 0.025 * wobble(0.0) + trail) * ramp(0.505, 0.48, u) * ramp(0.66, 0.95, v); // bent knee starts at u 0.505
        let hand = ramp(0.71, 0.74, u) * ramp(0.59, 0.575, v); // her hand, right beside the shoe
        let shin = ramp(0.56, 0.62, u) * ramp(0.555, 0.57, v) * ramp(0.74, 0.70, v) * (1.0 - hand);
        p = rotate(p, vec2((0.532 - 0.5) * aspect, 0.684 - 1.0), (amp * 6.0 * wobble(1.5) - trail.to_degrees() * 4.0) * shin);
        p = rotate(p, collar, swing.x + st.sway * (s - 0.3).sin()); // rigid, so her face doesn't bend; a gentle sway even when still
    } else {
        // Whole body bends from the feet: more angle higher up gives a soft curve.
        p = rotate(p, Vec2::ZERO, st.sway * s.sin() * -p.y);
    }

    // Hop and poke shake move everything.
    p.y -= st.hop * (2.0 * s).sin().abs();
    p.x += 0.02 * (poke_t * 40.0).sin() * (-poke_t * 6.0).exp();
    p
}

/// Scale around her feet as she enters a lively pose: a quick crouch, then a small stretch that settles.
/// `since` = seconds in this pose.
pub fn squash(pose: Pose, since: f32) -> Vec2 {
    if !matches!(pose, Pose::Cheer | Pose::Waiting | Pose::Excited | Pose::Done) {
        return vec2(1.0, 1.0);
    }
    let d = -0.12 * (since * 18.0).sin() * (-since * 5.0).exp(); // negative first: crouch
    vec2(1.0 - 0.5 * d, 1.0 + d)
}

/// One step of her pendulum while held. `swing` = (angle in degrees, angular speed in rad/s);
/// `dv` = change in the hold point's sideways speed since the last step, in her heights per second.
pub fn swing_step(swing: Vec2, dv: f32, dt: f32) -> Vec2 {
    const LENGTH: f32 = 0.45; // hold point to her middle, in heights
    const GRAVITY: f32 = 12.0; // g / length: about one swing per 1.8s
    const DAMPING: f32 = 1.2; // settles in about two seconds
    const MAX: f32 = 75.0; // degrees: her window's swing room (SWING_ROOM in main.rs)
    let angle = swing.x.to_radians();
    // The hold point speeding up leaves her body behind (positive angle swings her feet left).
    let mut w = swing.y + 0.4 * dv / LENGTH * angle.cos(); // 0.4: she's light, the mouse only nudges her
    w += (-GRAVITY * angle.sin() - DAMPING * w) * dt;
    let next = (swing.x + w.to_degrees() * dt).clamp(-MAX, MAX);
    if next.abs() == MAX {
        w = 0.0; // only a hard fling gets here: stop dead and let gravity pull her back, no bounce
    }
    vec2(next, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squash_crouches_then_settles() {
        assert_eq!(squash(Pose::Cheer, 0.0), vec2(1.0, 1.0));
        assert!(squash(Pose::Cheer, 0.08).y < 0.95);
        assert!(squash(Pose::Cheer, 0.25).y > 1.0);
        assert!((squash(Pose::Cheer, 1.0).y - 1.0).abs() < 0.005);
        assert_eq!(squash(Pose::Idle, 0.08), vec2(1.0, 1.0));
    }

    #[test]
    fn feet_stay_planted_when_idle() {
        for t in [0.0, 0.7, 1.9, 5.3] {
            for u in [0.3, 0.5, 0.7] {
                let p = warp(u, 1.0, t, Pose::Idle, 9.0, 0.667, Vec2::ZERO);
                assert!((p - vec2((u - 0.5) * 0.667, 0.0)).length() < 1e-4, "t={t} u={u} p={p:?}");
            }
        }
    }

    #[test]
    fn music_hand_waves_but_body_stays() {
        let moved = |u: f32, v: f32| (0..30).map(|i| (warp(u, v, i as f32 * 0.1, Pose::Music, 9.0, 0.667, Vec2::ZERO) - warp(u, v, 0.0, Pose::Music, 9.0, 0.667, Vec2::ZERO)).length()).fold(0.0, f32::max);
        assert!(moved(0.12, 0.36) > 2.0 * moved(0.5, 0.45));
    }

    #[test]
    fn held_collar_stays_under_the_cursor() {
        let at = |t: f32, v: f32| warp(COLLAR.0, v, t, Pose::Held, 9.0, 0.667, Vec2::ZERO);
        for t in [0.0, 0.7, 1.9] {
            assert!((at(t, COLLAR.1) - at(0.0, COLLAR.1)).length() < 0.01, "collar moved at t={t}");
        }
        let swing = (0..30).map(|i| (warp(0.4, 0.95, i as f32 * 0.1, Pose::Held, 9.0, 0.667, Vec2::ZERO) - warp(0.4, 0.95, 0.0, Pose::Held, 9.0, 0.667, Vec2::ZERO)).length()).fold(0.0, f32::max);
        assert!(swing > 0.02, "legs should swing: {swing}");
    }

    #[test]
    fn held_face_stays_rigid() {
        // Eyes and mouth in held.png keep their distances while she swings.
        let pts = [(0.33, 0.33), (0.50, 0.30), (0.42, 0.36)];
        let d = |t: f32, a: usize, b: usize| (warp(pts[a].0, pts[a].1, t, Pose::Held, 9.0, 0.667, vec2(t * 10.0, 0.0)) - warp(pts[b].0, pts[b].1, t, Pose::Held, 9.0, 0.667, vec2(t * 10.0, 0.0))).length();
        for t in [0.0, 0.7, 1.9] {
            assert!((d(t, 0, 1) - d(0.0, 0, 1)).abs() < 1e-4 && (d(t, 0, 2) - d(0.0, 0, 2)).abs() < 1e-4, "face bent at t={t}");
        }
    }

    #[test]
    fn swing_lags_then_settles() {
        // Yank the hold point to the right: her feet swing left (positive angle), then she comes to rest.
        let mut sw = swing_step(Vec2::ZERO, 2.0, 1.0 / 60.0);
        for _ in 0..5 {
            sw = swing_step(sw, 0.0, 1.0 / 60.0);
        }
        assert!(sw.x > 1.0, "{sw:?}");
        for _ in 0..900 {
            sw = swing_step(sw, 0.0, 1.0 / 60.0);
        }
        assert!(sw.x.abs() < 0.1 && sw.y.abs() < 0.1, "{sw:?}");
        // A hard fling swings her far, and she never comes back faster than she went (no bounce off a limit).
        let mut sw = swing_step(Vec2::ZERO, 30.0, 1.0 / 60.0);
        let mut peak = 0.0f32;
        for _ in 0..120 {
            sw = swing_step(sw, 0.0, 1.0 / 60.0);
            peak = peak.max(sw.x);
        }
        assert!(peak > 30.0 && sw.x < peak, "{peak} {sw:?}");
    }

    #[test]
    fn head_moves_more_than_legs() {
        let moved = |v: f32| (0..30).map(|i| (warp(0.5, v, i as f32 * 0.1, Pose::Idle, 9.0, 0.667, Vec2::ZERO) - warp(0.5, v, 0.0, Pose::Idle, 9.0, 0.667, Vec2::ZERO)).length()).fold(0.0, f32::max);
        assert!(moved(0.2) > 3.0 * moved(0.9));
    }
}
