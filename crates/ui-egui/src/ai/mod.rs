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
pub use supervisor::{CodexSupervisor, HarnessState, ModelInfo};
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
    pub model_infos: Vec<ModelInfo>,
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

        let default_models = vec![
            ModelInfo {
                id: "gpt-5.6-sol".to_string(),
                display_name: Some("GPT-5.6 Sol".to_string()),
                supported_efforts: vec!["low".into(), "medium".into(), "high".into()],
                default_effort: Some("medium".into()),
            },
            ModelInfo {
                id: "gpt-6.1-sol".to_string(),
                display_name: Some("GPT-6.1 Sol".to_string()),
                supported_efforts: vec!["low".into(), "medium".into(), "high".into(), "xhigh".into(), "max".into()],
                default_effort: Some("medium".into()),
            },
        ];

        Self {
            sessions: vec![initial_session],
            active_session_idx: 0,
            draft_text: String::new(),
            selected_model: "gpt-5.6-sol".to_string(),
            selected_effort: Some("medium".to_string()),
            available_models: vec!["gpt-5.6-sol".to_string(), "gpt-6.1-sol".to_string()],
            model_infos: default_models,
            harness_state,
            subview: AiPanelSubview::Conversation,
            is_running: false,
            supervisor,
        }
    }

    pub fn with_supervisor(supervisor: CodexSupervisor) -> Self {
        let mut state = Self::new();
        state.harness_state = supervisor.harness_state.clone();
        if !supervisor.model_infos.is_empty() {
            state.set_models(supervisor.model_infos.clone());
        } else if !supervisor.available_models.is_empty() {
            state.available_models = supervisor.available_models.clone();
            if let Some(first) = state.available_models.first() {
                state.selected_model = first.clone();
            }
        }
        state.supervisor = Some(supervisor);
        state
    }

    pub fn set_models(&mut self, models: Vec<ModelInfo>) {
        self.model_infos = models.clone();
        self.available_models = models.iter().map(|m| m.id.clone()).collect();
        if !self.available_models.contains(&self.selected_model) {
            if let Some(first) = self.available_models.first().cloned() {
                self.select_model(&first);
            }
        } else {
            self.sync_effort_to_selected_model();
        }
    }

    pub fn select_model(&mut self, model_id: &str) {
        self.selected_model = model_id.to_string();
        self.sync_effort_to_selected_model();
    }

    pub fn sync_effort_to_selected_model(&mut self) {
        if let Some(info) = self.model_infos.iter().find(|m| m.id == self.selected_model) {
            if info.supported_efforts.is_empty() {
                self.selected_effort = None;
            } else if let Some(current) = &self.selected_effort {
                if !info.supported_efforts.contains(current) {
                    self.selected_effort = info.default_effort.clone().or_else(|| info.supported_efforts.first().cloned());
                }
            } else {
                self.selected_effort = info.default_effort.clone().or_else(|| info.supported_efforts.first().cloned());
            }
        }
    }

    pub fn select_effort(&mut self, effort: Option<String>) {
        if let Some(eff) = &effort {
            if let Some(info) = self.model_infos.iter().find(|m| m.id == self.selected_model) {
                if info.supported_efforts.contains(eff) {
                    self.selected_effort = Some(eff.clone());
                } else if let Some(def) = &info.default_effort {
                    self.selected_effort = Some(def.clone());
                }
            } else {
                self.selected_effort = Some(eff.clone());
            }
        } else {
            self.selected_effort = None;
        }
    }

    pub fn supported_efforts_for_selected_model(&self) -> Vec<String> {
        self.model_infos
            .iter()
            .find(|m| m.id == self.selected_model)
            .map(|m| m.supported_efforts.clone())
            .unwrap_or_else(|| vec!["low".into(), "medium".into(), "high".into()])
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

        let (doc_uid, page_id, revision) = session.ds().map(|ds| (ds.uid, ds.page, ds.revision)).unwrap_or((0, figcraft_doc::NodeId(1), 0));

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
        let task = figcraft_engine::TaskAuthority::new_with_revision(task_id, doc_uid, revision, scope);

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
        dispatch_ai_event(app, ev);
    }
}

