use crate::args::{Arg, ArgList, ToArg};
use crate::wstr;
use widestring::{utf32str, Utf32Str};

fn rust_fmt<'a>(str: &wstr, args: &[Arg<'a>]) -> String {
    let mut s = String::new();
    let mut args = ArgList::new(args);
    let res = crate::format(str, &mut args, crate::output::fmt_write(&mut s));
    if res.is_err() {
        panic!("Formatting failed");
    }
    if args.remaining() > 0 {
        panic!("too many args");
    }
    s
}

macro_rules! assert_eq_fmt {
    ($expected: expr, $format:literal $(, $p:expr)*) => {
        assert_eq!($expected,  rust_fmt(utf32str!($format), &[$($p.to_arg()),*]))
    };
}

#[test]
fn test_plain() {
    assert_eq_fmt!("abc", "abc");
    assert_eq_fmt!("", "");
    assert_eq_fmt!("%", "%%");
    assert_eq_fmt!("% def", "%% def");
    assert_eq_fmt!("abc %", "abc %%");
    assert_eq_fmt!("abc % def", "abc %% def");
    assert_eq_fmt!("abc %% def", "abc %%%% def");
    assert_eq_fmt!("%%%", "%%%%%%");
}

#[test]
fn test_str() {
    assert_eq_fmt!("hello world", "hello %s", "world");
    assert_eq_fmt!("hello %world", "hello %%%s", "world");
    assert_eq_fmt!("     world", "%10s", "world");
    assert_eq_fmt!("worl", "%.4s", "world");
    assert_eq_fmt!("      worl", "%10.4s", "world");
    assert_eq_fmt!("worl      ", "%-10.4s", "world");
    assert_eq_fmt!("world     ", "%-10s", "world");
}

#[test]
fn test_int() {
    assert_eq_fmt!(format!(" {:023124}", 17), "% 0*i", 23125, 17);
    assert_eq_fmt!(" 000023125", "% 010i", 23125);
    assert_eq_fmt!("     23125", "% 10i", 23125);
    assert_eq_fmt!(" 23125", "% 5i", 23125);
    assert_eq_fmt!(" 23125", "% 4i", 23125);
    assert_eq_fmt!(" 23125    ", "%- 010i", 23125);
    assert_eq_fmt!(" 23125    ", "%- 10i", 23125);
    assert_eq_fmt!(" 23125", "%- 5i", 23125);
    assert_eq_fmt!(" 23125", "%- 4i", 23125);
    assert_eq_fmt!("+000023125", "%+ 010i", 23125);
    assert_eq_fmt!("    +23125", "%+ 10i", 23125);
    assert_eq_fmt!("+23125", "%+ 5i", 23125);
    assert_eq_fmt!("+23125", "%+ 4i", 23125);
    assert_eq_fmt!("23125     ", "%-010i", 23125);
    assert_eq_fmt!("23125     ", "%-10i", 23125);
    assert_eq_fmt!("23125", "%-5i", 23125);
    assert_eq_fmt!("23125", "%-4i", 23125);
}

#[test]
fn test_octal() {
    assert_eq_fmt!("0000055125", "% 010o", 23125);
    assert_eq_fmt!("     55125", "% 10o", 23125);
    assert_eq_fmt!("55125", "% 5o", 23125);
    assert_eq_fmt!("55125", "% 4o", 23125);
    assert_eq_fmt!("55125     ", "%- 010o", 23125);
    assert_eq_fmt!("55125     ", "%- 10o", 23125);
    assert_eq_fmt!("55125", "%- 5o", 23125);
    assert_eq_fmt!("55125", "%- 4o", 23125);
    assert_eq_fmt!("0000055125", "%+ 010o", 23125);
    assert_eq_fmt!("     55125", "%+ 10o", 23125);
    assert_eq_fmt!("55125", "%+ 5o", 23125);
    assert_eq_fmt!("55125", "%+ 4o", 23125);
    assert_eq_fmt!("55125     ", "%-010o", 23125);
    assert_eq_fmt!("55125     ", "%-10o", 23125);
    assert_eq_fmt!("55125", "%-5o", 23125);
    assert_eq_fmt!("55125", "%-4o", 23125);
}

