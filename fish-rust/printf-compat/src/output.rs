//! Various ways to output formatting data.

use core::fmt;

use super::{ArgList, Argument, DoubleFormat, Flags, Specifier};
use crate::{wstr, WString};
use std::fmt::Write;

/// Adapter for implementing `fmt::Write` for `WideWrite`, avoiding orphan rule.
pub struct WideWriteAdapt<'a, T: ?Sized>(&'a mut T);
impl<'a, T: WideWrite + ?Sized> fmt::Write for WideWriteAdapt<'a, T> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_str(s)
    }
}

/// The trait for receiving printf output.
pub trait WideWrite {
    /// Write a wstr.
    fn write_wstr(&mut self, s: &wstr) -> fmt::Result;

    /// Write a str.
    fn write_str(&mut self, s: &str) -> fmt::Result;

    /// Allows using write! macro.
    fn write_fmt(&mut self, args: fmt::Arguments) -> fmt::Result {
        let mut adapt = WideWriteAdapt(self);
        fmt::write(&mut adapt, args)
    }
}

/// Wide strings implement [`WideWrite`].
impl WideWrite for WString {
    fn write_wstr(&mut self, s: &wstr) -> fmt::Result {
        self.push_utfstr(s);
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_str(s);
        Ok(())
    }
}

/// A Writer which counts how many chars are written.
struct WriteCounter(usize);

impl WideWrite for WriteCounter {
    fn write_wstr(&mut self, s: &wstr) -> fmt::Result {
        self.0 += s.as_char_slice().len();
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0 += s.chars().count();
        Ok(())
    }
}

fn write_str(
    w: &mut impl WideWrite,
    flags: Flags,
    width: u64,
    precision: Option<u64>,
    b: &[char],
) -> fmt::Result {
    let string: String = b.iter().collect();
    let precision = precision.unwrap_or(string.len() as u64);
    if flags.contains(Flags::LEFT_ALIGN) {
        write!(
            w,
            "{:1$.prec$}",
            string,
            width as usize,
            prec = precision as usize
        )
    } else {
        write!(
            w,
            "{:>1$.prec$}",
            string,
            width as usize,
            prec = precision as usize
        )
    }
}

macro_rules! define_numeric {
    ($w: expr, $data: expr, $flags: expr, $width: expr, $precision: expr) => {
        define_numeric!($w, $data, $flags, $width, $precision, "")
    };
    ($w: expr, $data: expr, $flags: expr, $width: expr, $precision: expr, $ty:expr) => {{
        if $flags.contains(Flags::LEFT_ALIGN) {
            if $flags.contains(Flags::PREPEND_PLUS) {
                write!(
                    $w,
                    concat!("{:<+width$.prec$", $ty, "}"),
                    $data,
                    width = $width as usize,
                    prec = $precision as usize
                )
            } else if $flags.contains(Flags::PREPEND_SPACE) && !$data.is_sign_negative() {
                write!(
                    $w,
                    concat!(" {:<width$.prec$", $ty, "}"),
                    $data,
                    width = ($width as usize).wrapping_sub(1),
                    prec = $precision as usize
                )
            } else {
                write!(
                    $w,
                    concat!("{:<width$.prec$", $ty, "}"),
                    $data,
                    width = $width as usize,
                    prec = $precision as usize
                )
            }
        } else if $flags.contains(Flags::PREPEND_PLUS) {
            if $flags.contains(Flags::PREPEND_ZERO) {
                write!(
                    $w,
                    concat!("{:+0width$.prec$", $ty, "}"),
                    $data,
                    width = $width as usize,
                    prec = $precision as usize
                )
            } else {
                write!(
                    $w,
                    concat!("{:+width$.prec$", $ty, "}"),
                    $data,
                    width = $width as usize,
                    prec = $precision as usize
                )
            }
        } else if $flags.contains(Flags::PREPEND_ZERO) {
            if $flags.contains(Flags::PREPEND_SPACE) && !$data.is_sign_negative() {
                let mut d = WriteCounter(0);
                let _ = write!(
                    d,
                    concat!("{:.prec$", $ty, "}"),
                    $data,
                    prec = $precision as usize
                );
                if d.0 + 1 > $width as usize {
                    $width += 1;
                }
                write!(
                    $w,
                    concat!(" {:0width$.prec$", $ty, "}"),
                    $data,
                    width = ($width as usize).wrapping_sub(1),
                    prec = $precision as usize
                )
            } else {
                write!(
                    $w,
                    concat!("{:0width$.prec$", $ty, "}"),
                    $data,
                    width = $width as usize,
                    prec = $precision as usize
                )
            }
        } else {
            if $flags.contains(Flags::PREPEND_SPACE) && !$data.is_sign_negative() {
                let mut d = WriteCounter(0);
                let _ = write!(
                    d,
                    concat!("{:.prec$", $ty, "}"),
                    $data,
                    prec = $precision as usize
                );
                if (d.0 as u64) + 1 > $width as u64 {
                    $width = d.0 as u64 + 1;
                }
            }
            write!(
                $w,
                concat!("{:width$.prec$", $ty, "}"),
                $data,
                width = $width as usize,
                prec = $precision as usize
            )
        }
    }};
}

