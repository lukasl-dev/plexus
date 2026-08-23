mod compact;
mod json;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
    pub fn new(graph: &'a Graph) -> Self {
        let mut indices: Vec<_> = graph.node_indices().collect();
        indices.sort_by(|&left, &right| compare_nodes(&graph[left], &graph[right]));
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

        let nodes = indices
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

        Self {
            files,
            nodes,
            edges,
        }
    }
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
    use async_lsp::lsp_types::{Location, Position};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).to_owned();
    let uri = Url::from_file_path(root.join("src/example.rs")).unwrap();
    let mut graph = Graph::new();
    let mut node = |kind: NodeKind, name: &str, start: u32, end: u32| {
        graph.add_node(crate::graph::Node {
            name: name.into(),
            kind,
            location: Location::new(
                uri.clone(),
                Range::new(Position::new(start, 0), Position::new(end, 0)),
            ),
        })
    };

    let function = node(NodeKind::Function, "run", 7, 9);
    let method = node(NodeKind::Method, "increment", 3, 5);
    let field = node(NodeKind::Field, "value", 2, 2);
    let structure = node(NodeKind::Struct, "Counter", 1, 6);
    let file = node(NodeKind::File, "example.rs", 0, 10);
    graph.add_edge(file, structure, Relation::Contains);
    graph.add_edge(structure, field, Relation::Contains);
    graph.add_edge(structure, method, Relation::Contains);
    graph.add_edge(file, function, Relation::Contains);
    graph.add_edge(function, method, Relation::Calls);
    graph.add_edge(method, field, Relation::Reads);

    (graph, root)
}
