//! Implementation of the jsonlines history file format.
//! See the internal docs fish-history-file-format.md for details.
use super::file::MmapRegion;
use super::history::{HistoryItem, HistoryItemId};
use crate::prelude::*;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sonic_rs::{self, JsonValueTrait as _, LazyValue};
use std::{io::Write as _, time::SystemTime};

use super::history::{PreparedQuery, SearchQuery};

/// Number of base64url (no padding) characters needed to encode a u64.
const BASE64_U64_LEN: usize = 11;

/// Encode a u64 as a URL-safe base64 string (no padding).
/// Uses big-endian byte order for consistent on-disk representation.
fn base64_encode_u64(value: u64) -> String {
    URL_SAFE_NO_PAD.encode(value.to_be_bytes())
}

/// Decode a URL-safe base64 string to u64.
/// Returns None if the string is not valid base64 or doesn't decode to exactly 8 bytes.
#[inline(always)]
fn base64_decode_u64(s: &[u8]) -> Option<u64> {
    const N: usize = size_of::<u64>();
    let mut out = [0u8; N];
    let amt = URL_SAFE_NO_PAD.decode_slice(s, &mut out).ok()?;
    if amt != N {
        return None;
    }
    Some(u64::from_be_bytes(out))
}

// Convert a WString to and from UTF-8. Private-use-area characters are retained.
fn wstring_to_utf8(s: &WString) -> String {
    s.chars().collect()
}

fn utf8_to_wstring(s: &str) -> WString {
    s.chars().collect()
}

/// Set a WString from UTF-8, reusing the existing allocation.
fn wstring_set_utf8(dest: &mut WString, src: &str) {
    dest.clear();
    dest.extend(src.chars());
}

impl HistoryItem {
    /// Encode this item as a JSON line string, with a trailing newline.
    pub(super) fn to_json_line(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.write_to(&mut buf).unwrap();
        buf
    }

    fn apply_json_field(&mut self, key: &str, value: LazyValue<'_>) {
        match key {
            "cmd" => {
                if let Some(cmd) = value.as_str() {
                    wstring_set_utf8(&mut self.contents, cmd);
                }
            }
            "exit" => {
                if let Some(exit) = value.as_i64().and_then(|exit| i32::try_from(exit).ok()) {
                    self.exit_code = Some(exit);
                }
            }
            "dur" => {
                if let Some(dur) = value.as_u64() {
                    self.duration = Some(dur);
                }
            }
            "cwd" => {
                if let Some(cwd) = value.as_str() {
                    match &mut self.cwd {
                        Some(existing) => wstring_set_utf8(existing, cwd),
                        None => self.cwd = Some(utf8_to_wstring(cwd)),
                    }
                }
            }
            "sid" => {
                if let Some(sid) = value.as_str().and_then(|s| base64_decode_u64(s.as_bytes())) {
                    self.session_id = Some(sid);
                }
            }
            "paths" => {
                if let Some(paths) = value.into_array_iter() {
                    self.required_paths.clear();
                    self.required_paths.extend(
                        paths
                            .filter_map(Result::ok)
                            .filter_map(|entry| entry.as_str().map(utf8_to_wstring)),
                    );
                }
            }
            _ => {}
        }
    }

    /// Append this history item to a buffer in JSON lines format.
    pub(super) fn write_to(&self, buffer: &mut impl std::io::Write) -> std::io::Result<()> {
        use serde_core::ser::{SerializeMap as _, Serializer as _};

        let mut buffer = sonic_rs::writer::BufferedWriter::new(buffer);
        (|| -> Result<(), sonic_rs::Error> {
            let mut serializer = sonic_rs::Serializer::new(&mut buffer);
            let mut map = (&mut serializer).serialize_map(None)?;

            let id = base64_encode_u64(self.id.raw());
            map.serialize_entry("id", id.as_str())?;

            if !self.contents.is_empty() {
                let cmd = wstring_to_utf8(&self.contents);
                map.serialize_entry("cmd", cmd.as_str())?;
            }
            if !self.required_paths.is_empty() {
                let paths: Vec<String> = self.required_paths.iter().map(wstring_to_utf8).collect();
                map.serialize_entry("paths", &paths)?;
            }
            if let Some(exit) = self.exit_code {
                map.serialize_entry("exit", &exit)?;
            }
            if let Some(dur) = self.duration {
                map.serialize_entry("dur", &dur)?;
            }
            if let Some(cwd) = &self.cwd {
                let cwd = wstring_to_utf8(cwd);
                map.serialize_entry("cwd", cwd.as_str())?;
            }
            if let Some(sid) = self.session_id {
                let sid = base64_encode_u64(sid);
                map.serialize_entry("sid", sid.as_str())?;
            }
            map.end()
        })()
        .map_err(|err| std::io::Error::other(format!("json encode error: {err}")))?;
        buffer.write_all(b"\n")
    }
}

/// Offset to a specific line in the JSONL history file.
/// Each line contains JSON metadata for a history item.
/// Multiple lines may share the same ID and together form a single item.
/// Note the field order matters for sorting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FileLineOffset {
    id: HistoryItemId, // The history item this line contributes to.
    offset: usize,     // Byte offset within the file.
}

pub(super) struct HistoryFile<T: AsRef<[u8]> = MmapRegion> {
    // The backing data source.
    backing: Option<T>,
    // Offsets of lines within the file.
    // These are sorted such that item IDs are contiguous, and offsets are in ascending order.
    line_offsets: Vec<FileLineOffset>,
    // Starting positions for items within the line_offsets vector.
    // Each entry is an index into line_offsets pointing to the first line of an item.
    item_starts: Vec<usize>,
}

impl<T: AsRef<[u8]>> HistoryFile<T> {
    /// Create an empty history.
    pub fn create_empty() -> Self {
        Self {
            backing: None,
            line_offsets: Vec::new(),
            item_starts: Vec::new(),
        }
    }

