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
const BBMAJ_AM_GM_FMAJ: [&[f32]; 4] = [&[46., 50., 53., 57.], &[45., 48., 52., 55.], &[43., 46., 50., 53.], &[53., 57., 60., 64.]];

struct Track {
    name: &'static str,
    bpm: f32,
    transpose: f32,
    progs: [[&'static [f32]; 4]; 2],
    melody: f32, // chance of a melody note per eighth
    warmth: f32, // low-pass amount: lower is darker
}

const TRACKS: [Track; 6] = [
    Track { name: "Rainy Window Seat", bpm: 72.0, transpose: 0.0, progs: [FMAJ_EM_DM_CMAJ, DM9_G13_CMAJ9_AM7], melody: 0.28, warmth: 0.42 },
    Track { name: "Last Bus Home", bpm: 80.0, transpose: -2.0, progs: [AM7_DM7_G7_CMAJ7, CMAJ_AM_FMAJ_G7], melody: 0.20, warmth: 0.38 },
    Track { name: "Sunday Matcha", bpm: 68.0, transpose: 1.0, progs: [CMAJ_AM_FMAJ_G7, FMAJ_EM_DM_CMAJ], melody: 0.35, warmth: 0.46 },
    Track { name: "Night Shift Neon", bpm: 76.0, transpose: -3.0, progs: [EBMAJ_DM_CM_BBMAJ, BBMAJ_AM_GM_FMAJ], melody: 0.22, warmth: 0.33 },
    Track { name: "Paper Cranes", bpm: 64.0, transpose: 2.0, progs: [BBMAJ_AM_GM_FMAJ, AM7_DM7_G7_CMAJ7], melody: 0.30, warmth: 0.40 },
    Track { name: "Cozy Bytes", bpm: 84.0, transpose: -1.0, progs: [DM9_G13_CMAJ9_AM7, EBMAJ_DM_CM_BBMAJ], melody: 0.25, warmth: 0.44 },
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

/// The whole track as mono samples in -1..1. `seed` varies the arrangement, melody and drums.
fn render(track: &Track, seed: u32) -> Vec<f32> {
    let beat = 60.0 / track.bpm;
    let bar = beat * 4.0;
    let mut buf = vec![0.0f32; (bar * BARS as f32 * RATE as f32) as usize];
    let mut rng = Rng(seed | 1);
    let breakdown = 2 + (rng.next() * 2.0) as usize % 2; // a drumless section in the middle
    for section in 0..BARS / 8 {
        let prog = &track.progs[if section == 0 { 0 } else { (rng.next() * 2.0) as usize % 2 }];
        let drums = section != 0 && section != breakdown; // the intro has no drums either
        for b in 0..8 {
            let t0 = (section * 8 + b) as f32 * bar;
            let chord: Vec<f32> = prog[b % 4].iter().map(|n| n + track.transpose).collect();
            for (i, n) in chord.iter().enumerate() {
                keys(&mut buf, t0 + i as f32 * 0.018, *n, bar, 0.07); // slight strum
                if rng.next() < 0.5 {
                    keys(&mut buf, t0 + beat * 2.5 + i as f32 * 0.012, *n, bar - beat * 2.5, 0.035);
                }
            }
            // Bass on the one and a lazy push before beat three, kept in the 41-82 Hz octave.
            let root = freq((chord[0] - 28.0).rem_euclid(12.0) + 28.0);
            for (at, len) in [(0.0, beat * 2.0), (beat * 1.75, beat * 2.0)] {
                add(&mut buf, t0 + at, len, |t| 0.22 * (t / 0.01).min(1.0) * (-t * 2.2).exp() * ((TAU * root * t).sin() + 0.2 * (TAU * 2.0 * root * t).sin()));
            }
            if section >= 1 {
                for step in 0..8 {
                    if rng.next() < track.melody {
                        let n = chord[(rng.next() * chord.len() as f32) as usize % chord.len()] + 12.0;
                        let swing = if step % 2 == 1 { beat * 0.1 } else { 0.0 };
                        keys(&mut buf, t0 + step as f32 * beat / 2.0 + swing, n, beat * 1.5, 0.05);
                    }
                }
            }
            if !drums {
                continue;
            }
            for k in [0.0, beat * 2.0, beat * 2.5] {
                if k == beat * 2.5 && rng.next() < 0.6 {
                    continue;
                }
                let mut phase = 0.0;
                add(&mut buf, t0 + k, 0.35, |t| {
                    phase += TAU * (48.0 + 90.0 * (-t * 35.0).exp()) / RATE as f32;
                    0.55 * (-t * 9.0).exp() * phase.sin()
                });
            }
            for s in [beat, beat * 3.0] {
                let (mut lp, mut n) = (0.0, Rng(rng.0 | 1));
                add(&mut buf, t0 + s, 0.3, |t| {
                    lp += 0.35 * (n.noise() - lp);
                    0.22 * (-t * 16.0).exp() * lp + 0.08 * (-t * 30.0).exp() * (TAU * 185.0 * t).sin()
                });
            }
            for h in 0..8 {
                let swing = if h % 2 == 1 { beat * 0.1 } else { 0.0 };
                let vol = 0.035 + 0.025 * rng.next();
                let (mut lp, mut n) = (0.0, Rng(rng.0 | 1));
                add(&mut buf, t0 + h as f32 * beat / 2.0 + swing, 0.06, |t| {
                    let x = n.noise();
                    lp += 0.5 * (x - lp);
                    vol * (-t * 70.0).exp() * (x - lp) // high-passed noise
                });
            }
        }
    }
    // Vinyl: soft hiss plus the odd crackle. Then a warm low-pass, gentle saturation, and short fades at both ends.
    let (mut lp, mut pop) = (0.0f32, 0.0f32);
    let len = buf.len() as f32;
    for (i, s) in buf.iter_mut().enumerate() {
        if rng.next() < 0.0004 {
            pop = 0.25 * rng.noise();
        }
        pop *= 0.8;
        let x = *s + 0.012 * rng.noise() + pop;
        lp += track.warmth * (x - lp);
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
