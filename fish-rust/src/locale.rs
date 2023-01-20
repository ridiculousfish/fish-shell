/// Support for locale stuff.
use libc;
use std::sync::Mutex;

/// The numeric locale. Note this is a pure value type.
#[derive(Debug, Clone, Copy)]
pub struct NumericLocale {
    /// The decimal point. Only single-char decimal points are supported.
    pub decimal_point: char,

    /// The thousands separator, or None if none.
    /// Note some obscure locales like it_IT.ISO8859-15 seem to have a multi-char thousands separator!
    /// We do not support that.
    pub thousands_sep: Option<char>,

    /// The grouping of digits.
    /// This is to be read from left to right.
    /// For example, the number 88888888888888 with a grouping of [2, 3, 4, 4]
    /// would produce the string "8,8888,8888,888,88".
    /// If 0, no grouping at all.
    pub grouping: [u8; 4],

    /// If true, the group is repeated.
    /// If false, there are no groups after the last.
    pub group_repeat: bool,
}

impl NumericLocale {
    /// \return an iterator over the number of digits in our groups.
    pub fn digit_group_iter(&self) -> GroupDigitIter {
        GroupDigitIter {
            next_group: 0,
            grouping: self.grouping,
            group_repeat: self.group_repeat,
        }
    }
}

/// Iterator over the digits in a group, starting from the right.
/// This never returns None and never returns 0.
pub struct GroupDigitIter {
    next_group: u8,
    grouping: [u8; 4],
    group_repeat: bool,
}

impl GroupDigitIter {
    pub fn next(&mut self) -> usize {
        let idx = self.next_group as usize;
        if idx < self.grouping.len() {
            self.next_group += 1;
        }
        let gc = if idx < self.grouping.len() {
            self.grouping[idx]
        } else if self.group_repeat {
            self.grouping[self.grouping.len() - 1]
        } else {
            0
        };
        if gc == 0 {
            // No grouping.
            usize::max_value()
        } else {
            gc as usize
        }
    }
}

/// The "C" numeric locale.
pub const C_LOCALE: NumericLocale = NumericLocale {
    decimal_point: '.',
    thousands_sep: None,
    grouping: [0; 4],
    group_repeat: false,
};

// en_us numeric locale, for testing.
pub const EN_US_LOCALE: NumericLocale = NumericLocale {
    decimal_point: '.',
    thousands_sep: Some(','),
    grouping: [3, 3, 3, 3],
    group_repeat: true,
};

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

/// Convert a libc lconv to a NumericLocale.
unsafe fn lconv_to_locale(lconv: &libc::lconv) -> NumericLocale {
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
    NumericLocale {
        decimal_point,
        thousands_sep,
        grouping,
        group_repeat,
    }
}

/// Read the numeric locale, or None on any failure.
unsafe fn read_locale() -> Option<NumericLocale> {
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
    static ref NUMERIC_LOCALE: Mutex<Option<NumericLocale>> = Mutex::new(None);
}

pub fn get_numeric_locale() -> NumericLocale {
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

#[test]
fn test_group_iter() {
    let mut loc = EN_US_LOCALE;
    let mut iter = loc.digit_group_iter();
    for _ in 0..100 {
        assert_eq!(iter.next(), 3);
    }

    loc.group_repeat = false;
    iter = loc.digit_group_iter();
    assert_eq!(
        [iter.next(), iter.next(), iter.next(), iter.next()],
        [3, 3, 3, 3]
    );
    for _ in 0..100 {
        assert_eq!(iter.next(), usize::max_value());
    }

    loc.grouping = [5, 3, 1, 0];
    iter = loc.digit_group_iter();
    assert_eq!(iter.next(), 5);
    assert_eq!(iter.next(), 3);
    assert_eq!(iter.next(), 1);
    for _ in 0..100 {
        assert_eq!(iter.next(), usize::max_value());
    }
}
