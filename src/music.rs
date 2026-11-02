// Lofi music she plays for you. It's synthesized here (no audio files, no network), so it's royalty-free by construction.
// A player thread renders a track and plays it with Windows' PlaySound; the app drives it with play/pause/next/seek.
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const RATE: usize = 22_050;
const BARS: usize = 32; // 1:30 to 2:00 per track

// Chords as MIDI notes, root first.
const FMAJ_EM_DM_CMAJ: [&[f32]; 4] = [&[53., 57., 60., 64.], &[52., 55., 59., 62.], &[50., 53., 57., 60.], &[48., 52., 55., 59.]];
const DM9_G13_CMAJ9_AM7: [&[f32]; 4] = [&[50., 53., 57., 60., 64.], &[55., 59., 64., 65.], &[48., 52., 55., 59., 62.], &[57., 60., 64., 67.]];
const AM7_DM7_G7_CMAJ7: [&[f32]; 4] = [&[57., 60., 64., 67.], &[50., 53., 57., 60.], &[55., 59., 62., 65.], &[48., 52., 55., 59.]];
const EBMAJ_DM_CM_BBMAJ: [&[f32]; 4] = [&[51., 55., 58., 62.], &[50., 53., 57., 60.], &[48., 51., 55., 58.], &[46., 50., 53., 57.]];
const CMAJ_AM_FMAJ_G7: [&[f32]; 4] = [&[48., 52., 55., 59.], &[45., 48., 52., 55.], &[53., 57., 60., 64.], &[55., 59., 62., 65.]];
// Ballad progressions: I-V-vi-IV and vi-IV-I-V.
const C_G_AM_F: [&[f32]; 4] = [&[48., 52., 55., 59.], &[50., 55., 59., 62.], &[52., 57., 60., 64.], &[53., 57., 60., 65.]];
const AM_F_C_G: [&[f32]; 4] = [&[52., 57., 60., 64.], &[53., 57., 60., 64.], &[52., 55., 60., 64.], &[50., 55., 59., 62.]];
// The lofi songs' melodies follow imagined lyrics: pickups, one note per syllable, long notes at line ends, breaths between.
// "Rainy Window Seat" over Fmaj7 Em7 Dm7 Cmaj7: "rain on the window / humming a slow song / coffee is getting cold / I don't mind at all".
const RAINY: [&[(f32, f32, f32)]; 8] = [
    &[(0.0, 69.0, 0.5), (0.5, 72.0, 0.5), (1.0, 72.0, 0.5), (1.5, 76.0, 1.0), (2.5, 74.0, 1.0)],
    &[(0.5, 71.0, 0.5), (1.0, 72.0, 0.5), (1.5, 74.0, 0.5), (2.0, 71.0, 1.0), (3.0, 67.0, 1.0)],
    &[(0.0, 69.0, 0.5), (0.5, 72.0, 0.5), (1.0, 74.0, 0.5), (1.5, 72.0, 0.5), (2.0, 69.0, 0.5), (2.5, 65.0, 1.5)],
    &[(1.0, 64.0, 0.5), (1.5, 67.0, 0.5), (2.0, 71.0, 1.0), (3.0, 72.0, 1.0)],
    &[(0.0, 77.0, 1.0), (1.0, 76.0, 0.5), (1.5, 74.0, 0.5), (2.0, 72.0, 1.5)],
    &[(0.5, 74.0, 0.5), (1.0, 71.0, 0.5), (1.5, 67.0, 0.5), (2.0, 71.0, 2.0)],
    &[(0.0, 69.0, 0.5), (0.5, 72.0, 0.5), (1.0, 74.0, 1.0), (2.0, 77.0, 0.5), (2.5, 76.0, 0.5), (3.0, 74.0, 1.0)],
    &[(0.0, 72.0, 3.0)],
];
// "Last Bus Home" over Am7 Dm7 G7 Cmaj7: "last bus home / city lights are fading / lean my head and close my eyes / almost there".
const LAST_BUS: [&[(f32, f32, f32)]; 8] = [
    &[(0.5, 76.0, 0.5), (1.0, 74.0, 0.5), (1.5, 72.0, 1.5)],
    &[(0.0, 69.0, 0.5), (0.5, 72.0, 0.5), (1.0, 74.0, 0.5), (1.5, 72.0, 0.5), (2.0, 69.0, 0.5), (2.5, 65.0, 1.0)],
    &[(0.5, 67.0, 0.5), (1.0, 71.0, 0.5), (1.5, 74.0, 0.5), (2.0, 77.0, 1.0), (3.0, 74.0, 0.5), (3.5, 71.0, 0.5)],
    &[(0.0, 72.0, 1.0), (1.0, 71.0, 0.5), (1.5, 67.0, 2.0)],
    &[(0.0, 72.0, 0.5), (0.5, 76.0, 0.5), (1.0, 79.0, 1.0), (2.0, 76.0, 1.0), (3.0, 74.0, 0.5), (3.5, 72.0, 0.5)],
    &[(0.0, 74.0, 1.5), (1.5, 72.0, 0.5), (2.0, 69.0, 2.0)],
    &[(0.5, 71.0, 0.5), (1.0, 74.0, 0.5), (1.5, 77.0, 0.5), (2.0, 76.0, 0.5), (2.5, 74.0, 0.5), (3.0, 71.0, 1.0)],
    &[(0.0, 72.0, 1.0), (1.0, 67.0, 0.5), (1.5, 64.0, 2.5)],
];
// "Sunday Matcha" over Cmaj7 Am7 Fmaj7 G7: "sunday morning / matcha in my hands / no alarms today / just you and me".
const SUNDAY: [&[(f32, f32, f32)]; 8] = [
    &[(0.0, 76.0, 0.5), (0.5, 76.0, 0.5), (1.0, 79.0, 1.0), (2.0, 76.0, 0.5), (2.5, 72.0, 1.5)],
    &[(0.0, 72.0, 0.5), (0.5, 74.0, 0.5), (1.0, 76.0, 0.5), (1.5, 72.0, 0.5), (2.0, 69.0, 1.0), (3.0, 72.0, 1.0)],
    &[(0.5, 69.0, 0.5), (1.0, 72.0, 0.5), (1.5, 77.0, 1.0), (2.5, 76.0, 0.5), (3.0, 74.0, 1.0)],
    &[(0.0, 74.0, 0.5), (0.5, 71.0, 0.5), (1.0, 67.0, 2.0)],
    &[(0.0, 79.0, 1.0), (1.0, 76.0, 0.5), (1.5, 79.0, 0.5), (2.0, 81.0, 1.0), (3.0, 79.0, 1.0)],
    &[(0.0, 76.0, 1.5), (1.5, 72.0, 0.5), (2.0, 76.0, 1.0), (3.0, 74.0, 1.0)],
    &[(0.0, 72.0, 0.5), (0.5, 74.0, 0.5), (1.0, 77.0, 0.5), (1.5, 76.0, 0.5), (2.0, 74.0, 1.0), (3.0, 71.0, 1.0)],
    &[(0.0, 74.0, 1.0), (1.0, 71.0, 0.5), (1.5, 67.0, 0.5), (2.0, 72.0, 2.0)],
];
// "Night Shift Neon" over Ebmaj7 Dm7 Cm7 Bbmaj7: "neon on the wet street / walking with no hurry / every light's a slow heartbeat / carry me home".
const NIGHT_SHIFT: [&[(f32, f32, f32)]; 8] = [
    &[(0.5, 70.0, 0.5), (1.0, 70.0, 0.5), (1.5, 74.0, 0.5), (2.0, 75.0, 1.0), (3.0, 74.0, 1.0)],
    &[(0.0, 72.0, 0.5), (0.5, 69.0, 0.5), (1.0, 65.0, 1.0), (2.0, 69.0, 0.5), (2.5, 72.0, 1.5)],
    &[(0.5, 67.0, 0.5), (1.0, 70.0, 0.5), (1.5, 72.0, 0.5), (2.0, 74.0, 0.5), (2.5, 75.0, 0.5), (3.0, 74.0, 0.5), (3.5, 72.0, 0.5)],
    &[(0.0, 70.0, 2.5), (3.0, 65.0, 1.0)],
    &[(0.0, 79.0, 1.0), (1.0, 77.0, 0.5), (1.5, 75.0, 0.5), (2.0, 74.0, 1.0), (3.0, 70.0, 1.0)],
    &[(0.0, 72.0, 1.0), (1.0, 74.0, 0.5), (1.5, 72.0, 0.5), (2.0, 69.0, 2.0)],
    &[(0.0, 67.0, 0.5), (0.5, 70.0, 0.5), (1.0, 72.0, 0.5), (1.5, 70.0, 0.5), (2.0, 67.0, 1.0), (3.0, 63.0, 1.0)],
    &[(0.0, 65.0, 1.0), (1.0, 69.0, 1.0), (2.0, 70.0, 2.0)],
];
// "Letters Never Sent" over C G Am F: rises, sighs, then comes to rest.
const LETTERS: [&[(f32, f32, f32)]; 8] = [
    &[(0.0, 72.0, 1.0), (1.0, 76.0, 1.0), (2.0, 79.0, 2.0)],
    &[(0.0, 74.0, 1.5), (1.5, 76.0, 0.5), (2.0, 74.0, 1.0), (3.0, 71.0, 1.0)],
    &[(0.0, 72.0, 2.0), (2.0, 76.0, 1.0), (3.0, 72.0, 1.0)],
    &[(0.0, 77.0, 1.0), (1.0, 76.0, 1.0), (2.0, 72.0, 2.0)],
    &[(0.0, 76.0, 1.5), (1.5, 77.0, 0.5), (2.0, 79.0, 2.0)],
    &[(0.0, 79.0, 1.0), (1.0, 77.0, 1.0), (2.0, 74.0, 2.0)],
    &[(0.0, 76.0, 1.0), (1.0, 74.0, 1.0), (2.0, 72.0, 1.0), (3.0, 74.0, 1.0)],
    &[(0.0, 72.0, 4.0)],
];
// "Moonlit Walk Home" over Am F C G: a four-bar phrase, then a four-bar answer that lands back home.
const MOONLIT: [&[(f32, f32, f32)]; 8] = [
    &[(0.0, 76.0, 1.5), (1.5, 74.0, 0.5), (2.0, 72.0, 1.0), (3.0, 69.0, 1.0)],
    &[(0.0, 72.0, 1.5), (1.5, 69.0, 0.5), (2.0, 72.0, 1.0), (3.0, 74.0, 1.0)],
    &[(0.0, 76.0, 2.0), (2.0, 79.0, 1.0), (3.0, 76.0, 1.0)],
    &[(0.0, 74.0, 3.0), (3.0, 71.0, 1.0)],
    &[(0.0, 72.0, 1.0), (1.0, 74.0, 1.0), (2.0, 76.0, 2.0)],
    &[(0.0, 77.0, 1.5), (1.5, 76.0, 0.5), (2.0, 74.0, 2.0)],
    &[(0.0, 72.0, 1.0), (1.0, 74.0, 1.0), (2.0, 76.0, 1.0), (3.0, 79.0, 1.0)],
    &[(0.0, 74.0, 2.0), (2.0, 71.0, 1.0), (3.0, 69.0, 1.0)],
];
const BBMAJ_AM_GM_FMAJ: [&[f32]; 4] = [&[46., 50., 53., 57.], &[45., 48., 52., 55.], &[43., 46., 50., 53.], &[53., 57., 60., 64.]];

