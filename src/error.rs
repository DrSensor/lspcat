use serde_json::Value;
use tower_lsp::jsonrpc::{self, ErrorCode};

pub enum Error {
    Forbidden,
    FileNotOpen,
    ParseError,
    NoResponse,
}

impl From<Error> for jsonrpc::Error {
    fn from(value: Error) -> Self {
        value.maybe_data(None)
    }
}

impl Error {
    fn maybe_data(self, data: Option<Value>) -> jsonrpc::Error {
        match self {
            Error::Forbidden => jsonrpc::Error {
                code: ErrorCode::ServerError(-32903),
                message: "Forbidden".into(),
                data,
            },
            Error::FileNotOpen => jsonrpc::Error {
                code: jsonrpc::ErrorCode::ServerError(-32900),
                message: "File not yet open".into(),
                data,
            },
            Error::ParseError => jsonrpc::Error {
                code: ErrorCode::ParseError,
                message: ErrorCode::ParseError.description().into(),
                data,
            },
            Error::NoResponse => jsonrpc::Error {
                code: ErrorCode::ServerError(-32944),
                message: "No response".into(),
                data,
            },
        }
    }
    pub fn data(self, data: Value) -> jsonrpc::Error {
        self.maybe_data(Some(data))
    }
    pub fn msg(self, msg: &str) -> jsonrpc::Error {
        self.data(msg.into())
    }
}
