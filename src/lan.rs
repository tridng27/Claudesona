// Friends on the local network: Keqings find each other with a UDP broadcast, pair with a 4-digit code,
// then can poke each other and send short messages. Off until you switch it on in her menu. Only paired
// friends' pokes and messages get through; nothing about your sessions or code is ever sent.
use serde_json::{Value, json};

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const PORT: u16 = 47828;
const HELLO_EVERY: Duration = Duration::from_secs(2);
const GONE_AFTER: Duration = Duration::from_secs(8); // a friend not heard from for this long leaves the list
const MAX_TEXT: usize = 200;

/// Something a friend did, for her to show.
#[derive(Debug, PartialEq)]
pub enum Event {
    Poke(String),            // friend's name
    Message(String, String), // friend's name, text
    Paired(String),          // friend's name
}

/// A Keqing seen on the network.
#[derive(Clone, Debug)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub status: String,
    pub paired: bool,
    addr: SocketAddr,
    seen: Instant,
}

/// Who you are on the network, and who you trust.
pub struct Me {
    pub id: String,
    pub name: String,
    pub code: String,        // the 4 digits a friend types to pair with you
    pub status: String,      // working · idle · needs you…
    pub paired: Vec<String>, // friends' ids
    pending: Vec<String>,    // ids you sent your pairing request to
}

static ON: AtomicBool = AtomicBool::new(false);
static ME: Mutex<Option<Me>> = Mutex::new(None);
static PEERS: Mutex<Vec<Peer>> = Mutex::new(Vec::new());
static EXTRA: Mutex<String> = Mutex::new(String::new()); // friends' IPs on other subnets, as typed: broadcasts don't cross routers
static OUTBOX: OnceLock<Mutex<Sender<(Option<String>, String)>>> = OnceLock::new(); // (to peer id, or None = broadcast; packet)

/// Starts the network thread (once). `emit` gets friends' pokes, messages and pairings.
pub fn start(id: String, name: String, code: String, paired: Vec<String>, on: bool, emit: impl Fn(Event) + Send + 'static) {
    *ME.lock().unwrap() = Some(Me { id, name, code, status: String::new(), paired, pending: vec![] });
    ON.store(on, Ordering::Relaxed);
    let (tx, rx) = channel();
    if OUTBOX.set(Mutex::new(tx)).is_ok() {
        std::thread::spawn(move || run(rx, emit));
    }
}

pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
    if !on {
        PEERS.lock().unwrap().clear();
    }
}

pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Friends' IPs (comma or space separated) to say hello to directly, for friends behind another router.
pub fn set_extra(ips: &str) {
    *EXTRA.lock().unwrap() = ips.to_string();
}

pub fn extra() -> String {
    EXTRA.lock().unwrap().clone()
}

/// Her state as friends see it.
pub fn set_status(status: &str) {
    if let Some(me) = ME.lock().unwrap().as_mut()
        && me.status != status
    {
        me.status = status.to_string();
    }
}

/// Your pairing code and your paired friends' ids (to remember between runs).
pub fn code_and_paired() -> (String, Vec<String>) {
    ME.lock().unwrap().as_ref().map(|m| (m.code.clone(), m.paired.clone())).unwrap_or_default()
}

/// Keqings on the network right now, paired friends first.
pub fn peers() -> Vec<Peer> {
    let mut list: Vec<Peer> = PEERS.lock().unwrap().iter().filter(|p| p.seen.elapsed() < GONE_AFTER).cloned().collect();
    list.sort_by(|a, b| b.paired.cmp(&a.paired).then_with(|| a.name.cmp(&b.name)));
    list
}

/// Asks `peer` to pair, with the code shown on their screen.
pub fn pair(peer: &str, code: &str) {
    let Some(packet) = ME.lock().unwrap().as_mut().map(|me| {
        me.pending.retain(|p| p != peer);
        me.pending.push(peer.to_string());
        json!({"app": "claudesona", "t": "pair", "from": me.id, "name": me.name, "code": code.trim()}).to_string()
    }) else { return };
    send(Some(peer), packet);
}

pub fn poke(peer: &str) {
    if let Some(p) = packet("poke", None) {
        send(Some(peer), p);
    }
}

pub fn message(peer: &str, text: &str) {
    let text: String = text.trim().chars().take(MAX_TEXT).collect();
    if !text.is_empty()
        && let Some(p) = packet("msg", Some(&text))
    {
        send(Some(peer), p);
    }
}

/// Forgets a paired friend: their pokes and messages stop getting through.
pub fn unpair(peer: &str) {
    if let Some(me) = ME.lock().unwrap().as_mut() {
        me.paired.retain(|p| p != peer);
    }
    for p in PEERS.lock().unwrap().iter_mut().filter(|p| p.id == peer) {
        p.paired = false;
    }
}

