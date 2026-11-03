//! Graph commands: open/rename/delete graphs, size and seed, frames, comments, pins, exposed
//! inputs, selection, alignment and "create graph from selection".

use serde_json::{Value as J, json};

use materialcraft_core::{Comment, ExposedParam, Frame, MAX_SIZE_LOG2, MIN_SIZE_LOG2, NodeId, OutputFormat, Pin, Value};

use crate::commands::{CommandSpec, always, bad, boolp, f32p, has_graph, has_selection, i64p, ids_param, point, strp, u64p};
use crate::{Result, Session, cmd};

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("graph.open", "Open graph", [], None, "{graph: index or name}", always, open),
        cmd!("graph.rename", "Rename graph", [], None, "{name, graph?}", has_graph, rename),
        cmd!("graph.delete", "Delete graph", [], None, "{graph?}", has_graph, delete),
        cmd!("graph.duplicate", "Duplicate graph", [], None, "{graph?}", has_graph, duplicate),
        cmd!("graph.size", "Graph size", [], None, "{width, height} in pixels (powers of two, 16..8192) or {log2: [w, h]}", has_graph, size),
        cmd!("graph.seed", "Random seed", [], None, "{seed}", has_graph, |s, p| {
            let seed = i64p(p, "seed").ok_or_else(|| bad("graph.seed", "`seed` is required"))?.clamp(0, i32::MAX as i64) as i32;
            s.edit_merged("graph.seed", "Random seed", |s| {
                s.graph_mut()?.seed = seed;
                Ok(J::Null)
            })
        }),
        cmd!("graph.format", "Output format", [], None, "{format: u8|u16|f16|f32}", has_graph, format),
        cmd!("graph.cook", "Compute graph", [], None, "{full?: bool} cooks now; returns per-node timings and errors", has_graph, cook),
        cmd!("graph.attributes", "Graph attributes", [], None, "{category?, description?, author?, tags?}", has_graph, attributes),
        cmd!(
            "graph.frame.add",
            "Add Frame",
            [],
            None,
            "{title?, rect? [x, y, w, h], color? [r,g,b,a]} around the selection when no rect",
            has_graph,
            frame_add
        ),
        cmd!("graph.frame.edit", "Edit frame", [], None, "{id, title?, description?, rect?, color?}", has_graph, frame_edit),
        cmd!("graph.frame.delete", "Delete frame", [], None, "{id}", has_graph, frame_delete),
        cmd!("graph.comment.add", "Add Comment", [], None, "{text?, at? [x, y]}", has_graph, comment_add),
        cmd!("graph.comment.edit", "Edit comment", [], None, "{id, text?, at?, size?}", has_graph, comment_edit),
        cmd!("graph.comment.delete", "Delete comment", [], None, "{id}", has_graph, comment_delete),
        cmd!("graph.pin.add", "Add Navigation pin", [], None, "{name?, at? [x, y]}", has_graph, pin_add),
        cmd!("graph.pin.delete", "Delete pin", [], None, "{id}", has_graph, pin_delete),
        cmd!(
            "graph.exposed.add",
            "Expose parameter",
            [],
            None,
            "{id, label?, default (a typed value or number), min?, max?, group?, bind?: {node, param}}",
            has_graph,
            exposed_add
        ),
        cmd!("graph.exposed.set", "Set input value", [], None, "{id, value}", has_graph, exposed_set),
        cmd!("graph.exposed.delete", "Delete input", [], None, "{id}", has_graph, exposed_delete),
        cmd!(
            "graph.preset.save",
            "Save preset",
            [],
            None,
            "{name} stores the current input values as a preset (replacing one of that name)",
            has_graph,
            preset_save
        ),
        cmd!("graph.preset.apply", "Apply preset", [], None, "{name}", has_graph, preset_apply),
        cmd!("graph.preset.delete", "Delete preset", [], None, "{name}", has_graph, preset_delete),
        cmd!("graph.select", "Select nodes", [], None, "{ids, add?: bool}", has_graph, select),
        cmd!("graph.selectUpstream", "Select upstream nodes", [], None, "{ids?}", has_selection, |s, p| select_stream(s, p, true)),
        cmd!("graph.selectDownstream", "Select downstream nodes", [], None, "{ids?}", has_selection, |s, p| select_stream(s, p, false)),
        cmd!("graph.align", "Align nodes", [], None, "{mode: left|right|top|bottom|hcenter|vcenter|hdistribute|vdistribute, ids?}", has_selection, align),
        cmd!(
            "graph.createFromSelection",
            "Create graph from selection",
            [],
            None,
            "{name?, ids?} moves the nodes into a new graph and replaces them with an instance",
            has_selection,
            from_selection
        ),
    ]
}

