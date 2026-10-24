//! Result conversion for exports (IP-12): the pure `liella_abi::ret` encoders plus storing the
//! error in the calling thread's `errno`, pointer results, and the S6 rule (a contract violation
//! where the C interface has no error channel is a diagnostic plus abort, never UB).

use core::cmp::Ordering;
use core::ffi::c_void;

use liella_abi::ctypes::{c_int, ssize_t, wint_t};
use liella_abi::ret::{self as enc, Ret};
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::sys::Sys;
use liella_kernel::Fd;

use crate::errno;
use crate::sys::CSys;

fn finish<T>(r: Ret<T>) -> T {
    if let Some(e) = r.errno {
        errno::set(e);
    }
    r.value
}

/// `0` / `-1` with `errno`.
pub fn zero(r: KResult<()>) -> c_int {
    finish(enc::zero(r))
}

/// A non-negative `int` / `-1` with `errno`.
pub fn int(r: KResult<i64>) -> c_int {
    finish(enc::int(r))
}

/// A byte count / `-1` with `errno`.
pub fn ssize(r: KResult<usize>) -> ssize_t {
    finish(enc::ssize(r))
}

/// A raw system-call result as `ssize_t` (CANCEL-FAST): a non-negative result is the value; a
/// negative one goes the general way ([`ssize`] of [`Errno::decode`]), out of line.
#[inline]
pub fn raw_ssize(r: i64) -> ssize_t {
    if r >= 0 { ssize_t::try_from(r).unwrap_or(ssize_t::MAX) } else { raw_ssize_fail(r) }
}

#[cold]
#[inline(never)]
fn raw_ssize_fail(r: i64) -> ssize_t {
    ssize(Errno::decode(r).map(|v| usize::try_from(v).unwrap_or(usize::MAX)))
}

/// A raw system-call result as `0` / `-1` with `errno` (CANCEL-FAST), as [`zero`] of
/// [`Errno::decode`].
#[inline]
pub fn raw_zero(r: i64) -> c_int {
    if r >= 0 { 0 } else { zero(Errno::decode(r).map(|_| ())) }
}

/// A conversion's error as a raw system-call result (CANCEL-FAST): `-errno`.
#[inline]
pub fn raw_err(e: Errno) -> i64 {
    i64::from(e.get()).wrapping_neg()
}

/// The error number itself, `errno` untouched.
pub fn errnum(r: KResult<()>) -> c_int {
    finish(enc::errnum(r))
}

/// The error number itself, also stored in `errno` (`ttyname_r`, `ptsname_r`).
pub fn errnum_errno(r: KResult<()>) -> c_int {
    match r {
        Ok(()) => 0,
        Err(e) => {
            errno::set(e);
            e.get()
        }
    }
}

/// A `size_t` count / `(size_t)-1` with `errno`.
pub fn size_or_neg1(r: KResult<usize>) -> usize {
    finish(enc::size_or_neg1(r))
}

/// An address / `NULL` with `errno`.
pub fn null<T>(r: KResult<usize>) -> *mut T {
    core::ptr::with_exposed_provenance_mut(finish(enc::null(r)))
}

/// A value that only reports an error through `errno` (the value is returned either way, as
/// `strtol`'s clamped value is).
pub fn value_with<T>(v: T, e: Option<Errno>) -> T {
    if let Some(e) = e {
        errno::set(e);
    }
    v
}

/// A value and the `errno` it reports (`strtol`'s clamped value with `ERANGE`).
pub fn with_errno<T>(r: (T, Option<Errno>)) -> T {
    value_with(r.0, r.1)
}

/// The value of a call with no error channel; a failure (only possible through a contract
/// violation such as a NULL argument) aborts with a diagnostic (S6).
pub fn must<T>(r: KResult<T>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => fatal(e),
    }
}

/// `base + index`, or abort as [`must`].
pub fn at<T>(base: *const T, r: KResult<usize>) -> *mut T {
    base.cast_mut().wrapping_add(must(r))
}

/// `base + index`, `NULL` for `None`, or abort as [`must`].
pub fn found<T>(base: *const T, r: KResult<Option<usize>>) -> *mut T {
    must(r).map_or(core::ptr::null_mut(), |i| base.cast_mut().wrapping_add(i))
}

