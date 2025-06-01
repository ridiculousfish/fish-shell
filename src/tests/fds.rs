use crate::fds::{
    make_autoclose_pipes, sync_fchdir, sync_fchdir_lock, sync_fchdir_lock_count, ChdirLock,
    FIRST_HIGH_FD,
};
use crate::global_safety::RelaxedAtomicBool;
use crate::tests::prelude::*;
use libc::{FD_CLOEXEC, F_GETFD};
use std::ffi::OsStr;
use std::fs::canonicalize;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
#[serial]
fn test_pipes() {
    let _cleanup = test_init();
    // Here we just test that each pipe has CLOEXEC set and is in the high range.
    // Note pipe creation may fail due to fd exhaustion; don't fail in that case.
    let mut pipes = vec![];
    for _i in 0..10 {
        if let Ok(pipe) = make_autoclose_pipes() {
            pipes.push(pipe);
        }
    }
    for pipe in pipes {
        for fd in [&pipe.read, &pipe.write] {
            let fd = fd.as_raw_fd();
            assert!(fd >= FIRST_HIGH_FD);
            let flags = unsafe { libc::fcntl(fd, F_GETFD, 0) };
            assert!(flags >= 0);
            assert!(flags & FD_CLOEXEC != 0);
        }
    }
}

#[test]
#[serial]
fn test_sync_fchdir() {
    // Create a temp directory and return an `OwnedFd` for it, and its path.
    fn tempdir() -> (Arc<OwnedFd>, PathBuf) {
        use std::os::unix::ffi::OsStrExt;
        let mut template = *b"/tmp/fish_test_sync_fchdir.XXXXXX\0";
        let raw = unsafe { libc::mkdtemp(template.as_mut_ptr().cast()) };
        assert!(!raw.is_null(), "mkdtemp failed");

        let path = canonicalize(PathBuf::from(OsStr::from_bytes(unsafe {
            std::ffi::CStr::from_ptr(raw).to_bytes()
        })))
        .expect("Failed to canonicalize temp dir path");

        let fd = unsafe {
            OwnedFd::from_raw_fd(libc::open(
                raw,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            ))
        };
        assert!(fd.as_raw_fd() >= 0, "open failed");
        (Arc::new(fd), path)
    }

    let saved_cwd = std::env::current_dir().unwrap();

    let (dir1, path1) = tempdir();
    let (dir2, path2) = tempdir();
    let (dir1, dir2) = (&dir1, &dir2);

    // We can just fchdir without locking, no problem.
    sync_fchdir(dir1).expect("Failed to fchdir");
    assert_eq!(std::env::current_dir().unwrap(), path1);
    sync_fchdir(dir2).expect("Failed to fchdir");
    assert_eq!(std::env::current_dir().unwrap(), path2);

    // Take the lock.
    let lock1: ChdirLock = sync_fchdir_lock(dir1).expect("Failed to fchdir with lock");
    assert_eq!(sync_fchdir_lock_count(), 1);
    assert_eq!(std::env::current_dir().unwrap(), path1);

    // We can take a second lock for the same directory, it is uncontended.
    let lock2: ChdirLock = sync_fchdir_lock(dir1).expect("Failed to fchdir with second lock");
    assert_eq!(sync_fchdir_lock_count(), 2);
    assert_eq!(std::env::current_dir().unwrap(), path1);

    // We can 'chdir' to the same directory without locking.
    sync_fchdir(dir1).expect("Failed to fchdir");
    assert_eq!(sync_fchdir_lock_count(), 2);
    assert_eq!(std::env::current_dir().unwrap(), path1);

    // Kick off a thread which will try to take the lock, and block.
    let thread_done = Arc::new(RelaxedAtomicBool::new(false));
    let thread_done_clone = thread_done.clone();
    let dir2_clone = dir2.clone();
    let path2_clone = path2.clone();
    let handle = std::thread::spawn(move || {
        let _lock = sync_fchdir_lock(&dir2_clone).expect("Failed to fchdir in thread");
        assert_eq!(std::env::current_dir().unwrap(), path2_clone);
        thread_done_clone.store(true);
    });
    std::thread::sleep(std::time::Duration::from_millis(100));

    // We still hold the lock but now we expect it to be contended.
    // Nevertheless we can still 'chdir' to the same directory as long as we don't want the lock.
    sync_fchdir(dir1).expect("Failed to fchdir");
    assert_eq!(sync_fchdir_lock_count(), 2);
    assert!(!thread_done.load());

    // Release our locks, unblocking the thread.
    drop(lock1);
    assert_eq!(sync_fchdir_lock_count(), 1);
    drop(lock2);

    handle.join().unwrap();
    assert!(thread_done.load());

    std::env::set_current_dir(saved_cwd).unwrap();
    std::fs::remove_dir_all(&path1).unwrap();
    std::fs::remove_dir_all(&path2).unwrap();
}
