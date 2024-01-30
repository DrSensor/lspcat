mod edit;
mod error;
mod proxy;

use dashmap::DashMap;
use error::Error;
use smol::lock::{OnceCell, RwLock};
use smol::{fs, io, process::Command};
use std::borrow::Cow;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::env::{current_dir, temp_dir};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tower_lsp::{jsonrpc::Result, lsp_types::*, Client, LanguageServer, LspService, Server};

struct ProxyColletion {
    completion: Option<proxy::Completion>,
}

struct Content {
    language_id: Cow<'static, str>,
    path: PathBuf,
    file: fs::File,
    busy: bool,
}

struct Backend {
    tempdir: OnceCell<PathBuf>,
    client: Client,
    files: DashMap<Url, Content>,
    proxies: HashMap<&'static str, ProxyColletion>, // Map<language-id, Proxy>
}

impl Backend {
    fn get_proxy(
        &self,
        text_document: &TextDocumentIdentifier,
    ) -> Result<(&ProxyColletion, dashmap::mapref::one::Ref<Url, Content>)> {
        match self.files.get(&text_document.uri) {
            Some(content) => match self.proxies.get(content.language_id.as_ref()) {
                Some(proxy) => Ok((proxy, content)),
                None => Err(Error::Forbidden.msg(&format!(
                    "Missing proxy for language-id {}",
                    content.language_id
                ))),
            },
            None => Err(Error::FileNotOpen.msg(text_document.uri.path())),
        }
    }
}

/////////// Implementation ///////////

fn main() {
    use smol::Unblock;
    use std::io::{stdin, stdout};
    // STEP(1): Parse CLI args to HashMap<&'language_id str, Proxy>
    let mut proxies = HashMap::new();
    'example: {
        use proxy::PassThrough::*;
        let mut rescript_analysis = Command::new("rescript-analysis");
        rescript_analysis.arg("completion");
        proxies.insert(
            "rescript",
            ProxyColletion {
                completion: Some(proxy::Completion {
                    proxy: ExecCommand(RwLock::new(rescript_analysis)),
                    trigger_characters: Some(vec![".".to_string(), "(".to_string()]),
                }),
            },
        );
    }
    let (service, socket) = LspService::new(|client| Backend {
        client,
        proxies,
        tempdir: OnceCell::new(),
        files: DashMap::new(),
    });
    let stdin = Unblock::new(stdin());
    let stdout = Unblock::new(stdout());
    smol::block_on(async move {
        Server::new(stdin, stdout, socket).serve(service).await;
    })
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    /// STEP(2): Tell code editor that langserver has those capabilities based on parsed CLI args
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        use proxy::Capabilities;

        if let Some(pid) = params.process_id {
            let tempdir = {
                let mut hasher = DefaultHasher::new();
                let cwd = current_dir().expect("need permission");
                format!("{} {}", pid, cwd.display()).hash(&mut hasher);
                temp_dir().join(format!("lspcat-{}", hasher.finish()))
            };
            let _ = fs::create_dir(&tempdir).await;
            self.tempdir.set_blocking(tempdir).expect("must set once"); // WARNING: using async version didn't works
        }
        let mut completions = Vec::new();
        for proxy in self.proxies.values() {
            if let Some(proxy) = &proxy.completion {
                completions.push(proxy)
            }
        }
        let text_document = params.capabilities.text_document;
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        change: Some(TextDocumentSyncKind::INCREMENTAL), // in STEP(4), send full content rather than diff
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

    /// STEP(3): Code editor tell langserver of the language-id of the current file
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        use io::AsyncWriteExt;

        let doc = params.text_document;
        let Some(tempdir) = self.tempdir.get() else {
            return;
        };
        let cwd = current_dir().expect("need permission");
        let Ok(path) = Path::new(doc.uri.path())
            .strip_prefix(cwd)
            .map(|file| tempdir.join(file))
        else {
            return;
        };
        if let Some(dir) = path.parent() {
            if let Err(err) = fs::create_dir_all(dir).await {
                return self.client.log_message(MessageType::ERROR, err).await;
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
            self.client.log_message(MessageType::ERROR, err).await;
        }

        self.client
            .log_message(
                MessageType::LOG,
                format!("edit {} as {}", doc.uri.path(), path.display()),
            )
            .await;

        let language_id = doc.language_id.into();
        if let Some(mut content) = self.files.get_mut(&doc.uri) {
            content.language_id = language_id;
            content.file = file;
            content.path = path;
        } else {
            self.files.insert(
                doc.uri,
                Content {
                    language_id,
                    file,
                    path,
                    busy: false,
                },
            );
        }
    }

    /// STEP(4): Code editor send full content (not diff) on each stroke to the langserver
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        use edit::FileExt;
        use io::{AsyncSeekExt, SeekFrom};

        let Some(mut tmp) = self.files.get_mut(&params.text_document.uri) else {
            return;
        };
        if tmp.busy {
            return;
        }
        tmp.busy = true;
        if let Err(err) = tmp.file.seek(SeekFrom::Start(0)).await {
            return self.client.log_message(MessageType::ERROR, err).await;
        }

        // WARNING: this implementation have more I/O operations
        // for diff in params.content_changes {
        //     let Some(range) = diff.range else {
        //         continue;
        //     };
        //     if let Err(err) = tmp.file.apply_change(range, diff.text).await {
        //         self.client.log_message(MessageType::ERROR, err).await
        //     }
        // }

        // Less I/O operations
        if let Err(err) = tmp.file.apply_all_changes(params.content_changes).await {
            self.client.log_message(MessageType::ERROR, err).await;
        }

        if let Err(err) = tmp.file.sync_data().await {
            self.client.log_message(MessageType::ERROR, err).await;
        }
        tmp.busy = false;
    }

    /// STEP(5): Proxy code completion
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        use proxy::Proxy;

        let (proxy, content) = self.get_proxy(&params.text_document_position.text_document)?;
        match &proxy.completion {
            Some(completion) => completion.proxy_response(params, &content).await,
            None => Err(Error::Forbidden.msg("Missing proxy for code completion")),
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(if let Some(tempdir) = self.tempdir.get() {
            if let Err(err) = fs::remove_dir_all(tempdir).await {
                self.client.log_message(MessageType::ERROR, err).await;
            }
        })
    }
}
