//! The C conventions of spec §17.2 (`initgroups`) and §17.3–17.6 (phase 10): the `_r` packing
//! protocol, the library-owned records of the non-`_r` forms, the stream forms, the
//! process-wide cursors, `getpass` and `getlogin` (`login_tty` is `liella::term::login`). The
//! formats are `liella::users`; the database files, cursors and login accounting
//! `liella_legacy::{nss, cursors, accounting}` (through the switch); the C structures are packed by
//! `liella_abi::pack`. The exports in `posix::s17_users` convert the C arguments.
//!
//! **Results** (D-17.3). The `_r` forms pack the record into the caller's structure and buffer:
//! the buffer holds only the record's strings and pointer arrays (no copy of the line), aligned
//! for the arrays; a buffer too small is `ERANGE`, and an enumeration or a stream then stays on
//! that record (a stream is sought back to where it started), so a retry with a larger buffer
//! returns it. They return the error number, `ENOENT` at the end of an enumeration or stream, and
//! 0 with a NULL result for a record that is not there. The non-`_r` forms return a record in
//! a block of the calling thread (one per record kind, grown as needed, freed when the thread
//! exits): a call overwrites the previous result of the same kind in the same thread only. They
//! return NULL with `errno` for an error, `ENOENT` at the end of a stream, and NULL with `errno`
//! unchanged at the end of an enumeration (as the reference: the `errno = 0; loop; check errno`
//! idiom, owner ruling 2026-10-22, #409) and for a record that is not there.

use core::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::error::Errno;
use liella::mem::TryVec;
use liella::users::{self, PasswdOut, Utmp};
use liella_abi::layout::{struct_group, struct_passwd, struct_sgrp, struct_spwd, struct_utmp};
use liella_abi::pack::{self, Packer};
use liella_kernel::errno::KResult;
use liella_kernel::fd::Fd;
use liella_kernel::ops::TcSet;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_NOCTTY, O_RDWR};
use liella_legacy::accounting::{self as acct, UTMP_DB};
use liella::names::nss::Database;
use liella_legacy::cursors::{self, Cursor, Step};
use liella_legacy::nss::{self, Ctx};
use liella_legacy::nss_files::{self as files, Key};
use liella_rt::sync::RawMutex;

use crate::caller_alloc;
use crate::conv::{self, CPod};
use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::tb::RecordsSlot;
use crate::thread;

// ------------------------------------------------------------------------------ result blocks

/// The record kinds with a block per thread (pairs of words in the `SLOT_RECORDS` slots).
const PW: usize = 0;
const GR: usize = 1;
const SP: usize = 2;
const SG: usize = 3;
const UT: usize = 4;
const LOGIN: usize = 5;
/// `struct hostent` (§16.3).
pub(crate) const HOST: usize = 6;
/// `struct netent` (§16.3).
pub(crate) const NET: usize = 7;
/// `struct servent` (§16.5).
pub(crate) const SERV: usize = 8;
/// `struct protoent` (§16.5).
pub(crate) const PROTO: usize = 9;
/// `struct rpcent` (§16.5).
pub(crate) const RPC: usize = 10;
/// `struct aliasent` (§16.6).
pub(crate) const ALIAS: usize = 11;
/// `struct mntent` (§13.13, `getmntent`).
pub(crate) const MNT: usize = 12;
/// `struct fstab` (§13.13, the `fsent` functions).
pub(crate) const FS: usize = 13;
/// The texts per thread of the functions that returned one process-wide buffer (milestone 6;
/// D-C0.2, D-8.8), each a block of its function's fixed size (so it never moves once made).
pub(crate) const ECVT: usize = 14;
/// `fcvt`.
pub(crate) const FCVT: usize = 15;
/// `qecvt`.
pub(crate) const QECVT: usize = 16;
/// `qfcvt`.
pub(crate) const QFCVT: usize = 17;
/// `l64a`.
pub(crate) const L64A: usize = 18;
/// `tmpnam(NULL)`.
pub(crate) const TMPNAM: usize = 19;
/// `inet_nsap_ntoa(.., NULL)`.
pub(crate) const NSAP_NTOA: usize = 20;
/// `rcmd`'s canonical host name.
pub(crate) const RCMD_HOST: usize = 21;
/// `ttyname`.
pub(crate) const TTYNAME: usize = 22;
/// `ptsname`.
pub(crate) const PTSNAME: usize = 23;
/// `getttyent`'s `struct ttyent`.
pub(crate) const TTYENT: usize = 24;
/// `getusershell`.
pub(crate) const USER_SHELL: usize = 25;
/// The resolver's names: `p_type`, `p_class`, `p_rcode`, `p_option`, `p_time`,
/// `p_secstodate`, `sym_ntos`, `sym_ntop`, `hostalias`, `loc_ntoa(.., NULL)`.
pub(crate) const P_TYPE: usize = 26;
/// See [`P_TYPE`].
pub(crate) const P_CLASS: usize = 27;
/// See [`P_TYPE`].
pub(crate) const P_RCODE: usize = 28;
/// See [`P_TYPE`].
pub(crate) const P_OPTION: usize = 29;
/// See [`P_TYPE`].
pub(crate) const P_TIME: usize = 30;
/// See [`P_TYPE`].
pub(crate) const P_SECSTODATE: usize = 31;
/// See [`P_TYPE`].
pub(crate) const SYM_NTOS: usize = 32;
/// See [`P_TYPE`].
pub(crate) const SYM_NTOP: usize = 33;
/// See [`P_TYPE`].
pub(crate) const HOSTALIAS: usize = 34;
/// See [`P_TYPE`].
pub(crate) const LOC_NTOA: usize = 35;
/// `regexec`'s scratch (D-14.11): its `u32`, `u64`, `i32` and `usize` blocks.
pub(crate) const REGEX_U32: usize = 36;
/// See [`REGEX_U32`].
pub(crate) const REGEX_U64: usize = 37;
/// See [`REGEX_U32`].
pub(crate) const REGEX_I32: usize = 38;
/// See [`REGEX_U32`].
pub(crate) const REGEX_USIZE: usize = 39;
/// The lazy-DFA state store of a `regexec` call (D-14.12): one fixed block per thread, freed at
/// thread exit with the others. Groundwork only; no search requests it yet.
pub(crate) const REGEX_DFA: usize = 40;
/// The libresolv compat `_gethtby*` / `_gethtent` entry: its own block, not [`HOST`]'s (LG-7,
/// observed: the reference's result is never `gethostbyname`'s buffer).
pub(crate) const HT_HOST: usize = 41;
/// The libresolv compat `res_gethostby*` entry: one block for the three, not [`HOST`]'s (LG-7,
/// observed: the three return one buffer, never `gethostbyname`'s).
pub(crate) const RES_HOST: usize = 42;
/// The ONC RPC client's `rpc_createerr` record (§29.1, `__rpc_thread_createerr`).
pub(crate) const RPC_CREATEERR: usize = 43;
/// The ONC RPC client's `clnt_sperror` / `clnt_spcreateerror` text.
pub(crate) const RPC_TEXT: usize = 44;
/// The number of kinds.
const KINDS: usize = 45;
/// The words of the result table (an address and a size per kind).
const TABLE_WORDS: usize = KINDS * 2;
/// The bytes of a thread's result table: defined from [`TABLE_WORDS`] so the allocation and
/// the `Words<TABLE_WORDS>` view of it cannot drift apart.
const TABLE: usize = TABLE_WORDS * size_of::<usize>();
/// The smallest block allocated.
const MIN_BLOCK: usize = 256;

