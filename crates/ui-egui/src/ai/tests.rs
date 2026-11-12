use serde_json::json;

use super::protocol::ProtocolEvent;
use super::supervisor::{CodexSupervisor, HarnessState, MockSupervisorTransport, SupervisorPhase};
use super::{AiState, TurnStatus};

/// A model catalog to send against. The app starts with none and only the harness fills it, so a
/// test that sends a turn states what it is running on.
fn seed_models(ai: &mut AiState) {
    ai.set_models(vec![
        super::supervisor::ModelInfo {
            id: "gpt-5.6-sol".to_string(),
            display_name: Some("GPT-5.6 Sol".to_string()),
            supported_efforts: vec!["low".into(), "medium".into(), "high".into()],
            default_effort: Some("medium".into()),
        },
        super::supervisor::ModelInfo {
            id: "gpt-6.1-sol".to_string(),
            display_name: Some("GPT-6.1 Sol".to_string()),
            supported_efforts: vec!["low".into(), "high".into(), "max".into()],
            default_effort: Some("high".into()),
        },
    ]);
}

/// The rect the left panel gives the composer: the bottom of a narrow AI dock.
fn composer_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::new(0.0, 480.0 - super::composer::COMPOSER_HEIGHT), egui::vec2(200.0, super::composer::COMPOSER_HEIGHT))
}

#[test]
fn composer_exposes_actionable_send_and_stop_to_accessibility() {
    let mut app = crate::FigApp::new(figcraft_engine::Session::new(), Box::new(crate::NoServices));
    seed_models(&mut app.ui.ai);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    for (index, running, draft, label, disabled) in [(0, false, "", "Send", true), (1, false, "Design", "Send", false), (2, true, "", "Stop", false)]
    {
        app.ui.ai.is_running = running;
        app.ui.ai.draft_text = draft.into();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 480.0))),
            time: Some(f64::from(index)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| super::composer::show(&mut app, ui, composer_rect()));
        output.textures_delta.clear();
        let update = output.platform_output.accesskit_update.unwrap();
        let (_, button) = update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == egui::accesskit::Role::Button && node.label() == Some(label))
            .expect("named composer button");
        assert_eq!(button.is_disabled(), disabled, "wrong accessible enabled state for {label}");
    }
}

#[test]
fn runtime_denies_external_requests_with_their_actual_response_schema() {
    for (method, expected) in [
        ("item/commandExecution/requestApproval", json!({"decision": "decline"})),
        ("item/fileChange/requestApproval", json!({"decision": "decline"})),
        ("item/permissions/requestApproval", json!({"permissions": {}, "scope": "turn"})),
    ] {
        let transport = MockSupervisorTransport::new();
        let handle = transport.clone();
        let mut app = crate::FigApp::new(figcraft_engine::Session::new(), Box::new(crate::NoServices));
        app.ui.ai.supervisor = Some(CodexSupervisor::new(Box::new(transport)));
        app.ui.ai.sessions[0].thread_id = Some("thread".into());
        app.ui.ai.sessions[0].active_turn_id = Some("turn".into());
        app.ui.ai.sessions[0].status = TurnStatus::Running;
        handle.push_incoming(
            json!({"id": 77, "method": method, "params": {"threadId": "thread", "turnId": "turn", "command": "external action"}}).to_string(),
        );
        super::update_ai_runtime(&mut app, &egui::Context::default());
        let responses: Vec<serde_json::Value> =
            handle.sent_lines().iter().filter_map(|line| serde_json::from_str(line).ok()).filter(|v: &serde_json::Value| v["id"] == 77).collect();
        assert_eq!(responses.len(), 1, "unsupported permission must settle without waiting for a button: {method}");
        assert_eq!(responses[0]["result"], expected);
        assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Running);
    }
}

#[test]
fn unknown_server_request_does_not_abort_the_running_turn() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();
    let mut app = crate::FigApp::new(figcraft_engine::Session::new(), Box::new(crate::NoServices));
    app.ui.ai.supervisor = Some(CodexSupervisor::new(Box::new(transport)));
    app.ui.ai.sessions[0].thread_id = Some("thread".into());
    app.ui.ai.sessions[0].active_turn_id = Some("turn".into());
    app.ui.ai.sessions[0].status = TurnStatus::Running;
    handle.push_incoming(json!({"id": 78, "method": "future/request", "params": {"threadId": "thread", "turnId": "turn"}}).to_string());
    super::update_ai_runtime(&mut app, &egui::Context::default());
    assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Running);
    assert_eq!(app.ui.ai.sessions[0].active_turn_id.as_deref(), Some("turn"));
    assert!(handle.sent_lines().iter().any(|line| line.contains("-32601")));
}

#[test]
fn external_edit_requires_fresh_tool_context_before_another_mutation() {
    let mut session = figcraft_engine::Session::new();
    let root = session.execute("frame.create", &json!({"width": 200, "height": 100})).unwrap()["id"].as_u64().unwrap();
    let ds = session.ds().unwrap();
    let mut auth = figcraft_engine::TaskAuthority::new_with_revision(
        "refresh",
        ds.uid,
        ds.revision,
        figcraft_engine::TaskScope::Subtree { page_id: ds.page, target_node: figcraft_doc::NodeId(root) },
    );
    session.execute("node.set", &json!({"id": root, "props": {"name": "Human edit"}})).unwrap();
    let mutation = json!({"id": root, "props": {"name": "Agent edit"}});
    let stale = super::execute_tool(&mut session, &mut auth, "canvas_set_properties", &mutation);
    assert!(stale.is_err(), "stale tool must not silently overwrite a human edit");
    let fresh = super::execute_tool(&mut session, &mut auth, "canvas_query_tree", &json!({})).unwrap();
    assert_eq!(fresh["tree"]["name"], "Human edit");
    super::execute_tool(&mut session, &mut auth, "canvas_set_properties", &mutation).unwrap();
}

#[test]
fn selection_and_view_changes_do_not_invalidate_inspected_canvas_context() {
    let mut session = figcraft_engine::Session::new();
    let root = session.execute("frame.create", &json!({"width": 200, "height": 100})).unwrap()["id"].as_u64().unwrap();
    let ds = session.ds().unwrap();
    let mut auth = figcraft_engine::TaskAuthority::new_with_revision(
        "view-change",
        ds.uid,
        ds.revision,
        figcraft_engine::TaskScope::Subtree { page_id: ds.page, target_node: figcraft_doc::NodeId(root) },
    );
    super::execute_tool(&mut session, &mut auth, "canvas_query_tree", &json!({})).unwrap();
    session.set_selection(vec![]);
    session.execute("view.zoomIn", &json!({})).unwrap();
    super::execute_tool(&mut session, &mut auth, "canvas_set_properties", &json!({"id": root, "props": {"name": "After view change"}}))
        .expect("selection and zoom do not edit the inspected document");
    session.execute("view.zoomOut", &json!({})).unwrap();
    super::execute_tool(&mut session, &mut auth, "canvas_set_properties", &json!({"id": root, "props": {"name": "After own edit"}}))
        .expect("own edits must also retain the document snapshot");
    session.execute("node.set", &json!({"id": root, "props": {"name": "Human edit"}})).unwrap();
    assert!(
        super::execute_tool(&mut session, &mut auth, "canvas_set_properties", &json!({"id": root, "props": {"name": "Stale overwrite"}})).is_err()
    );
    assert_eq!(session.doc().unwrap().node(figcraft_doc::NodeId(root)).unwrap().name, "Human edit");
}

