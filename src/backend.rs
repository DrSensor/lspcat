use tower_lsp::{jsonrpc, lsp_types as lsp, LanguageServer};

/// TODO
pub struct Backend {
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        params: lsp::InitializeParams,
    ) -> jsonrpc::Result<lsp::InitializeResult> {
        todo!()
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        todo!()
    }
}
