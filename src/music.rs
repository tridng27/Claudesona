// Lofi music she plays for you. It's synthesized here (no audio files, no network), so it's royalty-free by construction.
// A player thread renders a track and plays it through rodio; the app drives it with play/pause/next/seek.
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const RATE: usize = 22_050;
const BARS: usize = 32; // 1:30 to 2:00 per track
// Her voice sits under the music, never over it: the mp3s peak near full scale, the music around 0.6.
const VOICE_VOLUME: f32 = 0.3;
// While she talks, the music goes muffled (low-passed) and a little quieter, so her words come through.
static MUFFLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const MUFFLE_HZ: f32 = 900.0; // low-pass cutoff at full muffle
const MUFFLE_DIP: f32 = 0.25; // volume taken off at full muffle
const MUFFLE_FADE: f32 = 0.15; // seconds to go muffled and back

/// Music source wrapper that eases into a low-pass and a small dip while MUFFLED is set, and back out after.
struct Muffle<S> {
    input: S,
    lp: [f32; 8], // filter state per channel
    amount: f32,  // 0 = clear, 1 = fully muffled
    ch: usize,    // channel of the next sample
}

impl<S: rodio::Source> Iterator for Muffle<S> {
    type Item = rodio::Sample;
    fn next(&mut self) -> Option<rodio::Sample> {
        let x = self.input.next()?;
        let (channels, rate) = (self.input.channels().get() as usize, self.input.sample_rate().get() as f32);
        let target = if MUFFLED.load(std::sync::atomic::Ordering::Relaxed) { 1.0 } else { 0.0 };
        self.amount += (target - self.amount) / (MUFFLE_FADE * rate * channels as f32);
        let c = self.ch % 8;
        self.lp[c] += (1.0 - (-TAU * MUFFLE_HZ / rate).exp()) * (x - self.lp[c]);
        self.ch = (self.ch + 1) % channels.max(1);
        Some((x + (self.lp[c] - x) * self.amount) * (1.0 - MUFFLE_DIP * self.amount))
    }
}

impl<S: rodio::Source> rodio::Source for Muffle<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.input.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.input.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.ch = 0; // seeking lands on a frame boundary
        self.input.try_seek(pos)
    }
}

