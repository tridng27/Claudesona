// The session panel (SPEC-v2.md, "Session panel"): a window beside her with one card per turn, its steps
// as rows that expand to the details, a pending permission pinned on top, and the Memory and Services tabs.
use crate::log::Work;
use crate::stream::Event;
use crate::{CORAL, CREAM, INK, SOFT_INK, i18n, memgraph, services};
use eframe::egui::{self, Color32, FontId, Key, Rect, RichText, Stroke, TextFormat, ViewportCommand, ViewportId, pos2, text::LayoutJob, vec2};
use serde_json::Value;
use std::collections::HashSet;

const PANEL_W: f32 = 400.0;
const PANEL_H: f32 = 540.0;
const SHOW_MAX: usize = 8000; // characters of a diff or file drawn in an expanded row
const RED: Color32 = Color32::from_rgb(0xB0, 0x3A, 0x2E);
const GREEN: Color32 = Color32::from_rgb(0x3E, 0x7D, 0x3A);

#[derive(Clone, Debug, PartialEq)]
pub struct Permission {
    pub request_id: String,
    pub tool: String,
    pub input: Value,
    pub tool_use_id: String,
}

#[derive(Debug)]
pub struct Step {
    pub id: String,
    pub name: String,
    pub input: Value,
    pub work: Work,
    pub running: String, // stream's activity label, shown until it's done
    pub done: bool,
    pub is_error: bool,
    pub refused: bool,
    pub output: String, // the tool result, short (stream.rs keeps 80 characters)
}

#[derive(Default, Debug)]
pub struct Turn {
    pub prompt: String,
    pub heard: Option<String>, // what you said, before it was cleaned into `prompt`
    pub summary: Option<String>,
    pub reply: String,
    live: String, // the reply streaming in
    pub steps: Vec<Step>,
    pub done: bool,
    pub interrupted: bool,
}

#[derive(Default, Debug)]
pub struct Timeline {
    pub turns: Vec<Turn>,
    pub pending: Vec<Permission>, // the first one is pinned on top
    pub session_id: String,
    pub model: String,
    pub mcp_servers: Vec<(String, String)>,
}

impl Timeline {
    pub fn user_prompt(&mut self, text: &str, cleaned_from: Option<&str>) {
        self.turns.push(Turn { prompt: text.into(), heard: cleaned_from.map(Into::into), ..Default::default() });
    }

    /// Her short version of a turn's reply (index into `turns`).
    pub fn set_summary(&mut self, turn: usize, text: &str) {
        if let Some(t) = self.turns.get_mut(turn) {
            t.summary = Some(text.into());
        }
    }

    pub fn push(&mut self, ev: &Event) {
        match ev {
            Event::Init { session_id, model, mcp_servers, .. } => {
                (self.session_id, self.model, self.mcp_servers) = (session_id.clone(), model.clone(), mcp_servers.clone());
            }
            Event::TextDelta(t) => self.open().live.push_str(t),
            Event::Text(t) => {
                let turn = self.open();
                (turn.reply, turn.live) = (t.clone(), String::new());
            }
            Event::Tool { id, name, input, label, work } => self.open().steps.push(Step {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                work: *work,
                running: label.clone(),
                done: false,
                is_error: false,
                refused: false,
                output: String::new(),
            }),
            Event::ToolDone { id, is_error, refused, summary } => {
                if let Some(s) = self.step(id) {
                    (s.done, s.is_error, s.refused, s.output) = (true, *is_error, *refused, summary.clone());
                }
                self.pending.retain(|p| p.tool_use_id != *id);
            }
            // A background helper ends after its turn did, so look through every turn.
            Event::AgentDone(id) => {
                if let Some(s) = self.step(id) {
                    s.done = true;
                }
            }
            Event::Permission { request_id, tool, input, tool_use_id } => {
                self.pending.push(Permission { request_id: request_id.clone(), tool: tool.clone(), input: input.clone(), tool_use_id: tool_use_id.clone() });
            }
            Event::Result { text, interrupted, .. } => {
                let turn = self.open();
                if !text.is_empty() {
                    turn.reply = text.clone();
                }
                (turn.live, turn.done, turn.interrupted) = (String::new(), true, *interrupted);
                self.pending.clear();
            }
            Event::Exit => {
                self.pending.clear();
                if let Some(t) = self.turns.last_mut() {
                    t.done = true;
                }
            }
            Event::AgentStart { .. } | Event::Compact | Event::Reset => {}
        }
    }

