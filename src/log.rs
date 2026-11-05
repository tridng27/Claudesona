// Everything that knows Claude Code's session-log format lives here. The format is internal and unversioned.
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;

/// What a tool call is doing, for her working pose.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Work {
    #[default]
    Plain, // anything else: the usual working pose
    Reading,  // Read, Grep, Glob
    Editing,  // Edit, Write, NotebookEdit
    Testing,  // a shell command that runs tests
    Shipping, // git commit / git push
    Web,      // WebSearch, WebFetch
    Service,  // an MCP tool (mcp__server__tool): one pose for all of them
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Prompt,                                   // you sent a message: she starts working
    Tool(String, Work),                       // short activity label, and what kind of work it is
    Failed,                                   // a tool call came back with an error
    Compact,                                  // Claude compacted its context
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
    // The summary after a compaction is logged as a "user" line, but nobody typed it.
    if d["isMeta"] == true || d["isSidechain"] == true || d["isCompactSummary"] == true {
        return vec![];
    }
    let content = &d["message"]["content"];
    match d["type"].as_str() {
        Some("user") => {
            // Tool results also come back as "user" lines; commands and caveats are wrapped in <tags>.
            let typed = content.as_str().is_some_and(|s| !s.trim_start().starts_with('<'));
            let blocks = content.as_array().into_iter().flatten();
            let has_text = blocks.clone().any(|b| b["type"] == "text");
            // A failed command or edit; the user (or a hook, setting or classifier) saying no doesn't count.
            let failed = blocks.clone().any(|b| b["type"] == "tool_result" && b["is_error"] == true && !refused(&b["content"]));
            // A background agent's tool_result only says it launched; its end comes as a <task-notification>.
            let launched = d["toolUseResult"]["status"] == "async_launched";
            let mut events: Vec<Event> = blocks
                .filter(|b| b["type"] == "tool_result" && !launched)
                .filter_map(|b| Some(Event::AgentDone(b["tool_use_id"].as_str()?.to_string())))
                .collect();
            if typed || has_text {
                events.insert(0, Event::Prompt);
            }
            if failed {
                events.push(Event::Failed);
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
                        let mut v = vec![Event::Tool(tool_label(name, &b["input"]), work_kind(name, &b["input"]))];
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
        Some("system") if d["subtype"] == "compact_boundary" => vec![Event::Compact],
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

/// An error result that only says no: the user rejected or interrupted it, or a hook, setting or classifier denied it.
fn refused(content: &Value) -> bool {
    let text = content.as_str().or_else(|| content[0]["text"].as_str()).unwrap_or("");
    ["The user doesn't want to proceed", "[Request interrupted by user", "Permission to use ", "Permission for this action was denied", "PreToolUse:"]
        .iter()
        .any(|p| text.starts_with(p))
}

/// What kind of work a tool call is (also used for the PreToolUse hook's tool_name and tool_input).
pub fn work_kind(name: &str, input: &Value) -> Work {
    match name {
        "Read" | "Grep" | "Glob" => Work::Reading,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => Work::Editing,
        "WebSearch" | "WebFetch" => Work::Web,
        n if n.starts_with("mcp__") => Work::Service,
        "Bash" | "PowerShell" => {
            let cmd = input["command"].as_str().unwrap_or("");
            let c = words(cmd);
            if c.contains(" git commit ") || c.contains(" git push ") {
                Work::Shipping
            } else if c.contains(" sed -i") {
                Work::Editing
            } else {
                match command_kind(cmd) {
                    0 => Work::Testing,
                    5 => Work::Web,
                    7 => Work::Reading, // looking around: ls, cat, grep, Get-Content…
                    _ => Work::Plain,
                }
            }
        }
        _ => Work::Plain,
    }
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
        _ => ui.using.replace("{name}", &name.strip_prefix("mcp__").map_or(name.to_string(), server_name)),
    }
}

/// "Google Calendar" from an MCP tool's `server__tool` part ("google_calendar__list_events"). claude.ai connectors
/// ("claude_ai_Gmail") and plugin servers ("plugin_design_figma") lose their prefix.
fn server_name(tool: &str) -> String {
    let server = tool.split("__").next().unwrap_or(tool);
    let plugin = server.strip_prefix("plugin_").and_then(|p| p.split_once('_')).map(|(_, s)| s);
    let server = server.strip_prefix("claude_ai_").or(plugin).unwrap_or(server);
    let words = server.split(['_', '-']).filter(|w| !w.is_empty()).map(|w| {
        let mut c = w.chars();
        c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    });
    words.collect::<Vec<_>>().join(" ")
}

/// The command lowercased and padded, with separators as spaces, so " word " matches whole words.
fn words(cmd: &str) -> String {
    format!(" {} ", cmd.to_lowercase().replace(['\n', '\r', '\t', ';', '|', '&', '(', ')'], " "))
}

/// Her line for a shell command, in the current language. Same command, same line; different commands vary.
fn command_line(cmd: &str) -> &'static str {
    let lines = crate::i18n::lines().commands[command_kind(cmd)];
    lines[cmd.bytes().map(usize::from).sum::<usize>() % lines.len()]
}

/// What kind of shell command it is: 0 tests, 1 git, 2 installs, 3 builds, 4 running the app, 5 web, 6 deleting,
/// 7 looking around, 8 anything else. First matching kind wins.
fn command_kind(cmd: &str) -> usize {
    let c = words(cmd);
    let has = |keys: &[&str]| keys.iter().any(|k| c.contains(k));
    // Whole test-runner words, so paths like tests/foo.rs and the shell's `test -f` don't count.
    if has(&[" cargo test ", " cargo nextest ", " npm test ", " npm run test", " pnpm test ", " yarn test ", " npx jest", " jest ", " pytest", " vitest", " go test ", " dotnet test ", " mvn test ", "gradle test ", " gradlew test ", " ctest ", " -m unittest ", " rspec ", " mocha ", " phpunit ", "invoke-pester"]) && !has(&[" install ", " cargo add ", " uv add ", " poetry add "]) {
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
            let Some((text, next)) = read_new(&path, *pos) else { continue };
            *pos = next;
            for ev in text.lines().flat_map(parse_line) {
                emit(&path, ev);
            }
        }
    }
}