#[test]
fn test_hex() {
    assert_eq_fmt!("0000005a55", "% 010x", 23125);
    assert_eq_fmt!("      5a55", "% 10x", 23125);
    assert_eq_fmt!(" 5a55", "% 5x", 23125);
    assert_eq_fmt!("5a55", "% 4x", 23125);
    assert_eq_fmt!("5a55      ", "%- 010x", 23125);
    assert_eq_fmt!("5a55      ", "%- 10x", 23125);
    assert_eq_fmt!("5a55 ", "%- 5x", 23125);
    assert_eq_fmt!("5a55", "%- 4x", 23125);
    assert_eq_fmt!("0000005a55", "%+ 010x", 23125);
    assert_eq_fmt!("      5a55", "%+ 10x", 23125);
    assert_eq_fmt!(" 5a55", "%+ 5x", 23125);
    assert_eq_fmt!("5a55", "%+ 4x", 23125);
    assert_eq_fmt!("5a55      ", "%-010x", 23125);
    assert_eq_fmt!("5a55      ", "%-10x", 23125);
    assert_eq_fmt!("5a55 ", "%-5x", 23125);
    assert_eq_fmt!("5a55", "%-4x", 23125);

    assert_eq_fmt!("0x00005a55", "%# 010x", 23125);
    assert_eq_fmt!("    0x5a55", "%# 10x", 23125);
    assert_eq_fmt!("0x5a55", "%# 5x", 23125);
    assert_eq_fmt!("0x5a55", "%# 4x", 23125);
    assert_eq_fmt!("0x5a55    ", "%#- 010x", 23125);
    assert_eq_fmt!("0x5a55    ", "%#- 10x", 23125);
    assert_eq_fmt!("0x5a55", "%#- 5x", 23125);
    assert_eq_fmt!("0x5a55", "%#- 4x", 23125);
    assert_eq_fmt!("0x00005a55", "%#+ 010x", 23125);
    assert_eq_fmt!("    0x5a55", "%#+ 10x", 23125);
    assert_eq_fmt!("0x5a55", "%#+ 5x", 23125);
    assert_eq_fmt!("0x5a55", "%#+ 4x", 23125);
    assert_eq_fmt!("0x5a55    ", "%#-010x", 23125);
    assert_eq_fmt!("0x5a55    ", "%#-10x", 23125);
    assert_eq_fmt!("0x5a55", "%#-5x", 23125);
    assert_eq_fmt!("0x5a55", "%#-4x", 23125);

    assert_eq_fmt!("0000005A55", "% 010X", 23125);
    assert_eq_fmt!("      5A55", "% 10X", 23125);
    assert_eq_fmt!(" 5A55", "% 5X", 23125);
    assert_eq_fmt!("5A55", "% 4X", 23125);
    assert_eq_fmt!("5A55      ", "%- 010X", 23125);
    assert_eq_fmt!("5A55      ", "%- 10X", 23125);
    assert_eq_fmt!("5A55 ", "%- 5X", 23125);
    assert_eq_fmt!("5A55", "%- 4X", 23125);
    assert_eq_fmt!("0000005A55", "%+ 010X", 23125);
    assert_eq_fmt!("      5A55", "%+ 10X", 23125);
    assert_eq_fmt!(" 5A55", "%+ 5X", 23125);
    assert_eq_fmt!("5A55", "%+ 4X", 23125);
    assert_eq_fmt!("5A55      ", "%-010X", 23125);
    assert_eq_fmt!("5A55      ", "%-10X", 23125);
    assert_eq_fmt!("5A55 ", "%-5X", 23125);
    assert_eq_fmt!("5A55", "%-4X", 23125);
}

#[test]
fn test_float() {
    assert_eq_fmt!("1234.000000", "%f", 1234f64);
    assert_eq_fmt!("1234.00000", "%.5f", 1234f64);
    assert_eq_fmt!("1234.560000", "%.*f", 6, 1234.56f64);
}

#[test]
fn test_char() {
    assert_eq_fmt!("a", "%c", 'a');
    assert_eq_fmt!("         a", "%10c", 'a');
    assert_eq_fmt!("a         ", "%-10c", 'a');
}

