//! Graphs: nodes, connections, frames, comments, navigation pins and exposed inputs.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

/// How a node's output size follows its surroundings ("Output size" base parameter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SizeMode {
    /// The size of the main (first connected) input; the graph size when none is connected.
    #[default]
    RelativeToInput,
    /// The graph ("parent") size.
    RelativeToParent,
    /// `offset` is the absolute log2 size (e.g. 10 = 1024 px).
    Absolute,
}

/// The output bit depth ("Output format").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputFormat {
    /// The main input's format; 16 bits for generators.
    #[default]
    Inherit,
    U8,
    U16,
    F16,
    F32,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 5] = [OutputFormat::Inherit, OutputFormat::U8, OutputFormat::U16, OutputFormat::F16, OutputFormat::F32];

    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Inherit => "Relative to input",
            OutputFormat::U8 => "8 bits per channel",
            OutputFormat::U16 => "16 bits per channel",
            OutputFormat::F16 => "16 bits per channel float (HDR)",
            OutputFormat::F32 => "32 bits per channel float (HDR)",
        }
    }

    /// The suffix of a node's size label: `8`, `16`, `16F`, `32F`.
    pub fn bits(self) -> &'static str {
        match self {
            OutputFormat::Inherit | OutputFormat::U16 => "16",
            OutputFormat::U8 => "8",
            OutputFormat::F16 => "16F",
            OutputFormat::F32 => "32F",
        }
    }
}

/// Parameters every node has ("Base parameters").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BaseParams {
    pub size_mode: SizeMode,
    /// log2 offset per axis (relative modes) or log2 size (absolute).
    pub size: [i32; 2],
    pub format: OutputFormat,
    /// Added to the graph's random seed for this node.
    pub seed: i32,
    /// Whether sampling wraps at the edges (horizontal, vertical).
    pub tiling: [bool; 2],
}

impl Default for BaseParams {
    fn default() -> Self {
        BaseParams { size_mode: SizeMode::RelativeToInput, size: [0, 0], format: OutputFormat::Inherit, seed: 0, tiling: [true, true] }
    }
}

/// A node instance: a catalog definition (`def`) with parameter overrides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: NodeId,
    /// Catalog id, e.g. `blend`, `perlin_noise`, `output`, `input_color`, `graph_instance`.
    pub def: String,
    /// User label; empty = the definition's label.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Graph-space position of the node's top-left corner.
    pub pos: [f32; 2],
    /// Values that differ from the catalog defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
    /// Parameters driven by a graph input (param id → exposed input id).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,
    /// Dynamic parameters: param id → a function graph computing its value from the graph's
    /// inputs at cook time.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub functions: BTreeMap<String, crate::function::FnGraph>,
    #[serde(default)]
    pub base: BaseParams,
    /// A disabled node passes its first input through.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

impl Node {
    pub fn new(id: NodeId, def: &str, pos: [f32; 2]) -> Node {
        Node {
            id,
            def: def.into(),
            label: String::new(),
            pos,
            params: BTreeMap::new(),
            bindings: BTreeMap::new(),
            functions: BTreeMap::new(),
            base: BaseParams::default(),
            disabled: false,
        }
    }
}

/// A link from an output port to an input port.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub from: NodeId,
    pub from_port: String,
    pub to: NodeId,
    pub to_port: String,
}

/// A titled, colored rectangle behind a group of nodes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub id: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// `[x, y, width, height]` in graph space.
    pub rect: [f32; 4],
    /// Linear RGBA (alpha is the fill opacity).
    pub color: [f32; 4],
}

/// A sticky note on the canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub text: String,
    pub pos: [f32; 2],
    #[serde(default = "comment_size")]
    pub size: [f32; 2],
}

fn comment_size() -> [f32; 2] {
    [180.0, 70.0]
}

/// A named bookmark on the canvas (Previous/Next pin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub id: u64,
    pub name: String,
    pub pos: [f32; 2],
}

/// The kind of value an exposed graph input carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposedParam {
    /// Identifier, unique in the graph.
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    pub default: Value,
    #[serde(default)]
    pub min: f32,
    #[serde(default = "one")]
    pub max: f32,
    /// Labels when the value is an enum.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

fn one() -> f32 {
    1.0
}

/// A named set of input values (graph presets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub values: BTreeMap<String, Value>,
}

