// Chatting with her (right-click popup) and the memory graph behind it. See SPEC.md, "Chat and memory".
// Memory lives in memory.json next to the exe: facts as nodes with typed links, a running summary of
// older chat, and the last few turns. Haiku (rewrite::ask) answers and, after each turn, updates the graph.
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

const TURNS_KEPT: usize = 10; // older turns fold into the summary
const MAX_NODES: usize = 200; // least recently used facts go first
const SEEDS: usize = 8; // best-matching facts per message, before adding their neighbours
const RECALL: usize = 30; // most facts sent with one message
const MODEL_URL: &str = "https://huggingface.co/minishlab/potion-base-8M/resolve/main/";
const MODEL_FILES: [&str; 3] = ["model.safetensors", "tokenizer.json", "config.json"];

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: u64,
    pub kind: String, // person, project, preference, event, thing
    pub text: String,
    pub links: Vec<(String, u64)>, // (relation, other node id)
    used: u64,                     // unix seconds it was last recalled or changed
    vec: Vec<f32>,                 // embedding; empty until the model is ready (not saved: cheap to redo)
}

#[derive(Default)]
struct Memory {
    nodes: Vec<Node>,
    summary: String,
    turns: Vec<(String, String)>, // (you, her)
    next: u64,
}

fn dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| Some(p.parent()?.to_path_buf())).unwrap_or_default()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn memory() -> MutexGuard<'static, Memory> {
    static MEM: OnceLock<Mutex<Memory>> = OnceLock::new();
    let m = MEM.get_or_init(|| Mutex::new(std::fs::read_to_string(dir().join("memory.json")).ok().and_then(|s| from_json(&s)).unwrap_or_default()));
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Loads memory.json (and the embedding model, if downloaded) off the UI thread at startup.
pub fn preload() {
    let _ = model();
    drop(memory());
}

fn from_json(s: &str) -> Option<Memory> {
    let v: Value = serde_json::from_str(s).ok()?;
    let str_of = |v: &Value| v.as_str().unwrap_or("").to_string();
    let nodes = v["nodes"].as_array()?.iter().filter_map(|n| {
        let links = n["links"].as_array().into_iter().flatten().filter_map(|l| Some((str_of(&l[0]), l[1].as_u64()?))).collect();
        Some(Node { id: n["id"].as_u64()?, kind: str_of(&n["kind"]), text: str_of(&n["text"]), links, used: n["used"].as_u64().unwrap_or(0), vec: vec![] })
    });
    let turns = v["turns"].as_array().into_iter().flatten().map(|t| (str_of(&t[0]), str_of(&t[1])));
    Some(Memory { nodes: nodes.collect(), summary: str_of(&v["summary"]), turns: turns.collect(), next: v["next"].as_u64().unwrap_or(1) })
}

fn to_json(m: &Memory) -> String {
    let nodes: Vec<Value> = m.nodes.iter().map(|n| json!({ "id": n.id, "kind": n.kind, "text": n.text, "links": n.links, "used": n.used })).collect();
    serde_json::to_string_pretty(&json!({ "next": m.next, "summary": m.summary, "turns": m.turns, "nodes": nodes })).unwrap_or_default()
}

fn save(text: String) {
    let path = dir().join("memory.json");
    let tmp = path.with_extension("json.tmp");
    // Write then rename, so a crash mid-write never leaves a half file.
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// The embedding model, once downloaded. The first call starts the ~30MB download in the background;
/// until it lands, recall falls back to shared words.
fn model() -> Option<&'static model2vec_rs::model::StaticModel> {
    static MODEL: OnceLock<model2vec_rs::model::StaticModel> = OnceLock::new();
    static FETCHING: AtomicBool = AtomicBool::new(false);
    if let Some(m) = MODEL.get() {
        return Some(m);
    }
    let folder = dir().join("model");
    let read = |f: &str| std::fs::read(folder.join(f)).ok();
    if let (Some(tok), Some(weights), Some(config)) = (read(MODEL_FILES[1]), read(MODEL_FILES[0]), read(MODEL_FILES[2])) {
        if let Ok(m) = model2vec_rs::model::StaticModel::from_bytes(tok, weights, config, None) {
            let _ = MODEL.set(m);
            return MODEL.get();
        }
    }
    if !FETCHING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&folder);
            for f in MODEL_FILES {
                let (part, done) = (folder.join(format!("{f}.part")), folder.join(f));
                // shortcut: curl ships with Windows 10+, macOS and most Linux distros; add an HTTP crate if one lacks it.
                let mut cmd = std::process::Command::new("curl");
                cmd.args(["-fsSL", "--retry", "2", "-o"]).arg(&part).arg(format!("{MODEL_URL}{f}"));
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
                }
                if done.exists() || cmd.status().is_ok_and(|s| s.success()) && std::fs::rename(&part, &done).is_ok() {
                    continue;
                }
                let _ = std::fs::remove_file(&part);
                FETCHING.store(false, Ordering::Relaxed); // try again on the next chat
                return;
            }
        });
    }
    None
}