#[test]
fn test_int_2() {
    assert_eq_fmt!("12", "%d", 12);
    assert_eq_fmt!("~148~", "~%d~", 148);
    assert_eq_fmt!("00-91232xx", "00%dxx", -91232);
    assert_eq_fmt!("ffffdbf0", "%x", -9232);
    assert_eq_fmt!("1B0", "%X", 432);
    assert_eq_fmt!("0000001B0", "%09X", 432);
    assert_eq_fmt!("      1B0", "%9X", 432);
    assert_eq_fmt!("      1EC", "%+9X", 492);
    assert_eq_fmt!("   0x11ed", "% #9x", 4589);
    assert_eq_fmt!(" 4", "%2o", 4);
    assert_eq_fmt!("          -4", "% 12d", -4);
    assert_eq_fmt!("          48", "% 12d", 48);
    assert_eq_fmt!("-4", "%ld", -4_i64);
    assert_eq_fmt!("-4", "%lld", -4_i64);
    assert_eq_fmt!("FFFFFFFFFFFFFFFC", "%lX", -4_i64);
    assert_eq_fmt!("48", "%ld", 48_i64);
    assert_eq_fmt!("48", "%lld", 48_i64);
    assert_eq_fmt!("-12     ", "%-8hd", -12_i16);

    assert_eq_fmt!("12", "%u", 12);
    assert_eq_fmt!("~148~", "~%u~", 148);
    assert_eq_fmt!("0091232xx", "00%uxx", 91232);
    assert_eq_fmt!("2410", "%x", 9232);
    assert_eq_fmt!("      1EC", "%9X", 492);
    assert_eq_fmt!("           4", "% 12u", 4);
    assert_eq_fmt!("          48", "% 12u", 48);
    assert_eq_fmt!("4", "%lu", 4_u64);
    assert_eq_fmt!("4", "%llu", 4_u64);
    assert_eq_fmt!("4", "%lX", 4_u64);
    assert_eq_fmt!("48", "%lu", 48_u64);
    assert_eq_fmt!("48", "%llu", 48_u64);
    assert_eq_fmt!("12      ", "%-8hu", 12_u16);

    // All signed values passed to unsigned types are mod 2^64.
    // Width specifiers are ignored.
    assert_eq_fmt!("-1", "%lld", -1);
    assert_eq_fmt!("-1", "%ld", -1);
    assert_eq_fmt!("-1", "%d", -1);
    assert_eq_fmt!("18446744073709551615", "%llu", -1);
    assert_eq_fmt!("18446744073709551615", "%lu", -1);
    assert_eq_fmt!("4294967295", "%u", -1);

    // Gross combinations of padding and precision.
    assert_eq_fmt!("                    1234565678", "%30d", 1234565678);
    assert_eq_fmt!("000000000000000000001234565678", "%030d", 1234565678);
    assert_eq_fmt!("          00000000001234565678", "%30.20d", 1234565678);
    // Here we specify both a precision and request zero-padding; the zero-padding is ignored (!).
    assert_eq_fmt!("          00000000001234565678", "%030.20d", 1234565678);
}

#[test]
fn test_float2() {
    assert_eq_fmt!("-46.380000", "%f", -46.38);
    assert_eq_fmt!("00000001.200", "%012.3f", 1.2);
    assert_eq_fmt!("0001.700e+00", "%012.3e", 1.7);
    assert_eq_fmt!("1.000000e+300", "%e", 1e300);
    assert_eq_fmt!("0000000002.6%!", "%012.3g%%!", 2.6);
    assert_eq_fmt!("-00000002.69", "%012.5G", -2.69);
    assert_eq_fmt!("+42.7850", "%+7.4f", 42.785);
    assert_eq_fmt!("{} 4.9312E+02", "{}% 7.4E", 493.12);
    assert_eq_fmt!("-1.2030E+02", "% 7.4E", -120.3);
    assert_eq_fmt!("INF       ", "%-10F", f64::INFINITY);
    assert_eq_fmt!("      +INF", "%+010F", f64::INFINITY);
    assert_eq_fmt!("nan", "% f", f64::NAN);
    assert_eq_fmt!("nan", "%+f", f64::NAN);
    assert_eq_fmt!("1000.0", "%.1f", 999.99);
    assert_eq_fmt!("10.0", "%.1f", 9.99);
    assert_eq_fmt!("1.0e+01", "%.1e", 9.99);
    assert_eq_fmt!("9.99", "%.2f", 9.99);
    assert_eq_fmt!("9.99e+00", "%.2e", 9.99);
    assert_eq_fmt!("9.990", "%.3f", 9.99);
    assert_eq_fmt!("9.990e+00", "%.3e", 9.99);
    assert_eq_fmt!("1e+01", "%.1g", 9.99);
    assert_eq_fmt!("1E+01", "%.1G", 9.99);
    assert_eq_fmt!("3.0", "%.1f", 2.99);
    assert_eq_fmt!("3.0e+00", "%.1e", 2.99);
    assert_eq_fmt!("3", "%.1g", 2.99);
    assert_eq_fmt!("2.6", "%.1f", 2.599);
    assert_eq_fmt!("2.6e+00", "%.1e", 2.599);
    // 'g' specifier changes meaning of precision to number of sigfigs.
    // This applies both to explicit precision, and the default precision, which is 6.
    assert_eq_fmt!("3", "%.1g", 2.599);
    assert_eq_fmt!("3", "%g", 3.0);
    assert_eq_fmt!("3", "%G", 3.0);
    assert_eq_fmt!("1.23423e+06", "%g", 1234234.532234234);
    assert_eq_fmt!("2.34902e+10", "%g", 23490234723.23423942394);
    assert_eq_fmt!("2.34902E+10", "%G", 23490234723.23423942394);

    assert_eq_fmt!("0", "%g", 0.0);
    assert_eq_fmt!("0", "%G", 0.0);
}

