// Talking to her: microphone → loudness gate → Whisper → wake word. See SPEC-v2.md, "Talking to her".
// Whisper `small` runs on the CPU through candle (pure Rust, so every target builds without a C toolchain).
// The model is downloaded into model/whisper-small/ next to the exe on first use; until then STT is off.

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{LayerNorm, Module, VarBuilder};
use candle_transformers::models::whisper::{self, Config, audio, model::AudioEncoder, model::Whisper};
use rodio::cpal::{self, traits::DeviceTrait, traits::HostTrait, traits::StreamTrait};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

const MODEL_URL: &str = "https://huggingface.co/openai/whisper-small/resolve/main/";
const MODEL_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];
const RATE: u32 = whisper::SAMPLE_RATE as u32;
const FRAME: usize = 480; // 30ms gate frames
const PREROLL: usize = 10; // quiet frames kept before speech, so the first syllable isn't clipped
const HANGOVER: usize = 20; // 600ms of quiet ends an utterance
const MIN_VOICED: usize = 8; // fewer loud frames than this (240ms) was a click or a cough
const MAX_FRAMES: usize = 500; // cut at 15s
const MIN_RMS: f32 = 0.002; // -54 dBFS: quieter than this is never speech (a DGM20 USB mic's speech sits at 0.005-0.03)
const FOLLOW_UP: Duration = Duration::from_secs(6); // after a bare "Claudesona", the next sentence is a command
const MAX_TOKENS: usize = 120;
const PEEK: usize = RATE as usize * 5 / 2; // 2.5s: enough to hear "hey Claudesona" at the start of a clip
const HINT: &str = " Claudesona, Claudesona ơi."; // Whisper's "previous text": teaches it her name in EN and VI

#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub text: String,
    pub wake_only: bool, // just "Claudesona": turn toward the user, the next sentence follows
}

#[derive(Clone)]
pub struct Handle {
    listening: Arc<AtomicBool>,
    ptt: Arc<AtomicBool>,
    ready: Arc<AtomicBool>, // model loaded: only then is the mic opened
}

impl Handle {
    /// Off while she's asleep, hidden or speaking (so she doesn't hear herself).
    pub fn set_listening(&self, on: bool) {
        self.listening.store(on, Ordering::Relaxed);
    }
    /// The model is loaded, so the mic can open.
    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }
    /// Mouse held on her: record regardless of the wake word, transcribe on release.
    pub fn push_to_talk(&self, held: bool) {
        self.ptt.store(held, Ordering::Relaxed);
    }
}

pub fn start(on_heard: impl FnMut(Heard) + Send + 'static) -> Handle {
    let h = Handle { listening: Arc::new(AtomicBool::new(true)), ptt: Arc::default(), ready: Arc::default() };
    let (tx, rx) = channel();
    let c = h.clone();
    std::thread::spawn(move || capture(&c, tx));
    let w = h.clone();
    std::thread::spawn(move || listen(&w, rx, on_heard));
    h
}

fn folder() -> PathBuf {
    let exe = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.to_path_buf())).unwrap_or_default();
    exe.join("model").join("whisper-small")
}

/// Downloads whatever model files are missing. Same as chat.rs's embeddings download.
fn fetch(folder: &Path) -> bool {
    let _ = std::fs::create_dir_all(folder);
    MODEL_FILES.iter().all(|f| {
        let (part, done) = (folder.join(format!("{f}.part")), folder.join(f));
        // shortcut: curl ships with Windows 10+, macOS and most Linux distros; add an HTTP crate if one lacks it.
        let mut cmd = std::process::Command::new("curl");
        cmd.args(["-fsSL", "--retry", "2", "-o"]).arg(&part).arg(format!("{MODEL_URL}{f}"));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let ok = done.exists() || cmd.status().is_ok_and(|s| s.success()) && std::fs::rename(&part, &done).is_ok();
        if !ok {
            let _ = std::fs::remove_file(&part);
        }
        ok
    })
}

// ---- microphone ----

