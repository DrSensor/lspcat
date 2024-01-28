use std::fmt::{Debug, UpperExp};

use smol::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use smol::stream::StreamExt;
use smol::{fs::File, io};
use tower_lsp::lsp_types::{MessageType, Range, TextDocumentContentChangeEvent};
use tower_lsp::Client;

/// Represent the edit state of a content
#[derive(Debug)]
pub enum State {
    Delete(usize, usize),
    Insert(usize, String),
    Replace(usize, String),
}

impl State {
    /// Get the edit `State` given the `range` of an edit and the edited `text` is known.
    /// Usually this information is available on `Incremental` changes of a content.
    ///
    /// # WARNING
    /// The `range` must be **byte** offset `(start, end)`, not `(line, column)` or character offset.
    /// See [`FileExt::get_state`] to get the `State` from `(line, column)`.
    ///
    /// # References
    /// - [`TextDocumentSyncKind`](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocumentSyncKind)
    /// - [`TextDocumentContentChangeEvent`](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocumentContentChangeEvent)
    pub fn of(range: (usize, usize), text: String) -> Self {
        let (start, end) = range;
        let length = end - start;
        match (length, text.is_empty()) {
            (_, true) => State::Delete(start, length),
            (0, false) => State::Insert(start, text),
            (_, false) => State::Replace(start, text),
        }
    }
}

pub trait FileExt
where
    Self: AsyncSeekExt + Unpin,
{
    /// Get the edit `State` given the `range` of an edit and the edited `text` is known.
    /// This internally call [`State::of`].
    async fn get_state(&mut self, range: Range, text: String) -> State;

    /// Apply the change of an edit `State` to this file.
    async fn apply(&mut self, state: State) -> io::Result<()>;

    /// Apply the change of the given `range` of an edit and the edited `text` to this file.
    async fn apply_change(&mut self, range: Range, text: String) -> io::Result<()> {
        let state = self.get_state(range, text).await;
        self.seek(SeekFrom::Start(0)).await?;
        self.apply(state).await
    }

    /// Get multiple edit `State` from [`lsp_types::DidChangeTextDocumentParams.content_changes`].
    async fn iter_states(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
        client: &Client,
    ) -> impl Iterator<Item = State>;

    /// Apply the changes of multiple edit `State` to this file.
    async fn apply_all(
        &mut self,
        states: impl IntoIterator<Item = State>,
        client: &Client,
    ) -> io::Result<()>;

    /// Apply the changes from [`lsp_types::DidChangeTextDocumentParams.content_changes`] to this file.
    async fn apply_all_changes(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
        client: &Client,
    ) -> io::Result<()> {
        let states: Vec<_> = self.iter_states(changes, client).await.collect();
        client
            .log_message(MessageType::LOG, format!("{:?}", states))
            .await;
        self.seek(SeekFrom::Start(0)).await?;
        self.apply_all(states, client).await // WARNING: refactoring `State<'a>::_(_, String)`
                                             // from `String` into `&'a str` or `Cow<'a, str>`
                                             // will make borrow-checker in here **angry**
    }
}

impl FileExt for File {
    async fn get_state(&mut self, range: Range, text: String) -> State {
        let lines = io::BufReader::new(self).lines();
        let range = lines
            .take(range.end.line as usize)
            .enumerate()
            .fold(
                (range.start.character as usize, range.end.character as usize),
                |mut offset, (loc, line)| {
                    if let Ok(line) = line {
                        let len = line.len() + 1;
                        if loc <= range.start.line as usize {
                            offset.0 += len;
                        }
                        offset.1 += len;
                    }
                    offset
                },
            )
            .await;
        State::of(range, text)
    }

    async fn apply(&mut self, state: State) -> io::Result<()> {
        let ref mut buf = Vec::new(); // TODO: limit buffer to 4K
        match state {
            State::Delete(offset, length) => {
                buf.resize(offset, 0);
                self.read_exact(buf).await?;
                self.seek(SeekFrom::Current(length as i64)).await?;
            }
            State::Insert(offset, text) => {
                buf.resize(offset, 0);
                self.read_exact(buf).await?;
                buf.append(&mut text.as_bytes().to_vec());
            }
            State::Replace(offset, text) => {
                buf.resize(offset, 0);
                self.read_exact(buf).await?;
                buf.append(&mut text.as_bytes().to_vec());
                self.seek(SeekFrom::Current(text.len() as i64 - 1)).await?;
            }
        };
        write_final(buf, self).await
    }