fn words(s: &str) -> Vec<String> {
    const STOP: &[&str] = &["the", "and", "you", "your", "for", "are", "what", "with", "that", "this", "have", "how", "was", "can", "about", "user", "của", "và", "cho", "không", "là"];
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3 && !STOP.contains(w)).map(String::from).collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    dot / (norm(a) * norm(b))
}

/// Indexes of the facts worth sending with `query`: the best matches, then the facts they link to (either way).
fn recall(m: &mut Memory, query: &str, query_vec: &[f32]) -> Vec<usize> {
    let q = words(query);
    let mut scored: Vec<(f32, usize)> = m
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let shared = words(&n.text).iter().filter(|w| q.contains(w)).count() as f32;
            (cosine(query_vec, &n.vec) + 0.4 * shared, i)
        })
        .filter(|(s, _)| *s > 0.35)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut picked: Vec<usize> = scored.iter().take(SEEDS).map(|s| s.1).collect();
    for i in picked.clone() {
        let id = m.nodes[i].id;
        for (j, n) in m.nodes.iter().enumerate() {
            let linked = m.nodes[i].links.iter().any(|l| l.1 == n.id) || n.links.iter().any(|l| l.1 == id);
            if linked && !picked.contains(&j) && picked.len() < RECALL {
                picked.push(j);
            }
        }
    }
    let t = now();
    for &i in &picked {
        m.nodes[i].used = t;
    }
    picked
}

