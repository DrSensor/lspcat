use crate::{proxy, Backend, Content};
use tower_lsp::{jsonrpc, lsp_types as lsp, Client};

pub fn backend(client: Client) -> Backend {
    todo!("create backend that proxy `rescript-analysis completion` command")
}

pub async fn completion(
    proxy: proxy::PassThrough,
    params: lsp::CompletionParams,
    content: &Content,
) -> jsonrpc::Result<Option<lsp::CompletionResponse>> {
    todo!("exec `rescript-analysis completion /tmp/<file> <row> <col> /tmp/<file> true` then parse the json output")
}
