//! Support for reading legacy YAML-based history files (fish 2.0+ format).

use std::{
    borrow::Cow,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{HistoryItem, HistoryItemId};
use crate::{common::bytes2wcstring, flog::flog};

// Our YAML history format is nearly-valid YAML (but isn't quite). Here it is:
//
//   - cmd: ssh blah blah blah
//     when: 2348237
//     paths:
//       - /path/to/something
//       - /path/to/something_else
//
//   Newlines are replaced by \n. Backslashes are replaced by \\.

/// Read one line, stripping off any newline, returning the number of bytes consumed.
fn read_line(data: &[u8]) -> (usize, &[u8]) {
    // Locate the newline.
    if let Some(newline) = data.iter().position(|&c| c == b'\n') {
        // we found a newline
        let line = &data[..newline];
        // Return the amount to advance the cursor; skip over the newline.
        (newline + 1, line)
    } else {
        // We ran off the end.
        (data.len(), b"")
    }
}

#[inline(always)]
/// Unescapes the fish-specific yaml variant, if it requires it.
fn maybe_unescape_yaml_fish_2_0(s: &[u8]) -> Cow<'_, [u8]> {
    // This is faster than s.contains(b'\\') and can be auto-vectorized to SIMD. See benchmark note
    // on unescape_yaml_fish_2_0().
    if !s.iter().copied().fold(false, |acc, b| acc | (b == b'\\')) {
        return s.into();
    }
    unescape_yaml_fish_2_0(s).into()
}

// Unescapes the fish-specific yaml variant. Use [`maybe_unescape_yaml_fish_2_0()`] if you're not
// positive the input contains an escape.
pub fn unescape_yaml_fish_2_0(s: &[u8]) -> Vec<u8> {
    // This function is in a very hot loop and the usage of boxed uninit memory benchmarks around 8%
    // faster on real-world escaped yaml samples from the fish history file.

    // This is a very long way around of writing `Box::new_uninit_slice(s.len())`, which
    // requires the unstablized nightly-only feature new_unit (#63291). It optimizes away.
    let mut result: Box<[_]> = std::iter::repeat_with(std::mem::MaybeUninit::uninit)
        .take(s.len())
        .collect();
    let mut chars = s.iter().copied();
    let mut src_idx = 0;
    let mut dst_idx = 0;
    loop {
        // While inspecting the asm reveals the compiler does not elide the bounds check from
        // the writes to `result`, benchmarking shows that using `result.get_unchecked_mut()`
        // everywhere does not result in a statistically significant improvement to the
        // performance of this function.
        let to_copy = chars.by_ref().take_while(|b| *b != b'\\').count();
        unsafe {
            let src = s[src_idx..].as_ptr();
            // Can use the following when feature(maybe_uninit_slice) is stabilized:
            // let dst = std::mem::MaybeUninit::slice_as_mut_ptr(&mut result[dst_idx..]);
            let dst = result[dst_idx..].as_mut_ptr().cast();
            std::ptr::copy_nonoverlapping(src, dst, to_copy);
        }
        dst_idx += to_copy;

        match chars.next() {
            Some(b'\\') => result[dst_idx].write(b'\\'),
            Some(b'n') => result[dst_idx].write(b'\n'),
            _ => break,
        };
        src_idx += to_copy + 2;
        dst_idx += 1;
    }

    let result = Box::leak(result);
    unsafe { Vec::from_raw_parts(result.as_mut_ptr().cast(), dst_idx, result.len()) }
}

fn trim_start(s: &[u8]) -> &[u8] {
    &s[s.iter().take_while(|c| c.is_ascii_whitespace()).count()..]
}

/// Trims leading spaces in the given string, returning how many there were.
fn trim_leading_spaces(s: &[u8]) -> (usize, &[u8]) {
    let count = s.iter().take_while(|c| **c == b' ').count();
    (count, &s[count..])
}

#[inline(always)]
#[allow(clippy::type_complexity)]
pub fn extract_prefix_and_unescape_yaml(line: &[u8]) -> Option<(Cow<'_, [u8]>, Cow<'_, [u8]>)> {
    let mut split = line.splitn(2, |c| *c == b':');
    let key = split.next().unwrap();
    let value = split.next()?;
    debug_assert!(split.next().is_none());

    let key = maybe_unescape_yaml_fish_2_0(key);

    // Skip a space after the : if necessary.
    let value = trim_start(value);
    let value = maybe_unescape_yaml_fish_2_0(value);

    Some((key, value))
}

fn time_from_seconds(offset: i64) -> SystemTime {
    if let Ok(n) = u64::try_from(offset) {
        UNIX_EPOCH + Duration::from_secs(n)
    } else {
        UNIX_EPOCH - Duration::from_secs(offset.unsigned_abs())
    }
}

