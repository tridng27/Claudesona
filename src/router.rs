// Decides what a message to her (spoken or typed) is for: chat, a prompt for the session, a permission answer,
// a slash command, a summary, a session command or memory. Plain cases are matched in code; the rest is one
// Haiku call (rewrite::ask, the rewriter's flags) that answers in JSON and also cleans up spoken prompts.
// The actions are described in SPEC-v2.md (Talking to her).
use crate::i18n::{fold, Lang};
use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct RouteCtx {
    pub waiting_for_permission: bool,
    pub has_session: bool,
    pub slash_commands: Vec<String>, // from the session's init line, with or without the leading /
    pub known_projects: Vec<String>, // folder names
    pub project_files: Vec<String>,  // relative names in the active project, for fixing misheard ones
    pub language: Lang,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionCmd {
    New { project: Option<String> },
    Resume { hint: Option<String> },
    Interrupt,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Chat(String), // the user's text, for her chat
    Say(String),  // a line she says as is (e.g. a slash command she can't run)
    Prompt { text: String },
    Answer { allow: bool, always: bool },
    Slash { command: String, args: String }, // command without the /
    Summarize,
    Session(SessionCmd),
    Remember(String),
    Forget(String),
    Skill(Skill), // one of her own built-in things, like the menu buttons
}

/// Her built-in functions, the same ones her menu offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Skill {
    MusicOn,
    MusicOff,
    NextSong,
    Coffee,
    Play(Option<Game>), // None: she asks which game
    Tour,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Game {
    Hangman,
    Sudoku,
}

const SYSTEM: &str = r#"You route messages that a user says or types to Claudesona, a desktop mascot that drives a Claude Code session. Reply with one JSON object only, no prose, no code fences.
Actions:
{"action":"chat"} - small talk, questions to her, or anything unclear.
{"action":"prompt","text":"..."} - work for the coding assistant (code, files, tasks, questions about the project, using services like email or calendar). "text" is the request cleaned up: if spoken, remove fillers (um, uh, like, you know, ờ, ừm, kiểu) and false starts, and fix obviously misheard file or function names using the project files listed. Keep the user's language and wording. Never add or drop requirements.
{"action":"answer","allow":true|false,"always":true|false} - only while a permission is waiting: yes/allow, no/deny, or always allow.
{"action":"slash","command":"compact","args":""} - a request that matches one of the available slash commands (command without the /).
{"action":"summarize"} - asks what Claude did or how the session is going.
{"action":"session","cmd":"new","hint":"..."} - start a session; hint is the project name if given.
{"action":"session","cmd":"resume","hint":"..."} - switch to or resume an earlier session; hint describes which.
{"action":"session","cmd":"interrupt"} - stop what Claude is doing right now.
{"action":"session","cmd":"end"} - close the session.
{"action":"remember","text":"..."} - asks her to remember a fact; text is the fact.
{"action":"forget","text":"..."} - asks her to forget something; text says what.
{"action":"skill","name":"..."} - asks her (not the coding assistant) to do one of her own things. name is one of: music_on (play music), music_off (stop the music), next_song, coffee (give her or have her drink coffee), play (play a game with her; add "game":"hangman" or "game":"sudoku" if named), tour (what can you do), hide (go away for a while).
If a message starts with [prompt], it is always a prompt: return it cleaned up without the [prompt] mark."#;

/// Starts the routing process early (e.g. when the mic hears speech), so `route` answers sooner.
pub fn prewarm() {
    crate::rewrite::prewarm(SYSTEM);
}

/// Routes `text` on a worker thread and calls `on_done` with the action there. Never blocks.
pub fn route(text: &str, ctx: RouteCtx, spoken: bool, on_done: impl FnOnce(Action) + Send + 'static) {
    let text = text.trim().to_string();
    std::thread::spawn(move || {
        let action = match fast(&text, &ctx, spoken) {
            Some(a) => a,
            None => {
                let forced = forced_prompt(&text).is_some();
                let out = crate::rewrite::ask(SYSTEM, &input(&text, &ctx, spoken));
                out.and_then(|o| parse(&o, &text, &ctx, spoken, forced)).unwrap_or(Action::Chat(text))
            }
        };
        on_done(action);
    });
}

/// Actions decided without Haiku. A spoken "Claude, …" still needs Haiku to clean it up, so it isn't one.
fn fast(text: &str, ctx: &RouteCtx, spoken: bool) -> Option<Action> {
    if let Some(rest) = text.strip_prefix('/') {
        let (cmd, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let (cmd, args) = (cmd.to_string(), args.trim().to_string());
        if cmd == "resume" {
            return Some(Action::Session(SessionCmd::Resume { hint: (!args.is_empty()).then_some(args) }));
        }
        return Some(slash(cmd, args, ctx));
    }
    if let Some(rest) = forced_prompt(text) {
        return (!spoken).then(|| Action::Prompt { text: rest.to_string() });
    }
    let words = normalize(text);
    if ["stop", "stop it", "dung lai", "dung", "ngung lai"].contains(&words.as_str()) {
        return Some(Action::Session(SessionCmd::Interrupt));
    }
    if ctx.waiting_for_permission {
        const ALWAYS: &[&str] = &["always", "always allow", "yes always", "luon", "luon luon", "luon cho phep", "co luon"];
        const YES: &[&str] = &["yes", "yeah", "yep", "yup", "ok", "okay", "sure", "allow", "go ahead", "yes go ahead", "do it", "co", "u", "uh", "duoc", "dong y", "cho phep", "lam di", "ok di"];
        const NO: &[&str] = &["no", "nope", "nah", "deny", "dont", "don't", "no way", "khong", "dung lam", "tu choi", "thoi"];
        let w = words.as_str();
        if ALWAYS.contains(&w) {
            return Some(Action::Answer { allow: true, always: true });
        }
        if YES.contains(&w) {
            return Some(Action::Answer { allow: true, always: false });
        }
        if NO.contains(&w) {
            return Some(Action::Answer { allow: false, always: false });
        }
    }
    None
}

/// The rest of a message starting with "Claude," (or "Claude ơi"), which always means a prompt.
fn forced_prompt(text: &str) -> Option<&str> {
    let lower = text.to_lowercase();
    let n = ["claude ơi,", "claude ơi", "claude,"].iter().find(|p| lower.starts_with(*p))?.len();
    // `to_lowercase` keeps these prefixes' byte lengths, so `n` is a boundary in `text` too.
    Some(text[n..].trim_start_matches([',', ' ']).trim())
}

fn slash(command: String, args: String, ctx: &RouteCtx) -> Action {
    // No init line yet (or no session): the list is unknown, so send it and let Claude Code answer.
    if ctx.slash_commands.is_empty() || ctx.slash_commands.iter().any(|c| c.trim_start_matches('/') == command) {
        Action::Slash { command, args }
    } else {
        Action::Say(crate::i18n::cant_slash(ctx.language, &command))
    }
}

/// Lowercase, no Vietnamese marks, no punctuation, single spaces.
fn normalize(text: &str) -> String {
    let t: String = fold(&text.to_lowercase()).chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn input(text: &str, ctx: &RouteCtx, spoken: bool) -> String {
    let message = match forced_prompt(text) {
        Some(rest) => format!("[prompt] {rest}"),
        None => text.to_string(),
    };
    let files: Vec<&str> = ctx.project_files.iter().take(300).map(String::as_str).collect();
    format!(
        "Context:\n- permission waiting: {}\n- session running: {}\n- slash commands: {}\n- projects: {}\n- project files: {}\n- {}\n\nMessage:\n{message}",
        ctx.waiting_for_permission,
        ctx.has_session,
        ctx.slash_commands.join(", "),
        ctx.known_projects.join(", "),
        files.join(", "),
        if spoken { "spoken (speech to text, may have mishearings)" } else { "typed" },
    )
}

/// Haiku's JSON → an action. None if it isn't usable, and the caller falls back to chat.
fn parse(out: &str, original: &str, ctx: &RouteCtx, spoken: bool, forced: bool) -> Option<Action> {
    let json = &out[out.find('{')?..=out.rfind('}')?]; // tolerates code fences or a stray sentence
    let v: Value = serde_json::from_str(json).ok()?;
    let s = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let action = if forced { "prompt" } else { v["action"].as_str()? };
    Some(match action {
        "chat" => Action::Chat(original.to_string()),
        "prompt" => {
            let typed = forced_prompt(original).unwrap_or(original).to_string();
            // Typed prompts are sent as typed; only speech gets the cleaned text.
            Action::Prompt { text: if spoken { s("text").unwrap_or(typed) } else { typed } }
        }
        "answer" if ctx.waiting_for_permission => {
            let always = v["always"] == true;
            Action::Answer { allow: v["allow"] == true || always, always }
        }
        "slash" => slash(s("command")?.trim_start_matches('/').to_string(), s("args").unwrap_or_default(), ctx),
        "summarize" => Action::Summarize,
        "session" => Action::Session(match v["cmd"].as_str()? {
            "new" => SessionCmd::New { project: s("hint") },
            "resume" => SessionCmd::Resume { hint: s("hint") },
            "interrupt" | "stop" => SessionCmd::Interrupt,
            "end" => SessionCmd::End,
            _ => return None,
        }),
        "remember" => Action::Remember(s("text")?),
        "forget" => Action::Forget(s("text")?),
        "skill" => Action::Skill(match v["name"].as_str()? {
            "music_on" => Skill::MusicOn,
            "music_off" => Skill::MusicOff,
            "next_song" => Skill::NextSong,
            "coffee" => Skill::Coffee,
            "play" => Skill::Play(match v["game"].as_str() {
                Some("hangman") => Some(Game::Hangman),
                Some("sudoku") => Some(Game::Sudoku),
                _ => None,
            }),
            "tour" => Skill::Tour,
            "hide" => Skill::Hide,
            _ => return None,
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RouteCtx {
        RouteCtx { slash_commands: vec!["compact".into(), "/clear".into()], has_session: true, ..Default::default() }
    }
    fn waiting() -> RouteCtx {
        RouteCtx { waiting_for_permission: true, ..ctx() }
    }

    #[test]
    fn fast_paths() {
        assert_eq!(fast("Claude, fix   the bug", &ctx(), false), Some(Action::Prompt { text: "fix   the bug".into() }));
        assert_eq!(fast("Claude ơi, sửa lỗi này", &ctx(), false), Some(Action::Prompt { text: "sửa lỗi này".into() }));
        assert_eq!(fast("Claude, fix the bug", &ctx(), true), None); // spoken: Haiku cleans it
        assert_eq!(fast("Claudesona, hi", &ctx(), false), None);
        assert_eq!(fast("Stop!", &ctx(), false), Some(Action::Session(SessionCmd::Interrupt)));
        assert_eq!(fast("Dừng lại.", &ctx(), true), Some(Action::Session(SessionCmd::Interrupt)));
        assert_eq!(fast("/compact keep the tests", &ctx(), false), Some(Action::Slash { command: "compact".into(), args: "keep the tests".into() }));
        assert_eq!(fast("/clear", &ctx(), false), Some(Action::Slash { command: "clear".into(), args: "".into() }));
        assert!(matches!(fast("/help", &ctx(), false), Some(Action::Say(s)) if s.contains("/help")));
        assert_eq!(fast("/help", &RouteCtx::default(), false), Some(Action::Slash { command: "help".into(), args: "".into() })); // before the init line
        assert_eq!(fast("/resume blog", &ctx(), false), Some(Action::Session(SessionCmd::Resume { hint: Some("blog".into()) })));
    }

    #[test]
    fn permission_words_only_while_waiting() {
        assert_eq!(fast("yes", &ctx(), true), None);
        assert_eq!(fast("Yes!", &waiting(), true), Some(Action::Answer { allow: true, always: false }));
        assert_eq!(fast("Có", &waiting(), true), Some(Action::Answer { allow: true, always: false }));
        assert_eq!(fast("không", &waiting(), true), Some(Action::Answer { allow: false, always: false }));
        assert_eq!(fast("nope.", &waiting(), false), Some(Action::Answer { allow: false, always: false }));
        assert_eq!(fast("Always", &waiting(), true), Some(Action::Answer { allow: true, always: true }));
        assert_eq!(fast("luôn luôn", &waiting(), true), Some(Action::Answer { allow: true, always: true }));
        assert_eq!(fast("yes but only the tests folder", &waiting(), true), None);
    }

    #[test]
    fn parses_haiku_json() {
        let p = |out: &str, spoken| parse(out, "um add uh dark mode", &ctx(), spoken, false);
        assert_eq!(p(r#"{"action":"prompt","text":"add dark mode"}"#, true), Some(Action::Prompt { text: "add dark mode".into() }));
        assert_eq!(p(r#"{"action":"prompt","text":"add dark mode"}"#, false), Some(Action::Prompt { text: "um add uh dark mode".into() }));
        assert_eq!(p("```json\n{\"action\":\"chat\"}\n```", true), Some(Action::Chat("um add uh dark mode".into())));
        assert_eq!(p(r#"{"action":"slash","command":"/compact"}"#, true), Some(Action::Slash { command: "compact".into(), args: "".into() }));
        assert!(matches!(p(r#"{"action":"slash","command":"help"}"#, true), Some(Action::Say(_))));
        assert_eq!(p(r#"{"action":"summarize"}"#, true), Some(Action::Summarize));
        assert_eq!(p(r#"{"action":"session","cmd":"new","hint":"blog"}"#, true), Some(Action::Session(SessionCmd::New { project: Some("blog".into()) })));
        assert_eq!(p(r#"{"action":"session","cmd":"resume","hint":""}"#, true), Some(Action::Session(SessionCmd::Resume { hint: None })));
        assert_eq!(p(r#"{"action":"session","cmd":"end"}"#, true), Some(Action::Session(SessionCmd::End)));
        assert_eq!(p(r#"{"action":"remember","text":"I study on Tuesdays"}"#, true), Some(Action::Remember("I study on Tuesdays".into())));
        assert_eq!(p(r#"{"action":"forget","text":"Tuesdays"}"#, true), Some(Action::Forget("Tuesdays".into())));
        assert_eq!(p(r#"{"action":"answer","allow":true}"#, true), None); // nothing is waiting
        assert_eq!(parse(r#"{"action":"answer","allow":false,"always":false}"#, "nah", &waiting(), true, false), Some(Action::Answer { allow: false, always: false }));
        assert_eq!(p(r#"{"action":"remember","text":""}"#, true), None);
        assert_eq!(p(r#"{"action":"skill","name":"coffee"}"#, true), Some(Action::Skill(Skill::Coffee)));
        assert_eq!(p(r#"{"action":"skill","name":"play","game":"sudoku"}"#, true), Some(Action::Skill(Skill::Play(Some(Game::Sudoku)))));
        assert_eq!(p(r#"{"action":"skill","name":"play"}"#, true), Some(Action::Skill(Skill::Play(None))));
        assert_eq!(p(r#"{"action":"skill","name":"fly"}"#, true), None);
        assert_eq!(p("not json at all", true), None);
        assert_eq!(p(r#"{"action":"fly"}"#, true), None);
        // "Claude, …" spoken: a prompt whatever Haiku called it
        assert_eq!(parse(r#"{"action":"chat"}"#, "Claude, um, hi there", &ctx(), true, true), Some(Action::Prompt { text: "um, hi there".into() }));
    }
}

#[cfg(test)]
mod live {
    // Calls the real `claude` (your plan): cargo test live_route -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_route_samples() {
        let ctx = super::RouteCtx {
            has_session: true,
            slash_commands: vec!["compact".into(), "clear".into(), "model".into(), "context".into()],
            known_projects: vec!["blog".into(), "claudesona".into()],
            project_files: vec!["src/main.rs".into(), "src/router.rs".into(), "src/settings_page.rs".into()],
            ..Default::default()
        };
        let samples = [
            "um so like add a uh dark mode to the uh settings page dot rs, no wait, settings page",
            "how's your day going?",
            "can you compact the context",
            "start a session in my blog",
            "ờ sửa cái lỗi trong file rao tơ chấm rs giúp tớ",
            "play some music",
            "let's play sudoku",
            "have a coffee, you earned it",
        ];
        for s in samples {
            let (tx, rx) = std::sync::mpsc::channel();
            let t = std::time::Instant::now();
            super::route(s, ctx.clone(), true, move |a| tx.send(a).unwrap());
            let a = rx.recv().unwrap();
            println!("{}ms {s:?}\n  → {a:?}", t.elapsed().as_millis());
        }
    }
}