fn graph_index(s: &Session, p: &J) -> Result<usize> {
    match p.get("graph") {
        None | Some(J::Null) => Ok(s.graph),
        Some(J::String(name)) => s.pkg.graph_by_name(name).map(|(i, _)| i).ok_or_else(|| bad("graph", format!("no graph `{name}`"))),
        Some(v) => {
            let i = v.as_u64().ok_or_else(|| bad("graph", "`graph` is an index or a name"))? as usize;
            s.pkg.graph(i)?;
            Ok(i)
        }
    }
}

fn open(s: &mut Session, p: &J) -> Result<J> {
    let i = graph_index(s, p)?;
    s.graph = i;
    s.selection.clear();
    s.view2d.node = None;
    s.dirty = true;
    Ok(json!({"graph": i, "name": s.pkg.graphs.get(i).map(|g| g.name.clone())}))
}

fn rename(s: &mut Session, p: &J) -> Result<J> {
    let i = graph_index(s, p)?;
    let name = strp(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("graph.rename", "`name` is required"))?.to_string();
    if s.pkg.graphs.iter().enumerate().any(|(j, g)| j != i && g.name == name) {
        return Err(bad("graph.rename", format!("a graph is already called `{name}`")));
    }
    s.edit("Rename graph", |s| {
        let old = std::mem::replace(&mut s.pkg.graph_mut(i)?.name, name.clone());
        // Instances follow the rename.
        for g in &mut s.pkg.graphs {
            for n in g.nodes.iter_mut().filter(|n| n.def == "graph_instance") {
                if n.params.get("graph") == Some(&Value::Text(old.clone())) {
                    n.params.insert("graph".into(), Value::Text(name.clone()));
                }
            }
        }
        Ok(json!({"name": name}))
    })
}

fn delete(s: &mut Session, p: &J) -> Result<J> {
    let i = graph_index(s, p)?;
    s.edit("Delete graph", |s| {
        s.pkg.graphs.remove(i);
        s.graph = s.graph.min(s.pkg.graphs.len().saturating_sub(1));
        s.selection.clear();
        s.view2d.node = None;
        Ok(json!({"graphs": s.pkg.graphs.len()}))
    })
}

fn duplicate(s: &mut Session, p: &J) -> Result<J> {
    let i = graph_index(s, p)?;
    s.edit("Duplicate graph", |s| {
        let mut g = s.pkg.graph(i)?.clone();
        g.reidentify();
        g.name = s.pkg.unique_graph_name(&g.name);
        s.pkg.graphs.push(g);
        s.graph = s.pkg.graphs.len() - 1;
        s.selection.clear();
        Ok(json!({"graph": s.graph}))
    })
}

