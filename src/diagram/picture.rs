//! `driftwall render`: a Mermaid picture of a contract, see "Contract picture" in
//! `docs/DESIGN.md`. A picture is generated from the contract alone and carries no directives.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::model::{Contract, EdgeKind, Endpoint, NodeId, PictureView};

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

    /// The end that stands for `end` in the picture of `view`, or `None` when it is not drawn.
    fn map(&self, end: &Endpoint, view: &PictureView) -> Option<Drawn> {
        let chain = self.chain(end);
        match view {
            PictureView::Full => Some(plain(end)),
            PictureView::Depth(depth) => {
                let depth = usize::try_from(*depth).unwrap_or(usize::MAX);
                let hidden_in = chain.get(depth.saturating_sub(1));
                Some(hidden_in.map_or_else(|| plain(end), |box_id| Drawn::Box(box_id.clone())))
            }
            PictureView::Focus(focus) => {
                let focus_chain = self.chains.get(focus)?;
                if chain.contains(focus) {
                    return Some(plain(end));
                }
                // why: a boundary around the focus is not drawn, and an edge to it says nothing.
                if matches!(end, Endpoint::Boundary(id) if focus_chain.contains(id)) {
                    return None;
                }
                Some(
                    chain
                        .iter()
                        .find(|id| !focus_chain.contains(id))
                        .map_or_else(|| plain(end), |outer| Drawn::Box(outer.clone())),
                )
            }
        }
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
            )
        })
        .chain(
            contract
                .boundary_edges
                .iter()
                .map(|e| (e.from.clone(), e.to.clone(), e.kind)),
        );
    // why: keyed by the ids so the arrows come out sorted by id, whatever the end kinds are.
    let mut merged: BTreeMap<(String, String), (Drawn, Drawn, Merged)> = BTreeMap::new();
    let mut extras = BTreeSet::new();
    for (from, to, kind) in declared {
        if let PictureView::Focus(focus) = view
            && scene.inside(&from, focus) == scene.inside(&to, focus)
            && !scene.inside(&from, focus)
        {
            continue;
        }
        let (Some(a), Some(b)) = (scene.map(&from, view), scene.map(&to, view)) else {
            continue;
        };
        if a == b && matches!(a, Drawn::Box(_)) {
            continue;
        }
        let (dotted, label) = parts(kind);
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
    let mut lines: Vec<String> = Vec::new();
    for (from, to, edge) in merged.values() {
        let arrow = if edge.solid { "-->" } else { "-.->" };
        let label = edge.label.filter(|label| !label.is_empty());
        lines.push(label.map_or_else(
            || format!("{} {arrow} {}", from.id(), to.id()),
            |label| format!("{} {arrow}|{label}| {}", from.id(), to.id()),
        ));
        if matches!(view, PictureView::Focus(_)) {
            for end in [from, to] {
                if matches!(end, Drawn::Node(_) | Drawn::Box(_)) && !is_inside(scene, end, view) {
                    extras.insert(end.clone());
                }
            }
        }
    }
    lines.extend(open_arrows(scene, view));
    (lines, extras)
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

/// The `U ==>|open| L` arrows of every `%% order open:` line, mapped and merged like edges.
fn open_arrows(scene: &Scene, view: &PictureView) -> Vec<String> {
    if matches!(view, PictureView::Focus(_)) {
        return Vec::new();
    }
    let mut pairs: BTreeSet<(Drawn, Drawn)> = BTreeSet::new();
    for chain in scene.contract.rules.order.iter().filter(|chain| chain.open) {
        for pair in chain.boundaries.windows(2) {
            let (Some(upper), Some(lower)) = (pair.first(), pair.last()) else {
                continue;
            };
            let (Some(a), Some(b)) = (
                scene.map(&Endpoint::Boundary(upper.clone()), view),
                scene.map(&Endpoint::Boundary(lower.clone()), view),
            ) else {
                continue;
            };
            if a == b && matches!(a, Drawn::Box(_)) {
                continue;
            }
            pairs.insert((a, b));
        }
    }
    pairs
        .iter()
        .map(|(a, b)| format!("{} ==>|open| {}", a.id(), b.id()))
        .collect()
}