    /// The turn events go to: the last one, or a new one without a prompt (a helper woke Claude up).
    fn open(&mut self) -> &mut Turn {
        if self.turns.last().is_none_or(|t| t.done) {
            self.turns.push(Turn::default());
        }
        self.turns.last_mut().unwrap()
    }

    fn step(&mut self, id: &str) -> Option<&mut Step> {
        self.turns.iter_mut().rev().flat_map(|t| t.steps.iter_mut()).find(|s| s.id == id)
    }
}

/// Lines added and removed by an Edit, MultiEdit or Write, not counting lines both sides share at the ends.
pub fn changes(name: &str, input: &Value) -> (usize, usize) {
    let pair = |e: &Value| {
        let (old, new): (Vec<&str>, Vec<&str>) = (e["old_string"].as_str().unwrap_or("").lines().collect(), e["new_string"].as_str().unwrap_or("").lines().collect());
        let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let tail = old[head..].iter().rev().zip(new[head..].iter().rev()).take_while(|(a, b)| a == b).count();
        (new.len() - head - tail, old.len() - head - tail)
    };
    match name {
        "Edit" => pair(input),
        "MultiEdit" => input["edits"].as_array().into_iter().flatten().map(pair).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)),
        "Write" => (input["content"].as_str().unwrap_or("").lines().count(), 0),
        _ => (0, 0),
    }
}

fn short(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() > max || s.trim().contains('\n') { format!("{}…", line.chars().take(max).collect::<String>()) } else { line.into() }
}

/// A step's one-line row: what it did, in the past tense once it's done.
pub fn label(s: &Step) -> String {
    if !s.done {
        return s.running.clone();
    }
    let ui = i18n::ui();
    let arg = |k: &str| short(s.input[k].as_str().unwrap_or(""), 40);
    let file = s.input["file_path"].as_str().map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p));
    let base = match (s.name.as_str(), file) {
        ("Read", Some(f)) => ui.step_read.replace("{f}", f),
        ("Write", Some(f)) => format!("{} (+{})", ui.step_wrote.replace("{f}", f), changes("Write", &s.input).0),
        ("Edit" | "MultiEdit", Some(f)) => {
            let (add, del) = changes(&s.name, &s.input);
            format!("{} (+{add} −{del})", ui.step_edited.replace("{f}", f))
        }
        ("Bash" | "PowerShell", _) if s.work == Work::Testing => ui.step_tests.into(),
        ("Bash" | "PowerShell", _) => ui.step_ran.replace("{c}", &arg("command")),
        ("Grep" | "Glob", _) => ui.step_searched.replace("{q}", &arg("pattern")),
        ("WebSearch", _) => ui.step_web.replace("{q}", &arg("query")),
        ("WebFetch", _) => ui.step_web.replace("{q}", &arg("url")),
        ("Agent" | "Task", _) => ui.step_helper.replace("{d}", &arg("description")),
        (n, _) if n.starts_with("mcp__") => n["mcp__".len()..].replacen("__", ": ", 1),
        (n, _) => n.into(),
    };
    match (s.refused, s.is_error) {
        (true, _) => format!("{base}: {}", ui.step_denied),
        (_, true) => format!("{base}: {}", ui.step_failed),
        _ => base,
    }
}

