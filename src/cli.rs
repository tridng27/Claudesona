// Command-line modes: `hook` (run by Claude Code hooks), `install` and `uninstall`.
// The running app listens on ADDR; that port is also its single-instance lock.
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

pub const ADDR: &str = "127.0.0.1:47827";
const EVENTS: [&str; 5] = ["SessionStart", "SessionEnd", "Notification", "PermissionRequest", "PreToolUse"];

pub fn send(msg: &str) -> std::io::Result<()> {
    let mut s = TcpStream::connect_timeout(&ADDR.parse().unwrap(), Duration::from_millis(300))?;
    s.write_all(msg.as_bytes())
}

/// Run by Claude Code hooks: forwards the hook JSON to the app, starting the app if it isn't running.
/// Prints nothing: SessionStart hook output would be added to Claude's context, and PermissionRequest output could answer the prompt.
pub fn hook() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let input = input.trim_start_matches('\u{feff}'); // some shells (PowerShell) prefix piped text with a UTF-8 BOM
    if send(input).is_err() {
        let is_start = serde_json::from_str::<Value>(input).is_ok_and(|v| v["hook_event_name"] == "SessionStart");
        if is_start && let Ok(me) = std::env::current_exe() {
            launch(&me);
        }
    }
}

/// The user's home folder (where `.claude` lives).
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default())
}

/// Where `install` puts her. `skin` and `music` folders go next to the exe.
fn install_dir() -> Result<PathBuf, String> {
    if cfg!(windows) {
        let local = std::env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA isn't set")?;
        Ok(PathBuf::from(local).join("Programs").join("Claudesona"))
    } else if cfg!(target_os = "macos") {
        Ok(home().join("Library").join("Application Support").join("Claudesona"))
    } else {
        Ok(home().join(".local").join("share").join("claudesona"))
    }
}

/// Starts the app detached from the hook (its own process group, no shared pipes), so it outlives the hook
/// and Claude Code doesn't wait on it.
#[cfg(not(windows))]
fn launch(exe: &PathBuf) {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let _ = Command::new(exe).process_group(0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Starts the app fully detached, so it outlives the hook process.
#[cfg(windows)]
fn launch(exe: &PathBuf) {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    const DETACHED: u32 = 0x0000_0008 | 0x0000_0200; // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    const BREAKAWAY: u32 = 0x0100_0000; // leave Claude Code's job object, if it allows that
    // No inherited pipes: Claude Code waits for the hook's output pipe to close, which would never happen
    // while she runs. Windows passes every inheritable handle to children, so un-mark ours first.
    unsafe {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
        for std in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetHandleInformation(GetStdHandle(std), HANDLE_FLAG_INHERIT, 0);
        }
    }
    let spawn =|flags| Command::new(exe).creation_flags(flags).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if spawn(DETACHED | BREAKAWAY).is_err() {
        let _ = spawn(DETACHED);
    }
}

fn settings_path() -> Result<PathBuf, String> {
    let home = home();
    if home.as_os_str().is_empty() {
        return Err("Couldn't find your home folder.".into());
    }
    Ok(home.join(".claude").join("settings.json"))
}

/// Removes our hook entries, then adds them back for `cmd` (if given). Other settings are untouched.
pub fn set_hooks(settings: &mut Value, cmd: Option<&str>) {
    if !settings.is_object() {
        *settings = json!({});
    }
    let hooks = settings.as_object_mut().unwrap().entry("hooks").or_insert_with(|| json!({}));
    let Some(map) = hooks.as_object_mut() else { return };
    for event in EVENTS {
        if let Some(list) = map.get_mut(event).and_then(Value::as_array_mut) {
            list.retain(|entry| !entry.to_string().contains("claudesona"));
        }
        if let Some(cmd) = cmd {
            let list = map.entry(event).or_insert_with(|| json!([]));
            if let Some(list) = list.as_array_mut() {
                let mut entry = json!({ "hooks": [{ "type": "command", "command": cmd }] });
                // Shell commands only: the log gets their line after they finish, so this is her only early signal.
                if event == "PreToolUse" {
                    entry["matcher"] = json!("Bash|PowerShell");
                }
                list.push(entry);
            }
        }
        if map.get(event).and_then(Value::as_array).is_some_and(|l| l.is_empty()) {
            map.remove(event);
        }
    }
    if map.is_empty() {
        settings.as_object_mut().unwrap().remove("hooks");
    }
}

fn update_settings(cmd: Option<&str>) -> Result<(), String> {
    let path = settings_path()?;
    let mut settings = match std::fs::read_to_string(&path) {
        Ok(text) => {
            // Refuse to touch a settings file we can't parse rather than overwrite it.
            let v = serde_json::from_str(&text).map_err(|e| format!("{} isn't valid JSON ({e}); fix it and try again.", path.display()))?;
            std::fs::copy(&path, path.with_extension("json.claudesona-backup")).map_err(|e| e.to_string())?;
            v
        }
        Err(_) => json!({}),
    };
    set_hooks(&mut settings, cmd);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|e| e.to_string())
}