fn size(s: &mut Session, p: &J) -> Result<J> {
    let log = |px: u64| 63 - px.clamp(1, 1 << 20).leading_zeros();
    let (w, h) = if let Some(a) = p.get("log2").and_then(J::as_array) {
        (a.first().and_then(J::as_u64).unwrap_or(10) as u32, a.get(1).and_then(J::as_u64).unwrap_or(10) as u32)
    } else {
        let w = u64p(p, "width").ok_or_else(|| bad("graph.size", "`width` is required"))?;
        (log(w), log(u64p(p, "height").unwrap_or(w)))
    };
    let (w, h) = (w.clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2), h.clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2));
    s.edit("Graph size", |s| {
        s.graph_mut()?.size = [w, h];
        Ok(json!({"width": 1u32 << w, "height": 1u32 << h}))
    })
}

fn format(s: &mut Session, p: &J) -> Result<J> {
    let f = match strp(p, "format") {
        Some("u8" | "8") => OutputFormat::U8,
        Some("u16" | "16") => OutputFormat::U16,
        Some("f16") => OutputFormat::F16,
        Some("f32" | "32") => OutputFormat::F32,
        _ => return Err(bad("graph.format", "`format` is u8, u16, f16 or f32")),
    };
    s.edit("Output format", |s| {
        s.graph_mut()?.format = f;
        Ok(J::Null)
    })
}

fn cook(s: &mut Session, p: &J) -> Result<J> {
    let cooked = if boolp(p, "full") == Some(true) { s.cook_full(s.graph) } else { s.cook().clone() };
    let g = s.graph()?;
    Ok(json!({
        "ms": cooked.ms,
        "nodes": cooked.nodes.iter().map(|(id, r)| {
            let n = g.node(*id).ok();
            json!({"id": id.0, "def": n.map(|n| n.def.clone()), "ms": r.ms, "cached": r.cached, "error": r.error,
                   "size": r.outputs.first().map(|i| [i.w, i.h])})
        }).collect::<Vec<_>>(),
    }))
}

fn attributes(s: &mut Session, p: &J) -> Result<J> {
    s.edit("Graph attributes", |s| {
        let a = &mut s.graph_mut()?.attributes;
        for (k, slot) in [("category", &mut a.category), ("description", &mut a.description), ("author", &mut a.author), ("tags", &mut a.tags)] {
            if let Some(v) = strp(p, k) {
                *slot = v.into();
            }
        }
        Ok(J::Null)
    })
}

fn rect_of(p: &J) -> Option<[f32; 4]> {
    let a = p.get("rect")?.as_array()?;
    let v: Vec<f32> = a.iter().filter_map(J::as_f64).map(|v| v as f32).filter(|v| v.is_finite()).collect();
    (v.len() == 4).then(|| [v[0], v[1], v[2].max(10.0), v[3].max(10.0)])
}

fn color_of(p: &J) -> Option<[f32; 4]> {
    materialcraft_core::parse_color(p.get("color")?)
}

/// Graph-space bounding rect of nodes `ids` (node cards are about 96×110).
pub fn nodes_rect(s: &Session, ids: &[NodeId]) -> Option<[f32; 4]> {
    let g = s.graph().ok()?;
    let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for n in g.nodes.iter().filter(|n| ids.contains(&n.id)) {
        r = [r[0].min(n.pos[0]), r[1].min(n.pos[1]), r[2].max(n.pos[0] + 96.0), r[3].max(n.pos[1] + 110.0)];
    }
    (r[0] < f32::MAX).then(|| [r[0], r[1], r[2] - r[0], r[3] - r[1]])
}

fn frame_add(s: &mut Session, p: &J) -> Result<J> {
    let rect = match rect_of(p) {
        Some(r) => r,
        None => nodes_rect(s, &s.selection.clone()).map(|r| [r[0] - 24.0, r[1] - 44.0, r[2] + 48.0, r[3] + 68.0]).unwrap_or([0.0, 0.0, 400.0, 260.0]),
    };
    let title = strp(p, "title").unwrap_or("Frame").to_string();
    let color = color_of(p).unwrap_or([0.55, 0.38, 0.12, 0.35]);
    s.edit("Add frame", |s| {
        let g = s.graph_mut()?;
        let id = g.alloc_id();
        g.frames.push(Frame { id, title, description: String::new(), rect, color });
        Ok(json!({"id": id}))
    })
}

