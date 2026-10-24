//! The C conventions of spec §13.13 (phase 12; D-13.8, D-13.10, D-13.15): the mount-table
//! streams (`setmntent`, `getmntent`, `getmntent_r`, `addmntent`, `endmntent`, `hasmntopt`) and
//! the `fstab` enumeration (`setfsent`, `getfsent`, `getfsspec`, `getfsfile`, `endfsent`). The
//! text format is `liella::fs::mnttab`; the streams are `crate::stdio`'s; the exports in
//! `posix::s13_filesystem` convert the C arguments.
//!
//! **Results.** `getmntent` and the `fsent` functions return a record in a block of the calling
//! thread (`crate::users`' result blocks, kinds `MNT` and `FS`; P-scope: sharing one between
//! threads was already a data race): a call overwrites the previous result of the same kind in
//! the same thread only; `getfsent`, `getfsspec` and `getfsfile` share theirs. The end of a
//! table is `NULL` with `errno` unchanged (X3). `getmntent_r` packs the four strings into the
//! caller's buffer: a buffer too small for them (0 included) is `ERANGE`, nothing is written
//! and the stream goes back to the start of the line, so a retry with a larger buffer returns
//! it; a negative size is `EINVAL`.
//!
//! **Arguments.** A NULL stream is `EINVAL` (as for every stream function); a NULL entry,
//! string field or query is `EINVAL` (`addmntent` returns 1, `hasmntopt` `NULL`); a NULL buffer
//! with a positive size, a NULL `mntbuf` and a NULL key are `EFAULT`. `endmntent(NULL)` returns 1
//! and does nothing.

use liella::error::Errno;
use liella::fs::mnttab::{self, Entry};
use liella::mem::TryVec;
use liella::stream::Whence;
use liella_abi::layout::{struct_fstab, struct_mntent};
use liella_abi::pack::Packer;
use liella_kernel::errno::KResult;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_RDONLY, S_IFDIR, S_IFMT};
use liella_legacy::cursors::{self, Step, FS_CURSOR};
use liella_legacy::stdio_compat as sc;

use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::users::{self, FS, MNT};

/// `_PATH_FSTAB`, with its terminator.
const FSTAB: &[u8] = b"/etc/fstab\0";

/// A C string field of a caller's record: `text` reads the string at an address (NULL: `None`).
pub type Text<'a> = &'a dyn Fn(usize) -> KResult<Option<&'a [u8]>>;

// ------------------------------------------------------------------------------ streams

/// `setmntent(file, mode)`: `fopen` with close-on-exec added (`e` after the mode's first letter).
pub fn setmntent(path: KResult<KStr<'_>>, mode: KResult<&[u8]>) -> KResult<usize> {
    let mode = mode?;
    let mut m = TryVec::new();
    if let Some((first, rest)) = mode.split_first() {
        m.push(*first)?;
        m.push(b'e')?;
        m.extend_from_slice(rest)?;
    }
    stdio::fopen(path, Ok(&m))
}

/// `endmntent(fp)`: closes the stream; 1 always (getmntent(3)), `errno` set when the close
/// failed. NULL does nothing.
pub fn endmntent(fp: usize) -> i32 {
    if fp != 0 {
        if let Err(e) = stdio::fclose(fp) {
            crate::errno::set(e);
        }
    }
    1
}

/// The next entry of the stream `fp`, with the length of its line as read (the newline
/// included), so that `getmntent_r` can go back to the line's start without asking the stream
/// for its position on every call. `Ok(None)` at the end.
fn next_entry(fp: usize) -> KResult<Option<(Entry, usize)>> {
    stdio::with(fp, Locking::Locked, |g, file, ext| {
        let mut line = TryVec::new();
        loop {
            line.clear();
            if sc::read_record(g, file, ext, b'\n', &mut line)? == 0 {
                return Ok(None);
            }
            let text = line.strip_suffix(b"\n").unwrap_or(&line);
            if let Some(e) = mnttab::parse(text)? {
                return Ok(Some((e, line.len())));
            }
        }
    })?
}

/// Pack the four strings of `e` with `k`: the `struct mntent`.
fn pack_mntent(e: &Entry, k: &mut Packer<'_>) -> Option<struct_mntent> {
    Some(struct_mntent { mnt_fsname: k.text(e.field(0))?, mnt_dir: k.text(e.field(1))?, mnt_type: k.text(e.field(2))?, mnt_opts: k.text(e.field(3))?, mnt_freq: e.freq, mnt_passno: e.passno })
}

/// `getmntent(fp)`: the entry's address in the calling thread's block; `None` at the end.
pub fn getmntent(fp: usize) -> KResult<Option<usize>> {
    match next_entry(fp)? {
        None => Ok(None),
        Some((e, _)) => users::own(MNT, |k| pack_mntent(&e, k)).map(Some),
    }
}

/// `getmntent_r(fp, out, buf, buflen)`: `out`'s address, its strings in `buf`; `None` at the end.
pub fn getmntent_r(fp: usize, out: Option<&mut struct_mntent>, buf: KResult<&mut [u8]>, buflen: i32) -> KResult<Option<usize>> {
    if buflen < 0 {
        return Err(Errno::EINVAL);
    }
    let out = out.ok_or(Errno::EFAULT)?;
    let buf = buf?;
    let Some((e, len)) = next_entry(fp)? else { return Ok(None) };
    if e.text_len() > buf.len() {
        // Back to the line's start: the position after it, less its length (a stream that
        // cannot tell its position stays where it is).
        if let Ok(back) = i64::try_from(len) {
            let _ = stdio::with(fp, Locking::Locked, |g, file, ext| sc::seek(g, file, ext, back.saturating_neg(), Whence::Cur));
        }
        return Err(Errno::ERANGE);
    }
    let base = buf.as_ptr().expose_provenance();
    *out = pack_mntent(&e, &mut Packer::new(buf, base)).ok_or(Errno::ERANGE)?;
    Ok(Some(core::ptr::from_mut(out).expose_provenance()))
}

