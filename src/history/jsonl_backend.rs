//! Implementation of the jsonlines history file format.
//! See the internal docs fish-history-file-format.md for details.
use super::file::MmapRegion;
use super::history::{HistoryItem, HistoryItemId};
use crate::prelude::*;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use json::JsonValue;
use std::time::SystemTime;

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

pub trait JsonObjectExt {
    #[allow(clippy::ref_option)]
    fn set_opt<T>(&mut self, key: &str, value: &Option<T>)
    where
        T: Clone + Into<JsonValue>;
}

impl JsonObjectExt for JsonValue {
    fn set_opt<T>(&mut self, key: &str, value: &Option<T>)
    where
        T: Clone + Into<JsonValue>,
    {
        if let Some(v) = value {
            self[key] = v.clone().into();
        }
    }
}

impl HistoryItem {
    /// Encode this item into a JSON object. Only includes fields that are present.
    /// For commands, "empty" means missing.
    pub(super) fn to_json(&self) -> JsonValue {
        let mut obj = JsonValue::new_object();
        obj["id"] = JsonValue::String(base64_encode_u64(self.id.raw()));

        if !self.contents.is_empty() {
            obj["cmd"] = JsonValue::String(wstring_to_utf8(&self.contents));
        }
        if !self.required_paths.is_empty() {
            let arr: Vec<JsonValue> = self
                .required_paths
                .iter()
                .map(|p| JsonValue::String(wstring_to_utf8(p)))
                .collect();
            obj["paths"] = json::JsonValue::Array(arr);
        }
        if let Some(cwd) = &self.cwd {
            obj["cwd"] = json::JsonValue::String(wstring_to_utf8(cwd));
        }
        obj.set_opt("exit", &self.exit_code);
        obj.set_opt("dur", &self.duration);
        if let Some(sid) = self.session_id {
            obj["sid"] = JsonValue::String(base64_encode_u64(sid));
        }
        obj
    }

    /// Encode this item as a JSON line string, with a trailing newline.
    pub(super) fn to_json_line(&self) -> String {
        let mut s = self.to_json().dump();
        s.push('\n');
        s
    }

    /// Add additional fields to this item from a JSON object.
    pub(super) fn annotate_from_json(&mut self, obj: &json::JsonValue) {
        if let Some(cmd) = obj["cmd"].as_str() {
            self.contents = utf8_to_wstring(cmd);
        }
        if let Some(exit) = obj["exit"].as_i32() {
            self.exit_code = Some(exit);
        }
        if let json::JsonValue::Array(array) = &obj["paths"] {
            self.required_paths = array
                .iter()
                .filter_map(|entry| entry.as_str())
                .map(utf8_to_wstring)
                .collect();
        }
        if let Some(dur) = obj["dur"].as_u64() {
            self.duration = Some(dur);
        }
        if let Some(cwd) = obj["cwd"].as_str() {
            self.cwd = Some(utf8_to_wstring(cwd));
        }
        if let Some(sid) = obj["sid"]
            .as_str()
            .and_then(|s| base64_decode_u64(s.as_bytes()))
        {
            self.session_id = Some(sid);
        }
    }

    /// Append this history item to a buffer in JSON lines format.
    pub(super) fn write_to(&self, buffer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.to_json().write(buffer)?;
        buffer.write_all(b"\n")?;
        Ok(())
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

    /// Return an iterator over all history items in the file.
    pub(super) fn items(
        &self,
    ) -> impl DoubleEndedIterator<Item = HistoryItem> + ExactSizeIterator + '_ {
        self.item_starts.iter().map(|&start| self.item_at(start))
    }

    /// Get an item by reverse index. Index 0 is the most recent item, 1 is second-most recent, etc.
    pub(super) fn get_from_back(&self, idx: usize) -> Option<HistoryItem> {
        if idx >= self.item_starts.len() {
            return None;
        }
        let start = self.item_starts[self.item_starts.len() - idx - 1];
        Some(self.item_at(start))
    }

