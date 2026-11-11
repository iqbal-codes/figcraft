//! AI Chat and Codex live canvas integration for FigCraft.

pub mod composer;
pub mod panel;
pub mod protocol;
pub mod sessions;
pub mod supervisor;
pub mod tools;

#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod tests;

pub use panel::AiPanelSubview;
pub use sessions::{AiMessage, AiSession, DraftAttachment, MessageRole, PendingApproval, ToolCallInfo, ToolStatus, TurnStatus};
pub use supervisor::{CodexSupervisor, HarnessState};
pub use tools::{canvas_tool_specs, execute_tool};

use egui::Context;
use serde_json::{Value, json};

use crate::FigApp;

/// AI Chat and composer state maintained across panels and turns.
pub struct AiState {
    pub sessions: Vec<AiSession>,
    pub active_session_idx: usize,
    pub draft_text: String,
    pub selected_model: String,
    pub selected_effort: Option<String>,
    pub available_models: Vec<String>,
    pub harness_state: HarnessState,
    pub subview: AiPanelSubview,
    pub is_running: bool,
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
            selected_effort: Some("medium".to_string()),
            available_models: vec!["gpt-5.6-sol".to_string(), "gpt-6.1-sol".to_string()],
            harness_state,
            subview: AiPanelSubview::Conversation,
            is_running: false,
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

    pub fn active_task(&self) -> Option<&figcraft_engine::TaskAuthority> {
        self.active_session().and_then(|s| s.task.as_ref())
    }

