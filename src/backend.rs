use crate::{Config, Content, ProxyFlags};
use dashmap::DashMap;
use smol::{fs, io, lock::OnceCell};
use std::{env, path::PathBuf};
use tower_lsp::{jsonrpc, lsp_types as lsp, Client, LanguageServer};

pub struct Backend {
    pub tempdir: OnceCell<PathBuf>,
    pub client: Client,
    pub files: DashMap<lsp::Url, Content>,
    pub lang: &'static str,
    pub proxy: ProxyFlags,
    pub config: &'static Config,
}

impl Backend {
    fn get_proxy(
        &self,
        text_document: &lsp::TextDocumentIdentifier,
    ) -> jsonrpc::Result<(&ProxyFlags, dashmap::mapref::one::Ref<lsp::Url, Content>)> {
        use crate::Error;

        match self.files.get(&text_document.uri) {
            Some(content) => Ok((&self.proxy, content)),
            // Some(content) => match self.proxy.get(content.language_id.as_ref()) {
            //     Some(proxy) => Ok((proxy, content)),
            //     None => Err(Error::Forbidden.msg(&format!(
            //         "Missing proxy for language-id {}",
            //         content.language_id
            //     ))),
            // },
            None => Err(Error::FileNotOpen.msg(text_document.uri.path())),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        params: lsp::InitializeParams,
    ) -> jsonrpc::Result<lsp::InitializeResult> {
        use crate::proxy::Capabilities as _;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash as _, Hasher as _};
        let text_document = params.capabilities.text_document;

        if let Some(pid) = params.process_id {
            let tempdir = {
                let mut hasher = DefaultHasher::new();
                let cwd = env::current_dir().expect("need permission");
                format!("{} {}", pid, cwd.display()).hash(&mut hasher);
                env::temp_dir().join(format!("lspcat-{}", hasher.finish()))
            };
            let _ = fs::create_dir(&tempdir).await;
            self.tempdir.set_blocking(tempdir).expect("must set once"); // WARNING: using async version didn't works
        }

        // let completions = self
        //     .proxies
        //     .values()
        //     .map_while(|proxy| proxy.completion.as_ref());
        let completions = vec![self.proxy.completion]
            .iter()
            .map_while(|maybe| maybe.as_ref());

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
                completion_provider: completions
                    .resolve_provider(text_document.map(|to| to.completion).flatten()),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn did_open(&self, params: lsp::DidOpenTextDocumentParams) {
        use io::AsyncWriteExt as _;
        use std::path::Path;

        let doc = params.text_document;
        let Some(tempdir) = self.tempdir.get() else {
            return;
        };
        let cwd = env::current_dir().expect("need permission");
        let Ok(path) = Path::new(doc.uri.path())
            .strip_prefix(cwd)
            .map(|file| tempdir.join(file))
        else {
            return;
        };
        if let Some(dir) = path.parent() {
            if let Err(err) = fs::create_dir_all(dir).await {
                return self.client.log_message(lsp::MessageType::ERROR, err).await;
            }
        }

        let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .await
        else {
            return;
        };
        let write_result = file.write_all(doc.text.as_bytes()).await;
        let sync_result = file.sync_data().await;
        if let Err(err) = write_result.and(sync_result) {
            self.client.log_message(lsp::MessageType::ERROR, err).await;
        }

        self.client
            .log_message(
                lsp::MessageType::LOG,
                format!("edit {} as {}", doc.uri.path(), path.display()),
            )
            .await;
    }

    async fn did_change(&self, params: lsp::DidChangeTextDocumentParams) {
        use crate::edit::FileExt as _;
        use io::{AsyncSeekExt as _, AsyncWriteExt as _, SeekFrom};

        let Some(mut tmp) = self.files.get_mut(&params.text_document.uri) else {
            return;
        };
        if tmp.busy {
            return;
        }
        tmp.busy = true;
        if let Err(err) = tmp.file.seek(SeekFrom::Start(0)).await {
            return self.client.log_message(lsp::MessageType::ERROR, err).await;
        }

        if self.config.incremental_changes {
            // WARNING: this implementation have more I/O operations
            // for diff in params.content_changes {
            //     let Some(range) = diff.range else {
            //         continue;
            //     };
            //     if let Err(err) = tmp.file.apply_change(range, diff.text).await {
            //         self.client.log_message(MessageType::ERROR, err).await
            //     }
            // }
            // INFO: Less I/O operations
            if let Err(err) = tmp.file.apply_all_changes(params.content_changes).await {
                self.client.log_message(lsp::MessageType::ERROR, err).await;
            }
        } else if let Some(content) = params.content_changes.first() {
            if let Err(err) = tmp.file.write_all(content.text.as_bytes()).await {
                self.client.log_message(lsp::MessageType::ERROR, err).await;
            }
        }

        if let Err(err) = tmp.file.sync_data().await {
            self.client.log_message(lsp::MessageType::ERROR, err).await;
        }
        tmp.busy = false;
    }

    async fn completion(
        &self,
        params: lsp::CompletionParams,
    ) -> jsonrpc::Result<Option<lsp::CompletionResponse>> {
        use crate::{proxy::Proxy as _, Error};

        let (proxy, content) = self.get_proxy(&params.text_document_position.text_document)?;
        match &proxy.completion {
            Some(completion) => completion.proxy_response(params, &content).await,
            None => Err(Error::Forbidden.msg("Missing proxy for code completion")),
        }
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(if let Some(tempdir) = self.tempdir.get() {
            if let Err(err) = fs::remove_dir_all(tempdir).await {
                self.client.log_message(lsp::MessageType::ERROR, err).await;
            }
        })
    }
}