/// The calling thread's result table: `TABLE_WORDS` atomic words.
pub(crate) type Table = liella_legacy::result_slots::Words<TABLE_WORDS>;

/// Claim the calling thread's regex scratch for one call (D-14.12): its records slot when the
/// busy flag (byte 8 of `SLOT_RECORDS`) was free. `None` when a `regexec` on this thread already
/// holds it, or the thread has no slot. A re-entrant call (a signal handler) fails to claim it and
/// uses its own per-call storage, so the thread's scratch blocks never carry two live borrows.
pub(crate) fn scratch_claim() -> Option<&'static RecordsSlot> {
    thread::records_slot().filter(|s| s.busy.compare_exchange(0, 1, Acquire, Relaxed).is_ok())
}

/// Release a claim [`scratch_claim`] returned.
pub(crate) fn scratch_release(slot: &RecordsSlot) {
    slot.busy.store(0, Release);
}

/// A block of at least `need` bytes in the cell pair (`addr`, `size`): the one there, or a new
/// one (the old one freed). Its address.
pub(crate) fn grown(need: usize, addr: usize, size: usize, keep: impl FnOnce(usize, usize)) -> KResult<usize> {
    if addr != 0 && size >= need {
        return Ok(addr);
    }
    let cap = need.max(MIN_BLOCK).checked_next_power_of_two().ok_or(Errno::ENOMEM)?;
    let p = caller_alloc::calloc(cap, 1);
    if p.is_null() {
        return Err(Errno::ENOMEM);
    }
    caller_alloc::release(addr);
    keep(p.expose_provenance(), cap);
    Ok(p.expose_provenance())
}

/// The calling thread's result table, allocated on first use (`ENOMEM` when it cannot be).
fn table() -> KResult<&'static Table> {
    table_of(thread::records_slot().ok_or(Errno::ENOMEM)?)
}

/// The result table of the thread whose records slot is `slot` (the caller's), allocated on
/// first use. The table word is read and written only here and in [`thread_ending`], always as
/// one word (never through a byte view of the slot).
pub(crate) fn table_of(slot: &RecordsSlot) -> KResult<&'static Table> {
    let mut t = slot.table.load(Relaxed);
    if t == 0 {
        let p = caller_alloc::calloc(TABLE, 1);
        if p.is_null() {
            return Err(Errno::ENOMEM);
        }
        t = p.expose_provenance();
        slot.table.store(t, Relaxed);
    }
    crate::tb::table_words::<TABLE_WORDS>(t).ok_or(Errno::ENOMEM)
}

/// The address of the calling thread's block for `kind`, with at least `need` bytes (kept
/// and grown between calls, freed when the thread exits).
pub(crate) fn block_addr(kind: usize, need: usize) -> KResult<usize> {
    block_addr_in(table()?, kind, need)
}

/// [`block_addr`] in the calling thread's table `words` (fetched once by a caller that needs
/// several blocks, such as one regex search).
pub(crate) fn block_addr_in(words: &Table, kind: usize, need: usize) -> KResult<usize> {
    let (at, size) = (kind.saturating_mul(2), kind.saturating_mul(2).saturating_add(1));
    grown(need, words.word(at), words.word(size), |a, n| {
        words.set_word(at, a);
        words.set_word(size, n);
    })
}

/// The calling thread's block for `kind`, with room for a `T` and `extra` bytes after it:
/// the record, the bytes, and their address.
fn block<T: CPod + 'static>(kind: usize, extra: usize) -> KResult<(&'static mut T, &'static mut [u8], usize)> {
    let need = size_of::<T>().checked_add(extra).ok_or(Errno::ENOMEM)?;
    conv::own_record::<T>(block_addr(kind, need)?, need).ok_or(Errno::ENOMEM)
}

/// The calling thread's text buffer of `kind` (`N` bytes, always the same `N` for a kind, so the
/// block is made once and never moves): `ENOMEM` when it cannot be had. Never shared between
/// threads (D-C0.2): a function that cannot get its buffer fails, it does not fall back.
pub(crate) fn result_buf<const N: usize>(kind: usize) -> KResult<&'static liella_legacy::result_slots::Buf<N>> {
    Ok(crate::tb::block_buf::<N>(block_addr(kind, N)?))
}

/// The calling thread's block for `kind` as `len` bytes, and its address.
pub(crate) fn block_bytes(kind: usize, len: usize) -> KResult<(usize, &'static mut [u8])> {
    let addr = block_addr(kind, len)?;
    Ok((addr, conv::own_bytes(addr, len).ok_or(Errno::ENOMEM)?))
}

/// Free the calling thread's blocks and its result table (at its exit).
pub(crate) fn thread_ending() {
    let Some(slot) = thread::records_slot() else { return };
    let t = slot.table.load(Relaxed);
    if t == 0 {
        return;
    }
    // Unhook the table before freeing anything: a signal handler interrupting the loop below
    // then finds no table (and allocates a fresh one) instead of reaching freed blocks.
    slot.table.store(0, Relaxed);
    if let Some(words) = crate::tb::table_words::<TABLE_WORDS>(t) {
        for kind in 0..KINDS {
            caller_alloc::release(words.word(kind.saturating_mul(2)));
        }
    }
    caller_alloc::release(t);
}