fn muffle<S: rodio::Source>(input: S) -> Muffle<S> {
    Muffle { input, lp: [0.0; 8], amount: if MUFFLED.load(std::sync::atomic::Ordering::Relaxed) { 1.0 } else { 0.0 }, ch: 0 }
}

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
// Keys for the second batch: F (ii-V-I-vi), D, A minor and Ab. These four are lighter, sharper and faster than the first six:
// brighter filter (warmth 0.6+), 82-88 BPM, no tape wobble or kick pump.
const GM7_C9_FMAJ7_DM7: [&[f32]; 4] = [&[55., 58., 62., 65.], &[48., 52., 55., 58., 62.], &[53., 57., 60., 64.], &[50., 53., 57., 60.]];
const BBMAJ7_AM7_GM7_C7: [&[f32]; 4] = [&[46., 50., 53., 57.], &[45., 48., 52., 55.], &[43., 46., 50., 53.], &[48., 52., 55., 58.]];
const DMAJ7_BM7_EM7_A7: [&[f32]; 4] = [&[50., 54., 57., 61.], &[47., 50., 54., 57.], &[52., 55., 59., 62.], &[45., 49., 52., 55.]];
const GMAJ7_FSM7_EM7_A7SUS: [&[f32]; 4] = [&[55., 59., 62., 66.], &[54., 57., 61., 64.], &[52., 55., 59., 62.], &[45., 50., 52., 55.]];
const AM9_FMAJ7_DM7_E7: [&[f32]; 4] = [&[45., 48., 52., 55., 59.], &[53., 57., 60., 64.], &[50., 53., 57., 60.], &[52., 56., 59., 62.]];
const FMAJ7_G6_EM7_AM7: [&[f32]; 4] = [&[53., 57., 60., 64.], &[55., 59., 62., 64.], &[52., 55., 59., 62.], &[45., 48., 52., 55.]];
const ABMAJ7_GM7_FM7_BB7: [&[f32]; 4] = [&[44., 48., 51., 55.], &[43., 46., 50., 53.], &[41., 44., 48., 51.], &[46., 50., 53., 56.]];
const EBMAJ7_CM7_FM7_BB7: [&[f32]; 4] = [&[51., 55., 58., 62.], &[48., 51., 55., 58.], &[41., 44., 48., 51.], &[46., 50., 53., 56.]];
// "Library Window Light" over Gm7 C9 Fmaj7 Dm7: "pages turning slowly / dust in golden light / I could stay here all day / reading through the night".
const LIBRARY_TUNE: [&[(f32, f32, f32)]; 8] = [
    &[(0.5, 70.0, 0.5), (1.0, 69.0, 0.5), (1.5, 67.0, 0.5), (2.0, 70.0, 1.0), (3.0, 74.0, 1.0)],
    &[(0.0, 72.0, 1.5), (1.5, 70.0, 0.5), (2.0, 67.0, 1.5)],
    &[(0.5, 69.0, 0.5), (1.0, 72.0, 0.5), (1.5, 76.0, 1.0), (2.5, 74.0, 0.5), (3.0, 72.0, 1.0)],
    &[(0.0, 69.0, 2.5)],
    &[(0.0, 74.0, 1.0), (1.0, 77.0, 0.5), (1.5, 74.0, 0.5), (2.0, 70.0, 1.5)],
    &[(0.5, 72.0, 0.5), (1.0, 74.0, 0.5), (1.5, 72.0, 0.5), (2.0, 70.0, 0.5), (2.5, 67.0, 1.5)],
    &[(0.0, 69.0, 0.5), (0.5, 72.0, 0.5), (1.0, 76.0, 1.0), (2.0, 77.0, 0.5), (2.5, 76.0, 0.5), (3.0, 74.0, 1.0)],
    &[(0.0, 72.0, 1.0), (1.0, 69.0, 2.5)],
];
// "Cat Nap Afternoon" over Dmaj7 Bm7 Em7 A7: "curled up in the sunlight / purring through the day / nothing on my mind now / let the hours drift away".
const CAT_NAP: [&[(f32, f32, f32)]; 8] = [
    &[(0.0, 73.0, 0.5), (0.5, 74.0, 0.5), (1.0, 78.0, 1.0), (2.0, 76.0, 0.5), (2.5, 74.0, 1.5)],
    &[(0.5, 71.0, 0.5), (1.0, 74.0, 0.5), (1.5, 78.0, 0.5), (2.0, 76.0, 1.0), (3.0, 74.0, 1.0)],
    &[(0.0, 71.0, 1.0), (1.0, 67.0, 0.5), (1.5, 71.0, 0.5), (2.0, 74.0, 1.5)],
    &[(0.5, 73.0, 0.5), (1.0, 76.0, 0.5), (1.5, 73.0, 0.5), (2.0, 69.0, 2.0)],
    &[(0.0, 78.0, 1.0), (1.0, 81.0, 1.0), (2.0, 78.0, 0.5), (2.5, 76.0, 1.5)],
    &[(0.0, 74.0, 1.5), (1.5, 73.0, 0.5), (2.0, 71.0, 2.0)],
    &[(0.5, 67.0, 0.5), (1.0, 71.0, 0.5), (1.5, 74.0, 0.5), (2.0, 79.0, 1.0), (3.0, 78.0, 0.5), (3.5, 76.0, 0.5)],
    &[(0.0, 73.0, 1.5), (1.5, 76.0, 0.5), (2.0, 74.0, 2.0)],
];
// "Midnight Ramen" over Am9 Fmaj7 Dm7 E7: "steam on the window / noodles in a bowl / city's still awake now / warming up my soul".
const RAMEN: [&[(f32, f32, f32)]; 8] = [
    &[(0.5, 72.0, 0.5), (1.0, 71.0, 0.5), (1.5, 69.0, 0.5), (2.0, 72.0, 1.0), (3.0, 76.0, 1.0)],
    &[(0.0, 77.0, 1.0), (1.0, 76.0, 0.5), (1.5, 72.0, 2.0)],
    &[(0.5, 69.0, 0.5), (1.0, 72.0, 0.5), (1.5, 74.0, 0.5), (2.0, 77.0, 0.5), (2.5, 76.0, 0.5), (3.0, 74.0, 1.0)],
    &[(0.0, 71.0, 1.0), (1.0, 68.0, 2.5)],
    &[(0.0, 76.0, 1.0), (1.0, 79.0, 0.5), (1.5, 76.0, 0.5), (2.0, 72.0, 1.5)],
    &[(0.5, 69.0, 0.5), (1.0, 72.0, 0.5), (1.5, 76.0, 0.5), (2.0, 77.0, 1.0), (3.0, 76.0, 1.0)],
    &[(0.0, 74.0, 0.5), (0.5, 72.0, 0.5), (1.0, 69.0, 1.0), (2.0, 72.0, 0.5), (2.5, 71.0, 1.5)],
    &[(0.0, 68.0, 1.0), (1.0, 71.0, 1.0), (2.0, 69.0, 2.0)],
];
// "Snowfall Study Hall" over Abmaj7 Gm7 Fm7 Bb7: "snow outside the window / quiet as can be / pencil tapping softly / just the lamp and me".
const SNOWFALL: [&[(f32, f32, f32)]; 8] = [
    &[(0.5, 72.0, 0.5), (1.0, 75.0, 0.5), (1.5, 79.0, 1.0), (2.5, 77.0, 0.5), (3.0, 75.0, 1.0)],
    &[(0.0, 74.0, 1.5), (1.5, 70.0, 0.5), (2.0, 67.0, 2.0)],
    &[(0.5, 68.0, 0.5), (1.0, 72.0, 0.5), (1.5, 75.0, 0.5), (2.0, 77.0, 1.0), (3.0, 75.0, 1.0)],
    &[(0.0, 74.0, 1.0), (1.0, 70.0, 2.5)],
    &[(0.0, 79.0, 1.0), (1.0, 80.0, 0.5), (1.5, 79.0, 0.5), (2.0, 75.0, 1.5)],
    &[(0.5, 70.0, 0.5), (1.0, 74.0, 0.5), (1.5, 77.0, 0.5), (2.0, 74.0, 1.0), (3.0, 70.0, 1.0)],
    &[(0.0, 72.0, 0.5), (0.5, 75.0, 0.5), (1.0, 77.0, 1.0), (2.0, 80.0, 0.5), (2.5, 79.0, 0.5), (3.0, 77.0, 1.0)],
    &[(0.0, 77.0, 1.0), (1.0, 74.0, 1.0), (2.0, 70.0, 2.0)],
];

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
    Marimba,     // short woody mallet pluck, bright and light
    Synth,       // soft square-wave lead: hollow and sharp, with a late vibrato
}

