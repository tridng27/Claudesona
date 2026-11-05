#![cfg_attr(windows, windows_subsystem = "windows")] // no console window behind her on Windows
// Claudesona: a desktop mascot that mirrors Claude Code's work. Behaviour is specified in SPEC.md.
// Drag to move, click to poke, right-click for the menu and chat.
mod anim;
mod chat;
mod cli;
mod game;
mod sudoku;
mod i18n;
mod log;
mod music;
mod rewrite;
mod voice;
#[cfg_attr(windows, path = "os_windows.rs")]
#[cfg_attr(not(windows), path = "os_unix.rs")]
mod os; // the few OS calls eframe doesn't cover

use anim::Pose;
use eframe::egui::{self, Color32, FontId, Key, Mesh, Painter, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Vec2, ViewportCommand, ViewportId, pos2, vec2};
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
const BUBBLE_ROOM: f32 = 240.0; // space above her head for the speech bubble (and the music player)
const MINI: f32 = 0.42; // mini height relative to hers
const POPUP_W: f32 = 270.0; // right-click popup width
const POPUP_SLIDE: f32 = 28.0; // points the popup slides out from her
const POPUP_IN: f64 = 0.18; // seconds the popup takes to slide and fade in
const FILLER_EVERY: f64 = 3.0; // seconds each general working line shows
const FINISHED_FLASH: f64 = 2.5; // seconds of Waiting pose when work is confirmed finished
const POSE_FADE: f64 = 0.12; // seconds to crossfade between poses
const TYPING_SHOWS: f64 = 1.5; // seconds she keeps cheering after your last keystroke
const CHEER_ON_SEND: f64 = 2.5; // seconds she cheers when your prompt reaches the log
const CHEER_LINE_EVERY: f64 = 20.0; // at most one cheer line this often
const SWING_ROOM: f32 = 140.0; // see-through window room on each side, so a big swing isn't cut off
const COFFEE_FOR: f64 = 8.0; // seconds she sips after you give her coffee
const DIZZY_AT: f32 = 8.0; // dragged this far (in her heights, less what's worn off) and she gets dizzy
const DIZZY_WEARS_OFF: f32 = 2.0; // heights per second
const DIZZY_FOR: f64 = 4.0; // seconds she stays dizzy
const REWRITE_TIMEOUT: f64 = 30.0;
const CREAM: Color32 = Color32::from_rgb(0xFB, 0xF4, 0xEA);
const CORAL: Color32 = Color32::from_rgb(0xD9, 0x77, 0x57);
const INK: Color32 = Color32::from_rgb(0x4A, 0x2C, 0x22);
const SOFT_INK: Color32 = Color32::from_rgb(0x8A, 0x5A, 0x4A);
const SIZES: [(&str, f32); 3] = [("S", 0.75), ("M", 1.0), ("L", 1.35)]; // egui zoom; eframe saves it with egui's memory
const IDLE_NUDGE: f64 = 25.0; // quiet seconds before she sparkles and offers help (after any bubble is gone)
const MUSIC_AFTER: f64 = 300.0; // idle seconds before she puts music on by herself
const SLEEP_AFTER: f64 = 600.0; // seconds with nothing going on before she dozes off (music keeps playing)
const OOPS_FOR: f64 = 2.0; // seconds she sobs after a tool call fails
const DAZED_FOR: f64 = 4.0; // seconds of spiral eyes after Claude compacts its context
// "What can I do": she shows each phase in its pose while explaining it.
const TOUR_STEP: f64 = 6.5; // seconds per step
const WORK_MUSIC_KEY: &str = "work_music"; // eframe storage key for the "While working" choice
const VOICE_KEY: &str = "voice"; // her voice on or off
const MUSIC_MODE_KEY: &str = "music_mode"; // Loop / Loop 1 / Shuffle
const PLAYLIST_KEY: &str = "playlist"; // name of the playlist to play
const LANG_KEY: &str = "lang"; // EN or VI

enum Msg {
    Log(PathBuf, Event),
    Rewritten { short: String, long: Option<String>, at: f64 }, // `at`: when its reply arrived, to match it
    Game { word: String, hint: String }, // a new hangman word from Haiku
    Chat(Option<(String, Option<String>)>), // her answer to your chat message (None: `claude` failed)
    Partial { reply: Option<f64>, short: String }, // the short answer so far, streaming in (chat: None, or the rewrite of the reply that arrived at that time)
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
    hide_at: f64,
    is_label: bool, // activity label rather than speech
}

/// The right-click popup: a separate window beside her, so it can sit outside her own window and take
/// keyboard focus for the chat box (hers never does).
struct Popup {
    her: Rect,     // her body on screen when it opened, in points
    work: Rect,    // the desktop area it must stay inside
    left: bool,    // she's on the right half of the screen, so it opens to her left
    size: Vec2,    // measured last frame
    input: String, // the chat box
    frames: u32,
    opened: f64, // when it opened, for the slide-and-fade in
    had_focus: bool,
    memories: bool,       // the memory list is open
    confirm_forget: bool, // "Forget everything" was clicked once
    voice_setup: bool,    // the "my own voice" fields are open
    voice_id: String,
    voice_key: String,    // used once to make the voice, never saved
    confirm_reset: bool,  // "Back to her default voice" was clicked once
}

struct Mini {
    id: String,
    label: String,
    done_at: Option<f64>,
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
        "music" => include_bytes!("../assets/music.png"),
        "game_excited" => include_bytes!("../assets/game_excited.png"),
        "game_excited_blink" => include_bytes!("../assets/game_excited_blink.png"),
        "game_smug" => include_bytes!("../assets/game_smug.png"),
        "game_pouty" => include_bytes!("../assets/game_pouty.png"),
        "cheer" => include_bytes!("../assets/cheer.png"),
        "cheer_blink" => include_bytes!("../assets/cheer_blink.png"),
        "held" => include_bytes!("../assets/held.png"),
        "held_blink" => include_bytes!("../assets/held_blink.png"),
        "held_dizzy" => include_bytes!("../assets/held_dizzy.png"),
        "coffee" => include_bytes!("../assets/coffee.png"),
        "coffee_blink" => include_bytes!("../assets/coffee_blink.png"),
        "done" => include_bytes!("../assets/done.png"),
        "done_blink" => include_bytes!("../assets/done_blink.png"),
        "oops" => include_bytes!("../assets/oops.png"),
        "oops_blink" => include_bytes!("../assets/oops_blink.png"),
        "dizzy" => include_bytes!("../assets/dizzy.png"),
        "reading" => include_bytes!("../assets/reading.png"),
        "reading_blink" => include_bytes!("../assets/reading_blink.png"),
        "editing" => include_bytes!("../assets/editing.png"),
        "editing_blink" => include_bytes!("../assets/editing_blink.png"),
        "testing" => include_bytes!("../assets/testing.png"),
        "testing_blink" => include_bytes!("../assets/testing_blink.png"),
        "shipping" => include_bytes!("../assets/shipping.png"),
        "websearch" => include_bytes!("../assets/websearch.png"),
        "sleep" => include_bytes!("../assets/sleep.png"),
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

/// System fonts that cover Vietnamese, kaomoji (Japanese) and symbols, per OS. Missing ones are skipped.
const SYSTEM_FONTS: &[&str] = if cfg!(windows) {
    &[r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\seguisym.ttf", r"C:\Windows\Fonts\seguiemj.ttf"]
} else if cfg!(target_os = "macos") {
    &[
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Apple Symbols.ttf",
    ]
} else {
    &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
    ]
};

/// Adds system fonts after egui's own, so Vietnamese, kaomoji (Japanese) and symbols render.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (i, file) in SYSTEM_FONTS.iter().enumerate() {
        let name = format!("system{i}");
        if let Ok(bytes) = std::fs::read(file) {
            fonts.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(egui::FontFamily::Proportional).or_default().push(name);
        }
    }
    ctx.set_fonts(fonts);
}

