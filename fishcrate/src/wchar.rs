pub use wchar::wchar_t;

/// Creates a null-terminated wide-char string, like the "L" prefix of C++.
macro_rules! L {
    ($string:literal) => {
        wchar::wchz!($string).as_ptr()
    };
}
pub(crate) use L;

/// A a pointer to a null-terminated wide-char string.
pub type wcharz_ptr_t = *const wchar_t;
