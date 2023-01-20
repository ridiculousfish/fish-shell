use crate::args::{Arg, ArgList};
use crate::locale::{Locale, C_LOCALE};
use crate::output::wide_write;
use crate::{wstr, WString};

/// The sprintf function entry points. Prefer to use the macros below.
pub fn sprintf_locale<'a>(fmt: &wstr, locale: &Locale, args: &[Arg<'a>]) -> WString {
    let mut s = WString::new();
    let mut args = ArgList::new(args);
    let res = crate::parser::format(fmt, &mut args, wide_write(&mut s, &locale));
    if !res.is_ok() {
        panic!("Format string panicked: {}", fmt);
    }
    if args.remaining() > 0 {
        panic!(
            "sprintf had {} unconsumed args for format string: {}",
            args.remaining(),
            fmt
        );
    }
    s
}

pub fn sprintf_c_locale<'a>(fmt: &wstr, args: &[Arg<'a>]) -> WString {
    sprintf_locale(fmt, &C_LOCALE, args)
}

/// The basic entry point. Accepts a format string as a &wstr, and a list of arguments.
#[macro_export]
macro_rules! sprintf {
    // Variant which allows a string literal.
    (
        $fmt:literal, // format string
        $($arg:expr),* // arguments
        $(,)? // optional trailing comma
    ) => {
        $crate::printf::sprintf_c_locale(
            widestring::utf32str!($fmt),
            &[$($crate::args::ToArg::to_arg($arg)),*]
        )
    };

    // Variant which allows a runtime format string, which must be of type &wstr.
    (
        $fmt:expr, // format string
        $($arg:expr),* // arguments
        $(,)? // optional trailing comma
    ) => {
        $crate::printf::sprintf_c_locale(
            $fmt,
            &[$($crate::args::ToArg::to_arg($arg)),*]
        )
    };
}

#[cfg(test)]
mod tests {
    use widestring::utf32str;

    // Test basic sprintf with both literals and wide strings.
    #[test]
    fn test_sprintf() {
        assert_eq!(sprintf!("Hello, %s!", "world"), "Hello, world!");
        assert_eq!(sprintf!(utf32str!("Hello, %ls!"), "world"), "Hello, world!");
        assert_eq!(
            sprintf!(utf32str!("Hello, %ls!"), utf32str!("world")),
            "Hello, world!"
        );
    }
}
