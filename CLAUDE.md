# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Claudesona is a Rust desktop mascot for Windows, macOS and Linux: a chibi anime girl who mirrors Claude Code's terminal work as speech bubbles. It's developed on Windows; macOS and Linux are built and tested by the GitHub Actions workflow (`.github/workflows/build.yml`) and haven't been run on real machines yet.
- `SPEC.md` holds the product decisions (phases, bubbles, minis, interaction, rewriter flags). Read it before changing behavior, and update it when a decision changes.
- `SPEC-v2.md` (gitignored, local only) holds v2: she hosts Claude Code sessions herself, takes voice input, manages MCP servers, and keeps a larger memory. v2 is built on `main`; where the two specs disagree, v2 wins.
- `README.md` covers install, uninstall and deploying to other machines.

Status as of 2026-11-02: everything in SPEC.md works on Windows except WSL support. Turn logic:
- A final reply is an assistant `text` block whose `stop_reason` isn't `tool_use`.
- A `system` line with subtype `turn_duration` ends the turn.
- She stays in Working until the rewrite lands.

## Commands

```
cargo run --release               # start the mascot from source
cargo test                        # all tests
cargo test <name>                 # tests whose name contains <name>
cargo build --release             # target\release\claudesona.exe is the single file to ship
target\release\claudesona.exe install     # copy to %LOCALAPPDATA%\Programs\Claudesona, add hooks, start
target\release\claudesona.exe uninstall   # remove her hooks from ~/.claude/settings.json, close her
```

- `cargo` lives in `~/.cargo/bin`, which isn't on Git Bash's PATH. Use PowerShell or the full path.
- Type-check macOS and Linux from Windows with `cargo check --target aarch64-apple-darwin` and `cargo check --target x86_64-unknown-linux-gnu` (targets installed with rustup). The Linux check needs `PKG_CONFIG` pointing at a stand-in that prints `-lasound` and `PKG_CONFIG_ALLOW_CROSS=1`, because ALSA's build script looks for its headers; checking never links.
- Close a running `claudesona.exe` before rebuilding, because Windows locks the file. `install` closes the installed copy by itself.
- To test the hook path, pipe JSON into `claudesona.exe hook` using a real stdin pipe (e.g. .NET `Process` with `RedirectStandardInput`). PowerShell's `|` doesn't feed windowed exes.

## Architecture

One exe, several modes (`src/main.rs` dispatches on the first argument):
- **App** (no argument): the egui window. Binding `127.0.0.1:47827` is both its IPC channel and its single-instance lock; a second copy exits quietly.
- **`hook`** (`src/cli.rs`): run by Claude Code's `SessionStart`, `SessionEnd`, `Notification`, `PermissionRequest` and `PreToolUse` (Bash|PowerShell) hooks. It forwards the hook JSON to the app over TCP. If the app isn't running and the event is `SessionStart`, it launches the app detached.
- **`install` / `uninstall`** (`src/cli.rs`): copy the exe and edit `~/.claude/settings.json`. Results show in a message box, because the exe has no console.

Two kinds of session:
- **Mirrored** (v1): Claude Code runs in a terminal; she reads its log and hooks, watch-only.
- **Hosted** (v2, `src/session.rs`): she runs `claude -p --input-format stream-json --output-format stream-json --permission-prompt-tool stdio …` and drives it over stdin/stdout. While one exists, log and hook events are ignored. One at a time.

Data flow into the UI thread, all on one `mpsc` channel (`Msg`):
- the log poller thread (`src/log.rs`): `Msg::Log(session path, Event)`
- the TCP listener thread: `Msg::Hook(json)`, e.g. a permission prompt, or `{"quit":true}` from install/uninstall
- rewrite threads (`src/rewrite.rs`, one `claude -p` per reply): `Msg::Rewritten`
- the hosted session's reader thread: `Msg::Stream(n, stream::Event)`; `n` drops late events from an ended session
- speech to text (`src/stt.rs`): `Msg::Heard`; spoken and typed input both go through `src/router.rs` (one Haiku call → an `Action`)

