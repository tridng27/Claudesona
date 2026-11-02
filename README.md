# Claudesona

A chibi desktop mascot for Claude Code on Windows. While Claude works she shows what it's doing, retells its replies in her own cheerful voice, and waves you over when Claude needs your permission.

## Install on a machine

Needs Windows 10 or 11 and Claude Code. The `claude` command must be on PATH for her in-character rewrites; without it she shows Claude's plain text.

1. Copy `claudesona.exe` to the machine. It's a single file with the art built in.
2. Run `claudesona.exe install` from a terminal.

`install` does three things:
- copies the exe to `%LOCALAPPDATA%\Programs\Claudesona\`
- adds three hooks (`SessionStart`, `Notification` and `PermissionRequest`) to `~/.claude/settings.json`, after saving a backup as `settings.json.claudesona-backup`
- starts her

From then on she starts with every new Claude Code session. Sessions that were already open need a restart.

To update, run `install` again with the new exe. It replaces the running copy.

## Uninstall

Run `claudesona.exe uninstall`. It removes her hooks (and nothing else) from `settings.json` and closes her. Then delete `%LOCALAPPDATA%\Programs\Claudesona`.

## Using her

- **Drag** to move her. She remembers the spot.
- **Click** her to poke her.
- **Click** her reply bubble to show the longer version.
- **Double-click** her to bring back the terminal you last sent a prompt from.
- **Right-click** for a menu: size (S / M / L), hide for 10 minutes, or quit until the next Claude Code session.
- Clicks anywhere else pass through to the windows behind her.

## Custom art

Make a `skin` folder next to the installed exe (`%LOCALAPPDATA%\Programs\Claudesona\skin`) and put your PNGs in it, then restart her:

| File | Shown when |
| :- | :- |
| `idle.png` (required) | nothing is happening |
| `idle_blink.png` | idle, eyes closed for a blink |
| `working.png` | Claude is working |
| `working_blink.png` | working, eyes closed |
| `waiting.png` | Claude needs your OK |
| `poke.png` | you click her |

- If `idle.png` is there, the whole skin replaces the built-in art. A missing pose uses your `idle.png`; a missing blink means no blinking. Without `idle.png` the folder is ignored.
- Every image: one full-body character standing on the bottom edge, transparent background, all six on the same canvas size and framing. Any portrait size works, e.g. 1024×1536.
- The animation bends fixed parts of the image: the head tilts around a neck point 38% from the top, hair swings at the sides between chin and shoulders, and the skirt hem at 52–72% from the top. Art laid out differently still shows, but the motion may look odd.
- Her clickable area is her non-transparent pixels.

## Build

```
cargo build --release      # target\release\claudesona.exe is the file to ship
cargo test
```

## Limits

- She reads Claude Code session logs on the Windows side only. Claude Code running inside WSL isn't supported yet.
- Each reply rewrite is a small `claude -p` call (Haiku) on the user's Claude plan.
