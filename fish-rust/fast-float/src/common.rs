use crate::InputIterator;
use core::convert::TryInto;
use core::iter::Peekable;
use core::ptr;

#[derive(Clone)]
pub struct Chars<Iter: InputIterator> {
    chars: Peekable<Iter>,
    consumed: usize,
    decimal_sep: char,
}

impl<Iter: InputIterator> Chars<Iter> {
    pub fn new(iter: Iter, decimal_sep: char) -> Self {
        Self {
            chars: iter.peekable(),
            consumed: 0,
            decimal_sep,
        }
    }

    #[inline]
    pub fn get_consumed(&self) -> usize {
        self.consumed
    }

    #[inline]
    pub fn get_decimal_sep(&self) -> char {
        self.decimal_sep
    }

    #[inline]
    pub fn clone_iter(&self) -> Peekable<Iter> {
        self.chars.clone()
    }

    #[inline]
    pub fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    /// If the next character is an ASCII digit, return its value (0-9).
    /// Otherwise, return None.
    #[inline]
    pub fn peek_digit(&mut self) -> Option<u8> {
        let c = self.peek()?;
        if c.is_ascii_digit() {
            Some(c as u8 - b'0')
        } else {
            None
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.chars.clone().count()
    }

    #[inline]
    pub fn step_by(&mut self, n: usize) -> &mut Self {
        if n > 0 {
            self.chars.nth(n - 1);
            self.consumed += n;
        }
        self
    }

    #[inline]
    pub fn step(&mut self) -> &mut Self {
        self.step_by(1)
    }

    #[inline]
    pub fn is_empty(&mut self) -> bool {
        self.peek().is_none()
    }

    #[inline]
    pub fn first(&mut self) -> char {
        self.peek().unwrap()
    }

    #[inline]
    pub fn first_is(&mut self, c: u8) -> bool {
        self.first() == c.into()
    }

    #[inline]
    pub fn first_either(&mut self, c1: u8, c2: u8) -> bool {
        let c = self.first();
        c == c1.into() || c == c2.into()
    }

    #[inline]
    pub fn check_first(&mut self, c: char) -> bool {
        self.peek() == Some(c.into())
    }

    #[inline]
    pub fn check_first_either(&mut self, c1: char, c2: char) -> bool {
        let first = self.peek();
        first == Some(c1.into()) || first == Some(c2.into())
    }

    #[inline]
    pub fn check_first_digit(&mut self) -> bool {
        if let Some(c) = self.peek() {
            c.is_ascii_digit()
        } else {
            false
        }
    }

    #[inline]
    pub fn parse_digits(&mut self, mut func: impl FnMut(u8)) {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                func(c as u8 - b'0');
                self.step();
            } else {
                break;
            }
        }
    }

    #[inline]
    pub fn try_read_u64(&self) -> Option<u64> {
        // This historically read a u64 in little endian: the first char should be in the LSB.
        // We emulate this by mapping chars above 0xFF to 0.
        // Note this does not advance us.
        let mut result: u64 = 0;
        let mut iter = self.chars.clone();
        for i in 0..8 {
            let c = iter.next()?;
            let c8: u8 = c.try_into().unwrap_or(0);
            result |= (c8 as u64) << (i * 8);
        }
        Some(result)
    }

    #[inline]
    pub fn read_u64(&self) -> u64 {
        self.try_read_u64().unwrap()
    }

    #[inline]
    pub fn offset_from(&self, other: &Self) -> isize {
        self.consumed.wrapping_sub(other.consumed) as isize // assuming the same end
    }

    // Note this is only called with lowercase inputs.
    #[inline]
    pub fn eq_ignore_case(&self, u: &[u8]) -> bool {
        let mut iter = self.chars.clone();
        for &c in u {
            debug_assert!(c.is_ascii_lowercase());
            let lowc: char = c.into();
            let upc: char = c.to_ascii_uppercase().into();
            let matches = iter
                .next()
                .map_or(false, |mine| mine == lowc || mine == upc);
            if !matches {
                return false;
            }
        }
        true
    }

    #[inline]
    pub fn get_first(&mut self) -> char {
        self.peek().unwrap()
    }

    // The following were factored from ByteSlice.

    #[inline]
    pub fn advance(&mut self, n: usize) -> &mut Self {
        self.step_by(n)
    }

    #[inline]
    pub fn skip_chars(&mut self, c: char) {
        while self.peek() == Some(c) {
            self.step();
        }
    }
}

// Most of these are inherently unsafe; we assume we know what we're calling and when.
pub trait ByteSlice: AsRef<[u8]> + AsMut<[u8]> {
    #[inline]
    fn write_u64(&mut self, value: u64) {
        debug_assert!(self.as_ref().len() >= 8);
        let dst = self.as_mut().as_mut_ptr() as *mut u64;
        unsafe { ptr::write_unaligned(dst, u64::to_le(value)) };
    }
}

impl ByteSlice for [u8] {}

#[inline]
pub fn is_8digits(v: u64) -> bool {
    let a = v.wrapping_add(0x4646_4646_4646_4646);
    let b = v.wrapping_sub(0x3030_3030_3030_3030);
    (a | b) & 0x8080_8080_8080_8080 == 0
}

#[inline]
pub fn parse_digits<Iter: InputIterator>(s: &mut Chars<Iter>, mut f: impl FnMut(u8)) {
    while !s.is_empty() {
        let c = (s.get_first() as u32).wrapping_sub('0' as u32);
        if c < 10 {
            f(c as u8);
            s.advance(1);
        } else {
            break;
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct AdjustedMantissa {
    pub mantissa: u64,
    pub power2: i32,
}

impl AdjustedMantissa {
    #[inline]
    pub const fn zero_pow2(power2: i32) -> Self {
        Self {
            mantissa: 0,
            power2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes_iter;

    #[test]
    fn test_read_write_u64() {
        let bytes = b"01234567";
        let iter = bytes_iter(bytes);
        let chars = Chars::new(iter, '.');
        let int = chars.read_u64();
        assert_eq!(int, 0x3736353433323130);

        let int = chars.read_u64();
        assert_eq!(int, 0x3736353433323130);

        let mut slc = [0u8; 8];
        slc.write_u64(0x3736353433323130);
        assert_eq!(&slc, bytes);
    }
}
