use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_lsp::lsp_types::request::{
    CallHierarchyOutgoingCalls, DocumentHighlightRequest, References, Request,
};
use async_lsp::lsp_types::{
    CallHierarchyItem, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CallHierarchyServerCapability, DidOpenTextDocumentParams, DocumentHighlightKind,
    DocumentHighlightParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    Location, OneOf, PartialResultParams, Position, Range, ReferenceContext, ReferenceParams,
    SymbolKind, TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    WorkDoneProgressParams,
};
use async_lsp::{Error as LspError, ErrorCode, LanguageServer, ServerSocket};
use petgraph::graph::NodeIndex;

use crate::graph::{Graph, Node, NodeKind, Relation};
use crate::lsp::Session;

const READY_ATTEMPTS: usize = 20;
const READY_RETRY_DELAY: Duration = Duration::from_millis(50);

#[derive(Clone)]
struct IndexedSymbol {
    node: NodeIndex,
    selection: Location,
}

#[derive(Clone, Copy)]
struct Scope {
    node: NodeIndex,
    range: Range,
}

#[derive(Default)]
struct WorkspaceGraph {
    graph: Graph,
    symbols_by_selection: HashMap<Location, NodeIndex>,
    files: HashMap<Url, NodeIndex>,
    scopes: HashMap<Url, Vec<Scope>>,
    callables: Vec<IndexedSymbol>,
    state_symbols: Vec<IndexedSymbol>,
}

pub async fn analyse(
    root: &Path,
    lsp_server: &str,
    language: &str,
    extension: &str,
) -> Result<Graph> {
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot resolve project root {}", root.display()))?;
    let documents = read_documents(&root, language, extension)?;
    if documents.is_empty() {
        bail!("no .{} files found below {}", extension, root.display());
    }

    let mut session = Session::start(&root, lsp_server).await?;
    let result = extract(&mut session, &documents).await;
    let finished = session.finish().await;

    match result {
        Ok(graph) => {
            finished?;
            Ok(graph)
        }
        Err(error) => {
            let _ = finished;
            Err(error)
        }
    }
}

async fn extract(session: &mut Session, documents: &[TextDocumentItem]) -> Result<Graph> {
    if !supports(&session.capabilities.document_symbol_provider) {
        bail!("language server does not provide document symbols");
    }

    for document in documents {
        session
            .server
            .did_open(DidOpenTextDocumentParams {
                text_document: document.clone(),
            })
            .with_context(|| format!("cannot open {} through LSP", document.uri))?;
    }
    session.wait_until_idle().await?;

    let mut workspace = WorkspaceGraph::default();
    for document in documents {
        workspace.add_file(document);
        let symbols = session
            .server
            .document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: document.uri.clone(),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .with_context(|| format!("cannot read symbols from {}", document.uri))?;
        workspace.add_symbols(&document.uri, symbols)?;
    }

    if supports_call_hierarchy(&session.capabilities.call_hierarchy_provider) {
        workspace.add_calls(&mut session.server).await?;
    }
    if supports(&session.capabilities.references_provider)
        && supports(&session.capabilities.document_highlight_provider)
    {
        workspace.add_state_accesses(&mut session.server).await?;
    }

    Ok(workspace.graph)
}

impl WorkspaceGraph {
    fn add_file(&mut self, document: &TextDocumentItem) {
        let location = Location::new(document.uri.clone(), document_range(&document.text));
        let node = self.graph.add_node(Node {
            name: document
                .uri
                .path_segments()
                .and_then(Iterator::last)
                .unwrap_or(document.uri.path())
                .to_owned(),
            kind: NodeKind::File,
            location,
        });
        self.files.insert(document.uri.clone(), node);
    }

    fn add_symbols(&mut self, uri: &Url, response: Option<DocumentSymbolResponse>) -> Result<()> {
        let Some(response) = response else {
            return Ok(());
        };

        match response {
            DocumentSymbolResponse::Nested(symbols) => {
                self.add_nested_symbols(uri, self.files[uri], false, symbols);
                Ok(())
            }
            DocumentSymbolResponse::Flat(symbols) if symbols.is_empty() => Ok(()),
            DocumentSymbolResponse::Flat(_) => bail!(
                "language server returned flat symbols for {uri}; Plexus requires hierarchical document symbols"
            ),
        }
    }

