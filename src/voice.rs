// Her voice. Fixed lines are ElevenLabs mp3s made once ahead of time (free to play). Chat answers and Claude's
// rewritten replies are spoken live, which needs the user's API key and costs credits each time. See SPEC.md, "Voice".
// Bundled files (assets/voice/, compiled in by build.rs) play by default. A user who makes their own voice gets
// a `voice` folder next to the exe, which then replaces the bundled voice entirely (voices are never mixed).
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MODEL: &str = "eleven_flash_v2_5"; // half the credits of the other models, and it speaks Vietnamese
const FORMAT: &str = "mp3_44100_64";
const COOLDOWN: Duration = Duration::from_secs(30); // chatter (not attention lines) at most this often
const RECENT: usize = 6; // chatter lines heard this recently aren't voiced again (the bubble still shows)
const ID_FILE: &str = "voice_id.txt"; // in a user voice folder: whose voice it is; also marks it as in use

static BUNDLED: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/voice.rs"));
const BUNDLED_ID: &str = include!(concat!(env!("OUT_DIR"), "/voice_id.rs")); // whose voice is bundled ("" if none)
const KEY_FILE: &str = "voice_key.txt"; // next to the exe; kept out of git (.gitignore)
static ON: AtomicBool = AtomicBool::new(true);
static CUSTOM: AtomicBool = AtomicBool::new(false); // the user's own voice is in use (checked at startup, after making or removing it)
static KEY: Mutex<String> = Mutex::new(String::new()); // the saved ElevenLabs key, for live speech ("" if none)
static CUSTOM_ID: Mutex<String> = Mutex::new(String::new()); // the user's own voice ID, if they made one
// When she last spoke and what (chatter waits COOLDOWN after any line, and skips the last RECENT ones).
static SPOKEN: Mutex<(Option<Instant>, Vec<String>)> = Mutex::new((None, Vec::new()));
static PROGRESS: Mutex<String> = Mutex::new(String::new()); // generation status for the popup

pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn progress() -> String {
    PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| Some(p.parent()?.to_path_buf())).unwrap_or_default()
}

fn user_dir() -> PathBuf {
    exe_dir().join("voice")
}

/// Who should speak: an attention line (Claude needs you), the tour, or anything else (chatter).
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Attention, // always plays, and restarts the charge
    Tour,      // always plays, and leaves the charge alone (you asked for the tour)
    Chatter,   // needs the charge, uses it up
}

/// Checks once at startup whether the user's own voice is there. Reads the disk: call off the UI thread.
pub fn init() {
    let id = std::fs::read_to_string(user_dir().join(ID_FILE)).unwrap_or_default();
    CUSTOM.store(!id.is_empty(), Ordering::Relaxed);
    *CUSTOM_ID.lock().unwrap_or_else(|e| e.into_inner()) = id.trim().to_string();
    *KEY.lock().unwrap_or_else(|e| e.into_inner()) = std::fs::read_to_string(exe_dir().join(KEY_FILE)).unwrap_or_default().trim().to_string();
}

