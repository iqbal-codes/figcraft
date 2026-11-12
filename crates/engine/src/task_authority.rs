//! Host-issued task authority and scope enforcement for AI agent design workflows.
//!
//! A task is bound to an open document and an authorized scope (either creating a new root
//! on an authorized page, or editing an existing subtree).
//! Mutations outside this scope or generic/external command bypasses are rejected at live-app
//! execution, and Stop immediately revokes admission for all future mutations.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
    /// Authorizes editing several existing subtrees on `page_id`, each root an object the user
    /// named separately. Reads and edits use the union of the roots; no root may be deleted, and
    /// nothing crosses the union boundary in either direction. `target_nodes` is nonempty and holds
    /// no repeats (see [`TaskScope::subtrees`]).
    Subtrees { page_id: NodeId, target_nodes: Vec<NodeId> },
}

impl TaskScope {
    /// The scope naming `roots` on `page_id`, deduplicated in first-mention order.
    ///
    /// An empty mention list is not a scope: the composer submits [`TaskScope::NewRoot`] then, so
    /// a `Subtrees` holding no authorized object is always a caller error.
    pub fn subtrees(page_id: NodeId, roots: impl IntoIterator<Item = NodeId>) -> Result<Self> {
        let mut target_nodes: Vec<NodeId> = Vec::new();
        for root in roots {
            if !target_nodes.contains(&root) {
                target_nodes.push(root);
            }
        }
        if target_nodes.is_empty() {
            return Err(EngineError::Other("Task scope needs at least one authorized object".into()));
        }
        Ok(TaskScope::Subtrees { page_id, target_nodes })
    }

    /// The page this scope is bound to.
    pub fn page_id(&self) -> NodeId {
        match self {
            TaskScope::NewRoot { page_id, .. } | TaskScope::Subtree { page_id, .. } | TaskScope::Subtrees { page_id, .. } => *page_id,
        }
    }

    /// The first authorized root, whose read also refreshes the task context: the first `NewRoot`
    /// frame once created, the subtree, else the first named object.
    pub fn first_root(&self) -> Option<NodeId> {
        match self {
            TaskScope::NewRoot { page_id, task_root } => Some(task_root.unwrap_or(*page_id)),
            TaskScope::Subtree { target_node, .. } => Some(*target_node),
            TaskScope::Subtrees { target_nodes, .. } => target_nodes.first().copied(),
        }
    }

    /// [`TaskScope::subtrees`], validated against `doc` as it is submitted: every named object
    /// exists and is on `page_id`. An object the user can no longer point at is never authority.
    pub fn subtrees_on(doc: &Document, page_id: NodeId, roots: impl IntoIterator<Item = NodeId>) -> Result<Self> {
        let scope = Self::subtrees(page_id, roots)?;
        scope.validate_on(doc)?;
        Ok(scope)
    }

    /// Whether the page and every root this scope names still exist, and every root is still on
    /// the bound page.
    pub fn validate_on(&self, doc: &Document) -> Result<()> {
        match self {
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
            TaskScope::Subtrees { page_id, target_nodes } => {
                if !doc.contains(*page_id) {
                    return Err(EngineError::Other(format!("Target page {} was removed externally", page_id.0)));
                }
                if target_nodes.is_empty() {
                    return Err(EngineError::Other("Task scope has no authorized objects".into()));
                }
                for root in target_nodes {
                    if !doc.contains(*root) {
                        return Err(EngineError::Other(format!("Authorized object {} was deleted externally", root.0)));
                    }
                    if *root != *page_id && !is_descendant_of(doc, *root, *page_id) {
                        return Err(EngineError::Other(format!("Authorized object {} is no longer on target page {}", root.0, page_id.0)));
                    }
                }
                Ok(())
            }
        }
    }
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
    #[serde(default)]
    revision_known: bool,
    // Session revisions include selection and view commands. Retain the inspected
    // copy-on-write document so those commands cannot masquerade as external edits.
    #[serde(skip)]
    context_document: Option<Arc<Document>>,
    pub scope: TaskScope,
    pub stopped: bool,
    pub provenance: TaskProvenance,
}

impl TaskAuthority {
    /// Create a new task authority bound to `doc_uid` and `scope`.
    pub fn new(task_id: impl Into<String>, doc_uid: u64, scope: TaskScope) -> Self {
        let mut task = Self::new_with_revision(task_id, doc_uid, 0, scope);
        task.revision_known = false;
        task
    }

