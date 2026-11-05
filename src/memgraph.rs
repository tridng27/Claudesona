// The memory graph view (SPEC-v2.md, "Memory"): facts as dots coloured by kind, links as lines, the user
// in the middle. A small force-directed layout settles a step per frame. Drag to pan, scroll to zoom,
// click a dot to read it; ✕ forgets it.
use crate::chat::Node;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use std::collections::HashMap;

const SIDE_W: f32 = 170.0; // the details area, when a dot is picked
const REFRESH: f64 = 2.0; // seconds between memory snapshots

#[derive(Default)]
pub struct GraphState {
    nodes: Vec<Node>,
    me: u64,
    pos: HashMap<u64, (Vec2, Vec2)>, // world position and velocity
    pan: Vec2,
    zoom: f32,
    picked: Option<u64>,
    fetched: Option<f64>,
    calm: u32, // frames since the layout last moved noticeably
}

fn colour(kind: &str) -> Color32 {
    match kind {
        "person" => crate::CORAL,
        "project" => Color32::from_rgb(0x5B, 0x8C, 0x85), // sage teal
        "work" => Color32::from_rgb(0x4F, 0x7C, 0xAC),    // dusty blue
        "study" => Color32::from_rgb(0x7F, 0xA6, 0x5A),   // olive
        "like" => Color32::from_rgb(0xE8, 0x7E, 0xA1),    // pink
        "preference" => Color32::from_rgb(0xE0, 0xA4, 0x58), // amber
        "event" => Color32::from_rgb(0x9D, 0x7B, 0xB0),   // lavender
        "session" => Color32::from_rgb(0xC9, 0xB7, 0x9C), // sand
        _ => crate::SOFT_INK,                             // thing, and anything unknown
    }
}

/// A spot near the origin that's the same for an id every time, so new dots don't all start stacked.
fn seed(id: u64) -> Vec2 {
    let a = id as f32 * 2.399_963; // golden angle
    vec2(a.cos(), a.sin()) * (30.0 + (id % 7) as f32 * 12.0)
}

/// One step of the layout. Returns how far the dots moved in total.
fn step(s: &mut GraphState) -> f32 {
    let ids: Vec<u64> = s.nodes.iter().map(|n| n.id).collect();
    let p: Vec<Vec2> = ids.iter().map(|id| s.pos[id].0).collect();
    let mut f = vec![Vec2::ZERO; ids.len()];
    // shortcut: all-pairs repulsion is O(n²), ~125k pairs at 500 facts; use a grid if it ever shows up in a profile.
    for i in 0..p.len() {
        for j in i + 1..p.len() {
            let d = p[i] - p[j];
            let push = d / d.length_sq().max(25.0) * 900.0;
            f[i] += push;
            f[j] -= push;
        }
        f[i] -= p[i] * 0.01; // a gentle pull to the middle keeps lone groups near
    }
    let at: HashMap<u64, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    for (i, n) in s.nodes.iter().enumerate() {
        for (_, to) in &n.links {
            if let Some(&j) = at.get(to) {
                let pull = (p[j] - p[i]) * 0.03;
                f[i] += pull;
                f[j] -= pull;
            }
        }
    }
    let mut moved = 0.0;
    for (i, id) in ids.iter().enumerate() {
        let (pos, vel) = s.pos.get_mut(id).expect("placed");
        if *id == s.me {
            (*pos, *vel) = (Vec2::ZERO, Vec2::ZERO); // the user stays in the middle
            continue;
        }
        *vel = (*vel + f[i]) * 0.6;
        *vel = vel.clamp(Vec2::splat(-20.0), Vec2::splat(20.0));
        *pos += *vel;
        moved += vel.length();
    }
    moved
}

