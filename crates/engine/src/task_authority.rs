//! Host-issued task authority and scope enforcement for AI agent design workflows.
//!
//! A task is bound to an open document and an authorized scope (either creating a new root
//! on an authorized page, or editing an existing subtree).
//! Mutations outside this scope or generic/external command bypasses are rejected at live-app
//! execution, and Stop immediately revokes admission for all future mutations.

use std::collections::{BTreeMap, BTreeSet};

use figcraft_doc::{Document, NodeId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{EngineError, Result, Session};

/// The authorized design scope for a task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskScope {
    /// Authorizes creating a new root frame on `page_id`.
    /// The first root frame created on this page becomes the `task_root`.
    /// Once created, only `task_root` and its descendants may be modified.
    NewRoot { page_id: NodeId, task_root: Option<NodeId> },
    /// Authorizes editing an existing subtree rooted at `target_node` on `page_id`.
    /// Only `target_node` and its descendants may be modified.
    Subtree { page_id: NodeId, target_node: NodeId },
}

/// Chronological record of edits and provenance under this task.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskProvenance {
    /// All node IDs created by this task.
    pub created_nodes: BTreeSet<NodeId>,
    /// All existing node IDs modified by this task.
    pub modified_nodes: BTreeSet<NodeId>,
    /// Initial properties before this task made changes, keyed by node ID.
    pub initial_properties: BTreeMap<NodeId, Value>,
    /// Chronological list of batch/command execution outcomes.
    pub edit_groups: Vec<TaskEditGroup>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskEditGroup {
    pub label: String,
    pub affected_nodes: Vec<NodeId>,
    pub timestamp: u64,
}

/// Host-issued task authority enforcing scope, admission, attribution and Stop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskAuthority {
    pub task_id: String,
    pub doc_uid: u64,
    pub scope: TaskScope,
    pub stopped: bool,
    pub provenance: TaskProvenance,
}