Other v2 modules: `src/panel.rs` (session panel window: turn cards, steps, Memory and Services tabs), `src/memgraph.rs` (memory graph view), `src/services.rs` (`mcp.json` next to the exe; the enabled servers are written to `mcp-session.json` and passed with `--mcp-config`).

## Rules

- Everything that knows Claude Code's log format lives in `src/log.rs`. The format is internal and unversioned. Its tests use real line shapes; when a Claude Code update breaks parsing, add the new shape to the tests first.
- Everything that knows the stream-json format lives in `src/stream.rs`, tested against real lines in `tests/fixtures/stream/`. The final reply is `result.result` (with partial messages on, `stop_reason` is always null). Helper steps carry `parent`; they show in the panel but never drive her pose.
- Hosted sessions run with the user's settings, hooks and plugins (no `--restricted`): they're the user's real sessions. Only her own helper calls (rewriter, router, chat, summaries) use the restricted flag set.
- Speech to text is pure-Rust Whisper (`candle`) so the macOS and Linux cross-checks need no C toolchain; don't swap in whisper.cpp. The model is `model/whisper-small/` next to the exe (`config.json`, `tokenizer.json`, `model.safetensors`).
- Poll log files; don't use file-change notifications. They don't fire on `\\wsl.localhost` paths, which WSL support will need.
- The hook must print nothing (SessionStart output is added to Claude's context; PermissionRequest and PreToolUse output could answer the dialog). Before spawning the app, it must clear the inherit flag on its std handles, or Claude Code waits forever on the hook's pipe.
- After a permission prompt, log activity in the next 2s doesn't end Waiting. Claude Code writes its log late, so lines from just before the prompt still arrive.
- The rewriter must keep the exact flag set in SPEC.md. `--no-session-persistence` keeps her from mirroring her own rewrites; `--restricted` keeps the user's hooks and plugins out. Don't swap in `--bare`: it only accepts API-key auth.
- Use eframe's glow renderer (`Renderer::Glow`). The default wgpu renderer gives an opaque black window on Windows.
- OS-specific calls live in `src/os_windows.rs` and `src/os_unix.rs` (same functions; `main.rs` picks one as `os`). Elsewhere, branch with `cfg!(windows)` / `cfg!(target_os = "macos")` and keep `windows-sys` inside `#[cfg(windows)]` code.
- Sound goes through rodio on every OS (one player thread owns the device). On Linux she runs on X11 (`with_x11`), because Wayland can't keep a window on top; click-through is off on Wayland sessions (`os::click_through`).
- eframe 0.37 apps implement `App::ui(&mut self, ui, frame)`, not `update`. `Mesh::colored_vertex` panics on textured meshes, so push `Vertex` structs directly.
- Animation bends each pose image on a grid (`src/anim.rs`). There are no cut-out layers, because the AI art has nothing painted behind her hair or arms. Body-part regions are hard-coded fractions of the 1024×1536 canvas.
- Art is compiled into the exe with `include_bytes!` from `assets/<pose>.png`. `assets/PROMPTS.md` holds the image prompts. A `skin` folder of `<pose>.png` files next to the exe overrides all of it when `skin/idle.png` loads (`custom()` / `load()` in `src/main.rs`).
- The window never takes keyboard focus (`with_active(false)`). Don't add keyboard shortcuts.
- The UI thread never blocks. File reads, TCP and `claude -p` run on worker threads, which call `ctx.request_repaint()` after sending an event.

## Environment

- Windows 11. Claude Code runs in the VS Code terminal, cmd, PowerShell, and sometimes WSL Ubuntu.
- WSL isn't installed on this PC as of 2026-11-02, so WSL support can't be tested here.
- Hook fields and rewriter flags were checked against Claude Code 2.1.316; stream-json, stdio permissions, interrupt and headless slash commands against 2.1.318.
- The user's installed copy usually holds port 47827. For a live test of a dev build, change the port temporarily and don't commit it.
