#![windows_subsystem = "windows"] // no console window behind her
// Claudesona: a desktop mascot that mirrors Claude Code's work. Behaviour is specified in SPEC.md.
// Drag to move, click to poke, right-click to quit.
mod anim;
mod cli;
mod log;
mod rewrite;
mod win;

use anim::Pose;
use eframe::egui::{self, Color32, FontId, Mesh, Painter, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2, ViewportCommand, pos2, vec2};
use log::Event;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

const HEIGHT: f32 = 250.0; // her on-screen height
const MARGIN: f32 = 30.0; // room for sway and hops
const GRID: (usize, usize) = (24, 36);
const WIDTH: f32 = 340.0;
const BUBBLE_ROOM: f32 = 170.0; // space above her head for the speech bubble
const MINI: f32 = 0.42; // mini height relative to hers
const REWRITE_TIMEOUT: f32 = 30.0;
const CREAM: Color32 = Color32::from_rgb(0xFB, 0xF4, 0xEA);
const CORAL: Color32 = Color32::from_rgb(0xD9, 0x77, 0x57);
const INK: Color32 = Color32::from_rgb(0x4A, 0x2C, 0x22);
const SIZES: [(&str, f32); 3] = [("S", 0.75), ("M", 1.0), ("L", 1.35)]; // egui zoom; eframe saves it with egui's memory
const POKE_LINES: [&str; 4] = ["Hey~ I'm busy!", "Mou~ don't poke me!", "Hmph! What is it?", "That tickles... stop it!"];

enum Msg {
    Log(PathBuf, Event),
    Rewritten { short: String, long: Option<String> },
    Hook(serde_json::Value), // from `claudesona hook`, or {"quit":true} from install/uninstall
}

struct Sprite {
    open: TextureHandle,
    blink: Option<TextureHandle>,
    alpha: Vec<u8>, // of `open`, for the click-through hit test
    size: [usize; 2],
}

struct Bubble {
    short: String,
    long: Option<String>,
    expanded: bool,
    hide_at: f32,
    is_label: bool, // activity label rather than speech
}

struct Mini {
    id: String,
    label: String,
    done_at: Option<f32>,
}

/// `<pose>.png` in a `skin` folder next to the exe replaces the built-in art for that pose.
fn custom(name: &str) -> Option<Vec<u8>> {
    std::fs::read(std::env::current_exe().ok()?.parent()?.join("skin").join(format!("{name}.png"))).ok()
}

/// Built-in art lives in the exe, so one file is all a machine needs.
fn asset(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "idle" => include_bytes!("../assets/idle.png"),
        "idle_blink" => include_bytes!("../assets/idle_blink.png"),
        "working" => include_bytes!("../assets/working.png"),
        "working_blink" => include_bytes!("../assets/working_blink.png"),
        "waiting" => include_bytes!("../assets/waiting.png"),
        "poke" => include_bytes!("../assets/poke.png"),
        _ => return None,
    })
}

/// Loads a pose from the skin folder (`skin` true) or the built-in art.
fn load(ctx: &egui::Context, name: &str, skin: bool) -> Option<(TextureHandle, Vec<u8>, [usize; 2])> {
    let bytes = if skin { custom(name)? } else { asset(name)?.to_vec() };
    let mut img = image::load_from_memory(&bytes).ok()?.into_rgba8();
    // Premultiply before shrinking, or the colour hidden under transparent pixels bleeds into her edges.
    for p in img.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = (p[c] as u32 * a / 255) as u8;
        }
    }
    let h = (HEIGHT * 2.0) as u32;
    let w = img.width() * h / img.height();
    let mut img = image::imageops::resize(&img, w, h, image::imageops::FilterType::Lanczos3);
    for p in img.pixels_mut() {
        let a = p[3];
        for c in 0..3 {
            p[c] = p[c].min(a); // Lanczos overshoot would break premultiplied colour
        }
    }
    let size = [w as usize, h as usize];
    let alpha = img.pixels().map(|p| p[3]).collect();
    let ci = egui::ColorImage::from_rgba_premultiplied(size, img.as_raw());
    Some((ctx.load_texture(name, ci, egui::TextureOptions::LINEAR), alpha, size))
}

