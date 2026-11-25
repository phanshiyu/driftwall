//! `driftwall render`: a Mermaid picture of a contract, see "Contract picture" in
//! `docs/DESIGN.md`. A picture is generated from the contract alone and carries no directives.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::model::{
    BoundaryEdge, Contract, Edge, EdgeKind, Endpoint, LayerOrder, Narrowing, NodeId, OpenAccess,
    PictureView,
};

/// What can go wrong while drawing a picture or syncing the blocks of a Markdown file.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PictureError {
    /// `--depth 0`: the top-level boundaries are depth 1.
    #[error("depth must be at least 1: top-level boundaries are depth 1")]
    ZeroDepth,
    /// `--focus` names an id the contract does not declare as a boundary with nodes.
    #[error("unknown boundary `{0}`")]
    UnknownBoundary(String),
    /// `--focus` names a node.
    #[error("`{0}` is a node, not a boundary")]
    NotABoundary(String),
    /// A marker line has arguments other than `--depth N` or `--focus BOUNDARY`.
    #[error("line {line}: unknown arguments in the marker: `{args}`")]
    BadMarker {
        /// Line of the marker.
        line: u32,
        /// The text after `driftwall render`.
        args: String,
    },
    /// A start marker with no end marker after it.
    #[error("line {line}: `<!-- driftwall render -->` has no `<!-- driftwall render end -->`")]
    Unclosed {
        /// Line of the start marker.
        line: u32,
    },
    /// An end marker with no start marker before it.
    #[error("line {line}: `<!-- driftwall render end -->` has no start marker")]
    StrayEnd {
        /// Line of the end marker.
        line: u32,
    },
    /// A start marker inside a block.
    #[error("line {line}: a start marker inside the block that starts on line {first}")]
    Nested {
        /// Line of the second start marker.
        line: u32,
        /// Line of the open block.
        first: u32,
    },
    /// A Markdown file with no block at all.
    #[error("no `<!-- driftwall render -->` block")]
    NoBlock,
}

/// What a picture node or box stands for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Drawn {
    /// A node, drawn as itself.
    Node(String),
    /// A boundary drawn as a subgraph, an edge end only.
    Boundary(String),
    /// A boundary drawn as one box with its contents hidden.
    Box(String),
}

impl Drawn {
    fn id(&self) -> &str {
        match self {
            Self::Node(id) | Self::Boundary(id) | Self::Box(id) => id,
        }
    }
}

/// The merged arrows between two drawn ends.
struct Merged {
    solid: bool,
    /// `Some(label)` while every merged edge has the same label, `None` once they differ.
    label: Option<&'static str>,
}

/// The pieces of a contract a picture needs, with the boundary tree read from the node chains.
struct Scene<'a> {
    contract: &'a Contract,
    /// Every boundary with a node at some depth, with its chain: outermost first, itself last.
    chains: BTreeMap<String, Vec<String>>,
}

impl<'a> Scene<'a> {
    fn new(contract: &'a Contract) -> Self {
        let mut chains: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for info in contract.nodes.values() {
            let chain: Vec<&str> = info.boundaries().collect();
            for (index, boundary) in chain.iter().enumerate() {
                chains.entry((*boundary).to_owned()).or_insert_with(|| {
                    chain
                        .iter()
                        .take(index + 1)
                        .map(|id| (*id).to_owned())
                        .collect()
                });
            }
        }
        Self { contract, chains }
    }

    /// The boundaries around an end, outermost first, the end itself last when it is a boundary.
    fn chain(&self, end: &Endpoint) -> Vec<String> {
        match end {
            Endpoint::Node(id) => self
                .contract
                .nodes
                .get(id)
                .map(|info| info.boundaries().map(str::to_owned).collect())
                .unwrap_or_default(),
            Endpoint::Boundary(id) => self.chains.get(id).cloned().unwrap_or_default(),
        }
    }

    fn label(&self, boundary: &str) -> String {
        sanitize(
            self.contract
                .boundary_labels
                .get(boundary)
                .map_or(boundary, String::as_str),
        )
    }

