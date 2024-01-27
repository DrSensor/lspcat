use crate::error::Error;
use crate::Content;

use super::{Capabilities, PassThrough, Proxy};
use smol::process::Command;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionClientCapabilities, CompletionOptions, CompletionOptionsCompletionItem,
    CompletionParams, CompletionResponse,
};

pub struct Completion {
    pub proxy: PassThrough,
    pub trigger_characters: Option<Vec<String>>,
}

impl Capabilities for Vec<&Completion> {
    type ServerOptions = CompletionOptions;
    type ClientCapabilities = CompletionClientCapabilities;

    /// STEP(2): Tell code editor that langserver has completion capability (only if specified by CLI args)
    fn resolve_provider(self, _: Option<Self::ClientCapabilities>) -> Option<Self::ServerOptions> {
        if self.is_empty() {
            None
        } else {
            let mut trigger_characters = Vec::new();
            for completion in self {
                if let Some(chars) = &completion.trigger_characters {
                    trigger_characters.append(&mut chars.clone());
                }
            }
            Some(CompletionOptions {
                trigger_characters: (!trigger_characters.is_empty()).then_some(trigger_characters),
                completion_item: Some(CompletionOptionsCompletionItem {
                    label_details_support: Some(true),
                }),
                ..Default::default()
            })
        }
    }
}

impl Proxy for Completion {
    type Params = CompletionParams;
    type Response = CompletionResponse;

    /// STEP(5): Proxy code completion
    async fn proxy_response(
        &self,
        params: Self::Params,
        content: &Content,
    ) -> Result<Option<Self::Response>> {
        let position = params.text_document_position.position;
        match &self.proxy {
            PassThrough::ExecCommand(cmd) => {
                let mut cmd = cmd.write().await;
                match cmd
                    .arg(content.path.to_string_lossy().to_string()) // TODO: create temporary file then use it
                    .arg(position.line.to_string())
                    .arg(position.character.to_string())
                    .arg(content.path.to_string_lossy().to_string())
                    .arg("true")
                    .output()
                    .await
                {
                    Ok(result) => {
                        match serde_json::from_slice::<CompletionResponse>(&result.stdout) {
                            Ok(response) => {
                                *cmd = Command::new("rescript-analysis");
                                cmd.arg("completion");
                                Ok(Some(response))
                            }
                            Err(err) => Err(Error::ParseError.msg(&err.to_string())),
                        }
                    }
                    Err(err) => Err(Error::NoResponse.msg(&err.to_string())),
                }
            }
            PassThrough::LangServer(_) => unimplemented!("serve:lsp-server"),
        }
    }
}