fn dispatch_ai_event(app: &mut FigApp, ev: protocol::ProtocolEvent) {
    match ev {
        protocol::ProtocolEvent::Initialized { models, .. } => {
            handle_initialized(app, models);
        }
        protocol::ProtocolEvent::ThreadStarted { thread_id } => {
            handle_thread_started(app, thread_id);
        }
        protocol::ProtocolEvent::TurnStarted { thread_id, turn_id } => {
            handle_turn_started(app, thread_id.as_deref(), turn_id);
        }
        protocol::ProtocolEvent::AgentDelta { thread_id, turn_id, text } => {
            handle_agent_delta(app, thread_id.as_deref(), turn_id.as_deref(), &text);
        }
        protocol::ProtocolEvent::ItemStarted { thread_id, item_id, tool_name, .. } => {
            handle_item_started(app, thread_id.as_deref(), item_id, tool_name);
        }
        protocol::ProtocolEvent::ItemCompleted { thread_id, item_id, is_error, summary, .. } => {
            handle_item_completed(app, thread_id.as_deref(), item_id, is_error, summary);
        }
        protocol::ProtocolEvent::ApprovalRequired { thread_id, turn_id, request_id, action, target } => {
            handle_approval_required(app, thread_id, turn_id, request_id, action, target);
        }
        protocol::ProtocolEvent::ToolCall { thread_id, turn_id, call_id, request_id, name, params } => {
            handle_tool_call(app, thread_id, turn_id, call_id, request_id, name, params);
        }
        protocol::ProtocolEvent::TurnCompleted { thread_id, .. } => {
            handle_turn_completed(app, thread_id.as_deref());
        }
        protocol::ProtocolEvent::Error { thread_id, message } => {
            handle_error_event(app, thread_id.as_deref(), message);
        }
    }
}

fn handle_initialized(app: &mut FigApp, models: Vec<String>) {
    if let Some(sup) = app.ui.ai.supervisor.as_ref()
        && !sup.model_infos.is_empty()
    {
        app.ui.ai.set_models(sup.model_infos.clone());
        app.ui.ai.harness_state = HarnessState::Ready;
        return;
    }
    if !models.is_empty() {
        let infos: Vec<_> = models
            .into_iter()
            .map(|id| ModelInfo {
                id,
                display_name: None,
                supported_efforts: vec!["low".into(), "medium".into(), "high".into()],
                default_effort: Some("medium".into()),
            })
            .collect();
        app.ui.ai.set_models(infos);
    }
    app.ui.ai.harness_state = HarnessState::Ready;
}

fn handle_thread_started(app: &mut FigApp, thread_id: String) {
    let session_idx = app
        .ui
        .ai
        .sessions
        .iter()
        .position(|s| s.pending_turn_prompt.is_some())
        .or_else(|| app.ui.ai.sessions.iter().position(|s| s.thread_id.is_none()));

    let Some(session_idx) = session_idx else { return };
    let Some(sess) = app.ui.ai.sessions.get_mut(session_idx) else { return };
    sess.thread_id = Some(thread_id.clone());
    let pending_prompt = sess.pending_turn_prompt.take();

    if let (Some(prompt), Some(sup)) = (pending_prompt, app.ui.ai.supervisor.as_mut()) {
        let sess_id = app.ui.ai.sessions.get(session_idx).map(|s| s.session_id.clone()).unwrap_or_default();
        sup.start_turn(&sess_id, &thread_id, &app.ui.ai.selected_model, &prompt, app.ui.ai.selected_effort.as_deref());
    }
}

fn handle_turn_started(app: &mut FigApp, thread_id: Option<&str>, turn_id: String) {
    let Some(sess_idx) = find_session_by_thread(&app.ui.ai.sessions, thread_id) else { return };
    let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) else { return };

    if sess.status == TurnStatus::Stopped {
        if let (Some(sup), Some(tid)) = (app.ui.ai.supervisor.as_mut(), thread_id) {
            sup.interrupt_turn(tid, &turn_id);
        }
    } else {
        app.ui.ai.is_running = true;
        sess.active_turn_id = Some(turn_id);
        sess.status = TurnStatus::Running;
        sess.ensure_assistant_turn();
    }
}

fn handle_agent_delta(app: &mut FigApp, thread_id: Option<&str>, turn_id: Option<&str>, text: &str) {
    let Some(sess_idx) = find_session_by_thread(&app.ui.ai.sessions, thread_id) else { return };
    let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) else { return };
    if sess.status != TurnStatus::Running {
        return;
    }
    if turn_id.is_some() && sess.active_turn_id.as_deref() != turn_id {
        return;
    }
    sess.append_assistant_delta(text);
}

