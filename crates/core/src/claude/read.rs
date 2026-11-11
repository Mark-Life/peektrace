//! Stream a transcript file into a header without holding the whole file.

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use super::header::{FileMeta, HeaderAcc};
use crate::model::SessionHeader;

const BOM: &[u8] = b"\xEF\xBB\xBF";
const READ_BUFFER: usize = 64 * 1024;

/// What one transcript yields: a header, or nothing for an empty file.
#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    Header(Box<SessionHeader>),
    Skip,
}

/// Fold a transcript from `reader`. Lines split on `\n`; bytes that are not
/// UTF-8 become U+FFFD and a leading BOM is dropped, as `TextDecoder` does.
pub fn read_transcript(mut reader: impl BufRead, meta: FileMeta) -> io::Result<Loaded> {
    let mut acc = HeaderAcc::default();
    let mut buf = Vec::new();
    let mut first = true;
    let mut text_empty = true;
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let mut line = buf.as_slice();
        if first {
            line = line.strip_prefix(BOM).unwrap_or(line);
            first = false;
        }
        text_empty &= line.is_empty();
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let text =
            std::str::from_utf8(line).map_or_else(|_| String::from_utf8_lossy(line), Cow::Borrowed);
        acc.push_line(&text);
    }
    if text_empty && meta.size_bytes == 0 {
        return Ok(Loaded::Skip);
    }
    Ok(Loaded::Header(Box::new(acc.finish(meta))))
}

/// Open and fold one transcript file.
pub fn load_file(path: &Path, meta: FileMeta) -> io::Result<Loaded> {
    let file = File::open(path)?;
    read_transcript(BufReader::with_capacity(READ_BUFFER, file), meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProjectSlug, SessionId};

    fn meta(size_bytes: u64) -> FileMeta {
        FileMeta {
            id: SessionId::new("s"),
            slug: ProjectSlug::new("p"),
            path: "/p/s.jsonl".to_owned(),
            size_bytes,
            mtime_ms: 1_000,
            source: None,
        }
    }

    fn load(bytes: &[u8]) -> Loaded {
        let size = bytes.len() as u64;
        read_transcript(bytes, meta(size)).expect("in-memory read")
    }

    #[test]
    fn empty_file_is_skipped() {
        assert_eq!(load(b""), Loaded::Skip);
    }

    #[test]
    fn bom_only_file_is_a_row() {
        let Loaded::Header(h) = load(BOM) else {
            panic!("expected a header")
        };
        assert_eq!(h.message_count, 0);
        assert_eq!(h.updated_at, "1970-01-01T00:00:01.000Z");
    }

    #[test]
    fn invalid_utf8_is_replaced() {
        let Loaded::Header(h) = load(b"{\"type\":\"ai-title\",\"aiTitle\":\"a\xFFb\"}\n") else {
            panic!("expected a header")
        };
        assert_eq!(h.title.as_deref(), Some("a\u{FFFD}b"));
    }
}