/// A record packed by `pack` into the calling thread's block for `kind`: its address.
pub(crate) fn own<T: CPod + 'static>(kind: usize, pack: impl Fn(&mut Packer<'_>) -> Option<T>) -> KResult<usize> {
    let mut m = Packer::measure();
    pack(&mut m).ok_or(Errno::ENOMEM)?;
    let (rec, bytes, base) = block::<T>(kind, m.needed())?;
    let mut k = Packer::new(bytes, base);
    *rec = pack(&mut k).ok_or(Errno::ENOMEM)?;
    Ok(core::ptr::from_mut(rec).expose_provenance())
}

/// The `_r` result: the record packed into `out` and `buf`, `*result` set (to `out`, or NULL);
/// the error number. `find` packs and reports `Ok(None)` for no record, `Err(ERANGE)` for a
/// buffer too small.
pub(crate) fn reentrant<T: Copy>(out: Option<&mut T>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, find: impl FnOnce(&mut [u8], usize) -> KResult<Option<T>>) -> i32 {
    let Some(result) = result else { return Errno::EFAULT.get() };
    *result = 0;
    let r = (|| -> KResult<Option<usize>> {
        let out = out.ok_or(Errno::EFAULT)?;
        let buf = buf?;
        let base = buf.as_ptr().expose_provenance();
        Ok(match find(buf, base)? {
            Some(v) => {
                *out = v;
                Some(core::ptr::from_mut(out).expose_provenance())
            }
            None => None,
        })
    })();
    match r {
        Ok(Some(a)) => {
            *result = a;
            0
        }
        Ok(None) => 0,
        Err(e) => e.get(),
    }
}

/// An enumeration's `_r` step: the end is `ENOENT`.
pub(crate) fn ended<T>(r: KResult<Option<T>>) -> KResult<Option<T>> {
    r?.ok_or(Errno::ENOENT).map(Some)
}

/// A lookup's non-`_r` result: `Ok(0)` (NULL, `errno` unchanged) when not found.
pub(crate) fn found(r: KResult<Option<KResult<usize>>>) -> KResult<usize> {
    r?.unwrap_or(Ok(0))
}

/// An enumeration's non-`_r` result: `Ok(0)` (NULL, `errno` unchanged) at the end, as the
/// reference leaves it (#409).
pub(crate) fn next_entry(r: KResult<Option<KResult<usize>>>) -> KResult<usize> {
    r?.unwrap_or(Ok(0))
}

/// A stream's non-`_r` result: `ENOENT` at the end.
pub(crate) fn next_of(r: KResult<Option<KResult<usize>>>) -> KResult<usize> {
    r?.ok_or(Errno::ENOENT)?
}

// ------------------------------------------------------------------------------ the four databases

/// One of the four record formats: its parser, packer, file, cursor and result kind.
pub(crate) struct Db<R: CPod + Copy + 'static> {
    file: &'static [u8],
    db: Database,
    cursor: &'static Cursor,
    kind: usize,
    /// Parse a line and pack it (`None` from the parser: no record; `Some(None)`: no room).
    pack: fn(&[u8], &mut Packer<'_>) -> Option<Option<R>>,
}

fn pack_pw(line: &[u8], k: &mut Packer<'_>) -> Option<Option<struct_passwd>> {
    users::passwd(line).map(|p| pack::passwd(&p, k))
}

fn pack_gr(line: &[u8], k: &mut Packer<'_>) -> Option<Option<struct_group>> {
    users::group(line).map(|g| pack::group(&g, k))
}

fn pack_sp(line: &[u8], k: &mut Packer<'_>) -> Option<Option<struct_spwd>> {
    users::shadow(line).map(|s| pack::spwd(&s, k))
}

fn pack_sg(line: &[u8], k: &mut Packer<'_>) -> Option<Option<struct_sgrp>> {
    users::gshadow(line).map(|g| pack::sgrp(&g, k))
}

/// `passwd`.
pub(crate) static PASSWD: Db<struct_passwd> = Db { file: files::PASSWD, db: Database::Passwd, cursor: &cursors::PW_CURSOR, kind: PW, pack: pack_pw };
/// `group`.
pub(crate) static GROUP: Db<struct_group> = Db { file: files::GROUP, db: Database::Group, cursor: &cursors::GR_CURSOR, kind: GR, pack: pack_gr };
/// `shadow`.
pub(crate) static SHADOW: Db<struct_spwd> = Db { file: files::SHADOW, db: Database::Shadow, cursor: &cursors::SP_CURSOR, kind: SP, pack: pack_sp };
/// `gshadow`.
pub(crate) static GSHADOW: Db<struct_sgrp> = Db { file: files::GSHADOW, db: Database::Gshadow, cursor: &cursors::SG_CURSOR, kind: SG, pack: pack_sg };

/// One line of a stream, without its newline: `f` reads each line until it takes one (the
/// stream is then past it); `Stay` seeks the stream back to where the line started. `None` at
/// the end of the stream.
fn stream_next<T>(fp: usize, mut f: impl FnMut(&[u8]) -> Step<T>) -> KResult<Option<T>> {
    use liella_legacy::stdio_compat as sc;
    stdio::with(fp, Locking::Locked, |g, file, ext| {
        let mut line = TryVec::new();
        loop {
            let start = sc::tell(g, file, ext).ok();
            line.clear();
            let mut any = false;
            loop {
                match sc::getc(g, file, ext)? {
                    None => break,
                    Some(c) => {
                        any = true;
                        if c == b'\n' {
                            break;
                        }
                        line.push(c)?;
                    }
                }
            }
            if !any {
                return Ok(None);
            }
            match f(&line) {
                Step::Skip => {}
                Step::Take(t) => return Ok(Some(t)),
                Step::Stay(e) => {
                    if let Some(s) = start.and_then(|s| i64::try_from(s).ok()) {
                        let _ = sc::seek(g, file, ext, s, liella::stream::Whence::Set);
                    }
                    return Err(e);
                }
            }
        }
    })?
}

/// The step of a `_r` enumeration over `buf`: parse, then pack or stay.
pub(crate) fn step_r<R: CPod + Copy + 'static>(d: &Db<R>, line: &[u8], buf: &mut [u8], base: usize) -> Step<R> {
    match (d.pack)(line, &mut Packer::new(buf, base)) {
        None => Step::Skip,
        Some(Some(r)) => Step::Take(r),
        Some(None) => Step::Stay(Errno::ERANGE),
    }
}

