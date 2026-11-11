use serde_json::json;

use super::protocol::ProtocolEvent;
use super::supervisor::{CodexSupervisor, HarnessState, MockSupervisorTransport};

#[test]
fn test_supervisor_initialization_and_model_discovery() {
    let mut transport = MockSupervisorTransport::new();

    // Prepare server responses
    transport.push_incoming(r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex/0.167.0"}}"#);
    transport.push_incoming(r#"{"jsonrpc":"2.0","id":2,"result":{"data":[{"id":"gpt-5.6-sol"},{"id":"gpt-6.1-sol"}]}}"#);
    transport.push_incoming(r#"{"jsonrpc":"2.0","id":3,"result":{"account":{"email":"test@example.com","planType":"plus"}}}"#);

    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    let events = supervisor.poll_events();
    assert_eq!(supervisor.harness_state, HarnessState::Ready);
    assert_eq!(supervisor.available_models, vec!["gpt-5.6-sol", "gpt-6.1-sol"]);
    assert_eq!(supervisor.account_email.as_deref(), Some("test@example.com"));
    assert_eq!(supervisor.plan_type.as_deref(), Some("plus"));

    let init_event = events.iter().find(|e| matches!(e, ProtocolEvent::Initialized { .. }));
    assert!(init_event.is_some());
}

#[test]
fn test_supervisor_stream_delta_and_approvals() {
    let transport = MockSupervisorTransport::new();
    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    let _ = supervisor.poll_events(); // flush init

    // Mock incoming delta
    let mut mock_trans = MockSupervisorTransport::new();
    mock_trans.push_incoming(r#"{"method":"item/agentMessage/delta","params":{"delta":"Designing a navbar"}}"#);
    mock_trans.push_incoming(r#"{"id":"req_appr","method":"item/permissions/requestApproval","params":{"action":"Create root","target":"Page 1"}}"#);

    supervisor = CodexSupervisor::new(Box::new(mock_trans));
    let events = supervisor.poll_events();

    assert!(events.contains(&ProtocolEvent::AgentDelta { text: "Designing a navbar".into() }));
    assert!(events.contains(&ProtocolEvent::ApprovalRequired {
        request_id: json!("req_appr"),
        action: "Create root".into(),
        target: "Page 1".into(),
    }));

    // Send response
    supervisor.send_response(&json!("req_appr"), json!({"decision": "allow"}));
}

#[test]
fn test_supervisor_interrupt_and_stop() {
    let transport = MockSupervisorTransport::new();
    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    supervisor.interrupt(Some("thread_42"));
    // Interrupt should not panic and should register
}

#[test]
fn test_supervisor_process_exit_reports_error() {
    let mut transport = MockSupervisorTransport::new();
    transport.alive = false; // dead process

    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    let events = supervisor.poll_events();

    assert!(events.iter().any(|e| matches!(e, ProtocolEvent::Error { .. })));
    assert!(matches!(supervisor.harness_state, HarnessState::MissingExecutable(_)));
}

#[test]
fn test_supervisor_malformed_protocol_line() {
    let mut transport = MockSupervisorTransport::new();
    transport.push_incoming("{ malformed json line without closure");

    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    let events = supervisor.poll_events();

    let err_event = events.iter().find(|e| matches!(e, ProtocolEvent::Error { .. }));
    assert!(err_event.is_some());
}

#[test]
fn test_ai_state_turn_and_stop() {
    let transport = MockSupervisorTransport::new();
    let supervisor = CodexSupervisor::new(Box::new(transport));
    let mut state = super::AiState::with_supervisor(supervisor);

    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));

    state.draft_text = "Create header frame".into();
    state.submit_turn(&mut session);

    assert!(state.is_running);
    assert_eq!(state.sessions[0].messages.len(), 1);
    assert_eq!(state.sessions[0].messages[0].text, "Create header frame");
    assert!(state.active_task.is_some());

    state.stop_active_task();
    assert!(!state.is_running);
    assert_eq!(state.sessions[0].status, super::TurnStatus::Stopped);
}
