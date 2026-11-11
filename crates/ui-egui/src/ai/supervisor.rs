//! Codex harness supervisor managing the subprocess stdio loop and event channels.

use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use serde_json::{Value, json};

#[cfg(not(target_arch = "wasm32"))]
use super::protocol::MAX_MESSAGE_SIZE;
use super::protocol::{
    ClientInfo, ProtocolEvent, ServerMessage, format_notification, format_request, format_response, message_to_event, parse_message_line,
};

/// Readiness state of the local AI harness.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum HarnessState {
    Ready,
    #[default]
    Checking,
    Unauthenticated(String),
    MissingExecutable(String),
    Unavailable(String),
}

/// Lifecycle phases of the supervisor protocol handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorPhase {
    Uninitialized,
    Initializing { init_req_id: u64 },
    Initialized,
    Discovering { model_req_id: Option<u64>, account_req_id: Option<u64> },
    Ready,
    Failed,
}

/// In-flight request correlation tracking.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestCorrelation {
    Initialize,
    ModelList,
    AccountRead,
    ThreadStart { session_id: String },
    TurnStart { thread_id: String, session_id: String },
    TurnInterrupt { thread_id: String, turn_id: String },
}

/// Abstract transport to allow real stdio subprocess or controlled test fixtures.
pub trait SupervisorTransport: Send {
    fn send_line(&mut self, line: &str) -> Result<(), String>;
    fn poll_lines(&mut self) -> Vec<String>;
    fn interrupt(&mut self);
    fn is_alive(&mut self) -> bool;
}

/// Supervisor driving Codex app-server over stdio.
pub struct CodexSupervisor {
    transport: Box<dyn SupervisorTransport>,
    next_id: u64,
    pub phase: SupervisorPhase,
    pub pending_requests: BTreeMap<u64, RequestCorrelation>,
    pub available_models: Vec<String>,
    pub account_email: Option<String>,
    pub plan_type: Option<String>,
    pub harness_state: HarnessState,
}

impl CodexSupervisor {
    pub fn try_spawn_local() -> Result<Self, HarnessState> {
        #[cfg(target_arch = "wasm32")]
        {
            Err(HarnessState::Unavailable("Local AI harness (Codex) is not available in web browser build.".into()))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(exe) = detect_codex_cli() {
                match SubprocessTransport::spawn(&exe) {
                    Ok(transport) => Ok(Self::new(Box::new(transport))),
                    Err(e) => Err(HarnessState::MissingExecutable(format!("Failed to spawn Codex: {e}"))),
                }
            } else {
                Err(HarnessState::MissingExecutable("Codex executable not found in PATH or standard locations.".into()))
            }
        }
    }

    pub fn new(transport: Box<dyn SupervisorTransport>) -> Self {
        let mut s = Self {
            transport,
            next_id: 1,
            phase: SupervisorPhase::Uninitialized,
            pending_requests: BTreeMap::new(),
            available_models: Vec::new(),
            account_email: None,
            plan_type: None,
            harness_state: HarnessState::Checking,
        };
        s.start_initialization();
        s
    }

    /// Step 1 of handshake: Send `initialize` request with client info and capabilities.
    pub fn start_initialization(&mut self) {
        let client_info = ClientInfo::default();
        let req_id = self.send_request(
            "initialize",
            json!({
                "clientInfo": client_info,
                "capabilities": {
                    "experimentalApi": true
                }
            }),
        );
        self.pending_requests.insert(req_id, RequestCorrelation::Initialize);
        self.phase = SupervisorPhase::Initializing { init_req_id: req_id };
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let line = format_request(id, method, params);
        let _ = self.transport.send_line(&line);
        id
    }

    pub fn send_notification(&mut self, method: &str, params: Value) {
        let line = format_notification(method, params);
        let _ = self.transport.send_line(&line);
    }

    pub fn send_response(&mut self, request_id: &Value, result: Value) {
        let line = format_response(request_id, result);
        let _ = self.transport.send_line(&line);
    }