    /// Create a new task authority capturing host document identity and revision.
    pub fn new_with_revision(task_id: impl Into<String>, doc_uid: u64, revision: u64, scope: TaskScope) -> Self {
        Self {
            task_id: task_id.into(),
            doc_uid,
            revision,
            revision_known: true,
            context_document: None,
            scope,
            stopped: false,
            provenance: TaskProvenance::default(),
        }
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

    /// A successful scoped read supplies the current context to the harness. Only
    /// this explicit refresh permits editing again after an external revision.
    pub fn refresh_context(&mut self, session: &Session) -> Result<()> {
        let state = session.ds()?;
        if self.stopped || state.uid != self.doc_uid {
            return Err(EngineError::Other("Task context is unavailable".into()));
        }
        self.revalidate_scope_against_document(&state.doc)?;
        self.revision = state.revision;
        self.revision_known = true;
        self.context_document = Some(state.doc.clone());
        Ok(())
    }

    /// Check whether `node_id` is within this task's authorized scope.
    pub fn is_in_scope(&self, doc: &Document, node_id: NodeId) -> bool {
        match &self.scope {
            TaskScope::NewRoot { page_id, task_root } => match task_root {
                Some(root) => is_descendant_of(doc, node_id, *root),
                None => node_id == *page_id,
            },
            TaskScope::Subtree { target_node, .. } => is_descendant_of(doc, node_id, *target_node),
            TaskScope::Subtrees { target_nodes, .. } => target_nodes.iter().copied().any(|root| is_descendant_of(doc, node_id, root)),
        }
    }

    /// Revalidate target and scope against latest document state after external changes.
    pub fn revalidate_scope_against_document(&self, doc: &Document) -> Result<()> {
        self.scope.validate_on(doc)
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
        let current_document = &session.ds()?.doc;
        let context_changed =
            self.context_document.as_ref().map_or(current_rev != self.revision, |snapshot| !Arc::ptr_eq(snapshot, current_document));
        if self.revision_known && context_changed {
            self.revalidate_scope_against_document(&session.ds()?.doc)?;
            return Err(EngineError::Other(
                "Document changed since the task's last context. Read canvas_query_tree at the task root before editing again.".into(),
            ));
        } else if !self.revision_known {
            self.revision = current_rev;
            self.revision_known = true;
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
                    self.context_document = Some(ds.doc.clone());
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
            TaskScope::Subtrees { page_id, target_nodes } => {
                let page = *page_id;
                let roots = target_nodes.clone();
                let authorized_before = |id: NodeId| roots.iter().copied().any(|root| is_descendant_of(before_doc, id, root));
                let authorized_after = |id: NodeId| roots.iter().copied().any(|root| is_descendant_of(after_doc, id, root));

                for &ch_id in changed {
                    let existed_before = before_doc.contains(ch_id);
                    let exists_after = after_doc.contains(ch_id);
                    let authorized_then = existed_before && authorized_before(ch_id);
                    let authorized_now = exists_after && authorized_after(ch_id);

                    if existed_before && !exists_after {
                        if roots.contains(&ch_id) {
                            return Err(EngineError::Other(format!("Mutation out of scope: cannot delete authorized object {}", ch_id.0)));
                        }
                        if !authorized_then {
                            return Err(EngineError::Other(format!(
                                "Mutation out of scope: cannot delete node {} outside the authorized objects",
                                ch_id.0
                            )));
                        }
                        self.provenance.deleted_nodes.insert(ch_id);
                        self.provenance.created_nodes.remove(&ch_id);
                        self.provenance.modified_nodes.remove(&ch_id);
                        affected.push(ch_id);
                        continue;
                    }

                    // An edit or a move that stays inside the union: a node inside one authorized
                    // object may be edited, or moved into another one.
                    if authorized_then && authorized_now {
                        self.provenance.modified_nodes.insert(ch_id);
                        affected.push(ch_id);
                        continue;
                    }

                    if !existed_before && authorized_now {
                        self.provenance.created_nodes.insert(ch_id);
                        affected.push(ch_id);
                        continue;
                    }

                    if authorized_then {
                        return Err(EngineError::Other(format!("Mutation out of scope: node {} was moved out of the authorized objects", ch_id.0)));
                    }
                    if authorized_now {
                        return Err(EngineError::Other(format!(
                            "Mutation out of scope: node {} was brought into the authorized objects from outside",
                            ch_id.0
                        )));
                    }

                    // On neither side of the union: only a structural parent's child list may
                    // differ, and only through authorized children edited on their own.
                    if existed_before
                        && exists_after
                        && is_structural_parent_change(before_doc, after_doc, ch_id, changed, &authorized_before, &authorized_after)
                    {
                        affected.push(ch_id);
                        continue;
                    }

                    return Err(EngineError::Other(format!("Mutation out of scope: node {} is outside the authorized objects", ch_id.0)));
                }

                // Every named object survives, on the bound page.
                for root in &roots {
                    if !after_doc.contains(*root) {
                        return Err(EngineError::Other(format!("Mutation out of scope: cannot delete authorized object {}", root.0)));
                    }
                    if *root != page && !is_descendant_of(after_doc, *root, page) {
                        return Err(EngineError::Other(format!(
                            "Mutation out of scope: authorized object {} is no longer on target page {}",
                            root.0, page.0
                        )));
                    }
                }
            }
        }

        self.provenance.edit_groups.push(TaskEditGroup { label: cmd_id.to_string(), affected_nodes: affected, timestamp: 0 });

        Ok(())
    }
}

/// Whether the only change to `id` — a node on neither side of the authorized objects — is its
/// child list changing through children that are authorized objects edited on their own.
///
/// A structural parent sits outside the union (the page a named object is on, the parent of two
/// named siblings), so an authorized child added to it or moved off it changes the parent itself.
/// Every other child keeps its relative order and every other property stays identical, so nothing
/// is smuggled past the scope by a reorder or a property edit on the parent.
fn is_structural_parent_change(
    before_doc: &Document,
    after_doc: &Document,
    id: NodeId,
    changed: &[NodeId],
    authorized_before: &impl Fn(NodeId) -> bool,
    authorized_after: &impl Fn(NodeId) -> bool,
) -> bool {
    let (Some(before), Some(after)) = (before_doc.node(id), after_doc.node(id)) else { return false };
    let mut appeared = after.children.iter().copied().filter(|child| !before.children.contains(child));
    if !appeared.all(|child| changed.contains(&child) && authorized_after(child)) {
        return false;
    }
    let mut gone = before.children.iter().copied().filter(|child| !after.children.contains(child));
    if !gone.all(|child| changed.contains(&child) && authorized_before(child)) {
        return false;
    }
    let kept_before: Vec<NodeId> = before.children.iter().copied().filter(|child| after.children.contains(child)).collect();
    let kept_after: Vec<NodeId> = after.children.iter().copied().filter(|child| before.children.contains(child)).collect();
    if kept_before != kept_after {
        return false;
    }
    let mut left = before.clone();
    let mut right = after.clone();
    left.children = kept_before;
    right.children = kept_after;
    left == right
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
