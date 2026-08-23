use std::ffi::OsStr;
use std::ops::ControlFlow;
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, anyhow};
use async_lsp::lsp_types::{
    ClientCapabilities, ClientInfo, DocumentSymbolClientCapabilities, InitializeParams,
    InitializedParams, ServerCapabilities, TextDocumentClientCapabilities, Url, WorkspaceFolder,
};
use async_lsp::router::Router;
use async_lsp::{LanguageServer, MainLoop, ServerSocket};
use async_process::Child;
use tokio::task::JoinHandle;

struct Stop;

pub(crate) struct Session {
    pub(crate) server: ServerSocket,
    pub(crate) capabilities: ServerCapabilities,
    _child: Child,
    main_loop: Option<JoinHandle<async_lsp::Result<()>>>,
}

impl Session {
    pub async fn start(root: &Path, executable: &str) -> Result<Self> {
        let root_uri = Url::from_directory_path(root)
            .map_err(|()| anyhow!("cannot convert {} to a file URI", root.display()))?;
        let (main_loop, server) = MainLoop::new_client(|_| {
            let mut router = Router::new(());
            router
                .event(|_, _: Stop| ControlFlow::Break(Ok(())))
                .unhandled_notification(|_, _| ControlFlow::Continue(()));
            router
        });

        let mut child = async_process::Command::new(executable)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("cannot start language server {executable:?}"))?;
        let stdout = child
            .stdout
            .take()
            .context("language server has no stdout")?;
        let stdin = child.stdin.take().context("language server has no stdin")?;

        let mut session = Self {
            server,
            capabilities: ServerCapabilities::default(),
            _child: child,
            main_loop: Some(tokio::spawn(main_loop.run_buffered(stdout, stdin))),
        };
        session.initialise(root, root_uri).await?;
        Ok(session)
    }

    async fn initialise(&mut self, root: &Path, root_uri: Url) -> Result<()> {
        let initialized = self
            .server
            .initialize(InitializeParams {
                process_id: Some(std::process::id()),
                capabilities: ClientCapabilities {
                    text_document: Some(TextDocumentClientCapabilities {
                        references: Some(Default::default()),
                        document_highlight: Some(Default::default()),
                        document_symbol: Some(DocumentSymbolClientCapabilities {
                            hierarchical_document_symbol_support: Some(true),
                            ..DocumentSymbolClientCapabilities::default()
                        }),
                        call_hierarchy: Some(Default::default()),
                        ..TextDocumentClientCapabilities::default()
                    }),
                    ..ClientCapabilities::default()
                },
                workspace_folders: Some(vec![WorkspaceFolder {
                    uri: root_uri,
                    name: root
                        .file_name()
                        .unwrap_or_else(|| OsStr::new("workspace"))
                        .to_string_lossy()
                        .into_owned(),
                }]),
                client_info: Some(ClientInfo {
                    name: "plexus".into(),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                }),
                ..InitializeParams::default()
            })
            .await
            .context("language server initialization failed")?;
        self.capabilities = initialized.capabilities;
        self.server
            .initialized(InitializedParams {})
            .context("cannot notify language server that initialization completed")
    }

    pub async fn finish(mut self) -> Result<()> {
        self.server
            .shutdown(())
            .await
            .context("LSP shutdown failed")?;
        self.server
            .exit(())
            .context("cannot send LSP exit notification")?;
        self.server.emit(Stop).context("cannot stop LSP client")?;
        self.main_loop
            .take()
            .expect("session always owns its main loop")
            .await
            .context("LSP client task failed")?
            .context("LSP client failed")?;

        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(main_loop) = self.main_loop.take() {
            main_loop.abort();
        }
    }
}