fn icon(s: &Step) -> &'static str {
    match (s.name.as_str(), s.work) {
        ("Agent" | "Task", _) => "🐣",
        (n, _) if n.starts_with("mcp__") => "🔌",
        (_, Work::Reading) => "📖",
        (_, Work::Editing) => "✏",
        (_, Work::Testing) => "🧪",
        (_, Work::Shipping) => "🚀",
        (_, Work::Web) => "🌐",
        (_, Work::Plain | Work::Service) => "⚙",
    }
}

/// An expanded row: the diff, the file written, or the command and what came back. Selectable text.
fn details(s: &Step) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut add = |text: &str, colour: Color32| {
        let room = SHOW_MAX.saturating_sub(job.text.len());
        let text: String = if text.len() > room { format!("{}…\n", text.chars().take(room).collect::<String>()) } else { text.into() };
        job.append(&text, 0.0, TextFormat { font_id: FontId::monospace(11.0), color: colour, ..Default::default() });
    };
    let get = |k: &str| s.input[k].as_str().unwrap_or("");
    let lines = |text: &str, sign: &str| text.lines().map(|l| format!("{sign}{l}\n")).collect::<String>();
    if let Some(p) = s.input["file_path"].as_str() {
        add(&format!("{p}\n"), SOFT_INK);
    }
    match s.name.as_str() {
        "Edit" | "MultiEdit" => {
            let edits = s.input["edits"].as_array().cloned().unwrap_or_else(|| vec![s.input.clone()]);
            for e in &edits {
                add(&lines(e["old_string"].as_str().unwrap_or(""), "- "), RED);
                add(&lines(e["new_string"].as_str().unwrap_or(""), "+ "), GREEN);
            }
        }
        "Write" => add(get("content"), INK),
        "Bash" | "PowerShell" => add(&format!("$ {}\n", get("command")), INK),
        "Read" => {}
        "Grep" | "Glob" => add(&format!("{}\n", get("pattern")), INK),
        _ => add(&format!("{}\n", serde_json::to_string_pretty(&s.input).unwrap_or_default()), INK),
    }
    if !s.output.is_empty() {
        add(&format!("→ {}", s.output), if s.is_error { RED } else { SOFT_INK });
    }
    job
}

#[derive(Clone, Debug, PartialEq)]
pub enum PanelAction {
    Allow(Permission),
    Deny(Permission),
    Always(Permission), // allow, and answer this tool by herself for the rest of the session
    Close,
    ServicesChanged, // mcp.json was saved; it applies to the next session
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Session,
    Memory,
    Services,
}

pub struct PanelState {
    her: Rect,  // her body on screen when it opened, in points
    work: Rect, // the desktop area it must stay inside
    left: bool, // open to her left
    opened: Option<f64>,
    frames: u32,
    had_focus: bool,
    tab: Tab,
    open_rows: HashSet<String>, // step ids
    open_replies: HashSet<usize>,
    pub services: services::ServicesState,
}

impl PanelState {
    /// `her` and `work` as main.rs works them out for the popup (screen points).
    pub fn new(her: Rect, work: Rect) -> Self {
        let left = her.center().x > work.center().x;
        PanelState { her, work, left, opened: None, frames: 0, had_focus: false, tab: Tab::Session, open_rows: HashSet::new(), open_replies: HashSet::new(), services: services::ServicesState::load() }
    }
}

/// Draws the panel in its own window. Returns what you clicked; on Close, drop the PanelState.
pub fn show(ctx: &egui::Context, s: &mut PanelState, tl: &Timeline, graph: &mut memgraph::GraphState) -> Option<PanelAction> {
    let t = ctx.input(|i| i.time);
    let opened = *s.opened.get_or_insert(t);
    let size = vec2(PANEL_W, PANEL_H.min(s.work.height()));
    let x = if s.left { s.her.left() - size.x - 6.0 } else { s.her.right() + 6.0 };
    let x = x.min(s.work.right() - size.x).max(s.work.left());
    let k = crate::popup_in((t - opened) as f32);
    let x = x + (1.0 - k) * crate::POPUP_SLIDE * if s.left { 1.0 } else { -1.0 };
    let y = (s.her.bottom() - size.y).min(s.work.bottom() - size.y).max(s.work.top());
    let builder = egui::ViewportBuilder::default()
        .with_title("Claudesona")
        .with_position(pos2(x, y))
        .with_inner_size(size)
        .with_decorations(false)
        .with_transparent(true)
        .with_has_shadow(false)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false);
    ctx.show_viewport_immediate(ViewportId::from_hash_of("panel"), builder, |ui, _| panel_ui(ui, s, tl, graph, k))
}