pub fn show(ui: &mut egui::Ui, s: &mut GraphState) {
    let t = ui.input(|i| i.time);
    if s.fetched.is_none_or(|f| t - f > REFRESH)
        && let Some((nodes, me)) = crate::chat::snapshot()
    {
        s.fetched = Some(t);
        if nodes.iter().map(|n| (n.id, &n.text, n.links.len())).ne(s.nodes.iter().map(|n| (n.id, &n.text, n.links.len()))) {
            s.calm = 0;
        }
        (s.nodes, s.me) = (nodes, me);
        // New dots start next to something they link to; gone ones lose their place.
        let placed: Vec<(u64, Vec2)> = s.nodes.iter().filter(|n| !s.pos.contains_key(&n.id)).map(|n| {
            let near = n.links.iter().find_map(|l| s.pos.get(&l.1)).map_or(Vec2::ZERO, |p| p.0);
            (n.id, near + seed(n.id))
        }).collect();
        for (id, p) in placed {
            s.pos.insert(id, (p, Vec2::ZERO));
        }
        s.pos.retain(|id, _| s.nodes.iter().any(|n| n.id == *id));
    }
    if s.zoom == 0.0 {
        s.zoom = 1.0;
    }
    let text = crate::i18n::ui();
    let full = ui.available_rect_before_wrap();
    let painter = ui.painter_at(full);
    painter.rect(full, 10.0, crate::CREAM, Stroke::new(1.0, crate::CORAL.gamma_multiply(0.5)), StrokeKind::Inside);
    if s.nodes.is_empty() {
        painter.text(full.center(), Align2::CENTER_CENTER, text.graph_empty, FontId::proportional(13.0), crate::SOFT_INK);
        ui.allocate_rect(full, Sense::hover());
        return;
    }
    if s.calm < 120 {
        s.calm = if step(s) < 0.05 * s.nodes.len() as f32 { s.calm + 1 } else { 0 };
        ui.ctx().request_repaint();
    }

    let picked = s.picked.and_then(|id| s.nodes.iter().find(|n| n.id == id));
    let area = if picked.is_some() { Rect::from_min_max(full.min, pos2(full.max.x - SIDE_W, full.max.y)) } else { full };
    let resp = ui.interact(area, ui.scope_id().with("memgraph"), Sense::click_and_drag());
    if resp.dragged() {
        s.pan += resp.drag_delta();
    }
    let middle = area.center() + s.pan;
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            // Zoom around the pointer, so what's under it stays put.
            let new = (s.zoom * (scroll * 0.002).exp()).clamp(0.2, 5.0);
            if let Some(m) = resp.hover_pos() {
                s.pan += (m - middle) * (1.0 - new / s.zoom);
            }
            s.zoom = new;
        }
    }
    let middle = area.center() + s.pan;
    let screen = |id: &u64| s.pos.get(id).map(|p| middle + p.0 * s.zoom);
    let painter = ui.painter_at(area);
    for n in &s.nodes {
        for (_, to) in &n.links {
            if let (Some(a), Some(b)) = (screen(&n.id), screen(to)) {
                painter.line_segment([a, b], Stroke::new(1.0, crate::SOFT_INK.gamma_multiply(0.35)));
            }
        }
    }
    let radius = |n: &Node| if n.id == s.me { 9.0 } else { 5.0 } * s.zoom.sqrt();
    let hover = resp.hover_pos();
    let mut under: Option<(f32, &Node, Pos2)> = None;
    for n in &s.nodes {
        let Some(c) = screen(&n.id) else { continue };
        let r = radius(n);
        let on = s.picked == Some(n.id);
        painter.circle(c, r, colour(&n.kind), Stroke::new(if on { 2.0 } else { 0.0 }, crate::INK));
        if let Some(h) = hover
            && h.distance(c) < r + 4.0
            && under.is_none_or(|u| h.distance(c) < u.0)
        {
            under = Some((h.distance(c), n, c));
        }
    }
    if let Some((_, n, c)) = under {
        let label: String = n.text.lines().next().unwrap_or("").chars().take(40).collect();
        painter.text(c + vec2(0.0, -radius(n) - 4.0), Align2::CENTER_BOTTOM, label, FontId::proportional(11.0), crate::INK);
    }
    if resp.clicked() {
        s.picked = under.map(|u| u.1.id);
    }

    // The details of the picked fact: kind, text, links both ways, and ✕ to forget it.
    let Some(n) = s.picked.and_then(|id| s.nodes.iter().find(|n| n.id == id)) else { return };
    let side = Rect::from_min_max(pos2(full.max.x - SIDE_W, full.min.y), full.max).shrink(8.0);
    let painter = ui.painter_at(side.expand(8.0));
    painter.line_segment([pos2(side.min.x - 8.0, side.min.y), pos2(side.min.x - 8.0, side.max.y)], Stroke::new(1.0, crate::CORAL.gamma_multiply(0.4)));
    let close = Rect::from_min_size(pos2(side.max.x - 18.0, side.min.y), vec2(18.0, 18.0));
    let close_resp = ui.interact(close, ui.scope_id().with("memgraph-forget"), Sense::click());
    painter.text(close.center(), Align2::CENTER_CENTER, "✕", FontId::proportional(13.0), if close_resp.hovered() { crate::CORAL } else { crate::SOFT_INK });
    painter.text(side.min, Align2::LEFT_TOP, &n.kind, FontId::proportional(11.0), colour(&n.kind));
    let mut y = side.min.y + 20.0;
    let mut write = |painter: &egui::Painter, s: String, size: f32, ink: Color32| {
        let g = painter.layout(s, FontId::proportional(size), ink, side.width());
        let h = g.size().y;
        painter.galley(pos2(side.min.x, y), g, ink);
        y += h + 6.0;
    };
    write(&painter, n.text.clone(), 12.5, crate::INK);
    let name = |id: u64| s.nodes.iter().find(|x| x.id == id).map_or(String::new(), |x| x.text.chars().take(40).collect());
    let mut links: Vec<String> = n.links.iter().map(|(r, to)| format!("{r} → {}", name(*to))).collect();
    links.extend(s.nodes.iter().flat_map(|x| x.links.iter().filter(|l| l.1 == n.id).map(|(r, _)| format!("{} → {r}", name(x.id)))));
    if !links.is_empty() {
        write(&painter, text.graph_links.to_string(), 11.0, crate::SOFT_INK);
        for l in links {
            write(&painter, l, 11.0, crate::SOFT_INK);
        }
    }
    if close_resp.clicked() {
        let id = n.id;
        std::thread::spawn(move || crate::chat::forget(id)); // saves memory.json: off the UI thread
        s.nodes.retain(|x| x.id != id);
        for x in &mut s.nodes {
            x.links.retain(|l| l.1 != id);
        }
        s.picked = None;
        s.fetched = Some(t); // the next snapshot comes after the forget lands
    }
}