fn frame_edit(s: &mut Session, p: &J) -> Result<J> {
    let id = u64p(p, "id").ok_or_else(|| bad("graph.frame.edit", "`id` is required"))?;
    s.edit_merged(&format!("frame{id}"), "Edit frame", |s| {
        let f = s.graph_mut()?.frames.iter_mut().find(|f| f.id == id).ok_or_else(|| bad("graph.frame.edit", format!("no frame {id}")))?;
        if let Some(t) = strp(p, "title") {
            f.title = t.into();
        }
        if let Some(t) = strp(p, "description") {
            f.description = t.into();
        }
        if let Some(r) = rect_of(p) {
            f.rect = r;
        }
        if let Some(c) = color_of(p) {
            f.color = c;
        }
        Ok(J::Null)
    })
}

fn frame_delete(s: &mut Session, p: &J) -> Result<J> {
    let id = u64p(p, "id").ok_or_else(|| bad("graph.frame.delete", "`id` is required"))?;
    s.edit("Delete frame", |s| {
        let g = s.graph_mut()?;
        let n = g.frames.len();
        g.frames.retain(|f| f.id != id);
        if n == g.frames.len() {
            return Err(bad("graph.frame.delete", format!("no frame {id}")));
        }
        Ok(J::Null)
    })
}

fn comment_add(s: &mut Session, p: &J) -> Result<J> {
    let at = point(p, "at").or_else(|| nodes_rect(s, &s.selection.clone()).map(|r| [r[0], r[1] - 90.0])).unwrap_or([0.0, -120.0]);
    let text = strp(p, "text").unwrap_or("Comment").to_string();
    s.edit("Add comment", |s| {
        let g = s.graph_mut()?;
        let id = g.alloc_id();
        g.comments.push(Comment { id, text, pos: at, size: [180.0, 70.0] });
        Ok(json!({"id": id}))
    })
}

fn comment_edit(s: &mut Session, p: &J) -> Result<J> {
    let id = u64p(p, "id").ok_or_else(|| bad("graph.comment.edit", "`id` is required"))?;
    s.edit_merged(&format!("comment{id}"), "Edit comment", |s| {
        let c = s.graph_mut()?.comments.iter_mut().find(|c| c.id == id).ok_or_else(|| bad("graph.comment.edit", format!("no comment {id}")))?;
        if let Some(t) = strp(p, "text") {
            c.text = t.into();
        }
        if let Some(a) = point(p, "at") {
            c.pos = a;
        }
        if let Some(sz) = point(p, "size") {
            c.size = [sz[0].max(40.0), sz[1].max(24.0)];
        }
        Ok(J::Null)
    })
}

fn comment_delete(s: &mut Session, p: &J) -> Result<J> {
    let id = u64p(p, "id").ok_or_else(|| bad("graph.comment.delete", "`id` is required"))?;
    s.edit("Delete comment", |s| {
        s.graph_mut()?.comments.retain(|c| c.id != id);
        Ok(J::Null)
    })
}

fn pin_add(s: &mut Session, p: &J) -> Result<J> {
    let at = point(p, "at").unwrap_or([0.0, 0.0]);
    s.edit("Add pin", |s| {
        let g = s.graph_mut()?;
        let id = g.alloc_id();
        let name = strp(p, "name").map(str::to_string).unwrap_or_else(|| format!("Pin {}", g.pins.len() + 1));
        g.pins.push(Pin { id, name, pos: at });
        Ok(json!({"id": id}))
    })
}

fn pin_delete(s: &mut Session, p: &J) -> Result<J> {
    let id = u64p(p, "id").ok_or_else(|| bad("graph.pin.delete", "`id` is required"))?;
    s.edit("Delete pin", |s| {
        s.graph_mut()?.pins.retain(|c| c.id != id);
        Ok(J::Null)
    })
}

