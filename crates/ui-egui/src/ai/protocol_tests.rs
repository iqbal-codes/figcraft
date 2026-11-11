use serde_json::json;

use super::protocol::{MAX_MESSAGE_SIZE, ProtocolEvent, ServerMessage, format_request, format_response, message_to_event, parse_message_line};

#[test]
fn test_parse_valid_response() {
    let line = r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex/0.167.0"}}"#;
    let msg = parse_message_line(line).unwrap();
    match msg {
        ServerMessage::Response { id, result, error } => {
            assert_eq!(id, 1);
            assert!(result.is_some());
            assert!(error.is_none());
        }
        _ => panic!("Expected Response"),
    }
}

#[test]
fn test_parse_notification_and_convert_to_event() {
    let line = r#"{"method":"item/agentMessage/delta","params":{"delta":"Hello world!"}}"#;
    let msg = parse_message_line(line).unwrap();
    let event = message_to_event(&msg).unwrap();
    assert_eq!(event, ProtocolEvent::AgentDelta { text: "Hello world!".into() });
}

#[test]
fn test_parse_tool_item_started_and_completed() {
    let line_start = r#"{"method":"item/started","params":{"item":{"id":"item_1","name":"create_node"}}}"#;
    let msg_start = parse_message_line(line_start).unwrap();
    let event_start = message_to_event(&msg_start).unwrap();
    assert_eq!(event_start, ProtocolEvent::ItemStarted { item_id: "item_1".into(), tool_name: Some("create_node".into()) });

    let line_end = r#"{"method":"item/completed","params":{"item":{"id":"item_1","status":"success","summary":"Created frame 42"}}}"#;
    let msg_end = parse_message_line(line_end).unwrap();
    let event_end = message_to_event(&msg_end).unwrap();
    assert_eq!(event_end, ProtocolEvent::ItemCompleted { item_id: "item_1".into(), is_error: false, summary: Some("Created frame 42".into()) });
}

#[test]
fn test_parse_server_approval_request() {
    let line = r#"{"id":"req_abc","method":"item/permissions/requestApproval","params":{"action":"Edit outside scope","target":"Document Root"}}"#;
    let msg = parse_message_line(line).unwrap();
    match &msg {
        ServerMessage::Request { id, method, .. } => {
            assert_eq!(id, &json!("req_abc"));
            assert_eq!(method, "item/permissions/requestApproval");
        }
        _ => panic!("Expected Request"),
    }

    let event = message_to_event(&msg).unwrap();
    assert_eq!(
        event,
        ProtocolEvent::ApprovalRequired { request_id: json!("req_abc"), action: "Edit outside scope".into(), target: "Document Root".into() }
    );
}

#[test]
fn test_parse_malformed_json_fails_cleanly() {
    let broken = r#"{"jsonrpc": "2.0", "id": 1, "result": { broken json"#;
    let res = parse_message_line(broken);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("malformed JSON"));
}

#[test]
fn test_parse_oversized_message_rejected() {
    let large = "a".repeat(MAX_MESSAGE_SIZE + 10);
    let res = parse_message_line(&large);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("exceeds maximum size"));
}

#[test]
fn test_format_request_and_response() {
    let req = format_request(42, "turn/interrupt", json!({"threadId": "t1"}));
    assert!(req.contains(r#""id":42"#));
    assert!(req.contains(r#""method":"turn/interrupt""#));

    let resp = format_response(&json!("req_123"), json!({"decision": "allow"}));
    assert!(resp.contains(r#""id":"req_123""#));
    assert!(resp.contains(r#""decision":"allow""#));
}
