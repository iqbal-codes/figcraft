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
    /// All node IDs deleted by this task.
    pub deleted_nodes: BTreeSet<NodeId>,
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
    pub revision: u64,
    pub scope: TaskScope,
    pub stopped: bool,
    pub provenance: TaskProvenance,
}

impl TaskAuthority {
    /// Create a new task authority bound to `doc_uid` and `scope`.
    pub fn new(task_id: impl Into<String>, doc_uid: u64, scope: TaskScope) -> Self {
        Self::new_with_revision(task_id, doc_uid, 0, scope)
    }

    /// Create a new task authority capturing host document identity and revision.
    pub fn new_with_revision(task_id: impl Into<String>, doc_uid: u64, revision: u64, scope: TaskScope) -> Self {
        Self { task_id: task_id.into(), doc_uid, revision, scope, stopped: false, provenance: TaskProvenance::default() }
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn doc_uid(&self) -> u64 {
        self.doc_uid
    }

    pub fn revision(&self) -> u64 {
        self.revision
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

    /// Check whether `node_id` is within this task's authorized scope.
    pub fn is_in_scope(&self, doc: &Document, node_id: NodeId) -> bool {
        match &self.scope {
            TaskScope::NewRoot { page_id, task_root } => match task_root {
                Some(root) => is_descendant_of(doc, node_id, *root),
                None => node_id == *page_id,
            },
            TaskScope::Subtree { target_node, .. } => is_descendant_of(doc, node_id, *target_node),
        }
    }

    /// Revalidate target and scope against latest document state after external changes.
    pub fn revalidate_scope_against_document(&self, doc: &Document) -> Result<()> {
        match &self.scope {
            TaskScope::Subtree { page_id, target_node } => {
                if !doc.contains(*page_id) {
                    return Err(EngineError::Other(format!("Target page {} was removed externally", page_id.0)));
                }
                if !doc.contains(*target_node) {
                    return Err(EngineError::Other(format!("Authorized target node {} was deleted externally", target_node.0)));
                }
                if !is_descendant_of(doc, *target_node, *page_id) && *target_node != *page_id {
                    return Err(EngineError::Other(format!("Authorized target node {} is no longer on target page {}", target_node.0, page_id.0)));
                }
                Ok(())
            }
            TaskScope::NewRoot { page_id, task_root } => {
                if !doc.contains(*page_id) {
                    return Err(EngineError::Other(format!("Target page {} was removed externally", page_id.0)));
                }
                if let Some(root) = task_root
                    && !doc.contains(*root)
                {
                    return Err(EngineError::Other(format!("Established task root {} was deleted externally", root.0)));
                }
                Ok(())
            }
        }
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

        // Revision validation & refresh after external concurrent changes (Spec axis finding 3)
        let current_rev = session.ds()?.revision;
        if self.revision > 0 && current_rev != self.revision {
            self.revalidate_scope_against_document(&session.ds()?.doc)?;
            self.revision = current_rev;
        } else if self.revision == 0 {
            self.revision = current_rev;
        }

        let backup_scope = self.scope.clone();
        let backup_provenance = self.provenance.clone();
        let backup_revision = self.revision;

        Self::verify_allowed_command(cmd_id, params)?;

        let mut mutation_result = None;
        let res = session.checkpointed(None, |s| {
            let before_doc = s.ds()?.doc.clone();
            let res_value = s.execute_as(Some(&self.task_id), cmd_id, params)?;
            let after_doc = s.ds()?.doc.clone();
            let changed = after_doc.changed_ids(&before_doc, usize::MAX).unwrap_or_default();
            self.validate_and_record_scope(&before_doc, &after_doc, &changed, cmd_id)?;
            mutation_result = Some(res_value);
            Ok(())
        });

        match res {
            Ok(()) => {
                if let Ok(ds) = session.ds() {
                    self.revision = ds.revision;
                }
                Ok(mutation_result.unwrap_or(Value::Null))
            }
            Err(err) => {
                self.scope = backup_scope;
                self.provenance = backup_provenance;
                self.revision = backup_revision;
                Err(err)
            }
        }
    }

    fn verify_allowed_command(cmd_id: &str, params: &Value) -> Result<()> {
        let mut total_count = 0;
        Self::verify_allowed_command_recursive(cmd_id, params, 0, &mut total_count)
    }

    fn verify_allowed_command_recursive(cmd_id: &str, params: &Value, depth: usize, total_count: &mut usize) -> Result<()> {
        if depth > 3 {
            return Err(EngineError::Other("Command batch exceeds maximum nesting depth of 3".into()));
        }
        *total_count += 1;
        if *total_count > 100 {
            return Err(EngineError::Other("Command batch exceeds maximum limit of 100 commands".into()));
        }

        // Explicit blocklist: reject system, external, global, clipboard and history escaping commands
        if cmd_id.starts_with("file.")
            || cmd_id.starts_with("app.")
            || cmd_id.starts_with("collab.")
            || cmd_id.starts_with("export.")
            || cmd_id.starts_with("plugin.")
            || cmd_id.starts_with("clipboard.")
            || cmd_id.starts_with("view.")
            || cmd_id.starts_with("history.")
            || cmd_id.starts_with("system.")
            || cmd_id.starts_with("pref.")
            || cmd_id.starts_with("dev.")
            || cmd_id.starts_with("page.")
            || cmd_id == "undo"
            || cmd_id == "redo"
            || cmd_id == "edit.copy"
            || cmd_id == "edit.cut"
            || cmd_id == "edit.paste"
        {
            return Err(EngineError::Other(format!("Command `{cmd_id}` is not permitted under design task scope")));
        }

        if cmd_id == "command.batch" {
            let commands = params
                .get("commands")
                .and_then(Value::as_array)
                .ok_or_else(|| EngineError::Other("Malformed batch: command.batch missing commands array".into()))?;
            for c in commands {
                let inner_cmd = c
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| EngineError::Other("Malformed batch: command item missing command string".into()))?;
                let inner_params = c.get("params").unwrap_or(&Value::Null);
                Self::verify_allowed_command_recursive(inner_cmd, inner_params, depth + 1, total_count)?;
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
            || cmd_id.starts_with("boolean.")
            || cmd_id.starts_with("group.")
            || cmd_id.starts_with("style.")
            || cmd_id == "edit.delete"
            || cmd_id == "edit.duplicate";

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
                        if !after_doc.contains(ch_id) {
                            *task_root = None;
                            return Err(EngineError::Other(format!(
                                "Mutation out of scope: cannot delete node {} while establishing task root",
                                ch_id.0
                            )));
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

                        if before_doc.contains(ch_id) && !after_doc.contains(ch_id) {
                            // Deletion check
                            if ch_id == root {
                                return Err(EngineError::Other(format!("Mutation out of scope: cannot delete established task root {}", root.0)));
                            }
                            if !self.provenance.created_nodes.contains(&ch_id) {
                                return Err(EngineError::Other(format!(
                                    "Mutation out of scope: cannot delete node {} outside task creation ownership",
                                    ch_id.0
                                )));
                            }
                            self.provenance.deleted_nodes.insert(ch_id);
                            self.provenance.created_nodes.remove(&ch_id);
                            self.provenance.modified_nodes.remove(&ch_id);
                            affected.push(ch_id);
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
                    if before_doc.contains(ch_id) && !after_doc.contains(ch_id) {
                        // Deletion check
                        if ch_id == target {
                            return Err(EngineError::Other(format!("Mutation out of scope: cannot delete authorized target node {}", target.0)));
                        }
                        if !is_descendant_of(before_doc, ch_id, target) {
                            return Err(EngineError::Other(format!(
                                "Mutation out of scope: cannot delete node {} outside authorized subtree {}",
                                ch_id.0, target.0
                            )));
                        }
                        self.provenance.deleted_nodes.insert(ch_id);
                        self.provenance.created_nodes.remove(&ch_id);
                        self.provenance.modified_nodes.remove(&ch_id);
                        affected.push(ch_id);
                        continue;
                    }

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
