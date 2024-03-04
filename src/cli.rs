use crate::{proxy, Config, ProxyFlags};
use fauxgen::generator;
use lexopt::{Arg, Error as ArgError, Parser};
use smol::{lock::RwLock, process::Command};
use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    rc::{self, Rc},
};

pub enum Error {
    Parse(Vec<ArgError>),
    MissingLangID,
}
// enum IterPipeline<'a, I: Iterator<Item = Result<ProxyFlags, ArgError>>> {
//     Next(&'a str, I),
//     Last(&'a OsStr),
// }
// enum Syn<'a> {
//     Full(),
//     Bin(&'a str),
// }

// #[generator(yield = (&str, ProxyFlags))]
// pub fn parse(args: &[OsString]) -> Result<(), Error> {
//     use std::iter;

//     let mut errors = vec![];

//     let mut segments = parse::segments(args);

//     let Some((lang, config)) = segments.next().and_then(|args| args.split_last()) else {
//         return Err(Error::MissingLangID);
//     };

//     let config = match parse::config(config) {
//         Ok(config) => config,
//         Err(err) => {
//             errors.push(err);
//             Config::default()
//         }
//     };

//     for segment in segments {}

//     Ok(())
// }

pub fn parse(
    args: &[OsString],
) -> Result<
    Pipeline<impl Iterator<Item = &[OsString]>, impl Iterator<Item = Option<ProxyFlags>>>,
    Error,
> {
    use std::iter;

    let mut errors = vec![];

    let mut segments = parse::segments(args);

    let Some((lang, config)) = segments.next().and_then(|args| args.split_last()) else {
        return Err(Error::MissingLangID);
    };

    let lang: Rc<str> = lang
        .to_string_lossy()
        .trim_start_matches('{')
        .trim_end_matches("}!")
        .into();
    if lang.is_empty() {
        return Err(Error::MissingLangID);
    }

    let config = match parse::config(config) {
        Ok(config) => config,
        Err(err) => {
            errors.push(err);
            Config::default()
        }
    };

    /*
    let iter = segments.map_while(|segment| {
        segment.split_last().and_then(|(last, segment)| {
            segment
                .is_empty()
                .then_some((Some(lang), parse::pipeline(&segment, "!"))) // there is no single scenario where 1 flag/arg suffice to build lsp proxy??
                                                                         // wait, maybe there is, example
                                                                         // lspcat {_}! lsp-server

            // let mut lang = Some(last);
            // let mut last_item = None;
            // if segment.is_empty() {
            //     lang = None;
            //     last_item = Some(last);
            // }
            // (lang, parse::pipeline(&segment, "!"), last_item)

            // let mut segment = segment.to_vec();
            // let mut lang = Some(last);
            // if segment.is_empty() {
            //     // i.e [..flags, flag] not [..flags, lang-id] then
            //     segment.push(last.clone());
            //     lang = None;
            // }
            // (
            //     lang,
            //     parse::pipeline(&segment, "!").collect::<Vec<_>>(), // this is not fun
            // )
        })
    });
    */
    // let _ = iter.collect::<Vec<_>>();

    // let _ = Rc::downgrade(&lang);

    // Ok(())
    Ok(Pipeline {
        segments,
        segment: iter::empty(),
        config,
        errors,
        lang,
        // lang: Rc::downgrade(&lang), //: Cow::Owned(lang),
    })
}

pub struct Pipeline<'a, I, P>
where
    I: Iterator<Item = &'a [OsString]> + 'a,
    P: Iterator<Item = Option<ProxyFlags>> + 'a,
{
    pub config: Config,
    segments: I,
    segment: P,
    lang: Rc<str>,
    errors: Vec<ArgError>,
}