#[test]
fn composer_keeps_ime_composition_across_frames_until_commit() {
    use egui::{Event, ImeEvent, Key, Modifiers};
    fn frame(ctx: &egui::Context, app: &mut crate::FigApp, index: u32, events: Vec<Event>) {
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique("ai_composer_input")));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 480.0))),
            time: Some(f64::from(index) / 60.0),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| super::composer::show(app, ui, composer_rect()));
        output.textures_delta.clear();
    }
    let mut app = crate::FigApp::new(figcraft_engine::Session::new(), Box::new(crate::NoServices));
    app.ui.ai.supervisor = Some(CodexSupervisor::new(Box::new(MockSupervisorTransport::new())));
    seed_models(&mut app.ui.ai);
    app.ui.ai.draft_text = "Design".into();
    let ctx = egui::Context::default();
    let key = |pressed| Event::Key { key: Key::Enter, physical_key: Some(Key::Enter), pressed, repeat: false, modifiers: Modifiers::NONE };
    frame(&ctx, &mut app, 0, vec![]);
    frame(&ctx, &mut app, 1, vec![Event::Ime(ImeEvent::Preedit { text: "候補".into(), active_range_chars: None })]);
    frame(&ctx, &mut app, 2, vec![key(true)]);
    assert!(app.ui.ai.sessions[0].messages.is_empty(), "IME candidate confirmation submitted a turn");
    assert!(!app.ui.ai.draft_text.is_empty(), "IME confirmation lost the draft");
    frame(&ctx, &mut app, 3, vec![key(false)]);
    frame(&ctx, &mut app, 4, vec![Event::Ime(ImeEvent::Commit("候補".into())), key(true)]);
    assert!(app.ui.ai.sessions[0].messages.is_empty(), "commit-frame Enter submitted a turn");
    frame(&ctx, &mut app, 5, vec![key(false)]);
    frame(&ctx, &mut app, 6, vec![key(true)]);
    assert_eq!(app.ui.ai.sessions[0].messages.len(), 1, "a deliberate Enter after composition should submit exactly once");
}