fn packet(t: &str, text: Option<&str>) -> Option<String> {
    let me = ME.lock().unwrap();
    let me = me.as_ref()?;
    let mut v = json!({"app": "claudesona", "t": t, "from": me.id, "name": me.name});
    if let Some(text) = text {
        v["text"] = json!(text);
    }
    Some(v.to_string())
}

fn send(to: Option<&str>, packet: String) {
    if let Some(tx) = OUTBOX.get() {
        let _ = tx.lock().unwrap().send((to.map(String::from), packet));
    }
}

/// The network thread: says hello every few seconds, reads packets, sends what the UI queued.
fn run(rx: Receiver<(Option<String>, String)>, emit: impl Fn(Event)) {
    let mut sock: Option<UdpSocket> = None;
    let mut last_hello = Instant::now() - HELLO_EVERY;
    let mut buf = [0u8; 2048];
    loop {
        if !is_on() {
            sock = None; // closes the port while off
            std::thread::sleep(Duration::from_millis(300));
            while rx.try_recv().is_ok() {} // drop anything queued while off
            continue;
        }
        let Some(s) = sock.as_ref() else {
            sock = UdpSocket::bind(("0.0.0.0", PORT)).ok().inspect(|s| {
                let _ = s.set_broadcast(true);
                let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
            });
            if sock.is_none() {
                std::thread::sleep(Duration::from_secs(2)); // port busy: try again later
            }
            continue;
        };
        if last_hello.elapsed() >= HELLO_EVERY {
            last_hello = Instant::now();
            if let Some(p) = hello() {
                let _ = s.send_to(p.as_bytes(), ("255.255.255.255", PORT));
                for ip in extra().split([',', ' ']).filter(|ip| !ip.is_empty()) {
                    let _ = s.send_to(p.as_bytes(), (ip, PORT));
                }
            }
        }
        while let Ok((to, packet)) = rx.try_recv() {
            let addr = to.and_then(|id| PEERS.lock().unwrap().iter().find(|p| p.id == id).map(|p| p.addr));
            if let Some(addr) = addr {
                let _ = s.send_to(packet.as_bytes(), addr);
            }
        }
        if let Ok((n, from)) = s.recv_from(&mut buf) {
            let text = String::from_utf8_lossy(&buf[..n]).into_owned();
            let (replies, events) = {
                let mut me = ME.lock().unwrap();
                let mut peers = PEERS.lock().unwrap();
                match me.as_mut() {
                    Some(me) => on_packet(me, &mut peers, from, &text, Instant::now()),
                    None => (vec![], vec![]),
                }
            };
            for r in replies {
                let _ = s.send_to(r.as_bytes(), from);
            }
            for e in events {
                emit(e);
            }
        }
    }
}

fn hello() -> Option<String> {
    let me = ME.lock().unwrap();
    let me = me.as_ref()?;
    Some(json!({"app": "claudesona", "t": "hello", "from": me.id, "name": me.name, "status": me.status}).to_string())
}

/// One packet from `from`: updates the friends list, answers pairing requests, and returns what to send back
/// and what she should show. Pure, so it's tested without a network.
fn on_packet(me: &mut Me, peers: &mut Vec<Peer>, from: SocketAddr, text: &str, now: Instant) -> (Vec<String>, Vec<Event>) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return (vec![], vec![]) };
    let (Some(id), Some(t)) = (v["from"].as_str(), v["t"].as_str()) else { return (vec![], vec![]) };
    if v["app"] != "claudesona" || id == me.id {
        return (vec![], vec![]); // not one of us, or our own broadcast coming back
    }
    let name: String = v["name"].as_str().unwrap_or("?").chars().take(40).collect();
    let paired = me.paired.iter().any(|p| p == id);
    // Every packet keeps its sender on the list (and tells us where to reach them).
    match peers.iter_mut().find(|p| p.id == id) {
        Some(p) => {
            (p.name, p.addr, p.seen, p.paired) = (name.clone(), from, now, paired);
            if t == "hello" || t == "hi" {
                p.status = v["status"].as_str().unwrap_or("").chars().take(40).collect();
            }
        }
        None => peers.push(Peer { id: id.into(), name: name.clone(), status: v["status"].as_str().unwrap_or("").into(), paired, addr: from, seen: now }),
    }
    let mut events = vec![];
    let mut replies = vec![];
    match t {
        // Answer every hello, so a friend who can only reach us by IP (another subnet) sees us too.
        "hello" => replies.push(json!({"app": "claudesona", "t": "hi", "from": me.id, "name": me.name, "status": me.status}).to_string()),
        // Someone typed our code: trust them, and tell them so.
        "pair" if v["code"].as_str() == Some(me.code.as_str()) => {
            if !paired {
                me.paired.push(id.into());
            }
            set_paired(peers, id);
            replies.push(json!({"app": "claudesona", "t": "paired", "from": me.id, "name": me.name}).to_string());
            events.push(Event::Paired(name));
        }
        // They accepted the code we typed.
        "paired" if me.pending.iter().any(|p| p == id) => {
            me.pending.retain(|p| p != id);
            if !paired {
                me.paired.push(id.into());
            }
            set_paired(peers, id);
            events.push(Event::Paired(name));
        }
        "poke" if paired => events.push(Event::Poke(name)),
        "msg" if paired => {
            let text: String = v["text"].as_str().unwrap_or("").chars().take(MAX_TEXT).collect();
            if !text.trim().is_empty() {
                events.push(Event::Message(name, text));
            }
        }
        _ => {}
    }
    (replies, events)
}

