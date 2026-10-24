//! The C shapes of spec §25.2 (System V IPC) that add something to the kernel call: `ftok`'s
//! key formula, the message buffer's `mtype` header, and the `*ctl` commands, whose untyped
//! argument (a `union semun`, a `struct msqid_ds *` or a smaller `*info` structure) is decoded
//! by command. The decoding plan is the command tables below (`*_shape`); the boundary
//! converts the C pointer to the planned shape (`conv::semctl_arg`, `conv::ipc_ctl_arg`).
//!
//! Design: each family's commands are one data table of `(command, shape)`; the same table
//! answers "what does the argument look like" and "is the command known", and the `*ctl`
//! functions then route by shape, so a command added to the kernel layer needs one table row.

use liella::error::{Errno, Result};
use liella_kernel::fd::Fd;
use liella_kernel::ops::{MsgCtl, SemCtlDs, SemCtlInt, ShmCtl};
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MsqidDs, SemidDs, ShmidDs};

/// `IPC_64`: the "new layout" bit the C library adds to every `*ctl` command; one the caller
/// passes is ignored (the kernel structures are always the 64-bit ones on x86-64).
pub const IPC_64: i32 = 0x100;

/// `ftok(path, id)` (§25.2): `(ino & 0xffff) | (dev & 0xff) << 16 | (id & 0xff) << 24`
/// (distinct files may share a key; the result may be negative).
pub fn ftok<S: Sys>(sys: &S, path: KStr<'_>, id: i32) -> Result<i32> {
    let st = sys.fstatat(Fd::CWD, path, 0)?;
    let ino = u32::try_from(st.ino & 0xffff).unwrap_or(0);
    let dev = u32::try_from(st.dev & 0xff).unwrap_or(0);
    let tag = u32::from_ne_bytes(id.to_ne_bytes()) & 0xff;
    Ok(i32::from_ne_bytes((ino | dev << 16 | tag << 24).to_ne_bytes()))
}

/// The bytes of a message whose text is `text` bytes: the `long mtype` header and the text.
pub fn msg_bytes(text: usize) -> Result<usize> {
    text.checked_add(size_of::<i64>()).ok_or(Errno::EINVAL)
}

/// How a `*ctl` command uses its argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtlShape {
    /// Ignored.
    None,
    /// An `int` value (`semctl(SETVAL)`).
    Val,
    /// The full `*id_ds` structure.
    Ds,
    /// A smaller statistics structure of this many bytes (`IPC_INFO`, `SEM_INFO`, ...).
    Info(usize),
    /// `semctl(GETALL/SETALL)`: an array of `unsigned short`, one per semaphore.
    Values,
}

use CtlShape::{Ds, Info, None as No, Val, Values};

/// `semctl` commands (`struct seminfo` is 40 bytes).
const SEM: &[(i32, CtlShape)] = &[
    (0, No), (1, Ds), (2, Ds), (3, Info(40)), (11, No), (12, No), (13, Values), (14, No), (15, No),
    (16, Val), (17, Values), (18, Ds), (19, Info(40)), (20, Ds),
];
/// `msgctl` commands (`struct msginfo` is 32 bytes).
const MSG: &[(i32, CtlShape)] = &[(0, No), (1, Ds), (2, Ds), (3, Info(32)), (11, Ds), (12, Info(32)), (13, Ds)];
/// `shmctl` commands (`struct shminfo` is 72 bytes, `struct shm_info` 48).
const SHM: &[(i32, CtlShape)] = &[(0, No), (1, Ds), (2, Ds), (3, Info(72)), (11, No), (12, No), (13, Ds), (14, Info(48)), (15, Ds)];

fn shape(table: &[(i32, CtlShape)], cmd: i32) -> Option<CtlShape> {
    let cmd = cmd & !IPC_64;
    table.iter().find(|(c, _)| *c == cmd).map(|&(_, s)| s)
}

/// `semctl`'s argument per command; `None` for an unknown command (`EINVAL`).
pub fn sem_shape(cmd: i32) -> Option<CtlShape> {
    shape(SEM, cmd)
}

/// `msgctl`'s argument per command.
pub fn msg_shape(cmd: i32) -> Option<CtlShape> {
    shape(MSG, cmd)
}

/// `shmctl`'s argument per command.
pub fn shm_shape(cmd: i32) -> Option<CtlShape> {
    shape(SHM, cmd)
}

