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
use std::{borrow::Cow, ffi::OsString, path::PathBuf};

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
    lang: Option<String>,
    completion: Option<proxy::Completion>,
    // ...reserved for other proxies...
}

struct Content {
    language_id: Cow<'static, str>,
    path: PathBuf,
    file: File,
    busy: bool,
}

#[derive(Default, Clone)]
pub struct Config {
    pub incremental_changes: bool,
}

fn main() {
    use smol::Unblock;
    use std::{io, sync::Arc};
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
    let mut segments = args.split_inclusive(|arg| {
        arg.to_str()
            .is_some_and(|arg| arg.starts_with('{') && arg.ends_with("}!"))
    });

    let Some((lang, config)) = segments.next().and_then(|args| args.split_last()) else {
        return print!("{HELP}");
    };

    let config = Arc::new(match cli::parse_config(config) {
        Ok(config) => config,
        Err(err) => {
            errors.push(err);
            Config::default()
        }
    });
    let mut lang = lang
        .to_string_lossy()
        .as_ref()
        .trim_start_matches('{')
        .trim_end_matches("}!");

    for (next_lang, proxies) in segments.map_while(|segment| {
        segment.split_last().map(|(last, segment)| {
            let mut segment = segment.to_vec();
            let mut lang = Some(last);
            if segment.is_empty() {
                segment.push(last.clone());
                lang = None;
            }
            (lang, segment.split(|arg| arg == "!").map(cli::parse_proxy))
        })
    }) {
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
        if let Some(next) = next_lang {
            lang = next.to_string_lossy().as_ref();
        }
    }

    for error in errors {
        println!("{error}");
    }
}