/// Facts as lines Haiku can read and refer back to: `[id] (kind) text; rel -> [id]`.
fn describe(m: &Memory, picked: &[usize]) -> String {
    picked
        .iter()
        .map(|&i| {
            let n = &m.nodes[i];
            let links: Vec<String> = n.links.iter().map(|(r, to)| format!("{r} -> [{to}]")).collect();
            format!("[{}] ({}) {}{}", n.id, n.kind, n.text, if links.is_empty() { String::new() } else { format!("; {}", links.join(", ")) })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn embed_missing(m: &mut Memory) {
    if let Some(model) = model() {
        for n in m.nodes.iter_mut().filter(|n| n.vec.is_empty()) {
            n.vec = model.encode_single(&n.text);
        }
    }
}

const CHAT_EN: &str = "You are Claudesona, a bubbly chibi anime girl desktop mascot who keeps the user company while they code with Claude Code. Chat with them in her voice: cheerful, energetic, cute and caring. Use a ~ now and then, end most messages with a kaomoji like (^▽^), (｀・ω・´), (≧▽≦), (・ω・) or (◕‿◕), and sometimes stretch a word (sooo, yaaay). No Japanese words. She calls herself \"your sidekick\", never \"I\" or \"Claude\". Use what you remember about the user and what Claude Code is doing when it helps, and never invent memories. You can't run tools or change code: for real work, cheer them on to ask Claude in their terminal. Answer in the user's language. Plain text only, no markdown. Output exactly: a short reply of at most 20 words, then a line containing only ---, then a longer reply of at most 60 words.";

const CHAT_VI: &str = "You are Claudesona, a bubbly chibi anime girl desktop mascot who keeps the user company while they code with Claude Code. Chat with them in Vietnamese, in her voice: cheerful, energetic, cute and caring. She calls herself \"tớ\" or \"Phụ tá\", never \"tôi\", \"mình\" or \"Claude\", and calls the user \"cậu\". Use a ~ now and then, end most messages with a kaomoji like (^▽^), (｀・ω・´), (≧▽≦), (・ω・) or (◕‿◕), and sometimes stretch a word (nhaaa, điii). No Japanese words. Use what you remember about the user and what Claude Code is doing when it helps, and never invent memories. You can't run tools or change code: for real work, cheer them on to ask Claude in their terminal. Plain text only, no markdown. Output exactly: a short reply of at most 20 words, then a line containing only ---, then a longer reply of at most 60 words.";

const KEEPER: &str = "You maintain the long-term memory of a desktop mascot about her user, as a graph of short facts. You get the current facts (with [id]s), the running summary, any old chat turns to fold into the summary, and the newest turn. Keep only durable things about the user and their world: name, likes and dislikes, projects, people, pets, plans, events. Skip small talk and anything about the mascot herself. Never duplicate a fact: update it instead. Forget facts the user corrects or asks you to forget. Write facts in the language the user used. Reply with JSON only, no prose: {\"summary\": \"the running summary of the chat, at most 80 words, updated with the old turns\", \"add\": [{\"key\": \"n1\", \"kind\": \"person|project|preference|event|thing\", \"text\": \"one short fact\"}], \"update\": [{\"id\": 12, \"text\": \"new text\"}], \"link\": [{\"from\": 12, \"rel\": \"works_on\", \"to\": \"n1\"}], \"forget\": [7]}. In links, use an existing [id] number or a new fact's key. Use [] for lists with nothing to do.";

pub fn persona() -> &'static str {
    match crate::i18n::lang() {
        crate::i18n::Lang::En => CHAT_EN,
        crate::i18n::Lang::Vi => CHAT_VI,
    }
}

/// Her answer to `msg`, as (short, long). Blocks for a second or more: call from a worker thread.
/// `work` says what Claude Code is doing right now; `partial` gets the short answer as it streams in.
/// None if `claude` failed.
pub fn reply(msg: &str, work: &str, partial: impl FnMut(&str)) -> Option<(String, Option<String>)> {
    let qv = model().map(|md| md.encode_single(msg)).unwrap_or_default();
    let input = {
        let mut m = memory();
        embed_missing(&mut m);
        let picked = recall(&mut m, msg, &qv);
        let recent: Vec<String> = m.turns.iter().map(|(u, h)| format!("User: {u}\nYou: {h}")).collect();
        format!(
            "What you remember about the user:\n{}\n\nEarlier chat, summarized:\n{}\n\nRecent chat:\n{}\n\nWhat Claude Code is doing right now:\n{}\n\nThe user says:\n{msg}",
            or_none(&describe(&m, &picked)),
            or_none(&m.summary),
            or_none(&recent.join("\n")),
            or_none(work)
        )
    };
    let (short, long) = crate::rewrite::split(&crate::rewrite::ask_stream(persona(), &input, crate::rewrite::short_part(partial))?)?;
    let said = long.clone().unwrap_or_else(|| short.clone());
    let (msg, qv) = (msg.to_string(), qv);
    std::thread::spawn(move || remember(&msg, &said, &qv));
    Some((short, long))
}

fn or_none(s: &str) -> &str {
    if s.trim().is_empty() { "(nothing yet)" } else { s }
}

/// Adds the turn, then asks Haiku to fold old turns into the summary and update the facts.
fn remember(msg: &str, said: &str, qv: &[f32]) {
    let input = {
        let mut m = memory();
        m.turns.push((msg.to_string(), said.to_string()));
        let old: Vec<String> = m.turns.iter().take(m.turns.len().saturating_sub(TURNS_KEPT)).map(|(u, h)| format!("User: {u}\nMascot: {h}")).collect();
        let picked = recall(&mut m, &format!("{msg} {said}"), qv);
        let text = format!(
            "Current facts:\n{}\n\nRunning summary:\n{}\n\nOld turns to fold into the summary:\n{}\n\nNewest turn:\nUser: {msg}\nMascot: {said}",
            or_none(&describe(&m, &picked)),
            or_none(&m.summary),
            or_none(&old.join("\n"))
        );
        save(to_json(&m)); // the turn is kept even if Haiku fails below
        text
    };
    let Some(out) = crate::rewrite::ask(KEEPER, &input) else { return };
    let Some(change) = out.find('{').zip(out.rfind('}')).and_then(|(a, b)| serde_json::from_str::<Value>(out.get(a..=b)?).ok()) else { return };
    let mut m = memory();
    apply(&mut m, &change);
    embed_missing(&mut m);
    save(to_json(&m));
}

/// Applies Haiku's JSON change to the graph, then trims it to MAX_NODES.
fn apply(m: &mut Memory, c: &Value) {
    let t = now();
    if let Some(s) = c["summary"].as_str().filter(|s| !s.trim().is_empty()) {
        m.summary = s.trim().to_string();
        let over = m.turns.len().saturating_sub(TURNS_KEPT);
        m.turns.drain(..over); // folded into the summary just now
    }
    let mut keys = std::collections::HashMap::new();
    for a in c["add"].as_array().into_iter().flatten() {
        let Some(text) = a["text"].as_str().map(str::trim).filter(|s| !s.is_empty()) else { continue };
        let id = m.next.max(1);
        m.next = id + 1;
        if let Some(k) = a["key"].as_str() {
            keys.insert(k.to_string(), id);
        }
        m.nodes.push(Node { id, kind: a["kind"].as_str().unwrap_or("thing").to_string(), text: text.to_string(), links: vec![], used: t, vec: vec![] });
    }
    for u in c["update"].as_array().into_iter().flatten() {
        if let (Some(id), Some(text)) = (u["id"].as_u64(), u["text"].as_str())
            && let Some(n) = m.nodes.iter_mut().find(|n| n.id == id)
        {
            (n.text, n.used, n.vec) = (text.trim().to_string(), t, vec![]);
        }
    }
    // A link end is an existing id (number or "12") or a new fact's key.
    let resolve = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| keys.get(s).copied().or_else(|| s.trim_matches(['[', ']']).parse().ok())));
    for l in c["link"].as_array().into_iter().flatten() {
        let (Some(from), Some(to), Some(rel)) = (resolve(&l["from"]), resolve(&l["to"]), l["rel"].as_str()) else { continue };
        if from != to
            && m.nodes.iter().any(|n| n.id == to)
            && let Some(n) = m.nodes.iter_mut().find(|n| n.id == from)
            && !n.links.iter().any(|x| x.1 == to && x.0 == rel)
        {
            n.links.push((rel.to_string(), to));
        }
    }
    let gone: Vec<u64> = c["forget"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    m.nodes.retain(|n| !gone.contains(&n.id));
    if m.nodes.len() > MAX_NODES {
        m.nodes.sort_by_key(|n| std::cmp::Reverse(n.used));
        m.nodes.truncate(MAX_NODES);
    }
    let ids: Vec<u64> = m.nodes.iter().map(|n| n.id).collect();
    for n in &mut m.nodes {
        n.links.retain(|l| ids.contains(&l.1));
    }
}

/// Every fact, newest first, with its links spelled out, for the memory viewer.
pub fn facts() -> Vec<(u64, String, Vec<String>)> {
    let m = memory();
    let mut out: Vec<_> = m
        .nodes
        .iter()
        .map(|n| {
            let links = n.links.iter().filter_map(|(r, to)| Some(format!("{r} → {}", m.nodes.iter().find(|x| x.id == *to)?.text))).collect();
            (n.id, format!("{} · {}", n.kind, n.text), links)
        })
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.0));
    out
}

