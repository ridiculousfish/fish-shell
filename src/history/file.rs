//! Implementation of backing files.

use crate::{
    path::{DirRemoteness, path_get_data_remoteness},
    wutil::FileId,
};
use nix::errno::Errno;
use nix::sys::mman::{MapFlags, ProtFlags};
use std::num::NonZeroUsize;
use std::ptr::NonNull;
use std::{
    fs::File,
    io::Read as _,
    ops::{Deref, DerefMut},
    time::{SystemTime, UNIX_EPOCH},
};

/// A type wrapping up the logic around mmap and munmap.
pub struct MmapRegion {
    ptr: NonNull<u8>,
    len: NonZeroUsize,
}

impl MmapRegion {
    /// Map a region `[0, len)` from a locked file.
    fn map_file(file: &File, len: NonZeroUsize) -> nix::Result<Self> {
        let ptr = unsafe {
            nix::sys::mman::mmap(
                None,
                len,
                ProtFlags::PROT_READ,
                MapFlags::MAP_PRIVATE,
                file,
                0,
            )
        }?;

        Ok(Self {
            ptr: ptr.cast(),
            len,
        })
    }

    /// Map anonymous memory of a given length.
    pub fn map_anon(len: NonZeroUsize) -> nix::Result<Self> {
        let ptr = unsafe {
            nix::sys::mman::mmap_anonymous(
                None,
                len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_PRIVATE,
            )
        }?;

        Ok(Self {
            ptr: ptr.cast(),
            len,
        })
    }
}

// SAFETY: MmapRegion has exclusive mutable access to the region
unsafe impl Send for MmapRegion {}
// SAFETY: MmapRegion does not offer interior mutability
unsafe impl Sync for MmapRegion {}

impl Deref for MmapRegion {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len.get()) }
    }
}

impl DerefMut for MmapRegion {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len.get()) }
    }
}

impl Drop for MmapRegion {
    fn drop(&mut self) {
        let _ = unsafe { nix::sys::mman::munmap(self.ptr.cast(), self.len.get()) };
    }
}

impl AsRef<[u8]> for MmapRegion {
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

/// Check if we should mmap the file.
/// Don't try mmap() on non-local filesystems.
fn should_mmap() -> bool {
    // mmap only if we are known not-remote.
    path_get_data_remoteness() != DirRemoteness::Remote
}

/// Construct a history file contents from a [`File`] reference and its file id.
pub fn load_raw_history_file(
    history_file: &File,
    file_id: FileId,
) -> std::io::Result<MmapRegion> {
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
    let Ok(len) = NonZeroUsize::try_from(len) else {
        return Err(std::io::Error::other(
            "History file is empty. Cannot create memory mapping with length 0.",
        ));
    };
    let map_anon = |mut file: &File, len: NonZeroUsize| -> std::io::Result<MmapRegion> {
        let mut region = MmapRegion::map_anon(len)?;
        // If we mapped anonymous memory, we have to read from the file.
        file.read_exact(&mut region)?;
        Ok(region)
    };
    let region = if should_mmap() {
        match MmapRegion::map_file(history_file, len) {
            Ok(region) => region,
            Err(err) => {
                if err == Errno::ENODEV {
                    // Our mmap failed with ENODEV, which means the underlying
                    // filesystem does not support mapping.
                    // Create an anonymous mapping and read() the file into it.
                    map_anon(history_file, len)?
                } else {
                    return Err(std::io::Error::from(err));
                }
            }
        }
    } else {
        map_anon(history_file, len)?
    };

    Ok(region)
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