    fn add_nested_symbols(
        &mut self,
        uri: &Url,
        parent: NodeIndex,
        inside_callable: bool,
        symbols: Vec<DocumentSymbol>,
    ) {
        for symbol in symbols {
            let callable = is_callable(symbol.kind);
            let local = inside_callable && is_state(symbol.kind) && !is_field(symbol.kind);
            let next_parent = if local {
                parent
            } else {
                let location = Location::new(uri.clone(), symbol.range);
                let selection = Location::new(uri.clone(), symbol.selection_range);
                let node = self.intern(
                    selection.clone(),
                    Node {
                        name: symbol.name,
                        kind: symbol.kind.into(),
                        location,
                    },
                );
                self.add_relation(parent, node, Relation::Contains);

                if callable {
                    self.callables.push(IndexedSymbol {
                        node,
                        selection: selection.clone(),
                    });
                    self.scopes.entry(uri.clone()).or_default().push(Scope {
                        node,
                        range: symbol.range,
                    });
                } else if is_field(symbol.kind) || (is_state(symbol.kind) && !inside_callable) {
                    self.state_symbols.push(IndexedSymbol { node, selection });
                }
                node
            };

            if let Some(children) = symbol.children {
                self.add_nested_symbols(uri, next_parent, inside_callable || callable, children);
            }
        }
    }

    async fn add_calls(&mut self, server: &mut ServerSocket) -> Result<()> {
        let mut definitions = self.callables.clone();
        if let Some(readiness_probe) = definitions.iter().position(|definition| {
            let kind = self.graph[definition.node].kind;
            kind == NodeKind::Function || kind == NodeKind::Method
        }) {
            definitions.swap(0, readiness_probe);
        }

        let mut items = Vec::new();
        let mut prepared = HashSet::new();
        for (index, definition) in definitions.into_iter().enumerate() {
            let params = CallHierarchyPrepareParams {
                text_document_position_params: at(&definition.selection),
                work_done_progress_params: WorkDoneProgressParams::default(),
            };
            let callables = prepare_call_hierarchy(server, &params, index == 0)
                .await
                .with_context(|| {
                    format!(
                        "cannot prepare call hierarchy at {}:{}:{}",
                        definition.selection.uri,
                        definition.selection.range.start.line,
                        definition.selection.range.start.character
                    )
                })?
                .unwrap_or_default();

            for item in callables {
                let selection = item_selection(&item);
                if prepared.insert(selection) {
                    let node = self.intern_call_item(&item);
                    items.push((node, item));
                }
            }
        }

        for (caller, item) in items {
            let calls = retry_request::<CallHierarchyOutgoingCalls>(
                server,
                &CallHierarchyOutgoingCallsParams {
                    item,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                },
            )
            .await
            .context("cannot read outgoing calls")?
            .unwrap_or_default();
            for call in calls {
                let callee = self.intern_call_item(&call.to);
                self.add_relation(caller, callee, Relation::Calls);
            }
        }

        Ok(())
    }

    async fn add_state_accesses(&mut self, server: &mut ServerSocket) -> Result<()> {
        for state in self.state_symbols.clone() {
            let references = retry_request::<References>(
                server,
                &ReferenceParams {
                    text_document_position: at(&state.selection),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                    context: ReferenceContext {
                        include_declaration: false,
                    },
                },
            )
            .await
            .with_context(|| {
                format!(
                    "cannot read references to {}:{}:{}",
                    state.selection.uri,
                    state.selection.range.start.line,
                    state.selection.range.start.character
                )
            })?
            .unwrap_or_default();

            let mut references_by_document: HashMap<_, HashSet<_>> = HashMap::new();
            for reference in references {
                if self.files.contains_key(&reference.uri) {
                    references_by_document
                        .entry(reference.uri)
                        .or_default()
                        .insert(reference.range);
                }
            }

            for (uri, references) in references_by_document {
                let position = references
                    .iter()
                    .next()
                    .expect("documents are inserted with at least one reference")
                    .start;
                let highlights = retry_request::<DocumentHighlightRequest>(
                    server,
                    &DocumentHighlightParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri: uri.clone() },
                            position,
                        },
                        work_done_progress_params: WorkDoneProgressParams::default(),
                        partial_result_params: PartialResultParams::default(),
                    },
                )
                .await
                .with_context(|| format!("cannot classify accesses in {uri}"))?
                .unwrap_or_default();

                for highlight in highlights {
                    if !references.contains(&highlight.range) {
                        continue;
                    }
                    let relation = match highlight.kind {
                        Some(kind) if kind == DocumentHighlightKind::READ => Relation::Reads,
                        Some(kind) if kind == DocumentHighlightKind::WRITE => Relation::Writes,
                        _ => continue,
                    };
                    if let Some(source) = self.enclosing_unit(&uri, highlight.range.start) {
                        self.add_relation(source, state.node, relation);
                    }
                }
            }
        }

        Ok(())
    }

    fn intern(&mut self, selection: Location, node: Node) -> NodeIndex {
        if let Some(&node) = self.symbols_by_selection.get(&selection) {
            return node;
        }
        let index = self.graph.add_node(node);
        self.symbols_by_selection.insert(selection, index);
        index
    }

    fn intern_call_item(&mut self, item: &CallHierarchyItem) -> NodeIndex {
        self.intern(
            item_selection(item),
            Node {
                name: item.name.clone(),
                kind: item.kind.into(),
                location: Location::new(item.uri.clone(), item.range),
            },
        )
    }

    fn add_relation(&mut self, source: NodeIndex, target: NodeIndex, relation: Relation) {
        if !self
            .graph
            .edges_connecting(source, target)
            .any(|edge| *edge.weight() == relation)
        {
            self.graph.add_edge(source, target, relation);
        }
    }

    fn enclosing_unit(&self, uri: &Url, position: Position) -> Option<NodeIndex> {
        self.scopes
            .get(uri)
            .into_iter()
            .flatten()
            .filter(|scope| contains(scope.range, position))
            .reduce(|outer, inner| {
                if encloses(outer.range, inner.range) {
                    inner
                } else {
                    outer
                }
            })
            .map(|scope| scope.node)
            .or_else(|| self.files.get(uri).copied())
    }
}