/// Adds Windows fonts after egui's own, so Vietnamese, kaomoji (Japanese) and symbols render.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, file) in [("segoe", "segoeui.ttf"), ("yugothic", "YuGothM.ttc"), ("symbols", "seguisym.ttf"), ("emoji", "seguiemj.ttf")] {
        if let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{file}")) {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(egui::FontFamily::Proportional).or_default().push(name.into());
        }
    }
    ctx.set_fonts(fonts);
}

/// Draws a pose image bent by the animation, standing on `feet`.
fn draw_sprite(painter: &Painter, tex: &TextureHandle, feet: Pos2, height: f32, t: f32, pose: Pose, poke_t: f32) {
    let mut mesh = Mesh::with_texture(tex.id());
    let (gx, gy) = GRID;
    for j in 0..=gy {
        for i in 0..=gx {
            let (u, v) = (i as f32 / gx as f32, j as f32 / gy as f32);
            let p = anim::warp(u, v, t, pose, poke_t, tex.aspect_ratio());
            mesh.vertices.push(egui::epaint::Vertex { pos: feet + vec2(p.x, p.y) * height, uv: pos2(u, v), color: Color32::WHITE });
        }
    }
    let w = gx as u32 + 1;
    for j in 0..gy as u32 {
        for i in 0..gx as u32 {
            let a = j * w + i;
            mesh.add_triangle(a, a + 1, a + w);
            mesh.add_triangle(a + 1, a + w + 1, a + w);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Rounded cream box with text; returns its rect. `bottom_center` is where its tail points.
fn draw_box(painter: &Painter, text: &str, font: FontId, color: Color32, max_w: f32, bottom_center: Pos2, tail: bool) -> Rect {
    let galley = painter.layout(text.to_string(), font, color, max_w);
    let size = galley.size() + vec2(20.0, 12.0);
    let rect = Rect::from_min_size(bottom_center - vec2(size.x / 2.0, size.y), size);
    if tail {
        let c = rect.center().x;
        let pts = vec![pos2(c - 8.0, rect.bottom() - 1.0), pos2(c + 8.0, rect.bottom() - 1.0), pos2(c + 2.0, rect.bottom() + 10.0)];
        painter.add(egui::Shape::convex_polygon(pts, CREAM, Stroke::new(2.0, CORAL)));
    }
    painter.rect(rect, 10.0, CREAM, Stroke::new(2.0, CORAL), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(10.0, 6.0), galley, Color32::BLACK);
    rect
}

struct App {
    ctx: egui::Context,
    sprites: Vec<Sprite>, // indexed by Pose as usize
    pose: Pose,
    start: Instant,
    poke_at: Option<f32>,
    blink_at: f32,
    rng: u64,
    inbox: Receiver<Msg>,
    outbox: Sender<Msg>,
    session: Option<PathBuf>, // the Claude session she follows: the one you last typed in
    bubble: Option<Bubble>,
    pending_reply: Option<(String, f32)>, // raw reply waiting for its rewrite, and when it arrived
    minis: Vec<Mini>,
    hit_rects: Vec<Rect>, // bubbles and menu drawn last frame, which must stay clickable
    passthrough: bool,
    waiting_since: f32,
    hwnd: isize,
    placed: bool, // checked that she's on screen after eframe restored her position
    terminal: isize, // foreground window when you last sent a prompt; double-click brings it back
    hidden_until: f32,
}

impl App {
    fn new(cc: &eframe::CreationContext, listener: TcpListener) -> Self {
        let ctx = cc.egui_ctx.clone();
        let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(w)) => w.hwnd.get(),
            _ => 0,
        };
        win::remove_border(hwnd);
        install_fonts(&ctx);
        // A skin folder with a readable idle.png replaces all built-in art, so characters never mix.
        let custom_idle = load(&ctx, "idle", true);
        let skin = custom_idle.is_some();
        let (idle, idle_alpha, idle_size) = custom_idle.or_else(|| load(&ctx, "idle", false)).expect("assets/idle.png is required");
        let sprite = |name: &str, blink: &str| {
            // shortcut: a missing pose falls back to idle instead of a drawn placeholder
            let (open, alpha, size) = load(&ctx, name, skin).unwrap_or_else(|| (idle.clone(), idle_alpha.clone(), idle_size));
            Sprite { open, blink: load(&ctx, blink, skin).map(|l| l.0), alpha, size }
        };
        let sprites = vec![sprite("idle", "idle_blink"), sprite("working", "working_blink"), sprite("waiting", ""), sprite("poke", "")];

        let (outbox, inbox) = channel();
        let tx = outbox.clone();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            log::watch(|path, ev| {
                let _ = tx.send(Msg::Log(path.clone(), ev));
                wake.request_repaint();
            })
        });
        let (tx, wake) = (outbox.clone(), ctx.clone());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let mut text = String::new();
                let _ = stream.read_to_string(&mut text);
                if let Ok(v) = serde_json::from_str(&text) {
                    let _ = tx.send(Msg::Hook(v));
                    wake.request_repaint();
                }
            }
        });
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        Self {
            ctx,
            sprites,
            pose: Pose::Idle,
            start: Instant::now(),
            poke_at: None,
            blink_at: 2.0,
            rng: seed | 1,
            inbox,
            outbox,
            session: None,
            bubble: None,
            pending_reply: None,
            minis: vec![],
            hit_rects: vec![],
            passthrough: false,
            waiting_since: 0.0,
            hwnd,
            placed: false,
            terminal: 0,
            hidden_until: 0.0,
        }
    }

    /// Resize her (egui zoom scales the whole window), keeping her feet where they were.
    fn set_zoom(&self, z: f32) {
        let old = self.ctx.zoom_factor();
        self.ctx.set_zoom_factor(z);
        // The new zoom only lands next frame, so these commands are still in old-zoom points.
        let size = window_size() * z / old;
        self.ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
        if let Some(outer) = self.ctx.input(|i| i.viewport().outer_rect) {
            let feet = outer.min + vec2(outer.width() / 2.0, outer.height());
            let mut pos = feet - size * vec2(0.5, 1.0);
            if let Some(work) = win::work_area(self.hwnd).map(|w| w / old) {
                pos = pos.min(work.max - size).max(work.min); // a bigger her mustn't spill off screen
            }
            self.ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos));
        }
    }

    fn rand01(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 10_000) as f32 / 10_000.0
    }

    fn say(&mut self, short: String, long: Option<String>, t: f32) {
        let hide_at = t + 8.0 + short.chars().count() as f32 / 12.0; // roughly reading time
        self.bubble = Some(Bubble { short, long, expanded: false, hide_at, is_label: false });
    }

    fn label(&mut self, text: String, t: f32) {
        self.bubble = Some(Bubble { short: text, long: None, expanded: false, hide_at: t + 120.0, is_label: true });
    }

    fn handle(&mut self, msg: Msg, t: f32) {
        let (path, ev) = match msg {
            Msg::Rewritten { short, long } => {
                // A rewrite that arrives after the timeout already showed the plain reply: drop it.
                if self.pending_reply.take().is_some() {
                    self.pose = Pose::Idle;
                    self.say(short, long, t);
                }
                return;
            }
            Msg::Hook(v) => {
                if v["quit"] == true {
                    self.ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                // PermissionRequest fires the moment the dialog shows. The permission_prompt Notification only
                // comes after ~6s without typing; it still covers sandbox network prompts, which have no PermissionRequest.
                let notified = v["notification_type"] == "permission_prompt" || v["message"].as_str().is_some_and(|m| m.contains("permission"));
                if v["hook_event_name"] == "PermissionRequest" || (v["hook_event_name"] == "Notification" && notified) {
                    self.pose = Pose::Waiting;
                    self.waiting_since = t;
                    self.say("I need your OK~!".into(), None, t + 600.0); // stays until Claude moves on
                }
                return;
            }
            Msg::Log(path, ev) => (path, ev),
        };
        if ev == Event::Prompt {
            self.session = Some(path.clone());
            let fg = win::foreground();
            if fg != 0 && fg != self.hwnd {
                self.terminal = fg;
            }
        }
        // Follow one session: events from other Claude windows are ignored.
        if *self.session.get_or_insert_with(|| path.clone()) != path {
            return;
        }
        // New log activity means the permission prompt was answered, except in the first 2s:
        // Claude Code writes its log a little late, so lines from just before the prompt still trickle in.
        let still_waiting = self.pose == Pose::Waiting && t - self.waiting_since < 2.0;
        if self.pose == Pose::Waiting && !still_waiting {
            self.pose = Pose::Working;
            self.bubble = None;
        }
        self.apply(ev, t);
        if still_waiting {
            self.pose = Pose::Waiting;
            self.say("I need your OK~!".into(), None, t + 600.0);
        }
    }

    fn apply(&mut self, ev: Event, t: f32) {
        match ev {
            Event::Prompt => {
                self.pose = Pose::Working;
                self.bubble = None;
                self.pending_reply = None;
            }
            Event::Tool(text) | Event::Note(text) => {
                self.pose = Pose::Working;
                self.label(text, t);
            }
            Event::AgentStart { id, label } => self.minis.push(Mini { id, label, done_at: None }),
            Event::AgentDone(id) => {
                if let Some(m) = self.minis.iter_mut().find(|m| m.id == id) {
                    m.done_at.get_or_insert(t);
                }
            }
            Event::Reply(text) => {
                self.pending_reply = Some((text.clone(), t));
                let (tx, wake) = (self.outbox.clone(), self.ctx.clone());
                std::thread::spawn(move || {
                    let (short, long) = rewrite::rewrite(&text).unwrap_or_else(|| (log::clean(&text, 160), None));
                    let _ = tx.send(Msg::Rewritten { short, long });
                    wake.request_repaint();
                });
            }
            Event::TurnEnd => {
                if self.pending_reply.is_none() {
                    self.pose = Pose::Idle;
                    if self.bubble.as_ref().is_some_and(|b| b.is_label) {
                        self.bubble = None;
                    }
                }
            }
        }
    }

    /// Is the window-local point over something clickable: her visible pixels or a bubble?
    fn hit(&self, p: Pos2, body: Rect, pose: Pose) -> bool {
        if self.hit_rects.iter().any(|r| r.contains(p)) {
            return true;
        }
        if !body.contains(p) {
            return false;
        }
        let s = &self.sprites[pose as usize];
        let uv = (p - body.min) / body.size();
        let (x, y) = ((uv.x * s.size[0] as f32) as usize, (uv.y * s.size[1] as f32) as usize);
        s.alpha.get(y.min(s.size[1] - 1) * s.size[0] + x.min(s.size[0] - 1)).is_some_and(|a| *a > 40)
    }
}

