// Chatting with her (right-click popup) and the memory graph behind it. See SPEC.md, "Chat and memory".
// Memory lives in memory.json next to the exe: facts as nodes with typed links, a running summary of
// older chat, and the last few turns. Haiku (rewrite::ask) answers and, after each turn, updates the graph.
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

const TURNS_KEPT: usize = 10; // older turns fold into the summary
const MAX_NODES: usize = 500; // least recently used facts go first, sessions before the rest
const SESSIONS_KEPT: usize = 5; // per project; older summaries fold into the project node's text
const FOLDED_MAX: usize = 800; // chars a project node's text may grow to with folded sessions
const PROMPT_MIN: usize = 20; // shorter prompts to Claude aren't worth a keeper call
const SEEDS: usize = 8; // best-matching facts per message, before adding their neighbours
const RECALL: usize = 30; // most facts sent with one message
const MODEL_URL: &str = "https://huggingface.co/minishlab/potion-base-8M/resolve/main/";
const MODEL_FILES: [&str; 3] = ["model.safetensors", "tokenizer.json", "config.json"];

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: u64,
    pub kind: String, // person, project, preference, event, thing, work, study, like, session
    pub text: String,
    pub links: Vec<(String, u64)>, // (relation, other node id)
    used: u64,                     // unix seconds it was last recalled or changed
    vec: Vec<f32>,                 // embedding; empty until the model is ready (not saved: cheap to redo)
    path: String,                  // project folder, for project nodes that sessions ran in
}

#[derive(Default)]
struct Memory {
    nodes: Vec<Node>,
    summary: String,
    turns: Vec<(String, String)>, // (you, her)
    next: u64,
    me: u64, // the user's own person node; 0 until there is one
}

fn dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| Some(p.parent()?.to_path_buf())).unwrap_or_default()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

static MEM: OnceLock<Mutex<Memory>> = OnceLock::new();

fn memory() -> MutexGuard<'static, Memory> {
    let m = MEM.get_or_init(|| Mutex::new(std::fs::read_to_string(dir().join("memory.json")).ok().and_then(|s| from_json(&s)).unwrap_or_default()));
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Every fact (without embeddings) and the user's own node id, for the graph view. Never waits:
/// None while memory is still loading or a worker holds it, so the caller keeps its last copy.
pub fn snapshot() -> Option<(Vec<Node>, u64)> {
    let m = MEM.get()?.try_lock().ok()?;
    Some((m.nodes.iter().map(|n| Node { vec: vec![], ..n.clone() }).collect(), m.me))
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
        let (used, path) = (n["used"].as_u64().unwrap_or(0), str_of(&n["path"]));
        Some(Node { id: n["id"].as_u64()?, kind: str_of(&n["kind"]), text: str_of(&n["text"]), links, used, vec: vec![], path })
    });
    let turns = v["turns"].as_array().into_iter().flatten().map(|t| (str_of(&t[0]), str_of(&t[1])));
    let me = v["me"].as_u64().unwrap_or(0); // v1 files have no "me": ensure_me finds or makes it
    Some(Memory { nodes: nodes.collect(), summary: str_of(&v["summary"]), turns: turns.collect(), next: v["next"].as_u64().unwrap_or(1), me })
}

fn to_json(m: &Memory) -> String {
    let nodes: Vec<Value> = m
        .nodes
        .iter()
        .map(|n| {
            let mut v = json!({ "id": n.id, "kind": n.kind, "text": n.text, "links": n.links, "used": n.used });
            if !n.path.is_empty() {
                v["path"] = json!(n.path);
            }
            v
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "next": m.next, "me": m.me, "summary": m.summary, "turns": m.turns, "nodes": nodes })).unwrap_or_default()
}

fn add_node(m: &mut Memory, kind: &str, text: &str) -> u64 {
    let id = m.next.max(1);
    m.next = id + 1;
    m.nodes.push(Node { id, kind: kind.into(), text: text.into(), links: vec![], used: now(), vec: vec![], path: String::new() });
    id
}

