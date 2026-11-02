// Rewrites Claude's reply in Claudesona's voice through `claude -p`, using the user's Claude plan.
// Flag choices are explained in SPEC.md (Rewriter).
use std::io::Write;
use std::process::{Command, Stdio};

const PERSONA: &str = "You are Claudesona, a genki chibi anime girl desktop mascot. Rewrite the message you are given in her voice: cheerful, energetic and cute, with an occasional ~ or kaomoji. Keep facts, file names and numbers exact. Use the same language as the message. Plain text only, no markdown. Output exactly: a short version of at most 20 words, then a line containing only ---, then a longer version of at most 60 words.";

/// (short, long) in her voice, or None if `claude` failed.
pub fn rewrite(text: &str) -> Option<(String, Option<String>)> {
    // An empty folder, so no project CLAUDE.md is loaded.
    let dir = std::env::temp_dir().join("claudesona-rewrite");
    std::fs::create_dir_all(&dir).ok()?;
    let mut cmd = Command::new("claude");
    cmd.args(["-p", "--model", "haiku", "--no-session-persistence", "--restricted", "--strict-mcp-config", "--tools", "", "--disable-slash-commands", "--system-prompt", PERSONA])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    let mut child = cmd.spawn().ok()?;
    // Text goes through stdin: Windows command lines are length-limited.
    let input: String = text.chars().take(6000).collect();
    child.stdin.take()?.write_all(input.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    split(&String::from_utf8_lossy(&out.stdout))
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