/// The whole lines written after byte `pos`, and where the next read starts. A half-written last line is
/// left for next time. None if there's no complete new line yet (or the file can't be read).
fn read_new(path: &PathBuf, pos: u64) -> Option<(String, u64)> {
    let mut bytes = Vec::new();
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(pos)).ok()?;
    f.read_to_end(&mut bytes).ok()?;
    let end = bytes.iter().rposition(|b| *b == b'\n')?;
    Some((String::from_utf8_lossy(&bytes[..end]).into_owned(), pos + end as u64 + 1))
}

fn len(p: &PathBuf) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// A saved session, for resuming it and for knowing the folders sessions ran in.
#[derive(Clone, Debug, PartialEq)]
pub struct Past {
    pub id: String,
    pub dir: PathBuf,  // the folder it ran in
    pub title: String, // Claude Code's title for it, else your first prompt
}

/// Saved sessions, newest first: the `max` newest, plus the newest of every project folder (so every folder
/// sessions ran in is known). Reads files: call from a worker thread.
pub fn past_sessions(max: usize) -> Vec<Past> {
    let root = crate::cli::home().join(".claude").join("projects");
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = logs(&root).into_iter().filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p))).collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let mut folders = std::collections::HashSet::new();
    files
        .into_iter()
        .enumerate()
        .filter(|(i, (_, p))| folders.insert(p.parent().map(PathBuf::from)) || *i < max)
        .filter_map(|(_, (_, p))| {
            let mut head = Vec::new();
            // shortcut: the first 512KB; a title written later than that falls back to the first prompt.
            File::open(&p).ok()?.take(512 * 1024).read_to_end(&mut head).ok()?;
            let (dir, title) = session_head(&String::from_utf8_lossy(&head))?;
            Some(Past { id: p.file_stem()?.to_string_lossy().into_owned(), dir: PathBuf::from(dir), title })
        })
        .collect()
}

