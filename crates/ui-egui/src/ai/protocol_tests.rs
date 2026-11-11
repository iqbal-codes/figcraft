use serde_json::json;

use super::protocol::{
    MAX_MESSAGE_SIZE, ProtocolEvent, ServerMessage, format_approval_response, format_dynamic_tool_response, format_notification, format_request,
    format_response, message_to_event, parse_message_line,
};

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
fn test_parse_thread_started_notification() {
    let line = r#"{"method":"thread/started","params":{"thread":{"id":"th_12345"}}}"#;
    let msg = parse_message_line(line).unwrap();
    let event = message_to_event(&msg).unwrap();
    assert_eq!(event, ProtocolEvent::ThreadStarted { thread_id: "th_12345".into() });
}

#[test]
fn test_parse_turn_started_notification() {
    let line = r#"{"method":"turn/started","params":{"threadId":"th_12345","turn":{"id":"turn_99"}}}"#;
    let msg = parse_message_line(line).unwrap();
    let event = message_to_event(&msg).unwrap();
    assert_eq!(event, ProtocolEvent::TurnStarted { thread_id: Some("th_12345".into()), turn_id: "turn_99".into() });
}

#[test]
fn test_parse_notification_and_convert_to_event() {
    let line = r#"{"method":"item/agentMessage/delta","params":{"threadId":"th_1","turnId":"t_1","delta":"Hello world!"}}"#;
    let msg = parse_message_line(line).unwrap();
    let event = message_to_event(&msg).unwrap();
    assert_eq!(event, ProtocolEvent::AgentDelta { thread_id: Some("th_1".into()), turn_id: Some("t_1".into()), text: "Hello world!".into() });
}

#[test]
fn test_parse_dynamic_tool_call() {
    let line = r#"{"id":"call_req_1","method":"item/tool/call","params":{"threadId":"th_1","turnId":"turn_1","callId":"call_abc","tool":"canvas_create_frame","arguments":{"name":"Navbar","x":0,"y":0,"width":800,"height":60}}}"#;
    let msg = parse_message_line(line).unwrap();
    let event = message_to_event(&msg).unwrap();
    assert_eq!(
        event,
        ProtocolEvent::ToolCall {
            thread_id: Some("th_1".into()),
            turn_id: Some("turn_1".into()),
            call_id: "call_abc".into(),
            request_id: json!("call_req_1"),
            name: "canvas_create_frame".into(),
            params: json!({"name":"Navbar","x":0,"y":0,"width":800,"height":60})
        }
    );
}

#[test]
fn test_parse_tool_item_started_and_completed() {
    let line_start = r#"{"method":"item/started","params":{"item":{"id":"item_1","name":"create_node"}}}"#;
    let msg_start = parse_message_line(line_start).unwrap();
    let event_start = message_to_event(&msg_start).unwrap();
    assert_eq!(
        event_start,
        ProtocolEvent::ItemStarted { thread_id: None, turn_id: None, item_id: "item_1".into(), tool_name: Some("create_node".into()) }
    );

    let line_end = r#"{"method":"item/completed","params":{"item":{"id":"item_1","status":"success","summary":"Created frame 42"}}}"#;
    let msg_end = parse_message_line(line_end).unwrap();
    let event_end = message_to_event(&msg_end).unwrap();
    assert_eq!(
        event_end,
        ProtocolEvent::ItemCompleted {
            thread_id: None,
            turn_id: None,
            item_id: "item_1".into(),
            is_error: false,
            summary: Some("Created frame 42".into())
        }
    );
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
        ProtocolEvent::ApprovalRequired {
            thread_id: None,
            turn_id: None,
            request_id: json!("req_abc"),
            action: "Edit outside scope".into(),
            target: "Document Root".into()
        }
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
fn test_format_request_notification_response_and_dynamic_tool() {
    let req = format_request(42, "turn/interrupt", json!({"threadId": "t1", "turnId": "turn_1"}));
    assert!(req.contains(r#""id":42"#));
    assert!(req.contains(r#""method":"turn/interrupt""#));

    let notif = format_notification("initialized", json!({}));
    assert!(notif.contains(r#""method":"initialized""#));
    assert!(!notif.contains(r#""id""#));

    let resp = format_response(&json!("req_123"), json!({"decision": "allow"}));
    assert!(resp.contains(r#""id":"req_123""#));
    assert!(resp.contains(r#""decision":"allow""#));

    let tool_resp = format_dynamic_tool_response(&json!("req_t1"), true, r#"{"created": 42}"#);
    assert!(tool_resp.contains(r#""id":"req_t1""#));
    assert!(tool_resp.contains(r#""success":true"#));
    assert!(tool_resp.contains(r#""inputText""#));

    let appr_resp = format_approval_response(&json!("req_a1"), true);
    assert!(appr_resp.contains(r#""decision":"allow""#));
}