/// The step of a non-`_r` enumeration: parse, then pack into the thread's block.
fn step_own<R: CPod + Copy + 'static>(d: &Db<R>, line: &[u8]) -> Step<KResult<usize>> {
    if (d.pack)(line, &mut Packer::measure()).is_none() {
        return Step::Skip;
    }
    Step::Take(own(d.kind, |k| (d.pack)(line, k).flatten()))
}

impl<R: CPod + Copy + 'static> Db<R> {
    /// `setpwent` (and friends).
    pub(crate) fn rewind(&self) {
        self.cursor.rewind(&CSys);
    }

    /// `endpwent`.
    pub(crate) fn end(&self) {
        self.cursor.end(&CSys);
    }

    /// The next record of the enumeration, `f` stepping over the lines: the `files` source's,
    /// when the database's line names it (D-16.1), else none.
    fn step<T>(&self, f: impl FnMut(&[u8]) -> Step<T>) -> KResult<Option<T>> {
        if !Ctx::system(&CSys).enumerates(self.db) {
            return Ok(None);
        }
        self.cursor.next(&CSys, self.file, f)
    }

    /// `getpwent()`.
    pub(crate) fn next(&self) -> KResult<usize> {
        next_entry(self.step(|line| step_own(self, line)))
    }

    /// `getpwent_r(out, buf, buflen, result)`.
    pub(crate) fn next_r(&self, out: Option<&mut R>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
        reentrant(out, buf, result, |buf, base| ended(self.step(|line| step_r(self, line, buf, base))))
    }

    /// `fgetpwent(stream)`.
    pub(crate) fn stream(&self, fp: usize) -> KResult<usize> {
        next_of(stream_next(fp, |line| step_own(self, line)))
    }

    /// `fgetpwent_r(stream, out, buf, buflen, result)`.
    pub(crate) fn stream_r(&self, fp: usize, out: Option<&mut R>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
        reentrant(out, buf, result, |buf, base| ended(stream_next(fp, |line| step_r(self, line, buf, base))))
    }

    /// `sgetspent(text)`: one line; `EINVAL` when it is no record.
    pub(crate) fn text(&self, text: KResult<&[u8]>) -> KResult<usize> {
        let text = text?;
        match step_own(self, text) {
            Step::Take(r) => r,
            _ => Err(Errno::EINVAL),
        }
    }

    /// `sgetspent_r(text, out, buf, buflen, result)`.
    pub(crate) fn text_r(&self, text: KResult<&[u8]>, out: Option<&mut R>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
        reentrant(out, buf, result, |buf, base| match step_r(self, text?, buf, base) {
            Step::Take(r) => Ok(Some(r)),
            Step::Stay(e) => Err(e),
            Step::Skip => Err(Errno::EINVAL),
        })
    }
}

/// A lookup's packed record: `ERANGE` when it did not fit.
pub(crate) fn packed<R>(r: KResult<Option<Option<R>>>) -> KResult<Option<R>> {
    match r? {
        None => Ok(None),
        Some(None) => Err(Errno::ERANGE),
        Some(Some(v)) => Ok(Some(v)),
    }
}

/// `getpwnam(name)` (`Key::Name`) and `getpwuid(uid)` (`Key::Id`).
pub fn pw_by(key: KResult<Key<'_>>) -> KResult<usize> {
    found(nss::found(nss::passwd_cached(&CSys, &crate::nscache::MEMORY, key?, |p| own(PW, |k| pack::passwd(p, k)))))
}

/// `getpwnam_r` and `getpwuid_r`.
pub fn pw_by_r(key: KResult<Key<'_>>, out: Option<&mut struct_passwd>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    reentrant(out, buf, result, |buf, base| packed(nss::found(nss::passwd_cached(&CSys, &crate::nscache::MEMORY, key?, |p| pack::passwd(p, &mut Packer::new(buf, base))))))
}

/// `getgrnam(name)` and `getgrgid(gid)`.
pub fn gr_by(key: KResult<Key<'_>>) -> KResult<usize> {
    found(nss::found(nss::group_cached(&CSys, &crate::nscache::MEMORY, key?, |g| own(GR, |k| pack::group(g, k)))))
}

/// `getgrnam_r` and `getgrgid_r`.
pub fn gr_by_r(key: KResult<Key<'_>>, out: Option<&mut struct_group>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    reentrant(out, buf, result, |buf, base| packed(nss::found(nss::group_cached(&CSys, &crate::nscache::MEMORY, key?, |g| pack::group(g, &mut Packer::new(buf, base))))))
}

/// `getspnam(name)`.
pub fn sp_by(name: KResult<&[u8]>) -> KResult<usize> {
    found(nss::found(nss::shadow(&Ctx::system(&CSys), name?, |s| own(SP, |k| pack::spwd(s, k)))))
}

/// `getspnam_r`.
pub fn sp_by_r(name: KResult<&[u8]>, out: Option<&mut struct_spwd>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    reentrant(out, buf, result, |buf, base| packed(nss::found(nss::shadow(&Ctx::system(&CSys), name?, |s| pack::spwd(s, &mut Packer::new(buf, base))))))
}

/// `getsgnam(name)`.
pub fn sg_by(name: KResult<&[u8]>) -> KResult<usize> {
    found(nss::found(nss::gshadow(&Ctx::system(&CSys), name?, |g| own(SG, |k| pack::sgrp(g, k)))))
}

/// `getsgnam_r`.
pub fn sg_by_r(name: KResult<&[u8]>, out: Option<&mut struct_sgrp>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    reentrant(out, buf, result, |buf, base| packed(nss::found(nss::gshadow(&Ctx::system(&CSys), name?, |g| pack::sgrp(g, &mut Packer::new(buf, base))))))
}

// ------------------------------------------------------------------------------ writing records

/// A C string field of a caller's record: `text` reads the string at an address (NULL: `None`).
type Text<'a> = &'a dyn Fn(usize) -> KResult<Option<&'a [u8]>>;

/// Write `line` to the stream `fp` in one piece.
fn put_line(fp: usize, line: &[u8]) -> KResult<()> {
    stdio::fputs(Ok(line), fp, Locking::Locked).map(|_| ())
}

