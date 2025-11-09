//! Implementation of backing files.

use crate::{
    path::{DirRemoteness, path_get_data_remoteness},
    wutil::FileId,
};
use libc::{ENODEV, MAP_ANONYMOUS, MAP_FAILED, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use std::{
    fs::File,
    io::Read,
    os::fd::AsRawFd,
    time::{SystemTime, UNIX_EPOCH},
};

/// A type wrapping up the logic around mmap and munmap.
pub struct MmapRegion {
    ptr: *mut u8,
    len: usize,
}

impl MmapRegion {
    /// Creates a new mmap'ed region.
    ///
    /// # Safety
    ///
    /// `ptr` must be the result of a successful `mmap()` call with length `len`.
    unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        assert!(ptr.cast() != MAP_FAILED);
        assert!(len > 0);
        Self { ptr, len }
    }

    /// Map a region `[0, len)` from a locked file.
    pub fn map_file(file: &File, len: usize) -> std::io::Result<Self> {
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                PROT_READ,
                MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };

        if ptr == MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }

        // SAFETY: mmap of `len` was successful and returned `ptr`
        Ok(unsafe { Self::new(ptr.cast(), len) })
    }

    /// Map anonymous memory of a given length.
    pub fn map_anon(len: usize) -> std::io::Result<Self> {
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }

        // SAFETY: mmap of `len` was successful and returned `ptr`
        Ok(unsafe { Self::new(ptr.cast(), len) })
    }

    /// Get an immutable view of the mapped memory as a byte slice.
    pub fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Get a mutable view of the mapped memory as a byte slice.
    /// Only available for writable mappings (e.g., anonymous mappings).
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

// SAFETY: MmapRegion has exclusive mutable access to the region
unsafe impl Send for MmapRegion {}
// SAFETY: MmapRegion does not offer interior mutability
unsafe impl Sync for MmapRegion {}

impl Drop for MmapRegion {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr.cast(), self.len) };
    }
}

impl AsRef<[u8]> for MmapRegion {
    fn as_ref(&self) -> &[u8] {
        self.bytes()
    }
}

/// Map a history file into memory from a [`File`] reference and its file id.
pub fn map_file(history_file: &File, file_id: FileId) -> Result<MmapRegion, std::io::Error> {
    // Check the file size.
    let len: usize = match file_id.size.try_into() {
        Ok(len) => len,
        Err(err) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("Cannot convert u64 to usize: {err}"),
            ));
        }
    };
    if len == 0 {
        return Err(std::io::Error::other(
            "History file is empty. Cannot create memory mapping with length 0.",
        ));
    }
    let map_anon = |mut file: &File, len: usize| -> std::io::Result<MmapRegion> {
        let mut region = MmapRegion::map_anon(len)?;
        // If we mapped anonymous memory, we have to read from the file.
        file.read_exact(region.bytes_mut())?;
        Ok(region)
    };
    let region = if should_mmap() {
        match MmapRegion::map_file(history_file, len) {
            Ok(region) => region,
            Err(err) => {
                if err.raw_os_error() == Some(ENODEV) {
                    // Our mmap failed with ENODEV, which means the underlying
                    // filesystem does not support mapping.
                    // Create an anonymous mapping and read() the file into it.
                    map_anon(history_file, len)?
                } else {
                    return Err(err);
                }
            }
        }
    } else {
        map_anon(history_file, len)?
    };

    Ok(region)
}

/// Check if we should mmap the file.
/// Don't try mmap() on non-local filesystems.
fn should_mmap() -> bool {
    // mmap only if we are known not-remote.
    path_get_data_remoteness() != DirRemoteness::Remote
}

pub fn time_to_seconds(ts: SystemTime) -> i64 {
    match ts.duration_since(UNIX_EPOCH) {
        Ok(d) => {
            // after epoch
            i64::try_from(d.as_secs()).unwrap()
        }
        Err(e) => {
            // before epoch
            -i64::try_from(e.duration().as_secs()).unwrap()
        }
    }
}
