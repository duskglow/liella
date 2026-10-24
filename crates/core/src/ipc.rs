//! POSIX message queues and shared memory (spec §25.3–25.4; D-25.2, D-25.3). Pipes and System V
//! IPC (§25.1–25.2) are the typed `Sys` calls themselves, with the C shapes in
//! `liella_legacy::ipc_compat`.
//!
//! * **Queue names** ([`queue_name`], §25.3): `/name`, one leading slash, which the kernel's
//!   `mqueue` namespace does not take (⚑ KERN). A name without the slash is `EINVAL`; what
//!   follows the slash is the kernel's to judge (mq_open(3), mq_overview(7)): a further slash
//!   or `.`/`..` is `EACCES`, nothing at all `ENOENT`, more than `NAME_MAX` bytes
//!   `ENAMETOOLONG`. `mq_unlink` refused by the sticky `mqueue` directory is `EACCES`, the
//!   error POSIX and mq_unlink(3) give (the kernel says `EPERM`).
//! * **Queues** ([`mq_open`]): the descriptor is always close-on-exec (POSIX: `exec` closes
//!   every message queue descriptor). The mode and the attributes count only with `O_CREAT`.
//!   Sending, receiving, notification and attributes are the kernel's calls.
//! * **Shared-memory names** ([`object_name`], §25.4): leading slashes are optional (POSIX
//!   leaves a name without one implementation-defined; the reference accepts it); after them
//!   the name is one path component of at most `NAME_MAX` bytes: empty, a further slash, `.`
//!   or `..` is `EINVAL` (`ENOENT` for `shm_unlink`: no object has such a name).
//! * **Shared-memory objects** ([`shm_open`], D-25.2): regular files in a directory, the tmpfs
//!   at `/dev/shm` for the C interface, opened close-on-exec and without following a symbolic
//!   link. Anything that is not a regular file (a directory, a FIFO, a device) is `EINVAL`
//!   rather than a descriptor to something that is not a shared-memory object; the open never
//!   blocks on a FIFO. A directory where an object was asked for is `EINVAL` too (POSIX's error
//!   for a name it cannot serve; the kernel says `EISDIR`), and a permission refusal by the
//!   sticky directory on unlink is `EACCES` (POSIX; the kernel says `EPERM`).

use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::path::{KStr, NAME_MAX, PATH_MAX};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MqAttr, O_CLOEXEC, O_CREAT, O_NOFOLLOW, O_NONBLOCK, S_IFMT, S_IFREG};

use crate::error::Result;

// ------------------------------------------------------------------------------ queues (§25.3)

/// The kernel's form of the queue name `name`: without its leading slash. `EINVAL` when it
/// does not start with one.
pub fn queue_name(name: KStr<'_>) -> Result<KStr<'_>> {
    match name.with_nul().split_first() {
        Some((b'/', rest)) => KStr::from_nul_terminated(rest),
        _ => Err(Errno::EINVAL),
    }
}

/// `mq_open(name, flags, mode, attr)`: a queue descriptor (always close-on-exec). `mode` and
/// `attr` are used only with `O_CREAT`.
pub fn mq_open<S: Sys>(sys: &S, name: KStr<'_>, flags: u32, mode: u32, attr: Option<&MqAttr>) -> Result<Fd> {
    let kname = queue_name(name)?;
    if flags & O_CREAT != 0 {
        sys.mq_open(kname, flags | O_CLOEXEC, mode, attr)
    } else {
        sys.mq_open(kname, flags | O_CLOEXEC, 0, None)
    }
}

/// `mq_unlink(name)`. A permission refusal by the sticky `mqueue` directory is `EACCES`
/// (POSIX, mq_unlink(3); the kernel says `EPERM`).
pub fn mq_unlink<S: Sys>(sys: &S, name: KStr<'_>) -> Result<()> {
    sys.mq_unlink(queue_name(name)?).map_err(|e| if e == Errno::EPERM { Errno::EACCES } else { e })
}

// ------------------------------------------------------------------------------ shared memory (§25.4)

/// The object name in `name`: after any leading slashes, one path component of at most
/// `NAME_MAX` bytes. `EINVAL` for an empty name, a further slash, `.` or `..`;
/// `ENAMETOOLONG` beyond `NAME_MAX`.
pub fn object_name(name: &[u8]) -> Result<&[u8]> {
    let start = name.iter().position(|&b| b != b'/').unwrap_or(name.len());
    let n = name.get(start..).unwrap_or(&[]);
    if n.is_empty() || n.contains(&b'/') || n == b"." || n == b".." {
        return Err(Errno::EINVAL);
    }
    if n.len() > NAME_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(n)
}

/// `dir` (ending with `/`) followed by the object name of `name`, terminated, in `buf`.
fn object_path<'b>(dir: &[u8], name: &[u8], buf: &'b mut [u8; PATH_MAX]) -> Result<KStr<'b>> {
    let n = object_name(name)?;
    let len = dir.len().saturating_add(n.len());
    if len >= PATH_MAX || dir.contains(&0) || n.contains(&0) {
        return Err(Errno::ENAMETOOLONG);
    }
    for (o, b) in buf.iter_mut().zip(dir.iter().chain(n)) {
        *o = *b;
    }
    let end = buf.get_mut(len).ok_or(Errno::ENAMETOOLONG)?;
    *end = 0;
    KStr::from_nul_terminated(buf.get(..=len).ok_or(Errno::ENAMETOOLONG)?)
}

/// `shm_open(name, oflag, mode)` in the directory `dir` (ending with `/`; `/dev/shm/` for the
/// C interface): the object's descriptor, close-on-exec, with the caller's access mode and
/// status flags.
pub fn shm_open<S: Sys>(sys: &S, dir: &[u8], name: &[u8], oflag: u32, mode: u32) -> Result<Fd> {
    let mut buf = [0u8; PATH_MAX];
    let path = object_path(dir, name, &mut buf)?;
    // Never wait on a FIFO: opened without blocking, checked, then given the caller's flags.
    let fd = sys.openat(Fd::CWD, path, oflag | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK, mode).map_err(|e| if e == Errno::EISDIR { Errno::EINVAL } else { e })?;
    let checked = sys.fstat(fd).and_then(|st| if st.mode & S_IFMT == S_IFREG { Ok(()) } else { Err(Errno::EINVAL) }).and_then(|()| {
        if oflag & O_NONBLOCK != 0 {
            return Ok(());
        }
        let fl = sys.fcntl_int(fd, FcntlInt::GetFl, 0)?;
        sys.fcntl_int(fd, FcntlInt::SetFl, fl & !i64::from(O_NONBLOCK)).map(|_| ())
    });
    match checked {
        Ok(()) => Ok(fd),
        Err(e) => {
            let _ = sys.close(fd);
            Err(e)
        }
    }
}

/// `shm_unlink(name)` in the directory `dir`. A name no object can have is `ENOENT`.
pub fn shm_unlink<S: Sys>(sys: &S, dir: &[u8], name: &[u8]) -> Result<()> {
    let mut buf = [0u8; PATH_MAX];
    let path = object_path(dir, name, &mut buf).map_err(|e| if e == Errno::EINVAL { Errno::ENOENT } else { e })?;
    sys.unlinkat(Fd::CWD, path, 0).map_err(|e| if e == Errno::EPERM { Errno::EACCES } else { e })
}
