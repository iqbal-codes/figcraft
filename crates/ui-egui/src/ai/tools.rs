//! Scoped design canvas tools and dynamic tool execution bridge.

use base64::Engine as _;
use figcraft_doc::NodeId;
use figcraft_engine::{EngineError, Result, Session, TaskAuthority, TaskScope};
use serde_json::{Value, json};

/// Return dynamic tool definitions advertised to the Codex agent via JSON-RPC.
pub fn canvas_tool_specs() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "name": "canvas_create_frame",
            "description": "Create a new frame or artboard on the authorized canvas. In NewRoot scope, creating the top-level frame establishes the task root.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Frame label or artboard name" },
                    "x": { "type": "number", "description": "X coordinate" },
                    "y": { "type": "number", "description": "Y coordinate" },
                    "width": { "type": "number", "description": "Width in pixels" },
                    "height": { "type": "number", "description": "Height in pixels" },
                    "parent": { "type": "integer", "description": "Parent node ID (optional)" }
                },
                "required": ["name", "x", "y", "width", "height"]
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_create_text",
            "description": "Create an editable text layer on the authorized canvas with font size, family, wrapping width and alignment.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text content" },
                    "x": { "type": "number", "description": "X coordinate relative to parent" },
                    "y": { "type": "number", "description": "Y coordinate relative to parent" },
                    "width": { "type": "number", "description": "Optional fixed width in pixels for text wrapping" },
                    "parent": { "type": "integer", "description": "Parent node ID (optional, defaults to task root)" },
                    "name": { "type": "string", "description": "Layer name (optional, defaults to first line of text)" },
                    "fontSize": { "type": "number", "description": "Font size in pixels (optional, default 14)" },
                    "fontFamily": { "type": "string", "description": "Font family name (optional, e.g. 'Inter')" },
                    "fontWeight": { "type": "string", "description": "Font weight or style (optional, e.g. 'Regular', 'Bold')" },
                    "props": { "type": "object", "description": "Additional text or node properties" }
                },
                "required": ["text", "x", "y"]
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_create_shape",
            "description": "Create a shape (rectangle or ellipse) inside the authorized canvas scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["rectangle", "ellipse"], "description": "Type of shape" },
                    "name": { "type": "string", "description": "Optional layer name" },
                    "x": { "type": "number", "description": "X coordinate relative to parent" },
                    "y": { "type": "number", "description": "Y coordinate relative to parent" },
                    "width": { "type": "number", "description": "Width in pixels" },
                    "height": { "type": "number", "description": "Height in pixels" },
                    "parent": { "type": "integer", "description": "Parent node ID (required)" }
                },
                "required": ["kind", "x", "y", "width", "height", "parent"]
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_set_layout",
            "description": "Configure auto-layout on a frame or component in the authorized canvas scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Node ID of the frame to configure" },
                    "mode": { "type": "string", "enum": ["none", "horizontal", "vertical", "grid"], "description": "Auto-layout mode" },
                    "gap": { "type": "number", "description": "Item spacing along main axis in pixels" },
                    "crossGap": { "type": "number", "description": "Cross-axis spacing in pixels" },
                    "padding": { "description": "Padding in pixels: number or [top, right, bottom, left]" },
                    "mainAlign": { "type": "string", "enum": ["min", "center", "max", "spaceBetween", "spaceAround", "spaceEvenly"], "description": "Primary axis alignment" },
                    "crossAlign": { "type": "string", "enum": ["min", "center", "max", "baseline"], "description": "Counter axis alignment" },
                    "wrap": { "type": "boolean", "description": "Whether items wrap onto multiple lines" },
                    "sizingH": { "type": "string", "enum": ["fixed", "hug", "fill"], "description": "Horizontal sizing" },
                    "sizingV": { "type": "string", "enum": ["fixed", "hug", "fill"], "description": "Vertical sizing" }
                },
                "required": ["id", "mode"]
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_set_properties",
            "description": "Update properties (name, geometry, fills, strokes, opacity, corner radius, auto-layout) of an existing node within scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Node ID to update" },
                    "props": { "type": "object", "description": "Properties map to apply" }
                },
                "required": ["id", "props"]
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_query_tree",
            "description": "Read nodes, hierarchy, names, types, dimensions, text content, and auto-layout settings in the authorized canvas scope. Bounded by node count and byte size.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node_id": { "type": "integer", "description": "Root node ID to inspect (optional, defaults to task scope root)" },
                    "depth": { "type": "integer", "description": "Max inspection depth (optional, default 3, max 10)" },
                    "max_nodes": { "type": "integer", "description": "Max total nodes to return (optional, default 200, max 500)" }
                }
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_export_preview",
            "description": "Export a visual raster PNG preview of a node or canvas area within authorized scope to inspect design layout and appearance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node_id": { "type": "integer", "description": "Node ID to render (optional, defaults to task scope root)" },
                    "scale": { "type": "number", "description": "Render scale factor (optional, default 1.0, max 2.0)" }
                }
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_delete_layer",
            "description": "Delete a layer within the authorized scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Node ID to delete" }
                },
                "required": ["id"]
            }
        }),
        json!({
            "type": "function",
            "name": "canvas_batch_commands",
            "description": "Execute an atomic batch of design commands inside the authorized scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "commands": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "command": { "type": "string" },
                                "params": { "type": "object" }
                            },
                            "required": ["command"]
                        }
                    }
                },
                "required": ["commands"]
            }
        }),
    ]
}

