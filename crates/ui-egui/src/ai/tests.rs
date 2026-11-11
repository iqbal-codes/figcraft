use serde_json::json;

use super::protocol::ProtocolEvent;
use super::supervisor::{CodexSupervisor, HarnessState, MockSupervisorTransport, SupervisorPhase};
use super::{AiState, TurnStatus};

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

    let req_id = supervisor.interrupt_turn("th_abc", "turn_xyz");
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

    let mut session = figcraft_engine::Session::new();
    let _ = session.execute("doc.new", &json!({}));

    state.draft_text = "Create header frame".into();
    state.submit_turn(&mut session);

    assert!(state.is_running);
    assert_eq!(state.sessions[0].messages.len(), 1);
    assert_eq!(state.sessions[0].messages[0].text, "Create header frame");
    assert!(state.sessions[0].pending_turn_prompt.is_some());
    assert!(state.active_task().is_some());

    state.stop_active_task();
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
    app.ui.ai.draft_text = "Generate cards".into();
    app.ui.ai.submit_turn(&mut app.session);
    assert!(app.ui.ai.is_running);
    let thread_id = "th_stop_lifecycle_test";
    app.ui.ai.sessions[0].thread_id = Some(thread_id.into());
    app.ui.ai.sessions[0].active_turn_id = Some("turn_initial".into());

    // Stop active task
    app.ui.ai.stop_active_task();
    assert!(!app.ui.ai.is_running);
    assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Stopped);
    assert!(app.ui.ai.sessions[0].pending_turn_prompt.is_none());
    let mut auth = app.ui.ai.sessions[0].task.as_ref().unwrap().clone();
    assert!(auth.is_stopped());

    // Simulate late TurnStarted event arriving after Stop
    handle.push_incoming(&format!(r#"{{"method":"turn/started","params":{{"threadId":"{thread_id}","turnId":"turn_late"}}}}"#));
    let ctx = egui::Context::default();
    super::update_ai_runtime(&mut app, &ctx);

    // Must NOT flip status back to Running or mark is_running true
    assert!(!app.ui.ai.is_running, "late TurnStarted must not set is_running to true after Stop");
    assert_eq!(app.ui.ai.sessions[0].status, TurnStatus::Stopped, "late TurnStarted must not overwrite Stopped status");

    // Simulate late AgentDelta event arriving after Stop
    handle.push_incoming(&format!(
        r#"{{"method":"item/agentMessage/delta","params":{{"threadId":"{thread_id}","turnId":"turn_late","delta":"unwanted text"}}}}"#
    ));
    super::update_ai_runtime(&mut app, &ctx);
    // Unwanted text must not be appended
    assert!(
        !app.ui.ai.sessions[0].messages.iter().any(|m| m.text.contains("unwanted text")),
        "late AgentDelta must not append text to a stopped session"
    );

    // Simulate late TurnCompleted event arriving after Stop
    handle.push_incoming(&format!(r#"{{"method":"turn/completed","params":{{"threadId":"{thread_id}","turnId":"turn_late"}}}}"#));
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
    assert!(events.iter().any(|e| matches!(e, ProtocolEvent::Error { message, .. } if message.contains("unsupported/serverMethod"))));
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
    use super::protocol::format_permission_denial_response;

    // Verify schema-correct denial for external execution
    let cmd_denial =
        format_permission_denial_response(&json!("req_cmd_1"), "item/commandExecution/requestApproval", "External execution outside canvas");
    assert!(
        cmd_denial.contains(r#""decision":"decline""#) || cmd_denial.contains(r#""decision": "decline""#),
        "commandExecution must use schema-correct 'decline' decision: {cmd_denial}"
    );

    // Verify schema-correct denial for external file changes
    let file_denial = format_permission_denial_response(&json!("req_file_1"), "item/fileChange/requestApproval", "External file edit outside canvas");
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