    /// The end that stands for `end` in the picture of `view`.
    fn map(&self, end: &Endpoint, view: &PictureView) -> Drawn {
        let chain = self.chain(end);
        match view {
            PictureView::Full => plain(end),
            PictureView::Depth(depth) => {
                let depth = usize::try_from(*depth).unwrap_or(usize::MAX);
                let hidden_in = chain.get(depth.saturating_sub(1));
                hidden_in.map_or_else(|| plain(end), |box_id| Drawn::Box(box_id.clone()))
            }
            PictureView::Focus(focus) => self.focus_end(end, focus),
        }
    }

    /// The end that stands for `end` in the picture of `--focus focus`: itself inside the focus,
    /// the focus for a boundary that holds it, else the outermost boundary around it that does not
    /// hold the focus, as a box, or the node itself when there is none.
    fn focus_end(&self, end: &Endpoint, focus: &str) -> Drawn {
        let empty = Vec::new();
        let focus_chain = self.chains.get(focus).unwrap_or(&empty);
        let chain = self.chain(end);
        if chain.iter().any(|id| id == focus) {
            return plain(end);
        }
        if matches!(end, Endpoint::Boundary(id) if focus_chain.contains(id)) {
            return Drawn::Boundary(focus.to_owned());
        }
        chain
            .iter()
            .find(|id| !focus_chain.contains(id))
            .map_or_else(|| plain(end), |outer| Drawn::Box(outer.clone()))
    }

    /// Whether `end` is the focus or inside it.
    fn inside(&self, end: &Endpoint, focus: &str) -> bool {
        self.chain(end).iter().any(|id| id == focus)
    }
}

fn plain(end: &Endpoint) -> Drawn {
    match end {
        Endpoint::Node(id) => Drawn::Node(id.to_string()),
        Endpoint::Boundary(id) => Drawn::Boundary(id.clone()),
    }
}

/// Whether the arrow is dotted and the label it carries.
fn parts(kind: EdgeKind) -> (bool, &'static str) {
    let arrow = kind.arrow();
    let label = arrow
        .split_once('|')
        .and_then(|(_, rest)| rest.strip_suffix('|'))
        .unwrap_or("");
    (arrow.starts_with("-."), label)
}

fn sanitize(label: &str) -> String {
    label
        .chars()
        .map(|c| match c {
            '"' => '\'',
            '\n' | '\r' => ' ',
            other => other,
        })
        .collect()
}

/// The boundaries and nodes of one level of the picture.
#[derive(Default)]
struct Level {
    subs: BTreeMap<String, Self>,
    boxes: BTreeSet<String>,
    nodes: BTreeSet<String>,
}

impl Level {
    fn at(&mut self, path: &[String]) -> &mut Self {
        match path.split_first() {
            None => self,
            Some((first, rest)) => self.subs.entry(first.clone()).or_default().at(rest),
        }
    }

    fn write(&self, scene: &Scene, depth: usize, lines: &mut Vec<String>) {
        let pad = "  ".repeat(depth);
        for (id, inner) in &self.subs {
            lines.push(format!("{pad}subgraph {id}[\"{}\"]", scene.label(id)));
            inner.write(scene, depth + 1, lines);
            lines.push(format!("{pad}end"));
        }
        for id in &self.boxes {
            lines.push(format!("{pad}{id}[\"{}\"]", scene.label(id)));
        }
        for id in &self.nodes {
            let label = NodeId::new(id)
                .ok()
                .and_then(|node| scene.contract.nodes.get(&node))
                .and_then(|info| info.label.as_deref());
            lines.push(label.map_or_else(
                || format!("{pad}{id}"),
                |label| format!("{pad}{id}[\"{}\"]", sanitize(label)),
            ));
        }
    }
}

