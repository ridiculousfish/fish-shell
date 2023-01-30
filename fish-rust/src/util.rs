//! Generic utilities library.
use std::time;

use crate::ffi::wcharz_t;
#[cfg(test)]
use crate::wchar::widestrs;
use crate::wchar::wstr;

#[cxx::bridge]
mod ffi {
    extern "C++" {
        include!("wutil.h");
        type wcharz_t = super::wcharz_t;
    }

    extern "Rust" {
        fn wcsfilecmp(a: wcharz_t, b: wcharz_t) -> i32;
        fn wcsfilecmp_glob(a: wcharz_t, b: wcharz_t) -> i32;
        fn get_time() -> u64;
    }
}

/// Compares two wide character strings with an (arguably) intuitive ordering. This function tries
/// to order strings in a way which is intuitive to humans with regards to sorting strings
/// containing numbers.
///
/// Most sorting functions would sort the strings 'file1.txt' 'file5.txt' and 'file12.txt' as:
///
/// file1.txt
/// file12.txt
/// file5.txt
///
/// This function regards any sequence of digits as a single entity when performing comparisons, so
/// the output is instead:
///
/// file1.txt
/// file5.txt
/// file12.txt
///
/// Which most people would find more intuitive.
///
/// This won't return the optimum results for numbers in bases higher than ten, such as hexadecimal,
/// but at least a stable sort order will result.
///
/// This function performs a two-tiered sort, where difference in case and in number of leading
/// zeroes in numbers only have effect if no other differences between strings are found. This way,
/// a 'file1' and 'File1' will not be considered identical, and hence their internal sort order is
/// not arbitrary, but the names 'file1', 'File2' and 'file3' will still be sorted in the order
/// given above.
pub fn wcsfilecmp(a: wcharz_t, b: wcharz_t) -> i32 {
    wcsfilecmp_(a.into(), b.into())
}
// TODO This should return `std::cmp::Ordering`.
pub fn wcsfilecmp_(a: &wstr, b: &wstr) -> i32 {
    let mut retval = 0;
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.len() && bi < b.len() {
        let ac = a.as_char_slice()[ai];
        let bc = b.as_char_slice()[bi];
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let (ad, bd);
            (retval, ad, bd) = wcsfilecmp_leading_digits(&a[ai..], &b[bi..]);
            ai += ad;
            bi += bd;
            if retval != 0 || ai == a.len() || bi == b.len() {
                break;
            }
            continue;
        }

        // Fast path: Skip towupper.
        if ac == bc {
            ai += 1;
            bi += 1;
            continue;
        }

        // Sort dashes after Z - see #5634
        let mut acl = if ac == '-' { '[' } else { ac };
        let mut bcl = if bc == '-' { '[' } else { bc };
        // TODO Compare the tail (enabled by Rust's Unicode support).
        acl = acl.to_uppercase().next().unwrap();
        bcl = bcl.to_uppercase().next().unwrap();

        if acl < bcl {
            retval = -1;
            break;
        } else if acl > bcl {
            retval = 1;
            break;
        } else {
            ai += 1;
            bi += 1;
        }
    }

    if retval != 0 {
        return retval; // we already know the strings aren't logically equal
    }

    if ai == a.len() {
        if bi == b.len() {
            // The strings are logically equal. They may or may not be the same length depending on
            // whether numbers were present but that doesn't matter. Disambiguate strings that
            // differ by letter case or length. We don't bother optimizing the case where the file
            // names are literally identical because that won't occur given how this function is
            // used. And even if it were to occur (due to being reused in some other context) it
            // would be so rare that it isn't worth optimizing for.
            match a.cmp(b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        } else {
            -1 // string a is a prefix of b and b is longer
        }
    } else {
        assert!(bi == b.len());
        return 1; // string b is a prefix of a and a is longer
    }
}

