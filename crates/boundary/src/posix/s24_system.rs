//! Translation for spec §24: the system (phase 5: §24.1 limits, §24.2 usage, §24.3 priorities
//! and scheduling, §24.4 configuration queries, and §24.6's `syscall`); phase 10: §24.5's
//! `gethostid` and `sethostid` (`crate::netaddr`); the rest of §24.5–24.6 in the validation
//! stage (it had been left out of phase 13's scope; validation T2), except the compat entries
//! of removed system calls (`bdflush`, `create_module`, `get_kernel_syms`, `query_module`,
//! `nfsservctl`, `sstk`, `sysctl`, `__sysctl`, `uselib`), which are phase 15's.
//!
//! `sysconf`'s three meanings share one return channel (D-24.1): the value, -1 without `errno`
//! for "no limit", -1 with `errno` for an unknown name. The fixed answers are the reference
//! glibc's (`liella_legacy::sysconf_table`, generated from `spec/abi/sysconf.tsv`), the computed
//! ones `liella::system`'s. `ulimit` and `syscall` are variadic: their C entry points are
//! adapters (`csys/src/adapter/variadic.c`) forwarding to `liella_ulimit2` and
//! `liella_syscall7` (OD-C1).

use liella::system;
use liella_abi::ctypes::{c_char, c_int, c_long, c_uint, c_ulong, c_void, size_t};
use liella_kernel::errno::Errno;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, Rusage, SchedParam, Sysinfo, Timespec, Utsname};
use liella_legacy::sys_compat;

use crate::conv;
use crate::export;
use crate::nr;
use crate::posix::s03_memory;
use crate::process;
use crate::raw;
use crate::ret;
use crate::strz;
use crate::sys::CSys;