/// Decode an item via the fish 2.0 format.
/// History item IDs are constructed synthetically using the given nonce.
pub fn decode_item_fish_2_0(mut data: &[u8], nonce: u16) -> Option<HistoryItem> {
    let (advance, line) = read_line(data);
    let line = trim_start(line);
    if !line.starts_with(b"- cmd") {
        return None;
    }

    let (_key, value) = extract_prefix_and_unescape_yaml(line)?;

    data = &data[advance..];
    let cmd = bytes2wcstring(&value);

    // Read the remaining lines.
    let mut indent = None;
    let mut when = UNIX_EPOCH;
    let mut paths = Vec::new();
    loop {
        let (advance, line) = read_line(data);

        let (this_indent, line) = trim_leading_spaces(line);
        let indent = *indent.get_or_insert(this_indent);
        if this_indent == 0 || indent != this_indent {
            break;
        }

        let Some((key, value)) = extract_prefix_and_unescape_yaml(line) else {
            break;
        };

        // We are definitely going to consume this line.
        data = &data[advance..];

        if *key == *b"when" {
            // Parse an int from the timestamp. Should this fail, 0 is acceptable.
            when = time_from_seconds(
                std::str::from_utf8(&value)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            );
        } else if *key == *b"paths" {
            // Read lines starting with " - " until we can't read any more.
            loop {
                let (advance, line) = read_line(data);
                let (leading_spaces, line) = trim_leading_spaces(line);
                if leading_spaces <= indent {
                    break;
                }

                let Some(line) = line.strip_prefix(b"- ") else {
                    break;
                };

                // We're going to consume this line.
                data = &data[advance..];

                let line = maybe_unescape_yaml_fish_2_0(line);
                paths.push(bytes2wcstring(&line));
            }
        }
    }

    let id = HistoryItemId::new(when, nonce);
    let mut result = HistoryItem {
        contents: cmd,
        ..HistoryItem::with_id(id)
    };
    result.set_required_paths(paths);
    Some(result)
}

fn complete_lines(s: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut lines = s.split(|&c| c == b'\n');
    // Remove either the last empty element (in case last line is newline-terminated) or the
    // trailing non-newline-terminated line
    lines.next_back();
    lines
}

/// Support for iteratively locating the offsets of history items.
/// Pass the file contents and a mutable reference to a `cursor`, initially 0.
/// If `cutoff_timestamp` is given, skip items created at or after that timestamp.
/// Returns [`None`] when done.
fn offset_of_next_item_fish_2_0(contents: &[u8], cursor: &mut usize) -> Option<usize> {
    let mut lines = complete_lines(&contents[*cursor..]).peekable();
    while let Some(mut line) = lines.next() {
        // Skip lines with a leading space, since these are in the interior of one of our items.
        if line.starts_with(b" ") {
            continue;
        }

        // Try to be a little YAML compatible. Skip lines with leading %, ---, or ...
        if line.starts_with(b"%") || line.starts_with(b"---") || line.starts_with(b"...") {
            continue;
        }

        // Hackish: fish 1.x rewriting a fish 2.0 history file can produce lines with lots of
        // leading "- cmd: - cmd: - cmd:". Trim all but one leading "- cmd:".
        while line.starts_with(b"- cmd: - cmd: ") {
            // Skip over just one of the - cmd. In the end there will be just one left.
            line = line.strip_prefix(b"- cmd: ").unwrap();
        }

        // Hackish: fish 1.x rewriting a fish 2.0 history file can produce commands like "when:
        // 123456". Ignore those.
        if line.starts_with(b"- cmd:    when:") {
            continue;
        }

        if line.starts_with(b"\0") {
            flog!(
                error,
                "ignoring corrupted history entry around offset",
                *cursor
            );
            continue;
        }

        if !line.starts_with(b"- cmd") {
            flog!(
                history,
                "ignoring corrupted history entry around offset",
                *cursor
            );
            continue;
        }

        /// # Safety
        ///
        /// Both `from` and `to` must be derived from the same slice.
        unsafe fn offset(from: &[u8], to: &[u8]) -> usize {
            let from = from.as_ptr();
            let to = to.as_ptr();
            // SAFETY: from and to are derived from the same slice, slices can't be longer than
            // isize::MAX
            let offset = unsafe { to.offset_from(from) };
            offset.try_into().unwrap()
        }

        // Advance the cursor past the last line of this entry
        *cursor = match lines.next() {
            Some(next_line) => unsafe { offset(contents, next_line) },
            None => contents.len(),
        };

        return Some(unsafe { offset(contents, line) });
    }

    None
}

/// Iterate over all history items in the given fish 2.0+ history contents.
/// Item IDs are constructed synthetically.
pub fn iterate_fish_2_0_history(contents: &[u8]) -> impl Iterator<Item = HistoryItem> + '_ {
    let mut cursor: usize = 0;
    let mut nonce: u16 = 0;
    std::iter::from_fn(move || {
        while let Some(offset) = offset_of_next_item_fish_2_0(contents, &mut cursor) {
            if let Some(item) = decode_item_fish_2_0(&contents[offset..], nonce) {
                nonce = nonce.wrapping_add(1);
                return Some(item);
            }
        }
        None
    })
}