macro_rules! define_unumeric {
    ($w: expr, $data: expr, $flags: expr, $width: expr, $precision: expr) => {
        define_unumeric!($w, $data, $flags, $width, $precision, "")
    };
    ($w: expr, $data: expr, $flags: expr, $width: expr, $precision: expr, $ty:expr) => {{
        if $flags.contains(Flags::LEFT_ALIGN) {
            if $flags.contains(Flags::ALTERNATE_FORM) {
                write!(
                    $w,
                    concat!("{:<#width$", $ty, "}"),
                    $data,
                    width = $width as usize
                )
            } else {
                write!(
                    $w,
                    concat!("{:<width$", $ty, "}"),
                    $data,
                    width = $width as usize
                )
            }
        } else if $flags.contains(Flags::ALTERNATE_FORM) {
            if $flags.contains(Flags::PREPEND_ZERO) {
                write!(
                    $w,
                    concat!("{:#0width$", $ty, "}"),
                    $data,
                    width = $width as usize
                )
            } else {
                write!(
                    $w,
                    concat!("{:#width$", $ty, "}"),
                    $data,
                    width = $width as usize
                )
            }
        } else if $flags.contains(Flags::PREPEND_ZERO) {
            write!(
                $w,
                concat!("{:0width$", $ty, "}"),
                $data,
                width = $width as usize
            )
        } else {
            write!(
                $w,
                concat!("{:width$", $ty, "}"),
                $data,
                width = $width as usize
            )
        }
    }};
}

/// Format a non-finite value.
fn format_non_finite(
    w: &mut impl WideWrite,
    value: f64,
    mut flags: Flags,
    mut width: u64,
    upper: bool,
) -> fmt::Result {
    assert!(!value.is_finite());
    // Do not pad with zeros as we are not finite, since 00000IN` makes no sense.
    // Do not place a leading + or ' ' if we are NaN, since +NaN makes no sense.
    // However +inf does make sense.
    flags.remove(Flags::PREPEND_ZERO);
    if value.is_nan() {
        flags.remove(Flags::PREPEND_PLUS);
        flags.remove(Flags::PREPEND_SPACE);
    }
    let mut tmp = String::new();
    // C emits inf/nan for "f", and INF/NAN for "F".
    // Rust only does inf/NaN.
    let _: () = define_numeric!(tmp, value, flags, width, 0 /* precision */)?;
    if upper {
        tmp.make_ascii_uppercase();
    } else {
        tmp.make_ascii_lowercase();
    }
    w.write_str(&tmp)
}

// Split a float into a mantissa and exponent.
fn split_float(value: f64, precision: usize) -> (String, i32) {
    assert!(value.is_finite());
    let formatted = format!("{:.*e}", precision, value);
    let mut parts = formatted.splitn(2, 'e');
    let mantissa = parts.next().unwrap().to_string();
    let exponent_str = parts.next().unwrap();
    assert!(parts.next().is_none());

    let exponent = exponent_str
        .parse::<i32>()
        .unwrap_or_else(|_| panic!("Failed to parse exponent: {}", exponent_str));
    (mantissa, exponent)
}

