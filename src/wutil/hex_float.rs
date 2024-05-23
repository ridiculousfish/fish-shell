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
    let mut digits: Vec<u8> = Vec::new();
    while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
        // Skip leading 0s.
        if !digits.is_empty() || d != 0 {
            digits.push(d as u8);
        }
        chars.next();
        consumed += 1;
    }
    // Must have at least one.
    if digits.is_empty() {
        return Err(Error::SyntaxError);
    }
    // Optionally parse a decimal and another sequence.
    // If we have no decimal, pretend it's here anyways.
    let decimal_point_pos: i32 = digits.len().try_into().map_err(|_| Error::Overflow)?;
    if chars.peek() == Some(&'.') {
        chars.next();
        consumed += 1;
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
            digits.push(d as u8);
            chars.next();
            consumed += 1;
        }
    }

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
    let zeros = mantissa.leading_zeros();
    mantissa <<= zeros;

    // Compute the exponent (base 2).
    // This has contributions from the explicit exponent,
    // hex digits (e.g. 0x1000p0 has an exponent of 8), and leading zeros.
    let exponent = decimal_point_pos
        .checked_mul(4)
        .and_then(|exp| exp.checked_add(explicit_exp))
        .and_then(|exp| exp.checked_sub(zeros as i32))
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
    bits |= (sign as u64) << 63;
    bits |= biased_exp << 52;
    bits |= mantissa >> (64 - 52);
    Ok(f64::from_bits(bits), consumed)
}

#[test]
fn test_parse_hex_float() {
    let syntaxError = Err(Error::SyntaxError);
    assert_eq!(parse_hex_float("".chars()), syntaxError);
    assert_eq!(parse_hex_float("1A3P1.1p2".chars()), Ok((4099.25, 9)));
    assert_eq!(parse_hex_float("1A3G.1p2".chars()), syntaxError);
}