/// A key is saved, so replies and chat can be spoken live.
pub fn has_key() -> bool {
    !KEY.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// Saves the ElevenLabs key next to the exe (plain text, like memory.json), or removes it when empty.
pub fn save_key(key: &str) {
    let key = key.trim().to_string();
    let path = exe_dir().join(KEY_FILE);
    let _ = if key.is_empty() { std::fs::remove_file(&path) } else { std::fs::write(&path, &key) };
    *KEY.lock().unwrap_or_else(|e| e.into_inner()) = key;
}

/// The voice ID in use: the user's own, else the bundled one.
fn voice_id() -> String {
    if custom() { CUSTOM_ID.lock().unwrap_or_else(|e| e.into_inner()).clone() } else { BUNDLED_ID.trim().to_string() }
}

/// The voice in use: the user's own if they made one, else the bundled one.
pub fn custom() -> bool {
    CUSTOM.load(Ordering::Relaxed)
}

/// Her voice is charged: on, there's a voice to speak with, and 30s have passed since she last spoke.
/// She shows it with sound waves; the next bubble with a recorded line plays and uses it up.
pub fn ready() -> bool {
    let (last, _) = &*SPOKEN.lock().unwrap_or_else(|e| e.into_inner());
    ON.load(Ordering::Relaxed) && (custom() || !BUNDLED.is_empty()) && last.is_none_or(|l| l.elapsed() >= COOLDOWN)
}

/// Whether `text` has a recording in the voice in use.
fn has(text: &str) -> bool {
    let name = file_name(text);
    if custom() { user_dir().join(format!("{name}.mp3")).exists() } else { BUNDLED.iter().any(|(n, _)| *n == name) }
}

/// What the voice actually reads: kaomoji, ~ and symbols removed (they'd be read out or stumbled over).
pub fn spoken(text: &str) -> String {
    let latin = |c: char| c.is_ascii_alphanumeric() || matches!(c as u32, 0xC0..=0x24F | 0x1E00..=0x1EFF); // incl. Vietnamese
    let plain = |c: char| latin(c) || c.is_whitespace() || ".,!?'’…-".contains(c);
    let mut out = String::new();
    let mut rest = text;
    // A bracketed group with anything but words in it is a kaomoji: "(｀・ω・´)", "(o^▽^o)".
    while let Some(open) = rest.find('(') {
        let close = rest[open..].find(')').map(|c| open + c);
        out.push_str(&rest[..open]);
        match close {
            Some(c) if !rest[open + 1..c].chars().all(plain) => rest = &rest[c + 1..],
            Some(c) => {
                out.push_str(&rest[open..=c]);
                rest = &rest[c + 1..];
            }
            None => {
                rest = &rest[open..];
                break;
            }
        }
    }
    out.push_str(rest);
    let kept: String = out.chars().filter(|&c| plain(c) || c == '(' || c == ')').collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// File name for a line: a stable hash of what's spoken, so editing a line only remakes that line.
pub fn file_name(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a
    for b in spoken(text).bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The mp3 for `text` in the voice in use. Reads a file: call off the UI thread (the music player thread does).
pub fn load(text: &str) -> Option<Vec<u8>> {
    let name = file_name(text);
    if custom() {
        return std::fs::read(user_dir().join(format!("{name}.mp3"))).ok();
    }
    BUNDLED.iter().find(|(n, _)| *n == name).map(|(_, b)| b.to_vec())
}

/// Whether to voice `text` now (it has a recording). Attention lines (Claude needs you) always play; chatter
/// needs her voice charged and skips lines heard recently. Playing anything restarts the charge.
fn should_play(text: &str, attention: bool, last: &mut Option<Instant>, recent: &mut Vec<String>) -> bool {
    if !ON.load(Ordering::Relaxed) {
        return false;
    }
    let name = file_name(text);
    if !attention && (last.is_some_and(|l| l.elapsed() < COOLDOWN) || recent.contains(&name)) {
        return false;
    }
    *last = Some(Instant::now());
    recent.push(name);
    if recent.len() > RECENT {
        recent.remove(0);
    }
    true
}

/// Speaks `text` if it's one of her recorded lines and `mode` allows it. Never blocks (a file check at most).
pub fn say(text: &str, mode: Mode) {
    if !has(text) {
        return;
    }
    if mode == Mode::Tour {
        if ON.load(Ordering::Relaxed) {
            crate::music::speak(text.to_string());
        }
        return;
    }
    let (last, recent) = &mut *SPOKEN.lock().unwrap_or_else(|e| e.into_inner());
    if should_play(text, mode == Mode::Attention, last, recent) {
        crate::music::speak(text.to_string());
    }
}

/// Speaks new text (a chat answer, a rewritten reply) through ElevenLabs right now. `always` ignores the charge
/// (chat); otherwise it needs her voice charged and uses it up. Costs credits; needs a saved key. Never blocks.
pub fn say_live(text: &str, always: bool) {
    let key = KEY.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (id, line) = (voice_id(), spoken(text));
    if !ON.load(Ordering::Relaxed) || key.is_empty() || id.is_empty() || line.is_empty() {
        return;
    }
    {
        let (last, _) = &mut *SPOKEN.lock().unwrap_or_else(|e| e.into_inner());
        if !always && last.is_some_and(|l| l.elapsed() < COOLDOWN) {
            return;
        }
        *last = Some(Instant::now()); // taken now, so a second reply right after doesn't also speak
    }
    let lang = match crate::i18n::lang() {
        crate::i18n::Lang::En => "en",
        crate::i18n::Lang::Vi => "vi",
    };
    std::thread::spawn(move || {
        if let Ok(mp3) = tts(&key, &id, &line, lang) {
            crate::music::speak_mp3(mp3);
        }
    });
}

/// Makes every voiced line with ElevenLabs into `dir` (skipping ones already there for this voice).
/// Blocks for minutes: run on a worker thread. Progress and errors go to `progress()`.
pub fn generate(key: &str, voice_id: &str, dir: &Path) -> Result<usize, String> {
    let set = |s: String| *PROGRESS.lock().unwrap_or_else(|e| e.into_inner()) = s;
    // The voice id goes into the URL: only allow what ElevenLabs ids are made of.
    if voice_id.is_empty() || !voice_id.chars().all(|c| c.is_ascii_alphanumeric()) || key.trim().is_empty() {
        let e = crate::i18n::ui().voice_need.to_string();
        set(e.clone());
        return Err(e);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // A different voice than last time: start over, so voices never mix.
    let id_file = dir.join(ID_FILE);
    if std::fs::read_to_string(&id_file).ok().as_deref() != Some(voice_id) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.path().extension().is_some_and(|x| x == "mp3") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let lines = crate::i18n::voiced();
    let mut made = 0;
    for (i, (text, lang)) in lines.iter().enumerate() {
        set(crate::i18n::ui().voice_making.replace("{i}", &(i + 1).to_string()).replace("{n}", &lines.len().to_string()));
        let out = dir.join(format!("{}.mp3", file_name(text)));
        if out.exists() {
            continue;
        }
        let mp3 = tts(key, voice_id, &spoken(text), lang).inspect_err(|e| set(e.clone()))?;
        std::fs::write(&out, mp3).map_err(|e| e.to_string())?;
        made += 1;
    }
    std::fs::write(&id_file, voice_id).map_err(|e| e.to_string())?;
    set(crate::i18n::ui().voice_done.replace("{n}", &lines.len().to_string()));
    Ok(made)
}

/// One line through the ElevenLabs API, as mp3 bytes. Uses curl (like the embedding model download), with the
/// key passed on stdin rather than the command line, where other programs could see it.
fn tts(key: &str, voice_id: &str, text: &str, lang: &str) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let body = serde_json::json!({ "text": text, "model_id": MODEL, "language_code": lang }).to_string();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let tmp = std::env::temp_dir().join(format!("claudesona-tts-{stamp}"));
    let body_file = tmp.with_extension("json");
    std::fs::write(&body_file, body).map_err(|e| e.to_string())?;
    let part = tmp.with_extension("part");
    let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice_id}?output_format={FORMAT}");
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-sS", "--fail-with-body", "-X", "POST", "-H", "Content-Type: application/json", "-K", "-", "--data-binary"])
        .arg(format!("@{}", body_file.display()))
        .arg("-o")
        .arg(&part)
        .arg(url)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|_| "curl isn't installed, so she can't reach ElevenLabs.".to_string())?;
    let config = format!("header = \"xi-api-key: {}\"\n", key.trim().replace(['"', '\\', '\n', '\r'], ""));
    let _ = child.stdin.take().map(|mut s| s.write_all(config.as_bytes()));
    let status = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&body_file);
    if !status.status.success() {
        // --fail-with-body leaves ElevenLabs' error message in the output file.
        let why = std::fs::read_to_string(&part).unwrap_or_default();
        let _ = std::fs::remove_file(&part);
        let detail = serde_json::from_str::<serde_json::Value>(&why).ok().and_then(|v| v["detail"]["message"].as_str().map(String::from));
        return Err(format!("ElevenLabs said no: {}", detail.unwrap_or_else(|| "check the API key and voice ID.".into())));
    }
    let mp3 = std::fs::read(&part).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&part);
    mp3
}

