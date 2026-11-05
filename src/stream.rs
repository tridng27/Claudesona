// Everything that knows the stream-json format of a hosted `claude -p` session lives here, the way
// src/log.rs holds the session-log format. Unversioned too: when an update breaks parsing, add the new
// line shape to tests/fixtures/stream first. Checked against Claude Code 2.1.318.
use crate::log::{Work, clean, tool_label, work_kind};
use serde_json::{Value, json};

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Sent again at the start of every turn; a new session_id after /clear.
    Init { session_id: String, model: String, tools: Vec<String>, mcp_servers: Vec<(String, String)>, slash_commands: Vec<String> },
    TextDelta(String), // a piece of Claude's text as it streams
    Text(String),      // a whole text block: a side note or the reply (Result carries the reply)
    Tool { id: String, name: String, input: Value, label: String, work: Work },
    AgentStart { id: String, label: String }, // an Agent call (a mini); ends with AgentDone
    ToolDone { id: String, is_error: bool, refused: bool, summary: String },
    AgentDone(String), // a background agent ended (its ToolDone only said it launched)
    /// Claude asks whether it may run a tool (`--permission-prompt-tool stdio`); answer with Session::allow/deny.
    Permission { request_id: String, tool: String, input: Value, tool_use_id: String },
    Compact,
    Reset, // /clear started a new conversation; an Init with the new session_id follows
    /// The turn is over. `text` is the final reply, or a local command's output (/context, /model…).
    Result { text: String, is_error: bool, interrupted: bool },
    Exit, // the process ended (sent by Session, not parsed)
}

pub fn parse_line(line: &str) -> Vec<Event> {
    let Ok(d) = serde_json::from_str::<Value>(line) else { return vec![] };
    // A subagent's own steps: v1 ignores sidechains too; the mini shows the agent as a whole.
    if !d["parent_tool_use_id"].is_null() {
        return vec![];
    }
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    match (d["type"].as_str().unwrap_or(""), d["subtype"].as_str().unwrap_or("")) {
        ("system", "init") => {
            let names = |v: &Value| v.as_array().into_iter().flatten().map(s).collect();
            let mcp = d["mcp_servers"].as_array().into_iter().flatten().map(|m| (s(&m["name"]), s(&m["status"]))).collect();
            vec![Event::Init { session_id: s(&d["session_id"]), model: s(&d["model"]), tools: names(&d["tools"]), mcp_servers: mcp, slash_commands: names(&d["slash_commands"]) }]
        }
        ("system", "compact_boundary") => vec![Event::Compact],
        ("system", "task_notification") => vec![Event::AgentDone(s(&d["tool_use_id"]))],
        ("conversation_reset", _) => vec![Event::Reset],
        ("stream_event", _) => match d["event"]["delta"]["text"].as_str() {
            Some(t) if d["event"]["delta"]["type"] == "text_delta" => vec![Event::TextDelta(t.to_string())],
            _ => vec![],
        },
        // A local command's output comes as a "<synthetic>" message and again in the result.
        ("assistant", _) if d["message"]["model"] == "<synthetic>" => vec![],
        ("assistant", _) => blocks(&d["message"]["content"])
            .flat_map(|b| match b["type"].as_str() {
                Some("text") if !b["text"].as_str().unwrap_or("").trim().is_empty() => vec![Event::Text(s(&b["text"]))],
                Some("tool_use") => {
                    let (id, name, input) = (s(&b["id"]), s(&b["name"]), b["input"].clone());
                    let mut v = vec![];
                    if matches!(name.as_str(), "Agent" | "Task") {
                        v.push(Event::AgentStart { id: id.clone(), label: clean(input["description"].as_str().unwrap_or("helper"), 18) });
                    }
                    v.insert(0, Event::Tool { label: tool_label(&name, &input), work: work_kind(&name, &input), id, name, input });
                    v
                }
                _ => vec![],
            })
            .collect(),
        ("user", _) => {
            // A background agent's tool_result only says it launched; its end comes as a task_notification.
            if d["tool_use_result"]["status"] == "async_launched" {
                return vec![];
            }
            let rejected = |id: &str| blocks(&d["tool_result_meta"]).any(|m| m["id"] == id && m["permission_decision"]["decision"] == "reject");
            blocks(&d["message"]["content"])
                .filter(|b| b["type"] == "tool_result")
                .map(|b| {
                    let id = s(&b["tool_use_id"]);
                    let c = &b["content"];
                    let text = c.as_str().map(str::to_string).unwrap_or_else(|| blocks(c).map(|x| s(&x["text"])).collect::<Vec<_>>().join("\n"));
                    let refused = rejected(&id) || text.starts_with("[Request interrupted by user");
                    Event::ToolDone { is_error: b["is_error"] == true, refused, summary: clean(&text, 80), id }
                })
                .collect()
        }
        ("control_request", _) if d["request"]["subtype"] == "can_use_tool" => {
            let r = &d["request"];
            vec![Event::Permission { request_id: s(&d["request_id"]), tool: s(&r["tool_name"]), input: r["input"].clone(), tool_use_id: s(&r["tool_use_id"]) }]
        }
        ("result", _) => vec![Event::Result {
            text: s(&d["result"]),
            is_error: d["is_error"] == true,
            interrupted: d["terminal_reason"] == "aborted_streaming",
        }],
        _ => vec![],
    }
}