/// How the chord instrument plays each bar (PianoArp and Pad keep their own rhythm under Sustain).
#[derive(Clone, Copy, PartialEq)]
enum Comp {
    Sustain,    // the instrument's own rhythm: held chord with a strum and the odd push
    Stabs,      // short chords on the off-beats
    Charleston, // a short hit on 1 and a longer one on the "and" of 2
    Arp,        // chord tones climbing and falling in eighths, an octave up
}

/// How the bass moves under each bar.
#[derive(Clone, Copy, PartialEq)]
enum Bass {
    Push,    // the one, and a lazy push before beat three
    Long,    // one long round note per bar
    Glide,   // 808: a long sub that glides from the previous note
    Octaves, // bouncing between the root and its octave in eighths
    Walk,    // quarter notes: root, fifth, octave, fifth
    Synco,   // syncopated hits around the beat
}

#[derive(Clone, Copy, PartialEq)]
enum Groove {
    BoomBap, // lazy swung kick-snare, the classic lofi beat
    RnB,     // half-time: syncopated kick, clap, rolling 16th hats, gliding 808 bass
    Soft,    // no kick: rim and shaker only
    Ballad,  // soft kick on 1, brush on 3, long bass notes, slow sustained melody
    Bounce,  // crisp and quick: kick on 1 and the "and" of 2, snappy snare on 2 and 4, 16th hats, an open hat at the end
}

