//! AI Chat and Codex live canvas integration for FigCraft.

pub mod composer;
pub mod panel;
pub mod protocol;
pub mod sessions;
pub mod supervisor;

#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod tests;

pub use panel::AiPanelSubview;
pub use sessions::{AiMessage, AiSession, DraftAttachment, MessageRole, PendingApproval, ToolCallInfo, ToolStatus, TurnStatus};
pub use supervisor::{CodexSupervisor, HarnessState};

use egui::Context;
use serde_json::json;

use crate::FigApp;

/// AI Chat and composer state maintained across panels and turns.
pub struct AiState {
    pub sessions: Vec<AiSession>,
    pub active_session_idx: usize,
    pub draft_text: String,
    pub selected_model: String,
    pub available_models: Vec<String>,
    pub harness_state: HarnessState,
    pub subview: AiPanelSubview,
    pub is_running: bool,
    pub active_task: Option<figcraft_engine::TaskAuthority>,
    pub supervisor: Option<CodexSupervisor>,
}

impl Default for AiState {
    fn default() -> Self {
        Self::new()
    }
}

impl AiState {
    pub fn new() -> Self {
        let (supervisor, harness_state) = match CodexSupervisor::try_spawn_local() {
            Ok(sup) => (Some(sup), HarnessState::Checking),
            Err(state) => (None, state),
        };

        let initial_session = AiSession::new("sess_1", 0, figcraft_doc::NodeId(1), "Canvas (New Root)");

        Self {
            sessions: vec![initial_session],
            active_session_idx: 0,
            draft_text: String::new(),
            selected_model: "gpt-5.6-sol".to_string(),
            available_models: vec!["gpt-5.6-sol".to_string(), "gpt-6.1-sol".to_string()],
            harness_state,
            subview: AiPanelSubview::Conversation,
            is_running: false,
            active_task: None,
            supervisor,
        }
    }

    pub fn with_supervisor(supervisor: CodexSupervisor) -> Self {
        let mut state = Self::new();
        state.harness_state = supervisor.harness_state.clone();
        state.available_models = supervisor.available_models.clone();
        if let Some(first) = state.available_models.first() {
            state.selected_model = first.clone();
        }
        state.supervisor = Some(supervisor);
        state
    }

    pub fn active_session(&self) -> Option<&AiSession> {
        self.sessions.get(self.active_session_idx)
    }

    pub fn active_session_mut(&mut self) -> Option<&mut AiSession> {
        self.sessions.get_mut(self.active_session_idx)
    }

    pub fn create_new_session(&mut self, session: &figcraft_engine::Session) {
        let num = self.sessions.len() + 1;
        let id = format!("sess_{num}");
        let (uid, page) = session.ds().map(|ds| (ds.uid, ds.page)).unwrap_or((0, figcraft_doc::NodeId(1)));
        let target = self.current_target_description(session);
        let mut new_sess = AiSession::new(id, uid, page, target);
        new_sess.title = format!("Session {num}");
        self.sessions.push(new_sess);
        self.active_session_idx = self.sessions.len().saturating_sub(1);
    }

    pub fn current_target_description(&self, session: &figcraft_engine::Session) -> String {
        if let Ok(ds) = session.ds() {
            if ds.selection.is_empty() {
                "Canvas (New Root)".to_string()
            } else if ds.selection.len() == 1 {
                if let Some(node_id) = ds.selection.first() {
                    if let Some(node) = ds.doc.node(*node_id) { format!("Subtree: {}", node.name) } else { "Subtree: 1 layer".to_string() }
                } else {
                    "Subtree: 1 layer".to_string()
                }
            } else {
                format!("Subtree: {} layers", ds.selection.len())
            }
        } else {
            "Canvas (New Root)".to_string()
        }
    }

    pub fn submit_turn(&mut self, session: &mut figcraft_engine::Session) {
        let text = self.draft_text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.draft_text.clear();

        if self.sessions.is_empty() {
            self.create_new_session(session);
        }

        let (doc_uid, page_id) = session.ds().map(|ds| (ds.uid, ds.page)).unwrap_or((0, figcraft_doc::NodeId(1)));

        let scope = if let Ok(ds) = session.ds() {
            if ds.selection.len() == 1 {
                if let Some(id) = ds.selection.first() {
                    figcraft_engine::TaskScope::Subtree { page_id, target_node: *id }
                } else {
                    figcraft_engine::TaskScope::NewRoot { page_id, task_root: None }
                }
            } else {
                figcraft_engine::TaskScope::NewRoot { page_id, task_root: None }
            }
        } else {
            figcraft_engine::TaskScope::NewRoot { page_id, task_root: None }
        };

        let task = figcraft_engine::TaskAuthority::new(format!("task_{}", self.active_session_idx), doc_uid, scope);

        if let Some(s) = self.sessions.get_mut(self.active_session_idx) {
            s.append_user_message(&text);
            s.status = TurnStatus::Running;
        }

        if let Some(sup) = self.supervisor.as_mut() {
            self.is_running = true;
            let thread_id = format!("thread-{}", self.active_session_idx);
            sup.start_turn(&thread_id, &text);
        } else {
            if let Some(s) = self.sessions.get_mut(self.active_session_idx) {
                let msg = match &self.harness_state {
                    HarnessState::Unavailable(reason) => format!("AI harness unavailable: {reason}"),
                    HarnessState::MissingExecutable(reason) => format!("Codex CLI missing: {reason}"),
                    HarnessState::Unauthenticated(reason) => format!("Codex unauthenticated: {reason}"),
                    _ => "AI harness is not ready.".to_string(),
                };
                s.append_assistant_message(&msg);
                s.status = TurnStatus::Failed;
            }
        }

        self.active_task = Some(task);
    }