    pub fn active_task_mut(&mut self) -> Option<&mut figcraft_engine::TaskAuthority> {
        self.active_session_mut().and_then(|s| s.task.as_mut())
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

        let task_id = format!("task_{}", self.active_session_idx);
        let task = figcraft_engine::TaskAuthority::new(task_id, doc_uid, scope);

        let active_idx = self.active_session_idx;
        if let Some(s) = self.sessions.get_mut(active_idx) {
            s.append_user_message(&text);
            s.status = TurnStatus::Running;
            s.task = Some(task);
        }

        if let Some(sup) = self.supervisor.as_mut() {
            self.is_running = true;
            if let Some(sess) = self.sessions.get_mut(active_idx) {
                if let Some(ref tid) = sess.thread_id {
                    sup.start_turn(&sess.session_id, tid, &self.selected_model, &text, self.selected_effort.as_deref());
                } else {
                    sess.pending_turn_prompt = Some(text);
                    sup.start_thread(&sess.session_id, &self.selected_model, Some("on-request"), None);
                }
            }
        } else {
            if let Some(s) = self.sessions.get_mut(active_idx) {
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
    }

    pub fn stop_active_task(&mut self) {
        self.is_running = false;
        if let Some(sess) = self.sessions.get_mut(self.active_session_idx) {
            sess.pending_turn_prompt = None;
            if let Some(auth) = sess.task.as_mut() {
                auth.stop();
            }
            if let (Some(sup), Some(tid), Some(turn_id)) = (self.supervisor.as_mut(), &sess.thread_id, &sess.active_turn_id) {
                sup.interrupt_turn(tid, turn_id);
            }
            sess.status = TurnStatus::Stopped;
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
            protocol::ProtocolEvent::ThreadStarted { thread_id } => {
                // Associate thread_id to the session that initiated thread/start
                let session_idx = app
                    .ui
                    .ai
                    .sessions
                    .iter()
                    .position(|s| s.pending_turn_prompt.is_some() || s.thread_id.is_none())
                    .unwrap_or(app.ui.ai.active_session_idx);

                let pending_prompt = if let Some(sess) = app.ui.ai.sessions.get_mut(session_idx) {
                    sess.thread_id = Some(thread_id.clone());
                    sess.pending_turn_prompt.take()
                } else {
                    None
                };

                if let (Some(prompt), Some(sup)) = (pending_prompt, app.ui.ai.supervisor.as_mut()) {
                    let sess_id = app.ui.ai.sessions.get(session_idx).map(|s| s.session_id.clone()).unwrap_or_default();
                    sup.start_turn(&sess_id, &thread_id, &app.ui.ai.selected_model, &prompt, app.ui.ai.selected_effort.as_deref());
                }
            }
            protocol::ProtocolEvent::TurnStarted { thread_id, turn_id } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);
                if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
                    if sess.status == TurnStatus::Stopped {
                        if let (Some(sup), Some(tid)) = (app.ui.ai.supervisor.as_mut(), thread_id.as_deref()) {
                            sup.interrupt_turn(tid, &turn_id);
                        }
                    } else {
                        app.ui.ai.is_running = true;
                        sess.active_turn_id = Some(turn_id);
                        sess.status = TurnStatus::Running;
                        sess.ensure_assistant_turn();
                    }
                }
            }
            protocol::ProtocolEvent::AgentDelta { thread_id, turn_id: _, text } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);
                if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx).filter(|s| s.status == TurnStatus::Running) {
                    sess.append_assistant_delta(&text);
                }
            }
            protocol::ProtocolEvent::ItemStarted { thread_id, turn_id: _, item_id, tool_name } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);
                if let (Some(sess), Some(name)) = (app.ui.ai.sessions.get_mut(sess_idx), tool_name) {
                    sess.append_tool_call(&item_id, &name, Value::Null);
                }
            }
            protocol::ProtocolEvent::ItemCompleted { thread_id, turn_id: _, item_id, is_error, summary } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);
                if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
                    let status = if is_error { ToolStatus::Failed } else { ToolStatus::Succeeded };
                    sess.update_tool_call(&item_id, status, summary);
                }
            }
            protocol::ProtocolEvent::ApprovalRequired { thread_id, turn_id, request_id, action, target } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);
                if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
                    sess.pending_approval = Some(PendingApproval { thread_id, turn_id, request_id, action, target });
                    sess.status = TurnStatus::AwaitingApproval;
                }
            }
            protocol::ProtocolEvent::ToolCall { thread_id, turn_id: _, call_id, request_id, name, params } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);

                let is_stopped = if let Some(sess) = app.ui.ai.sessions.get(sess_idx) {
                    sess.status == TurnStatus::Stopped || sess.task.as_ref().map(|t| t.is_stopped()).unwrap_or(true)
                } else {
                    true
                };

                if is_stopped {
                    if let Some(sup) = app.ui.ai.supervisor.as_mut() {
                        sup.send_response(
                            &request_id,
                            json!({
                                "success": false,
                                "contentItems": [{
                                    "type": "inputText",
                                    "text": "Task was stopped: no further mutations admitted"
                                }]
                            }),
                        );
                    }
                    continue;
                }

                let (status, summary, resp_val) = if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
                    if let Some(auth) = sess.task.as_mut() {
                        match tools::execute_tool(&mut app.session, auth, &name, &params) {
                            Ok(res) => {
                                let res_str = serde_json::to_string(&res).unwrap_or_else(|_| "{}".to_string());
                                (
                                    ToolStatus::Succeeded,
                                    Some(format!("Applied {name}")),
                                    json!({
                                        "success": true,
                                        "contentItems": [{
                                            "type": "inputText",
                                            "text": res_str
                                        }]
                                    }),
                                )
                            }
                            Err(err) => {
                                let err_str = format!("Tool error: {err}");
                                (
                                    ToolStatus::Failed,
                                    Some(err_str.clone()),
                                    json!({
                                        "success": false,
                                        "contentItems": [{
                                            "type": "inputText",
                                            "text": err_str
                                        }]
                                    }),
                                )
                            }
                        }
                    } else {
                        (
                            ToolStatus::Failed,
                            Some("TaskAuthority inactive".into()),
                            json!({
                                "success": false,
                                "contentItems": [{
                                    "type": "inputText",
                                    "text": "TaskAuthority inactive"
                                }]
                            }),
                        )
                    }
                } else {
                    (
                        ToolStatus::Failed,
                        Some("Session not found".into()),
                        json!({
                            "success": false,
                            "contentItems": [{
                                "type": "inputText",
                                "text": "Session not found"
                            }]
                        }),
                    )
                };

                if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
                    let effective_call_id = if call_id.is_empty() { request_id.as_str().unwrap_or("tool_call").to_string() } else { call_id };
                    sess.append_tool_call(&effective_call_id, &name, params);
                    sess.update_tool_call(&effective_call_id, status, summary);
                }

                if let Some(sup) = app.ui.ai.supervisor.as_mut() {
                    sup.send_response(&request_id, resp_val);
                }
            }
            protocol::ProtocolEvent::TurnCompleted { thread_id, turn_id: _ } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);
                if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
                    if sess.status != TurnStatus::Stopped {
                        sess.status = TurnStatus::Completed;
                    }
                    sess.active_turn_id = None;
                }
                app.ui.ai.is_running = false;
            }
            protocol::ProtocolEvent::Error { thread_id, message } => {
                let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref(), app.ui.ai.active_session_idx);
                if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
                    sess.append_assistant_message(&format!("Error: {message}"));
                    sess.status = TurnStatus::Failed;
                    sess.active_turn_id = None;
                }
                app.ui.ai.is_running = false;
            }
        }
    }
}

fn find_session_by_thread(sessions: &[AiSession], thread_id: Option<&str>, default_idx: usize) -> usize {
    if let Some(tid) = thread_id { sessions.iter().position(|s| s.thread_id.as_deref() == Some(tid)).unwrap_or(default_idx) } else { default_idx }
}