/// The Mermaid `flowchart` picture of `contract` for `view`, generated from `source` (the contract
/// file name, used in the first comment line).
///
/// # Errors
/// [`PictureError`] for depth 0 and for a focus that is not a boundary of the contract.
pub fn picture(
    contract: &Contract,
    view: &PictureView,
    source: &str,
) -> Result<String, PictureError> {
    let scene = Scene::new(contract);
    let args = match view {
        PictureView::Full => String::new(),
        PictureView::Depth(0) => return Err(PictureError::ZeroDepth),
        PictureView::Depth(depth) => format!(" --depth {depth}"),
        PictureView::Focus(boundary) => {
            if !scene.chains.contains_key(boundary) {
                return Err(if contract.nodes.keys().any(|id| id.as_str() == boundary) {
                    PictureError::NotABoundary(boundary.clone())
                } else {
                    PictureError::UnknownBoundary(boundary.clone())
                });
            }
            format!(" --focus {boundary}")
        }
    };
    let mut lines = vec![
        format!("%% generated by driftwall render{args} from {source}; do not edit"),
        "flowchart LR".to_owned(),
    ];
    let (arrows, extras) = arrows(&scene, view);
    let mut root = Level::default();
    structure(&scene, view, &mut root);
    for drawn in &extras {
        match drawn {
            Drawn::Box(id) => {
                root.boxes.insert(id.clone());
            }
            Drawn::Node(id) => {
                root.nodes.insert(id.clone());
            }
            Drawn::Boundary(_) => {}
        }
    }
    root.write(&scene, 1, &mut lines);
    lines.extend(arrows.into_iter().map(|line| format!("  {line}")));
    let mut text = lines.join("\n");
    text.push('\n');
    Ok(text)
}

/// Fills `root` with the boundaries, boxes and nodes the view draws, apart from what an edge adds.
fn structure(scene: &Scene, view: &PictureView, root: &mut Level) {
    match view {
        PictureView::Full | PictureView::Depth(_) => {
            let limit = match view {
                PictureView::Depth(depth) => usize::try_from(*depth).unwrap_or(usize::MAX),
                _ => usize::MAX,
            };
            for chain in scene.chains.values() {
                match chain.len().cmp(&limit) {
                    std::cmp::Ordering::Less => {
                        root.at(chain);
                    }
                    std::cmp::Ordering::Equal => {
                        let parent = chain.get(..chain.len() - 1).unwrap_or_default();
                        if let Some(id) = chain.last() {
                            root.at(parent).boxes.insert(id.clone());
                        }
                    }
                    std::cmp::Ordering::Greater => {}
                }
            }
            for (id, info) in &scene.contract.nodes {
                let chain: Vec<String> = info.boundaries().map(str::to_owned).collect();
                if chain.len() < limit {
                    root.at(&chain).nodes.insert(id.to_string());
                }
            }
        }
        PictureView::Focus(focus) => {
            let Some(focus_chain) = scene.chains.get(focus) else {
                return;
            };
            let depth = focus_chain.len();
            for chain in scene
                .chains
                .values()
                .filter(|c| c.get(depth - 1) == Some(focus))
            {
                root.at(chain.get(depth - 1..).unwrap_or_default());
            }
            for (id, info) in &scene.contract.nodes {
                let chain: Vec<String> = info.boundaries().map(str::to_owned).collect();
                if chain.get(depth - 1) == Some(focus) {
                    root.at(chain.get(depth - 1..).unwrap_or_default())
                        .nodes
                        .insert(id.to_string());
                }
            }
        }
    }
}

/// The arrow lines of the picture, sorted, and the outside ends a focus view must draw.
fn arrows(scene: &Scene, view: &PictureView) -> (Vec<String>, BTreeSet<Drawn>) {
    let contract = scene.contract;
    let declared = contract
        .edges
        .iter()
        .map(|(edge, kind)| {
            (
                Endpoint::Node(edge.from.clone()),
                Endpoint::Node(edge.to.clone()),
                *kind,
                None,
            )
        })
        .chain(
            contract
                .boundary_edges
                .iter()
                .map(|e| (e.from.clone(), e.to.clone(), e.kind, Some(e))),
        );
    // why: keyed by the ids so the arrows come out sorted by id, whatever the end kinds are.
    let mut merged: BTreeMap<(String, String), (Drawn, Drawn, Merged)> = BTreeMap::new();
    let mut extras = BTreeSet::new();
    for (from, to, kind, written) in declared {
        let ends = match view {
            PictureView::Focus(focus) => focus_ends(scene, focus, (&from, &to), written),
            PictureView::Full | PictureView::Depth(_) => {
                vec![(scene.map(&from, view), scene.map(&to, view))]
            }
        };
        let (dotted, label) = parts(kind);
        for (a, b) in ends {
            if a == b && matches!(a, Drawn::Box(_)) {
                continue;
            }
            merged
                .entry((a.id().to_owned(), b.id().to_owned()))
                .and_modify(|(_, _, m)| {
                    m.solid |= !dotted;
                    if m.label != Some(label) {
                        m.label = None;
                    }
                })
                .or_insert((
                    a,
                    b,
                    Merged {
                        solid: !dotted,
                        label: Some(label),
                    },
                ));
        }
    }
    let mut lines: Vec<String> = Vec::new();
    for (from, to, edge) in merged.values() {
        let arrow = if edge.solid { "-->" } else { "-.->" };
        let label = edge.label.filter(|label| !label.is_empty());
        lines.push(label.map_or_else(
            || format!("{} {arrow} {}", from.id(), to.id()),
            |label| format!("{} {arrow}|{label}| {}", from.id(), to.id()),
        ));
        extras.extend(outside(scene, [from, to], view));
    }
    let (open, open_ends) = open_arrows(scene, view);
    lines.extend(open);
    extras.extend(open_ends);
    (lines, extras)
}