/// Copies this exe to the install folder, adds the hooks and starts her.
pub fn install() -> Result<String, String> {
    let dir = install_dir()?;
    let target = dir.join(if cfg!(windows) { "claudesona.exe" } else { "claudesona" });
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if me != target {
        let _ = send(r#"{"quit":true}"#); // an older copy may be running and holding the file
        std::thread::sleep(Duration::from_millis(800));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::copy(&me, &target).map_err(|e| format!("Couldn't copy to {}: {e}", target.display()))?;
    }
    let cmd = format!("\"{}\" hook", target.display().to_string().replace('\\', "/"));
    update_settings(Some(&cmd))?;
    launch(&target);
    Ok(format!(
        "Claudesona is installed at\n{}\n\nShe starts with every new Claude Code session. Sessions already open need a restart.\nRewrites need the `claude` command on PATH.",
        target.display()
    ))
}

pub fn uninstall() -> Result<String, String> {
    update_settings(None)?;
    let _ = send(r#"{"quit":true}"#);
    Ok(format!("Claudesona's hooks are removed and she's closed.\nYou can delete {}.", install_dir()?.display()))
}

/// macOS and Linux: install and uninstall run in a terminal, so results go there.
#[cfg(not(windows))]
pub fn report(result: Result<String, String>) {
    match result {
        Ok(t) => println!("{t}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// The Windows exe has no console, so results are shown in a message box.
#[cfg(windows)]
pub fn report(result: Result<String, String>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONINFORMATION, MessageBoxW};
    let (text, icon) = match result {
        Ok(t) => (t, MB_ICONINFORMATION),
        Err(e) => (e, MB_ICONERROR),
    };
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(&text).as_ptr(), wide("Claudesona").as_ptr(), icon) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_are_added_once_and_removed_cleanly() {
        let other = json!({ "type": "command", "command": "other.exe" });
        let mut s = json!({ "theme": "dark", "hooks": { "Notification": [{ "hooks": [other] }] } });
        set_hooks(&mut s, Some("\"C:/x/claudesona.exe\" hook"));
        set_hooks(&mut s, Some("\"C:/x/claudesona.exe\" hook")); // installing twice adds nothing
        assert_eq!(s["hooks"]["Notification"].as_array().unwrap().len(), 2);
        assert_eq!(s["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(s["hooks"]["PermissionRequest"].as_array().unwrap().len(), 1);
        assert_eq!(s["hooks"]["PreToolUse"][0]["matcher"], "Bash|PowerShell");
        set_hooks(&mut s, None);
        assert_eq!(s, json!({ "theme": "dark", "hooks": { "Notification": [{ "hooks": [other] }] } }));
        let mut empty = json!({});
        set_hooks(&mut empty, Some("claudesona.exe hook"));
        set_hooks(&mut empty, None);
        assert_eq!(empty, json!({}));
    }
}