    /// Start a new thread for an AI session with model, instructions, and dynamic design tools.
    pub fn start_thread(&mut self, session_id: &str, model: &str, approval_policy: Option<&str>, instructions: Option<&str>) -> u64 {
        let tools = super::tools::canvas_tool_specs();
        let req_id = self.send_request(
            "thread/start",
            json!({
                "model": model,
                "approvalPolicy": approval_policy.unwrap_or("on-request"),
                "baseInstructions": instructions.unwrap_or("You are an expert design assistant in FigCraft. Use the provided canvas tools to inspect and mutate the design strictly within authorized scope."),
                "dynamicTools": tools,
            }),
        );
        self.pending_requests.insert(req_id, RequestCorrelation::ThreadStart { session_id: session_id.to_string() });
        req_id
    }

    /// Start a new turn bound to a specific thread and model.
    pub fn start_turn(&mut self, session_id: &str, thread_id: &str, model: &str, prompt: &str, effort: Option<&str>) -> u64 {
        let mut turn_params = json!({
            "threadId": thread_id,
            "model": model,
            "input": [
                { "type": "text", "text": prompt }
            ]
        });
        if let (Some(eff), Some(obj)) = (effort, turn_params.as_object_mut()) {
            obj.insert("effort".into(), json!(eff));
        }
        let req_id = self.send_request("turn/start", turn_params);
        self.pending_requests.insert(req_id, RequestCorrelation::TurnStart { thread_id: thread_id.to_string(), session_id: session_id.to_string() });
        req_id
    }

    /// Interrupt an active turn with its bound thread ID and turn ID.
    pub fn interrupt_turn(&mut self, thread_id: &str, turn_id: &str) -> u64 {
        let req_id = self.send_request(
            "turn/interrupt",
            json!({
                "threadId": thread_id,
                "turnId": turn_id,
            }),
        );
        self.pending_requests.insert(req_id, RequestCorrelation::TurnInterrupt { thread_id: thread_id.to_string(), turn_id: turn_id.to_string() });
        self.transport.interrupt();
        req_id
    }

    pub fn poll_events(&mut self) -> Vec<ProtocolEvent> {
        let mut events = Vec::new();
        if !self.transport.is_alive() {
            self.harness_state = HarnessState::MissingExecutable("Codex process exited unexpectedly".into());
            events.push(ProtocolEvent::Error { thread_id: None, message: "Codex process exited unexpectedly".into() });
            return events;
        }

        let raw_lines = self.transport.poll_lines();
        for line in raw_lines {
            match parse_message_line(&line) {
                Ok(msg) => {
                    self.handle_message(&msg, &mut events);
                }
                Err(err) => {
                    events.push(ProtocolEvent::Error { thread_id: None, message: err });
                }
            }
        }
        events
    }

