/*
 * Implementation of hex float parsing.
 * Floating suffixes (f/l/F/L) are not supported.
 *
 * Grammar:
 *
 * hexadecimal-floating-constant:
 *     hexadecimal-prefix hexadecimal-fractional-constant
 *         binary-exponent-part
 *
 *     hexadecimal-prefix hexadecimal-digit-sequence
 *         binary-exponent-part
 *
 *     hexadecimal-fractional-constant
 *         hexadecimal-digit-sequence_opt . hexadecimal-digit-sequence
 *         hexadecimal-digit-sequence .
 *
 *      binary-exponent-part:
 *            p sign_opt digit-sequence
 *            P sign_opt digit-sequence
 *
 *     hexadecimal-digit-sequence:
 *            hexadecimal-digit
 *            hexadecimal-digit-sequence hexadecimal-digit
 *
 * Note this omits an optional leading sign (+ or -).
 *
 * Hex digits may be lowercase or uppercase. The exponent is a power of 2.
 */

/// Error type for parsing a hexadecimal floating-point number.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum Error {
    /* Missing prefix, or exponent marker without exponent. */
    SyntaxError,

    /* Exponent overflows. */
    Overflow,
}

/// Parses a hexadecimal floating-point number from a character iterator.
///
/// # Arguments
///
/// * `chars` - An iterator over characters representing the hexadecimal float.
///
/// # Returns
///
/// A `Result` containing either:
/// - A tuple of the parsed floating-point number (`f64`) and the number of characters consumed (`usize`), or
/// - An `Error` if the parsing fails.
///
/// # Examples
///
/// ```
/// let input = "1A.3p4".chars();
/// let result = parse_hex_float(input);
/// assert!(result.is_ok());
/// ```
pub(super) fn parse_hex_float(chars: impl Iterator<Item = char>) -> Result<(f64, usize), Error> {
    const F64_EXP_BIAS: i32 = 1023;
    let mut chars = chars.peekable();
    let mut consumed = 0;
    // Parse sign?
    let negative = match chars.peek() {
        Some(&'+') | Some(&'-') => {
            consumed += 1;
            chars.next() == Some('-')
        }
        _ => false,
    };
    // Make a value of 1.0 or -1.0 for later.
    let sign = if negative { -1.0 } else { 1.0 };

    // Parse hex prefix.
    match (chars.next(), chars.next()) {
        (Some('0'), Some('x' | 'X')) => consumed += 2,
        _ => return Err(Error::SyntaxError),
    }

    // Parse a sequence of hex digits.
    // Keep track of how many - we trim leading zeros so this isn't apparent from the digits vector.
    let mut digits_count = 0;
    let mut digits: Vec<u8> = Vec::new();
    while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
        digits_count += 1;
        // Skip leading 0s.
        if !digits.is_empty() || d != 0 {
            digits.push(d as u8);
        }
        chars.next();
    }

    // Record the number of digits before the decimal (if any).
    let decimal_point_pos: i32 = digits.len().try_into().map_err(|_| Error::Overflow)?;

    // Optionally parse a decimal and another sequence.
    // If we have no decimal, pretend it's here anyways.
    if chars.peek() == Some(&'.') {
        chars.next();
        consumed += 1;
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
            digits_count += 1;
            digits.push(d as u8);
            chars.next();
        }
    }

    // Must have at least one.
    if digits_count == 0 {
        return Err(Error::SyntaxError);
    }
    consumed += digits_count;

    // Try parsing the explicit exponent.
    let mut explicit_exp: i32 = 0;
    if matches!(chars.peek(), Some('p') | Some('P')) {
        chars.next();
        consumed += 1;
        // Exponent sign?
        let negative = match chars.peek() {
            Some('+') | Some('-') => {
                consumed += 1;
                chars.next() == Some('-')
            }
            _ => false,
        };
        // Decimal digit sequence.
        let before = consumed;
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
            explicit_exp = explicit_exp
                .checked_mul(10)
                .and_then(|exp| exp.checked_add(d as i32))
                .ok_or(Error::Overflow)?;
            consumed += 1;
            chars.next();
        }
        // Need at least one digit.
        if consumed == before {
            return Err(Error::SyntaxError);
        }
        // Negating a non-negative value cannot overflow.
        if negative {
            explicit_exp = -explicit_exp;
        }
    }

    // Construct mantissa.
    let mut mantissa: u64 = 0;
    let mut shift = 64;
    for d in digits {
        shift -= 4;
        mantissa |= (d as u64) << shift;
        if shift == 0 {
            // Possible excess precision in the mantissa; ignore it.
            break;
        }
    }
    // Handle a zero mantissa.
    if mantissa == 0 {
        return Ok((0.0f64.copysign(sign), consumed));
    }

    // Normalize to leading 1.
    // The number of zeros, plus one, is subtracted from the exponent.
    // Plus one because hex 0001 should have an exponent of 0.
    let zeros = mantissa.leading_zeros();
    mantissa <<= zeros;

    // Compute the exponent (base 2).
    // This has contributions from the explicit exponent,
    // hex digits (e.g. 0x1000p0 has an exponent of 8), and leading zeros.
    let exponent = decimal_point_pos
        .checked_mul(4)
        .and_then(|exp| exp.checked_add(explicit_exp))
        .and_then(|exp| exp.checked_sub(1 + zeros as i32))
        .ok_or(Error::Overflow)?;

    // Return infinity if we exceed the max exponent, or zero if we are smaller than the min exponent.
    if exponent > 1023 {
        return Ok((f64::INFINITY.copysign(sign), consumed));
    } else if exponent < -1022 {
        // TODO: denormal.
        return Ok((0.0f64.copysign(sign), consumed));
    }
    let biased_exp: u64 = (exponent + F64_EXP_BIAS).try_into().unwrap();

    // Construct our float: sign, exponent, mantissa.
    // Note we do not bother to round the mantissa.
    let mut bits: u64 = 0;
    bits |= (negative as u64) << 63;
    bits |= biased_exp << 52;
    mantissa <<= 1; // Trim implicit 1 bit from mantissa.
    bits |= mantissa >> (64 - 52);
    Ok((f64::from_bits(bits), consumed))
}