/// An order as `-1`, `0` or `1` (only the sign is specified), or abort as [`must`].
pub fn order(r: KResult<Ordering>) -> c_int {
    match must(r) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// An `int` result of a call with no error channel.
pub fn int_must(r: KResult<i64>) -> c_int {
    c_int::try_from(must(r)).unwrap_or(c_int::MAX)
}

/// `()` of a call with no error channel.
pub fn unit(r: KResult<()>) {
    must(r)
}

/// Success as a nonzero `int`, failure as 0 with `errno` (`hcreate`, `hsearch_r`).
pub fn nonzero(r: KResult<()>) -> c_int {
    match r {
        Ok(()) => 1,
        Err(e) => {
            errno::set(e);
            0
        }
    }
}

/// Success as 0, failure as 1 with `errno` (`addmntent`).
pub fn zero_or_one(r: KResult<()>) -> c_int {
    match r {
        Ok(()) => 0,
        Err(e) => {
            errno::set(e);
            1
        }
    }
}

/// A `void` call whose failures only set `errno` (`hdestroy_r(NULL)`).
pub fn errno_only(r: KResult<()>) {
    if let Err(e) = r {
        errno::set(e);
    }
}

/// A C truth value: 1 or 0.
pub fn truth(b: bool) -> c_int {
    c_int::from(b)
}

/// A new descriptor, or -1 with `errno`.
pub fn fd(r: KResult<liella_kernel::Fd>) -> c_int {
    int(r.map(|f| i64::from(f.raw())))
}

/// A count (`select`, `poll`, `epoll_wait`), or -1 with `errno`.
pub fn count(r: KResult<usize>) -> c_int {
    int(r.map(|n| i64::try_from(n).unwrap_or(i64::MAX)))
}

/// A file offset (`lseek`), or -1 with `errno`.
pub fn off(r: KResult<u64>) -> i64 {
    match r {
        Ok(v) => i64::try_from(v).unwrap_or(i64::MAX),
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `__fdelt_chk`'s result: the word index, or the fortify failure for a descriptor outside
/// `[0, FD_SETSIZE)`.
pub fn fdelt(r: Option<u64>) -> i64 {
    match r {
        Some(i) => i64::try_from(i).unwrap_or(0),
        None => chk_fail(),
    }
}

/// A non-negative `int` result of a typed call (`pkey_alloc`, `inotify_add_watch`).
pub fn int32(r: KResult<i32>) -> c_int {
    int(r.map(i64::from))
}

/// A call without a result that reports `int` 0 or -1 (`readahead` returns `ssize_t` 0).
pub fn ssize_zero(r: KResult<()>) -> ssize_t {
    ssize(r.map(|()| 0))
}

/// A registration's result (`atexit`, `__cxa_atexit`, ...): 0, or -1 without memory.
pub fn registered(ok: bool) -> c_int {
    if ok { 0 } else { -1 }
}

/// A line number passed as `int` (`__assert`), as the unsigned value the message prints.
pub fn line(n: c_int) -> u32 {
    u32::try_from(n).unwrap_or(0)
}

/// An address, or `(void *)-1` with `errno` (`sbrk`).
pub fn addr_or_neg1<T>(r: KResult<usize>) -> *mut T {
    match r {
        Ok(a) => core::ptr::with_exposed_provenance_mut(a),
        Err(e) => {
            errno::set(e);
            core::ptr::without_provenance_mut(usize::MAX)
        }
    }
}

/// A typed address result (0 is NULL).
pub fn ptr<T>(a: usize) -> *mut T {
    core::ptr::with_exposed_provenance_mut(a)
}

/// An untyped address result.
pub fn addr(a: usize) -> *mut c_void {
    core::ptr::with_exposed_provenance_mut(a)
}

/// Write a diagnostic to standard error and terminate with `SIGABRT` (spec §2.4 `abort`
/// semantics: the signal is unblocked and, if caught, re-raised with the default action).
pub fn fatal(e: Errno) -> ! {
    let sys = CSys;
    let _ = sys.write(Fd::STDERR, b"liella: fatal: invalid argument to a C library function (");
    let _ = sys.write(Fd::STDERR, e.name().unwrap_or("error").as_bytes());
    let _ = sys.write(Fd::STDERR, b")\n");
    abort()
}

/// A fortify failure (spec §2.5 `__chk_fail`): the fixed diagnostic, then abort.
pub fn chk_fail() -> ! {
    let _ = CSys.write(Fd::STDERR, b"*** buffer overflow detected ***: terminated\n");
    abort()
}

/// The pointer argument itself, once the call (with no error channel) succeeded (`memset`,
/// `strcpy`, `memmove` return their destination).
pub fn same<T, U>(p: *mut T, r: KResult<U>) -> *mut T {
    must(r);
    p
}

/// `base + offset` units, `base` given as a mutable pointer (`stpcpy`, `mempcpy`).
pub fn done<T>(base: *mut T, r: KResult<usize>) -> *mut T {
    base.wrapping_add(must(r))
}

/// A path component as located by `basename`/`dirname`: in place in `base`, or the static
/// `"."` (D-4.9).
pub fn path_part(base: *mut i8, r: KResult<liella::bytes::PathPart>) -> *mut i8 {
    match must(r) {
        liella::bytes::PathPart::InPlace { start, .. } => base.wrapping_add(start),
        liella::bytes::PathPart::Dot => crate::data::DOT.as_ptr().cast_mut().cast::<i8>(),
    }
}

/// The `arc4random` family never fails: without entropy the process terminates with the
/// fixed diagnostic (spec §9.1).
pub fn arc4<T>(v: Option<T>) -> T {
    match v {
        Some(v) => v,
        None => {
            let _ = CSys.write(Fd::STDERR, b"Fatal glibc error: cannot get entropy for arc4random\n");
            abort()
        }
    }
}

/// The architecture trap instruction (`abort`'s last resort, spec §1.2).
pub fn trap() {
    crate::ffi::liella_machine_trap();
}

/// Terminate with `SIGABRT`.
pub fn abort() -> ! {
    liella_rt::fatal::abort(&CSys, trap)
}

/// A structure result stored into the caller's object (`stat`, `statfs`, `statvfs`): 0, or -1
/// with `errno` (`EFAULT` for a NULL destination).
pub fn store<T>(dst: Option<&mut T>, r: KResult<T>) -> c_int {
    zero(r.and_then(|v| dst.map(|d| *d = v).ok_or(Errno::EFAULT)))
}

/// A structure the kernel writes straight into the caller's object (`stat`, `fstatat`, `statx`):
/// `fill` gets the object itself, so the result is not built in a temporary and copied (WL-HOT-DU-2).
/// A NULL destination gets the call made into a temporary and then `EFAULT`, as [`store`] does.
#[inline]
pub fn store_into<T: Default>(dst: Option<&mut T>, fill: impl FnOnce(&mut T) -> KResult<()>) -> c_int {
    match dst {
        Some(d) => zero(fill(d)),
        None => {
            let mut t = T::default();
            store(None, fill(&mut t).map(|()| t))
        }
    }
}

/// A descriptor pair stored into the caller's `int[2]` (`pipe`, `pipe2`): 0, or -1 with `errno`.
pub fn pair(dst: Option<&mut [c_int; 2]>, r: KResult<[Fd; 2]>) -> c_int {
    zero(r.and_then(|[a, b]| dst.map(|d| *d = [a.raw(), b.raw()]).ok_or(Errno::EFAULT)))
}

/// Three IDs stored into the caller's cells (`getresuid`, `getresgid`): 0, or -1 with `errno`
/// (`EFAULT` for a NULL cell).
pub fn triple(a: Option<&mut u32>, b: Option<&mut u32>, c: Option<&mut u32>, r: KResult<(u32, u32, u32)>) -> c_int {
    zero(r.and_then(|(x, y, z)| match (a, b, c) {
        (Some(a), Some(b), Some(c)) => {
            (*a, *b, *c) = (x, y, z);
            Ok(())
        }
        _ => Err(Errno::EFAULT),
    }))
}

/// A result stored into the caller's object when one is given (`clock_getres`, `adjtime`'s
/// old delta): 0, or -1 with `errno`.
pub fn store_if<T>(dst: Option<&mut T>, r: KResult<T>) -> c_int {
    zero(r.map(|v| {
        if let Some(d) = dst {
            *d = v;
        }
    }))
}

/// A result stored into the caller's cell and reported as an error number
/// (`clock_getcpuclockid`): 0, or the error (`EFAULT` for a NULL cell).
pub fn errnum_store<T>(dst: Option<&mut T>, r: KResult<T>) -> c_int {
    errnum(r.and_then(|v| dst.map(|d| *d = v).ok_or(Errno::EFAULT)))
}

/// `time(t)`'s result: the value, also stored when `t` is given; -1 with `errno`.
pub fn time(dst: Option<&mut i64>, r: KResult<i64>) -> i64 {
    match r {
        Ok(v) => {
            if let Some(d) = dst {
                *d = v;
            }
            v
        }
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// Two results, each stored when its destination is given (`gettimeofday`): 0, or -1 with
/// `errno`.
pub fn store_both<A, B>(a: Option<&mut A>, b: Option<&mut B>, r: KResult<(A, B)>) -> c_int {
    zero(r.map(|(x, y)| {
        if let Some(a) = a {
            *a = x;
        }
        if let Some(b) = b {
            *b = y;
        }
    }))
}

/// A value with a structure stored when its destination is given (`times`): the value, or -1
/// with `errno`.
pub fn value_store<T>(dst: Option<&mut T>, r: KResult<(i64, T)>) -> i64 {
    match r {
        Ok((v, s)) => {
            if let Some(d) = dst {
                *d = s;
            }
            v
        }
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// A state with a structure stored into the caller's object (`ntp_gettime`): the state, or -1
/// with `errno` (`EFAULT` for a NULL destination).
pub fn state_store<T>(dst: Option<&mut T>, r: KResult<(i32, T)>) -> c_int {
    int(r.and_then(|(state, s)| dst.map(|d| *d = s).ok_or(Errno::EFAULT).map(|()| i64::from(state))))
}

/// C11's `timespec_get`/`timespec_getres` result: the base with the time stored when the
/// destination is given, or 0 (an unsupported base, a failed read).
pub fn base_store(dst: Option<&mut liella_kernel::uapi::Timespec>, r: (c_int, Option<liella_kernel::uapi::Timespec>)) -> c_int {
    match (r, dst) {
        ((base, Some(t)), Some(d)) => {
            *d = t;
            base
        }
        ((base, Some(_)), None) => base,
        _ => 0,
    }
}

/// A signed `int` result where every value is valid (`nice`, `getpriority`): the value, or -1
/// with `errno` (callers clear `errno` to tell them apart).
pub fn signed(r: KResult<i64>) -> c_int {
    match r {
        Ok(v) => c_int::try_from(v).unwrap_or(c_int::MAX),
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// A `long` result where every value is valid (`syscall`, `ulimit`): the value, or -1 with
/// `errno`.
pub fn long(r: KResult<i64>) -> i64 {
    match r {
        Ok(v) => v,
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `sysconf`/`pathconf`'s three meanings (D-24.1): the value; -1 without `errno` for "no
/// limit"; -1 with `errno` for an error.
pub fn sysconf(r: KResult<Option<i64>>) -> i64 {
    match r {
        Ok(Some(v)) => v,
        Ok(None) => -1,
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// A size where 0 means failure (`confstr`): the size, or 0 with `errno`.
pub fn size0(r: KResult<usize>) -> usize {
    match r {
        Ok(v) => v,
        Err(e) => {
            errno::set(e);
            0
        }
    }
}

/// A handler result (`signal`, `sigset`): the old handler, or `SIG_ERR` with `errno`.
pub fn handler(r: KResult<usize>) -> usize {
    match r {
        Ok(h) => h,
        Err(e) => {
            errno::set(e);
            usize::MAX
        }
    }
}

/// An error-number result storing a value when the destination is given (`pthread_sigmask`'s
/// old mask).
pub fn errnum_if<T>(dst: Option<&mut T>, r: KResult<T>) -> c_int {
    errnum(r.map(|v| {
        if let Some(d) = dst {
            *d = v;
        }
    }))
}

/// An entry address (`readdir`): the address, `NULL` at the end (`errno` untouched), or `NULL`
/// with `errno`.
pub fn entry<T>(r: KResult<Option<usize>>) -> *mut T {
    match r {
        Ok(Some(a)) => core::ptr::with_exposed_provenance_mut(a),
        Ok(None) => core::ptr::null_mut(),
        Err(e) => {
            errno::set(e);
            core::ptr::null_mut()
        }
    }
}

/// An entry address whose contract sets `errno` to 0 when there is none (`fts_read`,
/// `fts_children`, fts(3)): the address, `NULL` with `errno` 0, or `NULL` with the error.
pub fn entry_or_zero<T>(r: KResult<Option<usize>>) -> *mut T {
    match r {
        Ok(Some(a)) => core::ptr::with_exposed_provenance_mut(a),
        Ok(None) => {
            errno::set_raw(0);
            core::ptr::null_mut()
        }
        Err(e) => {
            errno::set(e);
            core::ptr::null_mut()
        }
    }
}

/// The pointer argument itself on success (`mkdtemp` returns its template), `NULL` with
/// `errno` on failure.
pub fn this_or_null<T, U>(p: *mut T, r: KResult<U>) -> *mut T {
    match r {
        Ok(_) => p,
        Err(e) => {
            errno::set(e);
            core::ptr::null_mut()
        }
    }
}

/// An ID returned as an `int` (`setfsuid`/`setfsgid`: the previous ID).
pub fn id_int(v: u32) -> c_int {
    c_int::from_ne_bytes(v.to_ne_bytes())
}

// ------------------------------------------------------------------------------ stdio (§12)

/// `EOF`.
const EOF: c_int = -1;

/// `WEOF`.
const WEOF: wint_t = u32::MAX;

/// A byte read (`fgetc`): the byte as `unsigned char` in an `int`; `EOF` at end of input
/// (`errno` untouched) or on an error (with `errno`).
pub fn byte_or_eof(r: KResult<Option<u8>>) -> c_int {
    match r {
        Ok(Some(b)) => c_int::from(b),
        Ok(None) => EOF,
        Err(e) => {
            errno::set(e);
            EOF
        }
    }
}

/// A wide character read (`fgetwc`): the character, or `WEOF` at end of input or on an error
/// (with `errno`).
pub fn wint_or_weof(r: KResult<Option<u32>>) -> wint_t {
    match r {
        Ok(Some(c)) => c,
        Ok(None) => WEOF,
        Err(e) => {
            errno::set(e);
            WEOF
        }
    }
}

/// A wide character written (`fputwc`, `ungetwc`): the character, or `WEOF` with `errno`.
pub fn wint_written(r: KResult<u32>) -> wint_t {
    wint_or_weof(r.map(Some))
}

/// A byte written (`fputc`, `ungetc`): the byte as `unsigned char`, or `EOF` with `errno`.
pub fn byte_written(r: KResult<u8>) -> c_int {
    byte_or_eof(r.map(Some))
}

/// An `int` read (`getw`): the value, or `EOF` at end of input or on an error (with `errno`).
pub fn int_or_eof(r: KResult<Option<i32>>) -> c_int {
    match r {
        Ok(Some(v)) => v,
        Ok(None) => EOF,
        Err(e) => {
            errno::set(e);
            EOF
        }
    }
}

/// An `int` result, or `EOF` with `errno` (`__overflow`, `fwide`).
pub fn int_or_neg1(r: KResult<i32>) -> c_int {
    match r {
        Ok(v) => v,
        Err(e) => {
            errno::set(e);
            EOF
        }
    }
}

/// `fgets`: `buf`, or NULL at end of input before any byte (`errno` untouched) or on an
/// error (with `errno`).
pub fn buf_or_null<T>(buf: *mut T, r: KResult<Option<usize>>) -> *mut T {
    match r {
        Ok(Some(_)) => buf,
        Ok(None) => core::ptr::null_mut(),
        Err(e) => {
            errno::set(e);
            core::ptr::null_mut()
        }
    }
}

/// `getline`: the length, or -1 at end of input or on an error (with `errno`).
pub fn record(r: KResult<Option<usize>>) -> ssize_t {
    match r {
        Ok(Some(n)) => ssize_t::try_from(n).unwrap_or(ssize_t::MAX),
        Ok(None) => -1,
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `fread`/`fwrite`: the complete elements of `size` bytes moved (D-12.10); `errno` if the
/// transfer stopped on an error. `n` is the count asked for: a whole transfer (`n` elements of a
/// nonzero `size`) answers `n` without a division (WL-HOT-CUT: a `div` per small `fwrite` costs
/// ~25 cycles on the timing hosts); a partial one rounds down to whole elements.
#[inline]
pub fn elements(t: liella::stream::Transfer, size: usize, n: usize) -> usize {
    if let Some(e) = t.error {
        errno::set(e);
    }
    if size != 0 && size.checked_mul(n) == Some(t.done) {
        return n;
    }
    t.done.checked_div(size).unwrap_or(0)
}

/// A query on a stream (`__fbufsize`, ...): the value, 0 for a stream that is not one.
pub fn or_zero<T: Default>(r: KResult<T>) -> T {
    r.unwrap_or_default()
}

/// A signed offset (`ftello`), or -1 with `errno`.
pub fn off_signed(r: KResult<i64>) -> i64 {
    match r {
        Ok(v) => v,
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}
