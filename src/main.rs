mod backend;
mod config;
mod edit;
mod error;
mod mock;
mod proxy;

use backend::Backend;
use error::Error;

use const_format::formatcp;
use smol::fs::File;
use std::{borrow::Cow, path::PathBuf};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const HELP: &'static str = formatcp!(
    r#"lsp🐈 {VERSION}

Usage: lspcat [CONFIG] ! [FLAGS] [OPTIONS]
       lspcat [CONFIG] ! [FLAGS] [OPTIONS] -- <LSP_SERVER>
       lspcat [CONFIG] ..< {{LANG}}! [FLAGS] [OPTIONS] [ -- <LSP_SERVER> ] ..[ ! [FLAGS] [OPTIONS] ].. >..

Flags:
  --lang <LANGUAGE>       switch LSP_SERVER mode for specific LANGUAGE

  --completion [COMMAND]  run COMMAND when auto-completion triggered
                          or just enable auto-completion for LSP_SERVER

Options:
  -t, --trigger-chars             trigger auto-completion when it hit specific characters.
      --completion-trigger-chars  (example)$ lspcat --completion -t "( . :"

Config:
    --incremental   save every edit change in incremental fashion (experimental)
"#
);

#[derive(Default)]
struct ProxyFlags {
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
    use std::io::{stdin, stdout};
    use tower_lsp::{LspService, Server};

    let mut args = std::env::args_os().collect::<Vec<_>>();
    args.remove(0); // exclude binary name
    if let Some("help" | "--help") = args.get(0).and_then(|arg| arg.to_str()) {
        print!("{HELP}");
        return;
    }

    // let backend = mock::rescript::backend();
    for args in args.split(|arg| arg == "!") {
        let oof = config::parse_args(args);
    }

    let (service, socket) = LspService::new(mock::rescript::backend);

    let stdin = Unblock::new(stdin());
    let stdout = Unblock::new(stdout());

    smol::block_on(async move {
        Server::new(stdin, stdout, socket).serve(service).await;
    })
}
