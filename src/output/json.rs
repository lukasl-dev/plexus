use std::io::{self, Write};
use std::path::Path;

use anyhow::{Result, anyhow};
use async_lsp::lsp_types::Url;
use serde::Serialize;

use super::{ExportEdge, SCHEMA, Snapshot, canonical_root};
use crate::graph::{Graph, NodeKind};

#[derive(Serialize)]
struct Document<'a> {
    schema: &'static str,
    positions: Positions,
    root: Url,
    files: &'a [Url],
    nodes: Vec<Node<'a>>,
    edges: &'a [ExportEdge],
}

#[derive(Serialize)]
struct Positions {
    encoding: &'static str,
    base: u8,
    end_exclusive: bool,
}

#[derive(Serialize)]
struct Node<'a> {
    id: usize,
    kind: NodeKind,
    name: &'a str,
    file: usize,
    range: [u32; 4],
}

pub fn write_json(graph: &Graph, root: &Path, mut output: impl Write) -> Result<()> {
    let root = canonical_root(root)?;
    let root = Url::from_directory_path(&root)
        .map_err(|()| anyhow!("cannot convert {} to a file URI", root.display()))?;
    let snapshot = Snapshot::new(graph)?;
    let nodes = snapshot
        .nodes
        .iter()
        .map(|node| Node {
            id: node.id,
            kind: node.kind,
            name: node.name,
            file: node.file,
            range: [
                node.range.start.line,
                node.range.start.character,
                node.range.end.line,
                node.range.end.character,
            ],
        })
        .collect();
    let document = Document {
        schema: SCHEMA,
        positions: Positions {
            encoding: "utf-16",
            base: 0,
            end_exclusive: true,
        },
        root,
        files: &snapshot.files,
        nodes,
        edges: &snapshot.edges,
    };

    serde_json::to_writer_pretty(&mut output, &document).map_err(io::Error::from)?;
    writeln!(output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(graph: &Graph, root: &Path) -> Vec<u8> {
        let mut output = Vec::new();
        write_json(graph, root, &mut output).unwrap();
        output
    }

    #[test]
    fn writes_canonical_json() {
        let (graph, root) = super::super::test_graph();
        let output = render(&graph, &root);
        let document: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(document["schema"], SCHEMA);
        assert_eq!(document["positions"]["encoding"], "utf-16");
        assert_eq!(document["nodes"][0]["kind"], "file");
        assert_eq!(document["nodes"][4]["kind"], "function");
        assert!(
            document["edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|edge| edge["relation"] == "calls")
        );
    }

    #[test]
    fn output_does_not_depend_on_graph_insertion_order() {
        let (graph, root) = super::super::test_graph();
        let (reversed, reversed_root) = super::super::reversed_test_graph();

        assert_eq!(render(&graph, &root), render(&reversed, &reversed_root));
    }

    #[test]
    fn preserves_unknown_node_kinds() {
        let (mut graph, root) = super::super::test_graph();
        let node = graph.node_indices().next().unwrap();
        graph[node].kind = NodeKind::Unknown(99);

        let document: serde_json::Value = serde_json::from_slice(&render(&graph, &root)).unwrap();
        assert!(
            document["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["kind"] == 99)
        );
    }

    #[test]
    fn rejects_nodes_without_a_stable_distinguishing_identity() {
        let (mut graph, root) = super::super::test_graph();
        let original = graph.node_indices().next().unwrap();
        graph.add_node(crate::graph::Node {
            name: graph[original].name.clone(),
            kind: graph[original].kind,
            location: graph[original].location.clone(),
        });

        let mut output = Vec::new();
        let error = write_json(&graph, &root, &mut output).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no stable distinguishing identity")
        );
    }
}
