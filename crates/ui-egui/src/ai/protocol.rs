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
    ThreadStarted { thread_id: String },
    TurnStarted { thread_id: Option<String>, turn_id: String },
    TurnCompleted { thread_id: Option<String>, turn_id: Option<String> },
    AgentDelta { thread_id: Option<String>, turn_id: Option<String>, text: String },
    ItemStarted { thread_id: Option<String>, turn_id: Option<String>, item_id: String, tool_name: Option<String> },
    ItemCompleted { thread_id: Option<String>, turn_id: Option<String>, item_id: String, is_error: bool, summary: Option<String> },
    ApprovalRequired { thread_id: Option<String>, turn_id: Option<String>, request_id: Value, action: String, target: String },
    ToolCall { thread_id: Option<String>, turn_id: Option<String>, call_id: String, request_id: Value, name: String, params: Value },
    Error { thread_id: Option<String>, message: String },
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
        ServerMessage::Notification { method, params } => {
            let thread_id = params.get("threadId").and_then(Value::as_str).map(|s| s.to_string());
            match method.as_str() {
                "thread/started" => {
                    let tid = params
                        .get("thread")
                        .and_then(|t| t.get("id"))
                        .and_then(Value::as_str)
                        .or_else(|| params.get("threadId").and_then(Value::as_str))?
                        .to_string();
                    Some(ProtocolEvent::ThreadStarted { thread_id: tid })
                }
                "turn/started" => {
                    let turn_id = params
                        .get("turn")
                        .and_then(|t| t.get("id"))
                        .and_then(Value::as_str)
                        .or_else(|| params.get("turnId").and_then(Value::as_str))
                        .unwrap_or("")
                        .to_string();
                    Some(ProtocolEvent::TurnStarted { thread_id, turn_id })
                }
                "turn/completed" => {
                    let turn_id = params
                        .get("turn")
                        .and_then(|t| t.get("id"))
                        .and_then(Value::as_str)
                        .or_else(|| params.get("turnId").and_then(Value::as_str))
                        .map(|s| s.to_string());
                    Some(ProtocolEvent::TurnCompleted { thread_id, turn_id })
                }
                "item/agentMessage/delta" => {
                    let delta = params.get("delta").and_then(Value::as_str)?;
                    let turn_id = params.get("turnId").and_then(Value::as_str).map(|s| s.to_string());
                    Some(ProtocolEvent::AgentDelta { thread_id, turn_id, text: delta.to_string() })
                }
                "item/started" => {
                    let item = params.get("item")?;
                    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                    let tool_name = item.get("name").and_then(Value::as_str).map(|s| s.to_string());
                    let turn_id = params.get("turnId").and_then(Value::as_str).map(|s| s.to_string());
                    Some(ProtocolEvent::ItemStarted { thread_id, turn_id, item_id, tool_name })
                }
                "item/completed" => {
                    let item = params.get("item")?;
                    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                    let is_error = item.get("status").and_then(Value::as_str) == Some("failed");
                    let summary = item.get("summary").and_then(Value::as_str).map(|s| s.to_string());
                    let turn_id = params.get("turnId").and_then(Value::as_str).map(|s| s.to_string());
                    Some(ProtocolEvent::ItemCompleted { thread_id, turn_id, item_id, is_error, summary })
                }
                "error" => {
                    let text = params.get("message").and_then(Value::as_str).unwrap_or("unknown server error");
                    Some(ProtocolEvent::Error { thread_id, message: text.to_string() })
                }
                _ => None,
            }
        }
        ServerMessage::Request { id, method, params } => {
            let thread_id = params.get("threadId").and_then(Value::as_str).map(|s| s.to_string());
            let turn_id = params.get("turnId").and_then(Value::as_str).map(|s| s.to_string());
            match method.as_str() {
                "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" | "item/permissions/requestApproval" => {
                    let action = params.get("action").and_then(Value::as_str).unwrap_or("permission request").to_string();
                    let target = params.get("target").or_else(|| params.get("command")).and_then(Value::as_str).unwrap_or("").to_string();
                    Some(ProtocolEvent::ApprovalRequired { thread_id, turn_id, request_id: id.clone(), action, target })
                }
                "item/tool/call" => {
                    let call_id = params.get("callId").and_then(Value::as_str).unwrap_or("").to_string();
                    let name = params.get("tool").or_else(|| params.get("name")).and_then(Value::as_str).unwrap_or("").to_string();
                    let tool_params = params.get("arguments").or_else(|| params.get("parameters")).cloned().unwrap_or(Value::Null);
                    Some(ProtocolEvent::ToolCall { thread_id, turn_id, call_id, request_id: id.clone(), name, params: tool_params })
                }
                _ => None,
            }
        }
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

/// Create a Client Notification line formatted as JSON-RPC 2.0.
pub fn format_notification(method: &str, params: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    })
    .to_string()
}

/// Create a Client Response line to an app-server request.
pub fn format_response(id: &Value, result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
    .to_string()
}

/// Format a DynamicToolCallResponse compliant with Codex app-server schema.
pub fn format_dynamic_tool_response(id: &Value, success: bool, text_content: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "success": success,
            "contentItems": [
                {
                    "type": "inputText",
                    "text": text_content
                }
            ]
        }
    })
    .to_string()
}

/// Format a JSON-RPC 2.0 error response.
pub fn format_error_response(id: &Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
    .to_string()
}

/// Format an ApprovalResponse compliant with Codex app-server schema.
pub fn format_approval_response(id: &Value, allow: bool) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "decision": if allow { "allow" } else { "deny" }
        }
    })
    .to_string()
}

/// Format a schema-correct permission denial response for Codex app-server.
pub fn format_permission_denial_response(id: &Value, method: &str, reason: &str) -> String {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "decision": "decline"
            }
        })
        .to_string(),
        "item/execCommand/requestApproval" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "decision": {
                    "denied": {
                        "rejection": reason
                    }
                }
            }
        })
        .to_string(),
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "decision": "decline"
            }
        })
        .to_string(),
    }
}