fn handle_item_started(app: &mut FigApp, thread_id: Option<&str>, item_id: String, tool_name: Option<String>) {
    let Some(sess_idx) = find_session_by_thread(&app.ui.ai.sessions, thread_id) else { return };
    let Some(name) = tool_name else { return };
    let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) else { return };
    sess.append_tool_call(&item_id, &name, Value::Null);
}

fn handle_item_completed(app: &mut FigApp, thread_id: Option<&str>, item_id: String, is_error: bool, summary: Option<String>) {
    let Some(sess_idx) = find_session_by_thread(&app.ui.ai.sessions, thread_id) else { return };
    let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) else { return };
    let status = if is_error { ToolStatus::Failed } else { ToolStatus::Succeeded };
    sess.update_tool_call(&item_id, status, summary);
}

fn handle_approval_required(app: &mut FigApp, thread_id: Option<String>, turn_id: Option<String>, request_id: Value, action: String, target: String) {
    let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref());
    if let Some(sess_idx) = sess_idx {
        let sess = &mut app.ui.ai.sessions[sess_idx];
        if turn_id.is_none() || sess.active_turn_id.as_ref() == turn_id.as_ref() {
            sess.pending_approval = Some(PendingApproval { thread_id, turn_id, request_id, action, target });
            sess.status = TurnStatus::AwaitingApproval;
        } else if let Some(sup) = app.ui.ai.supervisor.as_mut() {
            sup.record_and_send_response(&request_id, json!({ "decision": "decline", "denied": { "rejection": "Stale turn approval rejected" } }));
        }
    } else if let Some(sup) = app.ui.ai.supervisor.as_mut() {
        sup.record_and_send_response(&request_id, json!({ "decision": "decline", "denied": { "rejection": "Unknown thread approval rejected" } }));
    }
}

fn handle_tool_call(
    app: &mut FigApp,
    thread_id: Option<String>,
    turn_id: Option<String>,
    call_id: String,
    request_id: Value,
    name: String,
    params: Value,
) {
    let sess_idx = find_session_by_thread(&app.ui.ai.sessions, thread_id.as_deref());
    let Some(sess_idx) = sess_idx else {
        if let Some(sup) = app.ui.ai.supervisor.as_mut() {
            sup.send_response(
                &request_id,
                json!({
                    "success": false,
                    "contentItems": [{
                        "type": "inputText",
                        "text": "Stale or unknown thread rejected: no mutations admitted"
                    }]
                }),
            );
        }
        return;
    };

    let sess = &app.ui.ai.sessions[sess_idx];
    if turn_id.is_some() && sess.active_turn_id.as_ref() != turn_id.as_ref() {
        if let Some(sup) = app.ui.ai.supervisor.as_mut() {
            sup.send_response(
                &request_id,
                json!({
                    "success": false,
                    "contentItems": [{
                        "type": "inputText",
                        "text": "Stale turn rejected: no mutations admitted"
                    }]
                }),
            );
        }
        return;
    }

    let is_stopped = sess.status == TurnStatus::Stopped || sess.task.as_ref().map(|t| t.is_stopped()).unwrap_or(true);
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
        return;
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

fn handle_turn_completed(app: &mut FigApp, thread_id: Option<&str>) {
    if let Some(sess_idx) = find_session_by_thread(&app.ui.ai.sessions, thread_id)
        && let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx)
    {
        if sess.status != TurnStatus::Stopped {
            sess.status = TurnStatus::Completed;
        }
        sess.active_turn_id = None;
    }
    app.ui.ai.is_running = app.ui.ai.sessions.iter().any(|s| s.status == TurnStatus::Running);
}

fn handle_error_event(app: &mut FigApp, thread_id: Option<&str>, message: String) {
    if let Some(sess_idx) = find_session_by_thread(&app.ui.ai.sessions, thread_id) {
        if let Some(sess) = app.ui.ai.sessions.get_mut(sess_idx) {
            sess.append_assistant_message(&format!("Error: {message}"));
            sess.status = TurnStatus::Failed;
            sess.active_turn_id = None;
        }
    } else {
        // Global process failure or unassigned harness error
        app.ui.ai.harness_state = HarnessState::Unavailable(message);
    }
    app.ui.ai.is_running = app.ui.ai.sessions.iter().any(|s| s.status == TurnStatus::Running);
}

pub(crate) fn find_session_by_thread(sessions: &[AiSession], thread_id: Option<&str>) -> Option<usize> {
    let tid = thread_id?;
    sessions.iter().position(|s| s.thread_id.as_deref() == Some(tid))
}