fn panel_ui(ui: &mut egui::Ui, s: &mut PanelState, tl: &Timeline, graph: &mut memgraph::GraphState, k: f32) -> Option<PanelAction> {
    let text = i18n::ui();
    let ctx = ui.ctx().clone();
    let (focused, esc, close_asked) = ui.input(|i| (i.viewport().focused, i.key_pressed(Key::Escape), i.viewport().close_requested()));
    if s.frames == 0 {
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }
    s.frames += 1;
    s.had_focus |= focused == Some(true);
    // Clicking anywhere else takes its focus away: that closes it, like the popup.
    let mut action = (esc || close_asked || (s.had_focus && focused == Some(false))).then_some(PanelAction::Close);
    *ui.visuals_mut() = egui::Visuals::light();
    ui.visuals_mut().override_text_color = Some(INK);
    ui.set_opacity(k);
    if k < 1.0 {
        ctx.request_repaint();
    }
    let frame = egui::Frame::new().fill(CREAM).stroke(Stroke::new(2.0, CORAL)).corner_radius(10).inner_margin(10);
    frame.show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.horizontal(|ui| {
            for (tab, name) in [(Tab::Session, text.tab_session), (Tab::Memory, text.tab_memory), (Tab::Services, text.tab_services)] {
                if ui.selectable_label(s.tab == tab, name).clicked() {
                    s.tab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").clicked() {
                    action = Some(PanelAction::Close);
                }
            });
        });
        ui.separator();
        match s.tab {
            Tab::Session => {
                if let Some(a) = session_ui(ui, s, tl) {
                    action = Some(a);
                }
            }
            Tab::Memory => memgraph::show(ui, graph),
            Tab::Services => {
                s.services.status.clone_from(&tl.mcp_servers);
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| services::ui(ui, &mut s.services));
                if std::mem::take(&mut s.services.changed) {
                    action = action.take().or(Some(PanelAction::ServicesChanged));
                }
            }
        }
    });
    action
}

