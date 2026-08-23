use std::io::Write;
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
    let snapshot = Snapshot::new(graph);
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

    serde_json::to_writer_pretty(&mut output, &document)?;
    writeln!(output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_canonical_json() {
        let (graph, root) = super::super::test_graph();
        let mut output = Vec::new();
        write_json(&graph, &root, &mut output).unwrap();

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
}
