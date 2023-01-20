/// The numeric locale. Note this is a pure value type.
#[derive(Debug, Clone, Copy)]
pub struct Locale {
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

impl Locale {
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
pub const C_LOCALE: Locale = Locale {
    decimal_point: '.',
    thousands_sep: None,
    grouping: [0; 4],
    group_repeat: false,
};

// en_us numeric locale, for testing.
pub const EN_US_LOCALE: Locale = Locale {
    decimal_point: '.',
    thousands_sep: Some(','),
    grouping: [3, 3, 3, 3],
    group_repeat: true,
};

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
