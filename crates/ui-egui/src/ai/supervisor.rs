//! Codex harness supervisor managing the subprocess stdio loop and event channels.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value, json};

use super::protocol::{ClientInfo, ProtocolEvent, ServerMessage, format_request, format_response, message_to_event, parse_message_line};

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

/// Abstract transport to allow real stdio subprocess or controlled test fixtures.
pub trait SupervisorTransport: Send {
    fn send_line(&mut self, line: &str) -> Result<(), String>;
    fn poll_lines(&mut self) -> Vec<String>;
    fn interrupt(&mut self);
    fn is_alive(&self) -> bool;
}

/// Supervisor driving Codex app-server over stdio.
pub struct CodexSupervisor {
    transport: Box<dyn SupervisorTransport>,
    next_id: u64,
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
        let mut s =
            Self { transport, next_id: 1, available_models: Vec::new(), account_email: None, plan_type: None, harness_state: HarnessState::Checking };
        s.initialize();
        s
    }

    fn initialize(&mut self) {
        let client_info = ClientInfo::default();
        self.send_request("initialize", json!({ "clientInfo": client_info }));
        self.send_request("model/list", json!({}));
        self.send_request("account/read", json!({}));
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let line = format_request(id, method, params);
        let _ = self.transport.send_line(&line);
        id
    }

    pub fn send_response(&mut self, request_id: &Value, result: Value) {
        let line = format_response(request_id, result);
        let _ = self.transport.send_line(&line);
    }

    pub fn interrupt(&mut self, thread_id: Option<&str>) {
        if let Some(tid) = thread_id {
            self.send_request("turn/interrupt", json!({ "threadId": tid }));
        }
        self.transport.interrupt();
    }

    pub fn start_turn(&mut self, thread_id: &str, prompt: &str) -> u64 {
        self.send_request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [
                    { "type": "text", "text": prompt }
                ]
            }),
        )
    }

    pub fn poll_events(&mut self) -> Vec<ProtocolEvent> {
        let mut events = Vec::new();
        if !self.transport.is_alive() {
            self.harness_state = HarnessState::MissingExecutable("Codex process exited unexpectedly".into());
            events.push(ProtocolEvent::Error { message: "Codex process exited unexpectedly".into() });
            return events;
        }

        let raw_lines = self.transport.poll_lines();
        for line in raw_lines {
            match parse_message_line(&line) {
                Ok(msg) => {
                    self.handle_message(&msg, &mut events);
                }
                Err(err) => {
                    events.push(ProtocolEvent::Error { message: err });
                }
            }
        }
        events
    }

    fn handle_message(&mut self, msg: &ServerMessage, events: &mut Vec<ProtocolEvent>) {
        match msg {
            ServerMessage::Response { id: _, result, error } => {
                if let Some(err) = error {
                    events.push(ProtocolEvent::Error { message: err.to_string() });
                    return;
                }
                if let Some(res) = result {
                    // Check if this was model/list response
                    if let Some(data) = res.get("data").and_then(Value::as_array) {
                        let models: Vec<String> = data.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(|s| s.to_string())).collect();
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
                    // Check if this was account/read response
                    if let Some(acc) = res.get("account") {
                        self.account_email = acc.get("email").and_then(Value::as_str).map(|s| s.to_string());
                        self.plan_type = acc.get("planType").and_then(Value::as_str).map(|s| s.to_string());
                    }
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
    stdin_tx: Sender<String>,
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

        let (stdin_tx, stdin_rx): (Sender<String>, Receiver<String>) = channel();
        let (stdout_tx, stdout_rx): (Sender<String>, Receiver<String>) = channel();

        std::thread::Builder::new().name("codex-stdin".into()).spawn(move || {
            while let Ok(line) = stdin_rx.recv() {
                if stdin.write_all(line.as_bytes()).is_err() || stdin.write_all(b"\n").is_err() || stdin.flush().is_err() {
                    break;
                }
            }
        })?;

        std::thread::Builder::new().name("codex-stdout".into()).spawn(move || {
            let reader = BufReader::new(stdout);
            for line_res in reader.lines() {
                match line_res {
                    Ok(line) => {
                        if stdout_tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })?;

        Ok(Self { child, stdin_tx, stdout_rx })
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

    fn is_alive(&self) -> bool {
        // try_wait checks if child has exited
        match std::process::Command::new("kill").arg("-0").arg(self.child.id().to_string()).status() {
            Ok(s) => s.success(),
            Err(_) => false,
        }
    }
}

/// Controllable fixture transport for deterministic testing of protocol edge cases.
pub struct MockSupervisorTransport {
    pub sent_lines: Vec<String>,
    pub queued_lines: Vec<String>,
    pub alive: bool,
    pub interrupted: bool,
}

impl MockSupervisorTransport {
    pub fn new() -> Self {
        Self { sent_lines: Vec::new(), queued_lines: Vec::new(), alive: true, interrupted: false }
    }

    pub fn push_incoming(&mut self, line: impl Into<String>) {
        self.queued_lines.push(line.into());
    }
}

impl Default for MockSupervisorTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl SupervisorTransport for MockSupervisorTransport {
    fn send_line(&mut self, line: &str) -> Result<(), String> {
        self.sent_lines.push(line.to_string());
        Ok(())
    }

    fn poll_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.queued_lines)
    }

    fn interrupt(&mut self) {
        self.interrupted = true;
    }

    fn is_alive(&self) -> bool {
        self.alive
    }
}
