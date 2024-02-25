mod backend;
mod cli;
mod edit;
mod error;
mod mock;
mod proxy;

use backend::Backend;
use dashmap::DashMap;
use error::Error;

use const_format::formatcp;
use smol::{fs::File, lock::OnceCell};
use std::{borrow::Cow, path::PathBuf};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const HELP: &'static str = formatcp!(
    r#"lsp🐈 {VERSION}

Usage: lspcat [CONFIG] {{_}}! ..[PROXY [OPTIONS]]
       lspcat [CONFIG] {{_}}! ..[PROXY [OPTIONS]] -- <LSP_SERVER>
       lspcat [CONFIG] ..< {{LANG}}! ..[PROXY [OPTIONS]] [ -- <LSP_SERVER> ] ..[ ! ..[PROXY [OPTIONS]] [ -- <LSP_SERVER> ] ] >

Proxy:
  --lang <LANGUAGE>       switch LSP_SERVER mode for specific LANGUAGE

  --completion [COMMAND]  run COMMAND when auto-completion triggered
                          or enable auto-completion for LSP_SERVER

Options:
  -t, --trigger-chars             trigger auto-completion when it hit specific characters.
      --completion-trigger-chars  (example)$ lspcat --completion -t "( . :"

Config:
    --incremental   save every edit change in incremental fashion (experimental)
"#
);

#[derive(Default)]
struct ProxyFlags {
    lang: Option<&'static str>,
    completion: Option<proxy::Completion>,
    // ...reserved for other proxies...
}

struct Content {
    language_id: Cow<'static, str>,
    path: PathBuf,
    file: File,
    busy: bool,
}

#[derive(Default)]
pub struct Config {
    pub incremental_changes: bool,
}

fn main() {
    use smol::Unblock;
    use std::io;
    use tower_lsp::{LspService, Server};

    let mut args = std::env::args_os().collect::<Vec<_>>();
    args.remove(0); // exclude binary name

    if let Some("help" | "--help" | "-h") = args.get(0).and_then(|arg| arg.to_str()) {
        return print!("{HELP}");
    }
    if args.is_empty() {
        return print!("{HELP}");
    }

    let mut errors = vec![];
    let mut segments = args.split(|arg| {
        arg.to_str()
            .is_some_and(|arg| arg.starts_with('{') && arg.ends_with("}!"))
    });
    // TODO: segmennts = zip(lang, segment)

    let ref config = match segments.next().map(cli::parse_config) {
        Some(Err(err)) => {
            errors.push(err);
            Config::default()
        }
        Some(Ok(res)) => res,
        None => return print!("{HELP}"),
    };

    for proxies in segments.map(|segment| segment.split(|arg| arg == "!").map(cli::parse_proxy)) {
        let mut prev_stdin = None;
        let mut prev_stdout = None;

        // TODO(pipe): pipe each stdio here, or

        for proxy in proxies {
            let proxy = match proxy {
                Ok(flags) => flags,
                Err(err) => {
                    errors.push(err);
                    continue;
                }
            };

            let (service, socket) = LspService::new(|client| Backend {
                client,
                proxy,
                config,
                lang: "",
                tempdir: OnceCell::new(),
                files: DashMap::new(),
            });

            let stdin = io::stdin();
            let stdout = io::stdout();

            smol::block_on(async {
                let stdin = Unblock::new(stdin);
                let stdout = Unblock::new(stdout);

                // TODO(pipe): pipe each stdio here

                Server::new(stdin, stdout, socket).serve(service).await;
            });

            prev_stdin = Some(stdin);
            prev_stdout = Some(stdout);
        }
    }

    for error in errors {
        println!("{error}");
    }
}