    /// Create from a data source, parsing the JSON lines within to index line IDs and offsets.
    /// If cutoff is given, skip items whose timestamp is newer than cutoff.
    pub fn from_data(backing: T, cutoff: Option<SystemTime>) -> Self {
        // Our timestamps are encoded within the item IDs; construct an item ID that reflects the cutoff.
        let cutoff_id = cutoff.map(|ts| HistoryItemId::new(ts, 0));
        let try_make_line_offset = move |(offset, line): (usize, &[u8])| -> Option<FileLineOffset> {
            let id_raw = id_for_json_line(line)?;
            let id = HistoryItemId::from_raw(id_raw);
            match cutoff_id {
                Some(c_id) if id > c_id => None, // Skip items newer than cutoff
                _ => Some(FileLineOffset { id, offset }),
            }
        };
        let mut line_offsets: Vec<FileLineOffset> = iter_lines(backing.as_ref())
            .filter_map(try_make_line_offset)
            .collect();
        // The crux: stable-sort the line offsets!
        // This collects all lines with the same IDs together, in order of file offset.
        // The idea is that the first line establishes the item (including its command)
        // and subsequent lines add additional fields which are only discovered later (exit status, valid paths, etc).
        // Because the items are contiguous, we only need to walk the list once to assemble complete items.
        // Note that we expect that our input file is already mostly sorted, and Rust's default sort is optimized for this.
        line_offsets.sort();

        // Build item_starts: indices within line_offsets of the first line of each unique item.
        let item_starts: Vec<usize> = (0..line_offsets.len())
            .filter(|&idx| idx == 0 || line_offsets[idx].id != line_offsets[idx - 1].id)
            .collect();

        Self {
            backing: Some(backing),
            line_offsets,
            item_starts,
        }
    }

    /// Return true if the history file is empty.
    pub fn is_empty(&self) -> bool {
        self.line_offsets.is_empty()
    }

    /// Return the number of (valid) lines in the history file.
    pub fn line_count(&self) -> usize {
        self.line_offsets.len()
    }

    /// Return the number of unique items in the history file.
    pub fn item_count(&self) -> usize {
        self.item_starts.len()
    }

    /// Return the history item at the given index.
    pub fn item_at(&self, idx: usize) -> HistoryItem {
        let start = self.item_starts[idx];
        let mut line_idx = start;
        let line_offset = self.line_offsets[line_idx];
        let data = self.backing.as_ref().unwrap().as_ref();

        let line_offsets = &self.line_offsets;
        let mut item = HistoryItem::with_id(line_offset.id);
        while line_idx < line_offsets.len() && line_offsets[line_idx].id == line_offset.id {
            let line_offset = &line_offsets[line_idx];
            let (line, _) = read_line_at(data, line_offset.offset);
            for field in sonic_rs::to_object_iter(line) {
                let Ok((key, value)) = field else {
                    break;
                };
                item.apply_json_field(key.as_ref(), value);
            }
            line_idx += 1;
        }
        item
    }

    /// Get an item by reverse index, using provided buffers for parsing.
    /// Index 0 is the most recent item, 1 is second-most recent, etc.
    #[allow(dead_code)]
    pub fn get_from_back_with_buffers(&self, idx: usize) -> Option<HistoryItem> {
        let item_count = self.item_starts.len();
        if idx >= item_count {
            return None;
        }
        Some(self.item_at(item_count - idx - 1))
    }

    // Convenience cover that doesn't re-use storage (read: isn't optimized).
    #[allow(dead_code)]
    pub fn get_from_back(&self, idx: usize) -> Option<HistoryItem> {
        self.get_from_back_with_buffers(idx)
    }

    // Convenience cover over items() that doesn't re-use storage (read: isn't optimized).
    pub fn items(&self) -> impl DoubleEndedIterator<Item = HistoryItem> + ExactSizeIterator + '_ {
        let range = 0..self.item_starts.len();
        range.map(move |idx| self.item_at(idx))
    }

    /// Decode an item at the given index into the provided decoder.
    /// Reuses allocations in the decoder.
    pub fn decode_item_at(&self, idx: usize, decoder: &mut HistoryItemDecoder) {
        let start = self.item_starts[idx];
        let line_offset = self.line_offsets[start];
        decoder.reset(line_offset.id);

        let data = self.backing.as_ref().unwrap().as_ref();
        let mut line_idx = start;
        while line_idx < self.line_offsets.len() && self.line_offsets[line_idx].id == line_offset.id
        {
            let (line, _) = read_line_at(data, self.line_offsets[line_idx].offset);
            decoder.add_line(line);
            line_idx += 1;
        }
    }

    /// Decode an item by reverse index into the provided decoder.
    /// Returns false if index is out of bounds.
    pub fn decode_from_back(&self, idx: usize, decoder: &mut HistoryItemDecoder) -> bool {
        let item_count = self.item_starts.len();
        if idx >= item_count {
            return false;
        }
        self.decode_item_at(item_count - idx - 1, decoder);
        true
    }

    /// Search if item at index matches the search term.
    /// Returns true if matches, and decoder will have the item populated.
    pub fn search_matches(
        &self,
        idx: usize,
        query: &SearchQuery,
        decoder: &mut HistoryItemDecoder,
    ) -> bool {
        let start = self.item_starts[idx];
        let item_id = self.line_offsets[start].id;
        let data = self.backing.as_ref().unwrap().as_ref();
        let prepared = PreparedQuery::from_query(query);

        // Scan forward while same item ID, looking for cmd field.
        // Break early when found - cmd appears once per item.
        // "Last wins" semantics are handled by decode_item_at for matches.
        let mut line_idx = start;

        while line_idx < self.line_offsets.len() && self.line_offsets[line_idx].id == item_id {
            let (line, _) = read_line_at(data, self.line_offsets[line_idx].offset);

            if let Ok(value) = sonic_rs::get_from_slice(line, &["cmd"]) {
                if let Some(cmd) = value.as_str() {
                    if prepared.matches_str(cmd) {
                        self.decode_item_at(idx, decoder);
                        return true;
                    }
                    // Found cmd but didn't match - no need to check other lines.
                    return false;
                }
            }
            line_idx += 1;
        }

        // No cmd field found
        false
    }

    /// Search by reverse index.
    pub fn search_matches_from_back(
        &self,
        idx: usize,
        query: &SearchQuery,
        decoder: &mut HistoryItemDecoder,
    ) -> bool {
        let item_count = self.item_starts.len();
        if idx >= item_count {
            return false;
        }
        self.search_matches(item_count - idx - 1, query, decoder)
    }

    /// Shrink the history to at most max_records unique items, removing the oldest ones.
    /// This does not modify the file; it merely discards line offsets.
    pub fn shrink_to_max_records(&mut self, max_records: usize) {
        let num_records = self.item_count();
        if num_records <= max_records {
            return;
        } else if max_records == 0 {
            self.line_offsets.clear();
            self.item_starts.clear();
            return;
        }

        // Find the oldest item to keep; this contains the index of the first line to retain.
        // Remove item_starts and line_offsets prior to that.
        let oldest = self.item_starts[num_records - max_records];
        self.line_offsets.drain(0..oldest);
        self.item_starts.drain(0..(num_records - max_records));
        for start in &mut self.item_starts {
            *start -= oldest;
        }
    }
}