/// The user's own `person` node: the remembered one, else v1's "User is named…" fact, else a new one.
fn ensure_me(m: &mut Memory) -> u64 {
    if m.me != 0 && m.nodes.iter().any(|n| n.id == m.me) {
        return m.me;
    }
    let found = m.nodes.iter().find(|n| n.kind == "person" && n.text.to_lowercase().trim_start_matches("the ").starts_with("user")).map(|n| n.id);
    m.me = found.unwrap_or_else(|| add_node(m, "person", "The user"));
    m.me
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

const CHAT_EN: &str = "You are Claudesona, a bubbly chibi anime girl desktop mascot who keeps the user company while they code with Claude Code. Chat with them in her voice: cheerful, energetic, cute and caring. Use a ~ now and then, end most messages with a kaomoji like (^▽^), (｀・ω・´), (≧▽≦), (・ω・) or (◕‿◕), and sometimes stretch a word (sooo, yaaay). No Japanese words. She calls herself \"your sidekick\", never \"I\" or \"Claude\". Use what you remember about the user and what Claude Code is doing when it helps, and never invent memories. You can't run tools or change code in this chat, but you hand work to Claude Code for them: if they ask for real work here, tell them to say it again starting with \"Claude,\" (like \"Claude, fix the login bug\") and you'll pass it on. Answer in the user's language. Plain text only, no markdown. Output exactly: a short reply of at most 20 words, then a line containing only ---, then a longer reply of at most 60 words.";

const CHAT_VI: &str = "You are Claudesona, a bubbly chibi anime girl desktop mascot who keeps the user company while they code with Claude Code. Chat with them in Vietnamese, in her voice: cheerful, energetic, cute and caring. She calls herself \"tớ\" or \"Phụ tá\", never \"tôi\", \"mình\" or \"Claude\", and calls the user \"cậu\". Use a ~ now and then, end most messages with a kaomoji like (^▽^), (｀・ω・´), (≧▽≦), (・ω・) or (◕‿◕), and sometimes stretch a word (nhaaa, điii). No Japanese words. Use what you remember about the user and what Claude Code is doing when it helps, and never invent memories. You can't run tools or change code in this chat, but you hand work to Claude Code for them: if they ask for real work here, tell them to say it again starting with \"Claude,\" (like \"Claude, fix the login bug\") and you'll pass it on. Plain text only, no markdown. Output exactly: a short reply of at most 20 words, then a line containing only ---, then a longer reply of at most 60 words.";

const KEEPER: &str = "You maintain the long-term memory of a desktop mascot about her user, as one connected graph of short facts. You get the user's own node, the current facts (with [id]s), the running summary, any old chat turns to fold into the summary, and the newest message. Keep only durable things about the user and their world: name, projects, job, studies, favourites, people, pets, plans, events. Kinds: person (the user and people they know), project (things they build), work (job, role, team, workplace), study (courses, subjects, exams, schedule), like (personal favourites and dislikes: foods, games, music, shows, hobbies), preference (how they like things done), event (dated happenings, plans), thing (anything else). Skip small talk and anything about the mascot herself. Never duplicate a fact: update it instead. Forget facts the user corrects or asks you to forget. Every new fact must be linked to at least one existing fact it relates to (the user's own node if nothing closer), so the graph stays connected: never leave a fact on its own. Write facts in the language the user used. Reply with JSON only, no prose: {\"summary\": \"the running summary of the chat, at most 80 words, updated with the old turns; empty if there is no chat\", \"add\": [{\"key\": \"n1\", \"kind\": \"person|project|work|study|like|preference|event|thing\", \"text\": \"one short fact\"}], \"update\": [{\"id\": 12, \"text\": \"new text\"}], \"link\": [{\"from\": 12, \"rel\": \"works_on\", \"to\": \"n1\"}], \"forget\": [7]}. In links, use an existing [id] number or a new fact's key. Use [] for lists with nothing to do.";

const SUMMARY: &str = "You summarize a finished Claude Code session for a desktop mascot's memory. You get the user's requests and Claude's final replies. Write at most 40 words in three short parts: what the user asked, what changed, and what is left to do (or that nothing is). Plain text, no markdown, in the language the user used.";

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
    keep(|m| {
        m.turns.push((msg.to_string(), said.to_string()));
        let old: Vec<String> = m.turns.iter().take(m.turns.len().saturating_sub(TURNS_KEPT)).map(|(u, h)| format!("User: {u}\nMascot: {h}")).collect();
        let facts = keeper_facts(m, &format!("{msg} {said}"), qv);
        format!(
            "{facts}\n\nRunning summary:\n{}\n\nOld turns to fold into the summary:\n{}\n\nNewest turn:\nUser: {msg}\nMascot: {said}",
            or_none(&m.summary),
            or_none(&old.join("\n"))
        )
    });
}

