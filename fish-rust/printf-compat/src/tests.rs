use core::ffi::*;

unsafe extern "C" fn rust_fmt(str: *const u8, mut args: ...) -> Box<(c_int, String)> {
    let mut s = String::new();
    let bytes_written = crate::format(
        str as _,
        args.clone().as_va_list(),
        crate::output::fmt_write(&mut s),
    );
    assert!(bytes_written >= 0);
    let mut s2 = std::io::Cursor::new(vec![]);
    assert_eq!(
        bytes_written,
        crate::format(
            str as _,
            args.as_va_list(),
            crate::output::io_write(&mut s2),
        )
    );
    assert_eq!(s.as_bytes(), s2.get_ref());
    Box::new((bytes_written, s))
}

macro_rules! assert_eq_fmt {
    ($expected: expr, $format:expr $(, $p:expr)*) => {
        assert_eq!($expected,  rust_fmt($format, $($p),*).1)
    };
}

macro_rules! c_str {
    ($data:literal) => {
        concat!($data, "\0").as_ptr()
    };
}

#[test]
fn test_plain() {
    unsafe {
        assert_eq_fmt!("abc", c_str!("abc"));
        assert_eq_fmt!("", c_str!(""));
        assert_eq_fmt!("%", c_str!("%%"));
        assert_eq_fmt!("% def", c_str!("%% def"));
        assert_eq_fmt!("abc %", c_str!("abc %%"));
        assert_eq_fmt!("abc % def", c_str!("abc %% def"));
        assert_eq_fmt!("abc %% def", c_str!("abc %%%% def"));
        assert_eq_fmt!("%%%", c_str!("%%%%%%"));
    }
}

#[test]
fn test_str() {
    unsafe {
        assert_eq_fmt!("hello world", c_str!("hello %s"), c_str!("world"));
        assert_eq_fmt!("hello %world", c_str!("hello %%%s"), c_str!("world"));
        assert_eq_fmt!("     world", c_str!("%10s"), c_str!("world"));
        assert_eq_fmt!("worl", c_str!("%.4s"), c_str!("world"));
        assert_eq_fmt!("      worl", c_str!("%10.4s"), c_str!("world"));
        assert_eq_fmt!("worl      ", c_str!("%-10.4s"), c_str!("world"));
        assert_eq_fmt!("world     ", c_str!("%-10s"), c_str!("world"));
    }
}

#[test]
fn test_int() {
    unsafe {
        assert_eq_fmt!(format!(" {:023124}", 17), c_str!("% 0*i"), 23125, 17);
        assert_eq_fmt!(" 000023125", c_str!("% 010i"), 23125);
        assert_eq_fmt!("     23125", c_str!("% 10i"), 23125);
        assert_eq_fmt!(" 23125", c_str!("% 5i"), 23125);
        assert_eq_fmt!(" 23125", c_str!("% 4i"), 23125);
        assert_eq_fmt!(" 23125    ", c_str!("%- 010i"), 23125);
        assert_eq_fmt!(" 23125    ", c_str!("%- 10i"), 23125);
        assert_eq_fmt!(" 23125", c_str!("%- 5i"), 23125);
        assert_eq_fmt!(" 23125", c_str!("%- 4i"), 23125);
        assert_eq_fmt!("+000023125", c_str!("%+ 010i"), 23125);
        assert_eq_fmt!("    +23125", c_str!("%+ 10i"), 23125);
        assert_eq_fmt!("+23125", c_str!("%+ 5i"), 23125);
        assert_eq_fmt!("+23125", c_str!("%+ 4i"), 23125);
        assert_eq_fmt!("23125     ", c_str!("%-010i"), 23125);
        assert_eq_fmt!("23125     ", c_str!("%-10i"), 23125);
        assert_eq_fmt!("23125", c_str!("%-5i"), 23125);
        assert_eq_fmt!("23125", c_str!("%-4i"), 23125);
    }
}

#[test]
fn test_octal() {
    unsafe {
        assert_eq_fmt!("0000055125", c_str!("% 010o"), 23125);
        assert_eq_fmt!("     55125", c_str!("% 10o"), 23125);
        assert_eq_fmt!("55125", c_str!("% 5o"), 23125);
        assert_eq_fmt!("55125", c_str!("% 4o"), 23125);
        assert_eq_fmt!("55125     ", c_str!("%- 010o"), 23125);
        assert_eq_fmt!("55125     ", c_str!("%- 10o"), 23125);
        assert_eq_fmt!("55125", c_str!("%- 5o"), 23125);
        assert_eq_fmt!("55125", c_str!("%- 4o"), 23125);
        assert_eq_fmt!("0000055125", c_str!("%+ 010o"), 23125);
        assert_eq_fmt!("     55125", c_str!("%+ 10o"), 23125);
        assert_eq_fmt!("55125", c_str!("%+ 5o"), 23125);
        assert_eq_fmt!("55125", c_str!("%+ 4o"), 23125);
        assert_eq_fmt!("55125     ", c_str!("%-010o"), 23125);
        assert_eq_fmt!("55125     ", c_str!("%-10o"), 23125);
        assert_eq_fmt!("55125", c_str!("%-5o"), 23125);
        assert_eq_fmt!("55125", c_str!("%-4o"), 23125);
    }
}