/// `putpwent(p, stream)`: `EINVAL` for a NULL record or one that would not read back.
pub fn putpwent(p: Option<&struct_passwd>, fp: usize, text: Text<'_>) -> KResult<()> {
    let p = p.ok_or(Errno::EINVAL)?;
    let o = PasswdOut { name: text(p.pw_name)?, passwd: text(p.pw_passwd)?, uid: p.pw_uid, gid: p.pw_gid, gecos: text(p.pw_gecos)?, dir: text(p.pw_dir)?, shell: text(p.pw_shell)? };
    let mut line = TryVec::new();
    users::put_passwd(&o, &mut line)?;
    put_line(fp, &line)
}

/// The entries of a NULL-terminated string array at `addr` (NULL: none).
type List<'a> = &'a dyn Fn(usize) -> KResult<alloc::vec::Vec<&'a [u8]>>;

fn entries<'a>(addr: usize, list: List<'a>) -> KResult<alloc::vec::Vec<&'a [u8]>> {
    if addr == 0 { Ok(alloc::vec::Vec::new()) } else { list(addr) }
}

/// `putgrent(g, stream)`.
pub fn putgrent(g: Option<&struct_group>, fp: usize, text: Text<'_>, list: List<'_>) -> KResult<()> {
    let g = g.ok_or(Errno::EINVAL)?;
    let mut line = TryVec::new();
    users::put_group(text(g.gr_name)?, text(g.gr_passwd)?, g.gr_gid, entries(g.gr_mem, list)?, &mut line)?;
    put_line(fp, &line)
}

/// `putspent(s, stream)`.
pub fn putspent(s: Option<&struct_spwd>, fp: usize, text: Text<'_>) -> KResult<()> {
    let s = s.ok_or(Errno::EINVAL)?;
    let mut line = TryVec::new();
    let aging = [s.sp_lstchg, s.sp_min, s.sp_max, s.sp_warn, s.sp_inact, s.sp_expire];
    users::put_shadow(text(s.sp_namp)?, text(s.sp_pwdp)?, aging, s.sp_flag, &mut line)?;
    put_line(fp, &line)
}

/// `putsgent(g, stream)`.
pub fn putsgent(g: Option<&struct_sgrp>, fp: usize, text: Text<'_>, list: List<'_>) -> KResult<()> {
    let g = g.ok_or(Errno::EINVAL)?;
    let mut line = TryVec::new();
    users::put_gshadow(text(g.sg_namp)?, text(g.sg_passwd)?, entries(g.sg_adm, list)?, entries(g.sg_mem, list)?, &mut line)?;
    put_line(fp, &line)
}

/// `getpw(uid, buf)`: the user's `passwd(5)` line in `buf` (whose room the caller vouches for,
/// ⚑ HIST): 0, or -1 (`errno` unchanged when there is no such user).
pub fn getpw<'a>(uid: u32, buf: impl FnOnce(usize) -> KResult<&'a mut [u8]>) -> i32 {
    let r = (|| -> KResult<bool> {
        let line = nss::found(nss::passwd_cached(&CSys, &crate::nscache::MEMORY, Key::Id(uid), |p| -> KResult<TryVec<u8>> {
            let mut line = TryVec::new();
            users::passwd_line(p, &mut line)?;
            Ok(line)
        }))?;
        let Some(line) = line.transpose()? else { return Ok(false) };
        liella::net::copy_z(&line, buf(line.len().saturating_add(1))?).ok_or(Errno::EFAULT)?;
        Ok(true)
    })();
    match r {
        Ok(true) => 0,
        Ok(false) => -1,
        Err(e) => {
            crate::errno::set(e);
            -1
        }
    }
}

// ------------------------------------------------------------------------------ group lists

/// `getgrouplist(user, group, groups, &ngroups)`: the count, or -1 when `groups` (room for
/// `*ngroups`, which `list` views) is too small; `*ngroups` is set to the count either way and
/// as many GIDs as fit are stored.
pub fn getgrouplist<'a>(user: KResult<&[u8]>, group: u32, ngroups: Option<&mut i32>, list: impl FnOnce(usize) -> KResult<&'a mut [u32]>) -> i32 {
    let (Ok(user), Some(ngroups)) = (user, ngroups) else {
        crate::errno::set(Errno::EFAULT);
        return -1;
    };
    let gids = match nss::group_list(&crate::nscache::ctx(), user, group) {
        Ok(g) => g,
        Err(e) => {
            crate::errno::set(e);
            return -1;
        }
    };
    let cap = usize::try_from(*ngroups).unwrap_or(0);
    if let Ok(out) = list(cap.min(gids.len())) {
        for (d, s) in out.iter_mut().zip(gids.iter()) {
            *d = *s;
        }
    }
    *ngroups = i32::try_from(gids.len()).unwrap_or(i32::MAX);
    if gids.len() > cap { -1 } else { *ngroups }
}

/// `initgroups(user, group)`: the supplementary groups become `user`'s groups and `group`.
pub fn initgroups(user: KResult<&[u8]>, group: u32) -> KResult<()> {
    let gids = nss::group_list(&crate::nscache::ctx(), user?, group)?;
    crate::setxid::groups(Ok(&gids))
}

// ------------------------------------------------------------------------------ lckpwdf

/// `lckpwdf()`: 0, or -1 (`errno` unchanged when this process holds the lock already).
pub fn lckpwdf() -> i32 {
    match acct::lock_passwd(&CSys, acct::PWD_LOCK, acct::PWD_LOCK_TIMEOUT_MS) {
        Ok(true) => 0,
        Ok(false) => -1,
        Err(e) => {
            crate::errno::set(e);
            -1
        }
    }
}

/// `ulckpwdf()`: 0, or -1 when the lock was not held.
pub fn ulckpwdf() -> i32 {
    if acct::unlock_passwd(&CSys) { 0 } else { -1 }
}

// ------------------------------------------------------------------------------ shells, ttyslot

/// `getusershell()`: the next shell (in the calling thread's buffer, D-C0.2), 0 at the end;
/// 0 with `ENOMEM` when the thread's buffer cannot be had.
pub fn getusershell() -> usize {
    acct::next_shell(&CSys, acct::SHELLS, |s| {
        let mut z = [0u8; 4097];
        // A longer line is no path the kernel would take; it is cut to fit.
        let _ = liella::net::copy_z(s.get(..s.len().min(4096)).unwrap_or(&[]), &mut z);
        match result_buf::<4097>(USER_SHELL) {
            Ok(buf) => {
                buf.store(&z);
                buf.addr()
            }
            Err(e) => {
                crate::errno::set(e);
                0
            }
        }
    })
    .unwrap_or(0)
}