/// Execute a dynamic tool or standard command under authoritative task scope.
pub fn execute_tool(session: &mut Session, auth: &mut TaskAuthority, tool_name: &str, params: &Value) -> Result<Value> {
    if auth.is_stopped() {
        return Err(EngineError::Other("Task was stopped: no further tools admitted".into()));
    }

    match tool_name {
        "canvas_create_frame" => {
            let mut p = params.clone();
            if let Some(obj) = p.as_object_mut() {
                // Ensure page context if parent is omitted
                if !obj.contains_key("parent") {
                    let page_id = match auth.scope() {
                        TaskScope::NewRoot { page_id, .. } => *page_id,
                        TaskScope::Subtree { page_id, .. } => *page_id,
                    };
                    obj.insert("parent".into(), json!(page_id.0));
                }
            }
            auth.execute(session, "frame.create", &p)
        }
        "canvas_create_text" => {
            let mut p = params.clone();
            if let Some(obj) = p.as_object_mut() {
                if !obj.contains_key("text")
                    && let Some(c) = obj.get("characters").cloned()
                {
                    obj.insert("text".into(), c);
                }
                if !obj.contains_key("parent") {
                    let default_parent = match auth.scope() {
                        TaskScope::NewRoot { task_root, page_id } => task_root.unwrap_or(*page_id),
                        TaskScope::Subtree { target_node, .. } => *target_node,
                    };
                    obj.insert("parent".into(), json!(default_parent.0));
                }
                let mut props = obj.get("props").and_then(Value::as_object).cloned().unwrap_or_default();
                if let Some(fs) = obj.remove("fontSize") {
                    props.insert("fontSize".into(), fs);
                }
                if let Some(ff) = obj.remove("fontFamily") {
                    props.insert("fontFamily".into(), ff);
                }
                if let Some(fw) = obj.remove("fontWeight") {
                    props.insert("fontWeight".into(), fw);
                }
                if !props.is_empty() {
                    obj.insert("props".into(), Value::Object(props));
                }
            }
            auth.execute(session, "text.create", &p)
        }
        "canvas_create_shape" => {
            let kind = params.get("kind").and_then(Value::as_str).unwrap_or("rectangle");
            let cmd = if kind == "ellipse" { "shape.ellipse" } else { "shape.rectangle" };
            let mut p = params.clone();
            if let Some(obj) = p.as_object_mut() {
                obj.remove("kind");
            }
            auth.execute(session, cmd, &p)
        }
        "canvas_set_layout" => {
            let id = params.get("id").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("canvas_set_layout requires `id` integer".into()))?;
            let mut props = serde_json::Map::new();
            if let Some(mode) = params.get("mode").and_then(Value::as_str) {
                props.insert("layout".into(), json!(mode));
            }
            if let Some(gap) = params.get("gap") {
                props.insert("gap".into(), gap.clone());
            }
            if let Some(cg) = params.get("crossGap") {
                props.insert("crossGap".into(), cg.clone());
            }
            if let Some(pad) = params.get("padding") {
                props.insert("padding".into(), pad.clone());
            }
            if let Some(ma) = params.get("mainAlign") {
                props.insert("mainAlign".into(), ma.clone());
            }
            if let Some(ca) = params.get("crossAlign") {
                props.insert("crossAlign".into(), ca.clone());
            }
            if let Some(wrap) = params.get("wrap") {
                props.insert("wrap".into(), wrap.clone());
            }
            if let Some(sh) = params.get("sizingH") {
                props.insert("sizingH".into(), sh.clone());
            }
            if let Some(sv) = params.get("sizingV") {
                props.insert("sizingV".into(), sv.clone());
            }
            let payload = json!({
                "id": id,
                "props": Value::Object(props)
            });
            auth.execute(session, "node.set", &payload)
        }
        "canvas_set_properties" => auth.execute(session, "node.set", params),
        "canvas_delete_layer" => {
            let id =
                params.get("id").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("canvas_delete_layer requires `id` integer".into()))?;
            auth.execute(session, "edit.delete", &json!({ "ids": [id] }))
        }
        "canvas_batch_commands" => auth.execute(session, "command.batch", params),
        "canvas_query_tree" => {
            let doc = session.doc()?;
            let default_root = match auth.scope() {
                TaskScope::NewRoot { task_root, page_id } => task_root.unwrap_or(*page_id),
                TaskScope::Subtree { target_node, .. } => *target_node,
            };
            let start_id = params.get("node_id").and_then(Value::as_u64).map(NodeId).unwrap_or(default_root);

            if !auth.is_in_scope(doc, start_id) {
                return Err(EngineError::Other(format!("Query outside authorized task scope: node {} is not within scope", start_id.0)));
            }

            let max_depth = params.get("depth").and_then(Value::as_u64).unwrap_or(3).min(10) as usize;
            let max_nodes = params.get("max_nodes").and_then(Value::as_u64).unwrap_or(200).min(500) as usize;
            const MAX_BYTES: usize = 48 * 1024;

            let mut node_count = 0;
            let mut truncated = false;
            let tree = inspect_node_tree(doc, start_id, 0, max_depth, max_nodes, &mut node_count, &mut truncated);

            let serialized_len = serde_json::to_string(&tree).map(|s| s.len()).unwrap_or(0);
            if serialized_len > MAX_BYTES {
                truncated = true;
            }

            Ok(json!({
                "root": start_id.0,
                "total_nodes": node_count,
                "truncated": truncated,
                "tree": tree
            }))
        }
        "canvas_export_preview" => {
            let doc = session.doc()?;
            let default_target = match auth.scope() {
                TaskScope::NewRoot { task_root, .. } => *task_root,
                TaskScope::Subtree { target_node, .. } => Some(*target_node),
            };
            let target_id = params
                .get("node_id")
                .and_then(Value::as_u64)
                .map(NodeId)
                .or(default_target)
                .ok_or_else(|| EngineError::Other("canvas_export_preview requires a valid node_id or established task root".into()))?;

            if !auth.is_in_scope(doc, target_id) {
                return Err(EngineError::Other(format!("Export outside authorized task scope: node {} is not within scope", target_id.0)));
            }

            let scale = params.get("scale").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.1, 2.0);
            let area = figcraft_engine::cmd::file::export_area(doc, &[target_id], &figcraft_doc::ExportSetting::default())
                .ok_or_else(|| EngineError::Other("Nothing to render in target area".into()))?;
            let pix = figcraft_render::render_area(doc, &[target_id], area, scale)
                .ok_or_else(|| EngineError::Other("Rendering failed (empty area or dimensions exceed max size)".into()))?;
            let width = pix.width();
            let height = pix.height();
            let png_bytes = figcraft_render::encode_png(&pix).map_err(|e| EngineError::Other(format!("PNG encoding error: {e}")))?;
            let png_base64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

            Ok(json!({
                "node_id": target_id.0,
                "width": width,
                "height": height,
                "scale": scale,
                "byte_size": png_bytes.len(),
                "png_base64": png_base64
            }))
        }
        other => {
            // Fall back to direct engine command invocation with TaskAuthority scope validation
            auth.execute(session, other, params)
        }
    }
}