fn test_exhaustive(rust_fmt: &Utf32Str, c_fmt: *const i8) {
    // "There's only 4 billion floats so test them all."
    // This tests a format string expected to be of the form "%.*g" or "%.*e".
    // That is, it takes a precision and a double.
    println!("Testing {}", rust_fmt);
    let mut rust_str = String::with_capacity(128);
    let mut c_storage = [0u8; 128];
    let c_storage_ptr = c_storage.as_mut_ptr() as *mut i8;

    for i in 0..=u32::MAX {
        if i % 1000000 == 0 {
            println!("{:.2}%", (i as f64) / (u32::MAX as f64) * 100.0);
        }
        let f = f32::from_bits(i);
        let ff = f as f64;
        for preci in 0..=10 {
            let argv = &[preci.to_arg(), ff.to_arg()];
            let mut args = ArgList::new(argv);

            rust_str.clear();
            let _ = crate::format(rust_fmt, &mut args, crate::output::fmt_write(&mut rust_str));

            let printf_str = unsafe {
                let len = libc::snprintf(c_storage_ptr, c_storage.len(), c_fmt, preci, ff);
                assert!(len >= 0);
                let sl = std::slice::from_raw_parts(c_storage_ptr as *const u8, len as usize);
                std::str::from_utf8(sl).unwrap()
            };
            if rust_str != printf_str {
                println!(
                    "Rust and libc disagree on formatting float {i:x}: {ff}\n
                             with precision: {preci}
                              format string: {rust_fmt}
                             rust output: <{rust_str}>
                             libc output: <{printf_str}>"
                );
                assert_eq!(rust_str, printf_str);
            }
        }
    }
}

#[test]
#[ignore]
fn test_float_g_exhaustive() {
    // To run: cargo test test_float_g_exhaustive --release -- --ignored --nocapture
    test_exhaustive(utf32str!("%.*g"), b"%.*g\0".as_ptr() as *const i8);
}

#[test]
#[ignore]
fn test_float_e_exhaustive() {
    // To run: cargo test test_float_e_exhaustive --release -- --ignored --nocapture
    test_exhaustive(utf32str!("%.*e"), b"%.*e\0".as_ptr() as *const i8);
}

#[test]
fn test_str2() {
    assert_eq_fmt!(
        "test % with string: FOO yay\n",
        "test %% with string: %s yay\n",
        "FOO"
    );
    assert_eq_fmt!("test char ~", "test char %c", '~');
}

#[test]
fn test_str_concat() {
    assert_eq_fmt!("abc-def", "%s-%ls", "abc", utf32str!("def"));
}

#[test]
#[should_panic]
fn test_bad_format() {
    rust_fmt(utf32str!("%s"), &[123.to_arg()]);
}

#[test]
#[should_panic]
fn test_missing_arg() {
    rust_fmt(utf32str!("%s-%s"), &["abc".to_arg()]);
}

#[test]
#[should_panic]
fn test_too_many_args() {
    rust_fmt(utf32str!("%d"), &[1.to_arg(), 2.to_arg(), 3.to_arg()]);
}
