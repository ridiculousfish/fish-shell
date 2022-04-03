#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("fish.h");
        include!("fds.h");
        include!("wutil.h");

        fn make_pipes_ffi(read: &mut i32, write: &mut i32);

        fn make_fd_nonblocking(fd: i32) -> i32;

        fn wperror(msg: *const wchar_t);

        type wcstring;
        fn str2wcstring_ffi(inp: *const c_char, len: usize) -> UniquePtr<wcstring>;
    }
}

pub use ffi::wcstring;
pub use libc::c_char;
pub type wcstring_ptr = cxx::UniquePtr<wcstring>;
pub fn str2wcstring_ffi(s: &str) -> wcstring_ptr {
    ffi::str2wcstring_ffi(s.as_ptr() as *const c_char, s.len())
}
