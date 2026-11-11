//! JSON-RPC 2.0 protocol definitions and stream parsing for the Codex app-server.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Maximum message size allowed on stdio (16 MiB).
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Supported client information sent during initialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

impl Default for ClientInfo {
    fn default() -> Self {
        Self { name: "figcraft".into(), version: "0.15.0".into() }
    }
}

/// An incoming JSON-RPC message from Codex app-server.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerMessage {
    Response { id: u64, result: Option<Value>, error: Option<Value> },
    Notification { method: String, params: Value },
    Request { id: Value, method: String, params: Value },
}

/// Parsed event for UI and turn tracking.
#[derive(Clone, Debug, PartialEq)]
pub enum ProtocolEvent {
    Initialized { models: Vec<String>, account_email: Option<String>, plan_type: Option<String> },
    AgentDelta { text: String },
    ItemStarted { item_id: String, tool_name: Option<String> },
    ItemCompleted { item_id: String, is_error: bool, summary: Option<String> },
    TurnStarted { turn_id: String },
    TurnCompleted { turn_id: Option<String> },
    ApprovalRequired { request_id: Value, action: String, target: String },
    ToolCall { request_id: Value, name: String, params: Value },
    Error { message: String },
}

/// Parse a raw incoming line from the app-server.
pub fn parse_message_line(line: &str) -> Result<ServerMessage, String> {
    if line.len() > MAX_MESSAGE_SIZE {
        return Err("protocol error: message exceeds maximum size of 16 MiB".into());
    }

    let val: Value = serde_json::from_str(line.trim()).map_err(|e| format!("protocol error: malformed JSON: {e}"))?;

    let obj = val.as_object().ok_or("protocol error: expected JSON object")?;

    if let Some(id_val) = obj.get("id") {
        if let Some(method_val) = obj.get("method").and_then(Value::as_str) {
            // It is a Server-initiated Request
            Ok(ServerMessage::Request {
                id: id_val.clone(),
                method: method_val.to_string(),
                params: obj.get("params").cloned().unwrap_or(Value::Null),
            })
        } else if let Some(id_num) = id_val.as_u64() {
            // It is a Response to a Client Request
            Ok(ServerMessage::Response { id: id_num, result: obj.get("result").cloned(), error: obj.get("error").cloned() })
        } else {
            Err("protocol error: response id must be an integer".into())
        }
    } else if let Some(method) = obj.get("method").and_then(Value::as_str) {
        // It is a Notification
        Ok(ServerMessage::Notification { method: method.to_string(), params: obj.get("params").cloned().unwrap_or(Value::Null) })
    } else {
        Err("protocol error: message must have an id or method".into())
    }
}

/// Convert a ServerMessage into a high-level ProtocolEvent if applicable.
pub fn message_to_event(msg: &ServerMessage) -> Option<ProtocolEvent> {
    match msg {
        ServerMessage::Notification { method, params } => match method.as_str() {
            "item/agentMessage/delta" => {
                let delta = params.get("delta").and_then(Value::as_str)?;
                Some(ProtocolEvent::AgentDelta { text: delta.to_string() })
            }
            "item/started" => {
                let item = params.get("item")?;
                let item_id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                let tool_name = item.get("name").and_then(Value::as_str).map(|s| s.to_string());
                Some(ProtocolEvent::ItemStarted { item_id, tool_name })
            }
            "item/completed" => {
                let item = params.get("item")?;
                let item_id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                let is_error = item.get("status").and_then(Value::as_str) == Some("failed");
                let summary = item.get("summary").and_then(Value::as_str).map(|s| s.to_string());
                Some(ProtocolEvent::ItemCompleted { item_id, is_error, summary })
            }
            "turn/started" => {
                let turn_id = params.get("turnId").and_then(Value::as_str).unwrap_or("").to_string();
                Some(ProtocolEvent::TurnStarted { turn_id })
            }
            "turn/completed" => {
                let turn_id = params.get("turnId").and_then(Value::as_str).map(|s| s.to_string());
                Some(ProtocolEvent::TurnCompleted { turn_id })
            }
            "error" => {
                let text = params.get("message").and_then(Value::as_str).unwrap_or("unknown server error");
                Some(ProtocolEvent::Error { message: text.to_string() })
            }
            _ => None,
        },
        ServerMessage::Request { id, method, params } => match method.as_str() {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" | "item/permissions/requestApproval" => {
                let action = params.get("action").and_then(Value::as_str).unwrap_or("permission request").to_string();
                let target = params.get("target").or_else(|| params.get("command")).and_then(Value::as_str).unwrap_or("").to_string();
                Some(ProtocolEvent::ApprovalRequired { request_id: id.clone(), action, target })
            }
            "item/tool/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let tool_params = params.get("parameters").cloned().unwrap_or(Value::Null);
                Some(ProtocolEvent::ToolCall { request_id: id.clone(), name, params: tool_params })
            }
            _ => None,
        },
        ServerMessage::Response { .. } => None,
    }
}

/// Create a Client Request line formatted as JSON-RPC 2.0.
pub fn format_request(id: u64, method: &str, params: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
    .to_string()
}

/// Create a Client Response line to an app-server approval or tool call.
pub fn format_response(id: &Value, result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
    .to_string()
}