/// A `msgctl`/`shmctl` argument in its decoded shape.
pub enum CtlArg<'a, T> {
    /// Ignored by the command.
    None,
    /// The caller's `*id_ds`.
    Ds(&'a mut T),
    /// The caller's smaller statistics structure.
    Info(&'a mut [u8]),
}

/// A `semctl` argument (`union semun`) in its decoded shape.
pub enum SemArg<'a> {
    /// Ignored by the command.
    None,
    /// `val`.
    Val(i32),
    /// `buf`.
    Ds(&'a mut SemidDs),
    /// `__buf` (`struct seminfo`).
    Info(&'a mut [u8]),
    /// `array`, one entry per semaphore.
    Values(&'a mut [u16]),
}

/// The number of semaphores in set `id` (the length of `semctl`'s `GETALL`/`SETALL` array).
pub fn nsems<S: Sys>(sys: &S, id: i32) -> Result<usize> {
    let mut ds = SemidDs::default();
    sys.semctl_ds(id, SemCtlDs::Stat, &mut ds)?;
    usize::try_from(ds.nsems()).map_err(|_| Errno::EINVAL)
}

/// Copy the leading bytes of the structure words the kernel filled into `out`: the `*info`
/// structures are prefixes of the `*id_ds` scratch (native-endian words).
fn copy_prefix(words: &[u64], out: &mut [u8]) {
    for (chunk, w) in out.chunks_mut(size_of::<u64>()).zip(words) {
        let b = w.to_ne_bytes();
        let n = chunk.len();
        if let (Some(dst), Some(src)) = (chunk.get_mut(..n), b.get(..n)) {
            dst.copy_from_slice(src);
        }
    }
}

/// `semctl(id, num, cmd, arg)` (§25.2).
pub fn semctl<S: Sys>(sys: &S, id: i32, num: i32, cmd: i32, arg: SemArg<'_>) -> Result<i64> {
    let code = u32::try_from(cmd & !IPC_64).map_err(|_| Errno::EINVAL)?;
    let kind = sem_shape(cmd).ok_or(Errno::EINVAL)?;
    let r = match (kind, arg) {
        (No, SemArg::None) => sys.semctl_int(id, num, SemCtlInt::from_code(code).ok_or(Errno::EINVAL)?, 0)?,
        (Val, SemArg::Val(v)) => sys.semctl_int(id, num, SemCtlInt::from_code(code).ok_or(Errno::EINVAL)?, v)?,
        (Ds, SemArg::Ds(ds)) => sys.semctl_ds(id, SemCtlDs::from_code(code).ok_or(Errno::EINVAL)?, ds)?,
        (Info(_), SemArg::Info(out)) => {
            let mut scratch = SemidDs::default();
            let r = sys.semctl_ds(id, SemCtlDs::from_code(code).ok_or(Errno::EINVAL)?, &mut scratch)?;
            copy_prefix(&scratch.words, out);
            r
        }
        (Values, SemArg::Values(v)) => {
            if code == 13 { sys.semctl_getall(id, v)? } else { sys.semctl_setall(id, v)? }
            0
        }
        _ => return Err(Errno::EINVAL),
    };
    Ok(i64::from(r))
}

/// `msgctl(id, cmd, buf)` (§25.2).
pub fn msgctl<S: Sys>(sys: &S, id: i32, cmd: i32, arg: CtlArg<'_, MsqidDs>) -> Result<i64> {
    let c = MsgCtl::from_code(u32::try_from(cmd & !IPC_64).map_err(|_| Errno::EINVAL)?).ok_or(Errno::EINVAL)?;
    let mut scratch = MsqidDs::default();
    let r = match (msg_shape(cmd).ok_or(Errno::EINVAL)?, arg) {
        (No, CtlArg::None) => sys.msgctl(id, c, &mut scratch)?,
        (Ds, CtlArg::Ds(ds)) => sys.msgctl(id, c, ds)?,
        (Info(_), CtlArg::Info(out)) => {
            let r = sys.msgctl(id, c, &mut scratch)?;
            copy_prefix(&scratch.words, out);
            r
        }
        _ => return Err(Errno::EINVAL),
    };
    Ok(i64::from(r))
}

/// `shmctl(id, cmd, buf)` (§25.2).
pub fn shmctl<S: Sys>(sys: &S, id: i32, cmd: i32, arg: CtlArg<'_, ShmidDs>) -> Result<i64> {
    let c = ShmCtl::from_code(u32::try_from(cmd & !IPC_64).map_err(|_| Errno::EINVAL)?).ok_or(Errno::EINVAL)?;
    let mut scratch = ShmidDs::default();
    let r = match (shm_shape(cmd).ok_or(Errno::EINVAL)?, arg) {
        (No, CtlArg::None) => sys.shmctl(id, c, &mut scratch)?,
        (Ds, CtlArg::Ds(ds)) => sys.shmctl(id, c, ds)?,
        (Info(_), CtlArg::Info(out)) => {
            let r = sys.shmctl(id, c, &mut scratch)?;
            copy_prefix(&scratch.words, out);
            r
        }
        _ => return Err(Errno::EINVAL),
    };
    Ok(i64::from(r))
}