impl eframe::App for App {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = self.ctx.clone();
        let t = self.start.elapsed().as_secs_f32();
        while let Ok(msg) = self.inbox.try_recv() {
            self.handle(msg, t);
        }
        if let Some((text, since)) = &self.pending_reply
            && t - since > REWRITE_TIMEOUT
        {
            let plain = log::clean(text, 160);
            self.pending_reply = None;
            self.pose = Pose::Idle;
            self.say(plain, None, t);
        }
        if self.bubble.as_ref().is_some_and(|b| t > b.hide_at) {
            self.bubble = None;
        }
        self.minis.retain(|m| m.done_at.is_none_or(|d| t - d < 2.0));
        // Hidden: keep following Claude, but draw nothing and let every click through.
        if t < self.hidden_until {
            if !self.passthrough {
                self.passthrough = true;
                ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(true));
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(self.hidden_until - t));
            return;
        }

        let poke_t = self.poke_at.map(|p| t - p).filter(|d| *d < 1.5);
        let pose = if poke_t.is_some() { Pose::Poke } else { self.pose };
        if t > self.blink_at + 0.13 {
            self.blink_at = t + 3.0 + 3.0 * self.rand01();
        }
        let blinking = t > self.blink_at;

        // eframe restores the last position, which can be off screen (monitor unplugged, resolution changed).
        if !self.placed && t > 0.3 {
            self.placed = true;
            // Match the window to the restored zoom; the saved window size can be from another zoom.
            let size = window_size();
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            if let (Some(outer), Some(work)) = (ctx.input(|i| i.viewport().outer_rect), win::work_area(self.hwnd).map(|w| w / ctx.zoom_factor())) {
                let x = outer.min.x.min(work.right() - size.x).max(work.left());
                let y = outer.min.y.min(work.bottom() - size.y).max(work.top());
                if (x, y) != (outer.min.x, outer.min.y) {
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
                }
            }
        }

        let area = ui.max_rect();
        let feet = pos2(area.center().x, area.bottom() - MARGIN / 2.0);
        let aspect = self.sprites[0].open.aspect_ratio(); // poses share idle's canvas
        let body = Rect::from_min_max(pos2(feet.x - HEIGHT * aspect / 2.0, feet.y - HEIGHT), pos2(feet.x + HEIGHT * aspect / 2.0, feet.y));

        // Clicks pass through everything except her and her bubbles. The cursor is read from Windows
        // because a pass-through window gets no mouse events to tell us when to switch back.
        if let (Some((cx, cy)), Some(inner)) = (win::cursor(), ctx.input(|i| i.viewport().inner_rect)) {
            let ppp = ctx.pixels_per_point();
            let local = pos2(cx / ppp, cy / ppp) - inner.min.to_vec2();
            let dragging = ctx.input(|i| i.pointer.any_down()) && !self.passthrough;
            let pass = !dragging && !self.hit(local, body, pose);
            if pass != self.passthrough {
                self.passthrough = pass;
                ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(pass));
            }
        }

        let resp = ui.interact(body, ui.scope_id().with("her"), Sense::click_and_drag());
        if resp.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if resp.clicked() {
            self.poke_at = Some(t);
            if self.bubble.as_ref().is_none_or(|b| b.is_label) {
                let line = POKE_LINES[(self.rand01() * POKE_LINES.len() as f32) as usize % POKE_LINES.len()];
                self.say(line.into(), None, t - 6.0);
            }
        }
        if resp.double_clicked() && self.terminal != 0 {
            win::focus(self.terminal);
        }
        self.hit_rects.clear();
        let menu = resp.context_menu(|ui| {
            ui.horizontal(|ui| {
                ui.label("Size");
                for (name, z) in SIZES {
                    if ui.selectable_label(ctx.zoom_factor() == z, name).clicked() {
                        self.set_zoom(z);
                        ui.close();
                    }
                }
            });
            if ui.button("Hide for 10 min").clicked() {
                self.hidden_until = t + 600.0;
                ui.close();
            }
            if ui.button("Quit").clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        });
        self.hit_rects.extend(menu.map(|m| m.response.rect));

        let painter = ui.painter();
        let sprite = &self.sprites[pose as usize];
        let tex = if blinking { sprite.blink.as_ref().unwrap_or(&sprite.open) } else { &sprite.open };
        draw_sprite(painter, tex, feet, HEIGHT, t, pose, poke_t.unwrap_or(9.0));
        if pose == Pose::Waiting {
            let bob = (t * 6.0).sin().abs() * 6.0;
            let at = pos2(body.right() - 38.0, body.top() + 30.0 - bob);
            painter.text(at, egui::Align2::CENTER_CENTER, "!", FontId::proportional(34.0), CORAL);
        }

        // Minis line up beside her feet, alternating left and right.
        let mini_w = HEIGHT * MINI * aspect;
        for (i, m) in self.minis.iter().enumerate() {
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            let dist = HEIGHT * aspect / 2.0 + mini_w / 2.0 - 12.0 + (i / 2) as f32 * mini_w * 0.6;
            let mfeet = feet + vec2(side * dist, 0.0);
            let (mpose, text) = if m.done_at.is_some() { (Pose::Waiting, "done!") } else { (Pose::Working, m.label.as_str()) };
            let mt = t + i as f32 * 0.7; // out of step with each other
            draw_sprite(painter, &self.sprites[mpose as usize].open, mfeet, HEIGHT * MINI, mt, mpose, 9.0);
            draw_box(painter, text, FontId::proportional(10.0), INK, 90.0, mfeet - vec2(0.0, HEIGHT * MINI + 2.0), false);
        }

        let mut toggle = false;
        if let Some(b) = &self.bubble {
            let (text, font, color) = match (&b.long, b.expanded, b.is_label) {
                (_, _, true) => (b.short.clone(), FontId::proportional(12.5), Color32::from_rgb(0x8A, 0x5A, 0x4A)),
                (Some(long), true, _) => (long.clone(), FontId::proportional(14.0), INK),
                (Some(_), false, _) => (format!("{}\n(click for more)", b.short), FontId::proportional(14.0), INK),
                (None, _, _) => (b.short.clone(), FontId::proportional(14.0), INK),
            };
            let anchor = pos2(area.center().x, (feet.y - HEIGHT + 4.0).max(area.top() + 2.0));
            // shortcut: text taller than BUBBLE_ROOM clips at the top of the window
            let rect = draw_box(painter, &text, font, color, WIDTH - 40.0, anchor, true);
            self.hit_rects.push(rect);
            toggle = !b.is_label && b.long.is_some() && ui.interact(rect, ui.scope_id().with("bubble"), Sense::click()).clicked();
        }
        if toggle && let Some(b) = &mut self.bubble {
            b.expanded = !b.expanded;
            b.hide_at = t + 10.0 + b.long.as_deref().unwrap_or("").chars().count() as f32 / 12.0;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Window size in points; egui zoom turns it into her on-screen size.
fn window_size() -> Vec2 {
    vec2(WIDTH, HEIGHT + 2.0 * MARGIN + BUBBLE_ROOM)
}

fn main() -> eframe::Result {
    match std::env::args().nth(1).as_deref() {
        Some("hook") => return Ok(cli::hook()),
        Some("install") => return Ok(cli::report(cli::install())),
        Some("uninstall") => return Ok(cli::report(cli::uninstall())),
        _ => {}
    }
    // Port taken: she's already running.
    let Ok(listener) = TcpListener::bind(cli::ADDR) else { return Ok(()) };
    let size = window_size();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(size)
        .with_transparent(true)
        .with_decorations(false)
        .with_has_shadow(false)
        .with_always_on_top()
        .with_active(false) // never take keyboard focus from your terminal
        .with_resizable(false)
        .with_taskbar(false)
        .with_title("Claudesona");
    // First run: bottom-right above the taskbar. Later runs: eframe restores where you left her.
    if let Some((right, bottom)) = win::work_area_corner() {
        viewport = viewport.with_position(pos2(right - size.x - 10.0, bottom - size.y));
    }
    let opts = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow, // wgpu on Windows gives an opaque window
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native("Claudesona", opts, Box::new(|cc| Ok(Box::new(App::new(cc, listener)))))
}