#[derive(Clone, Copy, PartialEq)]
enum Inst {
    Keys,        // electric piano
    Pad,         // soft warm synth pad
    Vibes,       // vibraphone
    Piano,       // acoustic piano (leads)
    PianoArp,    // acoustic piano broken chords over a quiet string bed
    MusicBox,    // light plinky tines (leads)
    Flute,       // soft breathy-free flute with gentle vibrato (leads)
    DustyKeys,   // electric piano through a worn tape: mellower, with a slow pitch wobble
}

#[derive(Clone, Copy, PartialEq)]
enum Groove {
    BoomBap, // lazy swung kick-snare, the classic lofi beat
    RnB,     // half-time: syncopated kick, clap, rolling 16th hats, gliding 808 bass
    Soft,    // no kick: rim and shaker only
    Ballad,  // soft kick on 1, brush on 3, long bass notes, slow sustained melody
}

struct Track {
    name: &'static str,
    bpm: f32,
    transpose: f32,
    progs: [[&'static [f32]; 4]; 2],
    chords: Inst,
    lead: Inst,
    groove: Groove,
    melody: f32, // chance of a melody note per eighth (when there's no tune)
    tune: &'static [&'static [(f32, f32, f32)]], // written melody over progs[0], one bar per chord: (beat, MIDI note, beats long); empty = improvised
    warmth: f32, // low-pass amount: lower is darker
    lofi: bool,  // lofi hip-hop treatment: heavier swing, everything but the kick dips on each kick, quieter melody
}

const TRACKS: [Track; 6] = [
    Track { name: "Rainy Window Seat", bpm: 74.0, transpose: 0.0, progs: [FMAJ_EM_DM_CMAJ, DM9_G13_CMAJ9_AM7], chords: Inst::DustyKeys, lead: Inst::DustyKeys, groove: Groove::BoomBap, melody: 0.12, tune: &RAINY, warmth: 0.30, lofi: true },
    Track { name: "Last Bus Home", bpm: 70.0, transpose: -2.0, progs: [AM7_DM7_G7_CMAJ7, CMAJ_AM_FMAJ_G7], chords: Inst::Vibes, lead: Inst::Piano, groove: Groove::BoomBap, melody: 0.12, tune: &LAST_BUS, warmth: 0.38, lofi: false },
    Track { name: "Sunday Matcha", bpm: 76.0, transpose: 1.0, progs: [CMAJ_AM_FMAJ_G7, FMAJ_EM_DM_CMAJ], chords: Inst::PianoArp, lead: Inst::MusicBox, groove: Groove::Soft, melody: 0.14, tune: &SUNDAY, warmth: 0.44, lofi: false },
    Track { name: "Night Shift Neon", bpm: 64.0, transpose: -3.0, progs: [EBMAJ_DM_CM_BBMAJ, BBMAJ_AM_GM_FMAJ], chords: Inst::Pad, lead: Inst::Keys, groove: Groove::RnB, melody: 0.12, tune: &NIGHT_SHIFT, warmth: 0.33, lofi: false },
    Track { name: "Letters Never Sent", bpm: 58.0, transpose: 0.0, progs: [C_G_AM_F, C_G_AM_F], chords: Inst::Pad, lead: Inst::Flute, groove: Groove::Ballad, melody: 0.0, tune: &LETTERS, warmth: 0.42, lofi: false },
    Track { name: "Moonlit Walk Home", bpm: 60.0, transpose: -2.0, progs: [AM_F_C_G, AM_F_C_G], chords: Inst::PianoArp, lead: Inst::Piano, groove: Groove::Ballad, melody: 0.0, tune: &MOONLIT, warmth: 0.44, lofi: false },
];

/// How the next song is chosen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Loop,    // the playlist in order, then from the top
    LoopOne, // the same song again
    Shuffle, // a random song, never the same twice in a row
}

