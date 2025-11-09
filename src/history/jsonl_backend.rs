//! Implementation of the jsonlines history file format.
//! See the internal docs fish-history-file-format.md for details.
use super::file::MmapRegion;
use super::history::{HistoryItem, HistoryItemId};
use crate::prelude::*;
use json::JsonValue;
use std::time::SystemTime;

// Convert a WString to and from UTF-8. Private-use-area characters are retained.
fn wstring_to_utf8(s: &WString) -> String {
    s.chars().collect()
}

fn utf8_to_wstring(s: &str) -> WString {
    s.chars().collect()
}

pub trait JsonObjectExt {
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
        obj["id"] = JsonValue::from(self.id.raw());

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
        obj.set_opt("sid", &self.session_id);
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
        if let Some(sid) = obj["sid"].as_u64() {
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
/// This looks specifically for a string prefix of the form `{ "id": 123456, ... `.
///
/// It does NOT perform full JSON parsing or validation - this is deferred until the history item is actually decoded.
/// That is, if this function returns Some(id), then it contains the history item id if the line is valid JSON.
///
/// This is a hot function since it's used for the initial history parse, at which point all we're concerned about is
/// the "id" field. Note fish controls the key output order: "id" is always first when fish writes the file (see to_json),
/// so use a mini custom parser for strings of the form `{ "id": 123456, ... }`.
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

    // Whitespace, initial brace, whitespace, "id" key with quotes, colon, whitespace.
    let mut i = 0usize;
    ws(&mut i);
    eat_lit(&mut i, b"{")?;
    ws(&mut i);
    eat_lit(&mut i, br#""id""#)?;
    ws(&mut i);
    eat_lit(&mut i, b":")?;
    ws(&mut i);

    // Require at least one digit.
    if !line.get(i)?.is_ascii_digit() {
        return None;
    }

    // Reject digits after leading zero.
    if matches!(line.get(i..i + 2), Some([b'0', b'0'..=b'9'])) {
        return None;
    }

    // Parse a sequence of digits.
    let mut id: u64 = 0;
    while let Some(&b) = line.get(i).filter(|b| b.is_ascii_digit()) {
        let digit = (b - b'0') as u64;
        id = id.checked_mul(10)?.checked_add(digit)?;
        i += 1;
    }
    Some(id)
}

/// Parse the ID field from a JSON line.
/// Returns None if the line is not valid JSON or lacks an "id" field.
pub fn id_for_json_line(line: &[u8]) -> Option<u64> {
    if let Some(id) = try_parse_id_fast(line) {
        return Some(id);
    }
    let json = parse_json(line)?;
    json["id"].as_u64()
}

#[cfg(test)]
mod tests {
    use super::{HistoryFile, id_for_json_line, iter_lines, read_line_at, try_parse_id_fast};
    use crate::history::history::HistoryItem;
    use crate::prelude::*;

    // Test helper: assert that a HistoryItem matches expected values
    fn assert_item_eq(
        item: &HistoryItem,

        id: u64,
        cmd: &str,
        exit: Option<i32>,
        paths: Option<Vec<&str>>,
    ) {
        assert_eq!(item.id.raw(), id, "ID mismatch");
        assert_eq!(item.contents, WString::from(cmd), "Command mismatch");
        assert_eq!(item.exit_code, exit, "Exit code mismatch");
        if let Some(expected_paths) = paths {
            let expected: Vec<WString> = expected_paths.iter().map(|s| (*s).into()).collect();
            assert_eq!(item.required_paths, expected, "Paths mismatch");
        } else {
            assert!(item.required_paths.is_empty(), "Paths mismatch");
        }
    }

    #[test]
    fn test_try_parse_id_fast() {
        // Valid: basic cases
        assert_eq!(try_parse_id_fast(br#"{"id":0}"#), Some(0));
        assert_eq!(try_parse_id_fast(br#"{"id":42}"#), Some(42));
        assert_eq!(
            try_parse_id_fast(br#"{"id":18446744073709551615}"#),
            Some(u64::MAX)
        );

        // Basic cases
        assert_eq!(try_parse_id_fast(br#"{ "id" : 123 }"#), Some(123));
        assert_eq!(try_parse_id_fast(br#"{  "id"  :  456  }"#), Some(456));
        assert_eq!(try_parse_id_fast(b"{\t\"id\"\t:\t789}"), Some(789));
        assert_eq!(try_parse_id_fast(b"{\n\"id\"\n:\n999}"), Some(999));
        assert_eq!(try_parse_id_fast(br#"   {"id":111}"#), Some(111));

        // Invalid JSON but valid prefix
        assert_eq!(try_parse_id_fast(br#"{"id":123"#), Some(123));
        assert_eq!(try_parse_id_fast(br#"{"id":123,"#), Some(123));
        assert_eq!(try_parse_id_fast(br#"{"id":456,"cmd""#), Some(456));
        assert_eq!(try_parse_id_fast(br#"{"id":789 garbage}"#), Some(789));
        assert_eq!(try_parse_id_fast(br#"{"id":999,"malformed":}"#), Some(999));
        assert_eq!(try_parse_id_fast(br#"{"id":111}}}}}"#), Some(111));
        assert_eq!(
            try_parse_id_fast(br#"{"id":222,"cmd":"test"}extra"#),
            Some(222)
        );
        assert_eq!(try_parse_id_fast(br#"{"id":12.34}"#), Some(12));

        // Invalid JSON
        assert_eq!(try_parse_id_fast(br#""id":123}"#), None);
        assert_eq!(try_parse_id_fast(br#"{id:123}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id" 123}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id":}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id": }"#), None);

        // id not first key
        assert_eq!(try_parse_id_fast(br#"{"sid":7,"id":123}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"cmd":"test","id":456}"#), None);

        // Invalid: wrong key
        assert_eq!(try_parse_id_fast(br#"{"ID":123}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"idd":123}"#), None);

        // Invalid: non-numeric values
        assert_eq!(try_parse_id_fast(br#"{"id":"123"}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id":true}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id":null}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id":-123}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id":+123}"#), None);

        // Invalid: array not object
        assert_eq!(try_parse_id_fast(br#"[{"id":123}]"#), None);

        // Invalid: leading zero violations
        assert_eq!(try_parse_id_fast(br#"{"id":00}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id":01}"#), None);
        assert_eq!(try_parse_id_fast(br#"{"id":0123}"#), None);

        // Invalid: empty/incomplete
        assert_eq!(try_parse_id_fast(b""), None);
        assert_eq!(try_parse_id_fast(b"   "), None);
        assert_eq!(try_parse_id_fast(b"{"), None);

        // Overflow
        assert_eq!(try_parse_id_fast(br#"{"id":18446744073709551616}"#), None);
        assert_eq!(
            try_parse_id_fast(br#"{"id":99999999999999999999999}"#),
            None
        );
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
        assert_eq!(id_for_json_line(br#"{"id":42,"cmd":"true"}"#), Some(42));
        assert_eq!(
            id_for_json_line(br#"{ "sid":7, "id" :   9001 , "cmd":"make" }"#),
            Some(9001)
        );
        assert_eq!(
            id_for_json_line(br#"{"cmd":"echo \"id\"=7","extra":"\"id\"=8"}"#),
            None
        );
        assert_eq!(
            id_for_json_line(br#"{"id":18446744073709551615,"cmd":"max"}"#),
            Some(u64::MAX)
        );
        assert_eq!(
            id_for_json_line(br#"{"id":18446744073709551616,"cmd":"overflow"}"#),
            None
        );
    }

    #[test]
    fn test_item_count() {
        // Empty
        let history: HistoryFile<&[u8]> = HistoryFile::create_empty();
        assert_eq!(history.item_count(), 0);

        // Single item
        let data = br#"{"id":100,"cmd":"echo hello"}"#;
        let history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 1);

        // Multiple unique items, one line each
        let data = concat!(
            r#"{"id":100,"cmd":"ls"}"#,
            "\n",
            r#"{"id":200,"cmd":"pwd"}"#,
            "\n",
            r#"{"id":300,"cmd":"cd"}"#
        );
        let history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 3);

        // Item with multiple lines
        let data = concat!(
            r#"{"id":100,"cmd":"ls"}"#,
            "\n",
            r#"{"id":100,"exit":0}"#,
            "\n",
            r#"{"id":100,"paths":["/tmp"]}"#
        );
        let history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 1);

        // Mix of single-line and multi-line items
        let data = concat!(
            r#"{"id":100,"cmd":"ls"}"#,
            "\n",
            r#"{"id":100,"exit":0}"#,
            "\n",
            r#"{"id":200,"cmd":"pwd"}"#,
            "\n",
            r#"{"id":300,"cmd":"cd"}"#,
            "\n",
            r#"{"id":300,"exit":1}"#,
            "\n",
            r#"{"id":300,"paths":["/home"]}"#
        );
        let history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 3);

        // Unsorted input - should be sorted by from_data
        let data = concat!(
            r#"{"id":300,"cmd":"cd"}"#,
            "\n",
            r#"{"id":100,"cmd":"ls"}"#,
            "\n",
            r#"{"id":200,"cmd":"pwd"}"#,
            "\n",
            r#"{"id":100,"exit":0}"#
        );
        let history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 3);
    }

    #[test]
    fn test_item_parsing_single_items() {
        // Simple item with just a command
        let data = br#"{"id":42,"cmd":"echo hello"}"#;
        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 42, "echo hello", None, None);

        // Single line with all fields
        let data = br#"{"id":999,"cmd":"git commit","exit":1,"paths":["/repo/.git"]}"#;
        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 999, "git commit", Some(1), Some(vec!["/repo/.git"]));

        // Empty command (auxiliary item)
        let data = br#"{"id":77,"exit":0}"#;
        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 77, "", Some(0), None);

        // Unicode command
        let data = br#"{"id":888,"cmd":"echo \u4f60\u597d"}"#;
        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 888, "echo 你好", None, None);

        // Negative exit code (signal)
        let data = br#"{"id":555,"cmd":"killed","exit":-9}"#;
        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 555, "killed", Some(-9), None);

        // Empty paths array
        let data = br#"{"id":666,"cmd":"test","paths":[]}"#;
        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 666, "test", None, Some(vec![]));
    }

    #[test]
    fn test_item_parsing_multiple_lines() {
        // Item split across multiple lines (command, exit, paths)
        let data = concat!(
            r#"{"id":100,"cmd":"ls /tmp"}"#,
            "\n",
            r#"{"id":100,"exit":0}"#,
            "\n",
            r#"{"id":100,"paths":["/tmp","/home"]}"#
        );

        let history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 1);

        let item = history.items().next().unwrap();
        assert_item_eq(&item, 100, "ls /tmp", Some(0), Some(vec!["/tmp", "/home"]));

        // Lines written out of order - should be sorted correctly
        let data = concat!(
            r#"{"id":200,"exit":127}"#,
            "\n",
            r#"{"id":200,"cmd":"not_found"}"#,
            "\n",
            r#"{"id":200,"paths":[]}"#
        );

        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 200, "not_found", Some(127), Some(vec![]));

        // Invalid JSON in middle line - should skip and continue
        let data = concat!(
            r#"{"id":50,"cmd":"test"}"#,
            "\n",
            r#"{"id":50,"exit":INVALID}"#,
            "\n",
            r#"{"id":50,"paths":["/valid"]}"#
        );

        let history = HistoryFile::from_data(data, None);
        let item = history.items().next().unwrap();
        assert_item_eq(&item, 50, "test", None, Some(vec!["/valid"]));
    }

    #[test]
    fn test_item_parsing_multiple_items() {
        // Multiple distinct items in the file
        let data = concat!(
            r#"{"id":1,"cmd":"first"}"#,
            "\n",
            r#"{"id":2,"cmd":"second","exit":0}"#,
            "\n",
            r#"{"id":3,"cmd":"third"}"#,
            "\n",
            r#"{"id":3,"paths":["/a","/b","/c"]}"#
        );

        let history = HistoryFile::from_data(data, None);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_eq!(items.len(), 3);

        assert_item_eq(&items[0], 1, "first", None, None);
        assert_item_eq(&items[1], 2, "second", Some(0), None);
        assert_item_eq(&items[2], 3, "third", None, Some(vec!["/a", "/b", "/c"]));

        // Realistic scenario: unsorted history with multiple items
        let data = concat!(
            r#"{"id":300,"cmd":"cd /tmp"}"#,
            "\n",
            r#"{"id":100,"cmd":"echo start"}"#,
            "\n",
            r#"{"id":200,"cmd":"ls -la"}"#,
            "\n",
            r#"{"id":100,"exit":0}"#,
            "\n",
            r#"{"id":300,"exit":0}"#,
            "\n",
            r#"{"id":200,"exit":0}"#,
            "\n",
            r#"{"id":200,"paths":["/home/user"]}"#,
            "\n",
            r#"{"id":400,"cmd":"pwd"}"#
        );

        let history = HistoryFile::from_data(data, None);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_eq!(items.len(), 4); // IDs: 100, 200, 300, 400

        assert_item_eq(&items[0], 100, "echo start", Some(0), None);
        assert_item_eq(&items[1], 200, "ls -la", Some(0), Some(vec!["/home/user"]));
        assert_item_eq(&items[2], 300, "cd /tmp", Some(0), None);
        assert_item_eq(&items[3], 400, "pwd", None, None);
    }

    #[test]
    fn test_full_history_parsing_integration() {
        // Integration test: parse a complete history file with various scenarios
        let data = concat!(
            // Item 1000: command only
            r#"{"id":1000,"cmd":"git status"}"#,
            "\n",
            // Item 2000: command + exit (written together)
            r#"{"id":2000,"cmd":"cargo build","exit":0}"#,
            "\n",
            // Item 3000: built up incrementally
            r#"{"id":3000,"cmd":"find / -name '*.rs'"}"#,
            "\n",
            r#"{"id":3000,"exit":1}"#,
            "\n",
            r#"{"id":3000,"paths":["/usr","/home"]}"#,
            "\n",
            // Invalid line - should be skipped
            "not json at all\n",
            // Item 1500: appears later but ID is lower - will be sorted
            r#"{"id":1500,"cmd":"inserted later"}"#,
            "\n",
            // More lines for item 3000 (out of order in file)
            r#"{"id":3000,"extra":"ignored_field"}"#,
            "\n",
            // Item with very large ID
            r#"{"id":18446744073709551614,"cmd":"max id"}"#
        );

        let history = HistoryFile::from_data(data, None);
        let items: Vec<HistoryItem> = history.items().collect();

        // Should have 5 unique items: 1000, 1500, 2000, 3000, max
        assert_eq!(items.len(), 5);

        // Verify each item
        assert_item_eq(&items[0], 1000, "git status", None, None);
        assert_item_eq(&items[1], 1500, "inserted later", None, None);
        assert_item_eq(&items[2], 2000, "cargo build", Some(0), None);
        assert_item_eq(
            &items[3],
            3000,
            "find / -name '*.rs'",
            Some(1),
            Some(vec!["/usr", "/home"]),
        );
        assert_item_eq(&items[4], 18446744073709551614, "max id", None, None);
    }

    #[test]
    fn test_shrink_to_max_records() {
        // Shrink to 0 - should clear everything
        let data = concat!(
            r#"{"id":100,"cmd":"first"}"#,
            "\n",
            r#"{"id":200,"cmd":"second"}"#,
            "\n",
            r#"{"id":300,"cmd":"third"}"#
        );
        let mut history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 3);
        history.shrink_to_max_records(0);
        assert_eq!(history.item_count(), 0);
        assert!(history.is_empty());

        // Shrink when already within limit - should be no-op
        let data = concat!(
            r#"{"id":100,"cmd":"first"}"#,
            "\n",
            r#"{"id":200,"cmd":"second"}"#,
            "\n",
            r#"{"id":300,"cmd":"third"}"#
        );
        let mut history = HistoryFile::from_data(data, None);
        history.shrink_to_max_records(5);
        assert_eq!(history.item_count(), 3);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 100, "first", None, None);
        assert_item_eq(&items[1], 200, "second", None, None);
        assert_item_eq(&items[2], 300, "third", None, None);

        // Shrink to exact size - should be no-op
        let mut history = HistoryFile::from_data(data, None);
        history.shrink_to_max_records(3);
        assert_eq!(history.item_count(), 3);

        // Shrink to 1 - keep only newest
        let mut history = HistoryFile::from_data(data, None);
        history.shrink_to_max_records(1);
        assert_eq!(history.item_count(), 1);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 300, "third", None, None);

        // Shrink to 2 - keep two newest
        let mut history = HistoryFile::from_data(data, None);
        history.shrink_to_max_records(2);
        assert_eq!(history.item_count(), 2);
        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 200, "second", None, None);
        assert_item_eq(&items[1], 300, "third", None, None);

        // Shrink with multi-line items
        let data = concat!(
            r#"{"id":100,"cmd":"first"}"#,
            "\n",
            r#"{"id":100,"exit":0}"#,
            "\n",
            r#"{"id":200,"cmd":"second"}"#,
            "\n",
            r#"{"id":200,"exit":1}"#,
            "\n",
            r#"{"id":200,"paths":["/tmp"]}"#,
            "\n",
            r#"{"id":300,"cmd":"third"}"#
        );
        let mut history = HistoryFile::from_data(data, None);
        assert_eq!(history.item_count(), 3);
        assert_eq!(history.line_count(), 6);

        history.shrink_to_max_records(2);
        assert_eq!(history.item_count(), 2);
        assert_eq!(history.line_count(), 4); // 3 lines for item 200 + 1 line for item 300

        let items: Vec<HistoryItem> = history.items().collect();
        assert_item_eq(&items[0], 200, "second", Some(1), Some(vec!["/tmp"]));
        assert_item_eq(&items[1], 300, "third", None, None);

        // Test get_from_back still works after shrinking
        assert_eq!(history.get_from_back(0).unwrap().id.raw(), 300);
        assert_eq!(history.get_from_back(1).unwrap().id.raw(), 200);
        assert!(history.get_from_back(2).is_none());
    }
}

#[cfg(feature = "benchmark")]
#[cfg(test)]
mod bench {
    extern crate test;
    use super::*;
    use test::Bencher;

    // Generate a large in-memory history buffer for benchmarking.
    // Simulates realistic history by interleaving records with the same ID:
    // first the command, then the exit status and other fields.
    fn generate_history_buffer(num_items: usize) -> Vec<u8> {
        let mut buffer = Vec::new();
        for i in 0..num_items {
            let id = 1000000 + i;
            // First record: just the command when it starts
            let cmd_record = format!(r#"{{"id":{},"cmd":"echo test command number {}"}}"#, id, i);
            buffer.extend_from_slice(cmd_record.as_bytes());
            buffer.push(b'\n');

            // Second record: exit status and metadata when it finishes
            let meta_record = format!(
                r#"{{"id":{},"exit":0,"dur":{},"cwd":"/home/user/test","sid":{}}}"#,
                id,
                100 + (i % 1000),
                1000 + (i % 100)
            );
            buffer.extend_from_slice(meta_record.as_bytes());
            buffer.push(b'\n');
        }
        buffer
    }

    #[bench]
    fn bench_parse_history(b: &mut Bencher) {
        let buffer = generate_history_buffer(10_000);
        b.bytes = buffer.len() as u64;
        b.iter(|| {
            let _history = HistoryFile::from_data(buffer.as_slice(), None);
        });
    }
}
