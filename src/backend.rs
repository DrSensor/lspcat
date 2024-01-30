use crate::Config;
use tower_lsp::{jsonrpc, lsp_types as lsp, LanguageServer};

/// TODO
pub struct Backend {
    config: Config,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        params: lsp::InitializeParams,
    ) -> jsonrpc::Result<lsp::InitializeResult> {
        Ok(lsp::InitializeResult {
            capabilities: lsp::ServerCapabilities {
                text_document_sync: Some(lsp::TextDocumentSyncCapability::Options(
                    lsp::TextDocumentSyncOptions {
                        change: Some(if self.config.incremental_changes {
                            lsp::TextDocumentSyncKind::INCREMENTAL
                        } else {
                            lsp::TextDocumentSyncKind::FULL
                        }),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        todo!()
    }
}