fn exposed_add(s: &mut Session, p: &J) -> Result<J> {
    let id = strp(p, "id").map(str::trim).filter(|i| !i.is_empty()).ok_or_else(|| bad("graph.exposed.add", "`id` is required"))?.to_string();
    if s.graph()?.exposed.iter().any(|e| e.id == id) {
        return Err(bad("graph.exposed.add", format!("an input is already called `{id}`")));
    }
    // From a bound node parameter: its value, range and options become the input's.
    let bind = p.get("bind").and_then(|b| Some((NodeId(b.get("node")?.as_u64()?), b.get("param")?.as_str()?.to_string())));
    let mut default = Value::Float(0.5);
    let (mut min, mut max, mut options) = (0.0f32, 1.0f32, Vec::new());
    if let Some((node, param)) = &bind {
        let g = s.graph()?;
        let n = g.node(*node).map_err(|_| bad("graph.exposed.add", format!("no node {}", node.0)))?;
        let def = materialcraft_nodes::find(&n.def).ok_or_else(|| bad("graph.exposed.add", "unknown node type"))?;
        let pd = def.param(param).ok_or_else(|| bad("graph.exposed.add", format!("`{}` has no parameter `{param}`", n.def)))?;
        default = n.params.get(param.as_str()).cloned().unwrap_or_else(|| pd.default.clone());
        (min, max) = (pd.min, pd.max);
        options = pd.options.iter().map(|o| o.to_string()).collect();
    }
    if let Some(d) = p.get("default") {
        default =
            serde_json::from_value::<Value>(d.clone()).ok().or_else(|| default.coerce_json(d)).ok_or_else(|| bad("graph.exposed.add", "bad `default`"))?;
    }
    let ep = ExposedParam {
        id: id.clone(),
        label: strp(p, "label").unwrap_or(&id).to_string(),
        group: strp(p, "group").unwrap_or("").to_string(),
        default,
        min: f32p(p, "min").unwrap_or(min),
        max: f32p(p, "max").unwrap_or(max),
        options,
    };
    s.edit("Expose parameter", |s| {
        let g = s.graph_mut()?;
        g.exposed.push(ep);
        if let Some((node, param)) = bind {
            g.node_mut(node)?.bindings.insert(param, id.clone());
        }
        Ok(json!({"id": id}))
    })
}

fn exposed_set(s: &mut Session, p: &J) -> Result<J> {
    let id = strp(p, "id").ok_or_else(|| bad("graph.exposed.set", "`id` is required"))?.to_string();
    let v = p.get("value").ok_or_else(|| bad("graph.exposed.set", "`value` is required"))?;
    let cur = s.graph()?.exposed.iter().find(|e| e.id == id).map(|e| e.default.clone()).ok_or_else(|| bad("graph.exposed.set", format!("no input `{id}`")))?;
    let val = cur.coerce_json(v).ok_or_else(|| bad("graph.exposed.set", format!("`{id}` takes a {}", cur.type_name())))?;
    s.edit_merged(&format!("exposed:{id}"), "Set input", |s| {
        s.graph_mut()?.inputs.insert(id.clone(), val);
        Ok(J::Null)
    })
}

fn exposed_delete(s: &mut Session, p: &J) -> Result<J> {
    let id = strp(p, "id").ok_or_else(|| bad("graph.exposed.delete", "`id` is required"))?.to_string();
    s.edit("Delete input", |s| {
        let g = s.graph_mut()?;
        g.exposed.retain(|e| e.id != id);
        g.inputs.remove(&id);
        for n in &mut g.nodes {
            n.bindings.retain(|_, v| *v != id);
        }
        Ok(J::Null)
    })
}

fn preset_name(p: &J, cmd: &str) -> Result<String> {
    strp(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).ok_or_else(|| bad(cmd, "`name` is required"))
}