struct Track {
    name: &'static str,
    bpm: f32,
    transpose: f32,
    progs: [[&'static [f32]; 4]; 2],
    chords: Inst,
    lead: Inst,
    groove: Groove,
    comp: Comp,
    bass: Bass,
    lift: f32, // semitones the melody moves (12 = an octave up)
    melody: f32, // chance of a melody note per eighth (when there's no tune)
    tune: &'static [&'static [(f32, f32, f32)]], // written melody over progs[0], one bar per chord: (beat, MIDI note, beats long); empty = improvised
    warmth: f32, // low-pass amount: lower is darker
    lofi: bool,  // lofi hip-hop treatment: heavier swing, everything but the kick dips on each kick, quieter melody
}

// No two songs share their chord and melody instruments together, or their beat, chord rhythm and bass together.
const TRACKS: [Track; 10] = [
    Track { name: "Rainy Window Seat", bpm: 74.0, transpose: 0.0, progs: [FMAJ_EM_DM_CMAJ, DM9_G13_CMAJ9_AM7], chords: Inst::DustyKeys, lead: Inst::DustyKeys, groove: Groove::BoomBap, comp: Comp::Sustain, bass: Bass::Push, lift: 0.0, melody: 0.12, tune: &RAINY, warmth: 0.30, lofi: true },
    Track { name: "Last Bus Home", bpm: 70.0, transpose: -2.0, progs: [AM7_DM7_G7_CMAJ7, CMAJ_AM_FMAJ_G7], chords: Inst::Vibes, lead: Inst::Piano, groove: Groove::BoomBap, comp: Comp::Stabs, bass: Bass::Walk, lift: 0.0, melody: 0.12, tune: &LAST_BUS, warmth: 0.38, lofi: false },
    Track { name: "Sunday Matcha", bpm: 76.0, transpose: 1.0, progs: [CMAJ_AM_FMAJ_G7, FMAJ_EM_DM_CMAJ], chords: Inst::PianoArp, lead: Inst::MusicBox, groove: Groove::Soft, comp: Comp::Sustain, bass: Bass::Push, lift: 0.0, melody: 0.14, tune: &SUNDAY, warmth: 0.44, lofi: false },
    Track { name: "Night Shift Neon", bpm: 64.0, transpose: -3.0, progs: [EBMAJ_DM_CM_BBMAJ, BBMAJ_AM_GM_FMAJ], chords: Inst::Pad, lead: Inst::Keys, groove: Groove::RnB, comp: Comp::Sustain, bass: Bass::Glide, lift: 0.0, melody: 0.12, tune: &NIGHT_SHIFT, warmth: 0.33, lofi: false },
    Track { name: "Letters Never Sent", bpm: 58.0, transpose: 0.0, progs: [C_G_AM_F, C_G_AM_F], chords: Inst::Pad, lead: Inst::Flute, groove: Groove::Ballad, comp: Comp::Sustain, bass: Bass::Long, lift: 0.0, melody: 0.0, tune: &LETTERS, warmth: 0.42, lofi: false },
    Track { name: "Moonlit Walk Home", bpm: 60.0, transpose: -2.0, progs: [AM_F_C_G, AM_F_C_G], chords: Inst::PianoArp, lead: Inst::Piano, groove: Groove::Ballad, comp: Comp::Sustain, bass: Bass::Walk, lift: 0.0, melody: 0.0, tune: &MOONLIT, warmth: 0.44, lofi: false },
    Track { name: "Library Window Light", bpm: 86.0, transpose: 0.0, progs: [GM7_C9_FMAJ7_DM7, BBMAJ7_AM7_GM7_C7], chords: Inst::Keys, lead: Inst::Marimba, groove: Groove::Bounce, comp: Comp::Charleston, bass: Bass::Synco, lift: 0.0, melody: 0.12, tune: &LIBRARY_TUNE, warmth: 0.62, lofi: false },
    Track { name: "Cat Nap Afternoon", bpm: 82.0, transpose: 0.0, progs: [DMAJ7_BM7_EM7_A7, GMAJ7_FSM7_EM7_A7SUS], chords: Inst::Marimba, lead: Inst::Flute, groove: Groove::Soft, comp: Comp::Arp, bass: Bass::Long, lift: 0.0, melody: 0.12, tune: &CAT_NAP, warmth: 0.68, lofi: false },
    Track { name: "Midnight Ramen", bpm: 84.0, transpose: 0.0, progs: [AM9_FMAJ7_DM7_E7, FMAJ7_G6_EM7_AM7], chords: Inst::Keys, lead: Inst::Synth, groove: Groove::RnB, comp: Comp::Stabs, bass: Bass::Glide, lift: 0.0, melody: 0.12, tune: &RAMEN, warmth: 0.60, lofi: false },
    Track { name: "Snowfall Study Hall", bpm: 88.0, transpose: 0.0, progs: [ABMAJ7_GM7_FM7_BB7, EBMAJ7_CM7_FM7_BB7], chords: Inst::Pad, lead: Inst::Piano, groove: Groove::Bounce, comp: Comp::Sustain, bass: Bass::Octaves, lift: 12.0, melody: 0.12, tune: &SNOWFALL, warmth: 0.65, lofi: false },
];

/// How the next song is chosen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Loop,    // the playlist in order, then from the top
    LoopOne, // the same song again
    Shuffle, // a random song: all play once before any repeats, never the same twice in a row
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

// Songs compiled into the exe from assets/songs (see build.rs): (title, artist, mp3 bytes).
static SONGS: &[(&str, &str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/songs.rs"));

#[derive(Clone, PartialEq, Debug)]
enum Song {
    Bundled(usize), // one of SONGS
    Synth(usize),   // one of TRACKS
    File(PathBuf),  // from the music folder
}

impl Song {
    /// Built-in songs carry their artist's credit; the synthesized ones are hers.
    fn title(&self) -> String {
        match self {
            Song::Bundled(i) => format!("{} · {}", SONGS[*i].0, SONGS[*i].1),
            Song::Synth(i) => format!("{} · Claudesona", TRACKS[*i].name),
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
const AUDIO: [&str; 6] = ["mp3", "wav", "m4a", "aac", "flac", "ogg"];

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
    let built_in = (0..SONGS.len()).map(Song::Bundled).chain((0..TRACKS.len()).map(Song::Synth)).collect();
    let mut lists = vec![Playlist { name: BUILT_IN.into(), songs: built_in }];
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
/// `seen` holds the songs Shuffle has played this round: every song plays once before any repeats.
fn after(mode: Mode, cur: usize, len: usize, skip: bool, rng: &mut Rng, seen: &mut Vec<usize>) -> usize {
    match mode {
        Mode::LoopOne if !skip => cur,
        Mode::Shuffle if len > 1 => {
            seen.retain(|&i| i < len); // the playlist may have shrunk
            if !seen.contains(&cur) {
                seen.push(cur);
            }
            if seen.len() >= len {
                seen.clear(); // round over: start a new one
            }
            // `i != cur`: never the same song twice in a row, even across rounds.
            let left: Vec<usize> = (0..len).filter(|i| !seen.contains(i) && *i != cur).collect();
            let i = left[(rng.next() * left.len() as f32) as usize % left.len()];
            seen.push(i);
            i
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
    Volume(f32), // 0..=1
    Select(String), // make a playlist current by name, without playing
    Rescan,
    Voice(String), // one of her recorded lines (voice::say decided it should play)
    VoiceMp3(Vec<u8>), // a line just spoken live (voice::say_live)
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static LIBRARY: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(vec![]); // playlist names and song titles, for the UI
static PLAYER: OnceLock<Sender<Cmd>> = OnceLock::new();

/// Commands go to one player thread, which owns the sound device and the playing song.
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
pub fn set_volume(v: f32) {
    send(Cmd::Volume(v.clamp(0.0, 1.0)));
}
/// Restores the saved playlist at startup (the built-in one if that folder is gone).
pub fn select(playlist: &str) {
    send(Cmd::Select(playlist.to_string()));
}
/// Plays one of her recorded lines, under the music. Use voice::say, which decides whether she speaks.
/// Her voice is playing right now (the mic ignores her meanwhile).
pub fn speaking() -> bool {
    MUFFLED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn speak(text: String) {
    send(Cmd::Voice(text));
}
/// Plays mp3 bytes in her voice, under the music. Use voice::say_live.
pub fn speak_mp3(mp3: Vec<u8>) {
    send(Cmd::VoiceMp3(mp3));
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

/// One song at a time through rodio (the same on Windows, macOS and Linux): a synthesized track or a file.
struct Deck {
    out: Option<rodio::MixerDeviceSink>, // the sound device, opened on first play and kept
    player: Option<rodio::Player>,
    synth: Option<(usize, Vec<f32>)>, // rendered built-in track, kept so Loop 1 and seeking don't re-render
    len: f32,                         // seconds; 0 when a file doesn't say
    paused: bool,
    volume: f32,
}

impl Deck {
    fn silence(&mut self) {
        if let Some(p) = self.player.take() {
            p.stop();
        }
    }

    /// The sound device, opened on first use and kept. Err if there isn't one.
    fn device(&mut self) -> Result<&rodio::MixerDeviceSink, ()> {
        if self.out.is_none() {
            let mut out = rodio::DeviceSinkBuilder::open_default_sink().map_err(|_| ())?;
            out.log_on_drop(false);
            self.out = Some(out);
        }
        Ok(self.out.as_ref().unwrap())
    }

    /// Starts `song` `at` seconds in. Err if there's no sound device or the file can't be read or decoded.
    fn start(&mut self, song: &Song, at: f32, seed: u32) -> Result<(), ()> {
        use rodio::Source;
        self.silence();
        self.paused = false;
        let player = rodio::Player::connect_new(self.device()?.mixer());
        match song {
            Song::Synth(t) => {
                if self.synth.as_ref().is_none_or(|(i, _)| i != t) {
                    self.synth = Some((*t, render(&TRACKS[*t], seed)));
                }
                let pcm = &self.synth.as_ref().unwrap().1;
                self.len = pcm.len() as f32 / RATE as f32;
                let (mono, rate) = (std::num::NonZero::new(1).unwrap(), std::num::NonZero::new(RATE as u32).unwrap());
                player.append(muffle(rodio::buffer::SamplesBuffer::new(mono, rate, pcm.clone())));
            }
            Song::Bundled(i) => {
                let bytes = SONGS[*i].2;
                let decoder = rodio::Decoder::builder().with_data(std::io::Cursor::new(bytes)).with_byte_len(bytes.len() as u64).with_hint("mp3").build().map_err(|_| ())?;
                self.len = decoder.total_duration().map_or(0.0, |d| d.as_secs_f32());
                player.append(muffle(decoder));
            }
            Song::File(p) => {
                let decoder = rodio::Decoder::try_from(std::fs::File::open(p).map_err(|_| ())?).map_err(|_| ())?;
                self.len = decoder.total_duration().map_or(0.0, |d| d.as_secs_f32());
                player.append(muffle(decoder));
            }
        }
        player.set_volume(self.volume);
        if at > 0.0 {
            let _ = player.try_seek(Duration::from_secs_f32(at));
        }
        self.player = Some(player);
        Ok(())
    }

    /// Plays her voice (a newer line cuts off the one before), quieter than the music. None if it can't.
    fn say(&mut self, mp3: Option<Vec<u8>>) -> Option<rodio::Player> {
        let decoded = rodio::Decoder::new(std::io::Cursor::new(mp3?)).ok()?;
        let p = rodio::Player::connect_new(self.device().ok()?.mixer());
        p.set_volume(VOICE_VOLUME);
        p.append(decoded);
        Some(p)
    }

    fn pos(&self) -> f32 {
        self.player.as_ref().map_or(0.0, |p| p.get_pos().as_secs_f32())
    }

    fn ended(&self) -> bool {
        !self.paused && self.player.as_ref().is_some_and(|p| p.empty())
    }

    fn pause(&mut self) {
        if let Some(p) = &self.player {
            p.pause();
            self.paused = true;
        }
    }

    fn resume(&mut self) {
        if let Some(p) = &self.player {
            p.play();
            self.paused = false;
        }
    }

    fn seek(&mut self, secs: f32) {
        if let Some(p) = &self.player {
            let _ = p.try_seek(Duration::from_secs_f32(secs.max(0.0)));
        }
    }
}

/// Plays songs back to back from the current playlist until Stop, choosing each next one by the mode.
fn player(rx: Receiver<Cmd>) {
    let mut rng = Rng(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() | 1);
    let mut seed = 1u32;
    let mut lists = scan(&music_dir());
    publish(&lists);
    let mut deck = Deck { out: None, player: None, synth: None, len: 0.0, paused: false, volume: 1.0 };
    let (mut list, mut cur, mut mode) = (0usize, None::<usize>, Mode::Shuffle);
    let mut history: Vec<usize> = vec![]; // for Prev in shuffle
    let mut seen: Vec<usize> = vec![]; // played this shuffle round
    let mut voice = None::<rodio::Player>;
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
                    seen.clear();
                    deck.synth = None;
                }
                start = Some((s, 0.0));
            }
            (Some(Cmd::Play(None)), _) => {
                // Re-read the folder first: the saved playlist may have been changed or deleted since.
                refresh(&mut lists, &mut list, &mut cur);
                start = Some((first(mode, lists[list].songs.len(), &mut rng), 0.0));
            }
            (Some(Cmd::Next), Some(c)) => start = Some((after(mode, c, len, true, &mut rng, &mut seen), 0.0)),
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
            (Some(Cmd::Pause), Some(_)) if deck.paused => deck.resume(),
            (Some(Cmd::Pause), Some(_)) => deck.pause(),
            (Some(Cmd::Seek(s)), Some(_)) => deck.seek(if deck.len > 0.0 { s.min(deck.len) } else { s }),
            (Some(Cmd::Stop), _) => {
                deck.silence();
                (deck.synth, deck.paused, cur) = (None, false, None);
                history.clear();
                seen.clear();
            }
            (Some(Cmd::SetMode(m)), _) => mode = m,
        (Some(Cmd::Volume(v)), _) => {
            deck.volume = v;
            if let Some(p) = &deck.player {
                p.set_volume(v);
            }
        }
            (Some(Cmd::Select(name)), None) => list = lists.iter().position(|l| l.name == name).unwrap_or(0),
            (Some(Cmd::Voice(text)), _) => voice = deck.say(crate::voice::load(&text)).or(voice),
            (Some(Cmd::VoiceMp3(mp3)), _) => voice = deck.say(Some(mp3)).or(voice),
            (Some(Cmd::Rescan), _) => {
                let was_playing = cur.is_some();
                refresh(&mut lists, &mut list, &mut cur);
                if was_playing && cur.is_none() {
                    deck.silence(); // its file or folder is gone
                }
                history.clear(); // `seen` stays: opening the song list rescans, and that mustn't restart the shuffle round
            }
            (None, Some(c)) if deck.ended() => start = Some((after(mode, c, len, false, &mut rng, &mut seen), 0.0)),
            _ => {}
        }
        if voice.as_ref().is_some_and(|v| v.empty()) {
            voice = None;
        }
        MUFFLED.store(voice.is_some(), std::sync::atomic::Ordering::Relaxed);
        // Start the chosen song; if a file won't play, try the next ones (each at most once).
        // If nothing in the playlist plays, fall back to her own songs rather than going silent.
        if let Some((mut s, at)) = start {
            seed = seed.wrapping_mul(747_796_405).wrapping_add(rng.0);
            let mut tries = 0;
            let mut playing = true;
            while deck.start(&lists[list].songs[s], at, seed).is_err() {
                tries += 1;
                let len = lists[list].songs.len();
                if tries < len {
                    s = after(if mode == Mode::LoopOne { Mode::Loop } else { mode }, s, len, true, &mut rng, &mut seen);
                } else if list != 0 {
                    (list, tries, cur) = (0, 0, None);
                    history.clear();
                    seen.clear();
                    s = first(mode, lists[0].songs.len(), &mut rng);
                } else {
                    playing = false; // even built-in songs fail: there's no sound device
                    break;
                }
            }
            if !playing {
                cur = None;
            } else {
                if cur != Some(s) {
                    history.push(s);
                }
                cur = Some(s);
            }
        }
        *STATUS.lock().unwrap() = cur.map(|track| Status {
            playlist: list,
            track,
            name: lists[list].songs[track].title(),
            pos: if deck.len > 0.0 { deck.pos().min(deck.len) } else { deck.pos() },
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

/// Marimba: a woody bar, its tuned fourth partial ringing briefly over a short body.
fn marimba(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    add(buf, start, secs.min(1.2), |t| {
        let env = (t / 0.002).min(1.0) * (-t * 5.0).exp();
        let tone = (TAU * f * t).sin() + 0.3 * (TAU * 3.93 * f * t).sin() * (-t * 22.0).exp();
        vol * 1.2 * env * tone
    });
}

/// Soft square lead: odd harmonics (rounded off), a quick attack, and a vibrato that comes in late.
fn synth(buf: &mut [f32], start: f32, midi: f32, secs: f32, vol: f32) {
    let f = freq(midi);
    let mut phase = 0.0;
    add(buf, start, secs, |t| {
        phase += TAU * f * (1.0 + 0.005 * (TAU * 5.5 * t).sin() * ((t - 0.2) / 0.3).clamp(0.0, 1.0)) / RATE as f32;
        let env = (t / 0.01).min(1.0) * (-t * 0.8).exp() * ((secs - t) / 0.04).clamp(0.0, 1.0);
        let tone = phase.sin() + (3.0 * phase).sin() / 3.0 * 0.7 + (5.0 * phase).sin() / 5.0 * 0.4 + (7.0 * phase).sin() / 7.0 * 0.2;
        vol * 0.7 * env * tone
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
        Inst::Marimba => marimba(buf, start, midi, secs, vol),
        Inst::Synth => synth(buf, start, midi, secs, vol),
    }
}

/// A bar of the chord on `inst`, in that instrument's own rhythm.
fn chord_bar(buf: &mut [f32], inst: Inst, comp: Comp, t0: f32, beat: f32, chord: &[f32], rng: &mut Rng) {
    let bar = beat * 4.0;
    match comp {
        Comp::Stabs => {
            for k in [0.5, 1.5, 2.5, 3.5] {
                if k == 2.5 && rng.next() < 0.3 {
                    continue; // a gap now and then so it breathes
                }
                for (i, n) in chord.iter().enumerate() {
                    note(buf, inst, t0 + k * beat + i as f32 * 0.006, *n, beat * 0.35, 0.055);
                }
            }
            return;
        }
        Comp::Charleston => {
            for (k, len) in [(0.0, 1.2), (1.5, 2.3)] {
                for (i, n) in chord.iter().enumerate() {
                    note(buf, inst, t0 + k * beat + i as f32 * 0.01, *n, len * beat, 0.06);
                }
            }
            return;
        }
        Comp::Arp => {
            for (step, k) in [0, 1, 2, 3, 4, 3, 2, 1].into_iter().enumerate() {
                let n = chord.get(k).copied().unwrap_or(chord[0] + 12.0) + 12.0;
                note(buf, inst, t0 + step as f32 * beat / 2.0, n, beat * 0.9, 0.045);
            }
            return;
        }
        Comp::Sustain => {}
    }
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
        Inst::Keys | Inst::Vibes | Inst::Piano | Inst::MusicBox | Inst::Flute | Inst::DustyKeys | Inst::Marimba | Inst::Synth => {
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

/// A round bass note with a soft attack and release (no clicks on short notes).
fn bass_note(buf: &mut [f32], at: f32, midi: f32, secs: f32, vol: f32, decay: f32) {
    let f = freq(midi);
    add(buf, at, secs, |t| vol * (t / 0.01).min(1.0) * ((secs - t) / 0.015).clamp(0.0, 1.0) * (-t * decay).exp() * ((TAU * f * t).sin() + 0.2 * (TAU * 2.0 * f * t).sin()));
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
            chord_bar(&mut buf, track.chords, track.comp, t0, beat, &chord, &mut rng);
            // Bass in the 41-82 Hz octave.
            let root = (chord[0] - 28.0).rem_euclid(12.0) + 28.0;
            match track.bass {
                Bass::Glide => {
                    // 808: a long sub that glides up from the previous note.
                    let (from, to) = (freq(if last_bass > 0.0 { last_bass } else { root }), freq(root));
                    let mut phase = 0.0;
                    add(&mut buf, t0, bar * 0.95, |t| {
                        let f = to + (from - to) * (-t * 18.0).exp();
                        phase += TAU * f / RATE as f32;
                        0.32 * (t / 0.005).min(1.0) * (-t * 0.9).exp() * (phase.sin() * 1.4).tanh()
                    });
                }
                Bass::Long => {
                    let f = freq(root);
                    add(&mut buf, t0, bar, |t| 0.2 * (t / 0.02).min(1.0) * (-t * 0.8).exp() * ((TAU * f * t).sin() + 0.15 * (TAU * 2.0 * f * t).sin()));
                }
                Bass::Push => {
                    for at in [0.0, 1.75] {
                        bass_note(&mut buf, t0 + at * beat, root, beat * 2.0, 0.22, 2.2);
                    }
                }
                Bass::Octaves => {
                    for e in 0..8 {
                        let up = e % 2 == 1;
                        bass_note(&mut buf, t0 + e as f32 * beat / 2.0, root + if up { 12.0 } else { 0.0 }, beat * 0.4, if up { 0.12 } else { 0.2 }, 4.0);
                    }
                }
                Bass::Walk => {
                    for (q, step) in [0.0, 7.0, 12.0, 7.0].into_iter().enumerate() {
                        bass_note(&mut buf, t0 + q as f32 * beat, root + step, beat * 0.9, 0.19, 2.0);
                    }
                }
                Bass::Synco => {
                    for (at, len) in [(0.0, 0.6), (0.75, 0.4), (2.0, 0.6), (2.75, 0.4), (3.5, 0.4)] {
                        bass_note(&mut buf, t0 + at * beat, root, len * beat, 0.21, 3.0);
                    }
                }
            }
            last_bass = root;
            if section >= 1 && sing {
                for &(at, n, beats) in track.tune[b % track.tune.len()] {
                    let vol = if track.lofi { 0.055 } else { 0.075 }; // lofi keeps the melody back in the mix
                    note(&mut buf, track.lead, t0 + at * beat, n + track.transpose + track.lift, beats * beat * 1.15, vol);
                }
            } else if section >= 1 {
                for step in 0..8 {
                    if rng.next() < track.melody {
                        let n = chord[(rng.next() * chord.len() as f32) as usize % chord.len()] + 12.0 + track.lift;
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
                Groove::Bounce => {
                    for k in [0.0, 1.5, 2.5] {
                        if k != 2.5 || rng.next() < 0.5 {
                            kick(&mut buf, t0 + k * beat, 0.42);
                        }
                    }
                    // Snappy snare: a short bright burst plus a tight body tone.
                    for s in [1.0, 3.0] {
                        noise_hit(&mut buf, t0 + s * beat, 0.16, 24.0, 0.75, false, 0.15, rng.0);
                        add(&mut buf, t0 + s * beat, 0.08, |t| 0.07 * (-t * 35.0).exp() * (TAU * 210.0 * t).sin());
                    }
                    for h in 0..16 {
                        let at = t0 + h as f32 * beat / 4.0;
                        if h == 14 {
                            noise_hit(&mut buf, at, 0.22, 16.0, 0.5, true, 0.028, rng.0); // open hat
                        } else {
                            noise_hit(&mut buf, at, 0.035, 110.0, 0.5, true, if h % 2 == 0 { 0.03 } else { 0.017 }, rng.0);
                        }
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

/// 16-bit mono PCM WAV file bytes (tests write real files with it).
#[cfg(test)]
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
            // Written melodies: eight bars, each note inside its bar and in a singable range.
            assert!(track.tune.is_empty() || track.tune.len() == 8, "track {i}: tune isn't 8 bars");
            for (b, bar) in track.tune.iter().enumerate() {
                for &(at, n, beats) in *bar {
                    assert!(at + beats <= 4.0 && (60.0..=84.0).contains(&n), "track {i} bar {b}: note {n} at {at} for {beats}");
                }
            }
        }
        // Every song sounds like itself: no shared instrument pair, no shared beat + chord rhythm + bass.
        for (i, a) in TRACKS.iter().enumerate() {
            for b in &TRACKS[i + 1..] {
                assert!((a.chords, a.lead) != (b.chords, b.lead), "{} and {} share instruments", a.name, b.name);
                assert!((a.groove, a.comp, a.bass) != (b.groove, b.comp, b.bass), "{} and {} share a rhythm section", a.name, b.name);
            }
        }
        let bytes = wav(&[0.0; 10]);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 20);
    }

    #[test]
    fn muffle_softens_highs_and_leaves_clear_music_alone() {
        let tone = |hz: f32| -> Vec<f32> { (0..441).map(|i| (TAU * hz * i as f32 / 44_100.0).sin()).collect() };
        let run = |amount: f32, hz: f32| {
            let src = rodio::buffer::SamplesBuffer::new(std::num::NonZero::new(1).unwrap(), std::num::NonZero::new(44_100).unwrap(), tone(hz));
            let out: Vec<f32> = Muffle { input: src, lp: [0.0; 8], amount, ch: 0 }.collect();
            (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt()
        };
        let full = (0.5f32).sqrt(); // a sine's RMS
        assert!(run(1.0, 5000.0) < 0.2 * full); // 10ms: still almost fully muffled, highs mostly gone
        assert!((run(0.0, 5000.0) - full).abs() < 0.02 || MUFFLED.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn bundled_songs_decode_with_a_length() {
        for (title, artist, bytes) in SONGS {
            let d = rodio::Decoder::builder().with_data(std::io::Cursor::new(*bytes)).with_byte_len(bytes.len() as u64).with_hint("mp3").build();
            let secs = d.ok().and_then(|d| rodio::Source::total_duration(&d)).map_or(0.0, |t| t.as_secs_f32());
            assert!(secs > 60.0, "{title} by {artist}: {secs}s");
        }
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
        assert_eq!(names[0].1.len(), SONGS.len() + TRACKS.len());
        assert_eq!(names[1], (LOOSE, vec!["a".to_string(), "b".to_string()]));
        assert_eq!(names[2], ("Chill", vec!["1".to_string(), "2".to_string()]));
        assert_eq!(names.len(), 3); // no playlist for a folder without audio
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn modes_pick_the_next_song() {
        let mut rng = Rng(7);
        assert_eq!(after(Mode::Loop, 2, 3, false, &mut rng, &mut vec![]), 0);
        assert_eq!(after(Mode::LoopOne, 1, 3, false, &mut rng, &mut vec![]), 1);
        assert_eq!(after(Mode::LoopOne, 1, 3, true, &mut rng, &mut vec![]), 2); // Next still moves on
        for _ in 0..20 {
            assert_ne!(after(Mode::Shuffle, 1, 3, false, &mut rng, &mut vec![]), 1);
        }
        assert_eq!(after(Mode::Shuffle, 0, 1, false, &mut rng, &mut vec![]), 0); // a one-song playlist just repeats
        // Shuffle plays every song once per round, never repeating back to back across rounds.
        let (mut seen, mut cur) = (vec![], 3);
        let mut round = vec![cur];
        for _ in 0..9 {
            cur = after(Mode::Shuffle, cur, 10, false, &mut rng, &mut seen);
            assert!(!round.contains(&cur));
            round.push(cur);
        }
        for _ in 0..5 {
            let mut round = vec![];
            for _ in 0..10 {
                let next = after(Mode::Shuffle, cur, 10, false, &mut rng, &mut seen);
                assert!(next != cur && !round.contains(&next));
                round.push(next);
                cur = next;
            }
        }
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

// Plays a real file for a few seconds: run with `cargo test --release -- --ignored`.
#[test]
#[ignore]
fn deck_plays_a_file_with_pause_and_seek() {
    let dir = std::env::temp_dir().join(format!("claudesona-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone song.wav"); // a space, like real file names
    let tone: Vec<f32> = (0..RATE * 6).map(|i| 0.1 * (TAU * 440.0 * i as f32 / RATE as f32).sin()).collect();
    std::fs::write(&path, wav(&tone)).unwrap();
    let mut deck = Deck { out: None, player: None, synth: None, len: 0.0, paused: false, volume: 1.0 };
    deck.start(&Song::File(path.clone()), 0.0, 1).expect("plays the WAV");
    assert!((deck.len - 6.0).abs() < 0.1, "len {}", deck.len);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(deck.pos() > 0.8 && !deck.ended(), "pos {}", deck.pos());
    deck.pause();
    let held = deck.pos();
    std::thread::sleep(Duration::from_millis(400));
    assert!((deck.pos() - held).abs() < 0.05);
    deck.seek(5.5);
    deck.resume();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(deck.ended(), "should have reached the end, pos {}", deck.pos());
    deck.silence();
    assert!(deck.start(&Song::File(dir.join("missing.mp3")), 0.0, 1).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