/// `setusershell()` and `endusershell()`.
pub fn rewind_shells() {
    acct::rewind_shells(&CSys);
}

/// `ttyslot()`: the index of the session's terminal in `/etc/ttys`, 0 when there is none.
pub fn ttyslot() -> i32 {
    let mut line = [0u8; 256];
    let Some(n) = acct::session_tty(&CSys, &mut line) else { return 0 };
    i32::try_from(acct::tty_slot(&CSys, acct::TTYS, line.get(..n).unwrap_or(&[]))).unwrap_or(0)
}

// ------------------------------------------------------------------------------ login name

/// The session's login name (`getlogin`), given to `f`.
fn login_name<T>(f: impl FnOnce(&[u8]) -> T) -> KResult<T> {
    let mut line = [0u8; 256];
    let tty = acct::session_tty(&CSys, &mut line).and_then(|n| line.get(..n));
    acct::login_name(&CSys, acct::SYSTEM_LOGIN, tty, f)
}

/// `getlogin()`: the name in the calling thread's block.
pub fn getlogin() -> KResult<usize> {
    login_name(|name| -> KResult<usize> {
        let (addr, bytes) = block_bytes(LOGIN, name.len().saturating_add(1))?;
        liella::net::copy_z(name, bytes).ok_or(Errno::ENOMEM)?;
        Ok(addr)
    })?
}

/// `getlogin_r(buf, size)`: the error number (`ERANGE` when the name and its terminator do not
/// fit).
pub fn getlogin_r(buf: KResult<&mut [u8]>) -> i32 {
    let r = login_name(|name| -> KResult<()> { liella::net::copy_z(name, buf?).map(|_| ()).ok_or(Errno::ERANGE) });
    match r.and_then(|r| r) {
        Ok(()) => 0,
        Err(e) => e.get(),
    }
}

/// `setlogin(name)`: not available on Linux (`ENOSYS`, as the reference).
pub fn setlogin(_name: usize) -> KResult<()> {
    Err(Errno::ENOSYS)
}

// ------------------------------------------------------------------------------ getpass

/// `getpass`'s process-wide result block: its address and size (grown as needed, kept).
static PASS_ADDR: AtomicUsize = AtomicUsize::new(0);
static PASS_SIZE: AtomicUsize = AtomicUsize::new(0);
/// Serializes `getpass` (it changes the terminal).
static PASS_LOCK: RawMutex = RawMutex::new();

/// `getpass`'s default bound (D-17.8): the most bytes of input before the newline, 1 MiB.
pub const PASS_DEFAULT_LIMIT: usize = 1 << 20;
/// The bound in force for the process (`liella_getpass_set_limit`); 0 stands for the default.
static PASS_LIMIT: AtomicUsize = AtomicUsize::new(0);

/// The bound `getpass` applies now.
fn pass_limit() -> usize {
    match PASS_LIMIT.load(Relaxed) {
        0 => PASS_DEFAULT_LIMIT,
        n => n,
    }
}

/// `liella_getpass_set_limit(bytes)`: the process's bound on `getpass`'s line (0: the default
/// again); the bound that was in force. More than `PTRDIFF_MAX` is `EINVAL`.
pub fn set_pass_limit(bytes: usize) -> KResult<usize> {
    if isize::try_from(bytes).is_err() {
        return Err(Errno::EINVAL);
    }
    Ok(match PASS_LIMIT.swap(bytes, Relaxed) {
        0 => PASS_DEFAULT_LIMIT,
        n => n,
    })
}

/// The `termios` local-mode bits `getpass` turns off: `ECHO`, `ECHOE`, `ECHOK`, `ECHONL`.
const ECHO_BITS: u32 = 0o10 | 0o20 | 0o40 | 0o100;

/// Zero a buffer that held input (D-X.11's zeroing; the tests observe it).
fn wipe(buf: &mut [u8]) {
    #[cfg(feature = "selftest")]
    let dirty = crate::selftest::pass_wipe_before(buf);
    conv::secure_zero(buf);
    #[cfg(feature = "selftest")]
    crate::selftest::pass_wipe_after(dirty, buf);
}

/// The line `getpass` is reading. Its storage is zeroed before it is let go of: when it grows
/// (after the bytes are copied to the larger block) and when the line is dropped.
struct Secret(TryVec<u8>);