    /// Return the history item at the given start position. This walks over the contiguous lines with the same ID.
    /// Items may fail to decode (e.g. if the JSON is invalid), in which case None is returned.
    fn item_at(&self, start: usize) -> HistoryItem {
        let mut idx = start;
        let line_offset = self.line_offsets[idx];
        // We should always have backing data if we have item_starts.
        let data = self.backing.as_ref().unwrap().as_ref();

        // Construct an empty item.
        let mut item = HistoryItem::with_id(line_offset.id);
        while idx < self.line_offsets.len() && self.line_offsets[idx].id == line_offset.id {
            let line_offset = &self.line_offsets[idx];
            let (line, _) = read_line_at(data, line_offset.offset);
            if let Some(json) = parse_json(line) {
                item.annotate_from_json(&json);
            }
            idx += 1;
        }
        item
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

/// Read a single line from the buffer starting at the given offset.
/// Returns the line (without newline) and the offset of the next line, or None if this is the last line.
fn read_line_at(buf: &[u8], line_start: usize) -> (&[u8], Option<usize>) {
    let Some(remaining) = buf.get(line_start..) else {
        return (&[], None);
    };
    let line_len = remaining
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(remaining.len());
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

// Parse a JSON line into a JsonValue, returning None on failure.
fn parse_json(buf: &[u8]) -> Option<json::JsonValue> {
    let s = std::str::from_utf8(buf).ok()?;
    json::parse(s).ok()
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
    let json = parse_json(line)?;
    let s = json["id"].as_str()?;
    base64_decode_u64(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryFile, base64_decode_u64, base64_encode_u64, id_for_json_line, iter_lines,
        read_line_at, try_parse_id_fast,
    };
    use crate::history::history::{HistoryItem, HistoryItemId};
    use crate::prelude::*;

    // Test helper: assert that a HistoryItem matches expected values
    fn assert_item_eq(item: &HistoryItem, id: u64, cmd: &str, exit: Option<i32>, paths: &[&str]) {
        assert_eq!(item.id.raw(), id, "ID mismatch");
        assert_eq!(item.contents, WString::from(cmd), "Command mismatch");
        assert_eq!(item.exit_code, exit, "Exit code mismatch");
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
    fn test_item_parsing_single_items() {
        // Simple item with just a command
        let data = json_line(42, r#""cmd":"echo hello""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 42, "echo hello", None, &[]);

        // Single line with all fields
        let data = json_line(999, r#""cmd":"git commit","exit":1,"paths":["/repo/.git"]"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 999, "git commit", Some(1), &["/repo/.git"]);

        // Empty command (auxiliary item)
        let data = json_line(77, r#""exit":0"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 77, "", Some(0), &[]);

        // Unicode command
        let data = json_line(888, r#""cmd":"echo \u4f60\u597d""#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 888, "echo 你好", None, &[]);

        // Negative exit code (signal)
        let data = json_line(555, r#""cmd":"killed","exit":-9"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 555, "killed", Some(-9), &[]);

        // Empty paths array
        let data = json_line(666, r#""cmd":"test","paths":[]"#);
        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 666, "test", None, &[]);
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
        assert_item_eq(&item, 100, "ls /tmp", Some(0), &["/tmp", "/home"]);

        // Lines written out of order - should be sorted correctly
        let data = [
            json_line(200, r#""exit":127"#),
            json_line(200, r#""cmd":"not_found""#),
            json_line(200, r#""paths":[]"#),
        ]
        .join("\n");

        let history = HistoryFile::from_data(data.as_bytes(), None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 200, "not_found", Some(127), &[]);

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
        assert_item_eq(&item, 50, "test", None, &["/valid"]);
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

        assert_item_eq(&items[0], 1, "first", None, &[]);
        assert_item_eq(&items[1], 2, "second", Some(0), &[]);
        assert_item_eq(&items[2], 3, "third", None, &["/a", "/b", "/c"]);

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

        assert_item_eq(&items[0], 100, "echo start", Some(0), &[]);
        assert_item_eq(&items[1], 200, "ls -la", Some(0), &["/home/user"]);
        assert_item_eq(&items[2], 300, "cd /tmp", Some(0), &[]);
        assert_item_eq(&items[3], 400, "pwd", None, &[]);
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
            "not json at all".to_string(),
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
        assert_item_eq(&items[0], 1000, "git status", None, &[]);
        assert_item_eq(&items[1], 1500, "inserted later", None, &[]);
        assert_item_eq(&items[2], 2000, "cargo build", Some(0), &[]);
        assert_item_eq(
            &items[3],
            3000,
            "find / -name '*.rs'",
            Some(1),
            &["/usr", "/home"],
        );
        assert_item_eq(&items[4], max_id, "max id", None, &[]);
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
        assert_item_eq(&items[0], 100, "first", None, &[]);
        assert_item_eq(&items[1], 200, "second", None, &[]);
        assert_item_eq(&items[2], 300, "third", None, &[]);

        // Shrink to exact size - should be no-op
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        history.shrink_to_max_records(3);
        assert_eq!(history.item_count(), 3);

        // Shrink to 1 - keep only newest
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        history.shrink_to_max_records(1);
        assert_eq!(history.item_count(), 1);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 300, "third", None, &[]);

        // Shrink to 2 - keep two newest
        let mut history = HistoryFile::from_data(data.as_bytes(), None);
        history.shrink_to_max_records(2);
        assert_eq!(history.item_count(), 2);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 200, "second", None, &[]);
        assert_item_eq(&items[1], 300, "third", None, &[]);

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
        assert_item_eq(&items[0], 200, "second", Some(1), &["/tmp"]);
        assert_item_eq(&items[1], 300, "third", None, &[]);

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
        assert_item_eq(&history.get_from_back(0).unwrap(), 400, "fourth", None, &[]);
        assert_item_eq(&history.get_from_back(1).unwrap(), 300, "third", None, &[]);
        assert_item_eq(&history.get_from_back(2).unwrap(), 200, "second", None, &[]);
        assert_item_eq(&history.get_from_back(3).unwrap(), 100, "first", None, &[]);

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
