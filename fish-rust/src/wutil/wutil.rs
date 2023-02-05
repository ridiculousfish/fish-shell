use crate::wchar::wstr;

/// Given that \p cursor is a pointer into \p base, return the offset in characters.
/// This emulates C pointer arithmetic:
///    `wstr_offset_in(cursor, base)` is equivalent to C++ `cursor - base`.
pub fn wstr_offset_in(cursor: &wstr, base: &wstr) -> usize {
    let cursor = cursor.as_slice();
    let base = base.as_slice();
    // cursor may be a zero-length slice at the end of base,
    // which base.as_ptr_range().contains(cursor.as_ptr()) will reject.
    let base_range = base.as_ptr_range();
    let curs_range = cursor.as_ptr_range();
    assert!(
        base_range.start <= curs_range.start && curs_range.end <= base_range.end,
        "cursor should be a subslice of base"
    );
    let offset = unsafe { cursor.as_ptr().offset_from(base.as_ptr()) };
    assert!(offset >= 0, "offset should be non-negative");
    offset as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wchar::L;

    #[test]
    fn test_wstr_offset_in() {
        let base = L!("hello world");
        assert_eq!(wstr_offset_in(&base[6..], base), 6);
        assert_eq!(wstr_offset_in(&base[0..], base), 0);
        assert_eq!(wstr_offset_in(&base[6..], &base[6..]), 0);
        assert_eq!(wstr_offset_in(&base[base.len()..], base), base.len());
    }
}