impl Secret {
    fn new() -> Secret {
        Secret(TryVec::new())
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// Append `more`.
    fn push(&mut self, more: &[u8]) -> KResult<()> {
        let len = self.0.len();
        let need = len.checked_add(more.len()).ok_or(Errno::ENOMEM)?;
        if need > self.0.capacity() {
            let mut bigger = TryVec::with_capacity(need.max(len.saturating_mul(2)).max(64))?;
            bigger.extend_from_slice(self.0.as_slice())?;
            let mut old = core::mem::replace(&mut self.0, bigger);
            wipe(old.as_mut_slice());
        }
        self.0.extend_from_slice(more)
    }

    /// Zero and forget what was read.
    fn clear(&mut self) {
        wipe(self.0.as_mut_slice());
        self.0.clear();
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(self.0.as_mut_slice());
    }
}

/// Read one line from `fd` (until a newline or the end): its bytes without the newline. A line
/// longer than `limit` is read to its end, discarded and `EOVERFLOW`.
fn read_line_fd(fd: Fd, limit: usize) -> KResult<Secret> {
    let mut line = Secret::new();
    let mut chunk = [0u8; 256];
    let mut over = false;
    let r = loop {
        let n = match CSys.read(fd, &mut chunk) {
            Ok(n) => n,
            Err(e) => break Err(e),
        };
        let got = chunk.get(..n).unwrap_or(&[]);
        let (part, end) = match got.iter().position(|&c| c == b'\n') {
            Some(i) => (got.get(..i).unwrap_or(&[]), true),
            None => (got, n == 0),
        };
        if !over && line.len().saturating_add(part.len()) > limit {
            line.clear();
            over = true;
        }
        if !over {
            if let Err(e) = line.push(part) {
                break Err(e);
            }
        }
        if end {
            break if over { Err(Errno::EOVERFLOW) } else { Ok(()) };
        }
    };
    wipe(&mut chunk);
    r.map(|()| line)
}

/// Read one line from the stream `fp`, without the newline; a line longer than `limit` as
/// [`read_line_fd`].
fn read_line_stream(fp: usize, limit: usize) -> KResult<Secret> {
    let mut line = Secret::new();
    let mut over = false;
    while let Some(c) = stdio::getc(fp, Locking::Locked)? {
        if c == b'\n' {
            break;
        }
        if over {
            continue;
        }
        if line.len() >= limit {
            line.clear();
            over = true;
            continue;
        }
        line.push(&[c])?;
    }
    if over {
        return Err(Errno::EOVERFLOW);
    }
    Ok(line)
}

/// Write all of `text` to `fd`.
fn write_all(fd: Fd, mut text: &[u8]) -> KResult<()> {
    while !text.is_empty() {
        let n = CSys.write(fd, text)?;
        text = text.get(n.max(1)..).unwrap_or(&[]);
    }
    Ok(())
}

/// With echo off on `fd` (when it is a terminal), run `f`; the terminal is restored after. The
/// result and whether echo was turned off.
fn without_echo<T>(fd: Fd, f: impl FnOnce() -> KResult<T>) -> (KResult<T>, bool) {
    let saved = CSys.tcgets(fd).ok();
    let quiet = saved.is_some_and(|t| {
        let mut q = t;
        q.lflag &= !ECHO_BITS;
        CSys.tcsets(fd, TcSet::Flush, &q).is_ok()
    });
    let r = f();
    if let (Some(t), true) = (saved, quiet) {
        let _ = CSys.tcsets(fd, TcSet::Flush, &t);
    }
    (r, quiet)
}

/// `getpass(prompt)`: on the controlling terminal (`/dev/tty`), or else on standard input with
/// the prompt on standard error, read one line with echo off and restore the terminal; the line
/// (without its newline) in the process-wide buffer. The end of the input before a newline ends
/// the line (an empty one at once). A line of more than the bound (D-17.8: 1 MiB unless
/// `liella_getpass_set_limit` changed it) is never returned cut short: it is read to its end
/// and discarded, and the call fails with `EOVERFLOW`. Every buffer of the library's that held
/// the line is zeroed before it is let go of (a stream's own buffer belongs to the stream), and
/// the result block is zeroed before each new line goes in.
pub fn getpass(prompt: KResult<&[u8]>) -> KResult<usize> {
    let prompt = prompt?;
    let _g = PASS_LOCK.lock(&CSys);
    let limit = pass_limit();
    let line = match KStr::from_nul_terminated(b"/dev/tty\0").and_then(|p| CSys.openat(Fd::CWD, p, O_RDWR | O_NOCTTY | O_CLOEXEC, 0)) {
        Ok(tty) => {
            let (line, quiet) = without_echo(tty, || {
                write_all(tty, prompt)?;
                read_line_fd(tty, limit)
            });
            if quiet {
                let _ = write_all(tty, b"\n");
            }
            let _ = CSys.close(tty);
            line?
        }
        Err(_) => {
            let err = stdio::stderr();
            stdio::fputs(Ok(prompt), err, Locking::Locked)?;
            stdio::fflush(err, Locking::Locked)?;
            let (line, quiet) = without_echo(Fd::STDIN, || read_line_stream(stdio::stdin(), limit));
            if quiet {
                let _ = stdio::fputs(Ok(b"\n"), err, Locking::Locked);
                let _ = stdio::fflush(err, Locking::Locked);
            }
            line?
        }
    };
    let need = line.len().checked_add(1).ok_or(Errno::ENOMEM)?;
    let (old, old_size) = (PASS_ADDR.load(Relaxed), PASS_SIZE.load(Relaxed));
    let (addr, size) = if old != 0 && old_size >= need {
        (old, old_size)
    } else {
        // A larger block: the old one (the previous line) is zeroed before it is released.
        let mut fresh = (0, 0);
        grown(need, 0, 0, |a, n| fresh = (a, n))?;
        if let Some(b) = conv::own_bytes(old, old_size) {
            wipe(b);
        }
        caller_alloc::release(old);
        PASS_ADDR.store(fresh.0, Relaxed);
        PASS_SIZE.store(fresh.1, Relaxed);
        fresh
    };
    let out = conv::own_bytes(addr, size).ok_or(Errno::ENOMEM)?;
    wipe(out);
    liella::net::copy_z(line.bytes(), out).ok_or(Errno::ENOMEM)?;
    Ok(addr)
}

// ------------------------------------------------------------------------------ login records

/// A login record result in the calling thread's block: its address.
fn own_utmp(u: &Utmp) -> KResult<usize> {
    let (rec, _, _) = block::<struct_utmp>(UT, 0)?;
    *rec = pack::utmp(u);
    Ok(core::ptr::from_mut(rec).expose_provenance())
}

/// `utmpname(file)`.
pub fn utmpname(file: KResult<&[u8]>) -> KResult<()> {
    UTMP_DB.set_name(&CSys, file?)
}

/// `setutent()`: an open failure is only reported by the next call that needs the file.
pub fn setutent() {
    let _ = UTMP_DB.rewind(&CSys);
}

/// `endutent()`.
pub fn endutent() {
    UTMP_DB.close(&CSys);
}

/// `getutent()`: NULL at the end, `errno` unchanged.
pub fn getutent() -> KResult<usize> {
    UTMP_DB.next(&CSys)?.map_or(Ok(0), |u| own_utmp(&u))
}

/// `getutid(key)`.
pub fn getutid(key: Option<&struct_utmp>) -> KResult<usize> {
    own_utmp(&UTMP_DB.find_id(&CSys, &pack::utmp_in(key.ok_or(Errno::EINVAL)?))?)
}

/// `getutline(key)`.
pub fn getutline(key: Option<&struct_utmp>) -> KResult<usize> {
    own_utmp(&UTMP_DB.find_line(&CSys, &pack::utmp_in(key.ok_or(Errno::EINVAL)?))?)
}

/// The `_r` forms: the record in `out`, `*result` set to it (or NULL); 0, or -1 with `errno`
/// (none at the end of `getutent_r`).
fn utmp_r(r: KResult<Option<Utmp>>, out: Option<&mut struct_utmp>, result: Option<&mut usize>) -> i32 {
    let Some(result) = result else {
        crate::errno::set(Errno::EFAULT);
        return -1;
    };
    *result = 0;
    let r = r.and_then(|u| match (u, out) {
        (Some(u), Some(out)) => {
            *out = pack::utmp(&u);
            Ok(Some(core::ptr::from_mut(out).expose_provenance()))
        }
        (Some(_), None) => Err(Errno::EFAULT),
        (None, _) => Ok(None),
    });
    match r {
        Ok(Some(a)) => {
            *result = a;
            0
        }
        Ok(None) => -1,
        Err(e) => {
            crate::errno::set(e);
            -1
        }
    }
}

/// `getutent_r(buf, &result)`.
pub fn getutent_r(out: Option<&mut struct_utmp>, result: Option<&mut usize>) -> i32 {
    utmp_r(UTMP_DB.next(&CSys), out, result)
}

/// `getutid_r(key, buf, &result)`.
pub fn getutid_r(key: Option<&struct_utmp>, out: Option<&mut struct_utmp>, result: Option<&mut usize>) -> i32 {
    let r = key.ok_or(Errno::EINVAL).and_then(|k| UTMP_DB.find_id(&CSys, &pack::utmp_in(k))).map(Some);
    utmp_r(r, out, result)
}

/// `getutline_r(key, buf, &result)`.
pub fn getutline_r(key: Option<&struct_utmp>, out: Option<&mut struct_utmp>, result: Option<&mut usize>) -> i32 {
    let r = key.ok_or(Errno::EINVAL).and_then(|k| UTMP_DB.find_line(&CSys, &pack::utmp_in(k))).map(Some);
    utmp_r(r, out, result)
}

/// `pututline(rec)`: the caller's record, as the reference returns it.
pub fn pututline(rec: Option<&struct_utmp>) -> KResult<usize> {
    let rec = rec.ok_or(Errno::EINVAL)?;
    UTMP_DB.put(&CSys, &pack::utmp_in(rec))?;
    Ok(core::ptr::from_ref(rec).expose_provenance())
}

/// `updwtmp(file, rec)`: failures only set `errno`.
pub fn updwtmp(file: KResult<&[u8]>, rec: Option<&struct_utmp>) -> KResult<()> {
    let (file, rec) = (file?, rec.ok_or(Errno::EINVAL)?);
    let path = liella_kernel::path::KPathBuf::new(file)?;
    acct::append(&CSys, path.as_kstr().with_nul(), &pack::utmp_in(rec))
}

/// `getutmp(ux, u)` and `getutmpx(u, ux)`: the two layouts are the same (Appendix C).
pub fn copy_utmp(from: Option<&struct_utmp>, to: Option<&mut struct_utmp>) -> KResult<()> {
    *to.ok_or(Errno::EFAULT)? = *from.ok_or(Errno::EFAULT)?;
    Ok(())
}

/// `login(rec)`: a copy of `rec` as a user process of this process on the session's terminal
/// is written to `utmp` (in place of the record with its `ut_id`) and appended to `wtmp`. With
/// no terminal on standard input, output or error, the line is `???` and only `wtmp` gets it
/// (as `login(3)` documents). Failures are not reported.
pub fn login(rec: Option<&struct_utmp>) -> KResult<()> {
    let mut u = pack::utmp_in(rec.ok_or(Errno::EINVAL)?);
    u.kind = users::USER_PROCESS;
    u.pid = CSys.getpid();
    let mut line = [0u8; 256];
    match acct::session_tty(&CSys, &mut line) {
        Some(n) => {
            users::set_field(&mut u.line, line.get(..n).unwrap_or(&[]));
            let _ = acct::put_in(&CSys, acct::UTMP, &u);
        }
        None => users::set_field(&mut u.line, b"???"),
    }
    let _ = acct::append(&CSys, acct::WTMP, &u);
    Ok(())
}

/// `logout(line)`: 1 when the session on `line` was marked dead in `utmp`, else 0.
pub fn logout(line: KResult<&[u8]>) -> i32 {
    let r = line.and_then(|l| acct::logout_in(&CSys, acct::UTMP, l, acct::now(&CSys)?));
    i32::from(r.unwrap_or(false))
}

/// `logwtmp(line, name, host)`: append a record of this process at the current time to `wtmp`.
pub fn logwtmp(line: KResult<&[u8]>, name: KResult<&[u8]>, host: KResult<&[u8]>) -> KResult<()> {
    let rec = acct::wtmp_record(line?, name?, host?, CSys.getpid(), acct::now(&CSys)?);
    acct::append(&CSys, acct::WTMP, &rec)
}

/// `L_cuserid`: `cuserid`'s buffer size (`<stdio.h>`, ABI).
const L_CUSERID: usize = 9;

/// `cuserid(s)` (§12.12, D-12.15): the name of the effective user ID's entry in the user
/// database (the reference's choice, ⚑ HIST), cut to `L_cuserid` - 1 bytes, with its terminator;
/// in `s` (which holds `L_cuserid` bytes, all written: the name zero-filled, observed) or, for
/// NULL, in the calling thread's `getlogin` block (D-C0.2; cuserid(3): the string may be
/// overwritten by `getlogin`). Without an entry: NULL for NULL and an empty string in `s` (only
/// its first byte written), `errno` untouched, as `getpwuid`'s "not found" (the reference's
/// answers, probed 2026-10-04 in a user namespace and over a fixture database,
/// `spec/behaviour/users.md`); a lookup that fails is NULL with its error. The result's address.
pub fn cuserid(s: usize) -> KResult<usize> {
    let mut name = [0u8; L_CUSERID];
    let found = nss::found(nss::passwd_cached(&CSys, &crate::nscache::MEMORY, Key::Id(CSys.geteuid()), |p| {
        let n = p.name.len().min(L_CUSERID - 1);
        if let (Some(d), Some(src)) = (name.get_mut(..n), p.name.get(..n)) {
            d.copy_from_slice(src);
        }
    }))?;
    let Some(()) = found else {
        if s != 0 {
            conv::out_bytes(s, 1).fill(0);
        }
        return Ok(s);
    };
    if s != 0 {
        conv::out_bytes(s, L_CUSERID).copy_from_slice(&name);
        return Ok(s);
    }
    let len = name.iter().position(|&b| b == 0).unwrap_or(L_CUSERID - 1).saturating_add(1);
    let (addr, bytes) = block_bytes(LOGIN, len)?;
    bytes.copy_from_slice(name.get(..len).unwrap_or(&[0]));
    Ok(addr)
}
