use std::path::Path;

use async_lsp::lsp_types::SymbolKind;
use petgraph::graph::NodeIndex;
use plexus::graph::{Graph, Relation, mutates};

fn node(graph: &Graph, name: &str, kind: SymbolKind) -> NodeIndex {
    graph
        .node_indices()
        .find(|&node| graph[node].name == name && graph[node].kind == kind)
        .unwrap_or_else(|| panic!("missing {name:?} node"))
}

fn relates(graph: &Graph, source: NodeIndex, target: NodeIndex, relation: Relation) -> bool {
    graph
        .edges_connecting(source, target)
        .any(|edge| *edge.weight() == relation)
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "invokes rust-analyzer"]
async fn extracts_program_relationships() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rust");
    let graph = plexus::analysis::analyse(&root, "rust-analyzer", "rust", "rs")
        .await
        .unwrap();

    let counter = node(&graph, "Counter", SymbolKind::STRUCT);
    let field = node(&graph, "value", SymbolKind::FIELD);
    let increment = node(&graph, "increment", SymbolKind::METHOD);
    let value = node(&graph, "value", SymbolKind::METHOD);
    let tick = node(&graph, "tick", SymbolKind::FUNCTION);

    assert!(relates(&graph, counter, field, Relation::Contains));
    assert!(
        relates(&graph, tick, increment, Relation::Calls),
        "{graph:#?}"
    );
    assert!(relates(&graph, increment, field, Relation::Writes));
    assert!(relates(&graph, value, field, Relation::Reads));
    assert!(mutates(&graph, tick, counter));
    assert!(!mutates(&graph, value, counter));
}