/// Keeps the mic open only while someone may be talking to her, so the OS mic indicator tells the truth.
fn capture(h: &Handle, tx: Sender<Vec<f32>>) {
    let mut stream = None;
    loop {
        let want = h.ready.load(Ordering::Relaxed) && (h.listening.load(Ordering::Relaxed) || h.ptt.load(Ordering::Relaxed));
        if !want {
            stream = None;
        } else if stream.is_none() {
            match open_mic(tx.clone()) {
                Ok(s) => stream = Some(s),
                Err(e) => {
                    eprintln!("stt: no microphone: {e}");
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn open_mic(tx: Sender<Vec<f32>>) -> Result<cpal::Stream, String> {
    let dev = cpal::default_host().default_input_device().ok_or("no input device")?;
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let config = cfg.config();
    fn build<T: cpal::SizedSample>(dev: &cpal::Device, config: cpal::StreamConfig, tx: Sender<Vec<f32>>) -> Result<cpal::Stream, cpal::Error>
    where
        f32: cpal::FromSample<T>,
    {
        let channels = config.channels.max(1) as usize;
        let mut rs = Resampler::new(config.sample_rate);
        dev.build_input_stream::<T, _, _>(
            config,
            move |data, _| {
                let mut out = Vec::with_capacity(data.len() / channels);
                for frame in data.chunks(channels) {
                    let mono = frame.iter().map(|&s| cpal::Sample::to_sample::<f32>(s)).sum::<f32>() / channels as f32;
                    rs.push(mono, &mut out);
                }
                let _ = tx.send(out);
            },
            |e| eprintln!("stt: mic error: {e}"),
            None,
        )
    }
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&dev, config, tx),
        cpal::SampleFormat::I16 => build::<i16>(&dev, config, tx),
        cpal::SampleFormat::I32 => build::<i32>(&dev, config, tx),
        cpal::SampleFormat::U16 => build::<u16>(&dev, config, tx),
        f => return Err(format!("unsupported sample format {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Any rate to 16 kHz by averaging the input samples that fall in each output sample.
// shortcut: a box filter lets some aliasing through; fine for speech, use a windowed-sinc if accuracy suffers.
struct Resampler {
    rate: u32,
    pos: u32,
    acc: f32,
    n: u32,
    last: f32,
}

impl Resampler {
    fn new(rate: u32) -> Self {
        Resampler { rate: rate.max(1), pos: 0, acc: 0.0, n: 0, last: 0.0 }
    }
    fn push(&mut self, x: f32, out: &mut Vec<f32>) {
        self.acc += x;
        self.n += 1;
        self.pos += RATE;
        while self.pos >= self.rate {
            if self.n > 0 {
                self.last = self.acc / self.n as f32;
                (self.acc, self.n) = (0.0, 0);
            }
            out.push(self.last);
            self.pos -= self.rate;
        }
    }
}

// ---- loudness gate ----

/// Cuts 16 kHz audio into utterances: loud frames start one, `HANGOVER` quiet frames end it.
/// "Loud" is 3× a noise floor that drops fast and rises slowly, so a fan becomes background within seconds.
// shortcut: thresholds are tuned on paper; calibrate MIN_RMS and the 3× against real mics.
#[derive(Default)]
struct Gate {
    frame: Vec<f32>,
    utt: Vec<f32>,
    talking: bool,
    voiced: usize,
    quiet: usize,
    noise: f32,
}

impl Gate {
    fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        self.frame.extend_from_slice(samples);
        while self.frame.len() >= FRAME {
            let f: Vec<f32> = self.frame.drain(..FRAME).collect();
            let rms = (f.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
            let loud = rms > (self.noise * 3.0).max(MIN_RMS);
            self.noise = if self.noise == 0.0 { rms } else if rms < self.noise { self.noise * 0.8 + rms * 0.2 } else { self.noise * 0.995 + rms * 0.005 };
            self.utt.extend_from_slice(&f);
            if !self.talking {
                if loud {
                    (self.talking, self.voiced, self.quiet) = (true, 1, 0);
                } else if self.utt.len() > PREROLL * FRAME {
                    self.utt.drain(..self.utt.len() - PREROLL * FRAME);
                }
                continue;
            }
            if loud {
                (self.voiced, self.quiet) = (self.voiced + 1, 0);
            } else {
                self.quiet += 1;
            }
            if self.quiet >= HANGOVER || self.utt.len() >= MAX_FRAMES * FRAME {
                let utt = std::mem::take(&mut self.utt);
                if self.voiced >= MIN_VOICED {
                    out.push(utt);
                }
                self.talking = false;
            }
        }
        out
    }

    fn reset(&mut self) {
        let noise = self.noise;
        *self = Gate { noise, ..Gate::default() };
    }
}

// ---- the listening loop ----

fn listen(h: &Handle, rx: Receiver<Vec<f32>>, mut on_heard: impl FnMut(Heard)) {
    let dir = folder();
    let mut stt = loop {
        if fetch(&dir) {
            match Stt::load(&dir) {
                Ok(s) => break s,
                Err(e) => eprintln!("stt: can't load Whisper: {e}"),
            }
        }
        std::thread::sleep(Duration::from_secs(60)); // offline, or a broken download: try again later
    };
    h.ready.store(true, Ordering::Relaxed);
    let mut gate = Gate::default();
    let mut held: Option<Vec<f32>> = None; // push-to-talk recording
    let mut follow_up: Option<Instant> = None; // a bare wake word was heard; a command may follow until then
    loop {
        let chunk = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => Vec::new(), // still notice a push-to-talk release when the mic closes
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if h.ptt.load(Ordering::Relaxed) {
            held.get_or_insert_with(Vec::new).extend_from_slice(&chunk);
            gate.reset();
            continue;
        }
        if let Some(pcm) = held.take() {
            if pcm.len() >= MIN_VOICED * FRAME {
                if let Some(text) = stt.transcribe(&pcm) {
                    on_heard(Heard { text, wake_only: false });
                }
            }
            continue;
        }
        if !h.listening.load(Ordering::Relaxed) {
            gate.reset();
            follow_up = None;
            continue;
        }
        for utt in gate.push(&chunk) {
            let began = Instant::now() - Duration::from_secs_f32(utt.len() as f32 / RATE as f32);
            let following = follow_up.is_some_and(|t| began <= t);
            if !following && !stt.starts_with_name(&utt) {
                continue; // not for her: skip the full transcription
            }
            let Some(text) = stt.transcribe(&utt) else { continue };
            if let Some(heard) = route(&text, following) {
                follow_up = heard.wake_only.then(|| Instant::now() + FOLLOW_UP);
                on_heard(heard);
            }
        }
    }
}

/// What a transcript means: a command after the wake word, a bare wake word, or (right after one) a command.
fn route(text: &str, following_up: bool) -> Option<Heard> {
    match after_wake(text) {
        Some(rest) if rest.is_empty() => Some(Heard { text: String::new(), wake_only: true }),
        Some(rest) => Some(Heard { text: rest, wake_only: false }),
        None if following_up => Some(Heard { text: text.trim().to_string(), wake_only: false }),
        None => None,
    }
}

// ---- wake word ----

const FILLERS: [&str; 13] = ["hey", "hi", "hello", "ok", "okay", "oh", "um", "uh", "so", "alo", "à", "ê", "này"];
const VOCATIVES: [&str; 2] = ["ơi", "oi"]; // Vietnamese "Claudesona ơi" = "hey Claudesona"

fn clean(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The text after a leading "Claudesona" (heard loosely: "Claude Sona", "Cloud Zona", "Klaus Sona"…),
/// or None if the transcript doesn't start with her name. Empty means just her name.
fn after_wake(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    let mut skipped = 0;
    while i < words.len() {
        let c = clean(words[i]);
        if !c.is_empty() && (skipped == 2 || !FILLERS.contains(&c.as_str())) {
            break;
        }
        skipped += usize::from(!c.is_empty());
        i += 1;
    }
    let mut joined = String::new();
    for (k, w) in words.iter().enumerate().skip(i).take(4) {
        joined += &clean(w);
        if sounds_like_name(&joined) {
            let mut rest = &words[k + 1..];
            if rest.first().is_some_and(|w| VOCATIVES.contains(&clean(w).as_str())) {
                rest = &rest[1..];
            }
            let rest = rest.join(" ");
            return Some(rest.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string());
        }
    }
    None
}

fn sounds_like_name(word: &str) -> bool {
    let mut s: Vec<char> = word.replace("owner", "ona").replace("sauna", "sona").replace('k', "c").replace('z', "s").replace("ou", "au").replace("aw", "au").chars().collect();
    s.dedup(); // "sonna" → "sona", "klaussona" → "clausona"
    let s: String = s.into_iter().collect();
    // Must end like "-sona"/"-soner" so "Claude, so now…" and "Claude Sonnet" don't count.
    let close = s.starts_with("cl") && (s.ends_with("na") || s.ends_with("ner") || s.ends_with("nar")) && edits(&s, "claudesona") <= 3;
    // Whisper also hears "Klautsana", "Klotzane", "Clots on a", "Clô đơ sô na": compare consonant skeletons too.
    let k = skeleton(word);
    close || k.starts_with("cl") && k.ends_with("na") && edits(&k, "cladasana") <= 1
}

/// Every vowel run (Vietnamese ones too) becomes "a", and sound-alike consonants merge: "klotzane" → "cladsana".
fn skeleton(word: &str) -> String {
    let mut s: Vec<char> = word
        .chars()
        .map(|c| match c {
            'k' => 'c',
            'z' => 's',
            't' | 'đ' => 'd',
            c if "aeiouy".contains(c) || !c.is_ascii() => 'a',
            c => c,
        })
        .collect();
    s.dedup();
    s.into_iter().collect()
}

fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != b[j])).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

// ---- Whisper ----
// Candle's encoder, our own decoder. Candle's re-runs every earlier token at each step, and its Linear multiplies
// by a transposed weight, which copies the whole matrix per call: ~1s per token for `small`. This one caches
// self-attention keys and values, feeds one token at a time, and keeps weights pre-transposed.

/// A linear layer stored in × out, so a row times it reads the weights in order.
struct Lin {
    wt: Tensor,
    b: Option<Tensor>,
}

impl Lin {
    fn load(i: usize, o: usize, bias: bool, vb: VarBuilder) -> candle_core::Result<Lin> {
        let wt = vb.get((o, i), "weight")?.t()?.contiguous()?;
        Ok(Lin { wt, b: if bias { Some(vb.get(o, "bias")?) } else { None } })
    }
    fn forward(&self, x: &Tensor) -> candle_core::Result<Tensor> {
        let y = x.matmul(&self.wt)?;
        match &self.b {
            Some(b) => y.broadcast_add(b),
            None => Ok(y),
        }
    }
}

struct Attn {
    q: Lin,
    k: Lin,
    v: Lin,
    out: Lin,
    heads: usize,
    cache: Option<(Tensor, Tensor)>, // keys (heads × d/heads × tokens) and values (heads × tokens × d/heads) so far
}

impl Attn {
    fn load(d: usize, heads: usize, vb: VarBuilder) -> candle_core::Result<Attn> {
        let (q, k) = (Lin::load(d, d, true, vb.pp("q_proj"))?, Lin::load(d, d, false, vb.pp("k_proj"))?);
        let (v, out) = (Lin::load(d, d, true, vb.pp("v_proj"))?, Lin::load(d, d, true, vb.pp("out_proj"))?);
        Ok(Attn { q, k, v, out, heads, cache: None })
    }

    /// One token (1 × d) in. Self-attention (`audio` None) grows the cache; cross-attention fills it once per utterance.
    fn forward(&mut self, x: &Tensor, audio: Option<&Tensor>) -> candle_core::Result<Tensor> {
        let heads = |t: &Tensor| -> candle_core::Result<Tensor> {
            let (n, d) = t.dims2()?;
            t.reshape((n, self.heads, d / self.heads))?.transpose(0, 1)?.contiguous() // heads × n × d/heads
        };
        let kv = |src: &Tensor| -> candle_core::Result<(Tensor, Tensor)> { Ok((heads(&self.k.forward(src)?)?.t()?.contiguous()?, heads(&self.v.forward(src)?)?)) };
        let (kt, vh) = match (audio, self.cache.take()) {
            (Some(_), Some(kv)) => kv,
            (Some(a), None) => kv(a)?,
            (None, None) => kv(x)?,
            (None, Some((kt, vh))) => {
                let (k1, v1) = kv(x)?;
                (Tensor::cat(&[kt, k1], 2)?, Tensor::cat(&[vh, v1], 1)?)
            }
        };
        let qh = heads(&self.q.forward(x)?)?;
        let scale = 1.0 / ((qh.dim(2)? as f64).sqrt());
        let w = candle_nn::ops::softmax_last_dim(&(qh.matmul(&kt)? * scale)?)?;
        let y = w.matmul(&vh)?.transpose(0, 1)?.contiguous()?.flatten_from(1)?;
        self.cache = Some((kt, vh));
        self.out.forward(&y)
    }
}

struct Layer {
    attn: Attn,
    attn_ln: LayerNorm,
    cross: Attn,
    cross_ln: LayerNorm,
    fc1: Lin,
    fc2: Lin,
    mlp_ln: LayerNorm,
}

struct Decoder {
    embed: Tensor,   // vocab × d
    unembed: Tensor, // d × vocab
    pos: Tensor,
    layers: Vec<Layer>,
    ln: LayerNorm,
    n: usize, // tokens fed since reset
}

impl Decoder {
    fn load(cfg: &Config, vb: VarBuilder) -> candle_core::Result<Decoder> {
        let (d, heads) = (cfg.d_model, cfg.decoder_attention_heads);
        let layers = (0..cfg.decoder_layers)
            .map(|i| {
                let vb = vb.pp(format!("layers.{i}"));
                Ok(Layer {
                    attn: Attn::load(d, heads, vb.pp("self_attn"))?,
                    attn_ln: candle_nn::layer_norm(d, 1e-5, vb.pp("self_attn_layer_norm"))?,
                    cross: Attn::load(d, heads, vb.pp("encoder_attn"))?,
                    cross_ln: candle_nn::layer_norm(d, 1e-5, vb.pp("encoder_attn_layer_norm"))?,
                    fc1: Lin::load(d, 4 * d, true, vb.pp("fc1"))?,
                    fc2: Lin::load(4 * d, d, true, vb.pp("fc2"))?,
                    mlp_ln: candle_nn::layer_norm(d, 1e-5, vb.pp("final_layer_norm"))?,
                })
            })
            .collect::<candle_core::Result<_>>()?;
        let embed = vb.get((cfg.vocab_size, d), "embed_tokens.weight")?;
        Ok(Decoder {
            unembed: embed.t()?.contiguous()?,
            embed,
            pos: vb.get((cfg.max_target_positions, d), "embed_positions.weight")?,
            layers,
            ln: candle_nn::layer_norm(d, 1e-5, vb.pp("layer_norm"))?,
            n: 0,
        })
    }

    fn reset(&mut self) {
        self.n = 0;
        for l in &mut self.layers {
            (l.attn.cache, l.cross.cache) = (None, None);
        }
    }

    /// Feeds one token; returns the logits for the next one. `audio` is the encoder output, frames × d.
    fn step(&mut self, token: u32, audio: &Tensor) -> candle_core::Result<Vec<f32>> {
        let mut x = (self.embed.i(token as usize)? + self.pos.i(self.n)?)?.unsqueeze(0)?;
        self.n += 1;
        for l in &mut self.layers {
            x = (&x + l.attn.forward(&l.attn_ln.forward(&x)?, None)?)?;
            x = (&x + l.cross.forward(&l.cross_ln.forward(&x)?, Some(audio))?)?;
            x = (&x + l.fc2.forward(&l.fc1.forward(&l.mlp_ln.forward(&x)?)?.gelu()?)?)?;
        }
        self.ln.forward(&x)?.matmul(&self.unembed)?.i(0)?.to_vec1::<f32>()
    }
}

struct Stt {
    encoder: AudioEncoder,
    decoder: Decoder,
    config: Config,
    tok: tokenizers::Tokenizer,
    filters: Vec<f32>,
    sot: u32,
    eot: u32,
    transcribe: u32,
    no_timestamps: u32,
    no_speech: u32,
    langs: [u32; 2],
    hint: Vec<u32>, // <|startofprev|> + her name: Whisper otherwise hears "Persona", "close owner"…
}

impl Stt {
    fn load(dir: &Path) -> Result<Stt, String> {
        let config: Config = serde_json::from_slice(&std::fs::read(dir.join("config.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let tok = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| e.to_string())?;
        let id = |t: &str| tok.token_to_id(t).ok_or(format!("tokenizer has no {t}"));
        // Safety: the file isn't changed while mapped (it's only ever written as .part, then renamed).
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], DType::F32, &Device::Cpu) }.map_err(|e| e.to_string())?;
        let err = |e: candle_core::Error| e.to_string();
        Ok(Stt {
            encoder: Whisper::load(&vb, config.clone()).map_err(err)?.encoder, // its decoder is dropped
            decoder: Decoder::load(&config, vb.pp("model.decoder")).map_err(err)?,
            filters: mel_filters(config.num_mel_bins),
            config,
            sot: id(whisper::SOT_TOKEN)?,
            eot: id(whisper::EOT_TOKEN)?,
            transcribe: id(whisper::TRANSCRIBE_TOKEN)?,
            no_timestamps: id(whisper::NO_TIMESTAMPS_TOKEN)?,
            no_speech: whisper::NO_SPEECH_TOKENS.iter().find_map(|t| tok.token_to_id(t)).ok_or("tokenizer has no no-speech token")?,
            langs: [id("<|en|>")?, id("<|vi|>")?],
            hint: std::iter::once(id("<|startofprev|>")?).chain(tok.encode(HINT, false).map_err(|e| e.to_string())?.get_ids().iter().copied()).collect(),
            tok,
        })
    }

    /// 16 kHz mono in, text out; None for silence, noise or a failure.
    fn transcribe(&mut self, pcm: &[f32]) -> Option<String> {
        self.run(pcm, MAX_TOKENS).map_err(|e| eprintln!("stt: {e}")).ok().flatten()
    }

    /// Cheap check before a full transcription: does the clip start with her name?
    /// Only its first 2.5s and a few tokens, so chatter around her costs a fraction of the CPU.
    fn starts_with_name(&mut self, pcm: &[f32]) -> bool {
        let head = &pcm[..pcm.len().min(PEEK)];
        self.run(head, 8).map_err(|e| eprintln!("stt: {e}")).ok().flatten().and_then(|t| after_wake(&t)).is_some()
    }

    fn run(&mut self, pcm: &[f32], max_tokens: usize) -> candle_core::Result<Option<String>> {
        let n_mels = self.config.num_mel_bins;
        let mel = audio::pcm_to_mel(&self.config, pcm, &self.filters);
        let frames = mel.len() / n_mels;
        // shortcut: encode only the audio we have (+1s) instead of Whisper's fixed 30s window; several times
        // faster, and fine for short commands. Use all 3000 frames if transcripts get worse.
        let keep = ((pcm.len() / whisper::HOP_LENGTH + 100).div_ceil(2) * 2).min(whisper::N_FRAMES).min(frames);
        let mel = Tensor::from_vec(mel, (1, n_mels, frames), &Device::Cpu)?.narrow(2, 0, keep)?;
        let audio = self.encoder.forward(&mel, true)?.squeeze(0)?;
        self.decoder.reset();
        for &t in &self.hint {
            self.decoder.step(t, &audio)?;
        }

        // First step: is it speech at all, and which language (English or Vietnamese)?
        let first = self.decoder.step(self.sot, &audio)?;
        if softmax_at(&first, self.no_speech as usize) > whisper::NO_SPEECH_THRESHOLD as f32 {
            return Ok(None);
        }
        let [en, vi] = self.langs;
        let lang = if first[vi as usize] > first[en as usize] { vi } else { en };
        let mut logits = Vec::new();
        for t in [lang, self.transcribe, self.no_timestamps] {
            logits = self.decoder.step(t, &audio)?;
        }
        let mut text = Vec::new();
        // Nobody says more than ~12 tokens a second; a longer answer is Whisper looping ("Persona? Persona? …").
        let max = (pcm.len() * 12 / RATE as usize + 8).min(max_tokens);
        for _ in 0..max {
            for &s in &self.config.suppress_tokens {
                if let Some(l) = logits.get_mut(s as usize) {
                    *l = f32::NEG_INFINITY;
                }
            }
            // Special tokens all sit at or after end-of-text; only text or the end may come next.
            let next = logits[..=self.eot as usize].iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(self.eot, |(i, _)| i as u32);
            if next == self.eot {
                break;
            }
            text.push(next);
            logits = self.decoder.step(next, &audio)?;
        }
        let text = self.tok.decode(&text, true).map_err(candle_core::Error::msg)?;
        let text = text.trim();
        Ok((!text.is_empty() && !looping(text)).then(|| text.to_string()))
    }
}

/// Whisper's stuck-record failure on noise: the same sentence three or more times in a row.
fn looping(text: &str) -> bool {
    let parts: Vec<String> = text.split(['.', '?', '!']).map(clean).filter(|s| !s.is_empty()).collect();
    parts.windows(3).any(|w| w[0] == w[1] && w[1] == w[2])
}

fn softmax_at(logits: &[f32], i: usize) -> f32 {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = logits.iter().map(|l| (l - max).exp()).sum();
    (logits[i] - max).exp() / sum
}

/// Whisper's mel filterbank (librosa's Slaney mel scale and norm), n_mels × (N_FFT/2 + 1), row-major.
fn mel_filters(n_mels: usize) -> Vec<f32> {
    let bins = whisper::N_FFT / 2 + 1;
    let (f_sp, min_log_hz, min_log_mel, logstep) = (200.0 / 3.0, 1000.0, 15.0, (6.4f64).ln() / 27.0);
    let to_mel = |hz: f64| if hz >= min_log_hz { min_log_mel + (hz / min_log_hz).ln() / logstep } else { hz / f_sp };
    let to_hz = |mel: f64| if mel >= min_log_mel { min_log_hz * ((mel - min_log_mel) * logstep).exp() } else { mel * f_sp };
    let top = to_mel(RATE as f64 / 2.0);
    let pts: Vec<f64> = (0..n_mels + 2).map(|i| to_hz(top * i as f64 / (n_mels + 1) as f64)).collect();
    let mut out = vec![0f32; n_mels * bins];
    for m in 0..n_mels {
        let norm = 2.0 / (pts[m + 2] - pts[m]);
        for k in 0..bins {
            let f = k as f64 * RATE as f64 / whisper::N_FFT as f64;
            let rise = (f - pts[m]) / (pts[m + 1] - pts[m]);
            let fall = (pts[m + 2] - f) / (pts[m + 2] - pts[m + 1]);
            out[m * bins + k] = (rise.min(fall).max(0.0) * norm) as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wake(s: &str) -> Option<String> {
        after_wake(s)
    }

    #[test]
    fn hears_her_name_spelled_many_ways() {
        for s in [
            "Claudesona, open the readme",
            "claudesona open the readme",
            "Claude Sona, open the readme",
            "Claude-Sona: open the readme",
            "Claudia Sona open the readme",
            "Cloud Sona, open the readme",
            "Cloud Zona open the readme",
            "Klaus Sona, open the readme",
            "Klaus Sonna open the readme",
            "Claude's owner, open the readme",
            "Claude Sauna open the readme",
            "Claudsona open the readme",
            "CLAUDESONA!!! open the readme",
            "Hey Claudesona, open the readme",
            "Okay, so, Claude Sona open the readme",
            "Claudesona ơi, open the readme",
            // what Whisper small/base actually wrote for Windows TTS saying "Claudesona"
            "Klautsana, open the readme",
            "Hey Klotsana, open the readme",
            "Klotzana open the readme",
            "Clots on a, open the readme",
            "Claude Sanna, open the readme",
            "Clô đơ sô na ơi, open the readme",
        ] {
            assert_eq!(wake(s).as_deref(), Some("open the readme"), "{s}");
        }
    }

    #[test]
    fn bare_name_is_wake_only() {
        for s in ["Claudesona", "Claudesona.", "Claude Sona?", "Hey, Claudesona!", "Claudesona ơi", "Cloud Sona ...", "Klaatsana", "Klotzane"] {
            assert_eq!(wake(s).as_deref(), Some(""), "{s}");
        }
    }

    #[test]
    fn keeps_vietnamese_text_after_her_name() {
        assert_eq!(wake("Claudesona ơi, mở file main.rs giúp mình").as_deref(), Some("mở file main.rs giúp mình"));
    }

    #[test]
    fn ignores_speech_that_doesnt_start_with_her_name() {
        for s in [
            "",
            "Claude, add a dark mode",          // "Claude," is the prompt prefix, not her name
            "Claude Sonnet is a model",         // a real model name
            "Claude, so now what?",
            "Claude so now we refactor",
            "cloud storage is cheap",
            "close the window",
            "I asked Claudesona yesterday",     // her name, but not at the start
            "Thank you.",
            "Cảm ơn các bạn đã theo dõi",
            "hey hey hey Claudesona",           // more than two fillers: just chatter
            "Claude, Sarah sent the file",
            "Claude, Santa Ana is warm",
            "Claude Sonnet 4 is out",
            "Cloud Run is down",
        ] {
            assert_eq!(wake(s), None, "{s}");
        }
    }

    #[test]
    fn follow_up_after_bare_wake_word() {
        assert_eq!(route("Claudesona", false), Some(Heard { text: String::new(), wake_only: true }));
        assert_eq!(route(" run the tests ", true), Some(Heard { text: "run the tests".into(), wake_only: false }));
        assert_eq!(route("run the tests", false), None);
        assert_eq!(route("Claude Sona, run the tests", false), Some(Heard { text: "run the tests".into(), wake_only: false }));
    }

    #[test]
    fn resamples_any_rate_to_16k() {
        for rate in [8000, 16000, 44100, 48000] {
            let mut r = Resampler::new(rate);
            let mut out = Vec::new();
            for i in 0..rate {
                r.push((i as f32 / 10.0).sin(), &mut out);
            }
            assert_eq!(out.len(), RATE as usize, "{rate}");
        }
    }

    #[test]
    fn gate_cuts_one_utterance_with_preroll_and_ignores_clicks() {
        let quiet = |ms: usize| vec![0.001f32; ms * 16];
        let tone = |ms: usize| (0..ms * 16).map(|i| (i as f32 * 0.1).sin() * 0.2).collect::<Vec<_>>();
        let mut g = Gate::default();
        assert!(g.push(&quiet(1000)).is_empty());
        assert!(g.push(&tone(60)).is_empty()); // a click
        assert!(g.push(&quiet(1000)).is_empty());
        let mut got = g.push(&tone(1500));
        got.extend(g.push(&quiet(1000)));
        assert_eq!(got.len(), 1);
        let ms = got[0].len() / 16;
        assert!((1500 + 600..1500 + 600 + 400).contains(&ms), "{ms}ms"); // speech + hangover + pre-roll
    }

    #[test]
    fn mel_filters_look_like_whisper() {
        let f = mel_filters(80);
        assert_eq!(f.len(), 80 * 201);
        assert!(f.iter().all(|&w| w >= 0.0));
        assert!((0..80).all(|m| f[m * 201..(m + 1) * 201].iter().any(|&w| w > 0.0)), "every band covers a bin");
        // Known values from librosa.filters.mel(sr=16000, n_fft=400, n_mels=80).
        assert!((f[1] - 0.02486259).abs() < 1e-6, "{}", f[1]);
        assert!((f[40 * 201 + 43] - 0.014735566).abs() < 1e-6, "{}", f[40 * 201 + 43]); // log part of the scale
    }

    #[test]
    fn drops_stuck_records() {
        assert!(looping("Wasanite. Wasanite. Wasanite. Was"));
        assert!(looping("Mày ra rồi. Mày ra rồi. Mày ra rồi."));
        assert!(!looping("Claudesona. What time is it?"));
        assert!(!looping("No. No. Okay, do it."));
    }

    /// Live mic check: `cargo test --release stt::tests::mic -- --ignored --nocapture` (SECS=60), then talk.
    #[test]
    #[ignore]
    fn mic() {
        let dev = cpal::default_host().default_input_device().expect("no input device");
        println!("device: {:?} {:?}", dev.description().map(|d| d.name().to_string()), dev.default_input_config());
        let dir = std::env::var("WHISPER_DIR").map(PathBuf::from).unwrap_or_else(|_| folder());
        let mut stt = Stt::load(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let _s = open_mic(tx).unwrap();
        let secs: u64 = std::env::var("SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
        println!("listening {secs}s: talk now");
        let (mut gate, end, mut peak, mut tick) = (Gate::default(), Instant::now() + Duration::from_secs(secs), 0f32, Instant::now());
        while Instant::now() < end {
            let Ok(chunk) = rx.recv_timeout(Duration::from_millis(500)) else { println!("no audio arriving"); continue };
            let rms = (chunk.iter().map(|x| x * x).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
            peak = peak.max(rms);
            if tick.elapsed() > Duration::from_secs(1) {
                println!("loudest {peak:.4}  noise floor {:.4}  threshold {:.4}  talking {}", gate.noise, (gate.noise * 3.0).max(MIN_RMS), gate.talking);
                (peak, tick) = (0.0, Instant::now());
            }
            for utt in gate.push(&chunk) {
                let text = stt.transcribe(&utt);
                println!(">> {:.1}s: {text:?} wake: {:?}", utt.len() as f32 / 16000.0, text.as_deref().and_then(after_wake));
            }
        }
    }

    /// Latency on real speech: `WHISPER_DIR=<folder with the 3 model files> cargo test --release stt::tests::latency -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn latency() {
        use rodio::Source;
        let dir = std::env::var("WHISPER_DIR").map(PathBuf::from).unwrap_or_else(|_| folder());
        let t = Instant::now();
        let mut stt = Stt::load(&dir).unwrap();
        println!("load: {:?}", t.elapsed());
        let runs: usize = std::env::var("RUNS").ok().and_then(|r| r.parse().ok()).unwrap_or(1);
        let files = std::env::var("CLIPS").unwrap_or("38edf6e9b7c0ad2d 2605ed5fec742e33".into());
        for name in files.split_whitespace() {
            let path = if name.contains('.') { name.to_string() } else { format!("assets/voice/{name}.mp3") };
            let d = rodio::Decoder::try_from(std::fs::File::open(path).unwrap()).unwrap();
            let (rate, ch) = (d.sample_rate(), d.channels().get() as usize);
            let samples: Vec<f32> = d.collect();
            let mut r = Resampler::new(rate.get());
            let mut pcm = Vec::new();
            for f in samples.chunks(ch) {
                r.push(f.iter().sum::<f32>() / ch as f32, &mut pcm);
            }
            for _ in 0..runs {
                let t = Instant::now();
                let text = stt.transcribe(&pcm);
                let wake = text.as_deref().and_then(after_wake);
                let full = t.elapsed();
                let t = Instant::now();
                let named = stt.starts_with_name(&pcm);
                println!("{name}: {:.1}s audio → {text:?} (wake: {wake:?}) full {full:?}, name check {:?} ({named})", pcm.len() as f32 / 16000.0, t.elapsed());
            }
        }
    }
}
