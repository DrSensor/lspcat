use smol::{lock::RwLock, process::Command};
use tower_lsp::jsonrpc::Result;

pub enum PassThrough {
    ExecCommand(RwLock<Command>), // lspcat exec:"cli-command <row> <col> <file>"
    LangServer(RwLock<Command>),  // lspcat serve:"lsp-server --stdio"
}

/// STEP(2): Tell code editor that langserver has some capabilities based on parsed CLI args
pub trait Capabilities {
    type ServerOptions;
    type ClientCapabilities;
    fn resolve_provider(
        self,
        client: Option<Self::ClientCapabilities>,
    ) -> Option<Self::ServerOptions>;
}

/// STEP(4): Proxy some request
pub trait Proxy {
    type Params;
    type Response;
    async fn proxy_response(
        &self,
        params: Self::Params,
        content: &Content,
    ) -> Result<Option<Self::Response>>;
}

mod completion;
pub use completion::Completion;

use crate::Content;
