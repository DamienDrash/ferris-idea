//! One tool call without an MCP client (`SPEC-0281` behaviour 5): `idecraft call <folder> <tool>
//! [<json>]` serves the workspace over an in-memory pipe, speaks the same JSON-RPC a client would,
//! and returns the tool's answer. So every decision a dialog asks for is also reachable from a
//! shell, with the same rights and safe mode as `idecraft mcp`.

use rmcp::ServiceExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::IdeServer;

/// What a tool call returned.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The tool's result: the JSON it returned, or the whole result when it was not JSON.
    Ok(Value),
    /// The tool refused or failed; the message says why.
    ToolError(String),
}

/// Calls `tool` with `arguments` (a JSON object) on `server` and returns its answer. `Err` means
/// the call could not be made at all (the server did not start or answer).
pub async fn call_once(server: IdeServer, tool: &str, arguments: Value) -> Result<Answer, String> {
    let (client_io, server_io) = tokio::io::duplex(1 << 16);
    let serving = tokio::spawn(async move {
        if let Ok(running) = server.serve(tokio::io::split(server_io)).await {
            let _ = running.waiting().await;
        }
    });
    let (read, mut write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(read).lines();
    let mut send = async |message: Value| -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        write
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write.flush().await.map_err(|e| e.to_string())
    };
    send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "idecraft-call", "version": env!("CARGO_PKG_VERSION")},
        },
    }))
    .await?;
    reply(&mut lines, 1).await?;
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await?;
    send(json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    }))
    .await?;
    let answer = reply(&mut lines, 2).await?;
    serving.abort();
    Ok(interpret(&answer))
}

/// The tools of `server` (name, description, input schema each), as `tools/list` returns them
/// (`SPEC-0318` behaviour 29: a remote project's tools are its host's).
pub async fn list_tools(server: IdeServer) -> Result<Value, String> {
    let (client_io, server_io) = tokio::io::duplex(1 << 16);
    let serving = tokio::spawn(async move {
        if let Ok(running) = server.serve(tokio::io::split(server_io)).await {
            let _ = running.waiting().await;
        }
    });
    let (read, mut write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(read).lines();
    let mut send = async |message: Value| -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        write
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write.flush().await.map_err(|e| e.to_string())
    };
    send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "idecraft-serve", "version": env!("CARGO_PKG_VERSION")},
        },
    }))
    .await?;
    reply(&mut lines, 1).await?;
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await?;
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})).await?;
    let answer = reply(&mut lines, 2).await?;
    serving.abort();
    if let Some(e) = answer.get("error") {
        return Err(e["message"]
            .as_str()
            .unwrap_or("tools/list failed")
            .to_owned());
    }
    Ok(answer["result"]["tools"].clone())
}

/// Reads lines until the response with `id`.
async fn reply<R: tokio::io::AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
    id: u64,
) -> Result<Value, String> {
    loop {
        let line = lines
            .next_line()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("the MCP server closed before answering")?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message["id"] == json!(id) {
            return Ok(message);
        }
    }
}

/// Turns a JSON-RPC response to `tools/call` into an [`Answer`].
pub fn interpret(response: &Value) -> Answer {
    if let Some(error) = response.get("error") {
        let message = error["message"].as_str().unwrap_or("the tool failed");
        return Answer::ToolError(message.to_owned());
    }
    let result = &response["result"];
    let texts: Vec<&str> = result["content"]
        .as_array()
        .map(|c| c.iter().filter_map(|b| b["text"].as_str()).collect())
        .unwrap_or_default();
    if result["isError"] == json!(true) {
        let message = if texts.is_empty() {
            "the tool failed".to_owned()
        } else {
            texts.join("\n")
        };
        return Answer::ToolError(message);
    }
    match texts.as_slice() {
        [one] => Answer::Ok(serde_json::from_str(one).unwrap_or_else(|_| json!(one))),
        _ => Answer::Ok(result.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_results_and_errors() {
        let ok = json!({"result": {"content": [{"type": "text", "text": "{\"a\":1}"}]}});
        assert_eq!(interpret(&ok), Answer::Ok(json!({"a": 1})));
        let text = json!({"result": {"content": [{"type": "text", "text": "plain"}]}});
        assert_eq!(interpret(&text), Answer::Ok(json!("plain")));
        let refused = json!({"error": {"code": -32602, "message": "no dialog x"}});
        assert_eq!(interpret(&refused), Answer::ToolError("no dialog x".into()));
        let failed =
            json!({"result": {"isError": true, "content": [{"type": "text", "text": "boom"}]}});
        assert_eq!(interpret(&failed), Answer::ToolError("boom".into()));
    }
}
