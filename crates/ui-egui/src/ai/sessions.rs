//! AI Composer session, history, and approval state management.

use figcraft_doc::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::draft::ObjectMention;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TurnStatus {
    #[default]
    Idle,
    Running,
    AwaitingApproval,
    Stopped,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub tool_id: String,
    pub tool_name: String,
    pub params: Value,
    pub status: ToolStatus,
    pub result_summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub method: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub request_id: Value,
    pub action: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: MessageRole,
    pub text: String,
    /// The objects the user named, frozen when the turn was sent so the timeline shows the same
    /// references the task's authority was built from.
    #[serde(default)]
    pub mentions: Vec<ObjectMention>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<ToolCallInfo>,
}

impl AiMessage {
    pub fn user(text: impl Into<String>, mentions: Vec<ObjectMention>) -> Self {
        Self { role: MessageRole::User, text: text.into(), mentions, tool_call: None }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: MessageRole::Assistant, text: text.into(), mentions: Vec::new(), tool_call: None }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self { role: MessageRole::System, text: text.into(), mentions: Vec::new(), tool_call: None }
    }

    pub fn tool(tool_id: &str, tool_name: &str, params: Value) -> Self {
        Self {
            role: MessageRole::Tool,
            text: String::new(),
            mentions: Vec::new(),
            tool_call: Some(ToolCallInfo {
                tool_id: tool_id.to_string(),
                tool_name: tool_name.to_string(),
                params,
                status: ToolStatus::Running,
                result_summary: None,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftAttachment {
    pub name: String,
    pub is_image: bool,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiSession {
    pub session_id: String,
    pub thread_id: Option<String>,
    pub active_turn_id: Option<String>,
    pub pending_turn_prompt: Option<String>,
    pub title: String,
    pub created_at: String,
    pub doc_uid: u64,
    pub page_id: NodeId,
    pub target_name: String,
    pub messages: Vec<AiMessage>,
    pub pending_approval: Option<PendingApproval>,
    pub status: TurnStatus,
    pub status_message: Option<String>,
    pub retained_edits: Vec<NodeId>,
    #[serde(skip)]
    pub task: Option<figcraft_engine::TaskAuthority>,
}

impl AiSession {
    pub fn new(session_id: impl Into<String>, doc_uid: u64, page_id: NodeId, target_name: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            thread_id: None,
            active_turn_id: None,
            pending_turn_prompt: None,
            title: "New Conversation".into(),
            created_at: "Just now".into(),
            doc_uid,
            page_id,
            target_name: target_name.into(),
            messages: Vec::new(),
            pending_approval: None,
            status: TurnStatus::Idle,
            status_message: None,
            retained_edits: Vec::new(),
            task: None,
        }
    }

    pub fn append_user_message(&mut self, text: &str, mentions: Vec<ObjectMention>) {
        if self.messages.is_empty() {
            // First message sets title
            let first_line = text.lines().next().unwrap_or(text);
            let truncated: String = first_line.chars().take(28).collect();
            self.title = if first_line.len() > 28 { format!("{truncated}…") } else { truncated };
        }
        self.messages.push(AiMessage::user(text, mentions));
    }

    pub fn append_assistant_delta(&mut self, delta: &str) {
        if let Some(last) = self.messages.last_mut().filter(|m| m.role == MessageRole::Assistant) {
            last.text.push_str(delta);
            return;
        }
        self.messages.push(AiMessage::assistant(delta));
    }

    pub fn append_assistant_message(&mut self, text: &str) {
        self.messages.push(AiMessage::assistant(text));
    }

    pub fn ensure_assistant_turn(&mut self) {
        if !matches!(self.messages.last().map(|m| m.role), Some(MessageRole::Assistant)) {
            self.messages.push(AiMessage::assistant(""));
        }
    }

    pub fn append_tool_call(&mut self, tool_id: &str, tool_name: &str, params: Value) {
        self.messages.push(AiMessage::tool(tool_id, tool_name, params));
    }

    pub fn update_tool_call(&mut self, tool_id: &str, status: ToolStatus, summary: Option<String>) {
        for msg in self.messages.iter_mut().rev() {
            if let Some(tool) = msg.tool_call.as_mut().filter(|t| t.tool_id == tool_id) {
                tool.status = status;
                tool.result_summary = summary;
                break;
            }
        }
    }
}