export! { "GLIBC_2.2.5";
pub extern "C" fn getrlimit(resource: c_int, rl: Option<&mut Rlimit>) -> c_int {
    ret::store(rl, CSys.prlimit(0, conv::bits(resource), None))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getrlimit64(resource: c_int, rl: Option<&mut Rlimit>) -> c_int {
    ret::store(rl, CSys.prlimit(0, conv::bits(resource), None))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setrlimit(resource: c_int, rl: Option<&Rlimit>) -> c_int {
    ret::zero(conv::req(rl).and_then(|l| CSys.prlimit(0, conv::bits(resource), Some(l))).map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setrlimit64(resource: c_int, rl: Option<&Rlimit>) -> c_int {
    ret::zero(conv::req(rl).and_then(|l| CSys.prlimit(0, conv::bits(resource), Some(l))).map(drop))
}}

export! { "GLIBC_2.13";
pub extern "C" fn prlimit(pid: c_int, resource: c_int, new: Option<&Rlimit>, old: Option<&mut Rlimit>) -> c_int {
    ret::store_if(old, CSys.prlimit(pid, conv::bits(resource), new))
}}

export! { "GLIBC_2.13";
pub extern "C" fn prlimit64(pid: c_int, resource: c_int, new: Option<&Rlimit>, old: Option<&mut Rlimit>) -> c_int {
    ret::store_if(old, CSys.prlimit(pid, conv::bits(resource), new))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_ulimit2(cmd: c_int, newlimit: c_long) -> c_long {
    ret::long(sys_compat::ulimit(&CSys, &CSys, &process::ProcessAux, cmd, newlimit))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vlimit(resource: c_int, value: c_int) -> c_int {
    ret::zero(sys_compat::vlimit(&CSys, resource, value))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getrusage(who: c_int, usage: Option<&mut Rusage>) -> c_int {
    ret::store(usage, CSys.getrusage(who))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vtimes(current: Option<&mut sys_compat::Vtimes>, child: Option<&mut sys_compat::Vtimes>) -> c_int {
    ret::store_both(current, child, sys_compat::vtimes(&CSys))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn nice(inc: c_int) -> c_int {
    ret::signed(sys_compat::nice(&CSys, inc))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpriority(which: c_int, who: c_uint) -> c_int {
    ret::signed(sys_compat::getpriority(&CSys, conv::bits(which), who))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setpriority(which: c_int, who: c_uint, prio: c_int) -> c_int {
    ret::zero(CSys.setpriority(conv::bits(which), who, prio))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_setscheduler(pid: c_int, policy: c_int, param: Option<&SchedParam>) -> c_int {
    ret::zero(conv::req(param).and_then(|p| CSys.sched_setscheduler(pid, conv::bits(policy), p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sched_setscheduler(pid: c_int, policy: c_int, param: Option<&SchedParam>) -> c_int {
    ret::zero(conv::req(param).and_then(|p| CSys.sched_setscheduler(pid, conv::bits(policy), p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_getscheduler(pid: c_int) -> c_int {
    ret::int(CSys.sched_getscheduler(pid).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sched_getscheduler(pid: c_int) -> c_int {
    ret::int(CSys.sched_getscheduler(pid).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_setparam(pid: c_int, param: Option<&SchedParam>) -> c_int {
    ret::zero(conv::req(param).and_then(|p| CSys.sched_setparam(pid, p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_getparam(pid: c_int, param: Option<&mut SchedParam>) -> c_int {
    ret::store(param, CSys.sched_getparam(pid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sched_getparam(pid: c_int, param: Option<&mut SchedParam>) -> c_int {
    ret::store(param, CSys.sched_getparam(pid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_get_priority_min(policy: c_int) -> c_int {
    ret::int32(CSys.sched_get_priority_min(conv::bits(policy)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sched_get_priority_min(policy: c_int) -> c_int {
    ret::int32(CSys.sched_get_priority_min(conv::bits(policy)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_get_priority_max(policy: c_int) -> c_int {
    ret::int32(CSys.sched_get_priority_max(conv::bits(policy)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sched_get_priority_max(policy: c_int) -> c_int {
    ret::int32(CSys.sched_get_priority_max(conv::bits(policy)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_rr_get_interval(pid: c_int, ts: Option<&mut Timespec>) -> c_int {
    ret::store(ts, CSys.sched_rr_get_interval(pid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sched_yield() -> c_int {
    ret::zero(sys_compat::sched_yield(&CSys))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sched_yield() -> c_int {
    ret::zero(sys_compat::sched_yield(&CSys))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn sched_setaffinity(pid: c_int, size: size_t, mask: *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: mask holds size bytes (§24.3).
    ret::zero(unsafe { conv::buf(mask, size) }.and_then(|m| CSys.sched_setaffinity(pid, m)))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn sched_getaffinity(pid: c_int, size: size_t, mask: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: mask holds size bytes (§24.3).
    ret::zero(unsafe { conv::buf_mut(mask, size) }.and_then(|m| sys_compat::getaffinity(&CSys, pid, m)))
}}

export! { "GLIBC_2.29";
pub extern "C" fn getcpu(cpu: Option<&mut c_uint>, node: Option<&mut c_uint>) -> c_int {
    ret::store_both(cpu, node, CSys.getcpu())
}}

export! { "GLIBC_2.6";
pub extern "C" fn sched_getcpu() -> c_int {
    ret::int(CSys.getcpu().map(|(cpu, _)| i64::from(cpu)))
}}

export! { "GLIBC_2.6";
pub extern "C" fn __sched_cpucount(size: size_t, set: *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: set holds size bytes (§24.3).
    ret::int(unsafe { conv::buf(set, size) }.map(sys_compat::cpu_count))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __sched_cpualloc(count: size_t) -> *mut c_void {
    s03_memory::malloc(sys_compat::cpu_alloc_size(count))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __sched_cpufree(set: *mut c_void) {
    s03_memory::free(set)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sysconf(name: c_int) -> c_long {
    ret::sysconf(sys_compat::sysconf(&CSys, &CSys, &process::ProcessAux, name))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sysconf(name: c_int) -> c_long {
    ret::sysconf(sys_compat::sysconf(&CSys, &CSys, &process::ProcessAux, name))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pathconf(path: *const c_char, name: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§24.4).
    ret::sysconf(unsafe { strz::path(path) }.and_then(|p| sys_compat::pathconf(&CSys, sys_compat::Target::Path(p), name)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fpathconf(fd: c_int, name: c_int) -> c_long {
    ret::sysconf(sys_compat::pathconf(&CSys, sys_compat::Target::Fd(conv::fd(fd)), name))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn confstr(name: c_int, buf: *mut c_char, len: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: buf is NULL or holds len bytes (§24.4).
    ret::size0(unsafe { conv::opt_buf_mut(buf, len) }.and_then(|b| sys_compat::confstr(name, b.unwrap_or_default())))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __confstr_chk(name: c_int, buf: *mut c_char, len: size_t, buflen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as confstr, after the fortify check.
    ret::size0(unsafe { conv::opt_buf_mut(buf, conv::chk(len, buflen)) }.and_then(|b| sys_compat::confstr(name, b.unwrap_or_default())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpagesize() -> c_int {
    ret::int(sys_compat::sysconf(&CSys, &CSys, &process::ProcessAux, 30).map(|v| v.unwrap_or(4096)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __getpagesize() -> c_int {
    ret::int(sys_compat::sysconf(&CSys, &CSys, &process::ProcessAux, 30).map(|v| v.unwrap_or(4096)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn get_nprocs() -> c_int {
    ret::int(Ok(system::nprocs(&CSys)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn get_nprocs_conf() -> c_int {
    ret::int(Ok(system::nprocs_conf(&CSys)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn get_phys_pages() -> c_long {
    ret::sysconf(sys_compat::sysconf(&CSys, &CSys, &process::ProcessAux, 85))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn get_avphys_pages() -> c_long {
    ret::sysconf(sys_compat::sysconf(&CSys, &CSys, &process::ProcessAux, 86))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getdtablesize() -> c_int {
    ret::int(sys_compat::sysconf(&CSys, &CSys, &process::ProcessAux, 4).map(|v| v.unwrap_or(2_147_483_647)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_syscall7(nr: c_long, a: c_long, b: c_long, c: c_long, d: c_long, e: c_long, f: c_long) -> c_long {
    ret::long(raw::syscall(nr, [a, b, c, d, e, f]))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostid() -> c_long {
    crate::netaddr::gethostid()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sethostid(id: c_long) -> c_int {
    ret::zero(crate::netaddr::sethostid(id))
}}

/// A register-wide argument of a thin system-call wrapper (an address or an unsigned long, as
/// the kernel reads it).
fn word<T: Into<u64>>(v: T) -> i64 {
    i64::from_ne_bytes(v.into().to_ne_bytes())
}

/// An address argument of a thin system-call wrapper.
fn addr_word<T>(p: *const T) -> i64 {
    i64::from_ne_bytes(conv::addr(p).to_ne_bytes())
}

// ------------------------------------------------------------------------------ §24.5 identity

export! { "GLIBC_2.2.5";
pub extern "C" fn uname(u: Option<&mut Utsname>) -> c_int {
    ret::store(u, CSys.uname())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostname(name: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: name holds len writable bytes (§24.5).
    ret::zero(unsafe { conv::buf_mut(name.cast(), len) }.and_then(|b| system::hostname(&CSys, b)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __gethostname_chk(name: *mut c_char, len: size_t, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: as gethostname, after the fortify check.
    ret::zero(unsafe { conv::buf_mut(name.cast(), conv::chk(len, buflen)) }.and_then(|b| system::hostname(&CSys, b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getdomainname(name: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: name holds len writable bytes (§24.5).
    ret::zero(unsafe { conv::buf_mut(name.cast(), len) }.and_then(|b| system::domainname(&CSys, b)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __getdomainname_chk(name: *mut c_char, len: size_t, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: as getdomainname, after the fortify check.
    ret::zero(unsafe { conv::buf_mut(name.cast(), conv::chk(len, buflen)) }.and_then(|b| system::domainname(&CSys, b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sethostname(name: *const c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: name holds len bytes (§24.5).
    ret::zero(unsafe { conv::buf(name.cast(), len) }.and_then(|b| CSys.sethostname(b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setdomainname(name: *const c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: name holds len bytes (§24.5).
    ret::zero(unsafe { conv::buf(name.cast(), len) }.and_then(|b| CSys.setdomainname(b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gnu_get_libc_version() -> *const c_char {
    ret::ptr(sys_compat::LIBC_VERSION.as_ptr().expose_provenance())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gnu_get_libc_release() -> *const c_char {
    ret::ptr(sys_compat::LIBC_RELEASE.as_ptr().expose_provenance())
}}

// ------------------------------------------------------------------------------ §24.6 status and control

export! { "GLIBC_2.2.5";
pub extern "C" fn sysinfo(info: Option<&mut Sysinfo>) -> c_int {
    ret::store(info, CSys.sysinfo())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getloadavg(loadavg: *mut f64, nelem: c_int) -> c_int {
    // At most three; none for a count of 0 or less (the reference returns 0 for a negative one).
    let n = usize::try_from(nelem).unwrap_or(0).min(3);
    // UNSAFE(slice) SAFETY: loadavg holds nelem doubles (§24.6); only the first n are written.
    ret::int(unsafe { conv::buf_mut(loadavg.cast(), n.saturating_mul(8)) }.and_then(|out| {
        let mut v = [0f64; 3];
        let got = system::loadavg(&CSys, v.get_mut(..n).unwrap_or(&mut []))?;
        for (d, x) in out.chunks_exact_mut(8).zip(v.iter()).take(got) {
            d.copy_from_slice(&x.to_ne_bytes());
        }
        Ok(i64::try_from(got).unwrap_or(0))
    }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn reboot(howto: c_int) -> c_int {
    // The kernel's magic numbers (⚑ KERN, reboot(2)).
    ret::zero(CSys.reboot(0xfee1_dead, 672_274_793, conv::bits(howto), None))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn klogctl(kind: c_int, buf: *mut c_char, len: c_int) -> c_int {
    // A thin wrapper (§24.6): the kernel checks the buffer for the actions that use it.
    ret::int(raw::syscall(nr::SYSLOG, [i64::from(kind), addr_word(buf.cast_const()), i64::from(len), 0, 0, 0]))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn acct(path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is NULL or a NUL-terminated string (§24.6).
    ret::zero(unsafe { strz::opt_path(path) }.and_then(|p| CSys.acct(p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn quotactl(cmd: c_int, special: *const c_char, id: c_int, addr: *mut c_char) -> c_int {
    // A thin wrapper (§24.6): what `addr` names depends on the command; the kernel checks it.
    ret::zero(raw::syscall(nr::QUOTACTL, [i64::from(cmd), addr_word(special), i64::from(id), addr_word(addr.cast_const()), 0, 0]).map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ioperm(from: c_ulong, num: c_ulong, turn_on: c_int) -> c_int {
    ret::zero(CSys.ioperm(from, num, turn_on != 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iopl(level: c_int) -> c_int {
    ret::zero(CSys.iopl(conv::bits(level)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vhangup() -> c_int {
    ret::zero(CSys.vhangup())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn capget(hdr: *mut c_void, data: *mut c_void) -> c_int {
    // A thin wrapper (§24.6): the header's version says how much data the kernel uses.
    ret::zero(raw::syscall(nr::CAPGET, [addr_word(hdr.cast_const()), addr_word(data.cast_const()), 0, 0, 0, 0]).map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn capset(hdr: *mut c_void, data: *const c_void) -> c_int {
    ret::zero(raw::syscall(nr::CAPSET, [addr_word(hdr.cast_const()), addr_word(data), 0, 0, 0, 0]).map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn init_module(image: *mut c_void, len: c_ulong, params: *const c_char) -> c_int {
    ret::zero(raw::syscall(nr::INIT_MODULE, [addr_word(image.cast_const()), word(len), addr_word(params), 0, 0, 0]).map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn delete_module(name: *const c_char, flags: c_uint) -> c_int {
    ret::zero(raw::syscall(nr::DELETE_MODULE, [addr_word(name), i64::from(flags), 0, 0, 0, 0]).map(drop))
}}

// The removed module and NFS-server calls and `uselib` (§24.6): compat-only names
// (`name@GLIBC_2.2.5`, exported as `liella_compat_<name>` behind the generated `.symver` stubs of
// `csys/src/machine/compat_aliases.S`, R15-1/R15-2). Each still makes its system call, as the
// reference's do (a probe that made those numbers fail with a marker errno saw the marker); a
// current kernel answers `ENOSYS` (spec/behaviour/compat.md). `uselib` works where the kernel keeps
// CONFIG_USELIB.
crate::internal_entry! {
/// `create_module@GLIBC_2.2.5`: the kernel's address, or `(caddr_t)-1` with `errno`.
pub extern "C" fn liella_compat_create_module(name: *const c_char, size: size_t) -> *mut c_char {
    ret::addr_or_neg1(raw::syscall(nr::CREATE_MODULE, [addr_word(name), i64::from_ne_bytes(size.to_ne_bytes()), 0, 0, 0, 0]).map(|a| usize::from_ne_bytes(a.to_ne_bytes())))
}}

crate::internal_entry! {
/// `get_kernel_syms@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_get_kernel_syms(table: *mut c_void) -> c_int {
    ret::int(raw::syscall(nr::GET_KERNEL_SYMS, [addr_word(table.cast_const()), 0, 0, 0, 0, 0]))
}}

crate::internal_entry! {
/// `query_module@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_query_module(name: *const c_char, which: c_int, buf: *mut c_void, bufsize: size_t, out: *mut size_t) -> c_int {
    ret::int(raw::syscall(nr::QUERY_MODULE, [addr_word(name), i64::from(which), addr_word(buf.cast_const()), i64::from_ne_bytes(bufsize.to_ne_bytes()), addr_word(out.cast_const()), 0]))
}}

crate::internal_entry! {
/// `nfsservctl@GLIBC_2.2.5` (a `long` result: the reference returns all 64 bits of -1).
pub extern "C" fn liella_compat_nfsservctl(cmd: c_int, arg: *mut c_void, res: *mut c_void) -> c_long {
    ret::off(raw::syscall(nr::NFSSERVCTL, [i64::from(cmd), addr_word(arg.cast_const()), addr_word(res.cast_const()), 0, 0, 0]).map(|v| u64::from_ne_bytes(v.to_ne_bytes())))
}}

crate::internal_entry! {
/// `uselib@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_uselib(library: *const c_char) -> c_int {
    ret::zero(raw::syscall(nr::USELIB, [addr_word(library), 0, 0, 0, 0, 0]).map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn modify_ldt(func: c_int, ptr: *mut c_void, bytecount: c_ulong) -> c_int {
    // A thin wrapper (§24.6): reads fill `ptr`, writes read one descriptor from it. The kernel
    // returns a 32-bit `int` zero-extended to the register (observed on 6.8 and 5.10: -ENOSYS
    // arrives as 4294967258), so the low 32 bits are the result: a negative one is -1 with
    // `errno` (modify_ldt(2)), where the reference returns it as is with `errno` untouched.
    ret::int(raw::syscall(nr::MODIFY_LDT, [i64::from(func), addr_word(ptr.cast_const()), word(bytecount), 0, 0, 0]).and_then(|v| {
        let v = i32::from_ne_bytes(u32::try_from(v & 0xffff_ffff).unwrap_or(0).to_ne_bytes());
        match v {
            -4095..=-1 => Err(Errno::new(v.saturating_neg()).unwrap_or(Errno::EINVAL)),
            v => Ok(i64::from(v)),
        }
    }))
}}