fn preset_save(s: &mut Session, p: &J) -> Result<J> {
    let name = preset_name(p, "graph.preset.save")?;
    s.edit("Save preset", |s| {
        let g = s.graph_mut()?;
        let values = g.exposed.iter().filter_map(|e| g.input_value(&e.id).map(|v| (e.id.clone(), v.clone()))).collect();
        g.presets.retain(|x| x.name != name);
        g.presets.push(materialcraft_core::Preset { name: name.clone(), values });
        Ok(json!({"presets": g.presets.len()}))
    })
}

fn preset_apply(s: &mut Session, p: &J) -> Result<J> {
    let name = preset_name(p, "graph.preset.apply")?;
    s.edit("Apply preset", |s| {
        let g = s.graph_mut()?;
        let preset = g.presets.iter().find(|x| x.name == name).cloned().ok_or_else(|| bad("graph.preset.apply", format!("no preset `{name}`")))?;
        for (k, v) in preset.values {
            // Only inputs that still exist, with their current type.
            if g.exposed.iter().any(|e| e.id == k && e.default.type_name() == v.type_name()) {
                g.inputs.insert(k, v);
            }
        }
        Ok(J::Null)
    })
}

fn preset_delete(s: &mut Session, p: &J) -> Result<J> {
    let name = preset_name(p, "graph.preset.delete")?;
    s.edit("Delete preset", |s| {
        s.graph_mut()?.presets.retain(|x| x.name != name);
        Ok(J::Null)
    })
}

fn select(s: &mut Session, p: &J) -> Result<J> {
    let ids = ids_param(s, "graph.select", p)?;
    if boolp(p, "add") == Some(true) {
        for id in ids {
            if !s.selection.contains(&id) {
                s.selection.push(id);
            }
        }
    } else {
        s.selection = ids;
    }
    Ok(json!({"selection": s.selection.iter().map(|i| i.0).collect::<Vec<_>>()}))
}

fn select_stream(s: &mut Session, p: &J, up: bool) -> Result<J> {
    let ids = ids_param(s, "graph.select", p)?;
    let g = s.graph()?;
    let mut sel = ids.clone();
    for id in ids {
        for n in if up { g.upstream(id) } else { g.downstream(id) } {
            if !sel.contains(&n) {
                sel.push(n);
            }
        }
    }
    s.selection = sel;
    Ok(json!({"selected": s.selection.len()}))
}

fn align(s: &mut Session, p: &J) -> Result<J> {
    let ids = ids_param(s, "graph.align", p)?;
    let mode = strp(p, "mode").unwrap_or("left").to_string();
    s.edit("Align nodes", |s| {
        let g = s.graph_mut()?;
        let mut nodes: Vec<&mut materialcraft_core::Node> = g.nodes.iter_mut().filter(|n| ids.contains(&n.id)).collect();
        if nodes.len() < 2 {
            return Ok(J::Null);
        }
        let xs: Vec<f32> = nodes.iter().map(|n| n.pos[0]).collect();
        let ys: Vec<f32> = nodes.iter().map(|n| n.pos[1]).collect();
        let (minx, maxx) = (xs.iter().copied().fold(f32::MAX, f32::min), xs.iter().copied().fold(f32::MIN, f32::max));
        let (miny, maxy) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MIN, f32::max));
        match mode.as_str() {
            "left" => nodes.iter_mut().for_each(|n| n.pos[0] = minx),
            "right" => nodes.iter_mut().for_each(|n| n.pos[0] = maxx),
            "top" => nodes.iter_mut().for_each(|n| n.pos[1] = miny),
            "bottom" => nodes.iter_mut().for_each(|n| n.pos[1] = maxy),
            "hcenter" => nodes.iter_mut().for_each(|n| n.pos[0] = (minx + maxx) * 0.5),
            "vcenter" => nodes.iter_mut().for_each(|n| n.pos[1] = (miny + maxy) * 0.5),
            "hdistribute" | "vdistribute" => {
                let k = usize::from(mode == "vdistribute");
                nodes.sort_by(|a, b| a.pos[k].total_cmp(&b.pos[k]));
                let (lo, hi) = if k == 0 { (minx, maxx) } else { (miny, maxy) };
                let step = (hi - lo) / (nodes.len() - 1) as f32;
                for (i, n) in nodes.iter_mut().enumerate() {
                    n.pos[k] = lo + step * i as f32;
                }
            }
            other => return Err(bad("graph.align", format!("unknown mode `{other}`"))),
        }
        Ok(J::Null)
    })
}