    fn handle_message(&mut self, msg: &ServerMessage, events: &mut Vec<ProtocolEvent>) {
        match msg {
            ServerMessage::Response { id, result, error } => {
                let correlation = self.pending_requests.remove(id);

                if let Some(err) = error {
                    let err_str = err.to_string();
                    if matches!(correlation, Some(RequestCorrelation::Initialize)) {
                        self.phase = SupervisorPhase::Failed;
                        self.harness_state = HarnessState::Unavailable(format!("Initialize failed: {err_str}"));
                    }
                    events.push(ProtocolEvent::Error { thread_id: None, message: err_str });
                    return;
                }

                if let Some(res) = result {
                    match correlation {
                        Some(RequestCorrelation::Initialize) => {
                            self.phase = SupervisorPhase::Initialized;
                            // Step 2: Send required `initialized` notification
                            self.send_notification("initialized", json!({}));

                            // Step 3: Discover available models and account info
                            let m_id = self.send_request("model/list", json!({}));
                            self.pending_requests.insert(m_id, RequestCorrelation::ModelList);

                            let a_id = self.send_request("account/read", json!({}));
                            self.pending_requests.insert(a_id, RequestCorrelation::AccountRead);

                            self.phase = SupervisorPhase::Discovering { model_req_id: Some(m_id), account_req_id: Some(a_id) };
                        }
                        Some(RequestCorrelation::ModelList) => {
                            if let Some(data) = res.get("data").and_then(Value::as_array) {
                                let models: Vec<String> =
                                    data.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(|s| s.to_string())).collect();
                                if !models.is_empty() {
                                    self.available_models = models.clone();
                                    self.harness_state = HarnessState::Ready;
                                    self.phase = SupervisorPhase::Ready;
                                    events.push(ProtocolEvent::Initialized {
                                        models,
                                        account_email: self.account_email.clone(),
                                        plan_type: self.plan_type.clone(),
                                    });
                                }
                            }
                        }
                        Some(RequestCorrelation::AccountRead) => {
                            if let Some(acc) = res.get("account") {
                                self.account_email = acc.get("email").and_then(Value::as_str).map(|s| s.to_string());
                                self.plan_type = acc.get("planType").and_then(Value::as_str).map(|s| s.to_string());
                            }
                        }
                        Some(RequestCorrelation::ThreadStart { .. }) => {
                            let tid = res
                                .get("thread")
                                .and_then(|t| t.get("id"))
                                .and_then(Value::as_str)
                                .or_else(|| res.get("threadId").and_then(Value::as_str));
                            if let Some(tid_str) = tid {
                                events.push(ProtocolEvent::ThreadStarted { thread_id: tid_str.to_string() });
                            }
                        }
                        Some(RequestCorrelation::TurnStart { ref thread_id, .. }) => {
                            let tid = res
                                .get("turn")
                                .and_then(|t| t.get("id"))
                                .and_then(Value::as_str)
                                .or_else(|| res.get("turnId").and_then(Value::as_str));
                            if let Some(turn_id) = tid {
                                events.push(ProtocolEvent::TurnStarted { thread_id: Some(thread_id.clone()), turn_id: turn_id.to_string() });
                            }
                        }
                        Some(RequestCorrelation::TurnInterrupt { .. }) => {}
                        None => {
                            // Uncorrelated response: check if it contains model list or account
                            if let Some(data) = res.get("data").and_then(Value::as_array) {
                                let models: Vec<String> =
                                    data.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(|s| s.to_string())).collect();
                                if !models.is_empty() {
                                    self.available_models = models.clone();
                                    self.harness_state = HarnessState::Ready;
                                    events.push(ProtocolEvent::Initialized {
                                        models,
                                        account_email: self.account_email.clone(),
                                        plan_type: self.plan_type.clone(),
                                    });
                                }
                            }
                            if let Some(acc) = res.get("account") {
                                self.account_email = acc.get("email").and_then(Value::as_str).map(|s| s.to_string());
                                self.plan_type = acc.get("planType").and_then(Value::as_str).map(|s| s.to_string());
                            }
                        }
                    }
                }
            }
            ServerMessage::Notification { method, params } => {
                if method == "account/updated" {
                    if let Some(plan) = params.get("planType").and_then(Value::as_str) {
                        self.plan_type = Some(plan.to_string());
                    }
                    if let Some(email) = params.get("email").and_then(Value::as_str) {
                        self.account_email = Some(email.to_string());
                    }
                }
                if let Some(ev) = message_to_event(msg) {
                    events.push(ev);
                }
            }
            _ => {
                if let Some(ev) = message_to_event(msg) {
                    events.push(ev);
                }
            }
        }
    }
}

