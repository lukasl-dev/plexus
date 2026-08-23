mod compact;
mod json;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use async_lsp::lsp_types::{Range, Url};
use petgraph::visit::EdgeRef;
use serde::Serialize;

use crate::graph::{Graph, Node, NodeKind, Relation};

pub use compact::write_compact;
pub use json::write_json;

pub(super) const SCHEMA: &str = "plexus/graph@1";

pub(super) struct Snapshot<'a> {
    pub files: Vec<Url>,
    pub nodes: Vec<ExportNode<'a>>,
    pub edges: Vec<ExportEdge>,
}

pub(super) struct ExportNode<'a> {
    pub id: usize,
    pub kind: NodeKind,
    pub name: &'a str,
    pub file: usize,
    pub range: Range,
}

#[derive(Clone, Copy, Serialize)]
pub(super) struct ExportEdge {
    pub source: usize,
    pub relation: Relation,
    pub target: usize,
}

impl<'a> Snapshot<'a> {
    pub fn new(graph: &'a Graph) -> Result<Self> {
        let mut indices: Vec<_> = graph.node_indices().collect();
        indices.sort_by(|&left, &right| compare_nodes(&graph[left], &graph[right]));
        if let Some(nodes) = indices
            .windows(2)
            .find(|nodes| compare_nodes(&graph[nodes[0]], &graph[nodes[1]]).is_eq())
        {
            let node = &graph[nodes[0]];
            bail!(
                "nodes with name {:?}, kind {}, and location {} {:?} have no stable distinguishing identity",
                node.name,
                node.kind,
                node.location.uri,
                node.location.range
            );
        }
        let ids: HashMap<_, _> = indices
            .iter()
            .enumerate()
            .map(|(id, &index)| (index, id))
            .collect();

        let mut files: Vec<_> = indices
            .iter()
            .map(|&index| graph[index].location.uri.clone())
            .collect();
        files.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        files.dedup();

        let nodes: Vec<_> = indices
            .iter()
            .enumerate()
            .map(|(id, &index)| {
                let node = &graph[index];
                ExportNode {
                    id,
                    kind: node.kind,
                    name: &node.name,
                    file: file_id(&files, &node.location.uri),
                    range: node.location.range,
                }
            })
            .collect();
        let mut edges: Vec<_> = graph
            .edge_references()
            .map(|edge| ExportEdge {
                source: ids[&edge.source()],
                relation: *edge.weight(),
                target: ids[&edge.target()],
            })
            .collect();
        edges.sort_by_key(|edge| (edge.source, edge.relation, edge.target));
        validate_containment(&nodes, &edges)?;

        Ok(Self {
            files,
            nodes,
            edges,
        })
    }
}

fn validate_containment(nodes: &[ExportNode<'_>], edges: &[ExportEdge]) -> Result<()> {
    let mut file_nodes = HashMap::new();
    for node in nodes {
        if node.kind == NodeKind::File
            && let Some(previous) = file_nodes.insert(node.file, node.id)
        {
            bail!(
                "file {} has multiple file nodes: {} and {}",
                node.file,
                previous,
                node.id
            );
        }
    }

    let mut parents = HashMap::new();
    for edge in edges {
        if edge.relation != Relation::Contains {
            continue;
        }
        if nodes[edge.source].file != nodes[edge.target].file {
            bail!(
                "containment relationship {} -> {} crosses files",
                edge.source,
                edge.target
            );
        }
        if nodes[edge.target].kind == NodeKind::File {
            bail!("file node {} has a containment parent", edge.target);
        }
        if let Some(parent) = parents.insert(edge.target, edge.source)
            && parent != edge.source
        {
            bail!(
                "node {} has multiple containment parents: {} and {}",
                edge.target,
                parent,
                edge.source
            );
        }
    }

    for &child in parents.keys() {
        let mut ancestors = HashSet::new();
        let mut current = child;
        while let Some(&parent) = parents.get(&current) {
            if !ancestors.insert(current) {
                bail!("containment relationship involving node {child} is cyclic");
            }
            current = parent;
        }
    }

    Ok(())
}

pub fn write_summary(graph: &Graph, mut output: impl Write) -> Result<()> {
    writeln!(
        output,
        "{} nodes, {} relationships",
        graph.node_count(),
        graph.edge_count()
    )?;
    Ok(())
}

pub(super) fn canonical_root(root: &Path) -> Result<PathBuf> {
    root.canonicalize()
        .with_context(|| format!("cannot resolve project root {}", root.display()))
}

pub(super) fn display_uri(uri: &Url, root: &Path) -> String {
    uri.to_file_path()
        .ok()
        .and_then(|path| path.strip_prefix(root).ok().map(Path::to_owned))
        .map_or_else(
            || uri.to_string(),
            |path| path.to_string_lossy().into_owned(),
        )
}

fn file_id(files: &[Url], uri: &Url) -> usize {
    files
        .binary_search_by(|file| file.as_str().cmp(uri.as_str()))
        .expect("snapshot contains every node URI")
}

fn compare_nodes(left: &Node, right: &Node) -> std::cmp::Ordering {
    left.location
        .uri
        .as_str()
        .cmp(right.location.uri.as_str())
        .then_with(|| left.location.range.start.cmp(&right.location.range.start))
        .then_with(|| right.location.range.end.cmp(&left.location.range.end))
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.name.cmp(&right.name))
}

#[cfg(test)]
pub(super) fn test_graph() -> (Graph, PathBuf) {
    test_graph_in_order([0, 1, 2, 3, 4])
}

#[cfg(test)]
pub(super) fn reversed_test_graph() -> (Graph, PathBuf) {
    test_graph_in_order([4, 3, 2, 1, 0])
}

#[cfg(test)]
fn test_graph_in_order(order: [usize; 5]) -> (Graph, PathBuf) {
    use async_lsp::lsp_types::{Location, Position};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).to_owned();
    let uri = Url::from_file_path(root.join("src/example.rs")).unwrap();
    let mut graph = Graph::new();
    let nodes = [
        (NodeKind::Function, "run", 7, 9),
        (NodeKind::Method, "increment", 3, 5),
        (NodeKind::Field, "value", 2, 2),
        (NodeKind::Struct, "Counter", 1, 6),
        (NodeKind::File, "example.rs", 0, 10),
    ];
    let mut indices = HashMap::new();
    for id in order {
        let (kind, name, start, end) = nodes[id];
        let index = graph.add_node(crate::graph::Node {
            name: name.into(),
            kind,
            location: Location::new(
                uri.clone(),
                Range::new(Position::new(start, 0), Position::new(end, 0)),
            ),
        });
        indices.insert(id, index);
    }

    let function = indices[&0];
    let method = indices[&1];
    let field = indices[&2];
    let structure = indices[&3];
    let file = indices[&4];
    graph.add_edge(file, structure, Relation::Contains);
    graph.add_edge(structure, field, Relation::Contains);
    graph.add_edge(structure, method, Relation::Contains);
    graph.add_edge(file, function, Relation::Contains);
    graph.add_edge(function, method, Relation::Calls);
    graph.add_edge(method, field, Relation::Reads);

    (graph, root)
}