/// The folder a session ran in and its title, from the start of its log. None without a folder.
fn session_head(text: &str) -> Option<(String, String)> {
    let (mut cwd, mut title, mut prompt) = (None, None, None);
    for d in text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        cwd = cwd.or_else(|| d["cwd"].as_str().map(String::from));
        title = d["aiTitle"].as_str().map(String::from).or(title);
        let typed = d["message"]["content"].as_str().filter(|s| d["type"] == "user" && d["isMeta"] != true && !s.trim_start().starts_with('<'));
        prompt = prompt.or_else(|| typed.map(|s| clean(s, 60)));
    }
    Some((cwd?, title.or(prompt).unwrap_or_default()))
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
        assert_eq!(parse_line(tool), vec![Event::Tool("Reading main.rs…".into(), Work::Reading)]);
        assert_eq!(parse_line(text), vec![Event::Reply("Done! **Bold** and `code`.".into())]);
        assert_eq!(parse_line(note), vec![Event::Note("Checking files.".into())]);
        assert_eq!(parse_line(agent), vec![Event::Tool("Sending a helper…".into(), Work::Plain), Event::AgentStart { id: "a1".into(), label: "Find auth code".into() }]);
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
    fn failures_and_compaction() {
        let failed = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"Exit code 1\nerror[E0425]: x","is_error":true,"tool_use_id":"t1"}]},"toolUseResult":"Error: Exit code 1\nerror[E0425]: x"}"#;
        let bad_edit = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"<tool_use_error>String to replace not found in file.</tool_use_error>","is_error":true,"tool_use_id":"t2"},{"type":"tool_result","content":"File does not exist.","is_error":true,"tool_use_id":"t3"}]}}"#;
        let rejected = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed.","is_error":true,"tool_use_id":"t4"}]},"toolUseResult":"User rejected tool use"}"#;
        let interrupted = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"The user doesn't want to proceed with this tool use.","is_error":true,"tool_use_id":"t5"},{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#;
        let denied = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Permission to use Bash with command ls has been denied.","is_error":true,"tool_use_id":"t6"}]}}"#;
        let classifier = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Permission for this action was denied by the Claude Code auto mode classifier. Reason: [x].","is_error":true,"tool_use_id":"t7"}]}}"#;
        let hook = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"PreToolUse:mcp__x hook error: [guard] no.","is_error":true,"tool_use_id":"t8"}]}}"#;
        assert_eq!(parse_line(failed), vec![Event::AgentDone("t1".into()), Event::Failed]);
        assert_eq!(parse_line(bad_edit), vec![Event::AgentDone("t2".into()), Event::AgentDone("t3".into()), Event::Failed]);
        assert_eq!(parse_line(rejected), vec![Event::AgentDone("t4".into())]);
        assert_eq!(parse_line(interrupted), vec![Event::Prompt, Event::AgentDone("t5".into())]);
        for no in [denied, classifier, hook] {
            assert!(!parse_line(no).contains(&Event::Failed), "{no}");
        }
        let compact = r#"{"parentUuid":null,"logicalParentUuid":"p0","isSidechain":false,"type":"system","subtype":"compact_boundary","content":"Conversation compacted","isMeta":false,"level":"info","compactMetadata":{"trigger":"auto","preTokens":365701}}"#;
        let summary = r#"{"parentUuid":"c0","isSidechain":false,"type":"user","message":{"role":"user","content":"This session is being continued from a previous conversation that ran out of context."},"isVisibleInTranscriptOnly":true,"isCompactSummary":true}"#;
        assert_eq!(parse_line(compact), vec![Event::Compact]);
        assert!(parse_line(summary).is_empty());
    }

    #[test]
    fn work_kinds() {
        let kind = |name: &str, input: &str| work_kind(name, &serde_json::from_str(input).unwrap());
        let sh = |cmd: &str| work_kind("Bash", &serde_json::json!({ "command": cmd }));
        assert_eq!(kind("Read", r#"{"file_path":"src/a.rs"}"#), Work::Reading);
        assert_eq!(kind("Grep", r#"{"pattern":"x"}"#), Work::Reading);
        assert_eq!(kind("Glob", r#"{"pattern":"**/*.rs"}"#), Work::Reading);
        assert_eq!(kind("Edit", r#"{"file_path":"a.rs","old_string":"a","new_string":"b"}"#), Work::Editing);
        assert_eq!(kind("Write", r#"{"file_path":"a.rs","content":""}"#), Work::Editing);
        assert_eq!(kind("NotebookEdit", r#"{"notebook_path":"a.ipynb"}"#), Work::Editing);
        assert_eq!(kind("WebSearch", r#"{"query":"x"}"#), Work::Web);
        assert_eq!(kind("WebFetch", r#"{"url":"https://x"}"#), Work::Web);
        assert_eq!(kind("Agent", r#"{"description":"x"}"#), Work::Plain);
        assert_eq!(kind("mcp__gmail__search_threads", "{}"), Work::Service);
        assert_eq!(server_name("google_calendar__list_events"), "Google Calendar");
        assert_eq!(server_name("claude_ai_Gmail__search"), "Gmail");
        assert_eq!(server_name("plugin_design_figma__get"), "Figma");
        assert_eq!(server_name("my-server__x"), "My Server");
        assert_eq!(kind("PowerShell", r#"{"command":"cargo test log::"}"#), Work::Testing);
        assert_eq!(sh("cd /c/x && cargo test log::"), Work::Testing);
        assert_eq!(sh("npx vitest run"), Work::Testing);
        assert_eq!(sh("python -m pytest -q"), Work::Testing);
        assert_eq!(sh("npm run test:unit"), Work::Testing);
        assert_eq!(sh("git add -A && git commit -m \"fix\""), Work::Shipping);
        assert_eq!(sh("cargo test && git push origin main"), Work::Shipping);
        assert_eq!(sh("cat tests/foo.rs"), Work::Reading);
        assert_eq!(sh("cd /c/x; grep -n foo src/main.rs | head"), Work::Reading);
        assert_eq!(sh("Get-Content src/main.rs -TotalCount 40"), Work::Reading);
        assert_eq!(sh("sed -i 's/a/b/' src/main.rs"), Work::Editing);
        assert_eq!(sh("curl -s https://example.com"), Work::Web);
        assert_eq!(sh("pip install pytest"), Work::Plain);
        assert_eq!(sh("test -f .env && echo ok"), Work::Plain);
        assert_eq!(sh("git status"), Work::Plain);
    }

    #[test]
    fn reads_only_whole_lines() {
        let path = std::env::temp_dir().join(format!("claudesona-log-test-{}.jsonl", std::process::id()));
        std::fs::write(&path, "{\"a\":1}\n{\"b\":").unwrap();
        let (text, pos) = read_new(&path, 0).unwrap();
        assert_eq!((text.as_str(), pos), ("{\"a\":1}", 8)); // the half line waits
        assert_eq!(read_new(&path, pos), None);
        std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n").unwrap(); // the writer finishes it
        assert_eq!(read_new(&path, pos), Some(("{\"b\":2}".to_string(), 16)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn commands_get_lines_by_kind() {
        let first = |cmd: &str| crate::i18n::lines().commands[command_kind(cmd)][0];
        let is = |cmd: &str, word: &str| assert!(first(cmd).to_lowercase().contains(word), "{cmd} -> {}", first(cmd));
        is("cargo test parse", "test");
        is("cat tests/foo.rs", "files");
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

    #[test]
    fn session_folder_and_title() {
        // Shapes from a 2.1.318 log (long fields cut).
        let log = [
            r#"{"type":"last-prompt","leafUuid":"171bf5e8","sessionId":"3043b734"}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"<local-command-caveat>x"},"cwd":"C:\\Claudesona"}"#,
            r#"{"parentUuid":"171bf5e8","isSidechain":false,"type":"user","message":{"role":"user","content":"Can claudesona interact with a claude session?"},"cwd":"C:\\Claudesona","sessionId":"3043b734","version":"2.1.318"}"#,
            r#"{"type":"ai-title","aiTitle":"Claudesona interacting with Claude sessions","sessionId":"3043b734"}"#,
        ]
        .join("\n");
        assert_eq!(session_head(&log), Some((r"C:\Claudesona".into(), "Claudesona interacting with Claude sessions".into())));
        assert_eq!(session_head(&log.replace("ai-title", "x").replace("aiTitle", "x")).unwrap().1, "Can claudesona interact with a claude session?");
        assert_eq!(session_head(r#"{"type":"mode","mode":"normal"}"#), None);
    }
}
