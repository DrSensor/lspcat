use crate::Config;
use smol::{fs, io, lock::OnceCell};
use std::{env, path::PathBuf};
use tower_lsp::{jsonrpc, lsp_types as lsp, Client, LanguageServer};

pub struct Backend {
    tempdir: OnceCell<PathBuf>,
    client: Client,
    config: Config,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        params: lsp::InitializeParams,
    ) -> jsonrpc::Result<lsp::InitializeResult> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash as _, Hasher as _};

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

    async fn did_open(&self, params: lsp::DidOpenTextDocumentParams) {
        use io::AsyncWriteExt;
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

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        todo!()
    }
}