/// Draws a pose image bent by the animation, standing on `feet`. `since` = seconds in this pose, for the entry pop.
#[allow(clippy::too_many_arguments)]
fn draw_sprite(painter: &Painter, tex: &TextureHandle, feet: Pos2, height: f32, t: f32, pose: Pose, poke_t: f32, swing: Vec2, since: f32, alpha: f32) {
    let mut mesh = Mesh::with_texture(tex.id());
    let (gx, gy) = GRID;
    let scale = anim::squash(pose, since);
    let color = Color32::WHITE.gamma_multiply(alpha);
    for j in 0..=gy {
        for i in 0..=gx {
            let (u, v) = (i as f32 / gx as f32, j as f32 / gy as f32);
            let p = anim::warp(u, v, t, pose, poke_t, tex.aspect_ratio(), swing) * scale;
            mesh.vertices.push(egui::epaint::Vertex { pos: feet + p * height, uv: pos2(u, v), color });
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
    poke_at: Option<f64>,
    held: bool,  // you're dragging her
    swing: Vec2, // her pendulum while held: angle in degrees, speed in rad/s
    coffee_until: f64, // sipping the coffee you gave her until then
    dizzy: f32,       // how shaken she is, in heights dragged
    dizzy_until: f64, // seeing stars until then
    hold: Option<(Pos2, f32, f64)>, // while we move the window ourselves: last cursor (screen points), its sideways speed, time
    blink_at: f64,
    shown: Pose,      // the pose drawn last frame
    prev_shown: Pose, // the one before it, fading out
    shown_at: f64,    // when `shown` started
    rng: u64,
    inbox: Receiver<Msg>,
    outbox: Sender<Msg>,
    session: Option<PathBuf>, // the Claude session she follows: the one you last typed in
    bubble: Option<Bubble>,
    pending_reply: Option<(String, f64)>, // raw reply waiting for its rewrite, and when it arrived
    minis: Vec<Mini>,
    hit_rects: Vec<Rect>, // bubbles and menu drawn last frame, which must stay clickable
    passthrough: bool,
    waiting_since: f64,
    waiting_line: &'static str, // the permission line picked for this prompt, so it doesn't change while she waits
    hwnd: isize,
    placed: bool, // checked that she's on screen after eframe restored her position
    first_run: bool, // no saved window position yet
    terminal: isize, // foreground window when you last sent a prompt; double-click brings it back
    hidden_until: f64,
    shells: Vec<String>, // background commands still running in the followed session (task ids)
    nudge_at: f64,       // when she next offers help, if she stays idle and quiet
    sparkle_at: f64,     // when her last idle sparkle started
    idle_since: f64,     // start of the current idle stretch
    music: Option<bool>, // playing; true if she started it herself (it stops when work starts)
    work_music: bool,    // use the music pose instead of the working pose while Claude works
    tour: Option<(f64, usize)>, // "What can I do": start time and the step already said
    playlist_open: bool,
    view_list: usize,          // playlist shown in the song list
    list_scroll: usize,        // first song row shown
    music_mode: music::Mode,   // remembered
    lang: i18n::Lang,          // remembered
    playlist_name: String,     // remembered: the playlist to play
    game: Option<game::Game>,
    sudoku: Option<sudoku::Sudoku>,
    picking: bool, // "Play with me" asked which game
    game_loading: bool,
    recent_words: Vec<String>,  // so new rounds don't repeat
    game_line: String, // what she's saying in the game panel
    popup: Option<Popup>,
    chat_pending: bool,
    last_reply: String, // Claude's last reply, plain, so chat knows what just happened
    finished_at: f64,   // when Claude's work was last confirmed finished: she shows Done briefly
    work: log::Work,    // what the current tool call is doing, for her working pose
    oops_until: f64,    // a tool call failed: sobbing until then
    dazed_until: f64,   // Claude compacted its context: spiral eyes until then
    quiet_since: f64,   // start of the stretch with nothing going on; she sleeps after SLEEP_AFTER
    voice_on: bool,     // remembered
    filler: usize,      // the working line she said last
    last_input: u32,    // OS tick of the last keyboard or mouse input seen
    last_cursor: Option<(f32, f32)>,
    keys: Vec<f64>,     // recent inputs in your terminal with the mouse still: keystrokes
    typed_at: f64,      // last keystroke while she's cheering you on
    cheer_until: f64,   // cheering because a prompt was just sent
    cheer_said: f64,    // when she last said a cheer line
}

impl App {
    fn new(cc: &eframe::CreationContext, listener: TcpListener) -> Self {
        let ctx = cc.egui_ctx.clone();
        let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(w)) => w.hwnd.get(),
            _ => 0,
        };
        os::remove_border(hwnd);
        install_fonts(&ctx);
        // A skin folder with a readable idle.png replaces all built-in art, so characters never mix.
        let custom_idle = load(&ctx, "idle", true);
        let skin = custom_idle.is_some();
        let (idle, idle_alpha, idle_size) = custom_idle.or_else(|| load(&ctx, "idle", false)).expect("assets/idle.png is required");
        // A missing pose borrows the closest existing one (`stand_in`), then idle.
        let sprite = |name: &str, blink: &str, stand_in: &str| {
            let loaded = load(&ctx, name, skin).map(|l| (l, load(&ctx, blink, skin).map(|b| b.0)));
            let ((open, alpha, size), blink) = loaded.unwrap_or_else(|| match load(&ctx, stand_in, skin) {
                Some(l) => (l, None),
                None => ((idle.clone(), idle_alpha.clone(), idle_size), None),
            });
            Sprite { open, blink, alpha, size }
        };
        // Order matches the Pose enum.
        let sprites = vec![
            sprite("idle", "idle_blink", ""),
            sprite("working", "working_blink", ""),
            sprite("waiting", "", ""),
            sprite("poke", "", ""),
            sprite("music", "", ""),
            sprite("game_excited", "game_excited_blink", "idle"),
            sprite("game_smug", "", "waiting"),
            sprite("game_pouty", "", "poke"),
            sprite("cheer", "cheer_blink", "waiting"),
            sprite("held", "held_blink", "game_excited"),
            sprite("held_dizzy", "", "held"),
            sprite("coffee", "coffee_blink", "idle"),
            sprite("done", "done_blink", "waiting"),
            sprite("oops", "oops_blink", "idle"),
            sprite("dizzy", "", "idle"),
            sprite("reading", "reading_blink", "working"),
            sprite("editing", "editing_blink", "working"),
            sprite("testing", "testing_blink", "working"),
            sprite("shipping", "", "working"),
            sprite("websearch", "", "working"),
            sprite("sleep", "", "idle"),
        ];

        std::thread::spawn(chat::preload); // memory.json and the embedding model, off the UI thread
        std::thread::spawn(voice::init);
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
        // Restore the music choices; the player thread starts and reads the music folder in the background.
        let saved = |key: &str| cc.storage.and_then(|s| s.get_string(key));
        let music_mode = saved(MUSIC_MODE_KEY).and_then(|m| music::Mode::from_label(&m)).unwrap_or(music::Mode::Shuffle);
        let playlist_name = saved(PLAYLIST_KEY).unwrap_or_else(|| music::BUILT_IN.to_string());
        music::set_mode(music_mode);
        let voice_on = saved(VOICE_KEY).is_none_or(|v| v == "true");
        voice::set_on(voice_on);
        let lang = saved(LANG_KEY).and_then(|l| i18n::Lang::from_code(&l)).unwrap_or(i18n::Lang::En);
        i18n::set(lang);
        music::select(&playlist_name);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        Self {
            ctx,
            sprites,
            pose: Pose::Idle,
            start: Instant::now(),
            poke_at: None,
            held: false,
            swing: Vec2::ZERO,
            hold: None,
            dizzy: 0.0,
            coffee_until: -99.0,
            dizzy_until: -99.0,
            blink_at: 2.0,
            shown: Pose::Idle,
            prev_shown: Pose::Idle,
            shown_at: -99.0,
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
            waiting_line: "",
            hwnd,
            placed: false,
            first_run: saved("window").is_none(), // eframe's key for the saved window position
            terminal: 0,
            hidden_until: 0.0,
            shells: vec![],
            nudge_at: IDLE_NUDGE,
            sparkle_at: -99.0,
            idle_since: 0.0,
            music: None,
            work_music: cc.storage.and_then(|s| s.get_string(WORK_MUSIC_KEY)).is_some_and(|v| v == "true"),
            tour: None,
            playlist_open: false,
            view_list: 0,
            list_scroll: 0,
            music_mode,
            playlist_name,
            lang,
            game: None,
            sudoku: None,
            picking: false,
            game_loading: false,
            recent_words: vec![],
            game_line: String::new(),
            popup: None,
            chat_pending: false,
            last_reply: String::new(),
            finished_at: -99.0,
            work: log::Work::Plain,
            oops_until: -99.0,
            dazed_until: -99.0,
            quiet_since: 0.0,
            voice_on,
            filler: 0,
            last_input: 0,
            last_cursor: None,
            keys: vec![],
            typed_at: -99.0,
            cheer_until: -99.0,
            cheer_said: -99.0,
        }
    }

    /// Resize her (egui zoom scales the whole window), keeping her feet where they were.
    fn set_zoom(&self, z: f32) {
        let old = self.ctx.zoom_factor();
        self.ctx.set_zoom_factor(z);
        // The new zoom only lands next frame, so these commands are still in old-zoom points.
        let size = window_size() * z / old;
        // Called from the popup too, so name her window: plain commands would go to the popup.
        self.ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::InnerSize(size));
        if let Some(outer) = self.ctx.input_for(ViewportId::ROOT, |i| i.viewport().outer_rect) {
            let feet = outer.min + vec2(outer.width() / 2.0, outer.height());
            let mut pos = feet - size * vec2(0.5, 1.0);
            if let Some(work) = os::work_area(self.hwnd).map(|w| w / old) {
                pos = pos.min(work.max - size).max(work.min); // a bigger her mustn't spill off screen
            }
            self.ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::OuterPosition(pos));
        }
    }

    /// A random line from `lines`.
    fn pick(&mut self, lines: &'static [&'static str]) -> &'static str {
        lines[(self.rand01() * lines.len() as f32) as usize % lines.len()]
    }

    fn rand01(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 10_000) as f32 / 10_000.0
    }

    fn say(&mut self, short: String, long: Option<String>, t: f64) {
        voice::say(&short, voice::Mode::Chatter); // only her recorded lines; replies and chat speak live (see say_live)
        let hide_at = t + 8.0 + short.chars().count() as f64 / 12.0; // roughly reading time
        self.bubble = Some(Bubble { short, long, expanded: false, hide_at, is_label: false });
    }

    fn label(&mut self, text: String, t: f64) {
        self.bubble = Some(Bubble { short: text, long: None, expanded: false, hide_at: t + 120.0, is_label: true });
    }

    /// Her pose once Claude's turn is over: still working while a background command or agent runs.
    fn rest(&self) -> Pose {
        if self.shells.is_empty() && self.minis.iter().all(|m| m.done_at.is_some()) { Pose::Idle } else { Pose::Working }
    }

    /// Back to her resting pose. Work going to Idle is confirmed finished: she shows Done briefly (your turn).
    fn settle(&mut self, t: f64) {
        let was = self.pose;
        self.pose = self.rest();
        if was != Pose::Idle && self.pose == Pose::Idle {
            self.finished_at = t;
        }
    }

    fn set_music(&mut self, on: bool, t: f64) {
        if on {
            music::play();
            self.music = Some(false);
            let line = self.pick(i18n::lines().music);
            self.say(line.into(), None, t - 4.0);
        } else {
            music::stop();
            self.music = None;
            self.idle_since = t; // stopping it means no auto-play for another while
        }
    }

    /// Asks Haiku for a word on a worker thread; the panel shows "thinking" until it arrives.
    fn start_game(&mut self) {
        (self.game, self.game_loading, self.tour, self.sudoku, self.picking) = (None, true, None, None, false);
        self.game_line = game::line(i18n::lines().thinking, self.rand01(), "");
        voice::say(&self.game_line, voice::Mode::Chatter);
        let (tx, wake, avoid) = (self.outbox.clone(), self.ctx.clone(), self.recent_words.clone());
        let seed = (self.rand01() * 1_000_000.0) as u32;
        std::thread::spawn(move || {
            let (word, hint) = game::new_word(seed, &avoid);
            let _ = tx.send(Msg::Game { word, hint });
            wake.request_repaint();
        });
    }

    /// A fresh puzzle, made right here: it takes well under a millisecond.
    fn start_sudoku(&mut self) {
        (self.game, self.game_loading, self.tour, self.picking) = (None, false, None, false);
        self.sudoku = Some(sudoku::Sudoku::new((self.rand01() * 1_000_000.0) as u32));
        self.game_line = game::line(i18n::lines().sudoku_start, self.rand01(), "");
        voice::say(&self.game_line, voice::Mode::Chatter);
    }

    /// A game panel is open (or a hangman word is on its way, or she's asking which game).
    fn playing(&self) -> bool {
        self.picking || self.game.is_some() || self.game_loading || self.sudoku.is_some()
    }

    /// "Play with me": a small panel asking which game, with a button for each and a close.
    fn game_picker(&mut self, ui: &egui::Ui, painter: &Painter, anchor: Pos2) {
        let text = i18n::ui();
        let panel = Rect::from_min_size(anchor - vec2(110.0, 92.0), vec2(220.0, 88.0));
        painter.rect(panel, 10.0, CREAM, Stroke::new(2.0, CORAL), egui::StrokeKind::Inside);
        self.hit_rects.push(panel);
        painter.text(pos2(panel.center().x, panel.top() + 18.0), egui::Align2::CENTER_CENTER, text.pick_game, FontId::proportional(13.5), INK);
        let mut chosen = None;
        for (i, label) in [text.hangman, text.sudoku].into_iter().enumerate() {
            let r = Rect::from_center_size(pos2(panel.center().x + (i as f32 - 0.5) * 100.0, panel.top() + 50.0), vec2(92.0, 26.0));
            let resp = ui.interact(r, ui.scope_id().with(("pick-game", i)), Sense::click());
            painter.rect(r, 8.0, CORAL.gamma_multiply(if resp.hovered() { 0.3 } else { 0.15 }), Stroke::new(1.0, CORAL), egui::StrokeKind::Inside);
            painter.text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(13.0), INK);
            if resp.clicked() {
                chosen = Some(i);
            }
        }
        let close = Rect::from_center_size(pos2(panel.center().x, panel.bottom() - 11.0), vec2(60.0, 16.0));
        let close_resp = ui.interact(close, ui.scope_id().with("pick-close"), Sense::click());
        painter.text(close.center(), egui::Align2::CENTER_CENTER, text.close, FontId::proportional(11.5), if close_resp.hovered() { INK } else { SOFT_INK });
        match chosen {
            Some(0) => self.start_game(),
            Some(_) => self.start_sudoku(),
            None if close_resp.clicked() => self.picking = false,
            None => {}
        }
    }

    /// Her line, the 9x9 grid, and beside it a 1-9 pad and the buttons. Click a cell, then a number.
    fn sudoku_panel(&mut self, ui: &egui::Ui, painter: &Painter, anchor: Pos2, t: f64) {
        let Some(s) = &self.sudoku else { return };
        let text = i18n::ui();
        let w = WIDTH - 40.0;
        let panel = Rect::from_min_size(anchor - vec2(w / 2.0, 222.0), vec2(w, 220.0));
        painter.rect(panel, 10.0, CREAM, Stroke::new(2.0, CORAL), egui::StrokeKind::Inside);
        self.hit_rects.push(panel);
        let said = painter.layout(self.game_line.clone(), FontId::proportional(12.5), INK, w - 20.0);
        painter.galley(panel.min + vec2(10.0, 8.0), said, INK);
        let (cell, top) = (17.0, panel.top() + 46.0);
        let grid = Rect::from_min_size(pos2(panel.left() + 10.0, top), Vec2::splat(9.0 * cell));
        let (won, sel) = (s.won(), s.selected);
        let (mut pick, mut put) = (None, None);
        for i in 0..81 {
            let r = Rect::from_min_size(grid.min + vec2((i % 9) as f32, (i / 9) as f32) * cell, Vec2::splat(cell));
            let resp = ui.interact(r, ui.scope_id().with(("cell", i)), Sense::click());
            let near = sel.is_some_and(|j| i / 9 == j / 9 || i % 9 == j % 9 || (i / 27 == j / 27 && i % 9 / 3 == j % 9 / 3));
            let fill = if sel == Some(i) {
                CORAL.gamma_multiply(0.4)
            } else if resp.hovered() && !won {
                CORAL.gamma_multiply(0.2)
            } else if near {
                CORAL.gamma_multiply(0.1)
            } else {
                Color32::TRANSPARENT
            };
            painter.rect_filled(r, 0.0, fill);
            painter.rect_stroke(r, 0.0, Stroke::new(0.5, INK.gamma_multiply(0.15)), egui::StrokeKind::Inside);
            if s.cells[i] != 0 {
                let ink = if s.clash(i) { Color32::from_rgb(0xD0, 0x30, 0x30) } else if s.given[i] { INK } else { CORAL };
                painter.text(r.center(), egui::Align2::CENTER_CENTER, s.cells[i], FontId::proportional(12.5), ink);
            }
            if resp.clicked() && !won {
                pick = Some(i);
            }
        }
        // Thick lines around the 3x3 boxes.
        for k in 0..=3 {
            let d = k as f32 * 3.0 * cell;
            let stroke = Stroke::new(1.5, INK.gamma_multiply(0.6));
            painter.line_segment([grid.min + vec2(d, 0.0), grid.min + vec2(d, grid.height())], stroke);
            painter.line_segment([grid.min + vec2(0.0, d), grid.min + vec2(grid.width(), d)], stroke);
        }
        // The 1-9 pad, then Erase, New puzzle and Close.
        let (left, pad_on) = (grid.right() + 12.0, sel.is_some_and(|i| !s.given[i]) && !won);
        let btn = |r: Rect, id: (&str, usize), label: String, on: bool| {
            let resp = ui.interact(r, ui.scope_id().with(id), Sense::click());
            let fill = if resp.hovered() && on { CORAL.gamma_multiply(0.3) } else { CORAL.gamma_multiply(0.12) };
            painter.rect(r, 6.0, fill, Stroke::new(1.0, CORAL.gamma_multiply(if on { 1.0 } else { 0.4 })), egui::StrokeKind::Inside);
            painter.text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(12.5), if on { INK } else { INK.gamma_multiply(0.35) });
            resp.clicked() && on
        };
        for v in 1..=9u8 {
            let k = (v - 1) as f32;
            let r = Rect::from_min_size(pos2(left + (k % 3.0) * 38.0, top + (k / 3.0).floor() * 29.0), vec2(35.0, 26.0));
            if btn(r, ("pad", v as usize), v.to_string(), pad_on) {
                put = Some(v);
            }
        }
        let row = |n: f32| Rect::from_min_size(pos2(left, top + 90.0 + n * 28.0), vec2(111.0, 24.0));
        if btn(row(0.0), ("sudoku-btn", 0), text.erase.into(), pad_on) {
            put = Some(0);
        }
        let again = btn(row(1.0), ("sudoku-btn", 1), text.new_puzzle.into(), true);
        let close = btn(row(2.0), ("sudoku-btn", 2), text.close.into(), true);
        if let Some(s) = &mut self.sudoku {
            if pick.is_some() {
                s.selected = pick;
            }
            if let Some(v) = put {
                s.put(v);
                if s.won() {
                    self.game_line = game::line(i18n::lines().sudoku_won, self.rand01(), "");
                    voice::say(&self.game_line, voice::Mode::Chatter);
                    self.sparkle_at = t;
                }
            }
        }
        if again {
            self.start_sudoku();
        }
        if close {
            self.sudoku = None;
        }
    }

    /// The popup beside her, on whichever side has more room.
    fn open_popup(&self, body: Rect, t: f64) -> Option<Popup> {
        let inner = self.ctx.input(|i| i.viewport().inner_rect)?;
        let her = body.translate(inner.min.to_vec2());
        let work = os::work_area(self.hwnd)
            .map(|w| w / self.ctx.zoom_factor())
            .or_else(|| self.ctx.input(|i| i.viewport().monitor_size).map(|m| Rect::from_min_size(Pos2::ZERO, m)))
            .unwrap_or(Rect::EVERYTHING);
        let left = her.center().x > work.center().x;
        rewrite::prewarm(chat::persona()); // you'll likely type: have Haiku ready
        Some(Popup { her, work, left, size: vec2(POPUP_W, 360.0), input: String::new(), frames: 0, opened: t, had_focus: false, memories: false, confirm_forget: false, voice_setup: false, voice_id: String::new(), voice_key: String::new(), confirm_reset: false })
    }

    fn show_popup(&mut self, ctx: &egui::Context, t: f64) {
        let Some(p) = &self.popup else { return };
        let x = if p.left { p.her.left() - p.size.x - 6.0 } else { p.her.right() + 6.0 };
        let x = x.min(p.work.right() - p.size.x).max(p.work.left());
        // Slides out from her side: starts POPUP_SLIDE closer to her and eases out to its place.
        let k = popup_in((t - p.opened) as f32);
        let x = x + (1.0 - k) * POPUP_SLIDE * if p.left { 1.0 } else { -1.0 };
        let y = (p.her.bottom() - p.size.y).min(p.work.bottom() - p.size.y).max(p.work.top());
        let builder = egui::ViewportBuilder::default()
            .with_title("Claudesona")
            .with_position(pos2(x, y))
            .with_inner_size(p.size)
            .with_decorations(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false);
        ctx.show_viewport_immediate(ViewportId::from_hash_of("popup"), builder, |ui, _| self.popup_ui(ui, t));
    }

    /// The menu, the chat box and her memories. Runs inside the popup's own window.
    fn popup_ui(&mut self, ui: &mut egui::Ui, t: f64) {
        let Some(mut p) = self.popup.take() else { return };
        let text = i18n::ui();
        let ctx = ui.ctx().clone();
        let (focused, esc, close_asked) = ui.input(|i| (i.viewport().focused, i.key_pressed(Key::Escape), i.viewport().close_requested()));
        if p.frames == 0 {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        p.had_focus |= focused == Some(true);
        // Clicking anywhere else takes its focus away: that closes it, like a normal menu.
        let mut close = esc || close_asked || (p.had_focus && focused == Some(false));
        *ui.visuals_mut() = egui::Visuals::light();
        ui.visuals_mut().override_text_color = Some(INK);
        let k = popup_in((t - p.opened) as f32);
        ui.set_opacity(k);
        if k < 1.0 {
            ctx.request_repaint();
        }
        let frame = egui::Frame::new().fill(CREAM).stroke(Stroke::new(2.0, CORAL)).corner_radius(10).inner_margin(10);
        let shown = frame.show(ui, |ui| {
            ui.set_width(POPUP_W - 24.0);
            ui.horizontal(|ui| {
                ui.label(text.size);
                for (name, z) in SIZES {
                    if ui.selectable_label(ctx.zoom_factor() == z, name).clicked() {
                        self.set_zoom(z);
                        close = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(text.language);
                for lang in [i18n::Lang::En, i18n::Lang::Vi] {
                    if ui.selectable_label(self.lang == lang, lang.code()).clicked() {
                        self.lang = lang;
                        i18n::set(lang);
                    }
                }
            });
            if ui.button(if self.music.is_some() { text.stop_music } else { text.play_music }).clicked() {
                self.set_music(self.music.is_none(), t);
                close = true;
            }
            ui.horizontal(|ui| {
                ui.label(text.while_working);
                if ui.selectable_label(!self.work_music, text.working).clicked() {
                    self.work_music = false;
                }
                if ui.selectable_label(self.work_music, text.music).clicked() {
                    self.work_music = true;
                }
            });
            if ui.button(text.open_music_folder).clicked() {
                open_music_folder();
                close = true;
            }
            if ui.button(text.play_with_me).clicked() {
                (self.picking, self.tour) = (true, None);
                close = true;
            }
            if ui.button(text.give_coffee).clicked() {
                self.coffee_until = t + COFFEE_FOR;
                let line = self.pick(i18n::lines().coffee);
                self.say(line.into(), None, t - 6.0);
                close = true;
            }
            if ui.button(text.what_can_i_do).clicked() {
                self.tour = Some((t, usize::MAX));
                close = true;
            }
            if ui.button(text.hide).clicked() {
                self.hidden_until = t + 600.0;
                close = true;
            }
            if ui.button(text.quit).clicked() {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
            }

            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.voice_on, text.voice).changed() {
                    voice::set_on(self.voice_on);
                }
                if ui.selectable_label(p.voice_setup, text.my_voice).clicked() {
                    p.voice_setup = !p.voice_setup;
                }
            });
            if p.voice_setup {
                // The key is saved next to the exe: live speech (replies, chat) needs it every time.
                ui.horizontal(|ui| {
                    if voice::has_key() {
                        ui.label(text.key_saved);
                        if ui.button(text.forget_key).clicked() {
                            voice::save_key("");
                        }
                    } else {
                        ui.add(egui::TextEdit::singleline(&mut p.voice_key).hint_text(text.api_key_hint).password(true).desired_width(150.0));
                        if ui.button(text.save_key).clicked() {
                            voice::save_key(&std::mem::take(&mut p.voice_key));
                        }
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut p.voice_id).hint_text(text.voice_id_hint).desired_width(f32::INFINITY));
                let chars: usize = i18n::voiced().iter().map(|(l, _)| voice::spoken(l).chars().count()).sum();
                if ui.add_enabled(voice::has_key(), egui::Button::new(text.make_voice.replace("{n}", &chars.to_string()))).clicked() {
                    voice::make_custom(p.voice_id.clone());
                }
                if voice::custom() && ui.button(if p.confirm_reset { text.forget_sure } else { text.default_voice }).clicked() {
                    if p.confirm_reset {
                        voice::reset_custom();
                    }
                    p.confirm_reset = !p.confirm_reset;
                }
                let progress = voice::progress();
                if !progress.is_empty() {
                    ui.label(RichText::new(progress).small().color(SOFT_INK));
                }
            }

            ui.separator();
            let facts = chat::facts();
            ui.horizontal(|ui| {
                if ui.selectable_label(p.memories, format!("{} ({})", text.memories, facts.len())).clicked() {
                    p.memories = !p.memories;
                }
                // Two clicks, because it can't be undone.
                if ui.button(if p.confirm_forget { text.forget_sure } else { text.forget_all }).clicked() {
                    if p.confirm_forget {
                        chat::forget_all();
                    }
                    p.confirm_forget = !p.confirm_forget;
                }
            });
            if p.memories {
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for (id, fact, links) in &facts {
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("✕").clicked() {
                                chat::forget(*id);
                            }
                            ui.label(fact);
                        });
                        for l in links {
                            ui.label(RichText::new(format!("    {l}")).small().color(SOFT_INK));
                        }
                    }
                });
            }
            let hint = if self.chat_pending { text.chat_thinking } else { text.chat_hint };
            let edit = ui.add(egui::TextEdit::singleline(&mut p.input).hint_text(hint).desired_width(f32::INFINITY));
            if p.frames == 0 {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !p.input.trim().is_empty() && !self.chat_pending {
                self.send_chat(p.input.trim().to_string(), t);
                close = true;
            }
        });
        // Fit the window to what was drawn; the new size lands next frame.
        let size = vec2(POPUP_W, shown.response.rect.height().ceil());
        if (size - p.size).length() > 0.5 {
            p.size = size;
            ctx.request_repaint();
        }
        p.frames += 1;
        if !close {
            self.popup = Some(p);
        }
    }

    /// Sends your chat message to Haiku on a worker thread, with what Claude is doing right now.
    fn send_chat(&mut self, msg: String, t: f64) {
        self.chat_pending = true;
        self.label(i18n::ui().chat_thinking.into(), t);
        let doing = match (self.pose, &self.bubble) {
            (Pose::Waiting, _) => "Waiting for the user to answer a permission prompt or question.".to_string(),
            (Pose::Working, Some(b)) if b.is_label => format!("Working: {}", b.short),
            (Pose::Working, _) => "Working on the user's request.".to_string(),
            _ => "Nothing right now: idle.".to_string(),
        };
        let work = if self.last_reply.is_empty() { doing } else { format!("{doing}\nClaude's last reply: {}", self.last_reply) };
        let (tx, wake) = (self.outbox.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let partial = |short: &str| {
                let _ = tx.send(Msg::Partial { reply: None, short: short.to_string() });
                wake.request_repaint();
            };
            let answer = chat::reply(&msg, &work, partial);
            let _ = tx.send(Msg::Chat(answer));
            wake.request_repaint();
        });
    }

    fn handle(&mut self, msg: Msg, t: f64) {
        // A new prompt or a permission request ends the introduction; other activity carries on underneath it.
        if matches!(msg, Msg::Log(_, Event::Prompt)) || matches!(&msg, Msg::Hook(v) if v["hook_event_name"] == "PermissionRequest") {
            self.tour = None;
        }
        let (path, ev) = match msg {
            Msg::Rewritten { short, long, at } => {
                // Only the rewrite of the reply still waiting: not one that timed out, or an older reply's slow one.
                if self.pending_reply.as_ref().is_some_and(|p| p.1 == at) {
                    self.pending_reply = None;
                    self.settle(t);
                    voice::say_live(&short, false); // when her voice is charged
                    self.say(short, long, t);
                }
                return;
            }
            Msg::Game { word, hint } => {
                if self.game_loading {
                    self.recent_words.push(word.clone());
                    if self.recent_words.len() > 12 {
                        self.recent_words.remove(0);
                    }
                    self.game_line = game::line(i18n::lines().start, self.rand01(), "");
                    voice::say(&self.game_line, voice::Mode::Chatter);
                    (self.game, self.game_loading) = (Some(game::Game::new(word, hint)), false);
                }
                return;
            }
            Msg::Partial { reply, short } => {
                let wanted = match reply {
                    None => self.chat_pending,
                    Some(at) => self.pending_reply.as_ref().is_some_and(|p| p.1 == at),
                };
                if wanted {
                    self.say(short, None, t);
                }
                return;
            }
            Msg::Chat(answer) => {
                self.chat_pending = false;
                let (short, long) = answer.unwrap_or_else(|| (i18n::ui().chat_failed.to_string(), None));
                voice::say_live(&short, true); // you're talking to her: always
                self.say(short, long, t);
                return;
            }
            Msg::Hook(v) => {
                if v["quit"] == true {
                    self.ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                // PermissionRequest fires the moment the dialog shows. The permission_prompt Notification only
                // comes after ~6s without typing; it still covers sandbox network prompts, which have no PermissionRequest.
                let notified = v["notification_type"] == "permission_prompt" || v["message"].as_str().is_some_and(|m| m.contains("permission"));
                let followed = self.session.as_ref().is_none_or(|p| p.file_stem().is_some_and(|s| v["session_id"] == *s.to_string_lossy()));
                // A shell command is starting. Its log line only comes after it finishes.
                // Claude asking you a question waits on you just like a permission prompt.
                let asking = v["hook_event_name"] == "PreToolUse" && v["tool_name"] == "AskUserQuestion" && followed;
                if v["hook_event_name"] == "PreToolUse" && !asking && followed && !(self.pose == Pose::Waiting && t - self.waiting_since < 2.0) {
                    self.pose = Pose::Working;
                    self.work = log::work_kind(v["tool_name"].as_str().unwrap_or(""), &v["tool_input"]);
                    self.label(log::tool_label(v["tool_name"].as_str().unwrap_or(""), &v["tool_input"]), t);
                }
                // Claude Code is closing, which stops its background commands.
                // Its unfinished agents stop too. She says goodbye with a brief Done pose.
                if v["hook_event_name"] == "SessionEnd" && followed {
                    self.shells.clear();
                    self.minis.retain(|m| m.done_at.is_some());
                    if self.pending_reply.is_none() && self.pose != Pose::Waiting {
                        (self.pose, self.finished_at) = (Pose::Idle, t);
                        let line = self.pick(i18n::lines().bye);
                        self.say(line.into(), None, t - 4.0);
                    }
                }
                if asking || v["hook_event_name"] == "PermissionRequest" || (v["hook_event_name"] == "Notification" && notified) {
                    let fresh = self.pose != Pose::Waiting; // the later Notification backup doesn't call out again
                    self.pose = Pose::Waiting;
                    self.waiting_since = t;
                    self.waiting_line = self.pick(i18n::lines().waiting);
                    if fresh {
                        voice::say(self.waiting_line, voice::Mode::Attention); // out loud, always: Claude needs you
                    }
                    self.say(self.waiting_line.into(), None, t + 600.0); // stays until Claude moves on
                }
                return;
            }
            Msg::Log(path, ev) => (path, ev),
        };
        if ev == Event::Prompt {
            if self.session.as_ref() != Some(&path) {
                self.shells.clear(); // they belong to the session she stops following
            }
            self.session = Some(path.clone());
            let fg = os::foreground();
            if fg != 0 && fg != self.hwnd {
                self.terminal = fg;
            }
            self.cheer_until = t + CHEER_ON_SEND;
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
            self.say(self.waiting_line.into(), None, t + 600.0);
        }
    }

    fn apply(&mut self, ev: Event, t: f64) {
        match ev {
            Event::Prompt => {
                rewrite::prewarm(rewrite::persona()); // ready for this turn's reply
                self.pose = Pose::Working;
                self.work = log::Work::Plain;
                self.bubble = None;
                self.pending_reply = None;
            }
            Event::Tool(text, work) => {
                self.pose = Pose::Working;
                self.work = work;
                self.label(text, t);
            }
            Event::Note(text) => {
                self.pose = Pose::Working;
                self.label(text, t);
            }
            Event::Failed => self.oops_until = t + OOPS_FOR,
            Event::Compact => self.dazed_until = t + DAZED_FOR,
            Event::AgentStart { id, label } => self.minis.push(Mini { id, label, done_at: None }),
            Event::AgentDone(id) => {
                if let Some(m) = self.minis.iter_mut().find(|m| m.id == id) {
                    m.done_at.get_or_insert(t);
                }
            }
            Event::Reply(text) => {
                if self.bubble.as_ref().is_some_and(|b| b.is_label) {
                    self.bubble = None; // the last activity is stale: working lines fill in until the rewrite streams
                }
                self.last_reply = log::clean(&text, 300);
                self.pending_reply = Some((text.clone(), t));
                let (tx, wake) = (self.outbox.clone(), self.ctx.clone());
                std::thread::spawn(move || {
                    let partial = |short: &str| {
                        let _ = tx.send(Msg::Partial { reply: Some(t), short: short.to_string() });
                        wake.request_repaint();
                    };
                    let (short, long) = rewrite::rewrite(&text, partial).unwrap_or_else(|| (log::clean(&text, 160), None));
                    let _ = tx.send(Msg::Rewritten { short, long, at: t });
                    wake.request_repaint();
                });
            }
            Event::ShellStart(id) => self.shells.push(id),
            Event::ShellDone(id) => self.shells.retain(|s| *s != id),
            Event::TurnEnd => {
                if self.pending_reply.is_none() {
                    self.settle(t);
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
    // eframe calls this every 30s and on exit, next to the window position and size it already saves.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(WORK_MUSIC_KEY, self.work_music.to_string());
        storage.set_string(VOICE_KEY, self.voice_on.to_string());
        storage.set_string(MUSIC_MODE_KEY, self.music_mode.label().to_string());
        storage.set_string(PLAYLIST_KEY, self.playlist_name.clone());
        storage.set_string(LANG_KEY, self.lang.code().to_string());
    }

    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = self.ctx.clone();
        let t = self.start.elapsed().as_secs_f64(); // f64: an f32 count of seconds goes coarse after a few days of uptime
        while let Ok(msg) = self.inbox.try_recv() {
            self.handle(msg, t);
        }
        if let Some((text, since)) = &self.pending_reply
            && t - since > REWRITE_TIMEOUT
        {
            let plain = log::clean(text, 160);
            self.pending_reply = None;
            self.settle(t);
            self.say(plain, None, t);
        }
        if self.bubble.as_ref().is_some_and(|b| t > b.hide_at) {
            self.bubble = None;
        }
        // You're typing in the terminal you last prompted from: input arrives while the mouse stays still (Windows only;
        // which key is never read). A few inputs in a row start the cheer, so a lone click doesn't; then any key keeps it going.
        let (input, cursor) = (os::last_input(), os::cursor(1.0));
        if input != self.last_input && cursor == self.last_cursor && self.terminal != 0 && os::foreground() == self.terminal {
            self.keys.retain(|k| t - k < 1.5);
            self.keys.push(t);
            if self.keys.len() >= 4 || t - self.typed_at < TYPING_SHOWS {
                self.typed_at = t;
            }
        }
        (self.last_input, self.last_cursor) = (input, cursor);
        let cheering = matches!(self.pose, Pose::Idle | Pose::Working) && (t - self.typed_at < TYPING_SHOWS || t < self.cheer_until);
        if cheering && t - self.cheer_said > CHEER_LINE_EVERY && !self.playing() && self.tour.is_none() {
            self.cheer_said = t;
            let line = self.pick(i18n::lines().cheer);
            self.say(line.into(), None, t - 4.0);
        }
        // Working with nothing to say (Claude thinking, rewrite not back yet): a general line that expires after
        // FILLER_EVERY, so the next frame swaps in another until real activity or the reply arrives.
        if self.pose == Pose::Working && self.bubble.is_none() && !self.playing() && self.tour.is_none() {
            let lines = i18n::lines().working;
            let next = (self.filler + 1 + (self.rand01() * (lines.len() - 1) as f32) as usize) % lines.len(); // never the same twice
            self.filler = next;
            self.label(lines[next].into(), t);
            voice::say(lines[next], voice::Mode::Chatter); // uses her charge: one every ~30s while working
            if let Some(b) = &mut self.bubble {
                b.hide_at = t + FILLER_EVERY;
            }
        }
        self.minis.retain(|m| m.done_at.is_none_or(|d| t - d < 2.0));
        // Hidden: keep following Claude, but draw nothing and let every click through.
        if t < self.hidden_until {
            self.quiet_since = t; // hiding isn't a quiet stretch: she comes back awake
            if !self.passthrough {
                self.passthrough = true;
                ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(true));
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(self.hidden_until - t));
            return;
        }

        let poke_t = self.poke_at.map(|p| t - p).filter(|d| *d < 1.5);
        // The pose to show: music replaces idle, and optionally working; pokes and the tour override.
        // Anything going on keeps her awake; poking or dragging her wakes her.
        if self.pose != Pose::Idle || cheering || poke_t.is_some() || self.held || self.popup.is_some() || self.playing() || self.tour.is_some() || t < self.coffee_until {
            self.quiet_since = t;
        }
        let asleep = t - self.quiet_since > SLEEP_AFTER;
        let mut pose = match self.pose {
            Pose::Idle if asleep => Pose::Sleep,
            Pose::Idle if self.music.is_some() => Pose::Music,
            Pose::Working if t < self.oops_until => Pose::Oops,
            Pose::Working if self.work_music => Pose::Music,
            Pose::Working => match self.work {
                log::Work::Plain => Pose::Working,
                log::Work::Reading => Pose::Reading,
                log::Work::Editing => Pose::Editing,
                log::Work::Testing => Pose::Testing,
                log::Work::Shipping => Pose::Shipping,
                log::Work::Web => Pose::Web,
            },
            p => p,
        };
        if t < self.dazed_until && matches!(self.pose, Pose::Idle | Pose::Working) {
            pose = Pose::Dazed;
        }
        // The game takes her bubble space while open, except when Claude needs your OK.
        let game_shown = self.playing() && self.pose != Pose::Waiting;
        if game_shown {
            pose = match &self.game {
                Some(g) if g.won() => Pose::Pouty,
                Some(g) if g.lost() => Pose::Smug,
                _ => Pose::Excited,
            };
        }
        if self.pose == Pose::Idle && t - self.finished_at < FINISHED_FLASH && !game_shown {
            pose = Pose::Done;
        }
        if cheering && !game_shown {
            pose = Pose::Cheer;
        }
        if poke_t.is_some() {
            pose = Pose::Poke;
        }
        let mut tour_line = None; // drawn over her usual bubble, which stays for afterwards
        if let Some((start, said)) = self.tour {
            let step = ((t - start) / TOUR_STEP) as usize;
            let tour = i18n::lines().tour;
            if let Some((tour_pose, line)) = tour.get(step) {
                pose = *tour_pose;
                if step != said {
                    self.tour = Some((start, step));
                    voice::say(line, voice::Mode::Tour);
                    if step == 3 || step == tour.len() - 1 {
                        self.sparkle_at = t;
                    }
                }
                tour_line = Some(Bubble { short: line.to_string(), long: None, expanded: false, hide_at: f64::MAX, is_label: false });
            } else {
                self.tour = None;
            }
        }
        if t < self.coffee_until {
            pose = Pose::Coffee; // whatever she was doing, she stops for a sip
        }
        if t > self.blink_at + 0.13 {
            self.blink_at = t + 3.0 + 3.0 * self.rand01() as f64;
        }
        let blinking = t > self.blink_at;
        // Long idle: she puts on music. Music she started herself stops when work starts.
        if self.pose != Pose::Idle {
            self.idle_since = t;
            if self.music == Some(true) {
                self.set_music(false, t);
            }
        } else if self.music.is_none() && t - self.idle_since > MUSIC_AFTER {
            self.set_music(true, t);
            self.music = Some(true);
        }
        // Idle with nothing on screen for a while: sparkle and offer to help. Not while music plays: that's chill time.
        if self.pose != Pose::Idle || cheering || t < self.coffee_until || self.bubble.is_some() || poke_t.is_some() || self.music.is_some() || self.tour.is_some() || self.playing() || asleep {
            self.nudge_at = t + IDLE_NUDGE;
        } else if t > self.nudge_at {
            let line = self.pick(i18n::lines().idle);
            self.say(line.into(), None, t - 3.0);
            self.sparkle_at = t;
        }

        // eframe restores the last position, which can be off screen (monitor unplugged, resolution changed).
        if !self.placed && t > 0.3 {
            self.placed = true;
            // Match the window to the restored zoom; the saved window size can be from another zoom.
            let size = window_size();
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            // macOS and Linux have no work-area call here: the monitor size stands in for it.
            let work = os::work_area(self.hwnd)
                .map(|w| w / ctx.zoom_factor())
                .or_else(|| ctx.input(|i| i.viewport().monitor_size).map(|m| Rect::from_min_size(Pos2::ZERO, m)));
            if let (Some(outer), Some(work)) = (ctx.input(|i| i.viewport().outer_rect), work) {
                // First run off Windows (no saved spot, no taskbar corner at launch): bottom-right, clear of a dock or panel.
                let start = if self.first_run && !cfg!(windows) { pos2(work.right() - size.x + swing_room() - 10.0, work.bottom() - size.y - 70.0) } else { outer.min };
                let x = start.x.min(work.right() - size.x + swing_room()).max(work.left() - swing_room());
                let y = start.y.min(work.bottom() - size.y).max(work.top());
                if (x, y) != (outer.min.x, outer.min.y) {
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
                }
            }
        }

        let area = ui.max_rect();
        let feet = pos2(area.center().x, area.bottom() - MARGIN / 2.0);
        let aspect = self.sprites[0].open.aspect_ratio(); // poses share idle's canvas
        let body = Rect::from_min_max(pos2(feet.x - HEIGHT * aspect / 2.0, feet.y - HEIGHT), pos2(feet.x + HEIGHT * aspect / 2.0, feet.y));

        // Clicks pass through everything except her and her bubbles. The cursor is read from the OS
        // because a pass-through window gets no mouse events to tell us when to switch back.
        // Where that isn't possible (a Wayland session), her whole window stays clickable.
        let native_ppp = ctx.native_pixels_per_point().unwrap_or(1.0);
        let want = if !os::click_through() {
            Some(false)
        } else if let (Some((cx, cy)), Some(inner)) = (os::cursor(native_ppp), ctx.input(|i| i.viewport().inner_rect)) {
            let ppp = ctx.pixels_per_point();
            let local = pos2(cx / ppp, cy / ppp) - inner.min.to_vec2();
            let dragging = ctx.input(|i| i.pointer.any_down()) && !self.passthrough;
            Some(!dragging && !self.hit(local, body, pose))
        } else {
            None
        };
        if let Some(pass) = want
            && pass != self.passthrough
        {
            self.passthrough = pass;
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(pass));
        }

        let resp = ui.interact(body, ui.scope_id().with("her"), Sense::click_and_drag());
        // While held, we move the window ourselves so her hold point stays on the cursor, and swing her from
        // the cursor's movement. Where the OS cursor can't be read (Wayland), the OS drags her instead, without swinging.
        let cursor = os::cursor(native_ppp).map(|(x, y)| pos2(x, y) / ctx.pixels_per_point());
        if resp.drag_started() {
            self.held = true;
            self.swing = Vec2::ZERO;
            self.hold = cursor.map(|c| (c, 0.0, t));
            if self.hold.is_none() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
        } else if self.held && !os::mouse_down().unwrap_or_else(|| ctx.input(|i| i.pointer.any_down())) {
            self.held = false;
            self.hold = None;
            self.swing = Vec2::ZERO;
        }
        if let (Some((last, last_vx, last_t)), Some(c), Some(inner)) = (self.hold, cursor, ctx.input(|i| i.viewport().inner_rect)) {
            let dt = ((t - last_t) as f32).clamp(0.001, 0.05);
            let vx = (c.x - last.x) / dt;
            self.swing = anim::swing_step(self.swing, (vx - last_vx) / HEIGHT, dt);
            self.hold = Some((c, vx, t));
            self.dizzy = (self.dizzy + (c - last).length() / HEIGHT - DIZZY_WEARS_OFF * dt).max(0.0);
            if self.dizzy > DIZZY_AT {
                self.dizzy = 0.0;
                if t > self.dizzy_until {
                    let line = self.pick(i18n::lines().dizzy);
                    self.say(line.into(), None, t - 6.0);
                }
                self.dizzy_until = t + DIZZY_FOR; // more shaking keeps her dizzy
            }
            let hold_point = pos2(body.left() + body.width() * anim::COLLAR.0, body.top() + body.height() * anim::COLLAR.1);
            let shift = (c - inner.min.to_vec2()) - hold_point;
            if shift.length() > 0.5 {
                let outer = ctx.input(|i| i.viewport().outer_rect).unwrap_or(inner);
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(outer.min + shift));
            }
        }
        if self.held {
            pose = if t < self.dizzy_until { Pose::Dizzy } else { Pose::Held };
            ctx.request_repaint();
        } else {
            self.dizzy = 0.0;
        }
        if resp.clicked() {
            self.poke_at = Some(t);
            if self.bubble.as_ref().is_none_or(|b| b.is_label) {
                let line = self.pick(i18n::lines().poke);
                self.say(line.into(), None, t - 6.0);
            }
        }
        if resp.double_clicked() && self.terminal != 0 {
            os::focus(self.terminal);
        }
        self.hit_rects.clear();
        if resp.drag_started() {
            self.popup = None;
        }
        if resp.secondary_clicked() {
            self.popup = if self.popup.is_some() { None } else { self.open_popup(body, t) };
        }
        self.show_popup(&ctx, t);
        let text = i18n::ui();

        let painter = ui.painter();
        if pose != self.shown {
            (self.prev_shown, self.shown, self.shown_at) = (self.shown, pose, t);
        }
        let since = (t - self.shown_at) as f32;
        let fade = (since / POSE_FADE as f32).min(1.0);
        // From here on, drawing: an f32 animation clock wrapped every hour (each pose's motion repeats evenly within it); timers use `now`.
        let now = t;
        let t = (now % 3600.0) as f32;
        let tex = |p: Pose| {
            let s = &self.sprites[p as usize];
            if blinking { s.blink.as_ref().unwrap_or(&s.open) } else { &s.open }
        };
        if fade < 1.0 {
            // The old pose fades out under the new one; squaring keeps her from looking see-through midway.
            draw_sprite(painter, tex(self.prev_shown), feet, HEIGHT, t, self.prev_shown, 9.0, self.swing, 9.0, 1.0 - fade * fade);
        }
        draw_sprite(painter, tex(pose), feet, HEIGHT, t, pose, poke_t.map_or(9.0, |p| p as f32), self.swing, since, fade);
        if pose == Pose::Coffee {
            // Steam curls up from the mug's rim (coffee.png has none painted), following her head tilt.
            let rim = feet + anim::warp(0.535, 0.286, t, pose, 9.0, aspect, Vec2::ZERO) * HEIGHT;
            for (i, phase) in [0.0, 2.6].into_iter().enumerate() {
                let base = rim + vec2(i as f32 * 5.0, 0.0);
                let pulse = 0.6 + 0.4 * (t * 1.3 + phase).sin();
                let at = |f: f32| base + vec2((f * 5.0 - t * 3.0 + phase).sin() * 4.0 * f + f * 7.0, -f * 30.0);
                for k in 0..12 {
                    let (f0, f1) = (k as f32 / 12.0, (k + 1) as f32 / 12.0);
                    let alpha = pulse * (f0 * 4.0).min(1.0) * (1.0 - f0) * 0.7;
                    painter.line_segment([at(f0), at(f1)], Stroke::new(3.0 - 1.5 * f0, Color32::WHITE.gamma_multiply(alpha)));
                }
            }
        }
        if now < self.coffee_until {
            // Hearts float up around her, each on its own loop, and fade out as the coffee runs out.
            let fade = ((self.coffee_until - now) / 0.8).min(1.0) as f32;
            for (i, fx) in [0.10, 0.88, 0.22, 0.78, 0.5].into_iter().enumerate() {
                let p = (t / 2.2 + i as f32 * 0.37).fract();
                let x = body.left() + body.width() * fx + (p * 6.0 + i as f32).sin() * 6.0;
                let y = body.top() + body.height() * (0.55 - 0.5 * p);
                heart(painter, pos2(x, y), 5.0 + 3.0 * (i % 2) as f32, fade * (1.0 - p) * (p * 6.0).min(1.0));
            }
            ctx.request_repaint();
        }
        if now < self.dizzy_until || pose == Pose::Dizzy {
            // Stars circle her head, following her swing; the near side of the loop is bigger.
            let top = if matches!(pose, Pose::Held | Pose::Dizzy) { (0.40, 0.07) } else { (0.5, 0.06) };
            let head = feet + anim::warp(top.0, top.1, t, pose, 9.0, aspect, self.swing) * HEIGHT;
            let fade = if pose == Pose::Dizzy { 1.0 } else { ((self.dizzy_until - now) / 0.5).min(1.0) as f32 }; // the tour shows her dizzy
            for i in 0..3 {
                let a = t * 4.0 + i as f32 * std::f32::consts::TAU / 3.0;
                sparkle(painter, head + vec2(a.cos() * 42.0, a.sin() * 9.0), 6.0 + 2.5 * a.sin(), fade);
            }
            ctx.request_repaint();
        }
        if pose == Pose::Sleep {
            // Three Zs drift up beside her head (sleep.png's head sits around the middle of the canvas), growing as they rise.
            for i in 0..3 {
                let p = (t / 4.5 + i as f32 / 3.0).fract();
                let at = pos2(body.left() + body.width() * (0.66 + 0.10 * p) + (p * 5.0).sin() * 4.0, body.top() + body.height() * (0.40 - 0.22 * p));
                let alpha = (1.0 - p) * (p * 5.0).min(1.0);
                painter.text(at, egui::Align2::CENTER_CENTER, "Z", FontId::proportional(12.0 + 10.0 * p), CORAL.gamma_multiply(alpha));
            }
        }
        if pose == Pose::Waiting {
            let bob = (t * 6.0).sin().abs() * 6.0;
            let at = pos2(body.right() - 38.0, body.top() + 30.0 - bob);
            painter.text(at, egui::Align2::CENTER_CENTER, "!", FontId::proportional(34.0), CORAL);
        }
        let player = self.music.and_then(|_| music::status());
        if pose == Pose::Music && !player.as_ref().is_some_and(|p| p.paused) {
            // Notes drift up beside her and fade.
            for i in 0..3 {
                let p = (t / 3.0 + i as f32 / 3.0).fract();
                let x = body.right() - 30.0 + (p * 7.0 + i as f32).sin() * 8.0 - i as f32 * 10.0;
                let y = body.top() + body.height() * 0.45 - p * 70.0;
                music_note(painter, pos2(x, y), (1.0 - p) * (p * 5.0).min(1.0));
            }
        }
        // Positions are fractions of her canvas: beside her head, clear of her hair.
        let at = |fx: f32, fy: f32| pos2(body.left() + body.width() * fx, body.top() + body.height() * fy);
        if voice::ready() {
            // Her voice is charged: sound waves pulse out from both sides of her head until she speaks.
            sound_waves(painter, at(0.86, 0.24), 1.0, t);
            sound_waves(painter, at(0.14, 0.24), -1.0, t);
        }
        if pose == Pose::Working {
            // Two sweat drops, half a cycle apart, each sliding down and fading.
            for (i, (fx, fy)) in [(0.84, 0.13), (0.15, 0.20)].into_iter().enumerate() {
                let p = (t / 1.4 + i as f32 * 0.5).fract();
                sweat_drop(painter, at(fx, fy) + vec2(0.0, p * 14.0), 4.5, (1.0 - p).min(0.3) / 0.3);
            }
        }
        let since = (now - self.sparkle_at) as f32;
        if since < 3.0 {
            // Stars pop in one by one and twinkle, then fade out together.
            let fade = (3.0 - since).min(0.6) / 0.6;
            for (i, (fx, fy, r)) in [(0.12, 0.08, 9.0), (0.90, 0.05, 11.0), (0.06, 0.40, 7.0), (0.95, 0.33, 8.0), (0.50, -0.03, 6.0), (0.86, 0.55, 6.0)].into_iter().enumerate() {
                let local = since - i as f32 * 0.12;
                if local > 0.0 {
                    let twinkle = 0.6 + 0.4 * (local * 9.0 + i as f32).sin();
                    sparkle(painter, at(fx, fy), r * twinkle * (local * 4.0).min(1.0), fade);
                }
            }
        }

        // Minis line up beside her feet, alternating left and right.
        let mini_w = HEIGHT * MINI * aspect;
        for (i, m) in self.minis.iter().enumerate() {
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            let dist = HEIGHT * aspect / 2.0 + mini_w / 2.0 - 12.0 + (i / 2) as f32 * mini_w * 0.6;
            let mfeet = feet + vec2(side * dist, 0.0);
            let (mpose, text) = if m.done_at.is_some() { (Pose::Done, text.done) } else { (Pose::Working, m.label.as_str()) };
            let mt = t + i as f32 * 0.7; // out of step with each other
            draw_sprite(painter, &self.sprites[mpose as usize].open, mfeet, HEIGHT * MINI, mt, mpose, 9.0, Vec2::ZERO, 9.0, 1.0);
            draw_box(painter, text, FontId::proportional(10.0), INK, 90.0, mfeet - vec2(0.0, HEIGHT * MINI + 2.0), false);
        }

        // Media player above her head while music is on; her speech bubble sits above it.
        let mut anchor = pos2(area.center().x, (feet.y - HEIGHT + 4.0).max(area.top() + 2.0));
        let mut list_shown = false;
        if let Some(st) = player.filter(|_| !game_shown) {
            let w = WIDTH - 40.0;
            let card = Rect::from_min_size(anchor - vec2(w / 2.0, 62.0), vec2(w, 58.0));
            painter.rect(card, 10.0, CREAM, Stroke::new(2.0, CORAL), egui::StrokeKind::Inside);
            self.hit_rects.push(card);
            let small = FontId::proportional(12.5);
            painter.text(card.left_top() + vec2(10.0, 6.0), egui::Align2::LEFT_TOP, clip(&st.name, 30), small.clone(), INK);
            let time = format!("{} / {}", mmss(st.pos), mmss(st.len));
            painter.text(card.right_top() + vec2(-10.0, 6.0), egui::Align2::RIGHT_TOP, time, small, SOFT_INK);
            // Progress bar: click to jump.
            let bar = Rect::from_min_size(card.left_top() + vec2(10.0, 27.0), vec2(w - 20.0, 4.0));
            let done = if st.len > 0.0 { (st.pos / st.len).clamp(0.0, 1.0) } else { 0.0 };
            painter.rect_filled(bar, 2.0, CORAL.gamma_multiply(0.25));
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * done, bar.height())), 2.0, CORAL);
            painter.circle_filled(pos2(bar.left() + bar.width() * done, bar.center().y), 4.0, CORAL);
            let seek = ui.interact(bar.expand2(vec2(0.0, 6.0)), ui.scope_id().with("seek"), Sense::click());
            if seek.clicked() && let Some(p) = seek.interact_pointer_pos() {
                music::seek((p.x - bar.left()) / bar.width() * st.len);
                self.music = Some(false); // you're choosing now: don't stop it when work starts
            }
            // Mode on the left (Loop / Loop 1 / Shuffle), then the transport buttons.
            let mode_rect = Rect::from_min_size(pos2(card.left() + 8.0, card.top() + 34.0), vec2(56.0, 20.0));
            let mode_resp = ui.interact(mode_rect, ui.scope_id().with("mode"), Sense::click());
            painter.rect(mode_rect, 6.0, CORAL.gamma_multiply(if mode_resp.hovered() { 0.3 } else { 0.15 }), Stroke::new(1.0, CORAL), egui::StrokeKind::Inside);
            painter.text(mode_rect.center(), egui::Align2::CENTER_CENTER, i18n::mode_label(self.music_mode), FontId::proportional(11.5), INK);
            if mode_resp.clicked() {
                self.music_mode = self.music_mode.next(); // our copy: the player's status lags a moment behind quick clicks
                music::set_mode(self.music_mode);
            }
            let icons = [Icon::Prev, if st.paused { Icon::Play } else { Icon::Pause }, Icon::Next, Icon::List, Icon::Stop];
            for (i, icon) in icons.into_iter().enumerate() {
                let c = pos2(card.center().x + 22.0 + (i as f32 - 2.0) * 36.0, card.top() + 44.0);
                let r = ui.interact(Rect::from_center_size(c, vec2(30.0, 20.0)), ui.scope_id().with(("player", i)), Sense::click());
                if r.hovered() || (icon == Icon::List && self.playlist_open) {
                    painter.rect_filled(r.rect, 6.0, CORAL.gamma_multiply(0.18));
                }
                draw_icon(painter, icon, c, INK);
                if r.clicked() {
                    match icon {
                        Icon::Prev => music::prev(),
                        Icon::Play | Icon::Pause => music::toggle_pause(),
                        Icon::Next => music::next(),
                        Icon::List => {
                            self.playlist_open = !self.playlist_open;
                            (self.view_list, self.list_scroll) = (st.playlist, 0);
                            music::rescan(); // pick up songs added since
                        }
                        Icon::Stop => self.set_music(false, now),
                        Icon::Back => {} // only used in the song list header
                    }
                    if icon != Icon::Stop {
                        self.music = Some(false);
                    }
                }
            }
            anchor.y = card.top() - 4.0;
            if self.playlist_open {
                // The song list, above the player in place of her bubble: arrows switch playlists, the wheel scrolls.
                let library = music::library();
                let (row, rows) = (19.0, 7usize);
                let h = 24.0 + rows as f32 * row + 12.0;
                let list = Rect::from_min_size(pos2(card.left(), anchor.y - h), vec2(w, h));
                painter.rect(list, 10.0, CREAM, Stroke::new(2.0, CORAL), egui::StrokeKind::Inside);
                self.hit_rects.push(list);
                if library.is_empty() {
                    painter.text(list.center(), egui::Align2::CENTER_CENTER, text.loading, FontId::proportional(13.0), SOFT_INK);
                } else {
                    self.view_list = self.view_list.min(library.len() - 1);
                    let (name, songs) = &library[self.view_list];
                    // Header: < playlist name >, and "Add songs" to open the music folder.
                    let head_y = list.top() + 14.0;
                    for (i, dir) in [-1i32, 1].into_iter().enumerate() {
                        let c = pos2(list.left() + 18.0 + i as f32 * 168.0, head_y);
                        let r = ui.interact(Rect::from_center_size(c, vec2(22.0, 20.0)), ui.scope_id().with(("pl-arrow", i)), Sense::click());
                        if r.hovered() {
                            painter.rect_filled(r.rect, 5.0, CORAL.gamma_multiply(0.18));
                        }
                        draw_icon(painter, if dir < 0 { Icon::Back } else { Icon::Play }, c, if library.len() > 1 { INK } else { INK.gamma_multiply(0.3) });
                        if r.clicked() {
                            self.view_list = (self.view_list as i32 + dir).rem_euclid(library.len() as i32) as usize;
                            self.list_scroll = 0;
                        }
                    }
                    painter.text(pos2(list.left() + 102.0, head_y), egui::Align2::CENTER_CENTER, clip(i18n::playlist_name(name), 20), FontId::proportional(12.5), INK);
                    let add = Rect::from_min_size(pos2(list.right() - 82.0, head_y - 10.0), vec2(74.0, 20.0));
                    let add_resp = ui.interact(add, ui.scope_id().with("add-songs"), Sense::click());
                    painter.rect(add, 6.0, CORAL.gamma_multiply(if add_resp.hovered() { 0.3 } else { 0.15 }), Stroke::new(1.0, CORAL), egui::StrokeKind::Inside);
                    painter.text(add.center(), egui::Align2::CENTER_CENTER, text.add_songs, FontId::proportional(11.5), INK);
                    if add_resp.clicked() {
                        open_music_folder();
                    }
                    // Rows.
                    let max_scroll = songs.len().saturating_sub(rows);
                    if ui.rect_contains_pointer(list) {
                        // Raw wheel events: one notch moves one row.
                        let wheel: f32 = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::MouseWheel { delta, .. } = e { Some(delta.y) } else { None }).sum());
                        if wheel < 0.0 {
                            self.list_scroll += 1;
                        } else if wheel > 0.0 {
                            self.list_scroll = self.list_scroll.saturating_sub(1);
                        }
                    }
                    self.list_scroll = self.list_scroll.min(max_scroll);
                    for (k, i) in (self.list_scroll..songs.len().min(self.list_scroll + rows)).enumerate() {
                        let r = Rect::from_min_size(list.min + vec2(6.0, 26.0 + k as f32 * row), vec2(w - 12.0, row));
                        let resp = ui.interact(r, ui.scope_id().with(("song", self.view_list, i)), Sense::click());
                        let playing = self.view_list == st.playlist && i == st.track;
                        if playing || resp.hovered() {
                            painter.rect_filled(r, 6.0, CORAL.gamma_multiply(if playing { 0.25 } else { 0.12 }));
                        }
                        let label = format!("{}.  {}", i + 1, clip(&songs[i], 32));
                        painter.text(r.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, label, FontId::proportional(13.0), INK);
                        if playing {
                            draw_icon(painter, if st.paused { Icon::Pause } else { Icon::Play }, r.right_center() - vec2(14.0, 0.0), CORAL);
                        }
                        if resp.clicked() {
                            music::play_track(self.view_list, i);
                            self.playlist_name = name.clone();
                            self.music = Some(false);
                            self.playlist_open = false;
                        }
                    }
                    if max_scroll > 0 {
                        // A thin scroll indicator on the right edge.
                        let track_h = rows as f32 * row;
                        let thumb_h = track_h * rows as f32 / songs.len() as f32;
                        let y = list.top() + 26.0 + (track_h - thumb_h) * self.list_scroll as f32 / max_scroll as f32;
                        painter.rect_filled(Rect::from_min_size(pos2(list.right() - 5.0, y), vec2(2.5, thumb_h)), 1.0, CORAL.gamma_multiply(0.5));
                    }
                }
                list_shown = true;
            }
        } else {
            self.playlist_open = false;
        }

        if game_shown && self.picking {
            self.game_picker(ui, painter, anchor);
            list_shown = true;
        } else if game_shown && self.sudoku.is_some() {
            self.sudoku_panel(ui, painter, anchor, now);
            list_shown = true;
        } else if game_shown {
            // Her line, the hint, the word, the A-Z grid, then hearts and buttons.
            let w = WIDTH - 40.0;
            let panel = Rect::from_min_size(anchor - vec2(w / 2.0, 222.0), vec2(w, 220.0));
            painter.rect(panel, 10.0, CREAM, Stroke::new(2.0, CORAL), egui::StrokeKind::Inside);
            self.hit_rects.push(panel);
            let (mut guess, mut again, mut close) = (None, false, false);
            let mut y = panel.top() + 8.0;
            let said = painter.layout(self.game_line.clone(), FontId::proportional(13.5), INK, w - 20.0);
            painter.galley(pos2(panel.left() + 10.0, y), said, INK);
            y += 38.0;
            if let Some(g) = &self.game {
                let note = if g.won() {
                    text.solved.to_string()
                } else if g.lost() {
                    text.word_was.replace("{w}", &g.word)
                } else {
                    format!("{}{}", text.hint, g.hint)
                };
                let galley = painter.layout(note, FontId::proportional(12.0), SOFT_INK, w - 20.0);
                painter.galley(pos2(panel.left() + 10.0, y), galley, SOFT_INK);
                y += 34.0;
                painter.text(pos2(panel.center().x, y + 10.0), egui::Align2::CENTER_CENTER, g.masked(), FontId::monospace(20.0), INK);
                y += 28.0;
                // A-Z in rows of 9: click a letter to guess it.
                let (cols, cell, gap) = (9, vec2(28.0, 22.0), 3.0);
                let left = panel.center().x - (cols as f32 * (cell.x + gap) - gap) / 2.0;
                for (i, c) in ('A'..='Z').enumerate() {
                    let r = Rect::from_min_size(pos2(left + (i % cols) as f32 * (cell.x + gap), y + (i / cols) as f32 * (cell.y + gap)), cell);
                    let used = g.guessed.contains(&c);
                    let resp = ui.interact(r, ui.scope_id().with(("letter", i)), Sense::click());
                    let (fill, ink) = match (used, g.word.contains(c)) {
                        (true, true) => (CORAL.gamma_multiply(0.5), INK),
                        (true, false) => (INK.gamma_multiply(0.06), INK.gamma_multiply(0.3)),
                        _ if resp.hovered() && !g.over() => (CORAL.gamma_multiply(0.3), INK),
                        _ => (CORAL.gamma_multiply(0.12), if g.over() { INK.gamma_multiply(0.4) } else { INK }),
                    };
                    painter.rect_filled(r, 5.0, fill);
                    painter.text(r.center(), egui::Align2::CENTER_CENTER, c, FontId::proportional(13.0), ink);
                    if resp.clicked() && !used && !g.over() {
                        guess = Some(c);
                    }
                }
                let lives = game::LIVES - g.misses().min(game::LIVES);
                for i in 0..game::LIVES {
                    let color = if i < lives { CORAL } else { CORAL.gamma_multiply(0.25) };
                    painter.text(pos2(panel.left() + 12.0 + i as f32 * 13.0, panel.bottom() - 16.0), egui::Align2::LEFT_CENTER, "♥", FontId::proportional(13.0), color);
                }
            }
            for (i, label) in [text.new_word, text.close].into_iter().enumerate() {
                let r = Rect::from_center_size(pos2(panel.right() - 50.0 - (1 - i) as f32 * 86.0, panel.bottom() - 16.0), vec2(80.0, 22.0));
                let resp = ui.interact(r, ui.scope_id().with(("game-btn", i)), Sense::click());
                painter.rect(r, 8.0, if resp.hovered() { CORAL.gamma_multiply(0.3) } else { CORAL.gamma_multiply(0.15) }, Stroke::new(1.0, CORAL), egui::StrokeKind::Inside);
                painter.text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(12.5), INK);
                if resp.clicked() {
                    (again, close) = (i == 0, i == 1);
                }
            }
            if let Some(c) = guess {
                let r = self.rand01();
                if let Some(g) = &mut self.game {
                    g.guess(c);
                    let lines: &[&str] = if g.won() {
                        i18n::lines().player_won
                    } else if g.lost() {
                        i18n::lines().player_lost
                    } else if !g.word.contains(c) && g.misses() == game::LIVES - 1 {
                        i18n::lines().last_heart
                    } else if g.word.contains(c) {
                        i18n::lines().hit
                    } else {
                        i18n::lines().miss
                    };
                    self.game_line = game::line(lines, r, &g.word);
                    voice::say(&self.game_line, voice::Mode::Chatter);
                }
            }
            if again {
                self.start_game();
            }
            if close {
                (self.game, self.game_loading) = (None, false);
            }
            list_shown = true;
        }

        let mut toggle = false;
        if !list_shown && let Some(b) = tour_line.as_ref().or(self.bubble.as_ref()) {
            let (text, font, color) = match (&b.long, b.expanded, b.is_label) {
                (_, _, true) => (b.short.clone(), FontId::proportional(12.5), Color32::from_rgb(0x8A, 0x5A, 0x4A)),
                (Some(long), true, _) => (long.clone(), FontId::proportional(14.0), INK),
                (Some(_), false, _) => (format!("{}\n{}", b.short, text.click_for_more), FontId::proportional(14.0), INK),
                (None, _, _) => (b.short.clone(), FontId::proportional(14.0), INK),
            };
            // shortcut: text taller than BUBBLE_ROOM clips at the top of the window
            let rect = draw_box(painter, &text, font, color, WIDTH - 40.0, anchor, true);
            self.hit_rects.push(rect);
            toggle = tour_line.is_none() && !b.is_label && b.long.is_some() && ui.interact(rect, ui.scope_id().with("bubble"), Sense::click()).clicked();
        }
        if toggle && let Some(b) = &mut self.bubble {
            b.expanded = !b.expanded;
            b.hide_at = now + 10.0 + b.long.as_deref().unwrap_or("").chars().count() as f64 / 12.0;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// How far the popup's entrance has got (0 to 1), eased out so it settles softly.
fn popup_in(since: f32) -> f32 {
    1.0 - (1.0 - (since / POPUP_IN as f32).clamp(0.0, 1.0)).powi(3)
}

/// A four-point star: two long strokes, two short diagonals and a bright center.
fn sparkle(painter: &Painter, c: Pos2, r: f32, alpha: f32) {
    let gold = Color32::from_rgb(0xFF, 0xC8, 0x4A).gamma_multiply(alpha);
    let thick = Stroke::new((r * 0.3).max(1.0), gold);
    painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], thick);
    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], thick);
    let d = r * 0.4;
    let thin = Stroke::new((r * 0.15).max(1.0), gold);
    painter.line_segment([c - vec2(d, d), c + vec2(d, d)], thin);
    painter.line_segment([c - vec2(d, -d), c + vec2(d, -d)], thin);
    painter.circle_filled(c, r * 0.22, Color32::WHITE.gamma_multiply(alpha));
}

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Back, // a left-pointing triangle
    Prev,
    Play,
    Pause,
    Next,
    List,
    Stop,
}