/// The drawn ends of a declared edge under `--focus focus`: one `(from, to)` for every covered pair
/// with exactly one node inside the focus, and for every pair inside it. An end the edge wrote inside the focus is drawn as it
/// is, an end that is the focus or holds it is the focus, and an outside end is drawn as an
/// outside end of the picture, found from the covered node when the edge's own end holds the focus.
fn focus_ends(
    scene: &Scene,
    focus: &str,
    (from, to): (&Endpoint, &Endpoint),
    boundary_edge: Option<&BoundaryEdge>,
) -> Vec<(Drawn, Drawn)> {
    let pairs = match (from, to, boundary_edge) {
        (Endpoint::Node(a), Endpoint::Node(b), None) => vec![Edge {
            from: a.clone(),
            to: b.clone(),
        }],
        (_, _, edge) => edge
            .map(|edge| edge.pairs(&scene.contract.nodes))
            .unwrap_or_default(),
    };
    let holds = |end: &Endpoint| {
        matches!(end, Endpoint::Boundary(id)
            if scene.chains.get(focus).is_some_and(|chain| chain.contains(id)))
    };
    let side = |end: &Endpoint, node: &NodeId| {
        let node = Endpoint::Node(node.clone());
        if holds(end) && !scene.inside(&node, focus) {
            scene.focus_end(&node, focus)
        } else {
            scene.focus_end(end, focus)
        }
    };
    // why: a pair inside the focus is drawn as written, unless an end is an ancestor of the focus.
    let around = |end: &Endpoint| {
        matches!(end, Endpoint::Boundary(id)
            if id != focus && scene.chains.get(focus).is_some_and(|chain| chain.contains(id)))
    };
    pairs
        .iter()
        .filter(|pair| {
            let from_in = scene.inside(&Endpoint::Node(pair.from.clone()), focus);
            let to_in = scene.inside(&Endpoint::Node(pair.to.clone()), focus);
            from_in != to_in || (from_in && !around(from) && !around(to))
        })
        .map(|pair| (side(from, &pair.from), side(to, &pair.to)))
        .collect()
}

/// The ends of an arrow that a focus picture must declare because they lie outside the focus.
fn outside<'a>(
    scene: &'a Scene,
    ends: [&'a Drawn; 2],
    view: &'a PictureView,
) -> impl Iterator<Item = Drawn> + 'a {
    ends.into_iter()
        .filter(move |end| {
            matches!(view, PictureView::Focus(_))
                && matches!(end, Drawn::Node(_) | Drawn::Box(_))
                && !is_inside(scene, end, view)
        })
        .cloned()
}

/// Whether the drawn end is part of the focus subtree, so it needs no extra declaration.
fn is_inside(scene: &Scene, end: &Drawn, view: &PictureView) -> bool {
    let PictureView::Focus(focus) = view else {
        return true;
    };
    match end {
        Drawn::Node(id) => NodeId::new(id)
            .ok()
            .and_then(|node| scene.contract.nodes.get(&node))
            .is_some_and(|info| info.within(focus)),
        Drawn::Boundary(id) | Drawn::Box(id) => scene
            .chains
            .get(id)
            .is_some_and(|chain| chain.contains(focus)),
    }
}

