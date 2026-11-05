// MCP services (SPEC-v2.md, "MCP services"): `mcp.json` next to the exe, in Claude Code's `--mcp-config`
// shape. Servers that are on sit in `mcpServers`; off ones in `disabledServers`, so they keep their settings.
// Hosted sessions get `mcp-session.json`, written with only the ones that are on.
use crate::{CORAL, SOFT_INK, i18n};
use eframe::egui::{self, RichText};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Server {
    pub name: String,
    pub spec: Value, // {command, args, env} or {type, url, headers}, as Claude Code reads it
    pub on: bool,
}

#[derive(Default)]
pub struct ServicesState {
    pub servers: Vec<Server>,
    pub strict: bool,                 // pass --strict-mcp-config: ignore ~/.claude.json and .mcp.json servers
    pub status: Vec<(String, String)>, // (name, status) from the running session's init line
    pub changed: bool,                // set when something was saved; the panel turns it into an action
    error: Option<String>,            // mcp.json exists but isn't valid: never overwrite it
    name: String,
    target: String,
    secrets: Vec<(String, String)>,
    preview: Option<Value>, // the server about to be added, shown before it is
    confirm_remove: Option<String>,
}

fn dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| Some(p.parent()?.to_path_buf())).unwrap_or_default()
}

fn path() -> PathBuf {
    dir().join("mcp.json")
}

impl ServicesState {
    // shortcut: reads mcp.json on the UI thread when the panel opens; it's a few hundred bytes. Move to a worker if it grows.
    pub fn load() -> Self {
        match load_from(&path()) {
            Ok((servers, strict)) => ServicesState { servers, strict, ..Default::default() },
            Err(e) => ServicesState { error: Some(e), ..Default::default() },
        }
    }

    fn save(&mut self) {
        if self.error.is_some() {
            return;
        }
        self.changed = true;
        let text = serde_json::to_string_pretty(&to_json(&self.servers, self.strict)).unwrap_or_default();
        // Written on a worker; a later save always wins even if threads finish out of order.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        static LAST: Mutex<u64> = Mutex::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        std::thread::spawn(move || {
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if n > *last {
                let _ = std::fs::write(path(), text);
                *last = n;
            }
        });
    }
}

/// Servers and the strict flag from a file; no file means none. Err when the file can't be parsed.
fn load_from(p: &Path) -> Result<(Vec<Server>, bool), String> {
    let text = match std::fs::read_to_string(p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], false)),
        Err(e) => return Err(e.to_string()),
    };
    let d: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let list = |key: &str, on: bool| d[key].as_object().into_iter().flatten().map(move |(n, v)| Server { name: n.clone(), spec: v.clone(), on });
    Ok((list("mcpServers", true).chain(list("disabledServers", false)).collect(), d["strictMcpConfig"] == true))
}

fn to_json(servers: &[Server], strict: bool) -> Value {
    let map = |on: bool| servers.iter().filter(|s| s.on == on).map(|s| (s.name.clone(), s.spec.clone())).collect::<Map<_, _>>();
    let mut d = json!({ "mcpServers": map(true) });
    if servers.iter().any(|s| !s.on) {
        d["disabledServers"] = map(false).into();
    }
    if strict {
        d["strictMcpConfig"] = true.into();
    }
    d
}

/// Writes the servers that are on to `mcp-session.json` and returns its path for `--mcp-config`;
/// None when there are none (or mcp.json can't be read).
pub fn config_path_for_session() -> Option<PathBuf> {
    write_session(&path(), &dir().join("mcp-session.json"))
}

fn write_session(src: &Path, dst: &Path) -> Option<PathBuf> {
    let (servers, _) = load_from(src).ok()?;
    let on: Vec<Server> = servers.into_iter().filter(|s| s.on).collect();
    if on.is_empty() {
        return None;
    }
    std::fs::write(dst, serde_json::to_string_pretty(&to_json(&on, false)).ok()?).ok()?;
    Some(dst.to_path_buf())
}