/// Maybe prepend a sign to the given string.
/// This respects PREPEND_PLUS and PREPEND_SPACE.
fn maybe_prepend_sign(mut s: String, flags: Flags) -> String {
    if !s.starts_with('-') {
        if flags.contains(Flags::PREPEND_PLUS) {
            s.insert(0, '+');
        } else if flags.contains(Flags::PREPEND_SPACE) {
            s.insert(0, ' ');
        }
    }
    s
}

// Write out a float, applying padding.
// exp_type is expected to be "e", "E", or empty.
// If exponent is empty, then we omit the exp_type.
fn write_float_parts(
    w: &mut impl WideWrite,
    mut mantissa: String,
    mut exp_type: &str,
    exponent: String,
    flags: Flags,
    width: u64,
) -> fmt::Result {
    assert!(matches!(exp_type, "e" | "E" | ""));

    // Ignore exp_type if exponent is empty.
    if exponent.is_empty() {
        exp_type = "";
    }

    // Compute the width of everything.
    // We use "len" as a proxy for number of chars, as we expect ASCII.
    let total_width = mantissa.len() + exp_type.len() + exponent.len();

    // If we're lucky, no padding is required.
    let padding = width.saturating_sub(total_width as u64) as usize;
    if padding == 0 {
        write!(w, "{}{}{}", mantissa, exp_type, exponent)
    } else if flags.contains(Flags::LEFT_ALIGN) {
        // Pad on the right with spaces.
        write!(
            w,
            "{0}{1}{2}{3:4$}",
            mantissa, exp_type, exponent, "", padding
        )
    } else if flags.contains(Flags::PREPEND_ZERO) {
        // Insert zeros between the "sign" and the mantissa.
        // Note the "sign" may be a space, +, or -.
        let mut sign = "";
        for s in ["+", "-", " "] {
            if mantissa.starts_with(s) {
                mantissa.remove(0);
                sign = s;
                break;
            }
        }

        // This funny {1:0>2$} means "pad arg 1 with zeros on left to width given in arg 2".
        write!(
            w,
            "{0}{1:0>2$}{3}{4}{5}",
            sign, "", padding, mantissa, exp_type, exponent
        )
    } else {
        // Pad on the left with spaces.
        write!(
            w,
            "{0:1$}{2}{3}{4}",
            "", padding, mantissa, exp_type, exponent
        )
    }
}

