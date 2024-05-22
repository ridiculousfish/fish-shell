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
 * Hex digits may be lowercase or uppercase. The exponent is a power of 2.
 */

/* Error type for hex parsing. */
pub(super) enum Error {
    /* Missing prefix, or exponent marker without exponent. */
    SyntaxError,

    /* Exponent overflows. */
    Overflow,
}

/* Parse a hex float from a char iterator.

Return either an error, or a tuple of the value and number of chars consumed.
*/
pub(super) fn parse_hex_float(chars: impl Iterator<Item = char>) -> Result<(f64, usize), Error> {
    let chars = chars.peekable();
    let mut consumed = 0;
    // Parse hex prefix.
    match chars.next() {
        Some('0') => consumed += 1,
        _ => return Err(Error::SyntaxError),
    }
    match chars.next() {
        Some('x') | Some('X') => consumed += 1,
        _ => return Err(Error::SyntaxError),
    }
    // Parse a sequence of hex digits.
    let mut digits: Vec<u8> = Vec::new();
    while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
        digits.push(d as u8);
        chars.next();
        consumed += 1;
    }
    // Must have at least one.
    if digits.is_empty() {
        return Err(Error::SyntaxError);
    }
    // Optionally parse a decimal and another sequence.
    // If we have no decimal, pretend it's here anyways.
    let decimal_point_pos = digits.len();
    if chars.peek() == Some('.') {
        chars.next();
        consumed += 1;
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
            digits.push(d as u8);
            chars.next();
            consumed += 1;
        }
    }

    // Try parsing a base-2 exponent.
    let mut exponent: i32 = 0;
    if matches!(chars.peek(), Some('p') | Some('P')) {
        chars.next();
        consumed += 1;
        // Sign?
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
            exponent = exponent.checked_mul(10).ok_or(Error::Overflow)?;
            exponent = exponent.checked_add(d as i32).ok_or(Error::Overflow)?;
            consumed += 1;
            chars.next();
        }
        // Need at least one.
        if consumed == before {
            return Err(Error::SyntaxError);
        }
    }
}
