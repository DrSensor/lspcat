use smol::{lock::RwLock, process::Command};

pub enum PassThrough {
    ExecCommand(RwLock<Command>), // lspcat exec:"cli-command <row> <col> <file>"
    LangServer(RwLock<Command>),  // lspcat serve:"lsp-server --stdio"
}
