// Everything that knows Claude Code's session-log format lives here. The format is internal and unversioned.
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, PartialEq)]
pub enum Event {
    Prompt,                                   // you sent a message: she starts working
    Tool(String),                             // short activity label
    Note(String),                             // Claude's text between tool calls, cleaned and short
    Reply(String),                            // Claude's final text of the turn, raw (goes to the rewriter)
    TurnEnd,                                  // the turn is over
    AgentStart { id: String, label: String }, // a sub-agent was spawned
    AgentDone(String),                        // a tool call finished (minis match it by id)
    ShellStart(String),                       // a command went to the background (task id)
    ShellDone(String),                        // a background command ended (task id)
}

pub fn parse_line(line: &str) -> Vec<Event> {
    let Ok(d) = serde_json::from_str::<Value>(line) else { return vec![] };
    if d["isMeta"] == true || d["isSidechain"] == true {
        return vec![];
    }
    let content = &d["message"]["content"];
    match d["type"].as_str() {
        Some("user") => {
            // Tool results also come back as "user" lines; commands and caveats are wrapped in <tags>.
            let typed = content.as_str().is_some_and(|s| !s.trim_start().starts_with('<'));
            let blocks = content.as_array().into_iter().flatten();
            let has_text = blocks.clone().any(|b| b["type"] == "text");
            // A background agent's tool_result only says it launched; its end comes as a <task-notification>.
            let launched = d["toolUseResult"]["status"] == "async_launched";
            let mut events: Vec<Event> = blocks
                .filter(|b| b["type"] == "tool_result" && !launched)
                .filter_map(|b| Some(Event::AgentDone(b["tool_use_id"].as_str()?.to_string())))
                .collect();
            if typed || has_text {
                events.insert(0, Event::Prompt);
            }
            // Run in the background, or moved there after the 2-minute timeout.
            if let Some(id) = d["toolUseResult"]["backgroundTaskId"].as_str() {
                events.push(Event::ShellStart(id.to_string()));
            }
            events.extend(task_notification(content.as_str()));
            events
        }
        // Mid-turn, a background task's notification is logged only as a queued attachment.
        Some("attachment") if d["attachment"]["type"] == "queued_command" => task_notification(d["attachment"]["prompt"].as_str()),
        Some("assistant") => {
            // Text followed by more tool calls is a side note; anything else is the final reply.
            let interim = d["message"]["stop_reason"] == "tool_use";
            content
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|b| match b["type"].as_str() {
                    Some("text") => {
                        let text = b["text"].as_str().unwrap_or("").trim();
                        if clean(text, 1).is_empty() {
                            vec![]
                        } else if interim {
                            vec![Event::Note(clean(text, 80))]
                        } else {
                            vec![Event::Reply(text.to_string())]
                        }
                    }
                    Some("tool_use") => {
                        let name = b["name"].as_str().unwrap_or("");
                        let mut v = vec![Event::Tool(tool_label(name, &b["input"]))];
                        if matches!(name, "Agent" | "Task") {
                            let label = b["input"]["description"].as_str().unwrap_or("helper");
                            v.push(Event::AgentStart { id: b["id"].as_str().unwrap_or("").to_string(), label: clean(label, 18) });
                        }
                        v
                    }
                    _ => vec![],
                })
                .collect()
        }
        Some("system") if d["subtype"] == "turn_duration" => vec![Event::TurnEnd],
        _ => vec![],
    }
}

/// "<task-notification>\n<task-id>ID</task-id>\n<tool-use-id>…": the task ends, and so does the
/// Agent call that launched it if it was a background agent.
fn task_notification(text: Option<&str>) -> Vec<Event> {
    let Some(body) = text.and_then(|s| s.strip_prefix("<task-notification>")) else { return vec![] };
    let tag = |t: &str| Some(body.split(&format!("<{t}>")).nth(1)?.split('<').next()?.to_string());
    tag("task-id").map(Event::ShellDone).into_iter().chain(tag("tool-use-id").map(Event::AgentDone)).collect()
}

pub fn tool_label(name: &str, input: &Value) -> String {
    let file = input["file_path"].as_str().map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string());
    let ui = crate::i18n::ui();
    match (name, file) {
        ("Read", Some(f)) => ui.reading.replace("{f}", &f),
        ("Edit" | "Write", Some(f)) => ui.editing.replace("{f}", &f),
        ("Bash" | "PowerShell", _) => command_line(input["command"].as_str().unwrap_or("")).into(),
        ("Grep" | "Glob", _) => ui.searching.into(),
        ("Agent" | "Task", _) => ui.helper.into(),
        ("WebSearch" | "WebFetch", _) => ui.online.into(),
        _ => ui.using.replace("{name}", name),
    }
}