fn inspect_node_tree(
    doc: &figcraft_doc::Document,
    id: NodeId,
    depth: usize,
    max_depth: usize,
    max_nodes: usize,
    node_count: &mut usize,
    truncated: &mut bool,
) -> Value {
    if *node_count >= max_nodes {
        *truncated = true;
        return Value::Null;
    }
    *node_count += 1;

    if let Some(node) = doc.node(id) {
        let kind_str = node.kind.type_name();
        let coeffs = node.transform.as_coeffs();
        let x = coeffs[4];
        let y = coeffs[5];

        let mut node_obj = json!({
            "id": node.id.0,
            "name": node.name,
            "kind": kind_str,
            "x": x,
            "y": y,
            "width": node.width,
            "height": node.height,
        });

        if let figcraft_doc::NodeKind::Text(t) = &node.kind
            && let Some(map) = node_obj.as_object_mut()
        {
            map.insert("text".into(), json!(t.characters));
            map.insert("fontSize".into(), json!(t.style.font_size));
            map.insert("fontFamily".into(), json!(t.style.font_family));
        }

        if let Some(l) = &node.layout {
            let mode_str = match l.mode {
                figcraft_doc::LayoutMode::Horizontal => "horizontal",
                figcraft_doc::LayoutMode::Vertical => "vertical",
                figcraft_doc::LayoutMode::Grid => "grid",
            };
            if let Some(map) = node_obj.as_object_mut() {
                map.insert(
                    "layout".into(),
                    json!({
                        "mode": mode_str,
                        "gap": l.gap,
                        "crossGap": l.cross_gap,
                        "padding": l.padding,
                        "wrap": l.wrap,
                    }),
                );
            }
        }

        let sizing_h = match node.child_layout.sizing_h {
            figcraft_doc::Sizing::Fixed => "fixed",
            figcraft_doc::Sizing::Hug => "hug",
            figcraft_doc::Sizing::Fill => "fill",
        };
        let sizing_v = match node.child_layout.sizing_v {
            figcraft_doc::Sizing::Fixed => "fixed",
            figcraft_doc::Sizing::Hug => "hug",
            figcraft_doc::Sizing::Fill => "fill",
        };
        if let Some(map) = node_obj.as_object_mut() {
            map.insert("sizing".into(), json!({ "h": sizing_h, "v": sizing_v }));
        }

        let children: Vec<Value> = if depth < max_depth {
            node.children
                .iter()
                .filter_map(|&child_id| {
                    if *node_count >= max_nodes {
                        *truncated = true;
                        None
                    } else {
                        let child_val = inspect_node_tree(doc, child_id, depth + 1, max_depth, max_nodes, node_count, truncated);
                        if child_val.is_null() { None } else { Some(child_val) }
                    }
                })
                .collect()
        } else {
            if !node.children.is_empty() {
                *truncated = true;
            }
            Vec::new()
        };

        if let Some(map) = node_obj.as_object_mut() {
            map.insert("children".into(), Value::Array(children));
        }

        node_obj
    } else {
        Value::Null
    }
}