fn from_selection(s: &mut Session, p: &J) -> Result<J> {
    let ids = ids_param(s, "graph.createFromSelection", p)?;
    let base = strp(p, "name").unwrap_or("Sub_graph").to_string();
    s.edit("Create graph from selection", |s| {
        let name = s.pkg.unique_graph_name(&base);
        let src = s.graph()?.clone();
        let mut sub = materialcraft_core::Graph::new(&name);
        sub.size = src.size;
        sub.next_id = src.next_id;
        sub.replace_topology(
            src.nodes.iter().filter(|n| ids.contains(&n.id)).cloned().collect(),
            src.connections.iter().filter(|c| ids.contains(&c.from) && ids.contains(&c.to)).cloned().collect(),
        );
        let incoming: Vec<_> = src.connections.iter().filter(|c| ids.contains(&c.to) && !ids.contains(&c.from)).cloned().collect();
        let outgoing: Vec<_> = src.connections.iter().filter(|c| ids.contains(&c.from) && !ids.contains(&c.to)).cloned().collect();
        let rect = nodes_rect(s, &ids).unwrap_or([0.0; 4]);
        // One graph input per incoming wire, one output per distinct outgoing port.
        let mut inputs = Vec::new();
        for (k, c) in incoming.iter().enumerate() {
            let ident = format!("input_{}", k + 1);
            let id = sub.add_node("input_color", [rect[0] - 160.0, rect[1] + k as f32 * 120.0]);
            sub.node_mut(id)?.params.insert("identifier".into(), Value::Text(ident.clone()));
            sub.connect(id, "output", c.to, &c.to_port)?;
            inputs.push((c.from, c.from_port.clone(), ident));
        }
        let mut outputs: Vec<(NodeId, String, String)> = Vec::new();
        for c in &outgoing {
            if outputs.iter().any(|(n, p, _)| *n == c.from && *p == c.from_port) {
                continue;
            }
            let ident = format!("output_{}", outputs.len() + 1);
            let id = sub.add_node("output", [rect[0] + rect[2] + 80.0, rect[1] + outputs.len() as f32 * 120.0]);
            let usage = materialcraft_nodes::atomic::USAGE_IDS.len() as u32 - 1;
            sub.node_mut(id)?.params.extend([("identifier".to_string(), Value::Text(ident.clone())), ("usage".to_string(), Value::Enum(usage))]);
            sub.connect(c.from, &c.from_port, id, "input")?;
            outputs.push((c.from, c.from_port.clone(), ident));
        }
        s.pkg.graphs.push(sub);
        let g = s.graph_mut()?;
        g.remove_nodes(&ids);
        let inst = g.add_node("graph_instance", [rect[0], rect[1]]);
        g.node_mut(inst)?.params.insert("graph".into(), Value::Text(name.clone()));
        for (from, port, ident) in inputs {
            g.connect(from, &port, inst, &ident)?;
        }
        for c in outgoing {
            if let Some((_, _, ident)) = outputs.iter().find(|(n, p, _)| *n == c.from && *p == c.from_port) {
                g.connect(inst, ident, c.to, &c.to_port)?;
            }
        }
        s.selection = vec![inst];
        Ok(json!({"graph": name, "instance": inst.0}))
    })
}
