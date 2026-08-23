use async_lsp::lsp_types::{Location, SymbolKind};
use petgraph::graph::DiGraph;

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