    pub fn stop_active_task(&mut self) {
        self.is_running = false;
        if let Some(auth) = self.active_task.as_mut() {
            auth.stop();
        }
        if let Some(sup) = self.supervisor.as_mut() {
            sup.interrupt(Some(&format!("thread-{}", self.active_session_idx)));
        }
        if let Some(s) = self.sessions.get_mut(self.active_session_idx) {
            s.status = TurnStatus::Stopped;
        }
    }
}

/// Poll supervisor events, execute authoritative mutations, and update egui state.
pub fn update_ai_runtime(app: &mut FigApp, ctx: &Context) {
    let events = if let Some(sup) = app.ui.ai.supervisor.as_mut() { sup.poll_events() } else { Vec::new() };

    if events.is_empty() {
        return;
    }

    ctx.request_repaint();

    for ev in events {
        match ev {
            protocol::ProtocolEvent::Initialized { models, account_email: _, plan_type: _ } => {
                if !models.is_empty() {
                    app.ui.ai.available_models = models.clone();
                    if let (true, Some(first)) = (
                        app.ui.ai.selected_model.is_empty() || !app.ui.ai.available_models.contains(&app.ui.ai.selected_model),
                        app.ui.ai.available_models.first(),
                    ) {
                        app.ui.ai.selected_model = first.clone();
                    }
                }
                app.ui.ai.harness_state = HarnessState::Ready;
            }
            protocol::ProtocolEvent::TurnStarted { .. } => {
                app.ui.ai.is_running = true;
                let active_idx = app.ui.ai.active_session_idx;
                if let Some(sess) = app.ui.ai.sessions.get_mut(active_idx) {
                    sess.status = TurnStatus::Running;
                    sess.ensure_assistant_turn();
                }
            }
            protocol::ProtocolEvent::AgentDelta { text } => {
                let active_idx = app.ui.ai.active_session_idx;
                if let Some(sess) = app.ui.ai.sessions.get_mut(active_idx) {
                    sess.append_assistant_delta(&text);
                }
            }
            protocol::ProtocolEvent::ItemStarted { item_id, tool_name } => {
                let active_idx = app.ui.ai.active_session_idx;
                if let (Some(sess), Some(name)) = (app.ui.ai.sessions.get_mut(active_idx), tool_name) {
                    sess.append_tool_call(&item_id, &name, serde_json::Value::Null);
                }
            }
            protocol::ProtocolEvent::ItemCompleted { item_id, is_error, summary } => {
                let active_idx = app.ui.ai.active_session_idx;
                if let Some(sess) = app.ui.ai.sessions.get_mut(active_idx) {
                    let status = if is_error { ToolStatus::Failed } else { ToolStatus::Succeeded };
                    sess.update_tool_call(&item_id, status, summary);
                }
            }
            protocol::ProtocolEvent::ApprovalRequired { request_id, action, target } => {
                let active_idx = app.ui.ai.active_session_idx;
                if let Some(sess) = app.ui.ai.sessions.get_mut(active_idx) {
                    sess.pending_approval = Some(PendingApproval { request_id, action, target });
                    sess.status = TurnStatus::AwaitingApproval;
                }
            }
            protocol::ProtocolEvent::ToolCall { request_id, name, params } => {
                let (status, summary, result_val) = if let Some(auth) = app.ui.ai.active_task.as_mut() {
                    match auth.execute(&mut app.session, &name, &params) {
                        Ok(res) => (ToolStatus::Succeeded, Some(format!("Applied {name}")), json!({ "status": "ok", "result": res })),
                        Err(err) => (ToolStatus::Failed, Some(format!("Error: {err}")), json!({ "status": "error", "error": err.to_string() })),
                    }
                } else {
                    (ToolStatus::Failed, Some("TaskAuthority inactive".into()), json!({ "status": "error", "error": "TaskAuthority inactive" }))
                };

                let active_idx = app.ui.ai.active_session_idx;
                if let Some(sess) = app.ui.ai.sessions.get_mut(active_idx) {
                    let call_id = request_id.as_str().unwrap_or("tool_call").to_string();
                    sess.append_tool_call(&call_id, &name, params);
                    sess.update_tool_call(&call_id, status, summary);
                }

                if let Some(sup) = app.ui.ai.supervisor.as_mut() {
                    sup.send_response(&request_id, result_val);
                }
            }
            protocol::ProtocolEvent::TurnCompleted { .. } => {
                app.ui.ai.is_running = false;
                let active_idx = app.ui.ai.active_session_idx;
                if let Some(sess) = app.ui.ai.sessions.get_mut(active_idx) {
                    sess.status = TurnStatus::Completed;
                }
            }
            protocol::ProtocolEvent::Error { message } => {
                app.ui.ai.is_running = false;
                let active_idx = app.ui.ai.active_session_idx;
                if let Some(sess) = app.ui.ai.sessions.get_mut(active_idx) {
                    sess.append_assistant_message(&format!("Error: {message}"));
                    sess.status = TurnStatus::Failed;
                }
            }
        }
    }
}