/// Decoder for multi-line history items.
/// Parses lines and accumulates fields into a reusable HistoryItem.
/// Later lines overwrite earlier ones ("last wins").
pub(super) struct HistoryItemDecoder {
    item: HistoryItem,
}

impl HistoryItemDecoder {
    pub(super) fn new() -> Self {
        Self {
            item: HistoryItem::with_id(HistoryItemId::from_raw(0)),
        }
    }

    /// Reset for decoding a new item, keeping allocations.
    pub(super) fn reset(&mut self, id: HistoryItemId) {
        self.item.id = id;
        self.item.contents.clear();
        self.item.required_paths.clear();
        self.item.exit_code = None;
        self.item.duration = None;
        // Keep cwd allocation if present
        if let Some(ref mut cwd) = self.item.cwd {
            cwd.clear();
        }
        self.item.cwd = None;
        self.item.session_id = None;
    }

    /// Parse a line and merge its fields into the item.
    pub(super) fn add_line(&mut self, line: &[u8]) {
        for field in sonic_rs::to_object_iter(line) {
            let Ok((key, value)) = field else {
                break;
            };
            self.item.apply_json_field(key.as_ref(), value);
        }
    }

    /// Get a reference to the accumulated item.
    pub(super) fn item(&self) -> &HistoryItem {
        &self.item
    }
}

/// Read a single line from the buffer starting at the given offset.
/// Returns the line (without newline) and the offset of the next line, or None if this is the last line.
fn read_line_at(buf: &[u8], line_start: usize) -> (&[u8], Option<usize>) {
    let Some(remaining) = buf.get(line_start..) else {
        return (&[], None);
    };
    let line_len = memchr::memchr(b'\n', remaining).unwrap_or(remaining.len());
    let line = &remaining[..line_len];
    let next_line_start = line_start + line_len + 1;
    if next_line_start < buf.len() {
        (line, Some(next_line_start))
    } else {
        (line, None)
    }
}

// Iterate over lines of a (hopefully) UTF-8 encoded buffer.
// Returns tuples of (offset, line) where offset is the byte position in the original buffer.
// Newlines are not retained.
fn iter_lines(buf: &[u8]) -> impl Iterator<Item = (usize, &[u8])> + '_ {
    let mut offset = 0usize;
    std::iter::from_fn(move || {
        if offset >= buf.len() {
            return None;
        }
        let start = offset;
        let (line, next) = read_line_at(buf, start);
        offset = next.unwrap_or(buf.len());
        Some((start, line))
    })
}

