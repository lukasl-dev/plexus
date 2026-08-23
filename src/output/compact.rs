use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use anyhow::Result;

use super::{ExportNode, Snapshot, canonical_root, display_uri};
use crate::graph::{Graph, NodeKind, Relation};

struct Forest {
    parents: HashMap<usize, usize>,
    children: HashMap<usize, Vec<usize>>,
}

impl Forest {
    fn new(snapshot: &Snapshot<'_>) -> Self {
        let mut parents = HashMap::new();
        for edge in &snapshot.edges {
            if edge.relation == Relation::Contains {
                parents
                    .entry(edge.target)
                    .and_modify(|parent: &mut usize| *parent = (*parent).min(edge.source))
                    .or_insert(edge.source);
            }
        }

        let mut children: HashMap<_, Vec<_>> = HashMap::new();
        for (&child, &parent) in &parents {
            children.entry(parent).or_default().push(child);
        }
        for children in children.values_mut() {
            children.sort_unstable();
        }

        Self { parents, children }
    }
}

pub fn write_compact(graph: &Graph, root: &Path, mut output: impl Write) -> Result<()> {
    let root = canonical_root(root)?;
    let snapshot = Snapshot::new(graph);
    let forest = Forest::new(&snapshot);
    writeln!(output, "plexus/1 positions=utf-16,zero-based")?;
    write_files(&snapshot, &forest, &root, &mut output)?;
    write_relations(&snapshot, &mut output)
}

fn write_files(
    snapshot: &Snapshot<'_>,
    forest: &Forest,
    root: &Path,
    mut output: impl Write,
) -> Result<()> {
    let mut visited = HashSet::new();
    for (file, uri) in snapshot.files.iter().enumerate() {
        let nodes: Vec<_> = snapshot
            .nodes
            .iter()
            .filter(|node| node.file == file)
            .map(|node| node.id)
            .collect();
        let file_node = nodes
            .iter()
            .copied()
            .find(|&id| snapshot.nodes[id].kind == NodeKind::File);

        write!(output, "\n@")?;
        match file_node {
            Some(id) => write!(output, "{id}")?,
            None => write!(output, "-")?,
        }
        write!(output, " ")?;
        write_escaped(&mut output, &display_uri(uri, root))?;
        writeln!(output)?;

        for &id in &nodes {
            let nested = forest
                .parents
                .get(&id)
                .is_some_and(|parent| Some(*parent) != file_node);
            if Some(id) != file_node && !nested {
                write_tree(snapshot, forest, id, 0, &mut visited, &mut output)?;
            }
        }
        for id in nodes {
            if Some(id) != file_node && !visited.contains(&id) {
                write_tree(snapshot, forest, id, 0, &mut visited, &mut output)?;
            }
        }
    }
    Ok(())
}

fn write_tree<W: Write + ?Sized>(
    snapshot: &Snapshot<'_>,
    forest: &Forest,
    id: usize,
    depth: usize,
    visited: &mut HashSet<usize>,
    output: &mut W,
) -> Result<()> {
    if !visited.insert(id) {
        return Ok(());
    }

    for _ in 0..depth {
        write!(output, "  ")?;
    }
    let node = &snapshot.nodes[id];
    write!(output, "{id} ")?;
    write_kind(output, node)?;
    write!(
        output,
        " {}:{} ",
        node.range.start.line, node.range.start.character
    )?;
    write_escaped(output, node.name)?;
    writeln!(output)?;

    if let Some(children) = forest.children.get(&id) {
        for &child in children {
            write_tree(snapshot, forest, child, depth + 1, visited, output)?;
        }
    }
    Ok(())
}

fn write_relations(snapshot: &Snapshot<'_>, mut output: impl Write) -> Result<()> {
    for relation in [Relation::Calls, Relation::Reads, Relation::Writes] {
        let mut adjacency: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for edge in &snapshot.edges {
            if edge.relation == relation {
                adjacency
                    .entry(edge.source)
                    .or_default()
                    .insert(edge.target);
            }
        }
        if adjacency.is_empty() {
            continue;
        }

        writeln!(output, "\n{}", relation.name())?;
        for (source, targets) in adjacency {
            write!(output, "{source}:")?;
            for (index, target) in targets.into_iter().enumerate() {
                if index != 0 {
                    write!(output, ",")?;
                }
                write!(output, "{target}")?;
            }
            writeln!(output)?;
        }
    }
    Ok(())
}

fn write_kind<W: Write + ?Sized>(output: &mut W, node: &ExportNode<'_>) -> Result<()> {
    if node.kind == NodeKind::Function {
        write!(output, "fn")?;
    } else {
        write!(output, "{}", node.kind)?;
    }
    Ok(())
}

fn write_escaped<W: Write + ?Sized>(output: &mut W, value: &str) -> Result<()> {
    for character in value.chars() {
        match character {
            '\\' => write!(output, "\\\\")?,
            '\n' => write!(output, "\\n")?,
            '\r' => write!(output, "\\r")?,
            character => write!(output, "{character}")?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_compact_graph() {
        let (graph, root) = super::super::test_graph();
        let mut output = Vec::new();
        write_compact(&graph, &root, &mut output).unwrap();

        let expected = r#"plexus/1 positions=utf-16,zero-based

@0 src/example.rs
1 struct 1:0 Counter
  2 field 2:0 value
  3 method 3:0 increment
4 fn 7:0 run

calls
4:3

reads
3:2
"#;
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }
}