#[test]
fn test_hex() {
    unsafe {
        assert_eq_fmt!("0000005a55", c_str!("% 010x"), 23125);
        assert_eq_fmt!("      5a55", c_str!("% 10x"), 23125);
        assert_eq_fmt!(" 5a55", c_str!("% 5x"), 23125);
        assert_eq_fmt!("5a55", c_str!("% 4x"), 23125);
        assert_eq_fmt!("5a55      ", c_str!("%- 010x"), 23125);
        assert_eq_fmt!("5a55      ", c_str!("%- 10x"), 23125);
        assert_eq_fmt!("5a55 ", c_str!("%- 5x"), 23125);
        assert_eq_fmt!("5a55", c_str!("%- 4x"), 23125);
        assert_eq_fmt!("0000005a55", c_str!("%+ 010x"), 23125);
        assert_eq_fmt!("      5a55", c_str!("%+ 10x"), 23125);
        assert_eq_fmt!(" 5a55", c_str!("%+ 5x"), 23125);
        assert_eq_fmt!("5a55", c_str!("%+ 4x"), 23125);
        assert_eq_fmt!("5a55      ", c_str!("%-010x"), 23125);
        assert_eq_fmt!("5a55      ", c_str!("%-10x"), 23125);
        assert_eq_fmt!("5a55 ", c_str!("%-5x"), 23125);
        assert_eq_fmt!("5a55", c_str!("%-4x"), 23125);

        assert_eq_fmt!("0x00005a55", c_str!("%# 010x"), 23125);
        assert_eq_fmt!("    0x5a55", c_str!("%# 10x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%# 5x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%# 4x"), 23125);
        assert_eq_fmt!("0x5a55    ", c_str!("%#- 010x"), 23125);
        assert_eq_fmt!("0x5a55    ", c_str!("%#- 10x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%#- 5x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%#- 4x"), 23125);
        assert_eq_fmt!("0x00005a55", c_str!("%#+ 010x"), 23125);
        assert_eq_fmt!("    0x5a55", c_str!("%#+ 10x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%#+ 5x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%#+ 4x"), 23125);
        assert_eq_fmt!("0x5a55    ", c_str!("%#-010x"), 23125);
        assert_eq_fmt!("0x5a55    ", c_str!("%#-10x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%#-5x"), 23125);
        assert_eq_fmt!("0x5a55", c_str!("%#-4x"), 23125);

        assert_eq_fmt!("0000005A55", c_str!("% 010X"), 23125);
        assert_eq_fmt!("      5A55", c_str!("% 10X"), 23125);
        assert_eq_fmt!(" 5A55", c_str!("% 5X"), 23125);
        assert_eq_fmt!("5A55", c_str!("% 4X"), 23125);
        assert_eq_fmt!("5A55      ", c_str!("%- 010X"), 23125);
        assert_eq_fmt!("5A55      ", c_str!("%- 10X"), 23125);
        assert_eq_fmt!("5A55 ", c_str!("%- 5X"), 23125);
        assert_eq_fmt!("5A55", c_str!("%- 4X"), 23125);
        assert_eq_fmt!("0000005A55", c_str!("%+ 010X"), 23125);
        assert_eq_fmt!("      5A55", c_str!("%+ 10X"), 23125);
        assert_eq_fmt!(" 5A55", c_str!("%+ 5X"), 23125);
        assert_eq_fmt!("5A55", c_str!("%+ 4X"), 23125);
        assert_eq_fmt!("5A55      ", c_str!("%-010X"), 23125);
        assert_eq_fmt!("5A55      ", c_str!("%-10X"), 23125);
        assert_eq_fmt!("5A55 ", c_str!("%-5X"), 23125);
        assert_eq_fmt!("5A55", c_str!("%-4X"), 23125);
    }
}

#[test]
fn test_float() {
    unsafe {
        assert_eq_fmt!("1234.000000", c_str!("%f"), 1234f64);
        assert_eq_fmt!("1234.00000", c_str!("%.5f"), 1234f64);
        assert_eq_fmt!("0", c_str!("%.*f"), 1234f64, 3);
    }
}

#[test]
fn test_char() {
    unsafe {
        assert_eq_fmt!("a", c_str!("%c"), b'a' as c_int);
        assert_eq_fmt!("         a", c_str!("%10c"), b'a' as c_int);
        assert_eq_fmt!("a         ", c_str!("%-10c"), b'a' as c_int);
    }
}