pub fn forget(id: u64) {
    let mut m = memory();
    apply(&mut m, &json!({ "forget": [id] }));
    save(to_json(&m));
}

pub fn forget_all() {
    let mut m = memory();
    *m = Memory::default();
    save(to_json(&m));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, text: &str, links: Vec<(String, u64)>) -> Node {
        Node { id, kind: "thing".into(), text: text.into(), links, used: 0, vec: vec![] }
    }

    #[test]
    fn applies_haiku_changes_and_keeps_links_valid() {
        let mut m = Memory { next: 3, nodes: vec![node(1, "User is named Tri", vec![]), node(2, "User likes tea", vec![])], ..Default::default() };
        m.turns = (0..12).map(|i| (format!("u{i}"), format!("h{i}"))).collect();
        let change = json!({
            "summary": "They chatted about tea.",
            "add": [{ "key": "n1", "kind": "project", "text": "Building Claudesona" }],
            "update": [{ "id": 2, "text": "User loves green tea" }],
            "link": [{ "from": 1, "rel": "works_on", "to": "n1" }, { "from": "[1]", "rel": "likes", "to": 2 }, { "from": 1, "rel": "x", "to": 99 }],
            "forget": []
        });
        apply(&mut m, &change);
        assert_eq!(m.nodes.len(), 3);
        assert_eq!(m.nodes[1].text, "User loves green tea");
        assert_eq!(m.nodes[0].links, vec![("works_on".into(), 3), ("likes".into(), 2)]);
        assert_eq!((m.turns.len(), m.summary.as_str()), (TURNS_KEPT, "They chatted about tea."));
        apply(&mut m, &json!({ "forget": [3] }));
        assert_eq!(m.nodes[0].links, vec![("likes".into(), 2)]); // the dangling link went with the fact
        // The round trip through memory.json keeps everything but embeddings.
        let back = from_json(&to_json(&m)).unwrap();
        assert_eq!((back.nodes.len(), back.next, back.turns.len()), (2, 4, TURNS_KEPT));
        assert_eq!(back.nodes[0].links, m.nodes[0].links);
    }

    #[test]
    fn recall_follows_links_from_word_matches() {
        let mut m = Memory {
            nodes: vec![node(1, "Mochi is the user's cat", vec![("owned_by".into(), 2)]), node(2, "User is named Tri", vec![]), node(3, "User likes jazz", vec![])],
            ..Default::default()
        };
        let picked = recall(&mut m, "how is mochi doing?", &[]);
        assert_eq!(picked, vec![0, 1]); // Mochi by word, Tri through the link; jazz left out
    }
}