// Write an f64 to the writer, matching the 'g' and 'G' specifiers from printf.
fn write_auto(
    w: &mut impl WideWrite,
    value: f64,
    flags: Flags,
    width: u64,
    precision: u64,
    exp_type: &str,
) -> fmt::Result {
    // The precision changes meaning here from "number of digits after decimal point" to "maximum number of significant digits."
    // For example, `printf "%.1g" 2.599` should produce "3."
    // It is at least 1; use i64.
    // TODO: the calculation below is incorrect for large values, since we multiply by 10. Find a better way to handle sigfigs.
    assert!(exp_type == "g" || exp_type == "G");
    assert!(value.is_finite());
    let sigfigs = precision.max(1).min(i64::MAX as u64) as i64;

    // Helper get the base 10 exponent of a value.
    fn get_exponent(value: f64) -> i64 {
        if value == 0.0 {
            0
        } else {
            value.log10().floor() as i64
        }
    }

    let vabs = value.abs();
    let rounder = if vabs == 0.0 {
        1.0
    } else {
        (10.0_f64).powf((sigfigs - 1 - get_exponent(vabs)) as f64)
    };

    // Round to recalculate the exponent.
    let rounded_vabs = (vabs * rounder).round() / rounder;
    let rounded_exponent = get_exponent(rounded_vabs);

    // "Style e is used if the exponent from its conversion is less than -4 or greater than or equal to the precision."
    let digits_after_decimal;
    let use_style_e;
    if rounded_exponent < -4 || rounded_exponent >= sigfigs {
        use_style_e = true;
        digits_after_decimal = sigfigs - 1;
    } else {
        use_style_e = false;
        digits_after_decimal = sigfigs - rounded_exponent - 1;
    }

    let decimal_point = "."; // TODO: locale dependence

    let mut mantissa: String;
    let exponent: String; // maybe empty if not using style e.
    if digits_after_decimal >= 0 {
        // We can use Rust's formatting here, since we will show the entire mantissa.
        if use_style_e {
            let (m, exp) = split_float(value, digits_after_decimal as usize);
            mantissa = m;
            exponent = format!("{:+03}", exp);
        } else {
            // Like style 'f' except trimming 0s and decimal point (except in alt mode).
            mantissa = format!("{:.*}", digits_after_decimal as usize, value);
            exponent = "".to_string();
        }
    } else {
        // Gross: we need to round in the left side of the decimal point.
        // Construct an integer that represents the rounded value.
        let rounded = rounded_vabs.copysign(value);
        if use_style_e {
            let (m, exp) = split_float(rounded, rounded_exponent as usize);
            mantissa = m;
            exponent = format!("{:+03}", exp);
        } else {
            // Pure decimal representation.
            mantissa = format!("{}", rounded);
            exponent = "".to_string();
        }
    }

    // Maybe trim trailing zeros.
    if !flags.contains(Flags::ALTERNATE_FORM) {
        let trimmed = mantissa
            .trim_end_matches('0')
            .trim_end_matches(decimal_point);
        mantissa.truncate(trimmed.len());
    }

    // Handle the case of "0".
    if mantissa.is_empty() {
        mantissa.push('0');
    }

    // Maybe prepend a + or a space.
    mantissa = maybe_prepend_sign(mantissa, flags);

    // Do what write_float_parts does, except we may have no exponent.
    let exp_type_e = if exp_type == "G" { "E" } else { "e" };
    write_float_parts(w, mantissa, exp_type_e, exponent, flags, width)
}

/// Write an f64 to the writer, matching the 'e' and 'E' specifiers from printf.
fn write_scientific(
    w: &mut impl WideWrite,
    value: f64,
    flags: Flags,
    width: u64,
    precision: u64,
    exp_type: &str,
) -> fmt::Result {
    // This differs from Rust's e/E formatting in the following ways:
    //  - The exponent is always at least 2 digits.
    //  - The exponent is always prefixed with a sign.
    assert!(exp_type == "e" || exp_type == "E");
    assert!(value.is_finite());
    let (mut mantissa, exponent) = split_float(value, precision as usize);

    // Format exponent into a string, with at least 2 digits and leading +.
    let exponent = format!("{:+03}", exponent);

    // Maybe prepend a + or a space.
    mantissa = maybe_prepend_sign(mantissa, flags);
    write_float_parts(w, mantissa, exp_type, exponent, flags, width)
}