/// Descriptive metadata ("Attributes").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Attributes {
    pub category: String,
    pub description: String,
    pub author: String,
    pub tags: String,
}

/// Whether a graph makes images or is a function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphKind {
    #[default]
    Material,
    Function,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    #[serde(skip, default = "new_graph_id")]
    id: u64,
    #[serde(skip, default = "new_revision")]
    revision: u64,
    pub name: String,
    #[serde(default)]
    pub kind: GraphKind,
    pub nodes: Vec<Node>,
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub frames: Vec<Frame>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub pins: Vec<Pin>,
    /// log2 of the graph ("parent") size per axis.
    pub size: [u32; 2],
    #[serde(default)]
    pub format: OutputFormat,
    #[serde(default)]
    pub seed: i32,
    #[serde(default)]
    pub exposed: Vec<ExposedParam>,
    /// Current values of the exposed inputs (missing = default).
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    /// Named sets of input values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<Preset>,
    /// Preview images of the graph's input nodes: identifier → bitmap resource (a mesh map
    /// baked in TextureCraft, say). Instances feed their inputs from their ports instead.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_images: BTreeMap<String, String>,
    #[serde(default)]
    pub attributes: Attributes,
    /// The id the next node, frame, comment or pin gets.
    pub next_id: u64,
}

static NEXT_GRAPH_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

fn new_graph_id() -> u64 {
    NEXT_GRAPH_ID.fetch_add(1, Ordering::Relaxed)
}

fn new_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

impl PartialEq for Graph {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.kind == other.kind
            && self.nodes == other.nodes
            && self.connections == other.connections
            && self.frames == other.frames
            && self.comments == other.comments
            && self.pins == other.pins
            && self.size == other.size
            && self.format == other.format
            && self.seed == other.seed
            && self.exposed == other.exposed
            && self.inputs == other.inputs
            && self.presets == other.presets
            && self.input_images == other.input_images
            && self.attributes == other.attributes
            && self.next_id == other.next_id
    }
}

/// Indexed edges and dependency order for a single graph revision.
pub struct GraphTopology {
    pub order: Vec<NodeId>,
    nodes: HashMap<NodeId, usize>,
    inputs: HashMap<NodeId, HashMap<String, usize>>,
    incoming: HashMap<NodeId, Vec<NodeId>>,
}

impl GraphTopology {
    pub fn build(graph: &Graph) -> Self {
        let mut nodes = HashMap::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            nodes.entry(node.id).or_insert(index);
        }
        let mut indegrees: BTreeMap<NodeId, usize> = nodes.keys().map(|id| (*id, 0)).collect();
        let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut inputs = HashMap::new();
        for (index, connection) in graph.connections.iter().enumerate() {
            inputs.entry(connection.to).or_insert_with(HashMap::new).entry(connection.to_port.clone()).or_insert(index);
            incoming.entry(connection.to).or_default().push(connection.from);
            if nodes.contains_key(&connection.from) {
                outgoing.entry(connection.from).or_default().push(connection.to);
                if let Some(degree) = indegrees.get_mut(&connection.to) {
                    *degree += 1;
                }
            }
        }
        let mut ready: Vec<NodeId> = indegrees.iter().filter(|(_, degree)| **degree == 0).map(|(id, _)| *id).collect();
        let mut order = Vec::with_capacity(graph.nodes.len());
        while let Some(node) = ready.pop() {
            order.push(node);
            if let Some(neighbors) = outgoing.get(&node) {
                for neighbor in neighbors {
                    if let Some(degree) = indegrees.get_mut(neighbor) {
                        *degree = degree.saturating_sub(1);
                        if *degree == 0 {
                            ready.push(*neighbor);
                        }
                    }
                }
            }
        }
        Self { order, nodes, inputs, incoming }
    }

    pub fn node<'a>(&self, graph: &'a Graph, id: NodeId) -> Option<&'a Node> {
        graph.nodes.get(*self.nodes.get(&id)?)
    }

    pub fn input_of<'a>(&self, graph: &'a Graph, id: NodeId, port: &str) -> Option<&'a Connection> {
        graph.connections.get(*self.inputs.get(&id)?.get(port)?)
    }

    pub fn upstream(&self, id: NodeId) -> std::collections::BTreeSet<NodeId> {
        let mut seen = std::collections::BTreeSet::new();
        let mut stack = vec![id];
        while let Some(node) = stack.pop() {
            if let Some(parents) = self.incoming.get(&node) {
                for parent in parents {
                    if seen.insert(*parent) {
                        stack.push(*parent);
                    }
                }
            }
        }
        seen
    }
}

