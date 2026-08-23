use std::path::Path;

use petgraph::graph::NodeIndex;
use plexus::graph::{Graph, NodeKind, Relation, mutates};

fn node(graph: &Graph, name: &str, kind: NodeKind) -> NodeIndex {
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

    let counter = node(&graph, "Counter", NodeKind::Struct);
    let field = node(&graph, "value", NodeKind::Field);
    let increment = node(&graph, "increment", NodeKind::Method);
    let value = node(&graph, "value", NodeKind::Method);
    let tick = node(&graph, "tick", NodeKind::Function);

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