fn read_documents(root: &Path, language: &str, extension: &str) -> Result<Vec<TextDocumentItem>> {
    let mut paths = Vec::new();
    let mut pending = vec![root.to_owned()];

    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("cannot read directory {}", directory.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if !ignored_directory(&path) {
                    pending.push(path);
                }
            } else if file_type.is_file() && path.extension() == Some(OsStr::new(extension)) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("cannot read source file {}", path.display()))?;
            let uri = Url::from_file_path(&path)
                .map_err(|()| anyhow!("cannot convert {} to a file URI", path.display()))?;
            Ok(TextDocumentItem {
                uri,
                language_id: language.into(),
                version: 0,
                text,
            })
        })
        .collect()
}

fn ignored_directory(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(
            ".git"
                | ".hg"
                | ".svn"
                | ".zig-cache"
                | "node_modules"
                | "target"
                | "zig-cache"
                | "zig-out"
        )
    )
}

fn supports<T>(capability: &Option<OneOf<bool, T>>) -> bool {
    matches!(capability, Some(OneOf::Left(true) | OneOf::Right(_)))
}

fn supports_call_hierarchy(capability: &Option<CallHierarchyServerCapability>) -> bool {
    matches!(
        capability,
        Some(
            CallHierarchyServerCapability::Simple(true) | CallHierarchyServerCapability::Options(_)
        )
    )
}

fn at(selection: &Location) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: selection.uri.clone(),
        },
        position: selection.range.start,
    }
}

fn item_selection(item: &CallHierarchyItem) -> Location {
    Location::new(item.uri.clone(), item.selection_range)
}

fn is_callable(kind: SymbolKind) -> bool {
    kind == SymbolKind::FUNCTION || kind == SymbolKind::METHOD || kind == SymbolKind::CONSTRUCTOR
}

fn is_field(kind: SymbolKind) -> bool {
    kind == SymbolKind::FIELD || kind == SymbolKind::PROPERTY
}

fn is_state(kind: SymbolKind) -> bool {
    is_field(kind) || kind == SymbolKind::VARIABLE || kind == SymbolKind::CONSTANT
}

fn contains(range: Range, position: Position) -> bool {
    range.start <= position && position < range.end
}

fn encloses(outer: Range, inner: Range) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn document_range(text: &str) -> Range {
    let line = text.bytes().filter(|&byte| byte == b'\n').count() as u32;
    let character = text
        .rsplit_once('\n')
        .map_or(text, |(_, last_line)| last_line)
        .encode_utf16()
        .count() as u32;
    Range::new(Position::new(0, 0), Position::new(line, character))
}

async fn prepare_call_hierarchy(
    server: &mut ServerSocket,
    params: &CallHierarchyPrepareParams,
    wait_for_item: bool,
) -> async_lsp::Result<Option<Vec<CallHierarchyItem>>> {
    for attempt in 0..READY_ATTEMPTS {
        match server.prepare_call_hierarchy(params.clone()).await {
            Ok(items)
                if wait_for_item
                    && items.as_ref().is_none_or(Vec::is_empty)
                    && attempt + 1 < READY_ATTEMPTS =>
            {
                tokio::time::sleep(READY_RETRY_DELAY).await;
            }
            Err(LspError::Response(error))
                if error.code == ErrorCode::CONTENT_MODIFIED && attempt + 1 < READY_ATTEMPTS =>
            {
                tokio::time::sleep(READY_RETRY_DELAY).await;
            }
            result => return result,
        }
    }
    unreachable!()
}

async fn retry_request<R>(server: &ServerSocket, params: &R::Params) -> async_lsp::Result<R::Result>
where
    R: Request,
    R::Params: Clone,
{
    for attempt in 0..READY_ATTEMPTS {
        match server.request::<R>(params.clone()).await {
            Err(LspError::Response(error))
                if error.code == ErrorCode::CONTENT_MODIFIED && attempt + 1 < READY_ATTEMPTS =>
            {
                tokio::time::sleep(READY_RETRY_DELAY).await;
            }
            result => return result,
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_empty_flat_symbol_response() {
        let mut workspace = WorkspaceGraph::default();
        let uri = Url::parse("file:///empty.zig").unwrap();

        workspace
            .add_symbols(&uri, Some(DocumentSymbolResponse::Flat(Vec::new())))
            .unwrap();
    }
}
