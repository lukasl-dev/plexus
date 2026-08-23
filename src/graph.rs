use std::collections::HashSet;
use std::fmt;

use async_lsp::lsp_types::{Location, SymbolKind};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Serialize, Serializer};

pub type Graph = DiGraph<Node, Relation>;

#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub location: Location,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NodeKind {
    File,
    Module,
    Namespace,
    Package,
    Class,
    Method,
    Property,
    Field,
    Constructor,
    Enum,
    Interface,
    Function,
    Variable,
    Constant,
    String,
    Number,
    Boolean,
    Array,
    Object,
    Key,
    Null,
    EnumMember,
    Struct,
    Event,
    Operator,
    TypeParameter,
    Unknown(i32),
}

impl NodeKind {
    pub const fn name(self) -> Option<&'static str> {
        match self {
            Self::File => Some("file"),
            Self::Module => Some("module"),
            Self::Namespace => Some("namespace"),
            Self::Package => Some("package"),
            Self::Class => Some("class"),
            Self::Method => Some("method"),
            Self::Property => Some("property"),
            Self::Field => Some("field"),
            Self::Constructor => Some("constructor"),
            Self::Enum => Some("enum"),
            Self::Interface => Some("interface"),
            Self::Function => Some("function"),
            Self::Variable => Some("variable"),
            Self::Constant => Some("constant"),
            Self::String => Some("string"),
            Self::Number => Some("number"),
            Self::Boolean => Some("boolean"),
            Self::Array => Some("array"),
            Self::Object => Some("object"),
            Self::Key => Some("key"),
            Self::Null => Some("null"),
            Self::EnumMember => Some("enum-member"),
            Self::Struct => Some("struct"),
            Self::Event => Some("event"),
            Self::Operator => Some("operator"),
            Self::TypeParameter => Some("type-parameter"),
            Self::Unknown(_) => None,
        }
    }
}

impl From<SymbolKind> for NodeKind {
    fn from(kind: SymbolKind) -> Self {
        match kind {
            SymbolKind::FILE => Self::File,
            SymbolKind::MODULE => Self::Module,
            SymbolKind::NAMESPACE => Self::Namespace,
            SymbolKind::PACKAGE => Self::Package,
            SymbolKind::CLASS => Self::Class,
            SymbolKind::METHOD => Self::Method,
            SymbolKind::PROPERTY => Self::Property,
            SymbolKind::FIELD => Self::Field,
            SymbolKind::CONSTRUCTOR => Self::Constructor,
            SymbolKind::ENUM => Self::Enum,
            SymbolKind::INTERFACE => Self::Interface,
            SymbolKind::FUNCTION => Self::Function,
            SymbolKind::VARIABLE => Self::Variable,
            SymbolKind::CONSTANT => Self::Constant,
            SymbolKind::STRING => Self::String,
            SymbolKind::NUMBER => Self::Number,
            SymbolKind::BOOLEAN => Self::Boolean,
            SymbolKind::ARRAY => Self::Array,
            SymbolKind::OBJECT => Self::Object,
            SymbolKind::KEY => Self::Key,
            SymbolKind::NULL => Self::Null,
            SymbolKind::ENUM_MEMBER => Self::EnumMember,
            SymbolKind::STRUCT => Self::Struct,
            SymbolKind::EVENT => Self::Event,
            SymbolKind::OPERATOR => Self::Operator,
            SymbolKind::TYPE_PARAMETER => Self::TypeParameter,
            kind => {
                let code = serde_json::to_value(kind)
                    .expect("SymbolKind serializes as an integer")
                    .as_i64()
                    .and_then(|code| i32::try_from(code).ok())
                    .expect("SymbolKind contains an i32");
                Self::Unknown(code)
            }
        }
    }
}

impl fmt::Display for NodeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(code) => write!(formatter, "unknown:{code}"),
            kind => formatter.write_str(kind.name().expect("known kinds have names")),
        }
    }
}

impl Serialize for NodeKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Unknown(code) => serializer.serialize_i32(*code),
            kind => serializer.serialize_str(kind.name().expect("known kinds have names")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Relation {
    Contains,
    Calls,
    Reads,
    Writes,
}

impl Relation {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Calls => "calls",
            Self::Reads => "reads",
            Self::Writes => "writes",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unknown_lsp_symbol_kinds() {
        let lsp: SymbolKind = serde_json::from_value(serde_json::json!(99)).unwrap();
        let kind = NodeKind::from(lsp);

        assert_eq!(kind, NodeKind::Unknown(99));
        assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::json!(99));
        assert_eq!(kind.to_string(), "unknown:99");
    }
}