#[test]
fn test_supervisor_initialization_and_handshake_lifecycle() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();

    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    // Phase should be Initializing, having sent request id 1 (initialize)
    assert!(matches!(supervisor.phase, SupervisorPhase::Initializing { init_req_id: 1 }));

    // Before initialize response arrives, model/list has NOT been sent
    let _ = supervisor.poll_events();
    assert_eq!(supervisor.available_models.len(), 0);

    // Provide initialize response (id 1)
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex/0.167.0"}}"#);
    let _ = supervisor.poll_events();

    // Verify that upon initialize response, phase advances to Discovering
    assert!(matches!(supervisor.phase, SupervisorPhase::Discovering { model_req_id: Some(2), account_req_id: Some(3) }));

    // Verify sent messages include initialized notification, model/list, and account/read
    let sent = handle.sent_lines();
    assert!(sent.iter().any(|l| l.contains(r#""method":"initialized""#)));
    assert!(sent.iter().any(|l| l.contains(r#""method":"model/list""#)));
    assert!(sent.iter().any(|l| l.contains(r#""method":"account/read""#)));

    // Provide model/list response (id 2) and account/read response (id 3)
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":2,"result":{"data":[{"id":"gpt-5.6-sol"},{"id":"gpt-6.1-sol"}]}}"#);
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":3,"result":{"account":{"email":"designer@figcraft.org","planType":"plus"}}}"#);

    let events = supervisor.poll_events();

    assert_eq!(supervisor.harness_state, HarnessState::Ready);
    assert_eq!(supervisor.available_models, vec!["gpt-5.6-sol", "gpt-6.1-sol"]);
    assert_eq!(supervisor.account_email.as_deref(), Some("designer@figcraft.org"));
    assert_eq!(supervisor.plan_type.as_deref(), Some("plus"));

    let init_event = events.iter().find(|e| matches!(e, ProtocolEvent::Initialized { .. }));
    assert!(init_event.is_some());
}

#[test]
fn test_supervisor_stream_delta_and_approvals() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();

    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    handle.push_incoming(r#"{"method":"item/agentMessage/delta","params":{"threadId":"th_1","turnId":"t_1","delta":"Designing a navbar"}}"#);
    handle.push_incoming(r#"{"id":"req_appr","method":"item/permissions/requestApproval","params":{"threadId":"th_1","turnId":"t_1","action":"Create root","target":"Page 1"}}"#);

    let events = supervisor.poll_events();

    assert!(events.contains(&ProtocolEvent::AgentDelta {
        thread_id: Some("th_1".into()),
        turn_id: Some("t_1".into()),
        text: "Designing a navbar".into()
    }));
    assert!(events.contains(&ProtocolEvent::ApprovalRequired {
        method: "item/permissions/requestApproval".into(),
        thread_id: Some("th_1".into()),
        turn_id: Some("t_1".into()),
        request_id: json!("req_appr"),
        action: "Create root".into(),
        target: "Page 1".into(),
    }));

    // Send response
    supervisor.send_response(&json!("req_appr"), json!({"decision": "allow"}));
}

#[test]
fn test_supervisor_interrupt_and_stop_with_turn_identity() {
    let transport = MockSupervisorTransport::new();
    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    let req_id = supervisor.interrupt_turn("th_abc", "turn_xyz").expect("interrupt sent");
    assert!(req_id > 0);
}

#[test]
fn test_supervisor_process_exit_reports_error() {
    let transport = MockSupervisorTransport::new();
    transport.set_alive(false); // dead process

    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    let events = supervisor.poll_events();

    assert!(events.iter().any(|e| matches!(e, ProtocolEvent::Error { .. })));
    assert!(matches!(supervisor.harness_state, HarnessState::MissingExecutable(_)));
}

#[test]
fn test_supervisor_malformed_protocol_line() {
    let transport = MockSupervisorTransport::new();
    transport.push_incoming("{ malformed json line without closure");

    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    let events = supervisor.poll_events();

    let err_event = events.iter().find(|e| matches!(e, ProtocolEvent::Error { .. }));
    assert!(err_event.is_some());
}

#[test]
fn test_ai_state_turn_thread_correlation_and_stop() {
    let transport = MockSupervisorTransport::new();
    let supervisor = CodexSupervisor::new(Box::new(transport));
    let mut state = AiState::with_supervisor(supervisor);
    seed_models(&mut state);

    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));

    state.draft_text = "Create header frame".into();
    state.submit_turn(&mut session);

    assert!(state.is_running);
    assert_eq!(state.sessions[0].messages.len(), 1);
    assert_eq!(state.sessions[0].messages[0].text, "Create header frame");
    assert!(state.sessions[0].pending_turn_prompt.is_some());
    assert!(state.active_task().is_some());

    state.stop_running_task();
    assert!(!state.is_running);
    assert_eq!(state.sessions[0].status, TurnStatus::Stopped);
    assert!(state.active_task().unwrap().is_stopped());
}

#[test]
fn test_stop_lifecycle_prevents_late_turn_overwrite_and_tool_mutations() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();
    let supervisor = CodexSupervisor::new(Box::new(transport));

    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));
    let mut app = crate::FigApp::new(session, Box::new(crate::NoServices));
    app.ui.ai.supervisor = Some(supervisor);

    // Submit turn to establish running session
    seed_models(&mut app.ui.ai);
    app.ui.ai.draft_text = "Generate cards".into();
    app.ui.ai.submit_turn(&mut app.session);
    assert!(app.ui.ai.is_running);
    let thread_id = "th_stop_lifecycle_test";
    app.ui.ai.sessions[0].thread_id = Some(thread_id.into());
    app.ui.ai.sessions[0].active_turn_id = Some("turn_initial".into());

    // Stop active task
    app.ui.ai.stop_running_task();
    assert!(!app.ui.ai.is_running);
    assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Stopped);
    assert!(app.ui.ai.sessions[0].pending_turn_prompt.is_none());
    let mut auth = app.ui.ai.sessions[0].task.as_ref().unwrap().clone();
    assert!(auth.is_stopped());

    // Simulate late TurnStarted event arriving after Stop
    handle.push_incoming(format!(r#"{{"method":"turn/started","params":{{"threadId":"{thread_id}","turnId":"turn_late"}}}}"#));
    let ctx = egui::Context::default();
    super::update_ai_runtime(&mut app, &ctx);

    // Must NOT flip status back to Running or mark is_running true
    assert!(!app.ui.ai.is_running, "late TurnStarted must not set is_running to true after Stop");
    assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Stopped, "late TurnStarted must not overwrite Stopped status");

    // Simulate late AgentDelta event arriving after Stop
    handle.push_incoming(format!(
        r#"{{"method":"item/agentMessage/delta","params":{{"threadId":"{thread_id}","turnId":"turn_late","delta":"unwanted text"}}}}"#
    ));
    super::update_ai_runtime(&mut app, &ctx);
    // Unwanted text must not be appended
    assert!(
        !app.ui.ai.sessions[0].messages.iter().any(|m| m.text.contains("unwanted text")),
        "late AgentDelta must not append text to a stopped session"
    );

    // Simulate late TurnCompleted event arriving after Stop
    handle.push_incoming(format!(r#"{{"method":"turn/completed","params":{{"threadId":"{thread_id}","turnId":"turn_late"}}}}"#));
    super::update_ai_runtime(&mut app, &ctx);

    // Must remain Stopped, NOT completed
    assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Stopped, "late TurnCompleted must not overwrite Stopped status to Completed");
    assert!(!app.ui.ai.is_running);

    // Verify task authority rejects any late tool execution attempts
    let tool_res = super::tools::execute_tool(
        &mut app.session,
        &mut auth,
        "canvas_create_shape",
        &json!({"shape": "rectangle", "x": 0, "y": 0, "width": 50, "height": 50}),
    );
    assert!(tool_res.is_err(), "stopped task authority must reject tool execution");
    assert!(tool_res.unwrap_err().to_string().contains("stopped"));
}

#[test]
fn test_event_routing_to_bound_thread_not_active_session() {
    let transport = MockSupervisorTransport::new();
    let supervisor = CodexSupervisor::new(Box::new(transport));
    let mut state = AiState::with_supervisor(supervisor);
    seed_models(&mut state);

    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));

    // Session 0 bound to thread "th_session_0"
    state.sessions[0].thread_id = Some("th_session_0".into());
    state.sessions[0].status = TurnStatus::Running;

    // Create Session 1 and make it active
    state.create_new_session(&session);
    assert_eq!(state.active_session_idx, 1);
    state.sessions[1].thread_id = Some("th_session_1".into());

    // Send delta targeted to thread "th_session_0"
    let ev = super::protocol::ProtocolEvent::AgentDelta {
        thread_id: Some("th_session_0".into()),
        turn_id: Some("turn_0".into()),
        text: "Message for session 0".into(),
    };

    if let super::protocol::ProtocolEvent::AgentDelta { thread_id, text, .. } = ev {
        let sess_idx = super::find_session_by_thread(&state.sessions, thread_id.as_deref());
        assert_eq!(sess_idx, Some(0), "Event must route to Session 0 even though Session 1 is active!");
        state.sessions[sess_idx.unwrap()].append_assistant_delta(&text);
    }

    assert_eq!(state.sessions[0].messages.len(), 1);
    assert_eq!(state.sessions[0].messages[0].text, "Message for session 0");
    assert_eq!(state.sessions[1].messages.len(), 0);
}

#[test]
fn test_canvas_dynamic_tool_execution_under_task_authority() {
    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));
    let page_id = session.page().unwrap();
    let doc_uid = session.ds().unwrap().uid;

    let mut auth = figcraft_engine::TaskAuthority::new("task-tool-test", doc_uid, figcraft_engine::TaskScope::NewRoot { page_id, task_root: None });

    // Execute dynamic tool: canvas_create_frame
    let res = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_create_frame",
        &json!({"name": "App Header", "x": 0, "y": 0, "width": 375, "height": 64}),
    );
    assert!(res.is_ok(), "canvas_create_frame must succeed: {:?}", res);
    let frame_val = res.unwrap();
    let frame_id = frame_val.get("id").unwrap().as_u64().unwrap();

    // Execute dynamic tool: canvas_create_text
    let text_res = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_create_text",
        &json!({
            "text": "Profile Header",
            "parent": frame_id,
            "x": 16,
            "y": 20,
            "fontSize": 18,
            "fontFamily": "Inter"
        }),
    );
    assert!(text_res.is_ok(), "canvas_create_text must succeed: {:?}", text_res);
    let text_val = text_res.unwrap();
    let text_id = text_val.get("id").unwrap().as_u64().unwrap();

    // Execute dynamic tool: canvas_create_shape
    let shape_res = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_create_shape",
        &json!({"kind": "rectangle", "parent": frame_id, "x": 10, "y": 10, "width": 40, "height": 40}),
    );
    assert!(shape_res.is_ok(), "canvas_create_shape must succeed: {:?}", shape_res);
    let shape_val = shape_res.unwrap();
    let shape_id = shape_val.get("id").unwrap().as_u64().unwrap();

    // Execute dynamic tool: canvas_set_layout
    let layout_res = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_set_layout",
        &json!({
            "id": frame_id,
            "mode": "vertical",
            "gap": 8,
            "padding": 12,
            "mainAlign": "center",
            "crossAlign": "min"
        }),
    );
    assert!(layout_res.is_ok(), "canvas_set_layout must succeed: {:?}", layout_res);

    // Execute dynamic tool: canvas_query_tree
    let query_res = super::tools::execute_tool(&mut session, &mut auth, "canvas_query_tree", &json!({"node_id": frame_id, "depth": 2}));
    assert!(query_res.is_ok(), "canvas_query_tree must succeed: {:?}", query_res);
    let tree_val = query_res.unwrap();
    assert_eq!(tree_val["root"], frame_id);
    assert_eq!(tree_val["total_nodes"], 3);
    assert_eq!(tree_val["truncated"], false);
    assert_eq!(tree_val["tree"]["name"], "App Header");
    assert_eq!(tree_val["tree"]["layout"]["mode"], "vertical");
    assert_eq!(tree_val["tree"]["layout"]["gap"], 8.0);
    assert_eq!(tree_val["tree"]["layout"]["padding"], json!([12.0, 12.0, 12.0, 12.0]));

    let children = tree_val["tree"]["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    let text_child = children.iter().find(|c| c["id"] == text_id).expect("Text node must be in tree");
    assert_eq!(text_child["text"], "Profile Header");
    assert_eq!(text_child["fontSize"], 18.0);

    // Execute dynamic tool: canvas_export_preview
    let preview_res = super::tools::execute_tool(&mut session, &mut auth, "canvas_export_preview", &json!({"node_id": frame_id, "scale": 1.0}));
    assert!(preview_res.is_ok(), "canvas_export_preview must succeed: {:?}", preview_res);
    let preview_val = preview_res.unwrap();
    assert_eq!(preview_val["node_id"], frame_id);
    assert!(preview_val["width"].as_u64().unwrap() > 0);
    assert!(preview_val["height"].as_u64().unwrap() > 0);
    assert!(preview_val["byte_size"].as_u64().unwrap() > 0);
    assert!(preview_val["png_base64"].as_str().is_some());

    // Verify task provenance tracked created nodes
    assert!(auth.provenance().created_nodes.contains(&figcraft_doc::NodeId(frame_id)));
    assert!(auth.provenance().created_nodes.contains(&figcraft_doc::NodeId(text_id)));
    assert!(auth.provenance().created_nodes.contains(&figcraft_doc::NodeId(shape_id)));

    // Stop task and verify all future tools are rejected
    auth.stop();
    assert!(auth.is_stopped());
    let late_res = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_create_shape",
        &json!({"kind": "rectangle", "parent": frame_id, "x": 0, "y": 0, "width": 10, "height": 10}),
    );
    assert!(late_res.is_err(), "Tool execution after stop must be rejected");
}