impl Mode {
    pub fn next(self) -> Mode {
        match self {
            Mode::Loop => Mode::LoopOne,
            Mode::LoopOne => Mode::Shuffle,
            Mode::Shuffle => Mode::Loop,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Mode::Loop => "Loop",
            Mode::LoopOne => "Loop 1",
            Mode::Shuffle => "Shuffle",
        }
    }
    pub fn from_label(s: &str) -> Option<Mode> {
        [Mode::Loop, Mode::LoopOne, Mode::Shuffle].into_iter().find(|m| m.label() == s)
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Song {
    Synth(usize),  // one of TRACKS
    File(PathBuf), // from the music folder
}

impl Song {
    fn title(&self) -> String {
        match self {
            Song::Synth(i) => TRACKS[*i].name.to_string(),
            Song::File(p) => p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug)]
struct Playlist {
    name: String,
    songs: Vec<Song>,
}

pub const BUILT_IN: &str = "Claudesona's songs";
pub const LOOSE: &str = "My music"; // files directly in the music folder
const AUDIO: [&str; 6] = ["mp3", "wav", "wma", "m4a", "aac", "flac"];

/// `music` next to the exe, like `skin`. Each subfolder with audio files is a playlist.
pub fn music_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("music"))).unwrap_or_else(|| PathBuf::from("music"))
}

/// The built-in songs, then "My music" (loose files), then one playlist per subfolder, all sorted by name.
fn scan(dir: &Path) -> Vec<Playlist> {
    let audio = |p: &Path| p.extension().is_some_and(|e| AUDIO.contains(&e.to_string_lossy().to_lowercase().as_str()));
    let files = |d: &Path| {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && audio(p)).collect();
        v.sort();
        v.into_iter().map(Song::File).collect::<Vec<_>>()
    };
    let mut lists = vec![Playlist { name: BUILT_IN.into(), songs: (0..TRACKS.len()).map(Song::Synth).collect() }];
    let loose = files(dir);
    if !loose.is_empty() {
        lists.push(Playlist { name: LOOSE.into(), songs: loose });
    }
    let mut folders: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    folders.sort();
    for f in folders {
        let songs = files(&f);
        if !songs.is_empty() {
            lists.push(Playlist { name: f.file_name().unwrap_or_default().to_string_lossy().into_owned(), songs });
        }
    }
    lists
}

/// Which song plays after `cur` (of `len`) in `mode`. `skip` is a Next press: Loop 1 moves on then too.
fn after(mode: Mode, cur: usize, len: usize, skip: bool, rng: &mut Rng) -> usize {
    match mode {
        Mode::LoopOne if !skip => cur,
        Mode::Shuffle if len > 1 => {
            let i = (rng.next() * len as f32) as usize % len;
            if i == cur { (i + 1) % len } else { i }
        }
        _ => (cur + 1) % len.max(1),
    }
}

#[derive(Clone)]
pub struct Status {
    pub playlist: usize,
    pub track: usize,
    pub name: String,
    pub pos: f32, // seconds
    pub len: f32,
    pub paused: bool,
}

