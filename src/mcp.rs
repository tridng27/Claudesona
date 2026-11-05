// `claudesona mcp`: a stdio MCP server with one tool, `approve`, for hosted sessions'
// `--permission-prompt-tool mcp__claudesona__approve`. Newline-delimited JSON-RPC on stdin/stdout; stdout carries nothing else.
//
// Claude Code calls `approve` with {"tool_name", "input", "tool_use_id"} and wants text content holding
// {"behavior":"allow","updatedInput":{..}} or {"behavior":"deny","message":".."}.
// We forward {"permission": <arguments>} to the app on cli::ADDR, shut our write half, and read one line back:
// that same allow/deny JSON, plus "always": true to auto-allow the tool for the rest of this process (= the session).
use crate::cli;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

pub fn run() {
    let mut always = HashSet::new();
    let mut out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(reply) = respond(&msg, &mut always, ask_app) {
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
}

/// The app's answer to one permission request; None when it can't be reached.
fn ask_app(args: &Value) -> Option<Value> {
    let mut s = TcpStream::connect_timeout(&cli::ADDR.parse().unwrap(), Duration::from_millis(300)).ok()?;
    s.write_all(json!({ "permission": args }).to_string().as_bytes()).ok()?;
    s.shutdown(Shutdown::Write).ok()?; // the app reads to EOF
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?; // no timeout: you may take a while to answer
    serde_json::from_str(&line).ok()
}

/// Called by the UI thread to answer a `Msg::Permission`. `input` is the tool input to run with (usually the request's own).
#[allow(dead_code)] // v2: called by the permission bubble
pub fn answer(mut reply: TcpStream, allow: bool, always: bool, input: &Value, message: &str) {
    let v = if allow { json!({ "behavior": "allow", "updatedInput": input, "always": always }) } else { json!({ "behavior": "deny", "message": message }) };
    let _ = writeln!(reply, "{v}");
}

/// The JSON-RPC reply to one message, or None for notifications.
fn respond(msg: &Value, always: &mut HashSet<String>, ask: impl Fn(&Value) -> Option<Value>) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "claudesona", "version": env!("CARGO_PKG_VERSION") },
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": [{
            "name": "approve",
            "description": "Asks the user, through Claudesona, whether Claude may use a tool.",
            "inputSchema": {
                "type": "object",
                "properties": { "tool_name": { "type": "string" }, "input": { "type": "object" }, "tool_use_id": { "type": "string" } },
                "required": ["tool_name", "input"],
            },
        }]}),
        "tools/call" if msg["params"]["name"] == "approve" => {
            let args = &msg["params"]["arguments"];
            let tool = args["tool_name"].as_str().unwrap_or("").to_string();
            let decision = if always.contains(&tool) {
                json!({ "behavior": "allow", "updatedInput": args["input"] })
            } else {
                match ask(args) {
                    Some(mut v) if v["behavior"] == "allow" || v["behavior"] == "deny" => {
                        if v["behavior"] == "allow" && v["always"] == true {
                            always.insert(tool);
                        }
                        v.as_object_mut().unwrap().remove("always");
                        v
                    }
                    _ => json!({ "behavior": "deny", "message": "Claudesona isn't running, so nobody could approve this." }),
                }
            };
            json!({ "content": [{ "type": "text", "text": decision.to_string() }] })
        }
        m => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("unknown method {m}") } })),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn call(id: u64, tool: &str) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"approve","arguments":{"tool_name":tool,"input":{"file_path":"x.txt"},"tool_use_id":"t1"}}})
    }
    fn decision(reply: Option<Value>) -> Value {
        serde_json::from_str(reply.unwrap()["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn handshake() {
        let mut set = HashSet::new();
        let init = respond(&json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}), &mut set, |_| None).unwrap();
        assert_eq!(init["id"], 0);
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(respond(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}), &mut set, |_| None).is_none());
        let list = respond(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}), &mut set, |_| None).unwrap();
        assert_eq!(list["result"]["tools"][0]["name"], "approve");
        let bad = respond(&json!({"jsonrpc":"2.0","id":2,"method":"resources/list"}), &mut set, |_| None).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }

    #[test]
    fn app_gone_denies() {
        let d = decision(respond(&call(3, "Write"), &mut HashSet::new(), |_| None));
        assert_eq!(d["behavior"], "deny");
        assert!(d["message"].is_string());
    }

    #[test]
    fn always_skips_the_app() {
        let mut set = HashSet::new();
        let asked = Cell::new(0);
        let ask = |a: &Value| {
            asked.set(asked.get() + 1);
            Some(json!({"behavior":"allow","updatedInput":a["input"],"always":true}))
        };
        let d = decision(respond(&call(4, "Write"), &mut set, ask));
        assert_eq!(d, json!({"behavior":"allow","updatedInput":{"file_path":"x.txt"}})); // "always" isn't passed on
        let d = decision(respond(&call(5, "Write"), &mut set, ask));
        assert_eq!(d["behavior"], "allow");
        assert_eq!(asked.get(), 1);
        // Other tools still ask.
        let d = decision(respond(&call(6, "Bash"), &mut set, |_| Some(json!({"behavior":"deny","message":"no"}))));
        assert_eq!(d, json!({"behavior":"deny","message":"no"}));
    }
}