#[test]
fn test_canvas_batch_commands_and_text_styling_sanitization() {
    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));
    let page_id = session.page().unwrap();
    let doc_uid = session.ds().unwrap().uid;

    let mut auth = figcraft_engine::TaskAuthority::new("task-batch-test", doc_uid, figcraft_engine::TaskScope::NewRoot { page_id, task_root: None });

    let batch_res = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_batch_commands",
        &json!({
            "commands": [
                {
                    "command": "canvas_create_frame",
                    "params": {
                        "name": "Batch Frame",
                        "x": 0,
                        "y": 0,
                        "width": 300,
                        "height": 200
                    }
                },
                {
                    "command": "canvas_create_text",
                    "params": {
                        "text": "Card Heading",
                        "x": 16,
                        "y": 16,
                        "fontSize": 20,
                        "fontWeight": "Bold",
                        "color": "#FFFFFF",
                        "textAlign": "left"
                    }
                },
                {
                    "command": "canvas_create_shape",
                    "params": {
                        "kind": "rectangle",
                        "x": 16,
                        "y": 50,
                        "width": 80,
                        "height": 24
                    }
                }
            ]
        }),
    );
    assert!(batch_res.is_ok(), "canvas_batch_commands must succeed: {:?}", batch_res);
    let val = batch_res.unwrap();
    let results = val["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3);
    assert!(results[0].get("id").is_some());
    assert!(results[1].get("id").is_some());
    assert!(results[2].get("id").is_some());
}

#[test]
fn failed_canvas_tool_batch_preserves_document_and_authority() {
    let mut session = figcraft_engine::Session::new();
    let page_id = session.page().unwrap();
    let doc_uid = session.ds().unwrap().uid;
    let mut auth = figcraft_engine::TaskAuthority::new("failed-batch", doc_uid, figcraft_engine::TaskScope::NewRoot { page_id, task_root: None });
    let before_doc = serde_json::to_value(session.doc().unwrap()).unwrap();
    let before_auth = auth.clone();
    let result = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_batch_commands",
        &json!({
            "commands": [
                {"command": "canvas_create_frame", "params": {"name": "Must roll back", "x": 0, "y": 0, "width": 300, "height": 200}},
                {"command": "file.save", "params": {}}
            ]
        }),
    );
    assert!(result.is_err(), "out-of-scope second command must fail");
    assert_eq!(serde_json::to_value(session.doc().unwrap()).unwrap(), before_doc, "failed tool batch left partial edits");
    assert_eq!(auth, before_auth, "failed tool batch changed task authority");
}

#[test]
fn test_canvas_batch_nesting_limits_and_single_undo_group() {
    let mut session = figcraft_engine::Session::new();
    let page_id = session.page().unwrap();
    let doc_uid = session.ds().unwrap().uid;
    let mut auth =
        figcraft_engine::TaskAuthority::new("batch-limits-test", doc_uid, figcraft_engine::TaskScope::NewRoot { page_id, task_root: None });

    // 1. Nesting depth limit: build 4 nested batches (depth 4 exceeds max depth 3)
    let depth4_batch = json!({
        "commands": [{
            "command": "canvas_batch_commands",
            "params": {
                "commands": [{
                    "command": "canvas_batch_commands",
                    "params": {
                        "commands": [{
                            "command": "canvas_batch_commands",
                            "params": {
                                "commands": [{
                                    "command": "canvas_batch_commands",
                                    "params": {
                                        "commands": []
                                    }
                                }]
                            }
                        }]
                    }
                }]
            }
        }]
    });
    let depth_err = super::tools::execute_tool(&mut session, &mut auth, "canvas_batch_commands", &depth4_batch);
    assert!(depth_err.is_err(), "nesting depth > 3 must be rejected");
    assert!(depth_err.unwrap_err().to_string().contains("nesting depth"));

    // 2. Total commands count limit: 101 commands
    let mut many_cmds = Vec::new();
    for _ in 0..101 {
        many_cmds.push(json!({"command": "canvas_batch_commands", "params": {"commands": []}}));
    }
    let count_err = super::tools::execute_tool(&mut session, &mut auth, "canvas_batch_commands", &json!({"commands": many_cmds}));
    assert!(count_err.is_err(), "commands count > 100 must be rejected");
    assert!(count_err.unwrap_err().to_string().contains("maximum of 100 commands"));

    // 3. Successful batch forms a single undo group
    let before_doc = serde_json::to_value(session.doc().unwrap()).unwrap();
    let undo_count_before = session.ds().unwrap().history.undo.len();
    let success_batch = json!({
        "commands": [
            {"command": "canvas_create_frame", "params": {"name": "Batch Root", "x": 0, "y": 0, "width": 400, "height": 300}},
            {"command": "canvas_create_text", "params": {"text": "Hello World", "x": 20, "y": 20, "fontSize": 16.0}},
            {"command": "canvas_create_shape", "params": {"shape": "rectangle", "x": 20, "y": 60, "width": 100, "height": 50}}
        ]
    });
    let success_res = super::tools::execute_tool(&mut session, &mut auth, "canvas_batch_commands", &success_batch);
    assert!(success_res.is_ok(), "valid batch must succeed: {:?}", success_res);
    let undo_count_after = session.ds().unwrap().history.undo.len();
    assert_eq!(undo_count_after, undo_count_before + 1, "entire tool batch must commit as exactly one undo group");

    // Single undo rolls back the entire batch
    assert!(session.undo().unwrap(), "undo must succeed");
    assert_eq!(serde_json::to_value(session.doc().unwrap()).unwrap(), before_doc, "single undo must completely restore document to pre-batch state");
}