impl<'a, I, P> Iterator for Pipeline<'a, I, P>
where
    I: Iterator<Item = &'a [OsString]> + 'a,
    P: Iterator<Item = Option<ProxyFlags>> + 'a,
{
    type Item = (rc::Weak<str>, Option<ProxyFlags>);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(proxy) = self.segment.next() {
            Some((Rc::downgrade(&self.lang), proxy))
        } else {
            let Some((lang, segment)) = self.segments.next().and_then(|segment| {
                segment.split_last().and_then(|(last, segment)| {
                    (!segment.is_empty())
                        .then_some((parse::lang_segment(last), parse::pipeline(&segment, "!")))
                })
            }) else {
                return None;
            };
            self.lang = lang;
            let mut segment = segment.map(|result| match result {
                Ok(proxy) => Some(proxy),
                Err(err) => {
                    self.errors.push(err);
                    None
                }
            });
            if let Some(proxy) = segment.next() {
                self.segment = segment; // ERROR: mismatch lifetime? 🤔
                Some((Rc::downgrade(&self.lang), proxy))
            } else {
                None
            }
            // None
            // match segment.next() {
            //     Some(Ok(proxy)) => {
            //         self.segment = segment;
            //     }
            //     Some(Err(err)) => {
            //         self.errors.push(err);
            //         Some((Rc::downgrade(&self.lang), None))
            //     }
            //     None => None,
            // }
        }
    }
}

impl<'a, I, P> Pipeline<'a, I, P>
where
    I: Iterator<Item = &'a [OsString]> + 'a,
    P: Iterator<Item = Option<ProxyFlags>> + 'a,
{
    pub fn errors(&'a self) -> &'a [ArgError] {
        self.errors.as_slice()
    }
}

pub mod parse {
    use super::*;

    /// get lang-id of a segment {lang}!
    pub fn lang_segment(lang: &OsStr) -> Rc<str> {
        Rc::from(
            lang.to_string_lossy()
                .trim_start_matches('{')
                .trim_end_matches("}!"),
        )
    }

    /// split cli args by: {\w*}!
    pub fn segments<'a>(args: &'a [OsString]) -> impl Iterator<Item = &'a [OsString]> + 'a {
        args.split_inclusive(|arg| {
            arg.to_str()
                .is_some_and(|arg| arg.starts_with('{') && arg.ends_with("}!"))
        })
    }

    /// split segment by [token], returning [ProxyFlags]
    pub fn pipeline<'a>(
        segment: &'a [OsString],
        token: &'a str,
    ) -> impl Iterator<Item = Result<ProxyFlags, ArgError>> + 'a {
        segment.split(move |arg| arg == token).map(proxy)
    }

    /// parse flags for config
    pub fn config<'a>(args: impl IntoIterator<Item = &'a OsString>) -> Result<Config, ArgError> {
        let ref mut parser = Parser::from_args(args);
        let mut config = Config::default();

        while let Some(arg) = parser.next()? {
            use Arg::*;
            match arg {
                Long("incremental") => config.incremental_changes = true,

                _ => return Err(arg.unexpected()),
            }
        }
        Ok(config)
    }

    /// parse flags for lsp proxy
    pub fn proxy<'a>(args: impl IntoIterator<Item = &'a OsString>) -> Result<ProxyFlags, ArgError> {
        let ref mut parser = Parser::from_args(args);
        let mut proxy = ProxyFlags::default();

        enum As {
            Completion,
            Unset,
        }
        let mut opt = As::Unset;

        while let Some(arg) = parser.next()? {
            use Arg::*;
            match (&arg, &opt) {
                (Long("lang"), _) => {
                    proxy.lang = parser.value()?.into_string().ok();
                    opt = As::Unset;
                }

                (Long("completion"), _) => {
                    let exec = Command::new(parser.value()?);
                    proxy.completion = Some(proxy::Completion {
                        proxy: proxy::PassThrough::ExecCommand(RwLock::new(exec)),
                        trigger_characters: None,
                    });
                    opt = As::Completion;
                }
                (
                    Long("completion-trigger-chars" | "trigger-chars") | Short('t'),
                    As::Completion,
                ) => {
                    if let Some(ref mut trigger_chars) = proxy
                        .completion
                        .as_mut()
                        .map(|c| c.trigger_characters.get_or_insert(Vec::new()))
                    {
                        for chars in parser.values()?.filter_map(|str| str.into_string().ok()) {
                            let chars =
                                &mut chars.split(' ').map(|char| char.to_string()).collect();
                            trigger_chars.append(chars);
                        }
                    };
                }

                _ => return Err(arg.unexpected()),
            }
        }

        Ok(proxy)
    }
}