/// `addmntent(fp, ent)`: the entry's line written at the end of the file and flushed (each
/// call's output is in the file when it returns). `EINVAL` for a NULL entry or field, or an
/// entry that would not read back (an empty string, a source starting with `#`).
pub fn addmntent(fp: usize, ent: Option<&struct_mntent>, text: Text<'_>) -> KResult<()> {
    let ent = ent.ok_or(Errno::EINVAL)?;
    let field = |a: usize| text(a).and_then(|t| t.ok_or(Errno::EINVAL));
    let strings = [field(ent.mnt_fsname)?, field(ent.mnt_dir)?, field(ent.mnt_type)?, field(ent.mnt_opts)?];
    let mut line = TryVec::new();
    mnttab::put(strings, ent.mnt_freq, ent.mnt_passno, &mut line)?;
    stdio::with(fp, Locking::Locked, |g, file, ext| {
        if let Err(e) = sc::seek(g, file, ext, 0, Whence::End) {
            // A pipe or a terminal has only an end.
            if e != Errno::ESPIPE {
                return Err(e);
            }
        }
        let t = sc::write(g, file, ext, &line);
        if let Some(e) = t.error {
            return Err(e);
        }
        sc::flush(g, file, ext)
    })?
}

/// `hasmntopt(ent, opt)`: the address of the option in `mnt_opts`, 0 when there is none.
pub fn hasmntopt(ent: Option<&struct_mntent>, query: KResult<Option<&[u8]>>, text: Text<'_>) -> KResult<usize> {
    let ent = ent.ok_or(Errno::EINVAL)?;
    let query = query?.ok_or(Errno::EINVAL)?;
    let opts = text(ent.mnt_opts)?.ok_or(Errno::EINVAL)?;
    Ok(mnttab::find_option(opts, query).map_or(0, |at| ent.mnt_opts.saturating_add(at)))
}

// ------------------------------------------------------------------------------ fstab

/// Pack `e` as a `struct fstab` with `k`.
fn pack_fstab(e: &Entry, k: &mut Packer<'_>) -> Option<struct_fstab> {
    Some(struct_fstab {
        fs_spec: k.text(e.field(0))?,
        fs_file: k.text(e.field(1))?,
        fs_vfstype: k.text(e.field(2))?,
        fs_mntops: k.text(e.field(3))?,
        fs_type: k.text(mnttab::fs_type(e.field(3)))?,
        fs_freq: e.freq,
        fs_passno: e.passno,
    })
}

fn own_fstab(e: &Entry) -> KResult<usize> {
    users::own(FS, |k| pack_fstab(e, k))
}

/// `setfsent()`: 1 when `/etc/fstab` is open at its first line, 0 with `errno` when it cannot
/// be (D-13.15: a directory is `EISDIR`, not an empty table).
pub fn setfsent() -> i32 {
    let r = (|| {
        let path = KStr::from_nul_terminated(FSTAB)?;
        if CSys.fstatat(Fd::CWD, path, 0).is_ok_and(|st| st.mode & S_IFMT == S_IFDIR) {
            FS_CURSOR.end(&CSys);
            return Err(Errno::EISDIR);
        }
        FS_CURSOR.restart(&CSys, FSTAB)
    })();
    match r {
        Ok(()) => 1,
        Err(e) => {
            crate::errno::set(e);
            0
        }
    }
}

/// `getfsent()`: the next entry (the file opened on first use); `None` at the end.
pub fn getfsent() -> KResult<Option<usize>> {
    let e = FS_CURSOR.next(&CSys, FSTAB, |line| match mnttab::parse(line) {
        Ok(Some(e)) => Step::Take(Ok(e)),
        Ok(None) => Step::Skip,
        Err(err) => Step::Take(Err(err)),
    })?;
    match e {
        None => Ok(None),
        Some(e) => own_fstab(&e?).map(Some),
    }
}

/// `endfsent()`.
pub fn endfsent() {
    FS_CURSOR.end(&CSys);
}

/// `getfsspec(key)` (`field` 0) and `getfsfile(key)` (`field` 1): the first entry of
/// `/etc/fstab` whose decoded field equals `key`, read from the start through a descriptor of
/// its own (D-13.15: the enumeration's position is not moved); `None` when there is none.
pub fn lookup(key: KResult<&[u8]>, field: usize) -> KResult<Option<usize>> {
    let key = key?;
    let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(FSTAB)?, O_RDONLY | O_CLOEXEC, 0)?;
    let r = (|| {
        let mut line = TryVec::new();
        let mut at = 0u64;
        loop {
            let n = cursors::line_at(&CSys, fd, at, &mut line)?;
            if n == 0 {
                return Ok(None);
            }
            at = at.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
            if let Some(e) = mnttab::parse(&line)? {
                if e.field(field) == key {
                    return own_fstab(&e).map(Some);
                }
            }
        }
    })();
    let _ = CSys.close(fd);
    r
}