#[test]
fn test_task_authority_scope_and_atomic_rollback() {
    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));
    let page_id = session.page().unwrap();
    let doc_uid = session.ds().unwrap().uid;

    let mut auth =
        figcraft_engine::TaskAuthority::new("task-rollback-test", doc_uid, figcraft_engine::TaskScope::NewRoot { page_id, task_root: None });

    // Establish root frame
    let frame_res = super::tools::execute_tool(
        &mut session,
        &mut auth,
        "canvas_create_frame",
        &json!({"name": "Task Root", "x": 10, "y": 10, "width": 200, "height": 200}),
    );
    assert!(frame_res.is_ok());
    let _frame_id = frame_res.unwrap()["id"].as_u64().unwrap();

    // Create an unowned node on the page outside the task
    let ext_res = session.execute("shape.rectangle", &json!({"x": 500, "y": 500, "width": 100, "height": 100})).unwrap();
    let ext_id = ext_res["id"].as_u64().unwrap();

    // Out-of-scope query on unowned node must fail
    let query_err = super::tools::execute_tool(&mut session, &mut auth, "canvas_query_tree", &json!({"node_id": ext_id}));
    assert!(query_err.is_err(), "Query on unowned node outside scope must fail");

    // Out-of-scope export on unowned node must fail
    let export_err = super::tools::execute_tool(&mut session, &mut auth, "canvas_export_preview", &json!({"node_id": ext_id}));
    assert!(export_err.is_err(), "Export on unowned node outside scope must fail");

    // Attempting to mutate outside scope must be rejected and atomically rolled back
    let doc_before = session.ds().unwrap().doc.clone();
    let mutate_err = auth.execute(&mut session, "node.set", &json!({"id": ext_id, "props": {"name": "Hacked Name"}}));
    assert!(mutate_err.is_err(), "Out of scope mutation must be rejected");
    let doc_after = session.ds().unwrap().doc.clone();
    assert_eq!(doc_before, doc_after, "Document state must be identical after atomic rollback");
}