/// The `U ==>|open| L` arrows of every `%% order open:` line, mapped and merged like edges, and
/// the outside ends a focus picture draws for them.
///
/// A pair of layers is drawn when the open allowance still applies to some pair of their nodes,
/// after narrowing and denials, and not when a layer between them is open to both ends. In a focus
/// picture the layer that holds the focus counts as the focus, and only the pairs with the focus at
/// one end are drawn.
fn open_arrows(scene: &Scene, view: &PictureView) -> (Vec<String>, BTreeSet<Drawn>) {
    let mut pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut extras = BTreeSet::new();
    for chain in scene.contract.rules.order.iter().filter(|chain| chain.open) {
        let mut members = layer_members(scene.contract, chain);
        let held = match view {
            PictureView::Focus(focus) => {
                let Some(holder) = focus_layer(scene, chain, focus, &mut members) else {
                    continue;
                };
                Some(holder)
            }
            PictureView::Full | PictureView::Depth(_) => None,
        };
        for (upper, lower) in reduced_open_pairs(scene.contract, &members) {
            if held.is_some_and(|holder| upper != holder && lower != holder) {
                continue;
            }
            let (Some(a), Some(b)) = (
                layer_end(scene, chain, upper, held, view),
                layer_end(scene, chain, lower, held, view),
            ) else {
                continue;
            };
            if a == b && matches!(a, Drawn::Box(_)) {
                continue;
            }
            extras.extend(outside(scene, [&a, &b], view));
            pairs.insert((a.id().to_owned(), b.id().to_owned()));
        }
    }
    let lines = pairs
        .iter()
        .map(|(a, b)| format!("{a} ==>|open| {b}"))
        .collect();
    (lines, extras)
}

/// The place in `chain` of the layer that holds `focus`, with that layer's `members` cut down to the
/// nodes of the focus, or `None` when no layer of the chain holds it.
fn focus_layer<'a>(
    scene: &'a Scene,
    chain: &LayerOrder,
    focus: &str,
    members: &mut [Vec<&'a NodeId>],
) -> Option<usize> {
    let around = scene.chains.get(focus)?;
    let holder = chain
        .boundaries
        .iter()
        .position(|layer| around.contains(layer))?;
    *members.get_mut(holder)? = scene
        .contract
        .nodes
        .iter()
        .filter(|(_, info)| info.within(focus))
        .map(|(id, _)| id)
        .collect();
    Some(holder)
}

/// The drawn end of layer number `index` of `chain`: the focus when it is the layer `held`.
fn layer_end(
    scene: &Scene,
    chain: &LayerOrder,
    index: usize,
    held: Option<usize>,
    view: &PictureView,
) -> Option<Drawn> {
    if let (Some(holder), PictureView::Focus(focus)) = (held, view)
        && holder == index
    {
        return Some(Drawn::Boundary(focus.clone()));
    }
    let layer = chain.boundaries.get(index)?;
    Some(scene.map(&Endpoint::Boundary(layer.clone()), view))
}

/// The nodes of each layer of `chain`, in the order the chain lists the layers.
fn layer_members<'a>(contract: &'a Contract, chain: &LayerOrder) -> Vec<Vec<&'a NodeId>> {
    chain
        .boundaries
        .iter()
        .map(|layer| {
            contract
                .nodes
                .iter()
                .filter(|(_, info)| chain.place(info).is_some_and(|(id, _)| id == layer))
                .map(|(id, _)| id)
                .collect()
        })
        .collect()
}

/// The pairs of layers, by place in the chain, that are open and not implied by a layer between
/// them, for the nodes of each layer in `members`.
fn reduced_open_pairs(contract: &Contract, members: &[Vec<&NodeId>]) -> Vec<(usize, usize)> {
    // why: the allowance is the model's own question, so a picture and a lint cannot disagree.
    let open = |upper: usize, lower: usize| {
        let (Some(from), Some(to)) = (members.get(upper), members.get(lower)) else {
            return false;
        };
        from.iter().any(|a| {
            to.iter().any(|b| {
                let pair = Edge {
                    from: (*a).clone(),
                    to: (*b).clone(),
                };
                contract.denial(&pair).is_none()
                    && matches!(
                        contract.open_access(&pair, Narrowing::Design),
                        Some(OpenAccess::Allowed(_))
                    )
            })
        })
    };
    let mut found = Vec::new();
    for upper in 0..members.len() {
        for lower in upper + 1..members.len() {
            let implied = (upper + 1..lower).any(|mid| open(upper, mid) && open(mid, lower));
            if !implied && open(upper, lower) {
                found.push((upper, lower));
            }
        }
    }
    found
}