/// Smallest and largest graph size, as log2 (16 px .. 8192 px).
pub const MIN_SIZE_LOG2: u32 = 4;
pub const MAX_SIZE_LOG2: u32 = 13;

impl Graph {
    pub fn new(name: &str) -> Graph {
        Graph {
            id: new_graph_id(),
            revision: new_revision(),
            name: name.into(),
            kind: GraphKind::Material,
            nodes: Vec::new(),
            connections: Vec::new(),
            frames: Vec::new(),
            comments: Vec::new(),
            pins: Vec::new(),
            size: [10, 10],
            format: OutputFormat::U16,
            seed: 0,
            exposed: Vec::new(),
            inputs: BTreeMap::new(),
            presets: Vec::new(),
            input_images: BTreeMap::new(),
            attributes: Attributes::default(),
            next_id: 1,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Mark direct edits to `nodes` or `connections` as a new topology revision.
    pub fn touch_topology(&mut self) {
        self.revision = new_revision();
    }

    /// Give a duplicated graph its own identity and revision.
    pub fn reidentify(&mut self) {
        self.id = new_graph_id();
        self.touch_topology();
    }

    /// Insert a node with an existing id (for example, from a clipboard).
    pub fn insert_node(&mut self, node: Node) {
        self.nodes.push(node);
        self.touch_topology();
    }

    /// Replace nodes and connections as one graph load operation.
    pub fn replace_topology(&mut self, nodes: Vec<Node>, connections: Vec<Connection>) {
        self.nodes = nodes;
        self.connections = connections;
        self.touch_topology();
    }

    /// Reorder nodes without changing their ids or connections.
    pub fn reorder_nodes(&mut self, order: &[NodeId]) -> bool {
        if order.len() != self.nodes.len()
            || order.iter().copied().collect::<std::collections::BTreeSet<_>>() != self.nodes.iter().map(|node| node.id).collect()
        {
            return false;
        }
        let positions: HashMap<NodeId, usize> = order.iter().enumerate().map(|(index, id)| (*id, index)).collect();
        self.nodes.sort_by_key(|node| positions.get(&node.id).copied());
        self.touch_topology();
        true
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.touch_topology();
        id
    }

    pub fn node(&self, id: NodeId) -> Result<&Node> {
        self.nodes.iter().find(|n| n.id == id).ok_or(CoreError::NoNode(id.0))
    }

    /// Property edits leave topology unchanged; changing the node id requires `touch_topology`.
    pub fn node_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id).ok_or(CoreError::NoNode(id.0))
    }

    /// Add a node of definition `def` at `pos`; returns its id.
    pub fn add_node(&mut self, def: &str, pos: [f32; 2]) -> NodeId {
        let id = NodeId(self.alloc_id());
        self.nodes.push(Node::new(id, def, pos));
        self.touch_topology();
        id
    }

    /// Remove nodes and every connection touching them.
    pub fn remove_nodes(&mut self, ids: &[NodeId]) {
        let before = (self.nodes.len(), self.connections.len());
        self.nodes.retain(|n| !ids.contains(&n.id));
        self.connections.retain(|c| !ids.contains(&c.from) && !ids.contains(&c.to));
        if before != (self.nodes.len(), self.connections.len()) {
            self.touch_topology();
        }
    }

    /// Connect `from.from_port` → `to.to_port`, replacing whatever fed that input. Refuses
    /// connections that would make a cycle.
    pub fn connect(&mut self, from: NodeId, from_port: &str, to: NodeId, to_port: &str) -> Result<()> {
        self.node(from)?;
        self.node(to)?;
        if from == to || self.reaches(to, from) {
            return Err(CoreError::Cycle);
        }
        self.connections.retain(|c| !(c.to == to && c.to_port == to_port));
        self.connections.push(Connection { from, from_port: from_port.into(), to, to_port: to_port.into() });
        self.touch_topology();
        Ok(())
    }