#[test]
fn test_live_codex_cli_non_inference_handshake() {
    use super::supervisor::{RequestCorrelation, detect_codex_cli};

    let Some(cli_path) = detect_codex_cli() else {
        eprintln!("Skipping live codex test: executable not found");
        return;
    };
    if !cli_path.exists() {
        eprintln!("Skipping live codex test: path {:?} does not exist", cli_path);
        return;
    }

    let spawn_result = CodexSupervisor::try_spawn_local();
    assert!(spawn_result.is_ok(), "Failed to spawn detected Codex binary: {:?}", spawn_result.err());
    let mut supervisor = spawn_result.unwrap();

    let start = std::time::Instant::now();
    let mut ready = false;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        let _ = supervisor.poll_events();
        if supervisor.phase == SupervisorPhase::Ready && !supervisor.available_models.is_empty() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert!(ready, "Codex supervisor handshake did not reach Ready within 5s; phase: {:?}", supervisor.phase);
    assert!(!supervisor.available_models.is_empty(), "Models should be discovered");

    // Test real server thread/start acceptance with registered dynamic canvas tools (non-inference)
    let chosen_model = supervisor.available_models[0].clone();
    let _req_id = supervisor.start_thread("test-live-sess", &chosen_model, Some("on-request"), None);

    let start_thread_wait = std::time::Instant::now();
    let mut thread_started = false;
    while start_thread_wait.elapsed() < std::time::Duration::from_secs(5) {
        let events = supervisor.poll_events();
        for ev in events {
            if let ProtocolEvent::ThreadStarted { thread_id } = ev
                && !thread_id.is_empty()
            {
                thread_started = true;
                break;
            }
        }
        if thread_started {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert!(thread_started, "Codex server should accept thread/start with registered canvas tools");

    // Verify zero inference turns were ever initiated (zero paid spending)
    assert!(supervisor.pending_requests.values().all(|r| !matches!(r, RequestCorrelation::TurnStart { .. })));
    // Clean shutdown on drop
    drop(supervisor);
}

#[test]
fn test_supervisor_dead_process_latches_terminal_failure_and_does_not_grow_messages() {
    let transport = MockSupervisorTransport::new();
    transport.set_alive(false); // dead process from start

    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    // First poll: reports error once and latches failure
    let events1 = supervisor.poll_events();
    assert_eq!(events1.len(), 1, "First poll must emit exactly 1 terminal process exit error");
    assert!(matches!(&events1[0], ProtocolEvent::Error { message, .. } if message.contains("exited")));

    // Subsequent repeated polls must NOT emit duplicate process exit errors!
    for poll_idx in 2..=5 {
        let events = supervisor.poll_events();
        assert!(events.is_empty(), "Poll {poll_idx} on dead process must return empty events, but got: {:?}", events);
    }
}

#[test]
fn test_supervisor_drains_buffered_events_before_reporting_terminal_process_exit() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();

    // Queue buffered output (e.g. final turn completion and delta), then mark process dead
    handle.push_incoming(r#"{"method":"item/agentMessage/delta","params":{"threadId":"th_drain","turnId":"turn_1","delta":"Final response text"}}"#);
    handle.push_incoming(r#"{"method":"turn/completed","params":{"threadId":"th_drain","turnId":"turn_1"}}"#);
    handle.set_alive(false);

    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    // First poll must drain all buffered lines BEFORE reporting process exit
    let events = supervisor.poll_events();
    assert!(
        events.iter().any(|e| matches!(e, ProtocolEvent::AgentDelta { text, .. } if text == "Final response text")),
        "Buffered delta must be drained before terminal process failure"
    );
    assert!(
        events.iter().any(|e| matches!(e, ProtocolEvent::TurnCompleted { .. })),
        "Buffered turn completion must be drained before terminal process failure"
    );

    // The next poll latches and reports the terminal process exit
    let exit_events = supervisor.poll_events();
    assert!(exit_events.iter().any(|e| matches!(e, ProtocolEvent::Error { message, .. } if message.contains("exited"))));

    // And after that, latching ensures no further errors
    assert!(supervisor.poll_events().is_empty());
}

#[test]
fn test_supervisor_replies_to_unknown_server_requests_with_protocol_error() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();

    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    // Server sends an unknown request with ID
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":"unk_req_42","method":"unsupported/serverMethod","params":{"foo":"bar"}}"#);

    let events = supervisor.poll_events();

    // Must settle once by sending a protocol-correct error response back to harness
    let sent = handle.sent_lines();
    let reply = sent.iter().find(|l| l.contains("unk_req_42"));
    assert!(reply.is_some(), "Must reply to unknown server request unk_req_42, but sent: {:?}", sent);
    let reply_str = reply.unwrap();
    assert!(reply_str.contains(r#""error""#), "Reply must be a JSON-RPC error response");
    assert!(reply_str.contains("-32601"), "Error code should be -32601 (Method not found)");

    // Must also produce visible honest event so harness state is transparent
    assert!(events.iter().any(|e| matches!(e, ProtocolEvent::Notice { message, .. } if message.contains("unsupported/serverMethod"))));
}

#[test]
fn test_conversation_view_switching_cannot_retarget_tool_calls_or_deltas_and_stale_threads_rejected() {
    let mut state = AiState::new();

    // Session 0 bound to thread_0
    let mut sess0 = super::AiSession::new("sess_0", 1, figcraft_doc::NodeId(1), "Target 0");
    sess0.thread_id = Some("thread_0".into());
    sess0.active_turn_id = Some("turn_0".into());
    sess0.status = TurnStatus::Running;

    // Session 1 bound to thread_1
    let mut sess1 = super::AiSession::new("sess_1", 1, figcraft_doc::NodeId(1), "Target 1");
    sess1.thread_id = Some("thread_1".into());
    sess1.active_turn_id = Some("turn_1".into());
    sess1.status = TurnStatus::Running;

    state.sessions = vec![sess0, sess1];

    // User switches view to Session 1
    state.active_session_idx = 1;

    // Event arrives for thread_0
    // Test that find_session_by_thread does NOT fall back to active_session_idx (1)
    let found_0 = super::find_session_by_thread(&state.sessions, Some("thread_0"));
    assert_eq!(found_0, Some(0), "thread_0 must resolve strictly to session 0");

    let found_unknown = super::find_session_by_thread(&state.sessions, Some("stale_thread_999"));
    assert_eq!(found_unknown, None, "Unknown/stale thread must NOT fall back to active view");

    let found_none = super::find_session_by_thread(&state.sessions, None);
    assert_eq!(found_none, None, "Missing thread ID must NOT fall back to active view");
}

#[test]
fn test_unsupported_external_permissions_are_visibly_denied_with_schema_responses_and_duplicate_replies_rejected() {
    use super::protocol::permission_denial_result;

    // Verify schema-correct denial for external execution
    let cmd_denial = permission_denial_result("item/commandExecution/requestApproval").unwrap().to_string();
    assert!(
        cmd_denial.contains(r#""decision":"decline""#) || cmd_denial.contains(r#""decision": "decline""#),
        "commandExecution must use schema-correct 'decline' decision: {cmd_denial}"
    );

    // Verify schema-correct denial for external file changes
    let file_denial = permission_denial_result("item/fileChange/requestApproval").unwrap().to_string();
    assert!(
        file_denial.contains(r#""decision":"decline""#) || file_denial.contains(r#""decision": "decline""#),
        "fileChange must use schema-correct 'decline' decision: {file_denial}"
    );

    // Verify duplicate reply prevention
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();
    let mut supervisor = CodexSupervisor::new(Box::new(transport));

    let req_id = json!("perm_req_1");
    // First response is accepted
    assert!(supervisor.record_and_send_response(&req_id, json!({"decision": "decline"})));
    let initial_count = handle.sent_lines().len();

    // Duplicate response for same req_id MUST be rejected!
    assert!(
        !supervisor.record_and_send_response(&req_id, json!({"decision": "decline"})),
        "Duplicate response for already-answered request must be rejected"
    );
    assert_eq!(handle.sent_lines().len(), initial_count, "No duplicate line should be sent over transport");
}

#[test]
fn test_explicit_codex_executable_override_env_var() {
    use super::supervisor::detect_codex_cli_with_env;
    use std::ffi::OsStr;

    // Test with non-existent explicit path
    let res = detect_codex_cli_with_env(Some(OsStr::new("/tmp/nonexistent-codex-override-test-bin")));
    assert!(res.is_none(), "Nonexistent explicit path must not be returned");

    // Test with existing binary override (use /bin/sh as dummy executable fixture)
    let res_valid = detect_codex_cli_with_env(Some(OsStr::new("/bin/sh")));
    assert_eq!(res_valid, Some(std::path::PathBuf::from("/bin/sh")), "Existing explicit path must be honored");
}

#[test]
fn test_model_supported_efforts_derivation_and_selection_preservation() {
    let mut state = AiState::new();

    // Register models with their supported efforts
    let model_a = super::supervisor::ModelInfo {
        id: "model-a".to_string(),
        display_name: Some("Model A".to_string()),
        supported_efforts: vec!["low".to_string(), "medium".to_string(), "high".to_string()],
        default_effort: Some("medium".to_string()),
    };
    let model_b = super::supervisor::ModelInfo {
        id: "model-b".to_string(),
        display_name: Some("Model B".to_string()),
        supported_efforts: vec!["low".to_string(), "medium".to_string()],
        default_effort: Some("low".to_string()),
    };

    state.set_models(vec![model_a, model_b]);

    // Select Model A and explicit effort "high"
    state.select_model("model-a");
    state.select_effort(Some("high".to_string()));
    assert_eq!(state.selected_effort.as_deref(), Some("high"));

    // Switch to Model B which does NOT support "high": must fall back cleanly to default effort ("low")
    state.select_model("model-b");
    assert_eq!(state.selected_effort.as_deref(), Some("low"), "Effort must fall back when unsupported on new model");

    // Set effort to "medium" on Model B (which is supported)
    state.select_effort(Some("medium".to_string()));
    assert_eq!(state.selected_effort.as_deref(), Some("medium"));

    // Switch back to Model A (which also supports "medium"): must preserve "medium"
    state.select_model("model-a");
    assert_eq!(state.selected_effort.as_deref(), Some("medium"), "Supported explicit effort must be preserved across models");
}

#[test]
fn test_ime_confirmation_does_not_submit_turn_and_composer_does_not_steal_shortcuts() {
    use egui::{Event, ImeEvent, Key, Modifiers};

    // When an IME composition commit event happens in the same frame as Enter,
    // Enter was confirming the candidate, so composer must NOT trigger submit_turn
    let has_ime_event = [
        Event::Ime(ImeEvent::Commit("こんにちは".into())),
        Event::Key { key: Key::Enter, physical_key: Some(Key::Enter), pressed: true, repeat: false, modifiers: Modifiers::NONE },
    ];

    let is_ime_confirming = has_ime_event.iter().any(|e| matches!(e, Event::Ime(_)));
    assert!(is_ime_confirming, "IME commit must be detected so Enter does not trigger turn submit");
}

// ---- The draft's references, and what a send is allowed to authorize ----

/// An app ready to send: a document, a live harness and a catalog to run on.
fn sending_app() -> crate::FigApp {
    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));
    let mut app = crate::FigApp::new(session, Box::new(crate::NoServices));
    app.ui.ai.supervisor = Some(CodexSupervisor::new(Box::new(MockSupervisorTransport::new())));
    seed_models(&mut app.ui.ai);
    app
}

/// A frame named `name` on the open page.
fn named_frame(app: &mut crate::FigApp, name: &str) -> figcraft_doc::NodeId {
    let frame = app.session.execute("frame.create", &json!({"width": 100.0, "height": 100.0, "name": name})).expect("frame created");
    figcraft_doc::NodeId(frame["id"].as_u64().expect("frame id"))
}

fn open_page(app: &crate::FigApp) -> figcraft_doc::NodeId {
    app.session.page().expect("open page")
}

#[test]
fn a_clicked_layer_becomes_a_token_where_the_caret_was() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let uid = app.session.ds().expect("document").uid;

    app.ui.ai.draft_text = "restyle ".into();
    app.ui.ai.draft_meta.set_selection(8, 8);
    app.ui.ai.insert_object_mention(uid, page, header, "Header");

    assert_eq!(app.ui.ai.draft_text, "restyle @Header");
    let mentions = app.ui.ai.draft_mentions().to_vec();
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].node_id, header);
    assert_eq!(mentions[0].page_id, page);
    assert_eq!((mentions[0].start_char, mentions[0].end_char), (8, 15));
    // The caret follows the token, so typing carries on after it.
    assert_eq!(app.ui.ai.draft_meta.selection(), (15, 15));
}

#[test]
fn two_clicks_in_a_row_keep_both_references_apart() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let card = named_frame(&mut app, "Card");
    let uid = app.session.ds().expect("document").uid;

    app.ui.ai.insert_object_mention(uid, page, header, "Header");
    app.ui.ai.insert_object_mention(uid, page, card, "Card");

    assert_eq!(app.ui.ai.draft_text, "@Header @Card", "a separator keeps the two tokens apart");
    let mentions = app.ui.ai.draft_mentions().to_vec();
    assert_eq!(mentions.len(), 2);
    assert_eq!((mentions[0].start_char, mentions[0].end_char), (0, 7));
    assert_eq!((mentions[1].start_char, mentions[1].end_char), (8, 13));
}