#[test]
fn test_parse_hex_float_valid() {
    let parse = |input: &str| {
        let res =
            parse_hex_float(input.chars()).expect(format!("Failed to parse {}", input).as_str());
        // We expect to consume the entire string.
        assert_eq!(res.1, input.len());
        res.0
    };
    assert_eq!(parse("0x0"), 0.0);
    assert_eq!(parse("0X0"), 0.0);
    assert_eq!(parse("0X000"), 0.0);
    assert_eq!(parse("0X0000.8"), 0.5);
    assert_eq!(parse("0x1"), 1.0);
    assert_eq!(parse("0x1p0"), 1.0);
    assert_eq!(parse("0x1P0"), 1.0);
    assert_eq!(parse("0x1.8p1"), 3.0);
    assert_eq!(parse("0x2p2"), 8.0);
    assert_eq!(parse("0x1.8"), 1.5);
    assert_eq!(parse("0x1.2p3"), 9.0);
    assert_eq!(parse("0x10p-1"), 8.0);
    assert_eq!(parse("0x1.p1"), 2.0);
    assert_eq!(parse("0x.8p0"), 0.5);
    assert_eq!(parse("0x.1p4"), 1.0);
    assert_eq!(parse("0x2"), 2.0);
    assert_eq!(parse("0x2P1"), 4.0);
    assert_eq!(parse("0x2.4"), 2.25);
    assert_eq!(parse("0x2.4p2"), 9.0);
    assert_eq!(parse("0x3p-2"), 0.75);
    assert_eq!(parse("0x4p-3"), 0.5);
    assert_eq!(parse("0x5"), 5.0);
    assert_eq!(parse("0x5p1"), 10.0);
    assert_eq!(parse("0x5.1p0"), 5.0625);
    assert_eq!(parse("0x5.1p1"), 10.125);
    assert_eq!(parse("0x8"), 8.0);
    assert_eq!(parse("0x8p0"), 8.0);
    assert_eq!(parse("0x8.8"), 8.5);
    assert_eq!(parse("0x9"), 9.0);
    assert_eq!(parse("0x9p-1"), 4.5);
    assert_eq!(parse("0xA"), 10.0);
    assert_eq!(parse("0xAp1"), 20.0);
    assert_eq!(parse("0xB"), 11.0);
    assert_eq!(parse("0xBp-1"), 5.5);
    assert_eq!(parse("0xC"), 12.0);
    assert_eq!(parse("0xCp2"), 48.0);
    assert_eq!(parse("0xF"), 15.0);
    assert_eq!(parse("0xFp-2"), 3.75);
    assert_eq!(parse("0x10"), 16.0);
    assert_eq!(parse("0x10p-4"), 1.0);
    assert_eq!(parse("0x1A"), 26.0);
    assert_eq!(parse("0x1Ap3"), 208.0);
    assert_eq!(parse("0x1F"), 31.0);
    assert_eq!(parse("0x1Fp1"), 62.0);
    assert_eq!(parse("0x20"), 32.0);
    assert_eq!(parse("0x20p-5"), 1.0);
}

#[test]
fn test_parse_hex_float_errors() {
    let syntax_error = Err(Error::SyntaxError);
    assert_eq!(parse_hex_float("".chars()), syntax_error);
    assert_eq!(parse_hex_float("0xZ".chars()), syntax_error);
    assert_eq!(parse_hex_float("1A3P1.1p2".chars()), syntax_error);
    assert_eq!(parse_hex_float("1A3G.1p2".chars()), syntax_error);
}