    pub fn disconnect(&mut self, to: NodeId, to_port: &str) -> bool {
        let before = self.connections.len();
        self.connections.retain(|c| !(c.to == to && c.to_port == to_port));
        let removed = before != self.connections.len();
        if removed {
            self.touch_topology();
        }
        removed
    }

    /// The connection feeding `to.to_port`.
    pub fn input_of(&self, to: NodeId, to_port: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.to == to && c.to_port == to_port)
    }

    /// Whether `b` is downstream of (or equal to) `a`.
    pub fn reaches(&self, a: NodeId, b: NodeId) -> bool {
        let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for connection in &self.connections {
            outgoing.entry(connection.from).or_default().push(connection.to);
        }
        let mut stack = vec![a];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(n) = stack.pop() {
            if n == b {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            stack.extend(outgoing.get(&n).into_iter().flatten().copied());
        }
        false
    }

    /// Nodes in dependency order (inputs before the nodes they feed). Nodes on a cycle (only
    /// possible in a hand-edited file) are left out.
    pub fn topo_order(&self) -> Vec<NodeId> {
        GraphTopology::build(self).order
    }

    /// Every node upstream of `id` (not including it).
    pub fn upstream(&self, id: NodeId) -> Vec<NodeId> {
        let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for connection in &self.connections {
            incoming.entry(connection.to).or_default().push(connection.from);
        }
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            for &source in incoming.get(&n).into_iter().flatten() {
                if !out.contains(&source) {
                    out.push(source);
                    stack.push(source);
                }
            }
        }
        out
    }

    /// Every node downstream of `id` (not including it).
    pub fn downstream(&self, id: NodeId) -> Vec<NodeId> {
        let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for connection in &self.connections {
            outgoing.entry(connection.from).or_default().push(connection.to);
        }
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            for &target in outgoing.get(&n).into_iter().flatten() {
                if !out.contains(&target) {
                    out.push(target);
                    stack.push(target);
                }
            }
        }
        out
    }

    /// The value of exposed input `id` (its default when unset).
    pub fn input_value(&self, id: &str) -> Option<&Value> {
        self.inputs.get(id).or_else(|| self.exposed.iter().find(|e| e.id == id).map(|e| &e.default))
    }

    /// Graph size in pixels.
    pub fn size_px(&self) -> [u32; 2] {
        [1 << self.size[0].clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2), 1 << self.size[1].clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn topology_revision_mutator_checklist() {
        let expected = [
            "alloc_id",
            "add_node",
            "insert_node",
            "remove_nodes",
            "connect",
            "disconnect",
            "reorder_nodes",
            "replace_topology",
            "touch_topology",
            "reidentify",
            "deserialize",
        ];
        let mut observed = Vec::new();
        let mut graph = Graph::new("revision");
        let mut previous = graph.revision();
        macro_rules! bumped {
            ($name:literal, $edit:expr) => {{
                $edit;
                assert!(graph.revision() > previous, "{} did not advance the topology revision", $name);
                previous = graph.revision();
                observed.push($name);
            }};
        }
        bumped!("alloc_id", graph.alloc_id());
        bumped!("add_node", graph.add_node("uniform", [0.0, 0.0]));
        let source = graph.nodes[0].id;
        let node = Node::new(NodeId(99), "output", [0.0, 0.0]);
        bumped!("insert_node", graph.insert_node(node));
        bumped!("remove_nodes", graph.remove_nodes(&[NodeId(99)]));
        let sink = graph.add_node("output", [0.0, 0.0]);
        assert!(graph.revision() > previous);
        previous = graph.revision();
        bumped!("connect", graph.connect(source, "output", sink, "input").unwrap());
        bumped!("disconnect", graph.disconnect(sink, "input"));
        bumped!("reorder_nodes", assert!(graph.reorder_nodes(&[sink, source])));
        bumped!("replace_topology", graph.replace_topology(graph.nodes.clone(), Vec::new()));
        bumped!("touch_topology", graph.touch_topology());
        bumped!("reidentify", graph.reidentify());
        let encoded = serde_json::to_string(&graph).unwrap();
        let loaded: Graph = serde_json::from_str(&encoded).unwrap();
        assert_ne!(loaded.id(), graph.id());
        assert!(loaded.revision() > previous);
        observed.push("deserialize");
        assert_eq!(observed, expected);
        let revision = graph.revision();
        assert!(!graph.disconnect(sink, "missing"));
        assert!(graph.connect(sink, "output", sink, "input").is_err());
        graph.remove_nodes(&[]);
        assert_eq!(graph.revision(), revision);
    }

    fn reference_order(graph: &Graph) -> Vec<NodeId> {
        let mut degrees: BTreeMap<NodeId, usize> = graph.nodes.iter().map(|node| (node.id, 0)).collect();
        for connection in &graph.connections {
            if let Some(degree) = degrees.get_mut(&connection.to)
                && graph.nodes.iter().any(|node| node.id == connection.from)
            {
                *degree += 1;
            }
        }
        let mut ready: Vec<NodeId> = degrees.iter().filter(|(_, degree)| **degree == 0).map(|(id, _)| *id).collect();
        let mut order = Vec::new();
        while let Some(node) = ready.pop() {
            order.push(node);
            for connection in graph.connections.iter().filter(|connection| connection.from == node) {
                if let Some(degree) = degrees.get_mut(&connection.to) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.push(connection.to);
                    }
                }
            }
        }
        order
    }

    proptest! {
        #[test]
        fn indexed_order_matches_fresh_after_edits(edits in proptest::collection::vec((0u8..7, 0u8..12, 0u8..12), 1..120)) {
            let mut graph = Graph::new("random");
            for (action, source, target) in edits {
                match action {
                    0 => { graph.add_node("uniform", [0.0, 0.0]); }
                    1 => { graph.remove_nodes(&[NodeId(u64::from(source) + 1)]); }
                    2 => { let _ = graph.connect(NodeId(u64::from(source) + 1), "output", NodeId(u64::from(target) + 1), "input"); }
                    3 => { graph.disconnect(NodeId(u64::from(target) + 1), "input"); }
                    4 => {
                        let mut nodes: Vec<NodeId> = graph.nodes.iter().map(|node| node.id).collect();
                        nodes.reverse();
                        graph.reorder_nodes(&nodes);
                    }
                    5 => {
                        let mut connections = graph.connections.clone();
                        connections.push(Connection { from: NodeId(u64::from(source) + 1), from_port: "output".into(), to: NodeId(u64::from(target) + 1), to_port: "input".into() });
                        graph.replace_topology(graph.nodes.clone(), connections);
                    }
                    _ => { graph.touch_topology(); }
                }
                prop_assert_eq!(graph.topo_order(), reference_order(&graph));
            }
        }
    }

    #[test]
    fn indexed_order_preserves_cycles_duplicate_edges_and_dangling_sources() {
        let mut graph = Graph::new("malformed");
        let first = graph.add_node("uniform", [0.0, 0.0]);
        let second = graph.add_node("output", [0.0, 0.0]);
        graph.replace_topology(
            graph.nodes.clone(),
            vec![
                Connection { from: first, from_port: "output".into(), to: second, to_port: "input".into() },
                Connection { from: first, from_port: "output".into(), to: second, to_port: "input".into() },
                Connection { from: NodeId(999), from_port: "output".into(), to: second, to_port: "orphan".into() },
                Connection { from: second, from_port: "output".into(), to: first, to_port: "input".into() },
            ],
        );
        assert_eq!(graph.topo_order(), reference_order(&graph));
        assert!(graph.topo_order().is_empty());
    }

    #[test]
    fn connect_refuses_cycles_and_replaces_inputs() {
        let mut g = Graph::new("g");
        let a = g.add_node("uniform", [0.0, 0.0]);
        let b = g.add_node("blur", [100.0, 0.0]);
        let c = g.add_node("blur", [200.0, 0.0]);
        g.connect(a, "output", b, "input").unwrap();
        g.connect(b, "output", c, "input").unwrap();
        assert!(matches!(g.connect(c, "output", a, "input"), Err(CoreError::Cycle)));
        assert!(matches!(g.connect(c, "output", c, "input"), Err(CoreError::Cycle)));
        g.connect(a, "output", c, "input").unwrap();
        assert_eq!(g.connections.len(), 2);
        assert_eq!(g.topo_order().first(), Some(&a));
        assert_eq!(g.upstream(c), vec![a]);
        g.remove_nodes(&[a]);
        assert_eq!(g.connections.len(), 0);
    }
}