fn set_paired(peers: &mut [Peer], id: &str) {
    for p in peers.iter_mut().filter(|p| p.id == id) {
        p.paired = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(id: &str, code: &str) -> Me {
        Me { id: id.into(), name: format!("{id}-pc"), code: code.into(), status: String::new(), paired: vec![], pending: vec![] }
    }
    fn addr() -> SocketAddr {
        "192.168.1.20:47828".parse().unwrap()
    }

    #[test]
    fn hello_gets_a_hi_that_isnt_answered() {
        let mut a = Me { id: "a".into(), name: "A".into(), code: "1234".into(), status: "idle".into(), paired: vec![], pending: vec![] };
        let mut peers = vec![];
        let now = Instant::now();
        let (r, _) = on_packet(&mut a, &mut peers, addr(), r#"{"app":"claudesona","t":"hello","from":"b","name":"B","status":"working"}"#, now);
        assert_eq!(r.len(), 1);
        assert!(r[0].contains(r#""t":"hi""#));
        let (r, _) = on_packet(&mut a, &mut peers, addr(), r#"{"app":"claudesona","t":"hi","from":"b","name":"B","status":"waiting"}"#, now);
        assert!(r.is_empty());
        assert_eq!(peers[0].status, "waiting");
    }

    #[test]
    fn hello_lists_a_friend_and_ignores_strangers_and_ourselves() {
        let (mut a, mut peers, now) = (me("a", "1234"), vec![], Instant::now());
        on_packet(&mut a, &mut peers, addr(), r#"{"app":"claudesona","t":"hello","from":"b","name":"B","status":"working"}"#, now);
        on_packet(&mut a, &mut peers, addr(), r#"{"app":"claudesona","t":"hello","from":"a","name":"me"}"#, now);
        on_packet(&mut a, &mut peers, addr(), r#"{"app":"other","t":"hello","from":"c"}"#, now);
        on_packet(&mut a, &mut peers, addr(), "not json", now);
        assert_eq!(peers.iter().map(|p| (p.id.as_str(), p.status.as_str(), p.paired)).collect::<Vec<_>>(), [("b", "working", false)]);
    }

    #[test]
    fn pairing_needs_the_right_code_then_pokes_and_messages_get_through() {
        let (mut b, mut peers, now) = (me("b", "4321"), vec![], Instant::now());
        // Not paired yet: pokes and messages are dropped.
        let (_, ev) = on_packet(&mut b, &mut peers, addr(), r#"{"app":"claudesona","t":"poke","from":"a","name":"A"}"#, now);
        assert!(ev.is_empty());
        // Wrong code: nothing.
        let (r, ev) = on_packet(&mut b, &mut peers, addr(), r#"{"app":"claudesona","t":"pair","from":"a","name":"A","code":"0000"}"#, now);
        assert!(r.is_empty() && ev.is_empty() && b.paired.is_empty());
        // Right code: paired, and an answer goes back.
        let (r, ev) = on_packet(&mut b, &mut peers, addr(), r#"{"app":"claudesona","t":"pair","from":"a","name":"A","code":"4321"}"#, now);
        assert_eq!(ev, [Event::Paired("A".into())]);
        assert!(r[0].contains(r#""t":"paired""#) && b.paired == ["a"]);
        let (_, ev) = on_packet(&mut b, &mut peers, addr(), r#"{"app":"claudesona","t":"poke","from":"a","name":"A"}"#, now);
        assert_eq!(ev, [Event::Poke("A".into())]);
        let (_, ev) = on_packet(&mut b, &mut peers, addr(), r#"{"app":"claudesona","t":"msg","from":"a","name":"A","text":"lunch?"}"#, now);
        assert_eq!(ev, [Event::Message("A".into(), "lunch?".into())]);
    }

    #[test]
    fn a_paired_answer_counts_only_for_a_request_we_sent() {
        let (mut a, mut peers, now) = (me("a", "1234"), vec![], Instant::now());
        let (_, ev) = on_packet(&mut a, &mut peers, addr(), r#"{"app":"claudesona","t":"paired","from":"b","name":"B"}"#, now);
        assert!(ev.is_empty() && a.paired.is_empty()); // unasked: ignored
        a.pending.push("b".into());
        let (_, ev) = on_packet(&mut a, &mut peers, addr(), r#"{"app":"claudesona","t":"paired","from":"b","name":"B"}"#, now);
        assert_eq!(ev, [Event::Paired("B".into())]);
        assert_eq!(a.paired, ["b"]);
    }
}
