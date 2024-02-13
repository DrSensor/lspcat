mod backend;
mod edit;
mod error;
mod mock;
mod proxy;

use backend::Backend;
use error::Error;

use smol::fs::File;
use std::{borrow::Cow, path::PathBuf};

struct ProxyColletion {
    completion: Option<proxy::Completion>,
    // ...reserved for other proxies...
}

struct Content {
    language_id: Cow<'static, str>,
    path: PathBuf,
    file: File,
    busy: bool,
}

struct Config {
    incremental_changes: bool,
}

fn main() {
    use smol::Unblock;
    use std::io::{stdin, stdout};
    use tower_lsp::{LspService, Server};

    let (service, socket) = LspService::new(mock::rescript::backend);

    let stdin = Unblock::new(stdin());
    let stdout = Unblock::new(stdout());

    smol::block_on(async move {
        Server::new(stdin, stdout, socket).serve(service).await;
    })
}