/// Player button icons as shapes: symbol fonts aren't guaranteed to have them.
fn draw_icon(painter: &Painter, icon: Icon, c: Pos2, color: Color32) {
    let tri = |dir: f32, at: Pos2| egui::Shape::convex_polygon(vec![at + vec2(-4.0 * dir, -5.0), at + vec2(5.0 * dir, 0.0), at + vec2(-4.0 * dir, 5.0)], color, Stroke::NONE);
    let bar = |x: f32| Rect::from_center_size(c + vec2(x, 0.0), vec2(2.5, 10.0));
    match icon {
        Icon::Play => {
            painter.add(tri(1.0, c));
        }
        Icon::Back => {
            painter.add(tri(-1.0, c));
        }
        Icon::Pause => {
            painter.rect_filled(bar(-3.0), 1.0, color);
            painter.rect_filled(bar(3.0), 1.0, color);
        }
        Icon::Prev => {
            painter.add(tri(-1.0, c + vec2(1.5, 0.0)));
            painter.rect_filled(bar(-5.0), 1.0, color);
        }
        Icon::Next => {
            painter.add(tri(1.0, c - vec2(1.5, 0.0)));
            painter.rect_filled(bar(5.0), 1.0, color);
        }
        Icon::List => {
            for y in [-4.0, 0.0, 4.0] {
                painter.line_segment([c + vec2(-6.0, y), c + vec2(6.0, y)], Stroke::new(1.8, color));
            }
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.5, color);
        }
    }
}

