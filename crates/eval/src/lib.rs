//! # materialcraft-eval
//!
//! Graph cooking, L2. [`cook`] walks a graph in dependency order, works out each node's output
//! size (relative to its input or the graph, or absolute, under an optional preview cap), resolves
//! parameters (defaults, overrides, bindings to exposed inputs) and runs the node's cook. Results
//! live in a content-addressed [`Cache`]: a node's key hashes its definition, parameters, size,
//! seed, tiling and its inputs' keys, so editing one node re-cooks only what lies downstream, and
//! undo/redo finds earlier results again. Graph instances cook their sub-graph recursively.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use materialcraft_core::{Graph, GraphTopology, Node, NodeId, Package, SizeMode, Value};
use materialcraft_image::{Image, Kind};
use materialcraft_nodes::{Ctx, NodeDef, PortKind, find, resolve_params};

/// Sub-graphs may nest this deep (a graph instancing itself would recurse forever).
pub const MAX_DEPTH: u32 = 8;

/// A port as the editor and the evaluator see it (catalog ports, or a sub-graph's inputs and
/// outputs for a graph instance).
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub id: String,
    pub label: String,
    pub kind: PortKind,
}

/// The input and output ports of `node` in `pkg`.
pub fn ports(pkg: &Package, node: &Node) -> (Vec<Port>, Vec<Port>) {
    if node.def == "graph_instance" {
        let Some((_, sub)) = instance_graph(pkg, node) else { return (Vec::new(), Vec::new()) };
        let io = |def: &str| -> Vec<Port> {
            sub.nodes
                .iter()
                .filter(|n| n.def == def || (def == "input" && n.def.starts_with("input_")))
                .map(|n| {
                    let id = identifier(n);
                    let kind = if n.def == "input_grayscale" {
                        PortKind::Gray
                    } else if n.def == "input_color" {
                        PortKind::Color
                    } else {
                        PortKind::Any
                    };
                    Port { label: id.clone(), id, kind }
                })
                .collect()
        };
        return (io("input"), io("output"));
    }
    let Some(def) = find(&node.def) else { return (Vec::new(), Vec::new()) };
    let conv = |p: &materialcraft_nodes::PortDef| Port { id: p.id.into(), label: p.label.into(), kind: p.kind };
    (def.inputs.iter().map(conv).collect(), def.outputs.iter().map(conv).collect())
}

/// The graph a graph-instance node refers to.
pub fn instance_graph<'a>(pkg: &'a Package, node: &Node) -> Option<(usize, &'a Graph)> {
    match node.params.get("graph") {
        Some(Value::Text(name)) => pkg.graph_by_name(name),
        _ => None,
    }
}

/// An input or output node's identifier (its `identifier` parameter, default "input"/"output").
pub fn identifier(n: &Node) -> String {
    match n.params.get("identifier") {
        Some(Value::Text(s)) if !s.is_empty() => s.clone(),
        _ => if n.def == "output" { "output" } else { "input" }.into(),
    }
}

/// One node's cooked result.
#[derive(Clone, Debug)]
pub struct NodeResult {
    /// One image per output port (empty when the cook failed).
    pub outputs: Arc<Vec<Image>>,
    pub error: Option<String>,
    /// Cook time in milliseconds (0 when served from the cache).
    pub ms: f32,
    pub key: u64,
    pub cached: bool,
}

/// Results of a cook, by node.
#[derive(Clone, Debug, Default)]
pub struct Cooked {
    pub nodes: BTreeMap<NodeId, NodeResult>,
    /// Total wall time of the cook in milliseconds.
    pub ms: f32,
    pub cancelled: bool,
}

impl Cooked {
    /// Output `port` of node `id`.
    pub fn image(&self, id: NodeId, port: usize) -> Option<&Image> {
        self.nodes.get(&id).and_then(|r| r.outputs.get(port))
    }
}

/// LRU cache of node outputs, bounded by bytes.
pub struct Cache {
    entries: HashMap<u64, (Arc<Vec<Image>>, usize, u64)>,
    topologies: HashMap<u64, (u64, Arc<GraphTopology>)>,
    #[cfg(test)]
    cook_counts: HashMap<NodeId, usize>,
    bytes: usize,
    cap: usize,
    tick: u64,
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new(1 << 30)
    }
}

