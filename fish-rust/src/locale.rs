/// Support for the "current locale."
use libc;
pub use printf_compat::locale::{Locale, C_LOCALE};
use std::sync::Mutex;

/// Rust libc does not provide LC_GLOBAL_LOCALE, but it appears to be -1 everywhere.
const LC_GLOBAL_LOCALE: libc::locale_t = (-1 as isize) as libc::locale_t;

/// It's CHAR_MAX.
const CHAR_MAX: libc::c_char = libc::c_char::max_value();

/// \return the first character of a C string, or None if null, empty, has a length more than 1, or negative.
unsafe fn first_char(s: *const libc::c_char) -> Option<char> {
    #[allow(unused_comparisons)]
    if !s.is_null() && *s > 0 && *s <= 127 && *s.offset(1) == 0 {
        Some((*s as u8) as char)
    } else {
        None
    }
}

/// Convert a libc lconv to a Locale.
unsafe fn lconv_to_locale(lconv: &libc::lconv) -> Locale {
    let decimal_point = first_char(lconv.decimal_point).unwrap_or('.');
    let thousands_sep = first_char(lconv.thousands_sep);
    let empty = &[0 as libc::c_char];

    // Up to 4 groups.
    // group_cursor is terminated by either a 0 or CHAR_MAX.
    let mut group_cursor = lconv.grouping as *const libc::c_char;
    if group_cursor.is_null() {
        group_cursor = empty.as_ptr();
    }

    let mut grouping = [0; 4];
    let mut last_group: u8 = 0;
    let mut group_repeat = false;
    for group in grouping.iter_mut() {
        let gc = *group_cursor;
        if gc == 0 {
            // Preserve last_group, do not advance cursor.
            group_repeat = true;
        } else if gc == CHAR_MAX {
            // Remaining groups are 0, do not advance cursor.
            last_group = 0;
            group_repeat = false;
        } else {
            // Record last group, advance cursor.
            last_group = gc as u8;
            group_cursor = group_cursor.offset(1);
        }
        *group = last_group;
    }
    Locale {
        decimal_point,
        thousands_sep,
        grouping,
        group_repeat,
    }
}

/// Read the numeric locale, or None on any failure.
unsafe fn read_locale() -> Option<Locale> {
    const empty: [libc::c_char; 1] = [0];
    let loc = libc::newlocale(libc::LC_NUMERIC_MASK, empty.as_ptr(), LC_GLOBAL_LOCALE);
    if loc.is_null() {
        return None;
    }
    let lconv = libc::localeconv_l(loc);
    let result = if lconv.is_null() {
        None
    } else {
        Some(lconv_to_locale(&*lconv))
    };
    libc::freelocale(loc);
    result
}

lazy_static! {
    // Current numeric locale.
    static ref NUMERIC_LOCALE: Mutex<Option<Locale>> = Mutex::new(None);
}

pub fn get_numeric_locale() -> Locale {
    let mut locale = NUMERIC_LOCALE.lock().unwrap();
    if locale.is_none() {
        let new_locale = (unsafe { read_locale() }).unwrap_or(C_LOCALE);
        *locale = Some(new_locale);
    }
    locale.unwrap()
}

/// Invalidate the cached numeric locale.
pub fn invalidate_numeric_locale() {
    *NUMERIC_LOCALE.lock().unwrap() = None;
}