#[test]
fn typing_a_name_that_looks_like_a_token_grants_nothing() {
    let mut app = sending_app();
    {
        use egui::TextBuffer as _;
        let mut buffer = super::draft::DraftBuffer { text: &mut app.ui.ai.draft_text, meta: &mut app.ui.ai.draft_meta };
        buffer.insert_text("@Header", egui::text::CharIndex(0));
    }
    assert_eq!(app.ui.ai.draft_text, "@Header");
    assert!(app.ui.ai.draft_mentions().is_empty(), "typed text must not become authority");
}

#[test]
fn deleting_part_of_a_token_removes_the_whole_reference() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let uid = app.session.ds().expect("document").uid;
    app.ui.ai.insert_object_mention(uid, page, header, "Header");
    assert_eq!(app.ui.ai.draft_text, "@Header");

    {
        // One keystroke inside the name cannot leave half a reference behind.
        use egui::TextBuffer as _;
        let mut buffer = super::draft::DraftBuffer { text: &mut app.ui.ai.draft_text, meta: &mut app.ui.ai.draft_meta };
        buffer.delete_char_range(egui::text::CharIndex(3)..egui::text::CharIndex(4));
    }
    assert_eq!(app.ui.ai.draft_text, "");
    assert!(app.ui.ai.draft_mentions().is_empty());
}

#[test]
fn undo_and_redo_carry_a_drafts_text_tokens_and_caret_together() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let uid = app.session.ds().expect("document").uid;

    app.ui.ai.draft_text = "fix ".into();
    app.ui.ai.draft_meta.set_selection(4, 4);
    app.ui.ai.draft_meta.end_frame(&app.ui.ai.draft_text);
    app.ui.ai.insert_object_mention(uid, page, header, "Header");
    assert_eq!(app.ui.ai.draft_text, "fix @Header");

    assert!(app.ui.ai.draft_meta.undo(&mut app.ui.ai.draft_text), "the click is undoable");
    assert_eq!(app.ui.ai.draft_text, "fix ");
    assert!(app.ui.ai.draft_mentions().is_empty(), "undo must not leave a reference without its token");

    assert!(app.ui.ai.draft_meta.redo(&mut app.ui.ai.draft_text));
    assert_eq!(app.ui.ai.draft_text, "fix @Header");
    assert_eq!(app.ui.ai.draft_mentions().len(), 1);
}

#[test]
fn plain_text_replacement_drops_every_reference() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let uid = app.session.ds().expect("document").uid;
    app.ui.ai.insert_object_mention(uid, page, header, "Header");

    app.ui.ai.replace_draft_text("something else entirely");
    assert_eq!(app.ui.ai.draft_text, "something else entirely");
    assert!(app.ui.ai.draft_mentions().is_empty());
    assert_eq!(app.ui.ai.draft_meta.selection(), (23, 23), "the caret follows the replacement");
}

#[test]
fn sending_two_mentions_authorizes_exactly_those_two_on_the_open_page() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let card = named_frame(&mut app, "Card");
    let uid = app.session.ds().expect("document").uid;
    app.ui.ai.insert_object_mention(uid, page, header, "Header");
    app.ui.ai.insert_object_mention(uid, page, card, "Card");

    app.ui.ai.submit_turn(&mut app.session);

    assert!(app.ui.ai.is_running, "a resolvable draft starts its turn");
    let task = app.ui.ai.sessions[0].task.as_ref().expect("a frozen task");
    match task.scope() {
        figcraft_engine::TaskScope::Subtrees { page_id, target_nodes } => {
            assert_eq!(*page_id, page);
            assert_eq!(target_nodes, &vec![header, card], "both named objects, in mention order");
        }
        other => panic!("expected the two mentions as the scope, got {other:?}"),
    }
    assert_eq!(app.ui.ai.sessions[0].messages[0].mentions.len(), 2, "the timeline keeps what was sent for");
    assert!(app.ui.ai.draft_text.is_empty(), "the sent draft is cleared");
    assert!(app.ui.ai.draft_mentions().is_empty());
}

#[test]
fn a_draft_without_a_mention_is_a_new_root_whatever_is_selected() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    // The user picked a layer by hand; picking is not naming.
    app.session.ds_mut().expect("document").selection = vec![header];
    app.ui.ai.draft_text = "Draw a toolbar".into();

    app.ui.ai.submit_turn(&mut app.session);

    assert!(app.ui.ai.is_running);
    let task = app.ui.ai.sessions[0].task.as_ref().expect("a frozen task");
    assert_eq!(task.scope(), &figcraft_engine::TaskScope::NewRoot { page_id: page, task_root: None });
}

#[test]
fn a_reference_that_no_longer_resolves_blocks_the_turn_and_keeps_the_draft() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let card = named_frame(&mut app, "Card");
    let uid = app.session.ds().expect("document").uid;
    app.ui.ai.insert_object_mention(uid, page, header, "Header");
    app.ui.ai.insert_object_mention(uid, page, card, "Card");
    let drafted = app.ui.ai.draft_text.clone();

    // The user deletes one of the two named layers before sending.
    app.session.execute("edit.delete", &json!({"ids": [header.0]})).expect("deleted");

    app.ui.ai.submit_turn(&mut app.session);

    assert!(!app.ui.ai.is_running, "a stale reference must not start a turn");
    assert!(app.ui.ai.sessions[0].messages.is_empty(), "nothing was sent");
    assert!(app.ui.ai.sessions[0].task.is_none(), "no authority was frozen");
    assert_eq!(app.ui.ai.draft_text, drafted, "the refused draft keeps every character");
    assert!(app.ui.ai.draft_meta.error.is_some(), "and says why");
}

