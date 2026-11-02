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
}

struct Style {
    speed: f32,  // cycles per 3s
    sway: f32,   // whole-body bend, degrees at the top of her head
    head: f32,   // head tilt, degrees
    hair: f32,   // hair-tip swing, height units
    breath: f32, // stretch of the upper body
    hop: f32,    // bounce height
}

fn style(pose: Pose) -> Style {
    match pose {
        Pose::Idle => Style { speed: 1.0, sway: 2.0, head: 3.0, hair: 0.012, breath: 0.010, hop: 0.0 },
        Pose::Working => Style { speed: 2.2, sway: 0.6, head: 1.5, hair: 0.008, breath: 0.006, hop: 0.010 },
        Pose::Waiting => Style { speed: 1.5, sway: 2.5, head: 4.0, hair: 0.015, breath: 0.012, hop: 0.018 },
        Pose::Poke => Style { speed: 1.0, sway: 0.0, head: 0.0, hair: 0.010, breath: 0.0, hop: 0.0 },
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
pub fn warp(u: f32, v: f32, t: f32, pose: Pose, poke_t: f32, aspect: f32) -> Vec2 {
    let st = style(pose);
    let s = t * st.speed * TAU / 3.0;
    let mut p = vec2((u - 0.5) * aspect, v - 1.0);

    // Breathing: upper body stretches up a little; feet stay put.
    p.y *= 1.0 + st.breath * (0.5 + 0.5 * (2.0 * s).sin());

    // Hair tips (outer sides, chin to shoulder height) trail behind the head.
    let side = (u - 0.5).abs();
    let hair = ramp(0.12, 0.30, side) * ramp(0.05, 0.18, v) * ramp(0.50, 0.36, v);
    p.x += st.hair * (s - 0.9).sin() * hair;

    // Skirt hem swings a touch behind the body.
    let skirt = ramp(0.52, 0.62, v) * ramp(0.72, 0.64, v);
    p.x += st.hair * 0.5 * (s - 0.5).sin() * skirt;

    // Head tilts around the neck.
    let neck = vec2(0.0, 0.38 - 1.0);
    let head = ramp(0.44, 0.34, v);
    p = rotate(p, neck, st.head * (s + 0.4).sin() * head);

    // Whole body bends from the feet: more angle higher up gives a soft curve.
    p = rotate(p, Vec2::ZERO, st.sway * s.sin() * -p.y);

    // Hop and poke shake move everything.
    p.y -= st.hop * (2.0 * s).sin().abs();
    p.x += 0.02 * (poke_t * 40.0).sin() * (-poke_t * 6.0).exp();
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feet_stay_planted_when_idle() {
        for t in [0.0, 0.7, 1.9, 5.3] {
            for u in [0.3, 0.5, 0.7] {
                let p = warp(u, 1.0, t, Pose::Idle, 9.0, 0.667);
                assert!((p - vec2((u - 0.5) * 0.667, 0.0)).length() < 1e-4, "t={t} u={u} p={p:?}");
            }
        }
    }

    #[test]
    fn head_moves_more_than_legs() {
        let moved = |v: f32| (0..30).map(|i| (warp(0.5, v, i as f32 * 0.1, Pose::Idle, 9.0, 0.667) - warp(0.5, v, 0.0, Pose::Idle, 9.0, 0.667)).length()).fold(0.0, f32::max);
        assert!(moved(0.2) > 3.0 * moved(0.9));
    }
}
