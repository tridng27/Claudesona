// A hosted Claude Code session: one `claude -p` process in stream-json mode (SPEC-v2.md, Hosted sessions).
// No UI here: events go to a callback on the reader thread; the format itself lives in src/stream.rs.
use crate::stream::{self, Event};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Duration;

pub struct Session {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    requests: u32,
}

impl Session {
    /// Starts `claude` in `dir` and a reader thread that calls `on_event` for each event, then Exit.
    /// Permission prompts come as Event::Permission (`--permission-prompt-tool stdio`). `mcp_config` is her
    /// services file (`--mcp-config`); `strict_mcp` leaves out your own MCP servers (`--strict-mcp-config`).
    pub fn start(dir: &Path, resume: Option<&str>, model: Option<&str>, mcp_config: Option<&Path>, strict_mcp: bool, mut on_event: impl FnMut(Event) + Send + 'static) -> io::Result<Session> {
        let mut cmd = Command::new("claude");
        cmd.args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]);
        cmd.args(["--permission-prompt-tool", "stdio"]);
        if let Some(cfg) = mcp_config {
            cmd.arg("--mcp-config").arg(cfg);
        }
        if strict_mcp {
            cmd.arg("--strict-mcp-config");
        }
        if let Some(id) = resume {
            cmd.args(["--resume", id]);
        }
        if let Some(m) = model {
            cmd.args(["--model", m]);
        }
        cmd.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
        }
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().ok_or(io::ErrorKind::BrokenPipe)?;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
                stream::parse_line(&String::from_utf8_lossy(&line)).into_iter().for_each(&mut on_event);
            }
            on_event(Event::Exit);
        });
        Ok(Session { stdin: child.stdin.take(), child: Some(child), requests: 0 })
    }

    /// Sends a message, or a slash command as text. Sent mid-turn, it's queued for the next turn.
    pub fn send(&mut self, text: &str) -> io::Result<()> {
        self.write(&stream::user_message(text))
    }

    /// Stops the current turn (a Result with `interrupted` follows); the session stays usable.
    pub fn interrupt(&mut self) -> io::Result<()> {
        self.requests += 1;
        self.write(&stream::interrupt(&format!("interrupt-{}", self.requests)))
    }

    /// Answers a Permission event. `input` is the tool input from that event.
    pub fn allow(&mut self, request_id: &str, input: &Value) -> io::Result<()> {
        self.write(&stream::permission_answer(request_id, Some(input), ""))
    }

    pub fn deny(&mut self, request_id: &str, message: &str) -> io::Result<()> {
        self.write(&stream::permission_answer(request_id, None, message))
    }

    /// Stops the turn and closes the session; it's saved and can be resumed. Same as dropping it.
    pub fn end(self) {}

    fn write(&mut self, line: &Value) -> io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
        writeln!(stdin, "{line}")?;
        stdin.flush()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.interrupt();
        self.stdin = None; // end of input: claude saves and exits by itself
        let Some(mut child) = self.child.take() else { return };
        std::thread::spawn(move || {
            for _ in 0..50 {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = child.kill();
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// Runs the real `claude` (uses the plan's limits): cargo test session -- --ignored
    #[test]
    #[ignore]
    fn live_session() {
        let dir = std::env::temp_dir().join("claudesona-session-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "The secret word is kiwi.\n").unwrap();
        let (tx, rx) = channel();
        let mut s = Session::start(&dir, None, Some("haiku"), None, false, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let wait = |want: &dyn Fn(&Event) -> bool| loop {
            let e = rx.recv_timeout(Duration::from_secs(90)).expect("no event");
            println!("{e:?}");
            if want(&e) {
                return e;
            }
        };
        s.send("Use the Write tool to create out.txt containing the secret word from notes.txt.").unwrap();
        let Event::Permission { request_id, input, .. } = wait(&|e| matches!(e, Event::Permission { .. })) else { unreachable!() };
        s.allow(&request_id, &input).unwrap();
        wait(&|e| matches!(e, Event::Result { .. }));
        assert_eq!(std::fs::read_to_string(dir.join("out.txt")).unwrap().trim(), "kiwi");
        s.send("Write the numbers 1 to 300 in words, one per line. No tools.").unwrap();
        wait(&|e| matches!(e, Event::TextDelta(_)));
        s.interrupt().unwrap();
        assert!(matches!(wait(&|e| matches!(e, Event::Result { .. })), Event::Result { interrupted: true, .. }));
        s.end();
        wait(&|e| *e == Event::Exit);
    }
}