impl Cache {
    pub fn new(cap_bytes: usize) -> Cache {
        Cache {
            entries: HashMap::new(),
            topologies: HashMap::new(),
            #[cfg(test)]
            cook_counts: HashMap::new(),
            bytes: 0,
            cap: cap_bytes,
            tick: 0,
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.topologies.clear();
        self.bytes = 0;
    }

    fn topology(&mut self, graph: &Graph) -> Arc<GraphTopology> {
        let entry = self.topologies.entry(graph.id()).or_insert_with(|| (graph.revision(), Arc::new(GraphTopology::build(graph))));
        if entry.0 != graph.revision() {
            *entry = (graph.revision(), Arc::new(GraphTopology::build(graph)));
        }
        Arc::clone(&entry.1)
    }

    fn get(&mut self, key: u64) -> Option<Arc<Vec<Image>>> {
        self.tick += 1;
        let t = self.tick;
        self.entries.get_mut(&key).map(|e| {
            e.2 = t;
            e.0.clone()
        })
    }

    fn put(&mut self, key: u64, v: Arc<Vec<Image>>) {
        let size: usize = v.iter().map(|i| i.data.len() * 4).sum();
        self.tick += 1;
        if let Some(old) = self.entries.insert(key, (v, size, self.tick)) {
            self.bytes = self.bytes.saturating_sub(old.1);
        }
        self.bytes += size;
        while self.bytes > self.cap && self.entries.len() > 1 {
            let Some((&k, _)) = self.entries.iter().min_by_key(|(_, e)| e.2) else { break };
            if let Some(e) = self.entries.remove(&k) {
                self.bytes = self.bytes.saturating_sub(e.1);
            }
        }
    }
}

/// What to cook and how.
pub struct Options<'a> {
    /// Cap on the larger side of every node's output (previews); `None` = full size.
    pub max_size: Option<u32>,
    /// Cook only these nodes and what they depend on (`None` = the whole graph).
    pub only: Option<Vec<NodeId>>,
    /// Images for the graph's input nodes, by identifier (graph instances).
    pub inputs: BTreeMap<String, Image>,
    /// Values for the graph's exposed parameters (graph instances), over the graph's own.
    pub overrides: BTreeMap<String, Value>,
    /// Decodes a package resource (bitmap) by name.
    pub resource: &'a (dyn Fn(&str) -> Option<Image> + Sync),
    /// Set from another thread to stop between nodes.
    pub cancel: Option<&'a AtomicBool>,
    pub depth: u32,
}