/// Starts making the user's own voice in the background, with the saved key.
pub fn make_custom(voice_id: String) {
    let key = KEY.lock().unwrap_or_else(|e| e.into_inner()).clone();
    std::thread::spawn(move || {
        let id = voice_id.trim().to_string();
        if generate(&key, &id, &user_dir()).is_ok() {
            *CUSTOM_ID.lock().unwrap_or_else(|e| e.into_inner()) = id;
            CUSTOM.store(true, Ordering::Relaxed);
        }
    });
}

/// Goes back to the bundled voice: removes the user's voice folder.
pub fn reset_custom() {
    CUSTOM.store(false, Ordering::Relaxed);
    let _ = std::fs::remove_dir_all(user_dir());
    PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_kaomoji_and_symbols_but_keeps_words() {
        assert_eq!(spoken("I need your OK~! (｀・ω・´)"), "I need your OK!");
        assert_eq!(spoken("Sooo quiet… shall we build something fun? (o^▽^o)"), "Sooo quiet… shall we build something fun?");
        assert_eq!(spoken("Bugs to squash? ✧ Just say the word~!"), "Bugs to squash? Just say the word!");
        assert_eq!(spoken("Cậu ơiii~ có việc gì cho tớ làm không? (^▽^)"), "Cậu ơiii có việc gì cho tớ làm không?");
        assert_eq!(spoken("Words (in brackets) stay"), "Words (in brackets) stay");
        assert_eq!(file_name("Hi~ (◕‿◕)"), file_name("Hi"));
    }

    #[test]
    fn chatter_waits_but_attention_always_plays() {
        let (mut last, mut recent) = (None, vec![]);
        assert!(should_play("Hello there", false, &mut last, &mut recent));
        assert!(!should_play("Something else", false, &mut last, &mut recent)); // inside the cooldown
        assert!(should_play("I need your OK!", true, &mut last, &mut recent));
        last = Some(Instant::now() - COOLDOWN * 2);
        assert!(!should_play("Hello there", false, &mut last, &mut recent)); // heard recently
        assert!(should_play("Something else", false, &mut last, &mut recent));
    }

    #[test]
    fn every_voiced_line_has_something_to_say() {
        let lines = crate::i18n::voiced();
        assert!(lines.len() > 50);
        println!("{} voiced lines, {} characters", lines.len(), lines.iter().map(|(l, _)| spoken(l).chars().count()).sum::<usize>());
        for (text, lang) in lines {
            assert!(!spoken(text).is_empty() && !text.contains('{') && (lang == "en" || lang == "vi"), "{text}");
        }
    }
}