/// Detect the installed Codex executable on native platforms.
pub fn detect_codex_cli() -> Option<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let candidates = ["/opt/homebrew/bin/codex", "/usr/local/bin/codex", "/usr/bin/codex"];
        for path_str in candidates {
            let p = Path::new(path_str);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }

        // Try `which codex`
        if let Some(out) = std::process::Command::new("which").arg("codex").output().ok().filter(|o| o.status.success()) {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let p = PathBuf::from(s);
                if p.exists() {
                    return Some(p);
                }
            }
        }
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct SubprocessTransport {
    child: std::process::Child,
    stdin_tx: SyncSender<String>,
    stdout_rx: Receiver<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SubprocessTransport {
    pub fn spawn(executable: &Path) -> std::io::Result<Self> {
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Command, Stdio};

        let mut child = Command::new(executable).arg("app-server").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;

        let mut stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("Failed to open child stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("Failed to open child stdout"))?;

        let (stdin_tx, stdin_rx): (SyncSender<String>, Receiver<String>) = sync_channel(256);
        let (stdout_tx, stdout_rx): (SyncSender<String>, Receiver<String>) = sync_channel(256);

        std::thread::Builder::new().name("codex-stdin".into()).spawn(move || {
            while let Ok(line) = stdin_rx.recv() {
                if stdin.write_all(line.as_bytes()).is_err() || stdin.write_all(b"\n").is_err() || stdin.flush().is_err() {
                    break;
                }
            }
        })?;

        std::thread::Builder::new().name("codex-stdout".into()).spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line_buf = Vec::new();
            loop {
                let available = match reader.fill_buf() {
                    Ok([]) => break,
                    Ok(b) => b,
                    Err(_) => break,
                };

                let mut consumed = 0;
                let mut found_newline = false;
                for (i, &b) in available.iter().enumerate() {
                    if b == b'\n' {
                        consumed = i + 1;
                        found_newline = true;
                        break;
                    }
                }

                if found_newline {
                    let content_len = consumed - 1;
                    if line_buf.len() + content_len <= MAX_MESSAGE_SIZE {
                        line_buf.extend_from_slice(&available[..content_len]);
                        if let Ok(line) = String::from_utf8(std::mem::take(&mut line_buf)) {
                            let trimmed = line.trim().to_string();
                            if !trimmed.is_empty() && stdout_tx.send(trimmed).is_err() {
                                break;
                            }
                        }
                    } else {
                        line_buf.clear();
                        let _ = stdout_tx
                            .send(r#"{"method":"error","params":{"message":"protocol error: message exceeds maximum size of 16 MiB"}}"#.to_string());
                    }
                    reader.consume(consumed);
                } else {
                    consumed = available.len();
                    if line_buf.len() + consumed <= MAX_MESSAGE_SIZE {
                        line_buf.extend_from_slice(available);
                    } else {
                        line_buf.clear();
                    }
                    reader.consume(consumed);
                }
            }
        })?;

        Ok(Self { child, stdin_tx, stdout_rx })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SubprocessTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SupervisorTransport for SubprocessTransport {
    fn send_line(&mut self, line: &str) -> Result<(), String> {
        self.stdin_tx.send(line.to_string()).map_err(|e| format!("failed to send line to subprocess: {e}"))
    }

    fn poll_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(line) = self.stdout_rx.try_recv() {
            lines.push(line);
        }
        lines
    }

    fn interrupt(&mut self) {
        // Supported turn interrupt happens via JSON-RPC `turn/interrupt`.
    }

    fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => false,
            Err(_) => false,
        }
    }
}

/// Controllable fixture transport for deterministic testing of protocol edge cases.
#[derive(Clone, Default)]
pub struct MockTransportState {
    pub sent_lines: Vec<String>,
    pub queued_lines: Vec<String>,
    pub alive: bool,
    pub interrupted: bool,
}

#[derive(Clone, Default)]
pub struct MockSupervisorTransport {
    pub state: std::sync::Arc<std::sync::Mutex<MockTransportState>>,
}

impl MockSupervisorTransport {
    pub fn new() -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(MockTransportState {
                sent_lines: Vec::new(),
                queued_lines: Vec::new(),
                alive: true,
                interrupted: false,
            })),
        }
    }

    pub fn push_incoming(&self, line: impl Into<String>) {
        if let Ok(mut st) = self.state.lock() {
            st.queued_lines.push(line.into());
        }
    }

    pub fn sent_lines(&self) -> Vec<String> {
        self.state.lock().map(|st| st.sent_lines.clone()).unwrap_or_default()
    }

    pub fn set_alive(&self, alive: bool) {
        if let Ok(mut st) = self.state.lock() {
            st.alive = alive;
        }
    }
}

impl SupervisorTransport for MockSupervisorTransport {
    fn send_line(&mut self, line: &str) -> Result<(), String> {
        if let Ok(mut st) = self.state.lock() {
            st.sent_lines.push(line.to_string());
        }
        Ok(())
    }

    fn poll_lines(&mut self) -> Vec<String> {
        if let Ok(mut st) = self.state.lock() { std::mem::take(&mut st.queued_lines) } else { Vec::new() }
    }

    fn interrupt(&mut self) {
        if let Ok(mut st) = self.state.lock() {
            st.interrupted = true;
        }
    }

    fn is_alive(&mut self) -> bool {
        self.state.lock().map(|st| st.alive).unwrap_or(false)
    }
}
