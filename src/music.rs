// Lofi music she plays for you. It's synthesized here (no audio files, no network), so it's royalty-free by construction.
// A player thread renders a track and plays it with Windows' PlaySound; the app drives it with play/pause/next/seek.
use std::f32::consts::TAU;
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
    Hum,         // a soft hummed "ooh", like a singer without words (leads)
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
}

const TRACKS: [Track; 6] = [
    Track { name: "Rainy Window Seat", bpm: 66.0, transpose: 0.0, progs: [FMAJ_EM_DM_CMAJ, DM9_G13_CMAJ9_AM7], chords: Inst::Keys, lead: Inst::Hum, groove: Groove::BoomBap, melody: 0.14, tune: &RAINY, warmth: 0.36 },
    Track { name: "Last Bus Home", bpm: 70.0, transpose: -2.0, progs: [AM7_DM7_G7_CMAJ7, CMAJ_AM_FMAJ_G7], chords: Inst::Vibes, lead: Inst::Piano, groove: Groove::BoomBap, melody: 0.12, tune: &LAST_BUS, warmth: 0.38 },
    Track { name: "Sunday Matcha", bpm: 76.0, transpose: 1.0, progs: [CMAJ_AM_FMAJ_G7, FMAJ_EM_DM_CMAJ], chords: Inst::PianoArp, lead: Inst::MusicBox, groove: Groove::Soft, melody: 0.14, tune: &SUNDAY, warmth: 0.44 },
    Track { name: "Night Shift Neon", bpm: 64.0, transpose: -3.0, progs: [EBMAJ_DM_CM_BBMAJ, BBMAJ_AM_GM_FMAJ], chords: Inst::Pad, lead: Inst::Keys, groove: Groove::RnB, melody: 0.12, tune: &NIGHT_SHIFT, warmth: 0.33 },
    Track { name: "Letters Never Sent", bpm: 58.0, transpose: 0.0, progs: [C_G_AM_F, C_G_AM_F], chords: Inst::Pad, lead: Inst::Flute, groove: Groove::Ballad, melody: 0.0, tune: &LETTERS, warmth: 0.42 },
    Track { name: "Moonlit Walk Home", bpm: 60.0, transpose: -2.0, progs: [AM_F_C_G, AM_F_C_G], chords: Inst::PianoArp, lead: Inst::Piano, groove: Groove::Ballad, melody: 0.0, tune: &MOONLIT, warmth: 0.44 },
];

#[derive(Clone, Copy)]
pub struct Status {
    pub track: usize,
    pub pos: f32, // seconds
    pub len: f32,
    pub paused: bool,
}