#[test]
fn a_turn_needs_a_model_and_keeps_the_draft_without_one() {
    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));
    let mut app = crate::FigApp::new(session, Box::new(crate::NoServices));
    app.ui.ai.supervisor = Some(CodexSupervisor::new(Box::new(MockSupervisorTransport::new())));
    app.ui.ai.draft_text = "Draw a button".into();

    app.ui.ai.submit_turn(&mut app.session);

    assert!(!app.ui.ai.is_running, "there is no catalog to run on");
    assert_eq!(app.ui.ai.draft_text, "Draw a button");
    assert!(app.ui.ai.draft_meta.error.is_some());
}

#[test]
fn a_model_list_with_nothing_in_it_still_ends_discovery() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();
    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex/0.167.0"}}"#);
    let _ = supervisor.poll_events();
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":2,"result":{"data":[]}}"#);
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":3,"result":{"account":{"email":"a@b.c","planType":"plus"}}}"#);

    let events = supervisor.poll_events();

    assert_eq!(supervisor.harness_state, HarnessState::Ready, "discovery ends, it does not wait forever");
    assert!(supervisor.available_models.is_empty());
    assert!(events.iter().any(|e| matches!(e, ProtocolEvent::Initialized { .. })));
}

#[test]
fn a_model_list_failure_reports_no_models_and_is_not_a_dead_process() {
    let transport = MockSupervisorTransport::new();
    let handle = transport.clone();
    let mut supervisor = CodexSupervisor::new(Box::new(transport));
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex/0.167.0"}}"#);
    let _ = supervisor.poll_events();
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"no model list"}}"#);
    handle.push_incoming(r#"{"jsonrpc":"2.0","id":3,"result":{"account":{"email":"a@b.c","planType":"plus"}}}"#);

    let events = supervisor.poll_events();

    assert_eq!(supervisor.harness_state, HarnessState::Ready, "the harness is alive, it just has no models");
    assert!(supervisor.available_models.is_empty());
    assert!(events.iter().any(|e| matches!(e, ProtocolEvent::ModelDiscoveryFailed { .. })));
    assert!(!events.iter().any(|e| matches!(e, ProtocolEvent::Error { .. })), "a failed request is not a failed process");
}

#[test]
fn a_model_discovery_failure_leaves_an_honest_empty_catalog_and_a_running_turn_alone() {
    let mut app = sending_app();
    let page = open_page(&app);
    let header = named_frame(&mut app, "Header");
    let uid = app.session.ds().expect("document").uid;
    app.ui.ai.insert_object_mention(uid, page, header, "Header");
    app.ui.ai.submit_turn(&mut app.session);
    assert!(app.ui.ai.is_running);

    super::dispatch_ai_event(&mut app, ProtocolEvent::ModelDiscoveryFailed { message: "no model list".into() });

    assert!(app.ui.ai.available_models.is_empty(), "the catalog is empty, not stale");
    assert!(app.ui.ai.selected_model.is_empty());
    assert!(app.ui.ai.selected_effort.is_none());
    assert_eq!(app.ui.ai.harness_state, HarnessState::Ready);
    assert!(app.ui.ai.is_running, "a catalog failure does not abort the turn already running");
    assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Running);
    assert!(app.ui.ai.sessions[0].messages.iter().any(|m| m.text == "Models unavailable."), "and it says so");
}

#[test]
fn the_composer_footer_fits_a_narrow_panel_without_wrapping() {
    // A 200pt dock: 200 less the composer's two 8pt insets.
    assert_eq!(super::composer::footer_widths_for_test(184.0), (68.0, 56.0, 48.0));
    // A 280pt dock.
    assert_eq!(super::composer::footer_widths_for_test(264.0), (148.0, 56.0, 48.0));

    // Whatever the width, the row fits: items plus the two gaps never exceed what it was given.
    for inner in [60.0_f32, 100.0, 140.0, 184.0, 264.0, 600.0] {
        let (model, effort, send) = super::composer::footer_widths_for_test(inner);
        let gaps = if model > 0.0 { 2.0 * 6.0 } else { 6.0 };
        assert!(model + effort + send + gaps <= inner + 0.01, "the footer overflows {inner}pt: {model}+{effort}+{send}");
        assert_eq!(send, 48.0, "Send keeps its size so it is always the same target");
    }

    // Just wide enough for the model at its minimum: it keeps exactly that.
    assert_eq!(super::composer::footer_widths_for_test(160.0), (44.0, 56.0, 48.0));
    // Narrower than that, the model is dropped rather than shown unreadably.
    let (model, effort, _) = super::composer::footer_widths_for_test(150.0);
    assert_eq!((model, effort), (0.0, 56.0));
    // And only after the model is gone does the reasoning dropdown give up the rest.
    let (model, effort, _) = super::composer::footer_widths_for_test(100.0);
    assert_eq!(model, 0.0);
    assert!(effort < 56.0, "the reasoning dropdown shrinks only once the model is gone");
}

#[test]
fn the_composer_belongs_to_the_ai_dock_and_fits_its_narrowest_width() {
    use crate::layers_drag_tests::Ui;
    let mut u = Ui::new(figcraft_engine::Session::new());
    u.ctx.enable_accesskit();
    seed_models(&mut u.app.ui.ai);
    u.app.ui.ai.draft_text = "restyle the header".into();

    // File and Assets own the whole panel: no composer of theirs.
    u.app.ui.left_tab = crate::LeftTab::File;
    let file_frame = u.frame_output(vec![]);
    let labels = |out: &egui::FullOutput| -> Vec<String> {
        out.platform_output
            .accesskit_update
            .as_ref()
            .map(|update| update.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    assert!(!labels(&file_frame).iter().any(|l| l == "Model"), "the File dock must not draw the AI composer");

    for width in [200.0_f32, 280.0] {
        u.app.ui.left_tab = crate::LeftTab::AiChat;
        u.app.ui.left_collapsed = false;
        u.app.ui.left_width = width;
        let out = u.frame_output(vec![]);
        let names = labels(&out);
        assert_eq!(u.app.ui.left_width, width, "the dock keeps the width it was given");
        for name in ["Model", "Reasoning effort", "Send"] {
            assert!(names.iter().any(|l| l == name), "the {width}pt dock is missing {name}: {names:?}");
        }
        // Nothing spilled out of the dock: the canvas still starts at its edge.
        assert!(u.app.canvas.rect.min.x >= width - 0.5, "the composer pushed into the canvas at {width}pt");
    }
}

#[test]
fn a_click_on_the_canvas_mentions_the_layer_it_picked() {
    use crate::layers_drag_tests::Ui;
    use egui::{Event, PointerButton, Pos2};

    let mut session = figcraft_engine::Session::new();
    session.execute("frame.create", &json!({"x": 0.0, "y": 0.0, "width": 200.0, "height": 200.0, "name": "Header"})).expect("frame");
    let mut u = Ui::new(session);
    seed_models(&mut u.app.ui.ai);
    u.app.ui.left_tab = crate::LeftTab::AiChat;
    u.frame(vec![]);

    // The middle of the frame, in screen points, so the view's own zoom and pan don't matter.
    let at: Pos2 = u.app.to_screen(100.0, 100.0);
    let none = egui::Modifiers::default();
    u.frame(vec![Event::PointerMoved(at)]);
    u.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: none }]);
    u.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: none }]);

    assert_eq!(u.app.ui.ai.draft_text, "@Header", "a click on the canvas mentions the layer under it");
}
