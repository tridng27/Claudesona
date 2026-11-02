# Claudesona

A chibi desktop mascot for Claude Code on Windows, macOS and Linux. While Claude works she shows what it's doing, retells its replies in her own cheerful voice, and waves you over when Claude needs your permission. When things get quiet she plays music for you, and she's always up for a word-guessing game.

<p align="center"><img src="docs/tour.gif" alt="Claudesona introducing herself: idle, working, asking for permission, reading a reply, being poked, playing music, and her three game faces" width="330"></p>

## What she does

### Follows Claude Code

She watches your Claude Code session and changes pose with it:

- **Working**: she types on a glowing keyboard and shows what Claude is doing, like "Reading main.rs…" or a line about the command that's running (tests, builds, git, installs and more).
- **Needs your OK**: when Claude asks for permission she puts her hands on her hips, a "!" pops up, and she calls you.
- **Done**: Claude's final reply comes back rewritten in her voice. Click the bubble for the longer version.
- **Helpers**: every sub-agent Claude starts gets its own mini Claudesona by her feet, with a little "done!" when it finishes.

<p align="center"><img src="docs/demo.gif" alt="Claudesona working through tool calls with a helper mini beside her, asking for permission, then going back to idle" width="296"></p>

### Plays music

After 5 quiet minutes she puts on music by herself, or pick **Play music** from her right-click menu. A small player sits above her head: song name, time, a progress bar you can click to jump, previous, play/pause, next, the song list and stop.

There are six slow, chill songs, each with a written melody that phrases like sung lyrics. They're synthesized inside the app while it runs. There are no audio files and no downloads, so there's nothing to license:

| Song | Feel |
| :- | :- |
| Rainy Window Seat | classic lofi hip-hop: dusty wobbly electric piano, heavy swing, pumping beat |
| Last Bus Home | vibraphone chords, piano melody, soft boom-bap |
| Sunday Matcha | piano and music box, soft shaker |
| Night Shift Neon | warm pad and electric piano, slow R&B groove, deep 808 bass |
| Letters Never Sent | slow ballad, strings and flute |
| Moonlit Walk Home | slow ballad, piano |

#### Your own songs and playlists

Pick **Open music folder** from her right-click menu (or **Add songs** in the song list). It opens the `music` folder in her install folder (see Install) in your file manager:

- Make a folder inside it and drop songs in: each folder is a playlist named after the folder.
- Songs placed directly in the music folder show up as "My music".
- MP3, M4A/AAC, FLAC, OGG and WAV work. A file that won't play is skipped.

In the song list, the arrows next to the playlist name switch between "Claudesona's songs" and your playlists; click a song to play that playlist. The button on the left of the player cycles how the next song is picked:

| Mode | Next song |
| :- | :- |
| Loop | the playlist in order, then from the top |
| Loop 1 | the same song again |
| Shuffle | a random song from the playlist |

Your playlist and mode are remembered.

<p align="center"><img src="docs/music.gif" alt="The music player above Claudesona: the song list opens and a new song starts" width="330"></p>

### Plays with you

Pick **Play with me** and she starts a game of hangman. Claude (Haiku) picks a word and writes a hint, and you guess by clicking letters. You get six hearts. She cheers, teases and reacts to every guess, looks smug when she wins, and sulks a little when you do.

<p align="center"><img src="docs/game.gif" alt="A round of the word game: the player wins with PILLOW and she pouts, then loses on RAINBOW and she looks smug" width="330"></p>

### Her poses

<p align="center"><img src="docs/poses.png" alt="All eight poses: idle, working, needs your OK, poked, music, and the three game faces" width="680"></p>

## Install on a machine

Works on Windows 10/11, macOS and Linux (X11). Needs Claude Code. The `claude` command must be on PATH for her in-character rewrites and the word game. Without it she shows Claude's plain text, and the game uses a short built-in word list.

1. Get the file for your system: `claudesona.exe` on Windows, `claudesona` on macOS and Linux. Build it yourself (see Build), or download it from the latest run of the `build` workflow under the repository's Actions tab. It's a single file with the art built in.
2. Run `claudesona install` (`claudesona.exe install` on Windows) from a terminal.

`install` does three things:
- copies the file to her install folder:

  | System | Install folder |
  | :- | :- |
  | Windows | `%LOCALAPPDATA%\Programs\Claudesona` |
  | macOS | `~/Library/Application Support/Claudesona` |
  | Linux | `~/.local/share/claudesona` |

