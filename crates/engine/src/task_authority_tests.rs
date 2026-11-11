use serde_json::json;

use crate::task_authority::{TaskAuthority, TaskScope};
use crate::{NodeId, Session};

fn id_of(value: &serde_json::Value) -> NodeId {
    crate::params::id_of(&value["id"]).unwrap()
}

#[test]
fn new_root_authorizes_creation_and_descendants_but_rejects_siblings() {
    let mut s = Session::new();
    let page_id = s.page().unwrap();

    // Create a pre-existing sibling before the task starts
    let pre_existing_sibling = id_of(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 100, "height": 100})).unwrap());

    let doc_uid = s.ds().unwrap().uid;
    let mut task = TaskAuthority::new("task-test-01", doc_uid, TaskScope::NewRoot { page_id, task_root: None });

    // Step 1: Create a new root frame on page_id - should succeed and register task_root
    let root_res = task.execute(&mut s, "frame.create", &json!({"x": 200, "y": 0, "width": 300, "height": 400, "name": "App Screen"}));
    assert!(root_res.is_ok(), "Creating new root should succeed: {:?}", root_res);
    let new_root_id = id_of(&root_res.unwrap());

    match task.scope() {
        TaskScope::NewRoot { task_root, .. } => {
            assert_eq!(*task_root, Some(new_root_id), "Task root must be registered");
        }
        _ => panic!("Expected NewRoot scope"),
    }
    assert!(task.provenance().created_nodes.contains(&new_root_id));

    // Step 2: Add child to the task-owned root - should succeed
    let child_res = task.execute(&mut s, "shape.rectangle", &json!({"parent": new_root_id.0, "x": 10, "y": 10, "width": 80, "height": 40}));
    assert!(child_res.is_ok(), "Adding child to task root should succeed: {:?}", child_res);
    let child_id = id_of(&child_res.unwrap());
    assert!(task.provenance().created_nodes.contains(&child_id));

    // Step 3: Attempt to mutate the pre-existing sibling - must be REJECTED!
    let sibling_mutate = task.execute(&mut s, "node.set", &json!({"id": pre_existing_sibling.0, "props": {"name": "Hacked Sibling"}}));
    assert!(sibling_mutate.is_err(), "Mutating sibling outside task scope must fail");
    let err_msg = sibling_mutate.unwrap_err().to_string();
    assert!(err_msg.contains("out of scope"), "Error should mention out of scope: {err_msg}");

    // Sibling name must NOT have changed
    let sibling_node = s.doc().unwrap().node(pre_existing_sibling).unwrap();
    assert_ne!(sibling_node.name, "Hacked Sibling");
}

#[test]
fn subtree_authorizes_target_and_descendants_but_rejects_siblings_and_ancestors() {
    let mut s = Session::new();
    let page_id = s.page().unwrap();

    let frame_a = id_of(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 100, "height": 100, "name": "Frame A"})).unwrap());
    let frame_b = id_of(&s.execute("frame.create", &json!({"x": 200, "y": 0, "width": 100, "height": 100, "name": "Frame B"})).unwrap());

    let doc_uid = s.ds().unwrap().uid;
    let mut task = TaskAuthority::new("task-test-02", doc_uid, TaskScope::Subtree { page_id, target_node: frame_b });

    // Mutating authorized target frame_b should succeed
    let edit_b = task.execute(&mut s, "node.set", &json!({"id": frame_b.0, "props": {"name": "Updated Frame B"}}));
    assert!(edit_b.is_ok());
    assert!(task.provenance().modified_nodes.contains(&frame_b));

    // Adding child inside frame_b should succeed
    let add_child = task.execute(&mut s, "shape.rectangle", &json!({"parent": frame_b.0, "x": 10, "y": 10, "width": 20, "height": 20}));
    assert!(add_child.is_ok());

    // Attempting to mutate frame_a (sibling) must fail
    let edit_a = task.execute(&mut s, "node.set", &json!({"id": frame_a.0, "props": {"name": "Should Not Change"}}));
    assert!(edit_a.is_err());
    assert_eq!(s.doc().unwrap().node(frame_a).unwrap().name, "Frame A");

    // Attempting to mutate page_id directly must fail
    let edit_page = task.execute(&mut s, "node.set", &json!({"id": page_id.0, "props": {"name": "Should Not Change Page"}}));
    assert!(edit_page.is_err());
}

#[test]
fn generic_and_app_level_commands_are_rejected() {
    let mut s = Session::new();
    let page_id = s.page().unwrap();
    let doc_uid = s.ds().unwrap().uid;
    let mut task = TaskAuthority::new("task-test-03", doc_uid, TaskScope::NewRoot { page_id, task_root: None });

    // File/app/global commands should be rejected
    for bad_cmd in &["app.quit", "file.save", "file.revert", "file.close", "undo", "redo", "view.minimize"] {
        let res = task.execute(&mut s, bad_cmd, &json!({}));
        assert!(res.is_err(), "Command {bad_cmd} should be rejected");
        let msg = res.unwrap_err().to_string();
        assert!(msg.contains("not permitted under design task scope"), "Expected permission error for {bad_cmd}: {msg}");
    }
}

#[test]
fn batch_rollback_when_step_is_out_of_scope() {
    let mut s = Session::new();
    let page_id = s.page().unwrap();

    let sibling = id_of(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 100, "height": 100, "name": "Sibling Original"})).unwrap());
    let target = id_of(&s.execute("frame.create", &json!({"x": 200, "y": 0, "width": 100, "height": 100, "name": "Target Original"})).unwrap());

    let doc_uid = s.ds().unwrap().uid;
    let mut task = TaskAuthority::new("task-test-04", doc_uid, TaskScope::Subtree { page_id, target_node: target });

    // Batch with valid step 1 (modify target) followed by invalid step 2 (modify sibling)
    let batch_res = task.execute(
        &mut s,
        "command.batch",
        &json!({
            "label": "Atomic edit",
            "commands": [
                {"command": "node.set", "params": {"id": target.0, "props": {"name": "Target Modified"}}},
                {"command": "node.set", "params": {"id": sibling.0, "props": {"name": "Sibling Modified"}}}
            ]
        }),
    );

    assert!(batch_res.is_err(), "Batch containing out-of-scope step must fail");

    // Both target and sibling MUST retain original names (step 1 was rolled back!)
    assert_eq!(s.doc().unwrap().node(target).unwrap().name, "Target Original");
    assert_eq!(s.doc().unwrap().node(sibling).unwrap().name, "Sibling Original");
}

#[test]
fn stop_immediately_closes_admission_for_further_mutations() {
    let mut s = Session::new();
    let page_id = s.page().unwrap();
    let doc_uid = s.ds().unwrap().uid;
    let mut task = TaskAuthority::new("task-test-05", doc_uid, TaskScope::NewRoot { page_id, task_root: None });

    // Step 1: Create root before stop
    let root_res = task.execute(&mut s, "frame.create", &json!({"x": 0, "y": 0, "width": 200, "height": 200, "name": "Frame 1"}));
    assert!(root_res.is_ok());

    // Call stop
    task.stop();
    assert!(task.is_stopped());

    // Subsequent mutation must be rejected immediately
    let res = task.execute(&mut s, "shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 50}));
    assert!(res.is_err());
    let msg = res.unwrap_err().to_string();
    assert!(msg.contains("Task was stopped"), "Error message should state task was stopped: {msg}");
}