fn session_ui(ui: &mut egui::Ui, s: &mut PanelState, tl: &Timeline) -> Option<PanelAction> {
    let text = i18n::ui();
    let mut action = None;
    if !tl.model.is_empty() {
        let mcp: Vec<String> = tl.mcp_servers.iter().map(|(n, st)| format!("{n} ({st})")).collect();
        let head = format!("{} · {}{}", tl.model, tl.session_id.get(..8).unwrap_or(&tl.session_id), if mcp.is_empty() { String::new() } else { format!(" · {}", mcp.join(", ")) });
        ui.label(RichText::new(head).small().color(SOFT_INK));
    }
    if let Some(p) = tl.pending.first() {
        egui::Frame::new().fill(Color32::WHITE).stroke(Stroke::new(1.5, CORAL)).corner_radius(8).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text.asks.replace("{tool}", &p.tool)).strong());
            let step = Step { id: p.tool_use_id.clone(), name: p.tool.clone(), input: p.input.clone(), work: crate::log::work_kind(&p.tool, &p.input), running: String::new(), done: true, is_error: false, refused: false, output: String::new() };
            egui::ScrollArea::vertical().id_salt("permission").max_height(140.0).show(ui, |ui| ui.add(egui::Label::new(details(&step)).selectable(true)));
            ui.horizontal(|ui| {
                if ui.button(text.allow).clicked() {
                    action = Some(PanelAction::Allow(p.clone()));
                }
                if ui.button(text.deny).clicked() {
                    action = Some(PanelAction::Deny(p.clone()));
                }
                if ui.button(text.always).clicked() {
                    action = Some(PanelAction::Always(p.clone()));
                }
            });
        });
    }
    if tl.turns.is_empty() {
        ui.label(RichText::new(text.no_turns).color(SOFT_INK));
    }
    egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
        for (i, turn) in tl.turns.iter().enumerate() {
            egui::Frame::new().fill(Color32::from_rgb(0xFF, 0xFA, 0xF3)).stroke(Stroke::new(1.0, CORAL.gamma_multiply(0.4))).corner_radius(8).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !turn.prompt.is_empty() {
                    ui.label(RichText::new(&turn.prompt).strong());
                }
                if let Some(h) = &turn.heard {
                    ui.label(RichText::new(format!("🎤 {h}")).small().color(SOFT_INK));
                }
                if let Some(sum) = &turn.summary {
                    ui.label(RichText::new(sum).italics().color(CORAL));
                }
                for step in &turn.steps {
                    let open = s.open_rows.contains(&step.id);
                    let colour = if step.is_error && !step.refused { RED } else if step.done { INK } else { SOFT_INK };
                    if ui.selectable_label(open, RichText::new(format!("{} {}", icon(step), label(step))).color(colour)).clicked() {
                        if open {
                            s.open_rows.remove(&step.id);
                        } else {
                            s.open_rows.insert(step.id.clone());
                        }
                    }
                    if open {
                        egui::Frame::new().fill(Color32::WHITE).corner_radius(4).inner_margin(6).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical().id_salt(&step.id).max_height(220.0).show(ui, |ui| ui.add(egui::Label::new(details(step)).selectable(true)));
                        });
                    }
                }
                if !turn.done && !turn.live.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&turn.live).color(SOFT_INK)).selectable(true));
                } else if !turn.reply.is_empty() {
                    let open = s.open_replies.contains(&i);
                    if ui.selectable_label(open, RichText::new(format!("{} {}", if open { "▾" } else { "▸" }, text.full_reply)).small()).clicked() {
                        if open {
                            s.open_replies.remove(&i);
                        } else {
                            s.open_replies.insert(i);
                        }
                    }
                    if open {
                        ui.add(egui::Label::new(&turn.reply).selectable(true));
                    }
                }
            });
            ui.add_space(6.0);
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::parse_line;
    use serde_json::json;

    fn timeline(fixture: &str) -> Timeline {
        let mut tl = Timeline::default();
        tl.user_prompt("do it", Some("uh do it"));
        for ev in fixture.lines().flat_map(parse_line) {
            tl.push(&ev);
        }
        tl
    }

    #[test]
    fn tool_turn() {
        let tl = timeline(include_str!("../tests/fixtures/stream/tool_turn.jsonl"));
        assert_eq!(tl.turns.len(), 1);
        let t = &tl.turns[0];
        assert_eq!((t.prompt.as_str(), t.heard.as_deref(), t.done), ("do it", Some("uh do it"), true));
        assert_eq!(t.reply, "The secret word is **pineapple**, according to notes.txt.");
        assert_eq!(t.steps.len(), 1);
        assert_eq!((label(&t.steps[0]), icon(&t.steps[0])), ("Read notes.txt".to_string(), "📖"));
        assert!(t.steps[0].output.contains("pineapple"));
        assert_eq!(tl.model, "claude-haiku-5-5");
    }

    #[test]
    fn permission_pins_and_clears() {
        let fixture = include_str!("../tests/fixtures/stream/permission.jsonl");
        let mut tl = Timeline::default();
        let mut seen = vec![];
        for ev in fixture.lines().flat_map(parse_line) {
            tl.push(&ev);
            if let Some(p) = tl.pending.first() {
                seen.push(p.request_id.clone());
            }
        }
        assert!(seen.contains(&"75158ceb-67e9-47ca-b571-2fab2d448129".to_string()));
        assert!(tl.pending.is_empty()); // answered: the tool result (or the turn's end) unpins it
        let steps: Vec<_> = tl.turns.iter().flat_map(|t| &t.steps).collect();
        assert_eq!(label(steps[0]), "Wrote a.txt (+1)");
        assert!(label(steps[1]).ends_with(": denied"), "{}", label(steps[1]));
    }

    #[test]
    fn agents_end_even_after_their_turn() {
        let tl = timeline(include_str!("../tests/fixtures/stream/agent_turn.jsonl"));
        let agent = &tl.turns[0].steps[0];
        assert!(agent.done && label(agent).starts_with("Helper: "), "{}", label(agent));

        // The background helper's turn ends first; its end arrives later and wakes Claude in a new turn.
        let tl = timeline(include_str!("../tests/fixtures/stream/background_agent.jsonl"));
        assert_eq!(tl.turns.len(), 2);
        assert!(tl.turns[0].steps.iter().all(|s| s.done));
        assert_eq!(tl.turns[1].prompt, "");
        assert!(tl.turns[1].done);
    }

    #[test]
    fn draws_every_tab_without_panicking() {
        // Headless egui embeds the viewport in the parent, so the drawing code still runs.
        let mut tl = timeline(include_str!("../tests/fixtures/stream/permission.jsonl"));
        tl.pending.push(Permission { request_id: "r".into(), tool: "Edit".into(), input: json!({"file_path": "a.rs", "old_string": "a", "new_string": "b"}), tool_use_id: "x".into() });
        tl.set_summary(0, "Made a.txt~");
        let ctx = egui::Context::default();
        let mut s = PanelState::new(Rect::from_min_size(pos2(800.0, 400.0), vec2(200.0, 250.0)), Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0)));
        s.open_rows.extend(tl.turns.iter().flat_map(|t| t.steps.iter().map(|x| x.id.clone())));
        s.open_replies.insert(0);
        let mut graph = memgraph::GraphState::default();
        for tab in [Tab::Session, Tab::Memory, Tab::Services] {
            s.tab = tab;
            for _ in 0..2 {
                let mut out = ctx.run_ui(Default::default(), |ui| {
                    show(ui.ctx(), &mut s, &tl, &mut graph);
                });
                out.textures_delta.clear(); // no renderer here to hand them to
            }
        }
    }

    #[test]
    fn interrupted_turn() {
        let tl = timeline(include_str!("../tests/fixtures/stream/interrupt.jsonl"));
        assert!(tl.turns[0].interrupted && tl.turns.iter().all(|t| t.done));
    }

    #[test]
    fn counts_changed_lines() {
        let edit = json!({"old_string": "a\nb\nc\nd", "new_string": "a\nB\nB2\nd"});
        assert_eq!(changes("Edit", &edit), (2, 2));
        assert_eq!(changes("Edit", &json!({"old_string": "x", "new_string": "x\ny"})), (1, 0));
        assert_eq!(changes("MultiEdit", &json!({"edits": [edit, {"old_string": "q", "new_string": ""}]})), (2, 3));
        assert_eq!(changes("Write", &json!({"content": "1\n2\n3\n"})), (3, 0));
        let step = Step { id: "1".into(), name: "Edit".into(), input: json!({"file_path": "C:\\src\\app.rs", "old_string": "a", "new_string": "b\nc"}), work: Work::Editing, running: String::new(), done: true, is_error: false, refused: false, output: String::new() };
        assert_eq!(label(&step), "Edited app.rs (+2 −1)");
        let tests = Step { name: "Bash".into(), input: json!({"command": "cargo test"}), work: Work::Testing, is_error: true, ..step };
        assert_eq!(label(&tests), "Ran tests: failed");
    }
}