- adds five hooks (`SessionStart`, `SessionEnd`, `Notification`, `PermissionRequest` and `PreToolUse` for Bash/PowerShell) to `~/.claude/settings.json`, after saving a backup as `settings.json.claudesona-backup`
- starts her

From then on she starts with every new Claude Code session. Sessions that were already open need a restart.

To update, run `install` again with the new file. It replaces the running copy.

On macOS, a downloaded (not self-built) file may be blocked as coming from an unidentified developer. Allow it under System Settings > Privacy & Security, or run `xattr -d com.apple.quarantine claudesona` once.

## Uninstall

Run `claudesona uninstall`. It removes her hooks (and nothing else) from `settings.json` and closes her. Then delete her install folder (move your `music` folder out first if you want to keep your songs).

## Using her

- **Drag** to move her. She remembers the spot.
- **Click** her to poke her.
- **Click** her reply bubble to show the longer version.
- **Double-click** her to bring back the terminal you last sent a prompt from (Windows only).
- **Right-click** for the menu:
  - **Size**: S, M or L
  - **Language**: EN or VI. In Vietnamese she calls herself "tớ" or "Phụ tá" and calls you "cậu"; her menus, lines, reply rewrites and the word game all switch (game words are Vietnamese written without tone marks, like MEO for "mèo", since you guess on an A-Z grid)
  - **Play music** / **Stop music**
  - **While working**: show her working pose or her music pose while Claude works
  - **Open music folder**: add your own songs and playlists
  - **Play with me**: the word game
  - **What can I do**: she introduces herself and shows each of her poses
  - **Hide for 10 min**: she keeps following Claude, invisible and click-through
  - **Quit**: until the next Claude Code session
- Clicks anywhere else pass through to the windows behind her.

Size, position, language, the "While working" choice, your playlist and the play mode are remembered.

## Custom art

Make a `skin` folder in her install folder (see Install) and put your PNGs in it, then restart her:

| File | Shown when | If missing |
| :- | :- | :- |
| `idle.png` (required) | nothing is happening | the skin is ignored |
| `idle_blink.png` | idle, eyes closed for a blink | no blinking |
| `working.png` | Claude is working | `idle.png` |
| `working_blink.png` | working, eyes closed | no blinking |
| `waiting.png` | Claude needs your OK | `idle.png` |
| `poke.png` | you click her | `idle.png` |
| `music.png` | music is playing | `idle.png` |
| `game_excited.png` | you're playing the word game | `idle.png` |
| `game_excited_blink.png` | playing, eyes closed | no blinking |
| `game_smug.png` | she won the game | `waiting.png` |
| `game_pouty.png` | you won the game | `poke.png` |

- If `idle.png` is there, the whole skin replaces the built-in art; skin and built-in images are never mixed.
- Every image: one full-body character standing on the bottom edge, transparent background, all on the same canvas size and framing. Any portrait size works, e.g. 1024×1536.
- The animation bends fixed parts of the image: the head tilts around a neck point 38% from the top, hair swings at the sides between chin and shoulders, and the skirt hem at 52–72% from the top. In the music pose, the raised hand on the viewer's left (about shoulder height) sways to the beat. Art laid out differently still shows, but the motion may look odd.
- Her clickable area is her non-transparent pixels.
- `assets/PROMPTS.md` has the AI image prompts used for the built-in art.

## Build

```
cargo build --release      # target/release/claudesona (claudesona.exe on Windows) is the file to ship
cargo test
```

On Linux, building needs the ALSA headers for sound: `sudo apt install libasound2-dev pkg-config` (Debian/Ubuntu) or `sudo dnf install alsa-lib-devel` (Fedora). The `build` workflow in `.github/workflows` builds and tests all three systems on every push.

## Limits

- Linux: she runs as an X11 window (XWayland on Wayland desktops), because Wayland doesn't let a window stay on top or choose its position. On a Wayland session her see-through parts don't let clicks through, so her whole box is clickable. Transparency needs a compositor.
- macOS and Linux: double-click doesn't bring your terminal back.
- On Windows she can't see Claude Code running inside WSL yet.
- Each reply rewrite and each new game word is a small `claude -p` call (Haiku) on the user's Claude plan.
- Music volume follows your system volume only.
