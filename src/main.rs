mod backend;
mod edit;
mod error;
mod mock;
mod proxy;

use backend::Backend;
use error::Error;

/// TODO
struct Content {
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
