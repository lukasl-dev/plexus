use std::collections::HashSet;

use async_lsp::lsp_types::{Location, SymbolKind};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

pub type Graph = DiGraph<Node, Relation>;

#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Relation {
    Contains,
    Calls,
    Reads,
    Writes,
}

pub fn mutates(graph: &Graph, function: NodeIndex, r#type: NodeIndex) -> bool {
    let mut pending = vec![function];
    let mut visited = HashSet::new();

    while let Some(function) = pending.pop() {
        if !visited.insert(function) {
            continue;
        }

        for edge in graph.edges_directed(function, Direction::Outgoing) {
            match edge.weight() {
                Relation::Calls => pending.push(edge.target()),
                Relation::Writes
                    if graph
                        .edges_connecting(r#type, edge.target())
                        .any(|edge| *edge.weight() == Relation::Contains) =>
                {
                    return true;
                }
                _ => {}
            }
        }
    }

    false
}
