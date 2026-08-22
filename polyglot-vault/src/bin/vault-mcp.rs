//! MCP stdio server for a Polyglot vault (`08_MCP_AND_AI.md`).
//!
//! Usage: `vault-mcp <vault-root>`, spoken to over stdin/stdout as
//! newline-delimited JSON-RPC 2.0 — the MCP stdio transport. All tool logic
//! lives in `polyglot_vault::mcp`; this file is only the protocol loop.
//!
//! Written against the wire format directly rather than pulling in an MCP
//! SDK: the surface actually needed is three methods (`initialize`,
//! `tools/list`, `tools/call`), and every Rust SDK is async-first, which
//! would drag tokio into a crate that is otherwise entirely synchronous.
//!
//! **stdout is the protocol channel** — anything printed there that isn't a
//! JSON-RPC message corrupts the stream, so every diagnostic goes to stderr.

use std::io::{BufRead, Write};

use polyglot_vault::mcp::{McpServer, tool_definitions};
use serde_json::{Value, json};

/// Echoed back to the client when it asks for a version we know; otherwise
/// we answer with this one and let the client decide whether to proceed.
const FALLBACK_PROTOCOL_VERSION: &str = "2024-11-05";
const KNOWN_PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

fn main() {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("usage: vault-mcp <vault-root>");
        std::process::exit(2);
    };

    let server = match McpServer::open(std::path::Path::new(&root)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to open vault at {root}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("polyglot vault-mcp: indexed {} files from {}", server.file_count(), server.root().display());

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            // A malformed line has no id to answer against, so there is
            // nothing well-formed to reply with — note it and read on.
            eprintln!("skipping unparseable line");
            continue;
        };

        // A notification (no `id`) must never be answered — replying to one
        // is a protocol violation that some clients treat as fatal.
        let Some(id) = request.get("id").cloned() else { continue };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => {
                let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = if KNOWN_PROTOCOL_VERSIONS.contains(&requested) { requested } else { FALLBACK_PROTOCOL_VERSION };
                ok(id, json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "polyglot-vault", "version": env!("CARGO_PKG_VERSION")}
                }))
            }
            "ping" => ok(id, json!({})),
            "tools/list" => ok(id, json!({"tools": tool_definitions()})),
            "tools/call" => match call_tool(&server, &params) {
                Ok(text) => ok(id, json!({"content": [{"type": "text", "text": text}]})),
                // A tool that failed reports through `isError`, not a
                // JSON-RPC error: the model is meant to see the message and
                // adjust, which a transport-level error wouldn't let it do.
                Err(message) => ok(id, json!({"content": [{"type": "text", "text": message}], "isError": true})),
            },
            _ => err(id, -32601, &format!("unknown method: {method}")),
        };

        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            break; // client hung up
        }
    }
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn call_tool(server: &McpServer, params: &Value) -> Result<String, String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let string = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let number = |key: &str| args.get(key).and_then(Value::as_u64).map(|n| n as usize);

    match name {
        "vault_search" => {
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("auto");
            let hits = server.search(&string("query"), kind, number("limit"));
            Ok(render(&hits))
        }
        "vault_read" => {
            let mode = args.get("mode").and_then(Value::as_str).unwrap_or("full");
            server.read(&string("uri"), mode, number("start_line"), number("end_line"))
        }
        "vault_neighbors" => {
            let rel: Vec<String> = args
                .get("rel")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let direction = args.get("direction").and_then(Value::as_str).unwrap_or("both");
            let hits = server.neighbors(&string("uri"), &rel, number("depth").unwrap_or(1), direction, number("limit"));
            Ok(render(&hits))
        }
        "vault_propose_link" => {
            let id = server.propose_link(&string("from"), &string("to"), &string("rel"), "mcp-client", &string("rationale"))?;
            Ok(format!("Proposed. Pending approval by the user as {id} — the link does not exist until they approve it."))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Hits go out as pretty JSON rather than prose: the uri and
/// `neighbors_hint` are meant to be read back precisely and fed into the
/// next call, and a prose rendering invites the model to paraphrase them.
fn render(hits: &[polyglot_vault::mcp::Hit]) -> String {
    if hits.is_empty() {
        return "No results.".to_string();
    }
    serde_json::to_string_pretty(hits).unwrap_or_else(|e| format!("failed to serialize results: {e}"))
}