impl<'a> Options<'a> {
    pub fn new(resource: &'a (dyn Fn(&str) -> Option<Image> + Sync)) -> Options<'a> {
        Options { max_size: None, only: None, inputs: BTreeMap::new(), overrides: BTreeMap::new(), resource, cancel: None, depth: 0 }
    }
}

/// The magenta image a failed node shows.
pub fn error_image(w: u32, h: u32) -> Image {
    Image::filled(w, h, Kind::Color, [1.0, 0.0, 1.0, 1.0])
}

/// A fingerprint of the complete image, including its format and dimensions.
fn image_key(i: &Image) -> u64 {
    let mut hasher = DefaultHasher::new();
    (i.w, i.h, i.kind, i.data.len()).hash(&mut hasher);
    for value in i.data.iter() {
        value.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

fn hash_of(v: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Output size of `node` given its first connected input's size.
pub fn node_size(graph: &Graph, def: Option<&NodeDef>, node: &Node, input: Option<(u32, u32)>, max: Option<u32>) -> (u32, u32) {
    let parent = graph.size_px();
    let (bw, bh) = match node.base.size_mode {
        SizeMode::Absolute => {
            let s = |l: i32| 1u32 << l.clamp(4, 13);
            (s(node.base.size[0]), s(node.base.size[1]))
        }
        mode => {
            let base = match (mode, input, def.is_some_and(|d| d.generator)) {
                (SizeMode::RelativeToInput, Some(i), false) => i,
                _ => (parent[0], parent[1]),
            };
            let sh = |v: u32, o: i32| {
                let v = v as f64 * 2f64.powi(o.clamp(-12, 12));
                v.round().clamp(16.0, 8192.0) as u32
            };
            (sh(base.0, node.base.size[0]), sh(base.1, node.base.size[1]))
        }
    };
    match max {
        Some(m) if bw.max(bh) > m => {
            let k = m as f64 / bw.max(bh) as f64;
            (((bw as f64 * k).round() as u32).max(1), ((bh as f64 * k).round() as u32).max(1))
        }
        _ => (bw, bh),
    }
}

/// A function result (4 lanes) as a value of `like`'s type.
pub fn function_value(like: &Value, v: [f32; 4]) -> Option<Value> {
    let f = |x: f32| if x.is_finite() { x } else { 0.0 };
    Some(match like {
        Value::Float(_) => Value::Float(f(v[0])),
        Value::Float2(_) => Value::Float2([f(v[0]), f(v[1])]),
        Value::Float3(_) => Value::Float3([f(v[0]), f(v[1]), f(v[2])]),
        Value::Float4(_) => Value::Float4(v.map(f)),
        Value::Color(_) => Value::Color(v.map(f)),
        Value::Int(_) => Value::Int(f(v[0]).round().clamp(i32::MIN as f32, i32::MAX as f32) as i32),
        Value::Int2(_) => Value::Int2([f(v[0]).round() as i32, f(v[1]).round() as i32]),
        Value::Bool(_) => Value::Bool(f(v[0]) != 0.0),
        Value::Enum(_) => Value::Enum(f(v[0]).round().max(0.0) as u32),
        _ => return None,
    })
}

/// Cook graph `gi` of `pkg`.
pub fn cook(pkg: &Package, gi: usize, opts: &Options, cache: &mut Cache) -> Cooked {
    let t0 = now_ms();
    let mut out = Cooked::default();
    let Some(graph) = pkg.graphs.get(gi) else { return out };
    // The top-level graph's input nodes show their preview images; the caller's images win.
    let bound;
    let opts = if opts.depth == 0 && !graph.input_images.is_empty() {
        let mut inputs: BTreeMap<String, Image> = graph.input_images.iter().filter_map(|(id, res)| Some((id.clone(), (opts.resource)(res)?))).collect();
        inputs.extend(opts.inputs.iter().map(|(k, v)| (k.clone(), v.clone())));
        bound = Options { only: opts.only.clone(), inputs, overrides: opts.overrides.clone(), ..*opts };
        &bound
    } else {
        opts
    };
    let topology = cache.topology(graph);
    let needed: Option<std::collections::BTreeSet<NodeId>> = opts.only.as_ref().map(|ids| {
        let mut s: std::collections::BTreeSet<NodeId> = ids.iter().copied().collect();
        for id in ids {
            s.extend(topology.upstream(*id));
        }
        s
    });
    // Exposed parameter values: the graph's own, then the caller's overrides.
    let mut vars: BTreeMap<String, Value> = graph.exposed.iter().filter_map(|e| graph.input_value(&e.id).map(|v| (e.id.clone(), v.clone()))).collect();
    for (k, v) in &opts.overrides {
        vars.insert(k.clone(), v.clone());
    }
    for &id in &topology.order {
        if needed.as_ref().is_some_and(|s| !s.contains(&id)) {
            continue;
        }
        if opts.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            out.cancelled = true;
            break;
        }
        let Some(node) = topology.node(graph, id) else { continue };
        let r = cook_node(pkg, graph, &topology, node, &out, &vars, opts, cache);
        out.nodes.insert(id, r);
    }
    out.ms = (now_ms() - t0) as f32;
    out
}

fn cook_node(
    pkg: &Package,
    graph: &Graph,
    topology: &GraphTopology,
    node: &Node,
    done: &Cooked,
    vars: &BTreeMap<String, Value>,
    opts: &Options,
    cache: &mut Cache,
) -> NodeResult {
    let (in_ports, out_ports) = ports(pkg, node);
    let def = find(&node.def);
    // Gather inputs by port order, with their keys.
    let mut inputs: Vec<Option<Image>> = Vec::with_capacity(in_ports.len());
    let mut in_keys: Vec<u64> = Vec::with_capacity(in_ports.len());
    for p in &in_ports {
        let src = topology.input_of(graph, node.id, &p.id).and_then(|c| {
            let r = done.nodes.get(&c.from)?;
            let from = topology.node(graph, c.from)?;
            let idx = ports(pkg, from).1.iter().position(|o| o.id == c.from_port)?;
            Some((r.outputs.get(idx)?.clone(), r.key ^ hash_of(&idx)))
        });
        in_keys.push(src.as_ref().map_or(0, |s| s.1));
        inputs.push(src.map(|s| s.0));
    }
    let first = inputs.iter().flatten().next().map(|i| (i.w, i.h));
    let (w, h) = node_size(graph, def, node, first, opts.max_size);
    let fail = |msg: String, key: u64| NodeResult {
        outputs: Arc::new(vec![error_image(w, h); out_ports.len().max(1)]),
        error: Some(msg),
        ms: 0.0,
        key,
        cached: false,
    };
    // Parameters: defaults, overrides, then bindings to exposed inputs.
    let mut params = match def {
        Some(d) if node.def != "graph_instance" => resolve_params(d, &node.params),
        _ => node.params.clone(),
    };
    for (param, exposed) in &node.bindings {
        if let (Some(v), Some(cur)) = (vars.get(exposed), params.get(param))
            && let Some(c) = cur.coerce_json(&serde_json::to_value(v).unwrap_or_default())
        {
            params.insert(param.clone(), c);
        }
    }
    let seed = (graph.seed as u32).wrapping_mul(0x9e37_79b9).wrapping_add(node.base.seed as u32);
    // Dynamic parameters: functions of the graph's inputs, evaluated once per cook.
    for (param, f) in &node.functions {
        let Some(cur) = params.get(param) else { continue };
        match materialcraft_nodes::function::compile(f, vars, seed) {
            Ok(prog) => {
                let v = prog.eval([0.5, 0.5], [w as f32, h as f32], &[], true, &mut Vec::new());
                if let Some(c) = function_value(cur, v) {
                    params.insert(param.clone(), c);
                }
            }
            Err(e) => return fail(format!("function of `{param}`: {e}"), 0),
        }
    }
    let tile = node.base.tiling[0] && node.base.tiling[1];
    // Input nodes read the caller's image for their identifier.
    let input_img = node.def.starts_with("input_").then(|| opts.inputs.get(&identifier(node))).flatten();
    let resource_name = match params.get("resource") {
        Some(Value::Text(name)) => Some(name.as_str()),
        _ => None,
    };
    let resource_image = resource_name.and_then(opts.resource);
    let res_hash = resource_name.map(|name| (pkg.resource(name).map(|r| hash_of(&r.bytes)), resource_image.as_ref().map(image_key)));
    let sub_hash =
        if node.def == "graph_instance" { instance_graph(pkg, node).map_or(0, |(_, g)| hash_of(&serde_json::to_string(g).unwrap_or_default())) } else { 0 };
    let key = hash_of(&(
        &node.def,
        serde_json::to_string(&params).unwrap_or_default(),
        (w, h, seed, tile, node.disabled),
        &in_keys,
        res_hash,
        sub_hash,
        input_img.map(image_key),
        serde_json::to_string(vars).unwrap_or_default(),
    ));
    if node.def != "graph_instance"
        && let Some(v) = cache.get(key)
    {
        return NodeResult { outputs: v, error: None, ms: 0.0, key, cached: true };
    }
    #[cfg(test)]
    {
        *cache.cook_counts.entry(node.id).or_default() += 1;
    }
    let t0 = now_ms();
    let result: Result<Vec<Image>, String> = if node.disabled {
        Ok(vec![inputs.iter().flatten().next().cloned().unwrap_or_else(|| Image::filled(w, h, Kind::Gray, [0.0; 4])); out_ports.len().max(1)])
    } else if let Some(img) = input_img {
        let img = img.resize(w, h, tile);
        Ok(vec![if node.def == "input_grayscale" { img.to_gray() } else { img }])
    } else if node.def == "graph_instance" {
        cook_instance(pkg, node, &in_ports, &out_ports, &inputs, &params, (w, h), opts, cache)
    } else if let Some(d) = def {
        let resource = |name: &str| {
            if Some(name) == resource_name { resource_image.clone() } else { (opts.resource)(name) }
        };
        let ctx = Ctx { w, h, seed, tile, inputs: &inputs, params: &params, resource: &resource, vars };
        (d.cook)(&ctx)
    } else {
        Err(format!("unknown node `{}`", node.def))
    };
    match result {
        Ok(imgs) if imgs.len() == out_ports.len().max(1) || out_ports.is_empty() => {
            let key = if node.def == "graph_instance" { hash_of(&(key, imgs.iter().map(image_key).collect::<Vec<_>>())) } else { key };
            let v = Arc::new(imgs);
            cache.put(key, v.clone());
            NodeResult { outputs: v, error: None, ms: (now_ms() - t0) as f32, key, cached: false }
        }
        Ok(imgs) => fail(format!("`{}` made {} images for {} outputs", node.def, imgs.len(), out_ports.len()), key),
        Err(e) => fail(e, key),
    }
}

fn cook_instance(
    pkg: &Package,
    node: &Node,
    in_ports: &[Port],
    out_ports: &[Port],
    inputs: &[Option<Image>],
    params: &BTreeMap<String, Value>,
    size: (u32, u32),
    opts: &Options,
    cache: &mut Cache,
) -> Result<Vec<Image>, String> {
    if opts.depth >= MAX_DEPTH {
        return Err("graph instances nest too deep (does a graph use itself?)".into());
    }
    let (gi, sub) = instance_graph(pkg, node).ok_or("the instanced graph does not exist")?;
    let mut o = Options::new(opts.resource);
    o.max_size = opts.max_size;
    o.cancel = opts.cancel;
    o.depth = opts.depth + 1;
    o.inputs = in_ports.iter().zip(inputs).filter_map(|(p, i)| Some((p.id.clone(), i.clone()?))).collect();
    o.overrides = params.iter().filter(|(k, _)| sub.exposed.iter().any(|e| &e.id == *k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    let cooked = cook(pkg, gi, &o, cache);
    if cooked.cancelled {
        return Err("cancelled".into());
    }
    Ok(out_ports
        .iter()
        .map(|p| {
            sub.nodes
                .iter()
                .find(|n| n.def == "output" && identifier(n) == p.id)
                .and_then(|n| cooked.image(n.id, 0))
                .map(|i| i.resize(size.0, size.1, true))
                .unwrap_or_else(|| Image::filled(size.0, size.1, Kind::Gray, [0.0; 4]))
        })
        .collect())
}

/// The graph's output nodes with their usage id and image, in node order.
pub fn outputs<'c>(graph: &Graph, cooked: &'c Cooked) -> Vec<(NodeId, &'static str, String, Option<&'c Image>)> {
    graph
        .nodes
        .iter()
        .filter(|n| n.def == "output")
        .map(|n| {
            let usage = n.params.get("usage").and_then(Value::as_i32).unwrap_or(0).clamp(0, materialcraft_nodes::atomic::USAGE_IDS.len() as i32 - 1) as usize;
            (n.id, materialcraft_nodes::atomic::USAGE_IDS.get(usage).copied().unwrap_or("any"), identifier(n), cooked.image(n.id, 0))
        })
        .collect()
}

/// Milliseconds since the first call (0 on the web, where `std::time::Instant` is unavailable).
pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
    }
    #[cfg(target_arch = "wasm32")]
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft_core::Graph;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn cached_order_tracks_random_edits(edits in proptest::collection::vec((0u8..5, 0u8..12, 0u8..12), 1..100)) {
            let mut graph = Graph::new("random");
            let mut cache = Cache::default();
            for (action, from, to) in edits {
                let from = NodeId(u64::from(from) + 1);
                let to = NodeId(u64::from(to) + 1);
                match action {
                    0 => { graph.add_node("uniform", [0.0, 0.0]); }
                    1 => { graph.remove_nodes(&[from]); }
                    2 => { let _ = graph.connect(from, "output", to, "input"); }
                    3 => { graph.disconnect(to, "input"); }
                    _ => {
                        let mut ids: Vec<_> = graph.nodes.iter().map(|node| node.id).collect();
                        ids.reverse();
                        graph.reorder_nodes(&ids);
                    }
                }
                let topology = cache.topology(&graph);
                prop_assert_eq!(&topology.order, &graph.topo_order());
                let again = cache.topology(&graph);
                prop_assert!(Arc::ptr_eq(&topology, &again));
            }
        }
    }

    #[test]
    fn exactly_one_cook_per_dirty_node() {
        let mut graph = Graph::new("dirty");
        graph.size = [4, 4];
        let first = graph.add_node("uniform", [0.0, 0.0]);
        graph.node_mut(first).unwrap().params.insert("color".into(), Value::Color([0.1, 0.2, 0.3, 1.0]));
        let second = graph.add_node("uniform", [0.0, 0.0]);
        let output = graph.add_node("output", [0.0, 0.0]);
        graph.connect(first, "output", output, "input").unwrap();
        let mut package = Package::new("dirty");
        package.graphs.push(graph);
        let mut cache = Cache::default();
        let opts = Options::new(&no_res);
        cook(&package, 0, &opts, &mut cache);
        assert_eq!(cache.cook_counts, HashMap::from([(first, 1), (second, 1), (output, 1)]));
        cook(&package, 0, &opts, &mut cache);
        assert_eq!(cache.cook_counts, HashMap::from([(first, 1), (second, 1), (output, 1)]));
        package.graphs[0].node_mut(first).unwrap().params.insert("color".into(), Value::Color([0.2, 0.4, 0.6, 1.0]));
        cook(&package, 0, &opts, &mut cache);
        assert_eq!(cache.cook_counts, HashMap::from([(first, 2), (second, 1), (output, 2)]));
        package.graphs[0].disconnect(output, "input");
        cook(&package, 0, &opts, &mut cache);
        assert_eq!(cache.cook_counts, HashMap::from([(first, 2), (second, 1), (output, 3)]));
    }

    #[test]
    fn cycles_still_leave_cyclic_nodes_out() {
        let mut graph = Graph::new("cycle");
        let root = graph.add_node("uniform", [0.0, 0.0]);
        let first = graph.add_node("output", [0.0, 0.0]);
        let second = graph.add_node("output", [0.0, 0.0]);
        graph.replace_topology(
            graph.nodes.clone(),
            vec![
                materialcraft_core::Connection { from: first, from_port: "output".into(), to: second, to_port: "input".into() },
                materialcraft_core::Connection { from: second, from_port: "output".into(), to: first, to_port: "input".into() },
            ],
        );
        let mut package = Package::new("cycle");
        package.graphs.push(graph);
        let cooked = cook(&package, 0, &Options::new(&no_res), &mut Cache::default());
        assert_eq!(cooked.nodes.keys().copied().collect::<Vec<_>>(), vec![root]);
    }

    #[test]
    #[ignore = "release-only topology benchmark"]
    fn topology_500_nodes_1000_edits() {
        let mut graph = Graph::new("benchmark");
        graph.size = [4, 4];
        let first = graph.add_node("uniform", [0.0, 0.0]);
        let mut previous = first;
        for _ in 1..500 {
            let next = graph.add_node("uniform", [0.0, 0.0]);
            graph.connect(previous, "output", next, "input").unwrap();
            previous = next;
        }
        let mut package = Package::new("benchmark");
        package.graphs.push(graph);
        let mut cache = Cache::default();
        let options = Options::new(&no_res);
        let started = std::time::Instant::now();
        for iteration in 0..1_000 {
            package.graphs[0].node_mut(first).unwrap().params.insert("color".into(), Value::Color([iteration as f32 / 1000.0, 0.0, 0.0, 1.0]));
            let cooked = cook(&package, 0, &options, &mut cache);
            assert_eq!(cooked.nodes.len(), 500);
        }
        println!("topology_500_nodes_1000_edits: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0);
    }

    fn no_res(_: &str) -> Option<Image> {
        None
    }

    #[test]
    fn input_image_changes_outside_sample_invalidate_cache() {
        let mut graph = Graph::new("g");
        graph.size = [7, 7];
        let input = graph.add_node("input_grayscale", [0.0, 0.0]);
        let output = graph.add_node("output", [1.0, 0.0]);
        graph.connect(input, "output", output, "input").unwrap();
        let mut package = Package::new("p");
        package.graphs.push(graph);
        let mut options = Options::new(&no_res);
        let image = Image::filled(128, 128, Kind::Gray, [0.0; 4]);
        options.inputs.insert("input".into(), image.clone());
        let mut cache = Cache::default();
        let first = cook(&package, 0, &options, &mut cache);
        let mut changed = image;
        Arc::make_mut(&mut changed.data)[1] = 1.0;
        options.inputs.insert("input".into(), changed);
        let second = cook(&package, 0, &options, &mut cache);
        assert_ne!(first.image(output, 0).unwrap().data[1], second.image(output, 0).unwrap().data[1]);
    }

    #[test]
    fn resource_changes_after_prefix_invalidate_cache() {
        let mut graph = Graph::new("g");
        graph.size = [4, 4];
        let bitmap = graph.add_node("bitmap", [0.0, 0.0]);
        graph.node_mut(bitmap).unwrap().params.insert("resource".into(), Value::Text("image".into()));
        let mut package = Package::new("p");
        package.graphs.push(graph);
        package.resources.push(materialcraft_core::Resource { name: "image".into(), file: "image".into(), bytes: Arc::new(vec![0; 128]) });
        let first_resource = |_: &str| Some(Image::filled(16, 16, Kind::Gray, [0.0; 4]));
        let second_resource = |_: &str| Some(Image::filled(16, 16, Kind::Gray, [1.0; 4]));
        let mut cache = Cache::default();
        let first = cook(&package, 0, &Options::new(&first_resource), &mut cache);
        Arc::make_mut(&mut package.resources[0].bytes)[100] = 1;
        let second = cook(&package, 0, &Options::new(&second_resource), &mut cache);
        assert_ne!(first.image(bitmap, 0).unwrap().data[0], second.image(bitmap, 0).unwrap().data[0]);
    }

    #[test]
    fn instance_propagates_resource_changes_to_downstream_nodes() {
        let mut sub = Graph::new("sub");
        sub.size = [4, 4];
        let bitmap = sub.add_node("bitmap", [0.0, 0.0]);
        sub.node_mut(bitmap).unwrap().params.insert("resource".into(), Value::Text("image".into()));
        let sub_output = sub.add_node("output", [1.0, 0.0]);
        sub.connect(bitmap, "output", sub_output, "input").unwrap();
        let mut graph = Graph::new("main");
        graph.size = [4, 4];
        let instance = graph.add_node("graph_instance", [0.0, 0.0]);
        graph.node_mut(instance).unwrap().params.insert("graph".into(), Value::Text("sub".into()));
        let output = graph.add_node("output", [1.0, 0.0]);
        graph.connect(instance, "output", output, "input").unwrap();
        let mut package = Package::new("p");
        package.graphs.extend([graph, sub]);
        package.resources.push(materialcraft_core::Resource { name: "image".into(), file: "image".into(), bytes: Arc::new(vec![0; 128]) });
        let first_resource = |_: &str| Some(Image::filled(16, 16, Kind::Gray, [0.0; 4]));
        let second_resource = |_: &str| Some(Image::filled(16, 16, Kind::Gray, [1.0; 4]));
        let mut cache = Cache::default();
        let first = cook(&package, 0, &Options::new(&first_resource), &mut cache);
        Arc::make_mut(&mut package.resources[0].bytes)[100] = 1;
        let second = cook(&package, 0, &Options::new(&second_resource), &mut cache);
        assert_ne!(first.image(output, 0).unwrap().data[0], second.image(output, 0).unwrap().data[0]);
    }

    fn sample() -> (Package, NodeId, NodeId, NodeId) {
        let mut g = Graph::new("g");
        g.size = [6, 6];
        let noise = g.add_node("perlin_noise", [0.0, 0.0]);
        let blur = g.add_node("blur", [150.0, 0.0]);
        let out = g.add_node("output", [300.0, 0.0]);
        g.connect(noise, "output", blur, "input").unwrap();
        g.connect(blur, "output", out, "input").unwrap();
        let mut p = Package::new("p");
        p.graphs.push(g);
        (p, noise, blur, out)
    }

    #[test]
    fn cooks_and_caches() {
        let (mut p, noise, blur, out) = sample();
        let mut cache = Cache::default();
        let opts = Options::new(&no_res);
        let a = cook(&p, 0, &opts, &mut cache);
        assert!(a.nodes.values().all(|r| r.error.is_none() && !r.cached));
        assert_eq!(a.image(out, 0).map(|i| (i.w, i.h)), Some((64, 64)));
        // Same graph: everything from the cache.
        let b = cook(&p, 0, &opts, &mut cache);
        assert!(b.nodes.values().all(|r| r.cached));
        // Editing the blur re-cooks the blur and the output, not the noise.
        p.graphs[0].node_mut(blur).unwrap().params.insert("intensity".into(), Value::Float(5.0));
        let c = cook(&p, 0, &opts, &mut cache);
        assert!(c.nodes[&noise].cached && !c.nodes[&blur].cached && !c.nodes[&out].cached);
        assert_eq!(outputs(&p.graphs[0], &c).len(), 1);
    }

    #[test]
    fn preview_cap_and_unknown_nodes() {
        let (mut p, _, _, out) = sample();
        p.graphs[0].size = [10, 10];
        let bad = p.graphs[0].add_node("no_such_node", [0.0, 0.0]);
        let mut cache = Cache::default();
        let mut opts = Options::new(&no_res);
        opts.max_size = Some(128);
        let c = cook(&p, 0, &opts, &mut cache);
        assert_eq!(c.image(out, 0).map(|i| i.w), Some(128));
        assert!(c.nodes[&bad].error.is_some());
    }

    #[test]
    fn graph_instances_cook_their_subgraph() {
        let (mut p, ..) = sample();
        // A second graph: input → invert → output, instanced from the first.
        let mut sub = Graph::new("inv");
        sub.size = [6, 6];
        let i = sub.add_node("input_grayscale", [0.0, 0.0]);
        let inv = sub.add_node("invert", [100.0, 0.0]);
        let o = sub.add_node("output", [200.0, 0.0]);
        sub.connect(i, "output", inv, "input").unwrap();
        sub.connect(inv, "output", o, "input").unwrap();
        p.graphs.push(sub);
        let g = &mut p.graphs[0];
        let u = g.add_node("uniform", [0.0, 100.0]);
        g.node_mut(u).unwrap().params.insert("mode".into(), Value::Enum(1));
        g.node_mut(u).unwrap().params.insert("color".into(), Value::Color([0.25, 0.25, 0.25, 1.0]));
        let inst = g.add_node("graph_instance", [100.0, 100.0]);
        g.node_mut(inst).unwrap().params.insert("graph".into(), Value::Text("inv".into()));
        g.connect(u, "output", inst, "input").unwrap();
        // A graph instancing itself is refused, not a stack overflow.
        let selfi = g.add_node("graph_instance", [0.0, 200.0]);
        g.node_mut(selfi).unwrap().params.insert("graph".into(), Value::Text("g".into()));
        let mut cache = Cache::default();
        let c = cook(&p, 0, &Options::new(&no_res), &mut cache);
        let img = c.image(inst, 0).unwrap();
        assert!((img.data[0] - (1.0 - 0.0722 * 0.25 - 0.2126 * 0.25 - 0.7152 * 0.25)).abs() < 1e-4, "{}", img.data[0]);
        assert!(c.nodes[&selfi].error.is_some() || c.nodes[&selfi].outputs.len() == 1);
    }

    #[test]
    fn input_nodes_read_bound_preview_images() {
        let mut p = Package::new("p");
        let mut g = Graph::new("g");
        g.size = [4, 4];
        let i = g.add_node("input_grayscale", [0.0, 0.0]);
        g.node_mut(i).unwrap().params.insert("identifier".into(), Value::Text("curvature".into()));
        let o = g.add_node("output", [100.0, 0.0]);
        g.connect(i, "output", o, "input").unwrap();
        g.input_images.insert("curvature".into(), "curv".into());
        p.graphs.push(g);
        let res = |n: &str| (n == "curv").then(|| Image::filled(4, 4, Kind::Color, [0.8, 0.8, 0.8, 1.0]));
        let mut cache = Cache::default();
        let c = cook(&p, 0, &Options::new(&res), &mut cache);
        let img = c.image(o, 0).unwrap();
        assert!(img.is_gray() && (img.data[0] - 0.8).abs() < 1e-4, "bound image, as gray");
        // The caller's image wins over the bound one.
        let mut opts = Options::new(&res);
        opts.inputs.insert("curvature".into(), Image::filled(4, 4, Kind::Gray, [0.1; 4]));
        let c = cook(&p, 0, &opts, &mut cache);
        assert!((c.image(o, 0).unwrap().data[0] - 0.1).abs() < 1e-4);
    }
}
