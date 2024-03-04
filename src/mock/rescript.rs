use crate::{proxy, Backend, Config, Content, Error, ProxyFlags};
use dashmap::DashMap;
use smol::lock::{OnceCell, RwLock};
use smol::process::Command;
use tower_lsp::{jsonrpc, lsp_types as lsp, Client};

pub fn backend(client: Client) -> Backend<'static> {
    let mut rescript_analysis = Command::new("rescript-editor-analysis.exe");
    rescript_analysis.arg("completion");
    let proxy = ProxyFlags {
        lang: None,
        completion: Some(proxy::Completion {
            proxy: proxy::PassThrough::ExecCommand(RwLock::new(rescript_analysis)),
            trigger_characters: Some(vec![".".to_string(), "(".to_string()]),
        }),
    };
    Backend {
        client,
        proxy,
        lang: "rescript",
        config: &Config {
            incremental_changes: true,
        },
        tempdir: OnceCell::new(),
        files: DashMap::new(),
    }
}

pub async fn completion(
    proxy: &proxy::PassThrough,
    params: lsp::CompletionParams,
    content: &Content,
) -> jsonrpc::Result<Option<lsp::CompletionResponse>> {
    let position = params.text_document_position.position;
    match proxy {
        proxy::PassThrough::ExecCommand(cmd) => {
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
                    match serde_json::from_slice::<lsp::CompletionResponse>(&result.stdout) {
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
        proxy::PassThrough::LangServer(_) => unimplemented!("serve:lsp-server"),
    }
}