/// Shortens long names for the player and song list.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() > max { text.chars().take(max - 1).collect::<String>() + "…" } else { text.to_string() }
}

/// Opens the music folder in Explorer, creating it first. Each folder inside is a playlist.
fn open_music_folder() {
    std::thread::spawn(|| {
        let dir = music::music_dir();
        let _ = std::fs::create_dir_all(&dir);
        let opener = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(&dir).spawn();
    });
}

fn mmss(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// An eighth note: a tilted head, a stem and a flag.
/// A pink heart of half-width `r` centred at `c`: two circles over a point.
fn heart(painter: &Painter, c: Pos2, r: f32, alpha: f32) {
    let pink = Color32::from_rgb(0xFF, 0x6F, 0x91).gamma_multiply(alpha);
    painter.circle_filled(c + vec2(-r * 0.5, -r * 0.2), r * 0.55, pink);
    painter.circle_filled(c + vec2(r * 0.5, -r * 0.2), r * 0.55, pink);
    painter.add(egui::Shape::convex_polygon(vec![c + vec2(-r * 1.02, 0.0), c + vec2(r * 1.02, 0.0), c + vec2(0.0, r * 1.1)], pink, Stroke::NONE));
}

fn music_note(painter: &Painter, at: Pos2, alpha: f32) {
    let c = CORAL.gamma_multiply(alpha);
    painter.add(egui::Shape::ellipse_filled(at, vec2(4.5, 3.5), c));
    let top = at + vec2(4.0, -16.0);
    painter.line_segment([at + vec2(4.0, -1.0), top], Stroke::new(1.6, c));
    painter.line_segment([top, top + vec2(6.0, 6.0)], Stroke::new(2.0, c));
}

/// An anime sweat drop: a circle of radius `r` at `c` with a point on top.
/// Three arcs opening sideways (`dir` 1 right, -1 left) that light up outward in turn, like a sound coming.
fn sound_waves(painter: &Painter, c: Pos2, dir: f32, t: f32) {
    for i in 0..3 {
        let r = 5.0 + i as f32 * 5.0;
        let glow = 0.5 + 0.5 * (t * 4.0 - i as f32 * 0.9).sin();
        let points: Vec<Pos2> = (0..=8).map(|k| {
            let a = -0.7 + 1.4 * k as f32 / 8.0;
            c + vec2(dir * r * a.cos(), r * a.sin())
        }).collect();
        painter.add(egui::Shape::line(points, Stroke::new(2.2, CORAL.gamma_multiply(0.25 + 0.75 * glow))));
    }
}

fn sweat_drop(painter: &Painter, c: Pos2, r: f32, alpha: f32) {
    // The tip is 2r above the center, so its tangents touch the circle at -30° and 210°.
    let mut pts = vec![c - vec2(0.0, 2.0 * r)];
    pts.extend((0..=12).map(|i| {
        let a = (-30.0 + 20.0 * i as f32).to_radians();
        c + vec2(a.cos(), a.sin()) * r
    }));
    let fill = Color32::from_rgb(0xA8, 0xD8, 0xFF).gamma_multiply(alpha);
    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.2, Color32::from_rgb(0x4A, 0x8F, 0xD0).gamma_multiply(alpha))));
    painter.circle_filled(c + vec2(-r * 0.35, -r * 0.3), r * 0.25, Color32::WHITE.gamma_multiply(alpha));
}