impl TaskAuthority {
    /// Create a new task authority bound to `doc_uid` and `scope`.
    pub fn new(task_id: impl Into<String>, doc_uid: u64, scope: TaskScope) -> Self {
        Self { task_id: task_id.into(), doc_uid, scope, stopped: false, provenance: TaskProvenance::default() }
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn doc_uid(&self) -> u64 {
        self.doc_uid
    }

    pub fn scope(&self) -> &TaskScope {
        &self.scope
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Stop closes admission for later mutations immediately.
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    pub fn provenance(&self) -> &TaskProvenance {
        &self.provenance
    }

    /// Execute a command under this task's authority and scope.
    pub fn execute(&mut self, session: &mut Session, cmd_id: &str, params: &Value) -> Result<Value> {
        if self.stopped {
            return Err(EngineError::Other("Task was stopped: no further mutations admitted".into()));
        }

        let current_uid = session.ds()?.uid;
        if current_uid != self.doc_uid {
            return Err(EngineError::Other(format!(
                "Target document mismatch: active document {current_uid} does not match task document {}",
                self.doc_uid
            )));
        }

        Self::verify_allowed_command(cmd_id, params)?;

        let before_doc = session.ds()?.doc.clone();
        let before_history = session.ds()?.history.undo.len();

        let res_value = session.execute_as(Some(&self.task_id), cmd_id, params)?;

        let after_doc = session.ds()?.doc.clone();
        let changed = after_doc.changed_ids(&before_doc, usize::MAX).unwrap_or_default();

        if let Err(scope_err) = self.validate_and_record_scope(&before_doc, &after_doc, &changed, cmd_id) {
            // Roll back document and undo history
            if let Ok(ds) = session.ds_mut() {
                ds.doc = before_doc;
                ds.history.undo.truncate(before_history);
            }
            return Err(scope_err);
        }

        Ok(res_value)
    }

    fn verify_allowed_command(cmd_id: &str, params: &Value) -> Result<()> {
        if cmd_id == "command.batch" {
            if let Some(commands) = params.get("commands").and_then(Value::as_array) {
                for c in commands {
                    if let Some(inner_cmd) = c.get("command").and_then(Value::as_str) {
                        let inner_params = c.get("params").unwrap_or(&Value::Null);
                        Self::verify_allowed_command(inner_cmd, inner_params)?;
                    } else {
                        return Err(EngineError::Other("Malformed batch: command item missing command string".into()));
                    }
                }
            }
            return Ok(());
        }

        let allowed = cmd_id.starts_with("shape.")
            || cmd_id.starts_with("frame.")
            || cmd_id.starts_with("property.")
            || cmd_id.starts_with("layout.")
            || cmd_id.starts_with("text.")
            || cmd_id.starts_with("component.")
            || cmd_id.starts_with("node.")
            || cmd_id.starts_with("align.")
            || cmd_id.starts_with("distribute.")
            || cmd_id.starts_with("vector.")
            || cmd_id.starts_with("boolean.");

        if !allowed {
            return Err(EngineError::Other(format!("Command `{cmd_id}` is not permitted under design task scope")));
        }

        Ok(())
    }

    fn validate_and_record_scope(&mut self, before_doc: &Document, after_doc: &Document, changed: &[NodeId], cmd_id: &str) -> Result<()> {
        let mut affected = Vec::new();

        match &mut self.scope {
            TaskScope::NewRoot { page_id, task_root } => {
                let pid = *page_id;
                if task_root.is_none() {
                    let page_before = before_doc.node(pid);
                    let page_after = after_doc.node(pid).ok_or_else(|| EngineError::Other(format!("Target page {} not found in document", pid.0)))?;

                    let new_roots: Vec<NodeId> =
                        page_after.children.iter().copied().filter(|id| !page_before.map(|p| p.children.contains(id)).unwrap_or(false)).collect();

                    if new_roots.len() != 1 {
                        return Err(EngineError::Other(
                            "Mutation out of scope: task must create exactly one root frame on the authorized page".into(),
                        ));
                    }

                    let new_root = new_roots[0];
                    *task_root = Some(new_root);
                    self.provenance.created_nodes.insert(new_root);

                    for &ch_id in changed {
                        if ch_id == pid {
                            continue;
                        }
                        if ch_id != new_root && !is_descendant_of(after_doc, ch_id, new_root) {
                            *task_root = None;
                            return Err(EngineError::Other(format!(
                                "Mutation out of scope: node {} is outside newly created task root {}",
                                ch_id.0, new_root.0
                            )));
                        }
                        if !before_doc.contains(ch_id) {
                            self.provenance.created_nodes.insert(ch_id);
                        } else {
                            self.provenance.modified_nodes.insert(ch_id);
                        }
                        affected.push(ch_id);
                    }
                } else {
                    let root = match task_root {
                        Some(r) => *r,
                        None => {
                            return Err(EngineError::Other("Task root not established".into()));
                        }
                    };

                    for &ch_id in changed {
                        if ch_id == pid {
                            continue;
                        }
                        if ch_id != root && !is_descendant_of(after_doc, ch_id, root) {
                            return Err(EngineError::Other(format!("Mutation out of scope: node {} is outside task-owned root {}", ch_id.0, root.0)));
                        }
                        if !before_doc.contains(ch_id) {
                            self.provenance.created_nodes.insert(ch_id);
                        } else {
                            self.provenance.modified_nodes.insert(ch_id);
                        }
                        affected.push(ch_id);
                    }
                }
            }
            TaskScope::Subtree { target_node, .. } => {
                let target = *target_node;
                for &ch_id in changed {
                    if ch_id != target && !is_descendant_of(after_doc, ch_id, target) {
                        return Err(EngineError::Other(format!(
                            "Mutation out of scope: node {} is outside authorized target subtree {}",
                            ch_id.0, target.0
                        )));
                    }
                    if !before_doc.contains(ch_id) {
                        self.provenance.created_nodes.insert(ch_id);
                    } else {
                        self.provenance.modified_nodes.insert(ch_id);
                    }
                    affected.push(ch_id);
                }
            }
        }

        self.provenance.edit_groups.push(TaskEditGroup { label: cmd_id.to_string(), affected_nodes: affected, timestamp: 0 });

        Ok(())
    }
}

fn is_descendant_of(doc: &Document, node_id: NodeId, root_id: NodeId) -> bool {
    if node_id == root_id {
        return true;
    }
    let mut current = node_id;
    let mut depth = 0;
    while let Some(node) = doc.node(current) {
        depth += 1;
        if depth > 1000 {
            break;
        }
        if let Some(parent) = node.parent {
            if parent == root_id {
                return true;
            }
            current = parent;
        } else {
            break;
        }
    }
    false
}