enum Cmd {
    Play(Option<(usize, usize)>), // (playlist, song), or carry on with the current playlist
    Next,
    Prev,
    Pause, // toggles
    Seek(f32),
    Stop,
    SetMode(Mode),
    Select(String), // make a playlist current by name, without playing
    Rescan,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static LIBRARY: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(vec![]); // playlist names and song titles, for the UI
static PLAYER: OnceLock<Sender<Cmd>> = OnceLock::new();

/// Commands go to one player thread, which owns the sound and makes every PlaySound and MCI call.
fn send(cmd: Cmd) {
    let _ = PLAYER
        .get_or_init(|| {
            let (tx, rx) = channel();
            std::thread::spawn(move || player(rx));
            tx
        })
        .send(cmd);
}

pub fn play() {
    send(Cmd::Play(None));
}
pub fn play_track(playlist: usize, song: usize) {
    send(Cmd::Play(Some((playlist, song))));
}
pub fn next() {
    send(Cmd::Next);
}
pub fn prev() {
    send(Cmd::Prev);
}
pub fn toggle_pause() {
    send(Cmd::Pause);
}
pub fn seek(secs: f32) {
    send(Cmd::Seek(secs));
}
pub fn stop() {
    send(Cmd::Stop);
}
pub fn set_mode(mode: Mode) {
    send(Cmd::SetMode(mode));
}
/// Restores the saved playlist at startup (the built-in one if that folder is gone).
pub fn select(playlist: &str) {
    send(Cmd::Select(playlist.to_string()));
}
/// Re-reads the music folder; the song list calls this when it opens.
pub fn rescan() {
    send(Cmd::Rescan);
}
/// What's playing, or None when stopped.
pub fn status() -> Option<Status> {
    STATUS.lock().unwrap().clone()
}
/// Playlist names with their song titles.
pub fn library() -> Vec<(String, Vec<String>)> {
    LIBRARY.lock().unwrap().clone()
}

/// The library for the UI: playlist names and song titles.
fn publish(lists: &[Playlist]) {
    *LIBRARY.lock().unwrap() = lists.iter().map(|l| (l.name.clone(), l.songs.iter().map(Song::title).collect())).collect();
}

/// Re-reads the music folder, keeping the current playlist and song by name/path (files may have come or gone around
/// them). A playlist that's gone falls back to the built-in one; a song that's gone leaves `cur` empty.
fn refresh(lists: &mut Vec<Playlist>, list: &mut usize, cur: &mut Option<usize>) {
    let (name, playing) = (lists[*list].name.clone(), cur.map(|c| lists[*list].songs[c].clone()));
    *lists = scan(&music_dir());
    publish(lists);
    *list = lists.iter().position(|l| l.name == name).unwrap_or(0);
    *cur = playing.and_then(|p| lists[*list].songs.iter().position(|s| *s == p));
}

/// The song a playlist starts with: the first one, or a random one in Shuffle.
fn first(mode: Mode, len: usize, rng: &mut Rng) -> usize {
    if mode == Mode::Shuffle { (rng.next() * len as f32) as usize % len.max(1) } else { 0 }
}

/// Sends an MCI command string (Windows' media player engine plays the user's files). Err on failure.
fn mci(cmd: &str) -> Result<String, u32> {
    use windows_sys::Win32::Media::Multimedia::mciSendStringW;
    let wide: Vec<u16> = cmd.encode_utf16().chain([0]).collect();
    let mut out = [0u16; 128];
    let err = unsafe { mciSendStringW(wide.as_ptr(), out.as_mut_ptr(), out.len() as u32, std::ptr::null_mut()) };
    let end = out.iter().position(|c| *c == 0).unwrap_or(out.len());
    if err == 0 { Ok(String::from_utf16_lossy(&out[..end])) } else { Err(err) }
}

/// One song at a time: a synthesized track through PlaySound, or a file through MCI.
struct Deck {
    synth: Option<(usize, Vec<f32>)>, // rendered built-in track, kept so pause/seek/Loop 1 don't re-render
    _playing: Vec<u8>,                // WAV bytes Windows is reading; must outlive playback
    file: bool,                       // an MCI file is open as "claudesona"
    len: f32,
    from: f32, // synth: where the playing buffer starts
    since: Instant,
    paused: bool,
}

impl Deck {
    fn silence(&mut self) {
        use windows_sys::Win32::Media::Audio::PlaySoundW;
        unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
        if self.file {
            let _ = mci("close claudesona");
            self.file = false;
        }
    }