/// macOS: an accessory app, so she has no Dock icon and doesn't take focus. Linux: the X11 backend (XWayland on a
/// Wayland desktop), because Wayland lets no window keep itself on top or pick its own position.
fn platform_event_loop() -> Option<eframe::EventLoopBuilderHook> {
    #[cfg(target_os = "macos")]
    return Some(Box::new(|b| {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        b.with_activation_policy(ActivationPolicy::Accessory);
    }));
    #[cfg(all(unix, not(target_os = "macos")))]
    return Some(Box::new(|b| {
        use winit::platform::x11::EventLoopBuilderExtX11;
        b.with_x11();
    }));
    #[allow(unreachable_code)]
    None
}

/// Window size in points; egui zoom turns it into her on-screen size.
fn window_size() -> Vec2 {
    vec2(WIDTH + 2.0 * swing_room(), HEIGHT + 2.0 * MARGIN + BUBBLE_ROOM)
}

/// Room for her swing on each side of her window. It may hang off the screen edge, since it's see-through.
/// Always there, because resizing the window when you pick her up shows a frame of her in the wrong place.
/// None where clicks can't pass through (Wayland): it would block the desktop, and she doesn't swing there.
fn swing_room() -> f32 {
    if os::click_through() { SWING_ROOM } else { 0.0 }
}

