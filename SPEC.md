# Claudesona — v1 spec

A desktop mascot for Claude Code on Windows: a chibi anime girl who mirrors Claude's terminal work in speech bubbles. Written in Rust.
Claude Code may run in the VS Code terminal, cmd, PowerShell, or WSL Ubuntu. She must work with all of them.

## Behavior

**Phases (her pose)**

| Phase   | When                                                    | Look |
|---------|---------------------------------------------------------|------|
| Idle    | Nothing happening; also after a reply arrives           | Idle image, blinks, gentle bob/sway |
| Working | From your prompt until Claude replies (thinking, tools) | Hard-thinking image (narrowed eyes, furrowed brows, "^" mouth), blinks, livelier bounce, activity label |
| Waiting | Claude needs your permission                            | Proud image (hands on hips, starry eyes), "!" above her head, bubble "I need your OK~!" |

**Bubbles**
- Reply: Claude's reply rewritten in her voice (genki/cheerful), same language as the reply. Shows a 1–2 line version; click the bubble to expand to the longer version.
- Activity: short verbatim label from tool use ("Reading main.rs…"). No AI call.

**Minis (sub-agents)**
- Each sub-agent Claude spawns → a half-size her beside her feet with a tiny task label (the agent's `description`).
- When it finishes: happy "done!" pop, then she vanishes.

**Interaction**
- Drag her: move (position remembered). Starts bottom-right above the taskbar.
- Click her: poke pose (a bit annoyed) + a random mildly grumpy line ("Hey~ I'm busy!").
- Click bubble: expand.
- Double-click her: bring the terminal you last typed a prompt in to the front (the foreground window when the prompt reached the log).
- Right-click: menu with size (S / M / L, remembered), hide 10 min (she keeps following Claude, invisible and click-through), quit.

**Window**: transparent, borderless, always on top, ~250px tall. Clicks pass through everywhere except her, her bubble and minis.
**Launch**: starts on Claude Code's `SessionStart` hook unless she's already running.
**Sound**: none in v1.

## How it works

**One binary, two modes**
- `claudesona`: the mascot app. Listens on a fixed `127.0.0.1` port. If the port is taken, she's already running, so it exits (single instance).
- `claudesona hook`: what Claude Code hooks run. Reads the hook's JSON from stdin and forwards it to the app; on `SessionStart` it first launches the app if it isn't running.
- `claudesona install` / `uninstall`: copy the exe to `%LOCALAPPDATA%\Programs\Claudesona` and add or remove the hooks in `~/.claude/settings.json`.

**Hooks** (Windows `~/.claude/settings.json`)
- `SessionStart`: launch her.
- `PermissionRequest`: fires the moment a permission dialog shows → Waiting. The hook prints nothing, so the dialog is unchanged.
- `Notification`: permission prompts only (`notification_type: "permission_prompt"`) → Waiting. Claude Code sends it only after ~6s without typing, so it's a backup (and the only signal for sandbox network prompts). Idle "waiting for input" notices are ignored. Log activity ends Waiting, except in the first 2s, while lines from before the prompt still arrive.

**Reading Claude's work**
- Poll every `.jsonl` under `~/.claude/projects` about every 250ms. Read only new whole lines; history from before startup is skipped.
- Follow the session you last typed a prompt in; ignore the others.
- Line mapping:
  - `user` prompt → Working
  - `assistant` `text` with `stop_reason: "tool_use"` → small note label; any other `text` → final reply, sent to the rewriter
  - `assistant` `tool_use` → activity label; name `Agent` (older versions: `Task`) → spawn mini
  - `tool_result` with that mini's `tool_use_id` → mini done
  - `system` line with subtype `turn_duration` → turn over: back to Idle unless a rewrite is still pending

**Not built yet**
- WSL: Claude Code inside WSL keeps its logs in the Linux filesystem. Plan: WSL hooks call the Windows `.exe` through `/mnt/c/...` and register `transcript_path`, mapped to `\\wsl.localhost\<distro>\...`; stop polling on `SessionEnd`, because reading that path boots a stopped distro.

**Rewriter**
- For each reply, a background thread runs this from an empty working folder, with the reply text on stdin (avoids Windows command-line length limits):
  `claude -p --model haiku --no-session-persistence --restricted --strict-mcp-config --tools "" --disable-slash-commands --system-prompt "<persona>"`
  - `--no-session-persistence`: no log file, so she never mirrors her own rewrites.
  - `--restricted`: ignores your user and project settings, so your hooks and plugins don't run or leak into her voice.
  - Not `--bare`: it requires an API key, and you chose your Claude plan login.
- One call returns both the short and the long version (split on a marker line).
- These flags were checked against Claude Code 2.1.316's `--help` but haven't been run together yet.

**Crates**: `eframe` (with `persistence` to remember her position), `egui_extras` (PNG loading), `serde_json`, `windows-sys` (focus the terminal window). Everything else uses std: threads, `mpsc` channels, `TcpListener`, `Command`.

## Art (AI-generated)

- Prompts and step-by-step instructions: `assets/PROMPTS.md`.
- Chibi style, warm coral/terracotta and cream palette.
- Files: `assets/idle.png`, `idle_blink.png`, `working.png`, `working_blink.png`, `waiting.png`, `poke.png`. Minis reuse these at half size.
- All poses share one canvas framing (feet on the bottom edge); the animation's body regions are fractions of the image height.
- A missing pose falls back to idle; a missing blink means no blinking.
- Custom skins: `skin\<pose>.png` next to the exe. A readable `skin\idle.png` switches all poses to the skin (never mixed with built-in art). The canvas aspect comes from the images. Rules for artists are in README.md.

## Risks

- The session-log format is internal to Claude Code and can change on update. Keep the parser in one file.
- `claude -p` adds a few seconds per reply and uses your plan's limits. She stays in Working until the rewrite lands.
- Background sub-agents return right away, so their minis would vanish too early. Needs extra handling.
- Transparent + click-through windows on Windows: prototype this first.
- WSL isn't installed on this PC as of 2026-11-02 (`wsl.exe` reports the class isn't registered), so WSL support can't be tested here.
