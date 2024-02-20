use crate::{proxy, Config, ProxyFlags};
use lexopt::{Arg, Error, Parser};
use smol::{lock::RwLock, process::Command};
use std::ffi::OsString;

enum Flag {
    Value(&'static str),
    Toggle,
}

pub fn parse_args<'a>(
    args: impl IntoIterator<Item = &'a OsString>,
) -> Result<(Config, ProxyFlags, Option<OsString>), Error> {
    let ref mut parser = Parser::from_args(args);

    let mut config = Config::default();
    let mut proxy = ProxyFlags::default();
    let mut lang = None;

    while let Some(arg) = parser.next()? {
        use Arg::*;
        match arg {
            Long("lang") => lang = Some(parser.value()?),

            Long("completion") => {
                let exec = Command::new(parser.value()?);
                proxy.completion = Some(proxy::Completion {
                    proxy: proxy::PassThrough::ExecCommand(RwLock::new(exec)),
                    trigger_characters: None,
                });
            }
            Long("completion-trigger-chars" | "trigger-chars") | Short('t') => {
                if let Some(ref mut trigger_chars) = proxy
                    .completion
                    .as_mut()
                    .map(|c| c.trigger_characters.get_or_insert(Vec::new()))
                {
                    for chars in parser.values()?.map_while(|str| str.into_string().ok()) {
                        let chars = &mut chars.split(' ').map(|char| char.to_string()).collect();
                        trigger_chars.append(chars);
                    }
                };
            }

            Long("incremental") => config.incremental_changes = true,

            _ => return Err(arg.unexpected()),
        }
    }

    Ok((config, proxy, lang))
}