/// Write a single argument to the writer.
/// Returns the number of bytes written, or -1 on failure.
fn write_1_arg(arg: Argument, w: &mut impl WideWrite) -> fmt::Result {
    let Argument {
        flags,
        mut width,
        precision,
        specifier,
    } = arg;
    match specifier {
        Specifier::Percent => w.write_str("%"),
        Specifier::Literals(data) => write_str(w, flags, width, precision, data),
        Specifier::String(data) => write_str(w, flags, width, precision, data.as_char_slice()),
        Specifier::Hex(data) => {
            define_unumeric!(w, data, flags, width, precision.unwrap_or(0), "x")
        }
        Specifier::UpperHex(data) => {
            define_unumeric!(w, data, flags, width, precision.unwrap_or(0), "X")
        }
        Specifier::Octal(data) => {
            define_unumeric!(w, data, flags, width, precision.unwrap_or(0), "o")
        }
        Specifier::Uint(data) => {
            define_unumeric!(w, data, flags, width, precision.unwrap_or(0))
        }
        Specifier::Int(data) => define_numeric!(w, data, flags, width, precision.unwrap_or(0)),
        Specifier::Double { value, format } => {
            match format {
                any_format if !value.is_finite() => {
                    // C produces nan/inf and NAN/INF for %f and %F, respectively.
                    // Rust gives us NaN and inf.
                    // This matters if we are not finite.
                    format_non_finite(w, value, flags, width, any_format.is_upper())
                }
                DoubleFormat::Normal
                | DoubleFormat::Hex
                | DoubleFormat::UpperNormal
                | DoubleFormat::UpperHex => {
                    define_numeric!(w, value, flags, width, precision.unwrap_or(6))
                }

                DoubleFormat::Auto | DoubleFormat::UpperAuto => {
                    let exp_type = if format.is_upper() { "G" } else { "g" };
                    write_auto(w, value, flags, width, precision.unwrap_or(6), exp_type)
                }

                DoubleFormat::Scientific | DoubleFormat::UpperScientific => {
                    let exp_type = if format.is_upper() { "E" } else { "e" };
                    write_scientific(w, value, flags, width, precision.unwrap_or(6), exp_type)
                }
            }
        }
        Specifier::Char(data) => {
            if flags.contains(Flags::LEFT_ALIGN) {
                write!(w, "{:width$}", data as char, width = width as usize)
            } else {
                write!(w, "{:>width$}", data as char, width = width as usize)
            }
        }
        Specifier::Pointer(data) => {
            if flags.contains(Flags::LEFT_ALIGN) {
                write!(w, "{:<width$p}", data, width = width as usize)
            } else if flags.contains(Flags::PREPEND_ZERO) {
                write!(w, "{:0width$p}", data, width = width as usize)
            } else {
                write!(w, "{:width$p}", data, width = width as usize)
            }
        } //Specifier::WriteBytesWritten(_, _) => Err(Default::default()),
    }
}

/// Write to a struct that implements [`WideWrite`].
///
/// # Differences
///
/// There are a few differences from standard printf format:
///
/// - only valid UTF-8 data can be printed.
/// - an `X` format specifier with a `#` flag prints the hex data in uppercase,
///   but the leading `0x` is still lowercase
/// - an `o` format specifier with a `#` flag precedes the number with an `o`
///   instead of `0`
/// - `g`/`G` (shorted floating point) is aliased to `f`/`F`` (decimal floating
///   point)
/// - same for `a`/`A` (hex floating point)
/// - the `n` format specifier, [`Specifier::WriteBytesWritten`], is not
///   implemented and will cause an error if encountered.
pub fn wide_write(w: &mut impl WideWrite) -> impl FnMut(Argument) -> fmt::Result + '_ {
    move |arg| write_1_arg(arg, w)
}

// Adapts `fmt::Write` to `WideWrite`.
struct FmtWrite<'a, T>(&'a mut T);

impl<'a, T> WideWrite for FmtWrite<'a, T>
where
    T: fmt::Write,
{
    /// Write a wstr.
    fn write_wstr(&mut self, s: &wstr) -> fmt::Result {
        self.0.write_str(&s.to_string())
    }

    /// Write a str.
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_str(s)
    }

    /// Allows using write! macro.
    fn write_fmt(&mut self, args: fmt::Arguments) -> fmt::Result {
        self.0.write_fmt(args)
    }
}

/// Write to a struct that implements [`fmt::Write`].
pub fn fmt_write(w: &mut impl fmt::Write) -> impl FnMut(Argument) -> fmt::Result + '_ {
    move |arg| write_1_arg(arg, &mut FmtWrite(w))
}

/// Returns an object that implements [`Display`][fmt::Display] for safely
/// printing formatting data. This is slightly less performant than using
/// [`fmt_write`], but may be the only option.
///
/// This shares the same caveats as [`fmt_write`].
pub unsafe fn display<'a, 'b>(format: &'a wstr, args: ArgList<'b>) -> ArgListDisplay<'a, 'b> {
    ArgListDisplay { format, args }
}

/// Helper struct created by [`display`] for safely printing `printf`-style
/// formatting with [`format!`] and `{}`. This can be used with anything that
/// uses [`format_args!`], such as [`println!`] or the `log` crate.
pub struct ArgListDisplay<'a, 'b> {
    format: &'a wstr,
    args: ArgList<'b>,
}

impl<'a, 'b> fmt::Display for ArgListDisplay<'a, 'b> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        super::format(self.format, &mut self.args.clone(), fmt_write(f))
    }
}
