//! This crate provides a super-fast decimal number parser from strings into floats.
//!
//! ## Usage
//!
//! There's two top-level functions provided: [`parse`](crate::parse()) and
//! [`parse_partial`](crate::parse_partial()), both taking
//! either a string or a bytes slice and parsing the input into either `f32` or `f64`:
//!
//! - [`parse`](crate::parse()) treats the whole string as a decimal number and returns an
//!   error if there are invalid characters or if the string is empty.
//! - [`parse_partial`](crate::parse_partial()) tries to find the longest substring at the
//! beginning of the given input string that can be parsed as a decimal number and,
//! in the case of success, returns the parsed value along the number of characters processed;
//! an error is returned if the string doesn't start with a decimal number or if it is empty.
//! This function is most useful as a building block when constructing more complex parsers,
//! or when parsing streams of data.
//!
//! ## Examples
//!
//! ```rust
//! // Parse the entire string as a decimal number.
//! let s = "1.23e-02";
//! let x: f32 = fast_float::parse(s).unwrap();
//! assert_eq!(x, 0.0123);
//!
//! // Parse as many characters as possible as a decimal number.
//! let s = "1.23e-02foo";
//! let (x, n) = fast_float::parse_partial::<f32, _>(s).unwrap();
//! assert_eq!(x, 0.0123);
//! assert_eq!(n, 8);
//! assert_eq!(&s[n..], "foo");
//! ```

#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::missing_const_for_fn,
    clippy::use_self,
    clippy::module_name_repetitions,
    clippy::cargo_common_metadata
)]

use core::fmt::{self, Display};
use core::iter::FusedIterator;

mod binary;
mod common;
mod decimal;
mod float;
mod number;
mod parse;
mod simple;
mod table;

/// Iterator type that ParseFloat expects.
pub trait InputIterator: FusedIterator<Item = char> + Clone {}
impl<T> InputIterator for T where T: FusedIterator<Item = char> + Clone {}

/// A helpful function to create an InputIterator from a byte slice.
pub fn bytes_iter<S>(bytes: &S) -> impl InputIterator + '_
where
    S: AsRef<[u8]> + ?Sized,
{
    bytes.as_ref().iter().map(|&b| b as char).fuse()
}

/// Opaque error type for fast-float parsing functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Error;

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "error while parsing a float")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {
    fn description(&self) -> &str {
        "error while parsing a float"
    }
}

/// Result type alias for fast-float parsing functions.
pub type Result<T> = core::result::Result<T, Error>;

/// Trait for numerical float types that can be parsed from string.
pub trait FastFloat: float::Float {
    /// Parse a decimal number from string into float (full).
    ///
    /// # Errors
    ///
    /// Will return an error either if the string is not a valid decimal number.
    /// or if any characters are left remaining unparsed.
    #[inline]
    fn parse_float<Iter: InputIterator>(iter: Iter, decimal_sep: char) -> Result<Self> {
        match Self::parse_float_partial(iter.clone(), decimal_sep) {
            Ok((v, n)) if n == iter.count() => Ok(v),
            _ => Err(Error),
        }
    }

    /// Parse a decimal number from string into float (partial).
    ///
    /// This method parses as many characters as possible and returns the resulting number along
    /// with the number of digits processed (in case of success, this number is always positive).
    ///
    /// # Errors
    ///
    /// Will return an error either if the string doesn't start with a valid decimal number
    /// – that is, if no zero digits were processed.
    #[inline]
    fn parse_float_partial<Iter: InputIterator>(
        iter: Iter,
        decimal_sep: char,
    ) -> Result<(Self, usize)> {
        let mut chars = common::Chars::new(iter.peekable(), decimal_sep);
        parse::parse_float(&mut chars).ok_or(Error)
    }
}

impl FastFloat for f32 {}
impl FastFloat for f64 {}

/// Parse a decimal number from string into float (full).
///
/// # Errors
///
/// Will return an error either if the string is not a valid decimal number
/// or if any characters are left remaining unparsed.
#[inline]
pub fn parse<T: FastFloat, S: AsRef<[u8]>>(s: S) -> Result<T> {
    T::parse_float(bytes_iter(&s), '.')
}

/// Parse a decimal number from string into float (partial).
///
/// This function parses as many characters as possible and returns the resulting number along
/// with the number of digits processed (in case of success, this number is always positive).
///
/// # Errors
///
/// Will return an error either if the string doesn't start with a valid decimal number
/// – that is, if no zero digits were processed.
#[inline]
pub fn parse_partial<T: FastFloat, S: AsRef<[u8]>>(s: S) -> Result<(T, usize)> {
    T::parse_float_partial(bytes_iter(&s), '.')
}

/// Parse a decimal number from iterator into float (full).
///
/// # Errors
///
/// Will return an error either if the string is not a valid decimal number
/// or if any characters are left remaining unparsed.
#[inline]
pub fn parse_iter<T: FastFloat, Iter: InputIterator>(iter: Iter, decimal_sep: char) -> Result<T> {
    T::parse_float(iter, decimal_sep)
}

/// Parse a decimal number from iterator into float (partial).
///
/// This function parses as many characters as possible and returns the resulting number along
/// with the number of digits processed (in case of success, this number is always positive).
///
/// # Errors
///
/// Will return an error either if the string doesn't start with a valid decimal number
/// – that is, if no zero digits were processed.
#[inline]
pub fn parse_partial_iter<T: FastFloat, Iter: InputIterator>(
    iter: Iter,
    decimal_sep: char,
) -> Result<(T, usize)> {
    T::parse_float_partial(iter, decimal_sep)
}