/// Attempt to return the history id from a JSON line quickly, without a full parse.
/// This looks for `{ "id": "base64string", ... }`.
///
/// It does NOT perform full JSON parsing or validation - this is deferred until the history item is actually decoded.
/// That is, if this function returns Some(id), then it contains the history item id if the line is valid JSON.
///
/// This is a hot function since it's used for the initial history parse, at which point all we're concerned about is
/// the "id" field. Note fish controls the key output order: "id" is always first when fish writes the file (see to_json),
/// so use a mini custom parser.
#[inline(never)]
fn try_parse_id_fast(line: &[u8]) -> Option<u64> {
    // Helper to skip whitespace
    let ws = |i: &mut usize| {
        while line.get(*i).is_some_and(u8::is_ascii_whitespace) {
            *i += 1;
        }
    };

    // Helper to eat a literal
    let eat_lit = |i: &mut usize, lit: &[u8]| -> Option<()> {
        let v = line.get(*i..*i + lit.len())?;
        (v == lit).then(|| {
            *i += lit.len();
        })
    };

    // Whitespace, initial brace, whitespace, "id" key with quotes, colon, whitespace, opening quote.
    let mut i = 0usize;
    ws(&mut i);
    eat_lit(&mut i, b"{")?;
    ws(&mut i);
    eat_lit(&mut i, br#""id""#)?;
    ws(&mut i);
    eat_lit(&mut i, b":")?;
    ws(&mut i);
    eat_lit(&mut i, b"\"")?;

    let start = i;
    if i + BASE64_U64_LEN > line.len() {
        return None;
    }
    let id_slice: &[u8] = &line[start..start + BASE64_U64_LEN];
    i += BASE64_U64_LEN;

    eat_lit(&mut i, b"\"")?;
    base64_decode_u64(id_slice)
}

/// Parse the ID field from a JSON line.
/// Returns None if the line is not valid JSON or lacks an "id" field.
pub fn id_for_json_line(line: &[u8]) -> Option<u64> {
    if let Some(id) = try_parse_id_fast(line) {
        return Some(id);
    }
    // Fall back to a slow path.
    const ID_PATH: [&str; 1] = ["id"];
    let value = sonic_rs::get_from_slice(line, &ID_PATH).ok()?;
    let s = value.as_str()?;
    base64_decode_u64(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryFile, HistoryItemDecoder, base64_decode_u64, base64_encode_u64, id_for_json_line,
        iter_lines, read_line_at, try_parse_id_fast,
    };
    use crate::history::history::{HistoryItem, HistoryItemId};
    use crate::history::history::{SearchQuery, SearchType};
    use crate::prelude::*;

    // Test helper: assert that a HistoryItem matches expected values
    fn assert_item_eq(item: &HistoryItem, id: u64, cmd: &str, paths: &[&str]) {
        assert_eq!(item.id.raw(), id, "ID mismatch");
        assert_eq!(item.contents, WString::from(cmd), "Command mismatch");
        let paths: Vec<WString> = paths.iter().map(|s| WString::from_str(*s)).collect();
        assert_eq!(item.required_paths, paths, "Paths mismatch");
    }

    /// Helper to create a JSON line with a base64-encoded ID
    fn json_line(id: u64, extra: &str) -> String {
        let id_encoded = base64_encode_u64(id);
        if extra.is_empty() {
            format!(r#"{{"id":"{}"}}"#, id_encoded)
        } else {
            format!(r#"{{"id":"{}",{}}}"#, id_encoded, extra)
        }
    }

    #[test]
    fn test_base64_encode_decode() {
        // Round-trip tests for various u64 values
        for &value in &[0u64, 1, 42, 255, 256, 65535, u64::MAX / 2, u64::MAX] {
            let encoded = base64_encode_u64(value);
            assert_eq!(encoded.len(), 11, "Encoded length should be 11");
            let decoded = base64_decode_u64(encoded.as_bytes());
            assert_eq!(decoded, Some(value), "Round-trip failed for {}", value);
        }

        // Test typical timestamp-based IDs
        let timestamp_id = 1737745234567890123u64;
        let encoded = base64_encode_u64(timestamp_id);
        assert_eq!(base64_decode_u64(encoded.as_bytes()), Some(timestamp_id));

        // Verify all base64 characters are URL-safe
        let encoded = base64_encode_u64(u64::MAX);
        for c in encoded.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '_',
                "Non-URL-safe character: {}",
                c
            );
        }

        // Invalid: wrong length (doesn't decode to exactly 8 bytes)
        assert_eq!(base64_decode_u64(b"AAAAAAAAAA"), None); // 10 chars -> 7.5 bytes
        assert_eq!(base64_decode_u64(b"AAAAAAAAAAAA"), None); // 12 chars -> 9 bytes
        assert_eq!(base64_decode_u64(b""), None);

        // Invalid: bad characters
        assert_eq!(base64_decode_u64(b"AAAAAAAA!AA"), None);
        assert_eq!(base64_decode_u64(b"AAAAAAAA+AA"), None); // + is not URL-safe
        assert_eq!(base64_decode_u64(b"AAAAAAAA/AA"), None); // / is not URL-safe
    }

    #[test]
    fn test_try_parse_id_fast_base64() {
        // Valid base64 format
        let id = 42u64;
        let encoded = base64_encode_u64(id);
        let json_line = format!(r#"{{"id":"{}","cmd":"test"}}"#, encoded);
        assert_eq!(try_parse_id_fast(json_line.as_bytes()), Some(id));

        // Edge case: zero
        let encoded = base64_encode_u64(0);
        let json_line = format!(r#"{{"id":"{}"}}"#, encoded);
        assert_eq!(try_parse_id_fast(json_line.as_bytes()), Some(0));

        // Edge case: max u64
        let encoded = base64_encode_u64(u64::MAX);
        let json_line = format!(r#"{{"id":"{}"}}"#, encoded);
        assert_eq!(try_parse_id_fast(json_line.as_bytes()), Some(u64::MAX));

        // With whitespace
        let encoded = base64_encode_u64(123);
        let json_line = format!(r#"{{ "id" : "{}" }}"#, encoded);
        assert_eq!(try_parse_id_fast(json_line.as_bytes()), Some(123));

        // Invalid: wrong length string
        assert_eq!(try_parse_id_fast(br#"{"id":"AAAAAAAAAA"}"#), None); // 10 chars
        assert_eq!(try_parse_id_fast(br#"{"id":"AAAAAAAAAAAA"}"#), None); // 12 chars

        // Invalid: bad base64 characters
        assert_eq!(try_parse_id_fast(br#"{"id":"AAAAAAAA!AA"}"#), None);

        // Invalid: missing closing quote
        assert_eq!(try_parse_id_fast(br#"{"id":"AAAAAAAAAAA}"#), None);
    }

    #[test]
    fn test_read_line_at() {
        // Empty buffer
        let buf = b"";
        assert_eq!(read_line_at(buf, 0), (&b""[..], None));
        assert_eq!(read_line_at(buf, 10), (&b""[..], None));

        // Single line without newline
        let buf = b"hello";
        assert_eq!(read_line_at(buf, 0), (&b"hello"[..], None));

        // Single line with newline
        let buf = b"hello\n";
        assert_eq!(read_line_at(buf, 0), (&b"hello"[..], None));

        // Single line with newline followed by content
        let buf = b"hello\nworld";
        assert_eq!(read_line_at(buf, 0), (&b"hello"[..], Some(6)));
        assert_eq!(read_line_at(buf, 6), (&b"world"[..], None));

        // Multiple lines with newlines
        let buf = b"foo\nbar\nbaz\n";
        assert_eq!(read_line_at(buf, 0), (&b"foo"[..], Some(4)));
        assert_eq!(read_line_at(buf, 4), (&b"bar"[..], Some(8)));
        assert_eq!(read_line_at(buf, 8), (&b"baz"[..], None));

        // Empty lines (consecutive newlines)
        let buf = b"\n\nfoo\n";
        assert_eq!(read_line_at(buf, 0), (&b""[..], Some(1)));
        assert_eq!(read_line_at(buf, 1), (&b""[..], Some(2)));
        assert_eq!(read_line_at(buf, 2), (&b"foo"[..], None));

        // Line at various positions
        let buf = b"alpha\nbeta\ngamma";
        assert_eq!(read_line_at(buf, 0), (&b"alpha"[..], Some(6)));
        assert_eq!(read_line_at(buf, 6), (&b"beta"[..], Some(11)));
        assert_eq!(read_line_at(buf, 11), (&b"gamma"[..], None));

        // Offset at exact buffer length
        let buf = b"test";
        assert_eq!(read_line_at(buf, 4), (&b""[..], None));

        // Offset beyond buffer
        assert_eq!(read_line_at(buf, 5), (&b""[..], None));
        assert_eq!(read_line_at(buf, 100), (&b""[..], None));

        // Buffer ending with multiple newlines
        let buf = b"text\n\n";
        assert_eq!(read_line_at(buf, 0), (&b"text"[..], Some(5)));
        assert_eq!(read_line_at(buf, 5), (&b""[..], None));
    }

    #[test]
    fn test_iter_lines() {
        let lines: Vec<(usize, &[u8])> = iter_lines(b"foo\nbar\nbaz\n").collect();
        assert_eq!(
            lines,
            vec![(0, &b"foo"[..]), (4, &b"bar"[..]), (8, &b"baz"[..])]
        );

        let lines: Vec<(usize, &[u8])> = iter_lines(b"alpha\nomega").collect();
        assert_eq!(lines, vec![(0, &b"alpha"[..]), (6, &b"omega"[..])]);

        let lines: Vec<(usize, &[u8])> = iter_lines(b"\nfoo\n\n").collect();
        assert_eq!(lines, vec![(0, &b""[..]), (1, &b"foo"[..]), (5, &b""[..])]);

        let empty: Vec<(usize, &[u8])> = iter_lines(b"").collect();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_id_for_json_line() {
        // Base64 format via fast path (id is first key)
        let id = 42u64;
        let encoded = base64_encode_u64(id);
        let json_line = format!(r#"{{"id":"{}","cmd":"test"}}"#, encoded);
        assert_eq!(id_for_json_line(json_line.as_bytes()), Some(id));

        // Base64 format via slow path (id is not first key, needs full JSON parse)
        let json_line = format!(r#"{{"cmd":"test","id":"{}"}}"#, encoded);
        assert_eq!(id_for_json_line(json_line.as_bytes()), Some(id));

        // Edge cases
        let encoded_zero = base64_encode_u64(0);
        let json_line = format!(r#"{{"id":"{}"}}"#, encoded_zero);
        assert_eq!(id_for_json_line(json_line.as_bytes()), Some(0));

        let encoded_max = base64_encode_u64(u64::MAX);
        let json_line = format!(r#"{{"id":"{}"}}"#, encoded_max);
        assert_eq!(id_for_json_line(json_line.as_bytes()), Some(u64::MAX));

        // Invalid base64 in slow path
        assert_eq!(
            id_for_json_line(br#"{"cmd":"test","id":"invalid!!!!"}"#),
            None
        );
    }

    #[test]
    fn test_item_count() {
        // Empty
        let history: HistoryFile<&[u8]> = HistoryFile::create_empty();
        assert_eq!(history.item_count(), 0);

        // Single item
        let data = json_line(100, r#""cmd":"echo hello""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 1);

        // Multiple unique items, one line each
        let data = [
            json_line(100, r#""cmd":"ls""#),
            json_line(200, r#""cmd":"pwd""#),
            json_line(300, r#""cmd":"cd""#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 3);

        // Item with multiple lines
        let data = [
            json_line(100, r#""cmd":"ls""#),
            json_line(100, r#""exit":0"#),
            json_line(100, r#""paths":["/tmp"]"#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 1);

        // Mix of single-line and multi-line items
        let data = [
            json_line(100, r#""cmd":"ls""#),
            json_line(100, r#""exit":0"#),
            json_line(200, r#""cmd":"pwd""#),
            json_line(300, r#""cmd":"cd""#),
            json_line(300, r#""exit":1"#),
            json_line(300, r#""paths":["/home"]"#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 3);

        // Unsorted input - should be sorted by from_data
        let data = [
            json_line(300, r#""cmd":"cd""#),
            json_line(100, r#""cmd":"ls""#),
            json_line(200, r#""cmd":"pwd""#),
            json_line(100, r#""exit":0"#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 3);
    }

    #[test]
    fn test_exit_code_round_trip() {
        use std::time::SystemTime;

        // Single line with an exit code
        let data = json_line(999, r#""cmd":"git commit","exit":1,"paths":["/repo/.git"]"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.exit_code, Some(1));

        // Negative exit code (signal)
        let data = json_line(555, r#""cmd":"killed","exit":-9"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.exit_code, Some(-9));

        // No exit code present
        let data = json_line(1, r#""cmd":"still running""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.exit_code, None);

        // Exit code arriving on a later line for the same item
        let data = [
            json_line(100, r#""cmd":"ls /tmp""#),
            json_line(100, r#""exit":0"#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.exit_code, Some(0));

        // Write then re-parse round-trip
        let mut item = HistoryItem::with_id(HistoryItemId::new(SystemTime::now(), 0));
        item.contents = WString::from("echo hi");
        item.exit_code = Some(42);
        let encoded = item.to_json_line();
        let history = HistoryFile::from_data(encoded.as_slice(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.exit_code, Some(42));
    }

    #[test]
    fn test_duration_round_trip() {
        use std::time::SystemTime;

        // Single line with a duration
        let data = json_line(1, r#""cmd":"sleep 5","dur":5001"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.duration, Some(5001));

        // No duration present
        let data = json_line(2, r#""cmd":"still running""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.duration, None);

        // Duration arriving on a later line for the same item
        let data = [
            json_line(100, r#""cmd":"ls /tmp""#),
            json_line(100, r#""dur":12"#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.duration, Some(12));

        // Write then re-parse round-trip
        let mut item = HistoryItem::with_id(HistoryItemId::new(SystemTime::now(), 0));
        item.contents = WString::from("echo hi");
        item.duration = Some(1234);
        let encoded = item.to_json_line();
        let history = HistoryFile::from_data(encoded.as_slice(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.duration, Some(1234));
    }

    #[test]
    fn test_cwd_round_trip() {
        use std::time::SystemTime;

        // Single line with a cwd
        let data = json_line(1, r#""cmd":"ls","cwd":"/home/user""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.cwd, Some(WString::from("/home/user")));

        // No cwd present
        let data = json_line(2, r#""cmd":"still running""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.cwd, None);

        // cwd arriving on a later line for the same item, reusing the existing allocation
        let data = [
            json_line(100, r#""cmd":"ls /tmp""#),
            json_line(100, r#""cwd":"/repo""#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.cwd, Some(WString::from("/repo")));

        // Unicode cwd
        let data = json_line(3, r#""cmd":"ls","cwd":"/home/你好""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.cwd, Some(WString::from("/home/你好")));

        // Write then re-parse round-trip
        let mut item = HistoryItem::with_id(HistoryItemId::new(SystemTime::now(), 0));
        item.contents = WString::from("echo hi");
        item.cwd = Some(WString::from("/tmp"));
        let encoded = item.to_json_line();
        let history = HistoryFile::from_data(encoded.as_slice(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.cwd, Some(WString::from("/tmp")));
    }

    #[test]
    fn test_session_id_round_trip() {
        use std::time::SystemTime;

        // Single line with a session id
        let sid_encoded = base64_encode_u64(12345);
        let data = json_line(1, &format!(r#""cmd":"ls","sid":"{sid_encoded}""#));
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.session_id, Some(12345));

        // No session id present
        let data = json_line(2, r#""cmd":"still running""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.session_id, None);

        // Invalid base64 - field is simply not set
        let data = json_line(3, r#""cmd":"ls","sid":"not valid base64!""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.session_id, None);

        // Write then re-parse round-trip
        let mut item = HistoryItem::with_id(HistoryItemId::new(SystemTime::now(), 0));
        item.contents = WString::from("echo hi");
        item.session_id = Some(u64::MAX);
        let encoded = item.to_json_line();
        let history = HistoryFile::from_data(encoded.as_slice(), None);
        let item = history.items().next().unwrap();
        assert_eq!(item.session_id, Some(u64::MAX));
    }

    #[test]
    fn test_item_parsing_single_items() {
        // Simple item with just a command
        let data = json_line(42, r#""cmd":"echo hello""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 42, "echo hello", &[]);

        // Single line with all fields
        let data = json_line(999, r#""cmd":"git commit","exit":1,"paths":["/repo/.git"]"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 999, "git commit", &["/repo/.git"]);

        // Empty command (auxiliary item)
        let data = json_line(77, r#""exit":0"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 77, "", &[]);

        // Unicode command
        let data = json_line(888, r#""cmd":"echo \u4f60\u597d""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 888, "echo 你好", &[]);

        // Negative exit code (signal)
        let data = json_line(555, r#""cmd":"killed","exit":-9"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 555, "killed", &[]);

        // Empty paths array
        let data = json_line(666, r#""cmd":"test","paths":[]"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 666, "test", &[]);
    }

    #[test]
    fn test_item_parsing_multiple_lines() {
        // Item split across multiple lines (command, exit, paths)
        let data = [
            json_line(100, r#""cmd":"ls /tmp""#),
            json_line(100, r#""exit":0"#),
            json_line(100, r#""paths":["/tmp","/home"]"#),
        ]
        .join("\n");

        let history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 1);

        let item = history.items().next().unwrap();
        assert_item_eq(&item, 100, "ls /tmp", &["/tmp", "/home"]);

        // Lines written out of order - should be sorted correctly
        let data = [
            json_line(200, r#""exit":127"#),
            json_line(200, r#""cmd":"not_found""#),
            json_line(200, r#""paths":[]"#),
        ]
        .join("\n");

        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 200, "not_found", &[]);

        // Invalid JSON in middle line - should skip and continue
        let id50 = base64_encode_u64(50);
        let data = [
            json_line(50, r#""cmd":"test""#),
            format!(r#"{{"id":"{}","exit":INVALID}}"#, id50),
            json_line(50, r#""paths":["/valid"]"#),
        ]
        .join("\n");

        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 50, "test", &["/valid"]);
    }

    #[test]
    fn test_search_matches_decodes_escaped_cmd() {
        let data = json_line(42, r#""cmd":"say \"needle\" \u4f60\u597d","exit":0"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let mut decoder = HistoryItemDecoder::new();
        let query = SearchQuery::new(
            WString::from(r#"say "needle" 你好"#),
            SearchType::Exact,
            true,
        );

        assert!(history.search_matches_from_back(0, &query, &mut decoder));
        assert_eq!(
            decoder.item().contents,
            WString::from(r#"say "needle" 你好"#)
        );
        assert_eq!(decoder.item().exit_code, Some(0));
    }

    #[test]
    fn test_search_matches_finds_cmd_after_metadata_line() {
        let data = [
            json_line(100, r#""exit":0"#),
            json_line(100, r#""cmd":"needle later""#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let mut decoder = HistoryItemDecoder::new();
        let query = SearchQuery::new(WString::from("needle"), SearchType::Contains, true);

        assert!(history.search_matches_from_back(0, &query, &mut decoder));
        assert_eq!(decoder.item().contents, WString::from("needle later"));
        assert_eq!(decoder.item().exit_code, Some(0));
    }

    #[test]
    fn test_item_parsing_multiple_items() {
        // Multiple distinct items in the file
        let data = [
            json_line(1, r#""cmd":"first""#),
            json_line(2, r#""cmd":"second","exit":0"#),
            json_line(3, r#""cmd":"third""#),
            json_line(3, r#""paths":["/a","/b","/c"]"#),
        ]
        .join("\n");

        let history = HistoryFile::from_data(data.as_bytes(), None);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_eq!(items.len(), 3);

        assert_item_eq(&items[0], 1, "first", &[]);
        assert_item_eq(&items[1], 2, "second", &[]);
        assert_item_eq(&items[2], 3, "third", &["/a", "/b", "/c"]);

        // Realistic scenario: unsorted history with multiple items
        let data = [
            json_line(300, r#""cmd":"cd /tmp""#),
            json_line(100, r#""cmd":"echo start""#),
            json_line(200, r#""cmd":"ls -la""#),
            json_line(100, r#""exit":0"#),
            json_line(300, r#""exit":0"#),
            json_line(200, r#""exit":0"#),
            json_line(200, r#""paths":["/home/user"]"#),
            json_line(400, r#""cmd":"pwd""#),
        ]
        .join("\n");

        let history = HistoryFile::from_data(data.as_bytes(), None);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_eq!(items.len(), 4); // IDs: 100, 200, 300, 400

        assert_item_eq(&items[0], 100, "echo start", &[]);
        assert_item_eq(&items[1], 200, "ls -la", &["/home/user"]);
        assert_item_eq(&items[2], 300, "cd /tmp", &[]);
        assert_item_eq(&items[3], 400, "pwd", &[]);
    }

    #[test]
    fn test_full_history_parsing_integration() {
        // Integration test: parse a complete history file with various scenarios
        let max_id = 18446744073709551614u64;
        let data = [
            // Item 1000: command only
            json_line(1000, r#""cmd":"git status""#),
            // Item 2000: command + exit (written together)
            json_line(2000, r#""cmd":"cargo build","exit":0"#),
            // Item 3000: built up incrementally
            json_line(3000, r#""cmd":"find / -name '*.rs'""#),
            json_line(3000, r#""exit":1"#),
            json_line(3000, r#""paths":["/usr","/home"]"#),
            // Invalid line - should be skipped
            "not json at all".to_owned(),
            // Item 1500: appears later but ID is lower - will be sorted
            json_line(1500, r#""cmd":"inserted later""#),
            // More lines for item 3000 (out of order in file)
            json_line(3000, r#""extra":"ignored_field""#),
            // Item with very large ID
            json_line(max_id, r#""cmd":"max id""#),
        ]
        .join("\n");

        let history = HistoryFile::from_data(data.as_bytes(), None);
        let items: Vec<HistoryItem> = history.items().collect();

        // Should have 5 unique items: 1000, 1500, 2000, 3000, max
        assert_eq!(items.len(), 5);

        // Verify each item
        assert_item_eq(&items[0], 1000, "git status", &[]);
        assert_item_eq(&items[1], 1500, "inserted later", &[]);
        assert_item_eq(&items[2], 2000, "cargo build", &[]);
        assert_item_eq(&items[3], 3000, "find / -name '*.rs'", &["/usr", "/home"]);
        assert_item_eq(&items[4], max_id, "max id", &[]);
    }

    #[test]
    fn test_shrink_to_max_records() {
        // Shrink to 0 - should clear everything
        let data = [
            json_line(100, r#""cmd":"first""#),
            json_line(200, r#""cmd":"second""#),
            json_line(300, r#""cmd":"third""#),
        ]
        .join("\n");
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 3);
        history.shrink_to_max_records(0);
        assert_eq!(history.item_count(), 0);
        assert!(history.is_empty());

        // Shrink when already within limit - should be no-op
        let data = [
            json_line(100, r#""cmd":"first""#),
            json_line(200, r#""cmd":"second""#),
            json_line(300, r#""cmd":"third""#),
        ]
        .join("\n");
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        history.shrink_to_max_records(5);
        assert_eq!(history.item_count(), 3);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 100, "first", &[]);
        assert_item_eq(&items[1], 200, "second", &[]);
        assert_item_eq(&items[2], 300, "third", &[]);

        // Shrink to exact size - should be no-op
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        history.shrink_to_max_records(3);
        assert_eq!(history.item_count(), 3);

        // Shrink to 1 - keep only newest
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        history.shrink_to_max_records(1);
        assert_eq!(history.item_count(), 1);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 300, "third", &[]);

        // Shrink to 2 - keep two newest
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        history.shrink_to_max_records(2);
        assert_eq!(history.item_count(), 2);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 200, "second", &[]);
        assert_item_eq(&items[1], 300, "third", &[]);

        // Shrink with multi-line items
        let data = [
            json_line(100, r#""cmd":"first""#),
            json_line(100, r#""exit":0"#),
            json_line(200, r#""cmd":"second""#),
            json_line(200, r#""exit":1"#),
            json_line(200, r#""paths":["/tmp"]"#),
            json_line(300, r#""cmd":"third""#),
        ]
        .join("\n");
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 3);
        assert_eq!(history.line_count(), 6);

        history.shrink_to_max_records(2);
        assert_eq!(history.item_count(), 2);
        assert_eq!(history.line_count(), 4); // 3 lines for item 200 + 1 line for item 300

        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 200, "second", &["/tmp"]);
        assert_item_eq(&items[1], 300, "third", &[]);

        // Test get_from_back still works after shrinking
        assert_eq!(history.get_from_back(0).unwrap().id.raw(), 300);
        assert_eq!(history.get_from_back(1).unwrap().id.raw(), 200);
        assert!(history.get_from_back(2).is_none());
    }

    #[test]
    fn test_get_from_back() {
        let data = [
            json_line(100, r#""cmd":"first""#),
            json_line(200, r#""cmd":"second""#),
            json_line(300, r#""cmd":"third""#),
            json_line(400, r#""cmd":"fourth""#),
        ]
        .join("\n");
        let history = HistoryFile::from_data(data.as_bytes(), None);

        // Get items from most recent to oldest
        assert_item_eq(&history.get_from_back(0).unwrap(), 400, "fourth", &[]);
        assert_item_eq(&history.get_from_back(1).unwrap(), 300, "third", &[]);
        assert_item_eq(&history.get_from_back(2).unwrap(), 200, "second", &[]);
        assert_item_eq(&history.get_from_back(3).unwrap(), 100, "first", &[]);

        // Out of bounds returns None
        assert!(history.get_from_back(4).is_none());
        assert!(history.get_from_back(100).is_none());

        // Empty history
        let empty: HistoryFile<&[u8]> = HistoryFile::create_empty();
        assert!(empty.get_from_back(0).is_none());
    }

    #[test]
    fn test_from_data_with_cutoff() {
        use std::time::{Duration, SystemTime};

        // Create items with specific timestamps via their IDs
        // HistoryItemId encodes timestamp in the upper bits
        let ts_old = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let ts_middle = SystemTime::UNIX_EPOCH + Duration::from_secs(2000);
        let ts_new = SystemTime::UNIX_EPOCH + Duration::from_secs(3000);

        let id_old = HistoryItemId::new(ts_old, 0);
        let id_middle = HistoryItemId::new(ts_middle, 0);
        let id_new = HistoryItemId::new(ts_new, 0);

        let data = [
            json_line(id_old.raw(), r#""cmd":"old""#),
            json_line(id_middle.raw(), r#""cmd":"middle""#),
            json_line(id_new.raw(), r#""cmd":"new""#),
        ]
        .join("\n");

        // No cutoff - should get all 3 items
        let history = HistoryFile::from_data(data.as_bytes(), None);
        assert_eq!(history.item_count(), 3);

        // Cutoff at ts_middle - should exclude items newer than middle (i.e., exclude "new")
        let history = HistoryFile::from_data(data.as_bytes(), Some(ts_middle));
        assert_eq!(history.item_count(), 2);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_eq!(items[0].contents, WString::from("old"));
        assert_eq!(items[1].contents, WString::from("middle"));

        // Cutoff before all items - should get empty history
        let ts_very_old = SystemTime::UNIX_EPOCH + Duration::from_secs(500);
        let history = HistoryFile::from_data(data.as_bytes(), Some(ts_very_old));
        assert_eq!(history.item_count(), 0);

        // Cutoff after all items - should get all items
        let ts_future = SystemTime::UNIX_EPOCH + Duration::from_secs(10000);
        let history = HistoryFile::from_data(data.as_bytes(), Some(ts_future));
        assert_eq!(history.item_count(), 3);
    }
}

#[cfg(feature = "benchmark")]
#[cfg(test)]
mod bench {
    extern crate test;
    use super::*;
    use crate::history::{
        History, HistoryId, HistorySearch, SearchDirection, SearchFlags, SearchType,
    };
    use fish_widestring::wcs2osstring;
    use rand::prelude::IndexedRandom;
    use rand::rngs::StdRng;
    use rand::{RngExt, SeedableRng};
    use std::path::Path;
    use std::sync::Arc;
    use test::{Bencher, black_box};

    // Generate random text with a mix of ASCII and some Unicode characters.
    fn random_text(rng: &mut StdRng, len: usize) -> WString {
        const ASCII: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
        const UNICODE: [char; 6] = ['λ', 'ß', '中', '界', 'Ж', '🙂'];
        std::iter::repeat_with(|| {
            if rng.random_bool(1.0 / 16.0) {
                *UNICODE.choose(rng).unwrap()
            } else {
                *ASCII.choose(rng).unwrap() as char
            }
        })
        .take(len)
        .collect()
    }

    fn write_item_records(
        buffer: &mut Vec<u8>,
        id: HistoryItemId,
        rng: &mut StdRng,
        needle: Option<&wstr>,
    ) {
        let cmd_len = rng.random_range(8..96);
        let mut cmd = random_text(rng, cmd_len);
        if let Some(needle) = needle {
            cmd.push(' ');
            cmd.push_utfstr(needle);
        }
        let mut cmd_item = HistoryItem::with_id(id);
        cmd_item.contents = cmd;
        cmd_item.write_to(buffer).unwrap();

        let cwd = format!("/home/user/{}", random_text(rng, 8));
        let path1 = format!(
            "/home/user/{}/file{}",
            random_text(rng, 8),
            random_text(rng, 4)
        );
        let path2 = format!(
            "/home/user/{}/file{}",
            random_text(rng, 8),
            random_text(rng, 4)
        );

        let mut meta_item = HistoryItem::with_id(id);
        meta_item.exit_code = Some(rng.random_range(0..10));
        meta_item.duration = Some(rng.random_range(1000..10000));
        meta_item.cwd = Some(WString::from(cwd.as_str()));
        meta_item.session_id = Some(rng.random::<u64>() & ((1u64 << 48) - 1));
        meta_item.required_paths =
            vec![WString::from(path1.as_str()), WString::from(path2.as_str())];
        meta_item.write_to(buffer).unwrap();
    }

    // Generate a large in-memory history buffer for benchmarking using the real JSONL writer.
    // Simulates realistic history by interleaving records with the same ID:
    // first the command, then the exit status and other fields.
    fn generate_history_buffer(num_items: usize, needle: &wstr) -> Vec<u8> {
        let mut rng = StdRng::seed_from_u64(0x42);
        let mut buffer = Vec::new();
        for i in 0..num_items {
            let id = HistoryItemId::from_raw(1_000_000 + i as u64);
            let needle = if i == 0 { Some(needle) } else { None };
            write_item_records(&mut buffer, id, &mut rng, needle);
        }
        buffer
    }

    fn create_history_with_file(
        name: &wstr,
        hist_dir: &Path,
        item_count: usize,
        needle: &wstr,
    ) -> (Arc<History>, usize) {
        let buffer = generate_history_buffer(item_count, needle);
        let buffer_len = buffer.len();
        let mut filename = name.to_owned();
        filename.push_utfstr(L!("_history.jsonl"));
        let tmpfile = hist_dir.join(wcs2osstring(&filename));
        std::fs::write(&tmpfile, buffer).unwrap();
        let dir = WString::from_str(hist_dir.to_str().unwrap());
        (
            History::new_with_directory(
                HistoryId::Disk {
                    session_id: name.to_owned(),
                },
                Some(dir),
            ),
            buffer_len,
        )
    }

    #[bench]
    fn bench_parse_history_file_small(b: &mut Bencher) {
        let needle = WString::from("needle_token");
        let buffer = generate_history_buffer(10_000, &needle);
        b.bytes = buffer.len() as u64;
        b.iter(|| {
            let _history = HistoryFile::from_data(buffer.as_slice(), None);
        });
    }

    #[bench]
    fn bench_parse_history_file_large(b: &mut Bencher) {
        const ITEM_COUNT: usize = 1024 * 512;
        let needle = WString::from("needle_token");
        let buffer = generate_history_buffer(ITEM_COUNT, &needle);
        b.bytes = buffer.len() as u64;
        b.iter(|| {
            let _history = HistoryFile::from_data(buffer.as_slice(), None);
        });
    }

    #[bench]
    fn bench_search_history_oldest_match(b: &mut Bencher) {
        let hist_dir = fish_tempfile::new_dir().unwrap();

        const ITEM_COUNT: usize = 1024 * 64;
        let needle = WString::from("needle_token");
        let (history, file_size) = create_history_with_file(
            L!("bench_search_jsonl"),
            hist_dir.path(),
            ITEM_COUNT,
            &needle,
        );
        b.bytes = file_size as u64;
        let needle = WString::from("needle_token");
        b.iter(|| {
            let mut searcher = HistorySearch::new_with(
                Arc::clone(&history),
                needle.clone(),
                SearchType::Contains,
                SearchFlags::empty(),
                0,
            );
            let found = searcher.go_to_next_match(SearchDirection::Backward);
            black_box(found);
            if found {
                black_box(searcher.current_string());
            }
        });
    }

    #[bench]
    fn bench_write_history_items(b: &mut Bencher) {
        let needle = WString::from("needle_token");
        let buffer = generate_history_buffer(10_000, &needle);
        b.bytes = buffer.len() as u64;
        let items: Vec<HistoryItem> = HistoryFile::from_data(buffer.as_slice(), None)
            .items()
            .collect();

        b.iter(|| {
            let mut sink = std::io::sink();
            for item in &items {
                item.write_to(&mut sink).unwrap();
            }
            black_box(&mut sink);
        });
    }
}
