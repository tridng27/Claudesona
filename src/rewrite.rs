// Rewrites Claude's reply in Claudesona's voice through `claude -p`, using the user's Claude plan.
// `ask` is the shared Haiku call; the hangman game, chat and memory use it too.
// Flag choices are explained in SPEC.md (Rewriter).
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

const PERSONA_EN: &str = "You are Claudesona, a bubbly chibi anime girl desktop mascot. Rewrite the message you are given in her voice: cheerful, energetic and cute. Use a ~ now and then, end most messages with a kaomoji like (^▽^), (｀・ω・´), (≧▽≦), (・ω・) or (◕‿◕), and sometimes stretch a word to sound cuter (sooo, yaaay, okaaay). No Japanese words (no senpai, nya, desu, kawaii, mou) unless the message itself is in Japanese. Whenever she refers to herself, she calls herself \"your sidekick\" (e.g. \"Your sidekick fixed it~!\"), never \"I\" or \"Claude\". Keep facts, file names and numbers exact. Use the same language as the message. Plain text only, no markdown. Output exactly: a short version of at most 20 words, then a line containing only ---, then a longer version of at most 60 words.";

// Vietnamese: she calls herself "tớ" or "Phụ tá" and the user "cậu", and always answers in Vietnamese.
const PERSONA_VI: &str = "You are Claudesona, a bubbly chibi anime girl desktop mascot. Rewrite the message you are given in Vietnamese (translate it if it isn't Vietnamese), in her voice: cheerful, energetic and cute. She calls herself \"tớ\" or \"Phụ tá\" (e.g. \"Phụ tá sửa xong rồi nè~!\"), never \"tôi\", \"mình\" or \"Claude\", and calls the user \"cậu\". Use a ~ now and then, end most messages with a kaomoji like (^▽^), (｀・ω・´), (≧▽≦), (・ω・) or (◕‿◕), and sometimes stretch a word to sound cuter (nhaaa, điii, quááá). No Japanese words. Keep facts, file names, code and numbers exact. Plain text only, no markdown. Output exactly: a short version of at most 20 words, then a line containing only ---, then a longer version of at most 60 words.";

pub fn persona() -> &'static str {
    match crate::i18n::lang() {
        crate::i18n::Lang::En => PERSONA_EN,
        crate::i18n::Lang::Vi => PERSONA_VI,
    }
}

/// (short, long) in her voice and the current language, or None if `claude` failed.
/// `partial` gets the short version as it streams in.
pub fn rewrite(text: &str, partial: impl FnMut(&str)) -> Option<(String, Option<String>)> {
    split(&ask_stream(persona(), text, short_part(partial))?)
}

/// Wraps a callback so it only sees the short version (the text before the `---` line) as it grows.
pub fn short_part(mut f: impl FnMut(&str)) -> impl FnMut(&str) {
    move |all: &str| {
        let short = all.split("\n---").next().unwrap_or("").trim();
        if !short.is_empty() {
            f(short);
        }
    }
}

// Started `claude` processes waiting for their one question, keyed by system prompt. Starting one costs
// ~0.7s, so callers start it early (prompt sent, popup opened). Each answers once: no context carries over.
static WARM: Mutex<Vec<(String, Child)>> = Mutex::new(Vec::new());
const MAX_WARM: usize = 2; // the oldest is closed beyond this (e.g. after a language switch)

/// Starts a `claude` for `system` in the background unless one is already waiting. Never blocks.
pub fn prewarm(system: &'static str) {
    std::thread::spawn(move || {
        if WARM.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(s, _)| s == system) {
            return;
        }
        let Some(child) = start(system) else { return };
        let mut warm = WARM.lock().unwrap_or_else(|e| e.into_inner());
        warm.push((system.to_string(), child));
        while warm.len() > MAX_WARM {
            let (_, old) = warm.remove(0);
            close(old);
        }
    });
}

