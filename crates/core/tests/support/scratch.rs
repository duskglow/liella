//! Scratch paths for the tests: under the target directory (`CARGO_TARGET_TMPDIR`) unless a test
//! names another base, and removed with whatever they hold when dropped, so a test leaves nothing
//! behind whether it passes or fails. Included with `#[path]` by the test files that need it.
#![allow(dead_code)]

use std::ffi::CString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A path of this process's own; what is there when it is dropped is removed.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A fresh path `<target>/tmp/liella-<tag>-<pid>-<n>` where nothing exists yet.
    pub(crate) fn path(tag: &str) -> Scratch {
        Scratch::path_in(Path::new(env!("CARGO_TARGET_TMPDIR")), tag)
    }

    /// The same under `base` (for example `/tmp` for a Unix socket, whose path must be short).
    pub(crate) fn path_in(base: &Path, tag: &str) -> Scratch {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let p = base.join(format!("liella-{tag}-{}-{n}", std::process::id()));
        remove(&p);
        Scratch(p)
    }

    /// A fresh, empty, private (0700) directory with a SHORT path, for Unix-domain sockets: a
    /// socket's path must fit `sun_path` (108 bytes) and the checkout's path may be any length, so
    /// it is made under `/tmp` (not under the target directory), never relative to the tree.
    pub(crate) fn sock_dir(tag: &str) -> Scratch {
        use std::os::unix::fs::DirBuilderExt;
        let mut s = Scratch::path_in(Path::new("/tmp"), tag);
        let made = std::fs::DirBuilder::new().mode(0o700).create(&s.0);
        assert!(made.is_ok(), "socket directory {}: {made:?}", s.0.display());
        if let Ok(real) = s.0.canonicalize() {
            s.0 = real;
        }
        assert!(s.0.as_os_str().len() < 80, "socket directory {} leaves too little of sun_path", s.0.display());
        s
    }

    /// A fresh, empty directory under the target directory.
    pub(crate) fn dir(tag: &str) -> Scratch {
        let s = Scratch::path(tag);
        let made = std::fs::create_dir_all(&s.0);
        assert!(made.is_ok(), "scratch directory {}: {made:?}", s.0.display());
        s
    }

    /// The path as a C string.
    pub(crate) fn c(&self) -> CString {
        CString::new(self.0.clone().into_os_string().into_encoded_bytes()).unwrap_or_default()
    }

    /// The path as text (the tests' paths are ASCII).
    pub(crate) fn s(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        remove(&self.0);
    }
}

/// Remove `p`, a file, link or directory tree; a directory a test made unreadable or unwritable
/// gets its owner's permissions back first.
pub(crate) fn remove(p: &Path) {
    if std::fs::remove_file(p).is_ok() || std::fs::remove_dir_all(p).is_ok() || std::fs::symlink_metadata(p).is_err() {
        return;
    }
    open_up(p);
    let _ = std::fs::remove_dir_all(p);
}

// FS-ORDER: restores permissions on every entry; the order of visiting is irrelevant
fn open_up(p: &Path) {
    if std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()) {
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
        if let Ok(entries) = std::fs::read_dir(p) {
            for e in entries.flatten() {
                open_up(&e.path());
            }
        }
    }
}