    async fn iter_states(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
        client: &Client,
    ) -> impl Iterator<Item = State> {
        let reader = io::BufReader::new(self);
        let changes = changes
            .into_iter()
            .filter_map(|diff| diff.range.map(|range| (range, diff.text)));

        client
            .log_message(MessageType::LOG, format!("{:?}", changes))
            .await;

        let lines = {
            let max_line = changes
                // .by_ref()
                .clone()
                .map(|(range, _)| range.end.line)
                .max()
                .unwrap_or_default();

            client
                .log_message(MessageType::LOG, format!("{:?}", max_line))
                .await;

            reader.lines().take(max_line as usize + 1).enumerate()
        };

        let mut current_offset = 0;
        let mut offset = Vec::<(usize, Option<(usize, String)>)>::with_capacity({
            let (lower, upper) = changes.size_hint();
            upper.unwrap_or(lower)
        });
        let mut changes: Vec<_> = changes.collect();
        lines // TODO: replace with `scan`
            .for_each(|(loc, line)| {
                if let Ok(line) = line {
                    let pos = changes.iter().position(|(range, text)| {
                        if loc == range.start.line as usize {
                            offset.push((current_offset + range.start.character as usize, None))
                        }
                        if loc == range.end.line as usize {
                            if let Some((_, end @ None)) = offset.last_mut() {
                                end.get_or_insert((
                                    current_offset + range.end.character as usize,
                                    text.to_string(),
                                ));
                                return true; // swap_remove(pos) to optimize iteration
                            }
                        }
                        false
                    });
                    if let Some(pos) = pos {
                        changes.swap_remove(pos);
                    }
                    current_offset += line.len() + 1;
                }
            })
            .await;

        client
            .log_message(
                MessageType::LOG,
                format!("{:?} {:?} {:?}", current_offset, offset, changes),
            )
            .await;

        offset
            .into_iter()
            .filter_map(|(start, end)| end.map(|(end, text)| (start, end, text)))
            .map(|(start, end, text)| {
                let length = end - start;
                match (length, text.is_empty()) {
                    (_, true) => State::Delete(start, length),
                    (0, false) => State::Insert(start, text),
                    (_, false) => State::Replace(start, text),
                }
            })
    }

    async fn apply_all<'a>(
        &'a mut self,
        states: impl IntoIterator<Item = State>,
        client: &Client,
    ) -> io::Result<()> {
        let ref mut result = Vec::new(); // TODO: limit buffer to 4K
        let mut last_offset = 0;
        for state in states {
            match state {
                State::Delete(offset, length) => {
                    let ref mut buf = vec![0; offset - last_offset];
                    self.read_exact(buf).await?;
                    result.append(buf);
                    self.seek(SeekFrom::Current(length as i64)).await?;
                    last_offset = offset;
                    client
                        .log_message(MessageType::LOG, format!("{:?}", result))
                        .await;
                }
                State::Insert(offset, text) => {
                    let ref mut buf = vec![0; offset - last_offset];
                    self.read_exact(buf).await?;
                    result.append(buf);
                    result.append(&mut text.as_bytes().to_vec());
                    last_offset = offset;
                }
                State::Replace(offset, text) => {
                    let ref mut buf = vec![0; offset - last_offset];
                    self.read_exact(buf).await?;
                    result.append(buf);
                    result.append(&mut text.as_bytes().to_vec());
                    self.seek(SeekFrom::Current(text.len() as i64 - 1)).await?;
                    last_offset = offset;
                }
            }
        }
        write_final(result, self).await
    }
}

/// When mirroring the edit of an actual file into /tmp/file, it split into 2 stage: partial & final.
async fn write_final(buf: &mut Vec<u8>, file: &mut File) -> io::Result<()> {
    file.read_to_end(buf).await?;
    file.seek(SeekFrom::Start(0)).await?;
    file.write_all(buf).await?;
    file.set_len(buf.len() as u64).await
}
