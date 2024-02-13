use super::{Capabilities, PassThrough, Proxy};
use crate::{mock, Content};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types as lsp;

pub struct Completion {
    pub proxy: PassThrough,
    pub trigger_characters: Option<Vec<String>>,
}

impl<'a, Proxies> Capabilities for Proxies
where
    Proxies: Iterator<Item = &'a Completion>,
{
    type ServerOptions = lsp::CompletionOptions;
    type ClientCapabilities = lsp::CompletionClientCapabilities;

    fn resolve_provider(self, _: Option<Self::ClientCapabilities>) -> Option<Self::ServerOptions> {
        if let (0, None | Some(0)) = self.size_hint() {
            None
        } else {
            Some(lsp::CompletionOptions {
                trigger_characters: {
                    let result: Vec<_> = self
                        .map_while(|completion| completion.trigger_characters.as_ref())
                        .flat_map(|chars| chars.iter().map(String::from))
                        .collect();
                    (!result.is_empty()).then_some(result)
                },

                completion_item: Some(lsp::CompletionOptionsCompletionItem {
                    label_details_support: Some(true),
                }),

                ..Default::default()
            })
        }
    }
}

impl Proxy for Completion {
    type Params = lsp::CompletionParams;
    type Response = lsp::CompletionResponse;

    async fn proxy_response(
        &self,
        params: Self::Params,
        content: &Content,
    ) -> Result<Option<Self::Response>> {
        mock::rescript::completion(&self.proxy, params, content).await
    }
}