    /// Starts `song` `at` seconds in. Err if the file can't be played (unsupported format, missing).
    fn start(&mut self, song: &Song, at: f32, seed: u32) -> Result<(), ()> {
        self.silence(); // Windows stops reading the old buffer before it's replaced
        self.paused = false;
        match song {
            Song::Synth(t) => {
                if self.synth.as_ref().is_none_or(|(i, _)| i != t) {
                    self.synth = Some((*t, render(&TRACKS[*t], seed)));
                }
                let pcm = &self.synth.as_ref().unwrap().1;
                let skip = ((at.max(0.0) * RATE as f32) as usize).min(pcm.len());
                self.len = pcm.len() as f32 / RATE as f32;
                self._playing = wav(&pcm[skip..]);
                use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
                // Async: a blocking PlaySound can't be stopped from another call.
                unsafe { PlaySoundW(self._playing.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
                (self.from, self.since) = (skip as f32 / RATE as f32, Instant::now());
                Ok(())
            }
            Song::File(p) => {
                mci(&format!("open \"{}\" type mpegvideo alias claudesona", p.display())).map_err(|_| ())?;
                self.file = true;
                let ok = mci("set claudesona time format milliseconds").is_ok()
                    && mci(&format!("play claudesona from {}", (at.max(0.0) * 1000.0) as u32)).is_ok();
                self.len = mci("status claudesona length").ok().and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(0.0) / 1000.0;
                if !ok || self.len <= 0.0 {
                    self.silence();
                    return Err(());
                }
                Ok(())
            }
        }
    }

    fn pos(&self) -> f32 {
        if self.file {
            mci("status claudesona position").ok().and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(0.0) / 1000.0
        } else if self.paused {
            self.from
        } else {
            self.from + self.since.elapsed().as_secs_f32()
        }
    }

    fn ended(&self) -> bool {
        !self.paused && (self.synth.is_some() || self.file) && if self.file { mci("status claudesona mode").is_ok_and(|m| m.trim() == "stopped") } else { self.pos() >= self.len }
    }
}

/// Plays songs back to back from the current playlist until Stop, choosing each next one by the mode.
fn player(rx: Receiver<Cmd>) {
    let mut rng = Rng(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() | 1);
    let mut seed = 1u32;
    let mut lists = scan(&music_dir());
    publish(&lists);
    let mut deck = Deck { synth: None, _playing: vec![], file: false, len: 0.0, from: 0.0, since: Instant::now(), paused: false };
    let (mut list, mut cur, mut mode) = (0usize, None::<usize>, Mode::Shuffle);
    let mut history: Vec<usize> = vec![]; // for Prev in shuffle
    loop {
        let cmd = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let len = lists[list].songs.len();
        let mut start = None; // (song, seconds in)
        match (cmd, cur) {
            (Some(Cmd::Play(Some((l, s)))), _) if l < lists.len() && s < lists[l].songs.len() => {
                if l != list {
                    (list, cur) = (l, None);
                    history.clear();
                    deck.synth = None;
                }
                start = Some((s, 0.0));
            }
            (Some(Cmd::Play(None)), _) => {
                // Re-read the folder first: the saved playlist may have been changed or deleted since.
                refresh(&mut lists, &mut list, &mut cur);
                start = Some((first(mode, lists[list].songs.len(), &mut rng), 0.0));
            }
            (Some(Cmd::Next), Some(c)) => start = Some((after(mode, c, len, true, &mut rng), 0.0)),
            (Some(Cmd::Prev), Some(c)) => {
                // Like most players: past 3s, restart the song; otherwise go back one.
                if deck.pos() > 3.0 {
                    start = Some((c, 0.0));
                } else if mode == Mode::Shuffle && history.len() >= 2 {
                    history.pop();
                    start = history.pop().map(|p| (p, 0.0));
                } else {
                    start = Some(((c + len - 1) % len, 0.0));
                }
            }
            (Some(Cmd::Pause), Some(c)) if deck.paused => {
                if deck.file {
                    // "play" carries on from where it stopped, after a pause or a seek.
                    deck.paused = mci("play claudesona").is_err();
                } else {
                    start = Some((c, deck.from));
                }
            }
            (Some(Cmd::Pause), Some(_)) => {
                if deck.file {
                    let _ = mci("pause claudesona");
                } else {
                    deck.from = deck.pos();
                    use windows_sys::Win32::Media::Audio::PlaySoundW;
                    unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
                }
                deck.paused = true;
            }
            (Some(Cmd::Seek(s)), Some(c)) => {
                let s = s.clamp(0.0, deck.len);
                if deck.file {
                    let _ = mci(&format!("{} claudesona {} {}", if deck.paused { "seek" } else { "play" }, if deck.paused { "to" } else { "from" }, (s * 1000.0) as u32));
                } else if deck.paused {
                    deck.from = s;
                } else {
                    start = Some((c, s));
                }
            }
            (Some(Cmd::Stop), _) => {
                deck.silence();
                (deck.synth, deck._playing, deck.paused, cur) = (None, vec![], false, None);
                history.clear();
            }
            (Some(Cmd::SetMode(m)), _) => mode = m,
            (Some(Cmd::Select(name)), None) => list = lists.iter().position(|l| l.name == name).unwrap_or(0),
            (Some(Cmd::Rescan), _) => {
                let was_playing = cur.is_some();
                refresh(&mut lists, &mut list, &mut cur);
                if was_playing && cur.is_none() {
                    deck.silence(); // its file or folder is gone
                }
                history.clear();
            }
            (None, Some(c)) if deck.ended() => start = Some((after(mode, c, len, false, &mut rng), 0.0)),
            _ => {}
        }
        // Start the chosen song; if a file won't play, try the next ones (each at most once).
        // If nothing in the playlist plays, fall back to her own songs rather than going silent.
        if let Some((mut s, at)) = start {
            seed = seed.wrapping_mul(747_796_405).wrapping_add(rng.0);
            let mut tries = 0;
            while deck.start(&lists[list].songs[s], at, seed).is_err() {
                tries += 1;
                let len = lists[list].songs.len();
                if tries < len {
                    s = after(if mode == Mode::LoopOne { Mode::Loop } else { mode }, s, len, true, &mut rng);
                } else {
                    // Built-in songs always play, so this ends.
                    (list, tries, cur) = (0, 0, None);
                    history.clear();
                    s = first(mode, lists[0].songs.len(), &mut rng);
                }
            }
            if cur != Some(s) {
                history.push(s);
            }
            cur = Some(s);
        }
        *STATUS.lock().unwrap() = cur.map(|track| Status {
            playlist: list,
            track,
            name: lists[list].songs[track].title(),
            pos: deck.pos().min(deck.len),
            len: deck.len,
            paused: deck.paused,
        });
    }
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32
    }
    fn noise(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

fn freq(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

/// Adds `f(time since onset)` into `buf` from `start` for `secs`.
fn add(buf: &mut [f32], start: f32, secs: f32, mut f: impl FnMut(f32) -> f32) {
    let s = (start * RATE as f32) as usize;
    for i in 0..(secs * RATE as f32) as usize {
        let Some(out) = buf.get_mut(s + i) else { break };
        *out += f(i as f32 / RATE as f32);
    }
}

/// Electric-piano voice: a sine with soft overtones, slow decay and a little tremolo.
fn keys(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    add(buf, start, secs, |t| {
        let env = (t / 0.008).min(1.0) * (-t * 1.3).exp() * (1.0 - (t / secs).powi(8));
        let tone = (TAU * f * t).sin() + 0.3 * (TAU * 2.0 * f * t).sin() * (-t * 4.0).exp() + 0.08 * (TAU * 3.0 * f * t).sin() * (-t * 7.0).exp();
        vol * env * tone * (1.0 + 0.12 * (TAU * 4.5 * t).sin())
    });
}

/// Music box: a thin tine with a bright, slightly out-of-tune partial that fades fast.
fn music_box(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    add(buf, start, secs, |t| {
        let env = (t / 0.002).min(1.0) * (-t * 2.4).exp() * (1.0 - (t / secs).powi(8));
        let tone = (TAU * f * t).sin() + 0.25 * (TAU * 5.4 * f * t).sin() * (-t * 12.0).exp();
        vol * 0.9 * env * tone
    });
}

/// Flute: a pure tone that swells in, gains a gentle vibrato, and fades at the end.
fn flute(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    let mut phase = 0.0;
    add(buf, start, secs, |t| {
        let vib = 1.0 + 0.004 * (TAU * 5.0 * t).sin() * ((t - 0.25) / 0.3).clamp(0.0, 1.0);
        phase += TAU * f * vib / RATE as f32;
        let env = (t / 0.08).min(1.0) * ((secs - t) / 0.15).clamp(0.0, 1.0) * (1.0 - 0.15 * (t / secs));
        vol * 0.9 * env * (phase.sin() + 0.12 * (2.0 * phase).sin() + 0.04 * (3.0 * phase).sin())
    });
}

/// Warm pad: three slightly detuned sines and a soft octave, slow swell in and out.
fn pad(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    add(buf, start, secs, |t| {
        let env = (t / 0.6).min(1.0) * ((secs - t) / 0.5).clamp(0.0, 1.0);
        let tone = (TAU * f * t).sin() + (TAU * f * 1.004 * t).sin() + (TAU * f * 0.996 * t).sin() + 0.3 * (TAU * 2.0 * f * t).sin();
        vol * 0.45 * env * tone
    });
}

/// Vibraphone: a bright bar with a fast-fading overtone and a deep motor tremolo.
fn vibes(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    add(buf, start, secs, |t| {
        let env = (t / 0.003).min(1.0) * (-t * 1.6).exp() * (1.0 - (t / secs).powi(8));
        let tone = (TAU * f * t).sin() + 0.35 * (TAU * 4.0 * f * t).sin() * (-t * 9.0).exp();
        vol * env * tone * (1.0 + 0.3 * (TAU * 5.5 * t).sin())
    });
}

/// Acoustic piano: a hammer tick, overtones that fade faster the higher they are, and two slightly detuned strings.
fn piano(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    add(buf, start, secs, |t| {
        let env = (t / 0.004).min(1.0) * (-t * 0.9).exp() * (1.0 - (t / secs).powi(8));
        let mut tone = 0.0;
        for h in 1..=6 {
            let h = h as f32;
            let fh = f * h * (1.0 + 0.0004 * h * h); // real strings are slightly sharp up high
            tone += (TAU * fh * t).sin() * (-t * 1.2 * h).exp() / h.powf(1.3);
        }
        tone += 0.5 * (TAU * f * 1.0015 * t).sin() * (-t * 1.1).exp();
        vol * 0.8 * env * tone
    });
}

/// Dusty keys: a mellow electric piano with a slow tape wobble in pitch, the classic lofi sound.
fn dusty_keys(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    let mut phase = 0.0;
    let wow = start * 0.55; // the wobble keeps running across notes, like one tape
    add(buf, start, secs, |t| {
        phase += TAU * f * (1.0 + 0.003 * (TAU * 0.55 * (t + wow)).sin()) / RATE as f32;
        let env = (t / 0.01).min(1.0) * (-t * 1.1).exp() * (1.0 - (t / secs).powi(8));
        let tone = phase.sin() + 0.15 * (2.0 * phase).sin() * (-t * 3.0).exp();
        vol * 1.05 * env * tone * (1.0 + 0.1 * (TAU * 4.0 * t).sin())
    });
}

/// One note on `inst`.
fn note(buf: &mut [f32], inst: Inst, start: f32, midi: f32, secs: f32, vol: f32) {
    match inst {
        Inst::Keys => keys(buf, start, midi, secs, vol),
        Inst::MusicBox => music_box(buf, start, midi, secs, vol),
        Inst::Flute => flute(buf, start, midi, secs, vol),
        Inst::DustyKeys => dusty_keys(buf, start, midi, secs, vol),
        Inst::Pad => pad(buf, start, midi, secs, vol),
        Inst::Vibes => vibes(buf, start, midi, secs, vol * 0.8),
        Inst::Piano | Inst::PianoArp => piano(buf, start, midi, secs, vol),
    }
}

/// A bar of the chord on `inst`, in that instrument's own rhythm.
fn chord_bar(buf: &mut [f32], inst: Inst, t0: f32, beat: f32, chord: &[f32], rng: &mut Rng) {
    let bar = beat * 4.0;
    match inst {
        Inst::PianoArp => {
            // Left-hand root, then the chord broken upward and back in eighths, over a quiet string pad.
            piano(buf, t0, chord[0] - 12.0, bar, 0.07);
            let pattern = [0, 1, 2, 3, 2, 1, 2, 3];
            for (step, &k) in pattern.iter().enumerate().skip(1) {
                piano(buf, t0 + step as f32 * beat / 2.0, chord[k % chord.len()], beat * 2.0, 0.045);
            }
            for n in chord {
                pad(buf, t0, *n, bar + 0.3, 0.02);
            }
        }
        Inst::Keys | Inst::Vibes | Inst::Piano | Inst::MusicBox | Inst::Flute | Inst::DustyKeys => {
            for (i, n) in chord.iter().enumerate() {
                note(buf, inst, t0 + i as f32 * 0.018, *n, bar, 0.07); // slight strum
                if rng.next() < 0.5 {
                    note(buf, inst, t0 + beat * 2.5 + i as f32 * 0.012, *n, bar - beat * 2.5, 0.035);
                }
            }
        }
        Inst::Pad => {
            for n in chord {
                pad(buf, t0, *n, bar + 0.3, 0.05); // overlaps into the next bar for a smooth change
            }
        }
    }
}

/// Kick: a sine that drops in pitch.
fn kick(buf: &mut [f32], at: f32, vol: f32) {
    let mut phase = 0.0;
    add(buf, at, 0.35, |t| {
        phase += TAU * (48.0 + 90.0 * (-t * 35.0).exp()) / RATE as f32;
        vol * (-t * 9.0).exp() * phase.sin()
    });
}

/// Filtered noise burst; `color` is the low-pass amount (low = darker), `high` keeps only the hiss.
fn noise_hit(buf: &mut [f32], at: f32, secs: f32, decay: f32, color: f32, high: bool, vol: f32, seed: u32) {
    let (mut lp, mut n) = (0.0, Rng(seed | 1));
    add(buf, at, secs, |t| {
        let x = n.noise();
        lp += color * (x - lp);
        vol * (-t * decay).exp() * if high { x - lp } else { lp }
    });
}

/// The whole track as mono samples in -1..1. `seed` varies the arrangement, melody and drums.
fn render(track: &Track, seed: u32) -> Vec<f32> {
    let beat = 60.0 / track.bpm;
    let bar = beat * 4.0;
    let mut buf = vec![0.0f32; (bar * BARS as f32 * RATE as f32) as usize];
    let mut kicks = vec![0.0f32; if track.lofi { buf.len() } else { 0 }];
    let mut kick_times = vec![];
    let swing_amt = if track.lofi { 0.17 } else { 0.1 }; // lofi drags the off-beats further
    let mut rng = Rng(seed | 1);
    let breakdown = 2 + (rng.next() * 2.0) as usize % 2; // a drumless section in the middle
    let mut last_bass = 0.0;
    for section in 0..BARS / 8 {
        let which = if section == 0 { 0 } else { (rng.next() * 2.0) as usize % 2 };
        let prog = &track.progs[which];
        let sing = !track.tune.is_empty() && (which == 0 || track.progs[0] == track.progs[1]);
        let drums = section != 0 && section != breakdown; // the intro has no drums either
        for b in 0..8 {
            let t0 = (section * 8 + b) as f32 * bar;
            let chord: Vec<f32> = prog[b % 4].iter().map(|n| n + track.transpose).collect();
            chord_bar(&mut buf, track.chords, t0, beat, &chord, &mut rng);
            // Bass in the 41-82 Hz octave.
            let root = (chord[0] - 28.0).rem_euclid(12.0) + 28.0;
            if track.groove == Groove::RnB {
                // 808: a long sub that glides up from the previous note.
                let (from, to) = (freq(if last_bass > 0.0 { last_bass } else { root }), freq(root));
                let mut phase = 0.0;
                add(&mut buf, t0, bar * 0.95, |t| {
                    let f = to + (from - to) * (-t * 18.0).exp();
                    phase += TAU * f / RATE as f32;
                    0.32 * (t / 0.005).min(1.0) * (-t * 0.9).exp() * (phase.sin() * 1.4).tanh()
                });
            } else if track.groove == Groove::Ballad {
                // One long, round note per bar.
                let f = freq(root);
                add(&mut buf, t0, bar, |t| 0.2 * (t / 0.02).min(1.0) * (-t * 0.8).exp() * ((TAU * f * t).sin() + 0.15 * (TAU * 2.0 * f * t).sin()));
            } else {
                // Bass on the one and a lazy push before beat three.
                let f = freq(root);
                for at in [0.0, beat * 1.75] {
                    add(&mut buf, t0 + at, beat * 2.0, |t| 0.22 * (t / 0.01).min(1.0) * (-t * 2.2).exp() * ((TAU * f * t).sin() + 0.2 * (TAU * 2.0 * f * t).sin()));
                }
            }
            last_bass = root;
            if section >= 1 && sing {
                for &(at, n, beats) in track.tune[b % track.tune.len()] {
                    let vol = if track.lofi { 0.055 } else { 0.075 }; // lofi keeps the melody back in the mix
                    note(&mut buf, track.lead, t0 + at * beat, n + track.transpose, beats * beat * 1.15, vol);
                }
            } else if section >= 1 {
                for step in 0..8 {
                    if rng.next() < track.melody {
                        let n = chord[(rng.next() * chord.len() as f32) as usize % chord.len()] + 12.0;
                        let swing = if step % 2 == 1 { beat * swing_amt } else { 0.0 };
                        let len = if track.groove == Groove::Ballad { beat * 3.0 } else { beat * 1.5 }; // ballads sing long notes
                        note(&mut buf, track.lead, t0 + step as f32 * beat / 2.0 + swing, n, len, 0.05);
                    }
                }
            }
            if !drums {
                continue;
            }
            match track.groove {
                Groove::BoomBap => {
                    for k in [0.0, 2.0, 2.5] {
                        if k != 2.5 || rng.next() < 0.4 {
                            if track.lofi {
                                kick(&mut kicks, t0 + k * beat, 0.5);
                                kick_times.push(t0 + k * beat);
                            } else {
                                kick(&mut buf, t0 + k * beat, 0.45);
                            }
                        }
                    }
                    for s in [1.0, 3.0] {
                        noise_hit(&mut buf, t0 + s * beat, 0.3, 16.0, 0.3, false, 0.15, rng.0);
                        add(&mut buf, t0 + s * beat, 0.1, |t| 0.06 * (-t * 30.0).exp() * (TAU * 185.0 * t).sin());
                    }
                    for h in 0..8 {
                        let swing = if h % 2 == 1 { beat * swing_amt } else { 0.0 };
                        let vol = 0.022 + 0.015 * rng.next();
                        noise_hit(&mut buf, t0 + h as f32 * beat / 2.0 + swing, 0.06, 70.0, 0.5, true, vol, rng.0);
                    }
                }
                Groove::RnB => {
                    for k in [0.0, 1.75, 3.5] {
                        if k != 3.5 || rng.next() < 0.5 {
                            kick(&mut buf, t0 + k * beat, 0.38);
                        }
                    }
                    // Clap on 3 (half-time): three quick bursts smeared together.
                    for (i, d) in [0.0, 0.011, 0.023].iter().enumerate() {
                        noise_hit(&mut buf, t0 + 2.0 * beat + d, 0.25, if i == 2 { 14.0 } else { 60.0 }, 0.6, true, 0.14, rng.0.wrapping_add(i as u32));
                    }
                    for h in 0..16 {
                        let at = t0 + h as f32 * beat / 4.0;
                        let vol = if h % 4 == 0 { 0.03 } else { 0.012 + 0.012 * rng.next() };
                        noise_hit(&mut buf, at, 0.04, 90.0, 0.5, true, vol, rng.0);
                        // Now and then a 32nd-note roll.
                        if h % 4 == 3 && rng.next() < 0.15 {
                            noise_hit(&mut buf, at + beat / 8.0, 0.03, 90.0, 0.5, true, vol * 0.8, rng.0);
                        }
                    }
                }
                Groove::Ballad => {
                    kick(&mut buf, t0, 0.35);
                    noise_hit(&mut buf, t0 + 2.0 * beat, 0.35, 9.0, 0.3, false, 0.12, rng.0); // brush
                    for q in 0..4 {
                        noise_hit(&mut buf, t0 + q as f32 * beat, 0.08, 40.0, 0.6, true, 0.018, rng.0);
                    }
                }
                Groove::Soft => {
                    // Rim click on 2 and 4, shaker in eighths.
                    for s in [1.0, 3.0] {
                        add(&mut buf, t0 + s * beat, 0.05, |t| 0.09 * (-t * 60.0).exp() * (TAU * 1700.0 * t).sin());
                    }
                    for h in 0..8 {
                        let vol = if h % 2 == 1 { 0.04 } else { 0.025 };
                        noise_hit(&mut buf, t0 + h as f32 * beat / 2.0, 0.09, 35.0, 0.6, true, vol, rng.0);
                    }
                }
            }
        }
    }
    // A warm low-pass, gentle saturation, and short fades at both ends. (No vinyl crackle: it sounded like frying.)
    // Lofi "pump": everything else dips under each kick and swells back.
    if track.lofi {
        for &k in &kick_times {
            let s0 = (k * RATE as f32) as usize;
            for (i, x) in buf.iter_mut().skip(s0).take(RATE / 3).enumerate() {
                *x *= 1.0 - 0.35 * (-(i as f32 / RATE as f32) / 0.09).exp();
            }
        }
        for (x, k) in buf.iter_mut().zip(&kicks) {
            *x += k;
        }
    }
    let mut lp = 0.0f32;
    let len = buf.len() as f32;
    for (i, s) in buf.iter_mut().enumerate() {
        lp += track.warmth * (*s - lp);
        let fade = (i as f32 / (0.05 * RATE as f32)).min(1.0).min((len - i as f32) / (1.5 * RATE as f32));
        *s = (lp * 0.9).tanh() * 0.6 * fade.max(0.0);
    }
    buf
}

/// 16-bit mono PCM WAV file bytes.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend((RATE as u32).to_le_bytes());
    out.extend((RATE as u32 * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    for s in samples {
        out.extend(((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_are_audible_and_never_clip() {
        for (i, track) in TRACKS.iter().enumerate() {
            let samples = render(track, 7);
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
            assert!(peak < 0.99 && rms > 0.03, "track {i}: peak {peak}, rms {rms}");
        }
        let bytes = wav(&[0.0; 10]);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 20);
    }

    #[test]
    fn folders_become_playlists() {
        let dir = std::env::temp_dir().join(format!("claudesona-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in ["b.mp3", "a.wav", "notes.txt", "Chill/2.flac", "Chill/1.M4A", "Empty/readme.md"] {
            std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let lists = scan(&dir);
        let names: Vec<_> = lists.iter().map(|l| (l.name.as_str(), l.songs.iter().map(Song::title).collect::<Vec<_>>())).collect();
        assert_eq!(names[0].0, BUILT_IN);
        assert_eq!(names[0].1.len(), TRACKS.len());
        assert_eq!(names[1], (LOOSE, vec!["a".to_string(), "b".to_string()]));
        assert_eq!(names[2], ("Chill", vec!["1".to_string(), "2".to_string()]));
        assert_eq!(names.len(), 3); // no playlist for a folder without audio
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn modes_pick_the_next_song() {
        let mut rng = Rng(7);
        assert_eq!(after(Mode::Loop, 2, 3, false, &mut rng), 0);
        assert_eq!(after(Mode::LoopOne, 1, 3, false, &mut rng), 1);
        assert_eq!(after(Mode::LoopOne, 1, 3, true, &mut rng), 2); // Next still moves on
        for _ in 0..20 {
            assert_ne!(after(Mode::Shuffle, 1, 3, false, &mut rng), 1);
        }
        assert_eq!(after(Mode::Shuffle, 0, 1, false, &mut rng), 0); // a one-song playlist just repeats
        assert_eq!(Mode::from_label(Mode::LoopOne.label()), Some(Mode::LoopOne));
    }
}


// Plays real audio for a few seconds: run with `cargo test --release -- --ignored`.
#[test]
#[ignore]
fn player_plays_pauses_and_stops() {
    play_track(0, 2);
    std::thread::sleep(Duration::from_secs(2));
    let st = status().expect("playing");
    assert!(st.track == 2 && st.pos > 0.5 && !st.paused, "pos {}", st.pos);
    toggle_pause();
    std::thread::sleep(Duration::from_millis(300));
    let paused = status().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(paused.paused && status().unwrap().pos == paused.pos);
    seek(30.0);
    toggle_pause();
    std::thread::sleep(Duration::from_millis(300));
    assert!(status().unwrap().pos >= 30.0);
    stop();
    std::thread::sleep(Duration::from_millis(300));
    assert!(status().is_none());
}

// Plays a real file through MCI for a few seconds: run with `cargo test --release -- --ignored`.
#[test]
#[ignore]
fn deck_plays_a_file_with_pause_and_seek() {
    let dir = std::env::temp_dir().join(format!("claudesona-mci-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone song.wav"); // a space, like real file names
    let tone: Vec<f32> = (0..RATE * 6).map(|i| 0.1 * (TAU * 440.0 * i as f32 / RATE as f32).sin()).collect();
    std::fs::write(&path, wav(&tone)).unwrap();
    let mut deck = Deck { synth: None, _playing: vec![], file: false, len: 0.0, from: 0.0, since: Instant::now(), paused: false };
    deck.start(&Song::File(path.clone()), 0.0, 1).expect("MCI plays the WAV");
    assert!((deck.len - 6.0).abs() < 0.1, "len {}", deck.len);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(deck.pos() > 0.8 && !deck.ended(), "pos {}", deck.pos());
    mci("pause claudesona").unwrap();
    deck.paused = true;
    let held = deck.pos();
    std::thread::sleep(Duration::from_millis(400));
    assert!((deck.pos() - held).abs() < 0.05);
    mci("seek claudesona to 5500").unwrap();
    mci("play claudesona").unwrap();
    deck.paused = false;
    std::thread::sleep(Duration::from_millis(1200));
    assert!(deck.ended(), "should have reached the end, pos {}", deck.pos());
    deck.silence();
    assert!(deck.start(&Song::File(dir.join("missing.mp3")), 0.0, 1).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