enum Cmd {
    Play(Option<usize>), // a given track, or a random one
    Next,
    Prev,
    Pause, // toggles
    Seek(f32),
    Stop,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static PLAYER: OnceLock<Sender<Cmd>> = OnceLock::new();

/// Commands go to one player thread, which owns the sound buffer and makes every PlaySound call.
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
pub fn play_track(i: usize) {
    send(Cmd::Play(Some(i)));
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
/// What's playing, or None when stopped. Shows the old track for the moment a new one takes to render.
pub fn status() -> Option<Status> {
    *STATUS.lock().unwrap()
}
pub const COUNT: usize = TRACKS.len();
pub fn name(i: usize) -> &'static str {
    TRACKS[i].name
}

/// Plays tracks back to back in random order (never the same twice in a row) until Stop.
fn player(rx: Receiver<Cmd>) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    let silence = || unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
    let mut rng = Rng(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() | 1);
    let mut seed = 1u32;
    // `_playing` is only read by Windows, through the pointer given to PlaySound; it must outlive playback.
    let (mut cur, mut pcm, mut _playing) = (None::<usize>, Vec::<f32>::new(), Vec::<u8>::new());
    let mut history: Vec<usize> = vec![]; // for Prev
    let (mut from, mut since, mut paused) = (0.0f32, Instant::now(), false); // the buffer starts at `from` seconds
    loop {
        let cmd = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let len = pcm.len() as f32 / RATE as f32;
        let pos = if paused { from } else { from + since.elapsed().as_secs_f32() };
        let mut start = None; // (track, seconds in) to play from
        match (cmd, cur) {
            (Some(Cmd::Play(t)), _) => start = Some((t.unwrap_or_else(|| pick(&mut rng, cur)), 0.0)),
            (Some(Cmd::Next), Some(c)) => start = Some((pick(&mut rng, Some(c)), 0.0)),
            (Some(Cmd::Prev), Some(c)) => {
                // Like most players: past 3s, restart the track; otherwise go back one.
                if pos > 3.0 || history.len() < 2 {
                    start = Some((c, 0.0));
                } else {
                    history.pop();
                    start = history.pop().map(|p| (p, 0.0));
                }
            }
            (Some(Cmd::Pause), Some(c)) if paused => start = Some((c, from)),
            (Some(Cmd::Pause), Some(_)) => {
                silence();
                (from, paused) = (pos, true);
            }
            (Some(Cmd::Seek(s)), Some(c)) if !paused => start = Some((c, s)),
            (Some(Cmd::Seek(s)), Some(_)) => from = s.clamp(0.0, len),
            (Some(Cmd::Stop), _) => {
                silence(); // before the buffer is freed
                (cur, pcm, _playing, paused) = (None, vec![], vec![], false);
                history.clear();
            }
            (None, Some(_)) if !paused && pos >= len => start = Some((pick(&mut rng, cur), 0.0)),
            _ => {}
        }
        if let Some((t, at)) = start {
            silence(); // Windows stops reading the old buffer before we replace it
            if cur != Some(t) {
                seed = seed.wrapping_mul(747_796_405).wrapping_add(rng.0);
                pcm = render(&TRACKS[t], seed);
                history.push(t);
            }
            cur = Some(t);
            let skip = ((at.max(0.0) * RATE as f32) as usize).min(pcm.len());
            _playing = wav(&pcm[skip..]);
            // Async: a blocking PlaySound can't be stopped from another call.
            unsafe { PlaySoundW(_playing.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
            (from, since, paused) = (skip as f32 / RATE as f32, Instant::now(), false);
        }
        let len = pcm.len() as f32 / RATE as f32;
        let pos = if paused { from } else { from + since.elapsed().as_secs_f32() };
        *STATUS.lock().unwrap() = cur.map(|track| Status { track, pos: pos.min(len), len, paused });
    }
}

/// A random track, never `cur` again.
fn pick(rng: &mut Rng, cur: Option<usize>) -> usize {
    let i = (rng.next() * TRACKS.len() as f32) as usize % TRACKS.len();
    if Some(i) == cur { (i + 1) % TRACKS.len() } else { i }
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

/// Hum: a voice-like "ooh" that scoops up into the note, then sways with vibrato.
fn hum(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    let mut phase = 0.0;
    add(buf, start, secs, |t| {
        let scoop = 1.0 - 0.02 * (-t * 25.0).exp(); // starts a little flat, like a singer settling on the pitch
        let vib = 1.0 + 0.005 * (TAU * 5.2 * t).sin() * ((t - 0.2) / 0.3).clamp(0.0, 1.0);
        phase += TAU * f * scoop * vib / RATE as f32;
        let env = (t / 0.07).min(1.0) * ((secs - t) / 0.2).clamp(0.0, 1.0);
        vol * 0.85 * env * (phase.sin() + 0.3 * (2.0 * phase).sin() + 0.15 * (3.0 * phase).sin() + 0.05 * (4.0 * phase).sin())
    });
}

/// One note on `inst`.
fn note(buf: &mut [f32], inst: Inst, start: f32, midi: f32, secs: f32, vol: f32) {
    match inst {
        Inst::Keys => keys(buf, start, midi, secs, vol),
        Inst::MusicBox => music_box(buf, start, midi, secs, vol),
        Inst::Flute => flute(buf, start, midi, secs, vol),
        Inst::Hum => hum(buf, start, midi, secs, vol),
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
        Inst::Keys | Inst::Vibes | Inst::Piano | Inst::MusicBox | Inst::Flute | Inst::Hum => {
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
                    note(&mut buf, track.lead, t0 + at * beat, n + track.transpose, beats * beat * 1.15, 0.075);
                }
            } else if section >= 1 {
                for step in 0..8 {
                    if rng.next() < track.melody {
                        let n = chord[(rng.next() * chord.len() as f32) as usize % chord.len()] + 12.0;
                        let swing = if step % 2 == 1 { beat * 0.1 } else { 0.0 };
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
                            kick(&mut buf, t0 + k * beat, 0.45);
                        }
                    }
                    for s in [1.0, 3.0] {
                        noise_hit(&mut buf, t0 + s * beat, 0.3, 16.0, 0.3, false, 0.15, rng.0);
                        add(&mut buf, t0 + s * beat, 0.1, |t| 0.06 * (-t * 30.0).exp() * (TAU * 185.0 * t).sin());
                    }
                    for h in 0..8 {
                        let swing = if h % 2 == 1 { beat * 0.1 } else { 0.0 };
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
}


// Plays real audio for a few seconds: run with `cargo test --release -- --ignored`.
#[test]
#[ignore]
fn player_plays_pauses_and_stops() {
    play_track(2);
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