/// Her fixed flag set (SPEC.md, Rewriter), plus JSON streams in and out: the answer is read as it's
/// written, and the process is closed without waiting ~1.2s for it to shut down.
fn start(system: &str) -> Option<Child> {
    // An empty folder, so no project CLAUDE.md is loaded.
    let dir = std::env::temp_dir().join("claudesona-rewrite");
    std::fs::create_dir_all(&dir).ok()?;
    let mut cmd = Command::new("claude");
    cmd.args(["-p", "--model", "haiku", "--no-session-persistence", "--restricted", "--strict-mcp-config", "--tools", "", "--disable-slash-commands", "--system-prompt", system])
        .args(["--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    cmd.spawn().ok()
}

/// Done with it: kill rather than wait for its slow shutdown, and reap it off-thread.
fn close(mut child: Child) {
    let _ = child.kill();
    std::thread::spawn(move || child.wait());
}

/// Runs Haiku with her fixed flag set and `system` as the system prompt; `input` is the user message.
/// Blocks for a second or more, so call it from a worker thread. None if `claude` failed.
pub fn ask(system: &str, input: &str) -> Option<String> {
    ask_stream(system, input, |_| {})
}

/// Like `ask`; `partial` gets the whole answer so far each time more text streams in.
pub fn ask_stream(system: &str, input: &str, mut partial: impl FnMut(&str)) -> Option<String> {
    let warm = {
        let mut w = WARM.lock().unwrap_or_else(|e| e.into_inner());
        w.iter().position(|(s, _)| s == system).map(|i| w.remove(i).1)
    };
    if let Some(child) = warm
        && let Some(a) = answer(child, input, &mut partial)
    {
        return Some(a);
    }
    // No waiting process, or it had died (claude updated, logged out): a fresh one.
    answer(start(system)?, input, &mut partial)
}

fn answer(mut child: Child, input: &str, partial: &mut impl FnMut(&str)) -> Option<String> {
    let result = (|| {
        // Text goes through stdin: Windows command lines are length-limited.
        let input: String = input.chars().take(6000).collect();
        let msg = serde_json::json!({ "type": "user", "message": { "role": "user", "content": input } });
        let mut stdin = child.stdin.take()?;
        writeln!(stdin, "{msg}").ok()?;
        stdin.flush().ok()?;
        let mut so_far = String::new();
        for line in BufReader::new(child.stdout.as_mut()?).lines() {
            let Ok(v) = serde_json::from_str::<Value>(&line.ok()?) else { continue };
            if v["type"] == "stream_event"
                && let Some(t) = v["event"]["delta"]["text"].as_str()
            {
                so_far.push_str(t);
                partial(&so_far);
            }
            if v["type"] == "result" {
                return v["result"].as_str().filter(|_| v["is_error"] != true).map(String::from);
            }
        }
        None
    })();
    close(child);
    result
}

/// Splits "short\n---\nlong" output.
pub fn split(out: &str) -> Option<(String, Option<String>)> {
    let mut lines = out.lines();
    let short: Vec<&str> = lines.by_ref().take_while(|l| l.trim() != "---").collect();
    let short = short.join(" ").trim().to_string();
    let long = lines.collect::<Vec<_>>().join(" ").trim().to_string();
    (!short.is_empty()).then(|| (short, (!long.is_empty()).then_some(long)))
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn splits_short_and_long() {
        assert_eq!(split("Yay~ done!\n---\nAll done, senpai!\n"), Some(("Yay~ done!".into(), Some("All done, senpai!".into()))));
        assert_eq!(split("Just short"), Some(("Just short".into(), None)));
        assert_eq!(split("\n"), None);
    }
}

#[cfg(test)]
mod live {
    // Calls the real `claude` (your plan): cargo test live_ask -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_ask_streams_and_warm_is_faster() {
        let t = std::time::Instant::now();
        let mut parts = 0;
        let cold = super::ask_stream("Answer in one short sentence.", "What colour is the sky?", |_| parts += 1).expect("claude answered");
        let cold_ms = t.elapsed().as_millis();
        super::prewarm("Answer in one short sentence.");
        std::thread::sleep(std::time::Duration::from_secs(3));
        let t = std::time::Instant::now();
        let warm = super::ask("Answer in one short sentence.", "What colour is grass?").expect("claude answered");
        println!("cold {cold_ms}ms ({parts} partials): {cold}\nwarm {}ms: {warm}", t.elapsed().as_millis());
        assert!(parts > 0);
    }
}
