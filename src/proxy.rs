mod completion;
pub use completion::Completion;

use crate::Content;
use smol::{lock::RwLock, process::Command};
use tower_lsp::jsonrpc;

pub enum PassThrough {
    ExecCommand(RwLock<Command>), // lspcat exec:"cli-command <row> <col> <file>"
    LangServer(RwLock<Command>),  // lspcat serve:"lsp-server --stdio"
}

pub trait Proxy {
    type Params;
    type Response;
    async fn proxy_response(
        &self,
        params: Self::Params,
        content: &Content,
    ) -> jsonrpc::Result<Option<Self::Response>>;
}

pub trait Capabilities {
    type ServerOptions;
    type ClientCapabilities;
    fn resolve_provider(
        self,
        client: Option<Self::ClientCapabilities>,
    ) -> Option<Self::ServerOptions>;
}