/// Whether hosted sessions get `--strict-mcp-config`.
pub fn strict() -> bool {
    load_from(&path()).is_ok_and(|(_, s)| s)
}

/// A server entry from what you typed: a URL, or a command line (double quotes group words).
/// The secrets become `headers` for a URL and `env` for a command.
fn server_spec(target: &str, secrets: &[(String, String)]) -> Option<Value> {
    let t = target.trim();
    let secrets: Map<String, Value> = secrets.iter().filter(|(k, _)| !k.trim().is_empty()).map(|(k, v)| (k.trim().to_string(), v.clone().into())).collect();
    let mut spec = if t.starts_with("http://") || t.starts_with("https://") {
        json!({ "type": if t.trim_end_matches('/').ends_with("/sse") { "sse" } else { "http" }, "url": t })
    } else {
        let mut words = split_command(t).into_iter();
        json!({ "command": words.next()?, "args": words.collect::<Vec<_>>() })
    };
    if !secrets.is_empty() {
        let key = if spec["url"].is_string() { "headers" } else { "env" };
        spec[key] = secrets.into();
    }
    Some(spec)
}

fn split_command(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut any) = (vec![], String::new(), false, false);
    for c in line.chars() {
        match c {
            '"' => (quoted, any) = (!quoted, true),
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// What a server runs, one line, with secret values as dots.
fn describe(spec: &Value) -> String {
    let main = match spec["url"].as_str() {
        Some(u) => u.to_string(),
        None => std::iter::once(&spec["command"]).chain(spec["args"].as_array().into_iter().flatten()).filter_map(Value::as_str).map(|w| if w.contains(' ') { format!("\"{w}\"") } else { w.into() }).collect::<Vec<_>>().join(" "),
    };
    let secrets = spec["env"].as_object().or(spec["headers"].as_object()).into_iter().flatten().map(|(k, _)| format!("  {k}=••••••"));
    std::iter::once(main).chain(secrets).collect::<Vec<_>>().join("\n")
}

pub fn ui(ui: &mut egui::Ui, s: &mut ServicesState) {
    let text = i18n::ui();
    if let Some(e) = &s.error {
        ui.colored_label(CORAL, text.svc_broken.replace("{e}", e));
        return;
    }
    if ui.checkbox(&mut s.strict, text.svc_strict).changed() {
        s.save();
    }
    ui.separator();
    if s.servers.is_empty() {
        ui.label(RichText::new(text.svc_empty).color(SOFT_INK));
    }
    let mut remove = None;
    let mut toggled = false;
    for (i, srv) in s.servers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            toggled |= ui.checkbox(&mut srv.on, RichText::new(&srv.name).strong()).changed();
            if let Some((_, st)) = s.status.iter().find(|(n, _)| *n == srv.name) {
                ui.label(RichText::new(st).small().color(SOFT_INK));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Two clicks, because its secrets go with it.
                let sure = s.confirm_remove.as_deref() == Some(srv.name.as_str());
                if ui.small_button(if sure { text.forget_sure } else { "✕" }).clicked() {
                    if sure {
                        remove = Some(i);
                    }
                    s.confirm_remove = if sure { None } else { Some(srv.name.clone()) };
                }
            });
        });
        ui.label(RichText::new(describe(&srv.spec)).monospace().small().color(SOFT_INK));
    }
    if let Some(i) = remove {
        s.servers.remove(i);
    }
    if toggled || remove.is_some() {
        s.save();
    }

    ui.separator();
    if let Some(spec) = &s.preview {
        // Servers run with your rights: say exactly what will run before it's added.
        ui.label(text.svc_runs);
        ui.label(RichText::new(describe(spec)).monospace());
        ui.horizontal(|ui| {
            if ui.button(text.svc_confirm).clicked() {
                let spec = s.preview.take().unwrap_or_default();
                s.servers.push(Server { name: s.name.trim().to_string(), spec, on: true });
                (s.name, s.target, s.secrets) = (String::new(), String::new(), vec![]);
                s.save();
            }
            if ui.button(text.svc_cancel).clicked() {
                s.preview = None;
            }
        });
        return;
    }
    ui.add(egui::TextEdit::singleline(&mut s.name).hint_text(text.svc_name).desired_width(f32::INFINITY));
    ui.add(egui::TextEdit::singleline(&mut s.target).hint_text(text.svc_target).desired_width(f32::INFINITY));
    for (k, v) in &mut s.secrets {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(k).hint_text("KEY").desired_width(110.0));
            ui.label("=");
            ui.add(egui::TextEdit::singleline(v).password(true).desired_width(f32::INFINITY));
        });
    }
    ui.horizontal(|ui| {
        if ui.small_button(text.svc_secret).clicked() {
            s.secrets.push(Default::default());
        }
        let name = s.name.trim();
        let ok = !name.is_empty() && !s.servers.iter().any(|x| x.name == name);
        let spec = server_spec(&s.target, &s.secrets).filter(|_| ok);
        if ui.add_enabled(spec.is_some(), egui::Button::new(text.svc_add)).clicked() {
            s.preview = spec;
        }
    });
    ui.label(RichText::new(text.svc_next).small().color(SOFT_INK));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_command_lines() {
        assert_eq!(split_command(r#"npx -y @scope/server "C:\My Files" """#), ["npx", "-y", "@scope/server", r"C:\My Files", ""]);
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn builds_specs() {
        let secret = [("API_KEY".to_string(), "s3cret".to_string()), (" ".into(), "x".into())];
        assert_eq!(server_spec("node server.js --port 3", &secret), Some(json!({"command":"node","args":["server.js","--port","3"],"env":{"API_KEY":"s3cret"}})));
        assert_eq!(server_spec("https://example.com/mcp", &[]), Some(json!({"type":"http","url":"https://example.com/mcp"})));
        assert_eq!(server_spec("https://example.com/sse", &secret).unwrap()["headers"], json!({"API_KEY":"s3cret"}));
        assert_eq!(server_spec("  ", &[]), None);
        assert!(!describe(&server_spec("node a.js", &secret).unwrap()).contains("s3cret"));
    }

    #[test]
    fn round_trip_and_session_file() {
        let dir = std::env::temp_dir().join(format!("claudesona-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (src, dst) = (dir.join("mcp.json"), dir.join("mcp-session.json"));
        assert_eq!(load_from(&src).unwrap().0.len(), 0); // no file yet
        assert_eq!(write_session(&src, &dst), None);

        let servers = vec![
            Server { name: "gmail".into(), spec: server_spec("npx gmail-mcp", &[("TOKEN".into(), "t".into())]).unwrap(), on: true },
            Server { name: "yt".into(), spec: server_spec("https://yt.example/mcp", &[]).unwrap(), on: false },
        ];
        std::fs::write(&src, serde_json::to_string(&to_json(&servers, true)).unwrap()).unwrap();
        let (back, strict) = load_from(&src).unwrap();
        assert!(strict);
        assert_eq!(back.iter().map(|s| (s.name.as_str(), s.on, &s.spec)).collect::<Vec<_>>(), servers.iter().map(|s| (s.name.as_str(), s.on, &s.spec)).collect::<Vec<_>>());

        assert_eq!(write_session(&src, &dst), Some(dst.clone()));
        let session: Value = serde_json::from_str(&std::fs::read_to_string(&dst).unwrap()).unwrap();
        assert_eq!(session, json!({"mcpServers":{"gmail":{"command":"npx","args":["gmail-mcp"],"env":{"TOKEN":"t"}}}}));

        std::fs::write(&src, "{ not json").unwrap();
        assert!(load_from(&src).is_err()); // and ServicesState then refuses to save over it
        let _ = std::fs::remove_dir_all(&dir);
    }
}
