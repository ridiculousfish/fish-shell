use super::{ArgList, Argument, DoubleFormat, Flags, SignedInt, Specifier, UnsignedInt};
use crate::wstr;
use itertools::Itertools;
use std::fmt;

fn next_char(sub: &[char]) -> &[char] {
    sub.get(1..).unwrap_or(&[])
}

/// Parse the [Flags field](https://en.wikipedia.org/wiki/Printf_format_string#Flags_field).
fn parse_flags(mut sub: &[char]) -> (Flags, &[char]) {
    let mut flags: Flags = Flags::empty();
    while let Some(&ch) = sub.get(0) {
        flags.insert(match ch {
            '-' => Flags::LEFT_ALIGN,
            '+' => Flags::PREPEND_PLUS,
            ' ' => Flags::PREPEND_SPACE,
            '0' => Flags::PREPEND_ZERO,
            '\'' => Flags::THOUSANDS_GROUPING,
            '#' => Flags::ALTERNATE_FORM,
            _ => break,
        });
        sub = next_char(sub)
    }
    (flags, sub)
}

/// Parse the [Width field](https://en.wikipedia.org/wiki/Printf_format_string#Width_field).
fn parse_width<'a>(mut sub: &'a [char], args: &mut ArgList) -> (u64, &'a [char]) {
    let mut width: u64 = 0;
    if sub.get(0) == Some(&'*') {
        return (args.arg_u64(), next_char(sub));
    }
    while let Some(&ch) = sub.get(0) {
        match ch {
            // https://rust-malaysia.github.io/code/2020/07/11/faster-integer-parsing.html#the-bytes-solution
            '0'..='9' => width = width * 10 + ((ch as u64) & 0x0f),
            _ => break,
        }
        sub = next_char(sub);
    }
    (width, sub)
}

/// Parse the [Precision field](https://en.wikipedia.org/wiki/Printf_format_string#Precision_field).
fn parse_precision<'a>(sub: &'a [char], args: &mut ArgList) -> (Option<u64>, &'a [char]) {
    match sub.get(0) {
        Some(&'.') => {
            let (prec, sub) = parse_width(next_char(sub), args);
            (Some(prec), sub)
        }
        _ => (None, sub),
    }
}

#[derive(Debug, Copy, Clone)]
enum Length {
    Int,
    /// `hh`
    Char,
    /// `h`
    Short,
    /// `l`
    Long,
    /// `ll`
    LongLong,
    /// `z`
    Usize,
    /// `t`
    Isize,
}

impl Length {
    fn parse_signed(self, args: &mut ArgList) -> SignedInt {
        match self {
            Length::Int => SignedInt::Int(args.arg_i32()),
            Length::Char => SignedInt::Char(args.arg_i8()),
            Length::Short => SignedInt::Short(args.arg_i16()),
            Length::Long => SignedInt::Long(args.arg_i64()),
            Length::LongLong => SignedInt::LongLong(args.arg_i64()),
            // for some reason, these exist as different options, yet produce the same output
            Length::Usize | Length::Isize => SignedInt::Isize(args.arg_i64()),
        }
    }

    fn parse_unsigned(self, args: &mut ArgList) -> UnsignedInt {
        match self {
            Length::Int => UnsignedInt::Int(args.arg_u32()),
            Length::Char => UnsignedInt::Char(args.arg_u8()),
            Length::Short => UnsignedInt::Short(args.arg_u16()),
            Length::Long => UnsignedInt::Long(args.arg_u64()),
            Length::LongLong => UnsignedInt::LongLong(args.arg_u64()),
            // for some reason, these exist as different options, yet produce the same output
            Length::Usize | Length::Isize => UnsignedInt::Isize(args.arg_u64()),
        }
    }
}

/// Parse the [Length field](https://en.wikipedia.org/wiki/Printf_format_string#Length_field).
fn parse_length(sub: &[char]) -> (Length, &[char]) {
    match sub.get(0).copied() {
        Some('h') => match sub.get(1).copied() {
            Some('h') => (Length::Char, sub.get(2..).unwrap_or(&[])),
            _ => (Length::Short, next_char(sub)),
        },
        Some('l') => match sub.get(1).copied() {
            Some('l') => (Length::LongLong, sub.get(2..).unwrap_or(&[])),
            _ => (Length::Long, next_char(sub)),
        },
        Some('z') => (Length::Usize, next_char(sub)),
        Some('t') => (Length::Isize, next_char(sub)),
        _ => (Length::Int, sub),
    }
}

/// Parse a format parameter and write it somewhere.
pub fn format<'a, 'b>(
    format: &'a wstr,
    args: &mut ArgList<'b>,
    mut handler: impl FnMut(Argument) -> fmt::Result,
) -> fmt::Result {
    let mut iter = format.as_char_slice().split(|&c| c == '%');

    if let Some(begin) = iter.next() {
        handler(Specifier::Literals(begin).into())?;
    }
    let mut last_was_percent = false;
    for (sub, next) in iter.map(Some).chain(core::iter::once(None)).tuple_windows() {
        let sub = match sub {
            Some(sub) => sub,
            None => break,
        };
        if last_was_percent {
            handler(Specifier::Literals(sub).into())?;
            last_was_percent = false;
            continue;
        }
        let (flags, sub) = parse_flags(sub);
        let (width, sub) = parse_width(sub, args);
        let (precision, sub) = parse_precision(sub, args);
        let (length, sub) = parse_length(sub);
        let ch = sub
            .get(0)
            .unwrap_or(if next.is_some() { &'%' } else { &'\0' });
        handler(Argument {
            flags,
            width,
            precision,
            specifier: match ch {
                '%' => {
                    last_was_percent = true;
                    Specifier::Percent
                }
                'd' | 'i' => Specifier::Int(length.parse_signed(args)),
                'x' => Specifier::Hex(length.parse_unsigned(args)),
                'X' => Specifier::UpperHex(length.parse_unsigned(args)),
                'u' => Specifier::Uint(length.parse_unsigned(args)),
                'o' => Specifier::Octal(length.parse_unsigned(args)),
                'f' | 'F' => Specifier::Double {
                    value: args.arg_f64(),
                    format: DoubleFormat::Normal.set_upper(ch.is_ascii_uppercase()),
                },
                'e' | 'E' => Specifier::Double {
                    value: args.arg_f64(),
                    format: DoubleFormat::Scientific.set_upper(ch.is_ascii_uppercase()),
                },
                'g' | 'G' => Specifier::Double {
                    value: args.arg_f64(),
                    format: DoubleFormat::Auto.set_upper(ch.is_ascii_uppercase()),
                },
                'a' | 'A' => Specifier::Double {
                    value: args.arg_f64(),
                    format: DoubleFormat::Hex.set_upper(ch.is_ascii_uppercase()),
                },
                's' => Specifier::String(args.arg_str()),
                'c' => Specifier::Char(args.arg_c()),
                'p' => Specifier::Pointer(args.arg_p()),
                //'n' => Specifier::WriteBytesWritten(written, args.arg()),
                _ => return Result::Err(fmt::Error),
            },
        })?;
        handler(Specifier::Literals(next_char(sub)).into())?;
    }
    Result::Ok(())
}