fn blocks(v: &Value) -> impl Iterator<Item = &Value> + Clone {
    v.as_array().into_iter().flatten()
}

// Lines written to the process's stdin.

pub fn user_message(text: &str) -> Value {
    json!({ "type": "user", "message": { "role": "user", "content": text } })
}

/// Stops the current turn; the process stays up for the next message.
pub fn interrupt(request_id: &str) -> Value {
    json!({ "type": "control_request", "request_id": request_id, "request": { "subtype": "interrupt" } })
}

/// The answer to a Permission event. Allow must echo the tool input (it may be edited).
pub fn permission_answer(request_id: &str, allow: Option<&Value>, deny_message: &str) -> Value {
    let response = match allow {
        Some(input) => json!({ "behavior": "allow", "updatedInput": input }),
        None => json!({ "behavior": "deny", "message": deny_message }),
    };
    json!({ "type": "control_response", "response": { "subtype": "success", "request_id": request_id, "response": response } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(fixture: &str) -> Vec<Event> {
        fixture.lines().flat_map(parse_line).collect()
    }

    /// Events without the streamed text pieces, which are many and only checked by count.
    fn main(fixture: &str) -> Vec<Event> {
        events(fixture).into_iter().filter(|e| !matches!(e, Event::TextDelta(_))).collect()
    }

    #[test]
    fn tool_turn() {
        let ev = events(include_str!("../tests/fixtures/stream/tool_turn.jsonl"));
        let Event::Init { session_id, model, tools, mcp_servers, slash_commands } = &ev[0] else { panic!("{:?}", ev[0]) };
        assert_eq!((session_id.as_str(), model.as_str()), ("4a7b1123-8219-4d7e-978d-5322110ea5c8", "claude-haiku-5-5"));
        assert!(tools.iter().any(|t| t == "Read") && slash_commands.iter().any(|c| c == "compact"));
        assert!(mcp_servers.contains(&("plugin:apify:apify".into(), "pending".into())));
        let deltas: String = ev.iter().filter_map(|e| if let Event::TextDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(deltas, "The secret word is **pineapple**, according to notes.txt.");
        let rest = &main(include_str!("../tests/fixtures/stream/tool_turn.jsonl"))[1..];
        let Event::Tool { id, name, label, work, .. } = &rest[0] else { panic!("{:?}", rest[0]) };
        assert_eq!((id.as_str(), name.as_str(), label.as_str(), *work), ("toolu_0122cAmzecV68T56UGcQayWm", "Read", "Reading notes.txt…", Work::Reading));
        assert_eq!(rest[1], Event::ToolDone { id: id.clone(), is_error: false, refused: false, summary: "1 The secret word is pineapple. 2".into() });
        assert_eq!(rest[2], Event::Text("The secret word is **pineapple**, according to notes.txt.".into()));
        assert_eq!(rest[3], Event::Result { text: "The secret word is **pineapple**, according to notes.txt.".into(), is_error: false, interrupted: false });
        assert_eq!(rest.len(), 4);
    }

    #[test]
    fn agent_turn() {
        // The subagent's own tool calls (parent_tool_use_id set) don't show up.
        let ev = main(include_str!("../tests/fixtures/stream/agent_turn.jsonl"));
        let id = "toolu_01Ad698NtDw1W5E5FQSXBmPx".to_string();
        assert!(matches!(&ev[1], Event::Tool { name, work: Work::Plain, .. } if name == "Agent"));
        assert_eq!(ev[2], Event::AgentStart { id: id.clone(), label: "Count lines in not…".into() });
        // The notification of a foreground agent comes before its tool_result.
        assert_eq!(ev[3], Event::AgentDone(id.clone()));
        assert!(matches!(&ev[4], Event::ToolDone { id: i, is_error: false, .. } if *i == id));
        assert!(matches!(&ev[6], Event::Result { text, .. } if text == "The subagent counted one line in notes.txt."));
        assert_eq!(ev.len(), 7);
    }

    #[test]
    fn background_agent() {
        let ev = main(include_str!("../tests/fixtures/stream/background_agent.jsonl"));
        let id = "toolu_01HorGwA2eyiHjzJ21tKGYCb".to_string();
        // Launch reply skipped; the turn ends; the notification ends the mini and starts a turn by itself.
        assert!(matches!(&ev[2], Event::AgentStart { id: i, .. } if *i == id));
        assert!(matches!(&ev[4], Event::Result { .. }));
        assert_eq!(ev[5], Event::AgentDone(id));
        assert!(matches!(&ev[6], Event::Init { .. }));
        assert!(matches!(&ev[8], Event::Result { is_error: false, .. }));
        assert!(!ev.iter().any(|e| matches!(e, Event::ToolDone { .. })));
    }

    #[test]
    fn interrupt_and_permission() {
        let ev = main(include_str!("../tests/fixtures/stream/interrupt.jsonl"));
        let results: Vec<_> = ev.iter().filter(|e| matches!(e, Event::Result { .. })).collect();
        assert_eq!(*results[0], Event::Result { text: "".into(), is_error: true, interrupted: true });
        assert_eq!(*results[1], Event::Result { text: "ok".into(), is_error: false, interrupted: false });

        let ev = main(include_str!("../tests/fixtures/stream/permission.jsonl"));
        let perms: Vec<_> = ev.iter().filter_map(|e| if let Event::Permission { request_id, tool, tool_use_id, .. } = e { Some((request_id.as_str(), tool.as_str(), tool_use_id.as_str())) } else { None }).collect();
        assert_eq!(perms, [("75158ceb-67e9-47ca-b571-2fab2d448129", "Write", "toolu_01CRhtNRTeG6a85dRWPkgHVn"), ("6e523262-1624-4c7e-a6a7-916269d5ef49", "Write", "toolu_01P64wze5puk1FCCi5o49APJ")]);
        let done: Vec<_> = ev.iter().filter_map(|e| if let Event::ToolDone { is_error, refused, .. } = e { Some((*is_error, *refused)) } else { None }).collect();
        assert_eq!(done, [(false, false), (true, true)]); // allowed, then denied by the user: not an Oops
    }

    #[test]
    fn slash_commands() {
        let ev = main(include_str!("../tests/fixtures/stream/slash.jsonl"));
        let results: Vec<_> = ev.iter().filter_map(|e| if let Event::Result { text, .. } = e { Some(text.as_str()) } else { None }).collect();
        assert_eq!(results, ["custom-ok there", "/help isn't available in this environment.", "", "Set model to `Sonnet 5.5` for this session only", ""]);
        assert!(ev.contains(&Event::Compact) && ev.contains(&Event::Reset));
        let inits: Vec<_> = ev.iter().filter_map(|e| if let Event::Init { session_id, model, .. } = e { Some((&session_id[..8], model.as_str())) } else { None }).collect();
        assert_eq!(inits.last(), Some(&("21f4f232", "claude-sonnet-5-5"))); // /clear: new id; /model stuck
        // Only /hi (a custom command, run by the model) has text; local output only arrives in the result.
        let texts: Vec<_> = ev.iter().filter(|e| matches!(e, Event::Text(_))).collect();
        assert_eq!(texts, [&Event::Text("custom-ok there".into())]);
    }

    #[test]
    fn stdin_lines() {
        assert_eq!(user_message("hi"), json!({"type":"user","message":{"role":"user","content":"hi"}}));
        let allow = permission_answer("r1", Some(&json!({"a":1})), "");
        assert_eq!(allow["response"]["response"], json!({"behavior":"allow","updatedInput":{"a":1}}));
        assert_eq!(permission_answer("r1", None, "no")["response"]["response"]["behavior"], "deny");
        assert!(parse_line("not json").is_empty());
    }
}