fn main() -> eframe::Result {
    match std::env::args().nth(1).as_deref() {
        Some("hook") => return Ok(cli::hook()),
        Some("install") => return Ok(cli::report(cli::install())),
        Some("uninstall") => return Ok(cli::report(cli::uninstall())),
        // For the developer: make the bundled voice into assets/voice/ (key from ELEVENLABS_API_KEY), then rebuild.
        Some("voices") => {
            let id = std::env::args().nth(2).unwrap_or_default();
            let key = std::env::var("ELEVENLABS_API_KEY").unwrap_or_default();
            let dir = std::env::current_dir().unwrap_or_default().join("assets").join("voice");
            return Ok(cli::report(voice::generate(&key, &id, &dir).map(|n| format!("Made {n} new lines in {}. Rebuild to bundle them.", dir.display()))));
        }
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
    if let Some((right, bottom)) = os::work_area_corner() {
        viewport = viewport.with_position(pos2(right - size.x + swing_room() - 10.0, bottom - size.y));
    }
    let opts = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow, // wgpu on Windows gives an opaque window
        persist_window: true,
        event_loop_builder: platform_event_loop(),
        ..Default::default()
    };
    eframe::run_native("Claudesona", opts, Box::new(|cc| Ok(Box::new(App::new(cc, listener)))))
}