/// Her line for a shell command, in the current language. Same command, same line; different commands vary.
fn command_line(cmd: &str) -> &'static str {
    let lines = crate::i18n::lines().commands[command_kind(cmd)];
    lines[cmd.bytes().map(usize::from).sum::<usize>() % lines.len()]
}

/// What kind of shell command it is: 0 tests, 1 git, 2 installs, 3 builds, 4 running the app, 5 web, 6 deleting,
/// 7 looking around, 8 anything else. First matching kind wins.
fn command_kind(cmd: &str) -> usize {
    let c = format!(" {} ", cmd.to_lowercase().replace(['\n', ';', '|', '&', '(', ')'], " "));
    let has = |keys: &[&str]| keys.iter().any(|k| c.contains(k));
    if has(&["test", "pytest", "jest", "vitest"]) {
        0
    } else if has(&[" git ", " gh "]) {
        1
    } else if has(&["install", " cargo add ", " pip ", " winget ", " npm i ", " choco "]) {
        2
    } else if has(&["build", " cargo check ", " tsc ", " make ", "compile", " clippy "]) {
        3
    } else if has(&[" cargo run ", " npm run ", " npm start ", " python ", " node ", " npx ", ".exe "]) {
        4
    } else if has(&[" curl ", " wget ", "invoke-webrequest", "invoke-restmethod"]) {
        5
    } else if has(&[" rm ", " del ", " rmdir ", "remove-item"]) {
        6
    } else if has(&[" ls ", " dir ", " cat ", " grep ", " rg ", " find ", " head ", " tail ", " wc ", " sed ", "get-childitem", "get-content", "select-string"]) {
        7
    } else {
        8
    }
}

/// Bubble-friendly text: no code blocks or markdown symbols, one paragraph, at most `max` chars.
pub fn clean(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let line = line.trim().trim_start_matches(['#', '-', '*', '>', '|', ' ']);
        if !line.is_empty() {
            out.push_str(line);
            out.push(' ');
        }
    }
    let out: String = out.replace(['*', '`'], "").split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > max { out.chars().take(max).collect::<String>() + "…" } else { out }
}

/// Polls every session log for new lines and sends events. History from before startup isn't replayed.
pub fn watch(emit: impl Fn(&PathBuf, Event)) {
    // shortcut: scans ~/.claude/projects on this PC only; switch to hook-registered sessions (SPEC.md) for WSL.
    let root = crate::cli::home().join(".claude").join("projects");
    // Read position per file: logs that exist at startup are skipped to their end; new ones are read from the start.
    let mut positions: HashMap<PathBuf, u64> = logs(&root).into_iter().map(|p| (p.clone(), len(&p))).collect();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        for path in logs(&root) {
            let pos = positions.entry(path.clone()).or_insert(0);
            if len(&path) <= *pos {
                continue;
            }
            let mut bytes = Vec::new();
            let Ok(mut f) = File::open(&path) else { continue };
            if f.seek(SeekFrom::Start(*pos)).is_err() || f.read_to_end(&mut bytes).is_err() {
                continue;
            }
            // Only consume whole lines; a half-written last line is read again next round.
            let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else { continue };
            *pos += end as u64 + 1;
            for ev in String::from_utf8_lossy(&bytes[..end]).lines().flat_map(parse_line) {
                emit(&path, ev);
            }
        }
    }
}