/// wcsfilecmp, but frozen in time for glob usage.
pub fn wcsfilecmp_glob(a: wcharz_t, b: wcharz_t) -> i32 {
    wcsfilecmp_glob_(a.into(), b.into())
}
pub fn wcsfilecmp_glob_(a: &wstr, b: &wstr) -> i32 {
    let mut retval = 0;
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.len() && bi < b.len() {
        let ac = a.as_char_slice()[ai];
        let bc = b.as_char_slice()[bi];
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let (ad, bd);
            (retval, ad, bd) = wcsfilecmp_leading_digits(&a[ai..], &b[bi..]);
            ai += ad;
            bi += bd;
            // If we know the strings aren't logically equal or we've reached the end of one or both
            // strings we can stop iterating over the chars in each string.
            if retval != 0 || ai == a.len() || bi == b.len() {
                break;
            }
            continue;
        }

        // Fast path: Skip towlower.
        if ac == bc {
            ai += 1;
            bi += 1;
            continue;
        }

        // TODO Compare the tail (enabled by Rust's Unicode support).
        let acl = ac.to_lowercase().next().unwrap();
        let bcl = bc.to_lowercase().next().unwrap();
        if acl < bcl {
            retval = -1;
            break;
        } else if acl > bcl {
            retval = 1;
            break;
        } else {
            ai += 1;
            bi += 1;
        }
    }

    if retval != 0 {
        return retval; // we already know the strings aren't logically equal
    }

    if ai == a.len() {
        if bi == b.len() {
            // The strings are logically equal. They may or may not be the same length depending on
            // whether numbers were present but that doesn't matter. Disambiguate strings that
            // differ by letter case or length. We don't bother optimizing the case where the file
            // names are literally identical because that won't occur given how this function is
            // used. And even if it were to occur (due to being reused in some other context) it
            // would be so rare that it isn't worth optimizing for.
            match a.cmp(b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        } else {
            -1 // string a is a prefix of b and b is longer
        }
    } else {
        assert!(bi == b.len());
        return 1; // string b is a prefix of a and a is longer
    }
}

/// Get the current time in microseconds since Jan 1, 1970.
pub fn get_time() -> u64 {
    time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// Compare the strings to see if they begin with an integer that can be compared and return the
// result of that comparison.
fn wcsfilecmp_leading_digits(a: &wstr, b: &wstr) -> (i32, usize, usize) {
    // Ignore leading 0s.
    let mut ai = a.as_char_slice().iter().take_while(|c| **c == '0').count();
    let mut bi = b.as_char_slice().iter().take_while(|c| **c == '0').count();

    let mut ret = 0;
    loop {
        let ac = a.as_char_slice().get(ai).unwrap_or(&'\0');
        let bc = b.as_char_slice().get(bi).unwrap_or(&'\0');
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            // We keep the cmp value for the
            // first differing digit.
            //
            // If the numbers have the same length, that's the value.
            if ret == 0 {
                // Comparing the string value is the same as numerical
                // for wchar_t digits!
                if ac > bc {
                    ret = 1;
                }
                if bc > ac {
                    ret = -1;
                }
            }
        } else {
            // We don't have negative numbers and we only allow ints,
            // and we have already skipped leading zeroes,
            // so the longer number is larger automatically.
            if ac.is_ascii_digit() {
                ret = 1;
            }
            if bc.is_ascii_digit() {
                ret = -1;
            }
            break;
        }
        ai += 1;
        bi += 1;
    }

    // For historical reasons, we skip trailing whitespace
    // like fish_wcstol does!
    // This is used in sorting globs, and that's supposed to be stable.
    ai += a
        .as_char_slice()
        .iter()
        .skip(ai)
        .take_while(|c| c.is_whitespace())
        .count();
    bi += b
        .as_char_slice()
        .iter()
        .skip(bi)
        .take_while(|c| c.is_whitespace())
        .count();
    (ret, ai, bi)
}

#[cfg(test)]
#[widestrs]
mod tests {
    use super::*;
    #[test]
    fn test_wcsfilecmp() {
        assert_eq!(wcsfilecmp_("abc12"L, "abc5"L), 1);
    }
    #[test]
    fn test_wcsfilecmp_glob() {
        assert_eq!(wcsfilecmp_glob_("alpha.txt"L, "beta.txt"L), -1);
    }
}