/// Learns about the user from a prompt they sent Claude (never Claude's reply), on a worker thread.
pub fn learn_from_prompt(text: &str) {
    let text: String = text.trim().chars().take(2000).collect();
    if text.chars().count() < PROMPT_MIN {
        return;
    }
    std::thread::spawn(move || {
        let qv = model().map(|md| md.encode_single(&text)).unwrap_or_default();
        keep(|m| {
            let facts = keeper_facts(m, &text, &qv);
            format!("{facts}\n\nThe user just sent this to their coding assistant. There is no chat: leave the summary empty, and learn only durable facts about the user:\n{text}")
        });
    });
}

/// The facts the keeper sees for `query`, always including the user's own node.
fn keeper_facts(m: &mut Memory, query: &str, qv: &[f32]) -> String {
    let me = ensure_me(m);
    let mut picked = recall(m, query, qv);
    if let Some(i) = m.nodes.iter().position(|n| n.id == me).filter(|i| !picked.contains(i)) {
        picked.insert(0, i);
    }
    format!("The user's own node: [{me}]\n\nCurrent facts:\n{}", describe(m, &picked))
}

/// Builds the keeper's input from memory (`build` may change it, e.g. add the turn), asks Haiku, applies the change.
fn keep(build: impl FnOnce(&mut Memory) -> String) {
    // One at a time: two quick updates would each start from the same old memory, and the later save would undo the first.
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let input = {
        let mut m = memory();
        let text = build(&mut m);
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
    let mut added = vec![];
    for a in c["add"].as_array().into_iter().flatten() {
        let Some(text) = a["text"].as_str().map(str::trim).filter(|s| !s.is_empty()) else { continue };
        let id = add_node(m, a["kind"].as_str().unwrap_or("thing"), text);
        added.push(id);
        if let Some(k) = a["key"].as_str() {
            keys.insert(k.to_string(), id);
        }
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
    // The graph stays connected: a new fact Haiku left on its own hangs off the user.
    for id in added {
        if !m.nodes.iter().any(|n| (n.id == id && !n.links.is_empty()) || n.links.iter().any(|l| l.1 == id)) {
            let me = ensure_me(m);
            if let Some(n) = m.nodes.iter_mut().find(|n| n.id == id && id != me) {
                n.links.push(("about".into(), me));
            }
        }
    }
    let gone: Vec<u64> = c["forget"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    m.nodes.retain(|n| !gone.contains(&n.id));
    if m.nodes.len() > MAX_NODES {
        // Kept first: the user, then other facts, then sessions; most recently used first within each.
        let me = m.me;
        m.nodes.sort_by_key(|n| (n.id != me, n.kind == "session", std::cmp::Reverse(n.used)));
        m.nodes.truncate(MAX_NODES);
    }
    let ids: Vec<u64> = m.nodes.iter().map(|n| n.id).collect();
    for n in &mut m.nodes {
        n.links.retain(|l| ids.contains(&l.1));
    }
}

/// Has Haiku summarize a finished session on a worker thread, and stores it as a `session` node linked to
/// the project. `turns` are (user prompt, Claude's final text). `on_done` gets the summary, or None if
/// there was nothing to summarize or `claude` failed.
pub fn summarize_session(project_path: &str, turns: &[(String, String)], on_done: impl FnOnce(Option<String>) + Send + 'static) {
    let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let mut lines: Vec<String> = turns.iter().map(|(u, a)| format!("User: {}\nClaude: {}", cut(u, 600), cut(a, 900))).collect();
    // shortcut: keeps the newest ~12k chars; a long session's start is left out of its summary.
    while lines.len() > 1 && lines.iter().map(String::len).sum::<usize>() > 12_000 {
        lines.remove(0);
    }
    let path = project_path.to_string();
    std::thread::spawn(move || {
        let ask = || crate::rewrite::ask(SUMMARY, &lines.join("\n\n"));
        let summary = if lines.is_empty() { None } else { ask() }.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(s) = &summary {
            let mut m = memory();
            add_session(&mut m, &path, s);
            save(to_json(&m));
        }
        on_done(summary);
    });
}

fn folder_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A folder path in one spelling: `C:/x`, `C:\x\` and (on Windows and macOS, whose disks ignore case) `c:\X` match.
fn same_folder(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = p.replace('\\', "/");
        let p = p.trim_end_matches('/');
        if cfg!(any(windows, target_os = "macos")) { p.to_lowercase() } else { p.to_string() }
    };
    !a.is_empty() && norm(a) == norm(b)
}

/// The project node for a folder: the one sessions ran in there, else a project fact naming the folder.
fn find_project(m: &Memory, path: &str) -> Option<usize> {
    let name = folder_name(path).to_lowercase();
    let by_name = |n: &Node| n.kind == "project" && n.path.is_empty() && !name.is_empty() && n.text.to_lowercase().contains(&name);
    m.nodes.iter().position(|n| n.kind == "project" && same_folder(&n.path, path)).or_else(|| m.nodes.iter().position(by_name))
}

/// Stores `summary` as the project's newest session; sessions past SESSIONS_KEPT fold into the project's text.
fn add_session(m: &mut Memory, path: &str, summary: &str) {
    let project = match find_project(m, path) {
        Some(i) => m.nodes[i].id,
        None => {
            let id = add_node(m, "project", folder_name(path));
            let me = ensure_me(m);
            if let Some(n) = m.nodes.iter_mut().find(|n| n.id == me) {
                n.links.push(("works_on".into(), id));
            }
            id
        }
    };
    add_node(m, "session", summary);
    m.nodes.last_mut().expect("just added").links.push(("session_of".into(), project));
    let mut sessions: Vec<&Node> = m.nodes.iter().filter(|n| n.kind == "session" && n.links.iter().any(|l| l.1 == project)).collect();
    sessions.sort_by_key(|n| n.id); // ids only grow: oldest first
    let old = &sessions[..sessions.len().saturating_sub(SESSIONS_KEPT)];
    let (old_ids, folded): (Vec<u64>, Vec<String>) = old.iter().map(|n| (n.id, n.text.clone())).unzip();
    let p = m.nodes.iter_mut().find(|n| n.id == project).expect("project exists");
    (p.path, p.used) = (path.to_string(), now());
    if !folded.is_empty() {
        const MARK: &str = "\nEarlier: ";
        let (head, mut earlier) = match p.text.split_once(MARK) {
            Some((h, e)) => (h.to_string(), format!("{e} / ")),
            None => (p.text.clone(), String::new()),
        };
        earlier += &folded.join(" / ");
        // Too long: drop the oldest folded history, keeping the project's own text.
        let over = (head.chars().count() + MARK.len() + earlier.chars().count()).saturating_sub(FOLDED_MAX);
        let earlier: String = earlier.chars().skip(over).collect();
        (p.text, p.vec) = (format!("{head}{MARK}{earlier}"), vec![]);
    }
    apply(m, &json!({ "forget": old_ids }));
}

/// The newest session summary for the project at `project_path`, for her greeting.
pub fn last_session(project_path: &str) -> Option<String> {
    let m = memory();
    let project = m.nodes[find_project(&m, project_path)?].id;
    let sessions = m.nodes.iter().filter(|n| n.kind == "session" && n.links.iter().any(|l| l.1 == project));
    sessions.max_by_key(|n| n.id).map(|n| n.text.clone())
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
        Node { id, kind: "thing".into(), text: text.into(), links, used: 0, vec: vec![], path: String::new() }
    }

    #[test]
    fn loads_v1_memory_and_finds_the_user() {
        let v1 = r#"{"next": 3, "summary": "s", "turns": [["hi", "hey"]], "nodes": [
            {"id": 1, "kind": "like", "text": "Loves pho", "links": [["about", 2]], "used": 5},
            {"id": 2, "kind": "person", "text": "User is named Tri", "links": [], "used": 9}]}"#;
        let mut m = from_json(v1).unwrap();
        assert_eq!((m.nodes.len(), m.me, m.nodes[0].kind.as_str()), (2, 0, "like"));
        assert_eq!(ensure_me(&mut m), 2); // v1's "User is named…" fact becomes her user node
        assert_eq!(from_json(&to_json(&m)).unwrap().me, 2);
    }

    #[test]
    fn new_facts_without_links_hang_off_the_user() {
        let mut m = Memory { next: 1, ..Default::default() };
        apply(&mut m, &json!({ "add": [{ "key": "n1", "kind": "like", "text": "Loves phở" }, { "key": "n2", "kind": "study", "text": "Calculus II" }, { "key": "n3", "kind": "event", "text": "Exam on Nov 20" }],
            "link": [{ "from": "n3", "rel": "exam_of", "to": "n2" }] }));
        let me = m.me;
        assert_eq!(m.nodes.iter().find(|n| n.id == me).unwrap().kind, "person"); // made, as there was none
        let links = |t: &str| m.nodes.iter().find(|n| n.text == t).unwrap().links.clone();
        assert_eq!(links("Loves phở"), vec![("about".into(), me)]);
        assert_eq!(links("Calculus II"), vec![]); // linked to by the exam already
        assert_eq!(links("Exam on Nov 20"), vec![("exam_of".into(), 2)]);
    }

    #[test]
    fn evicts_sessions_first_then_least_recently_used() {
        let mut m = Memory { next: 1000, me: 1, ..Default::default() };
        m.nodes.push(Node { kind: "person".into(), ..node(1, "The user", vec![]) });
        for i in 2..MAX_NODES as u64 - 1 {
            // 497 facts + the user + 2 sessions: full
            m.nodes.push(Node { used: i, ..node(i, "fact", vec![]) });
        }
        m.nodes.push(Node { kind: "session".into(), used: 10_000, ..node(900, "fresh session", vec![]) });
        m.nodes.push(Node { kind: "session".into(), used: 9_000, ..node(901, "old session", vec![]) });
        apply(&mut m, &json!({ "add": [{ "kind": "thing", "text": "new fact" }] }));
        assert_eq!(m.nodes.len(), MAX_NODES);
        let has = |id: u64| m.nodes.iter().any(|n| n.id == id);
        assert!(has(1) && has(2) && has(900) && !has(901)); // both sessions would go before facts; one had to
        apply(&mut m, &json!({ "add": [{ "kind": "thing", "text": "another" }] }));
        assert!(!has_id(&m, 900) && has_id(&m, 2));
        apply(&mut m, &json!({ "add": [{ "kind": "thing", "text": "a third" }] }));
        assert!(!has_id(&m, 2) && has_id(&m, 3) && has_id(&m, 1)); // then the least recently used fact; never the user
    }

    fn has_id(m: &Memory, id: u64) -> bool {
        m.nodes.iter().any(|n| n.id == id)
    }

    #[test]
    fn one_folder_however_it_is_spelled() {
        assert!(same_folder("C:/x", "C:\\x\\") && same_folder("/home/tri/blog/", "/home/tri/blog"));
        assert!(!same_folder("C:\\x", "C:\\xy") && !same_folder("", ""));
        assert_eq!(same_folder("C:\\Code\\Blog", "c:/code/blog"), cfg!(any(windows, target_os = "macos")));
        let mut m = Memory { next: 1, ..Default::default() };
        add_session(&mut m, "C:/code/blog", "one");
        add_session(&mut m, "C:\\code\\blog\\", "two");
        assert_eq!(m.nodes.iter().filter(|n| n.kind == "project").count(), 1);
    }

    #[test]
    fn sessions_link_to_their_project_and_old_ones_fold() {
        let mut m = Memory { next: 1, ..Default::default() };
        for i in 1..=7 {
            add_session(&mut m, "C:\\code\\blog\\", &format!("session {i}"));
        }
        let p = m.nodes.iter().find(|n| n.kind == "project").unwrap();
        assert_eq!((p.text.as_str(), p.path.as_str()), ("blog\nEarlier: session 1 / session 2", "C:\\code\\blog\\"));
        let sessions: Vec<&str> = m.nodes.iter().filter(|n| n.kind == "session").map(|n| n.text.as_str()).collect();
        assert_eq!(sessions, ["session 3", "session 4", "session 5", "session 6", "session 7"]);
        assert!(m.nodes.iter().filter(|n| n.kind == "session").all(|n| n.links == vec![("session_of".into(), p.id)]));
        assert!(m.nodes.iter().find(|n| n.id == m.me).unwrap().links.contains(&("works_on".into(), p.id)));
        // A project fact the keeper wrote is found by folder name; the greeting gets the newest session.
        let mut m2 = Memory { next: 1, ..Default::default() };
        let id = add_node(&mut m2, "project", "Building the Blog engine");
        add_session(&mut m2, "/home/tri/blog", "fixed login bug");
        assert_eq!(m2.nodes.iter().filter(|n| n.kind == "project").count(), 1);
        assert_eq!(m2.nodes.last().unwrap().links, vec![("session_of".into(), id)]);
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