fn len(p: &PathBuf) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Every session log file.
fn logs(root: &PathBuf) -> Vec<PathBuf> {
    let Ok(dirs) = std::fs::read_dir(root) else { return vec![] };
    dirs.flatten()
        .filter_map(|d| std::fs::read_dir(d.path()).ok())
        .flat_map(|r| r.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_line_shapes() {
        let prompt = r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":"Try to speak something"}}"#;
        let image_prompt = r#"{"type":"user","message":{"content":[{"type":"text","text":"[Image #2] look"},{"type":"image","source":{}}]}}"#;
        let meta = r#"{"type":"user","isMeta":true,"message":{"content":"<local-command-caveat>x"}}"#;
        let command = r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#;
        let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#;
        let tool = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"C:\\Claudesona\\src\\main.rs"}}]}}"#;
        let text = r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"Done! **Bold** and `code`."}]}}"#;
        let note = r#"{"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"text","text":"Checking **files**."}]}}"#;
        let agent = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"a1","name":"Agent","input":{"description":"Find auth code"}}]}}"#;
        let agent_result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"a1","content":"found"}]}}"#;
        let turn_end = r#"{"type":"system","subtype":"turn_duration","durationMs":1200}"#;
        let thinking = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":""}]}}"#;
        assert_eq!(parse_line(prompt), vec![Event::Prompt]);
        assert_eq!(parse_line(image_prompt), vec![Event::Prompt]);
        assert!(parse_line(meta).is_empty() && parse_line(command).is_empty());
        assert_eq!(parse_line(result), vec![Event::AgentDone("t1".into())]);
        assert_eq!(parse_line(tool), vec![Event::Tool("Reading main.rs…".into())]);
        assert_eq!(parse_line(text), vec![Event::Reply("Done! **Bold** and `code`.".into())]);
        assert_eq!(parse_line(note), vec![Event::Note("Checking files.".into())]);
        assert_eq!(parse_line(agent), vec![Event::Tool("Sending a helper…".into()), Event::AgentStart { id: "a1".into(), label: "Find auth code".into() }]);
        assert_eq!(parse_line(agent_result), vec![Event::AgentDone("a1".into())]);
        assert_eq!(parse_line(turn_end), vec![Event::TurnEnd]);
        assert!(parse_line(thinking).is_empty() && parse_line("not json").is_empty());
        let bg = r#"{"type":"user","message":{"content":[{"tool_use_id":"t2","type":"tool_result","content":"Command running in background with ID: bdizax5j0."}]},"toolUseResult":{"stderr":"","backgroundTaskId":"bdizax5j0"}}"#;
        let bg_done = r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>bdizax5j0</task-id>\n<tool-use-id>t2</tool-use-id>\n<status>completed</status>\n</task-notification>"}}"#;
        let plugin_note = r#"{"type":"user","message":{"content":"<task-notification>\nponytail: its hook was skipped.\n</task-notification>"}}"#;
        assert_eq!(parse_line(bg), vec![Event::AgentDone("t2".into()), Event::ShellStart("bdizax5j0".into())]);
        assert_eq!(parse_line(bg_done), vec![Event::ShellDone("bdizax5j0".into()), Event::AgentDone("t2".into())]);
        assert!(parse_line(plugin_note).is_empty());
        // Background agent: the launch reply isn't its end; the notification is.
        let async_agent = r#"{"type":"user","message":{"content":[{"tool_use_id":"a2","type":"tool_result","content":[{"type":"text","text":"Async agent launched successfully."}]}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"ae851c9688370f621","description":"PR 1.3"}}"#;
        let async_done = r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>ae851c9688370f621</task-id>\n<tool-use-id>a2</tool-use-id>\n<output-file>C:\\x.output</output-file>\n<status>completed</status>\n</task-notification>"}}"#;
        assert!(parse_line(async_agent).is_empty());
        assert_eq!(parse_line(async_done), vec![Event::ShellDone("ae851c9688370f621".into()), Event::AgentDone("a2".into())]);
        // When it ends mid-turn, the notification only shows up as a queued attachment.
        let queued = r#"{"parentUuid":"b0","isSidechain":false,"attachment":{"type":"queued_command","prompt":"<task-notification>\n<task-id>a92510ec3180d73c5</task-id>\n<tool-use-id>a3</tool-use-id>\n<status>completed</status>\n<summary>Agent \"Review\" completed</summary>\n</task-notification>"},"type":"attachment"}"#;
        assert_eq!(parse_line(queued), vec![Event::ShellDone("a92510ec3180d73c5".into()), Event::AgentDone("a3".into())]);
    }

    #[test]
    fn commands_get_lines_by_kind() {
        let first = |cmd: &str| crate::i18n::lines().commands[command_kind(cmd)][0];
        let is = |cmd: &str, word: &str| assert!(first(cmd).to_lowercase().contains(word), "{cmd} -> {}", first(cmd));
        is("cargo test parse", "test");
        is("cd /c/x && git status", "git");
        is("npm install", "packages");
        is("cargo build --release", "build");
        is("ls -la", "files");
        is("echo hi", "command");
    }

    #[test]
    fn clean_drops_code_and_caps_length() {
        assert_eq!(clean("Hi\n```rust\nfn x() {}\n```\n- one\n## Two", 160), "Hi one Two");
        assert!(clean(&"word ".repeat(100), 160).ends_with('…'));
    }
}
