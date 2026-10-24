//! Translation for spec §20: signals (phase 5: §20.1–20.9, except the cancellation and
//! set-ID uses of the internal signal, which are phase 6).
//!
//! The internal signal is 32 alone (D-20.1), so `SIGRTMIN` is 33. Actions and sets are
//! converted by `liella_legacy::signal_compat`, which never lets a C caller block, catch or see
//! the internal signal; the handler's address reaches the kernel through `crate::raw`, with
//! the library's restorer (D-20.2, ⚑ KERN). `signal` has BSD semantics (D-20.4). `psignal` writes through
//! the `stderr` stream; `psiginfo` writes to descriptor 2 behind the stream's back (as glibc's does).

use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_abi::layout::sigset_t;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SigInfo, StackT, Timespec};
use liella_legacy::signal_compat::{self, CSigaction, CSigstack, CSigvec};

use crate::cancel;
use crate::conv;
use crate::export;
use crate::raw;
use crate::ret;
use crate::strz;
use crate::sys::{self, CSys, CP};

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_current_sigrtmin() -> c_int {
    signal_compat::current_rtmin()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_current_sigrtmax() -> c_int {
    signal_compat::current_rtmax()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_allocate_rtsig(high: c_int) -> c_int {
    signal_compat::allocate_rtsig(conv::flag(high))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigaction(sig: c_int, act: Option<&CSigaction>, oact: Option<&mut CSigaction>) -> c_int {
    ret::store_if(oact, signal_compat::sigaction(sig, act, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sigaction(sig: c_int, act: Option<&CSigaction>, oact: Option<&mut CSigaction>) -> c_int {
    ret::store_if(oact, signal_compat::sigaction(sig, act, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn signal(sig: c_int, handler: usize) -> usize {
    ret::handler(signal_compat::bsd_signal(sig, handler, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn bsd_signal(sig: c_int, handler: usize) -> usize {
    ret::handler(signal_compat::bsd_signal(sig, handler, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ssignal(sig: c_int, handler: usize) -> usize {
    ret::handler(signal_compat::bsd_signal(sig, handler, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sysv_signal(sig: c_int, handler: usize) -> usize {
    ret::handler(signal_compat::sysv_signal(sig, handler, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sysv_signal(sig: c_int, handler: usize) -> usize {
    ret::handler(signal_compat::sysv_signal(sig, handler, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn siginterrupt(sig: c_int, flag: c_int) -> c_int {
    ret::zero(signal_compat::siginterrupt(sig, conv::flag(flag), sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigset(sig: c_int, disp: usize) -> usize {
    ret::handler(signal_compat::sigset(&CSys, sig, disp, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sighold(sig: c_int) -> c_int {
    ret::zero(signal_compat::sighold(&CSys, sig, true))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigrelse(sig: c_int) -> c_int {
    ret::zero(signal_compat::sighold(&CSys, sig, false))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigignore(sig: c_int) -> c_int {
    ret::zero(signal_compat::sigignore(sig, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigvec(sig: c_int, vec: Option<&CSigvec>, ovec: Option<&mut CSigvec>) -> c_int {
    ret::store_if(ovec, signal_compat::sigvec(sig, vec, sys::restorer_addr(), raw::sigaction))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigreturn(_context: *mut c_void) -> c_int {
    ret::zero(signal_compat::sigreturn())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gsignal(sig: c_int) -> c_int {
    ret::zero(signal_compat::raise(&CSys, sig))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn raise(sig: c_int) -> c_int {
    ret::zero(signal_compat::raise(&CSys, sig))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigprocmask(how: c_int, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) -> c_int {
    ret::store_if(oset, signal_compat::sigprocmask(&CSys, how, set))
}}

export! { "GLIBC_2.32";
pub extern "C" fn pthread_sigmask(how: c_int, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) -> c_int {
    ret::errnum_if(oset, signal_compat::sigprocmask(&CSys, how, set))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigemptyset(set: Option<&mut sigset_t>) -> c_int {
    ret::zero(conv::req(set).map(signal_compat::emptyset))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigfillset(set: Option<&mut sigset_t>) -> c_int {
    ret::zero(conv::req(set).map(signal_compat::fillset))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigaddset(set: Option<&mut sigset_t>, sig: c_int) -> c_int {
    ret::zero(conv::req(set).and_then(|s| signal_compat::addset(s, sig)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigdelset(set: Option<&mut sigset_t>, sig: c_int) -> c_int {
    ret::zero(conv::req(set).and_then(|s| signal_compat::delset(s, sig)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigismember(set: Option<&sigset_t>, sig: c_int) -> c_int {
    ret::int(conv::req(set).and_then(|s| signal_compat::ismember(s, sig)).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sigaddset(set: Option<&mut sigset_t>, sig: c_int) -> c_int {
    conv::req(set).map_or(0, |s| signal_compat::raw_op(s, sig, b'+'))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sigdelset(set: Option<&mut sigset_t>, sig: c_int) -> c_int {
    conv::req(set).map_or(0, |s| signal_compat::raw_op(s, sig, b'-'))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sigismember(set: Option<&sigset_t>, sig: c_int) -> c_int {
    conv::req(set).map_or(0, |s| signal_compat::raw_ismember(s, sig))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigisemptyset(set: Option<&sigset_t>) -> c_int {
    ret::int(conv::req(set).map(|s| i64::from(signal_compat::isemptyset(s))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigandset(dest: Option<&mut sigset_t>, a: Option<&sigset_t>, b: Option<&sigset_t>) -> c_int {
    ret::zero(conv::req(dest).and_then(|d| conv::req(a).and_then(|a| conv::req(b).map(|b| signal_compat::combine_into(d, a, b, false)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigorset(dest: Option<&mut sigset_t>, a: Option<&sigset_t>, b: Option<&sigset_t>) -> c_int {
    ret::zero(conv::req(dest).and_then(|d| conv::req(a).and_then(|a| conv::req(b).map(|b| signal_compat::combine_into(d, a, b, true)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigblock(mask: c_int) -> c_int {
    ret::signed(signal_compat::sigblock(&CSys, mask).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigsetmask(mask: c_int) -> c_int {
    ret::signed(signal_compat::sigsetmask(&CSys, mask).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn siggetmask() -> c_int {
    ret::signed(signal_compat::siggetmask(&CSys).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sigpause(sig_or_mask: c_int, is_sig: c_int) -> c_int {
    ret::zero(signal_compat::sigpause(&CSys, sig_or_mask, conv::flag(is_sig)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigpause(mask: c_int) -> c_int {
    ret::zero(signal_compat::sigpause(&CSys, mask, false))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __xpg_sigpause(sig: c_int) -> c_int {
    ret::zero(signal_compat::sigpause(&CSys, sig, true))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn kill(pid: c_int, sig: c_int) -> c_int {
    ret::zero(CSys.kill(pid, sig))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn killpg(pgrp: c_int, sig: c_int) -> c_int {
    ret::zero(signal_compat::killpg(&CSys, pgrp, sig))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigqueue(pid: c_int, sig: c_int, value: u64) -> c_int {
    ret::zero(signal_compat::sigqueue(&CSys, pid, sig, value))
}}

export! { "GLIBC_2.30";
pub extern "C" fn tgkill(tgid: c_int, tid: c_int, sig: c_int) -> c_int {
    ret::zero(CSys.tgkill(tgid, tid, sig))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pause() -> c_int {
    cancel::point(ret::zero(CP.pause()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigsuspend(mask: Option<&sigset_t>) -> c_int {
    cancel::point(ret::zero(conv::req(mask).and_then(|m| signal_compat::sigsuspend(&CP, m))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sigsuspend(mask: Option<&sigset_t>) -> c_int {
    cancel::point(ret::zero(conv::req(mask).and_then(|m| signal_compat::sigsuspend(&CP, m))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigwait(set: Option<&sigset_t>, sig: Option<&mut c_int>) -> c_int {
    cancel::point(ret::errnum_store(sig, conv::req(set).and_then(|s| signal_compat::sigwait(&CP, s))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigwaitinfo(set: Option<&sigset_t>, info: Option<&mut SigInfo>) -> c_int {
    cancel::point(ret::int32(conv::req(set).and_then(|s| signal_compat::sigtimedwait(&CP, s, info, None))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigtimedwait(set: Option<&sigset_t>, info: Option<&mut SigInfo>, timeout: Option<&Timespec>) -> c_int {
    cancel::point(ret::int32(conv::req(set).and_then(|s| signal_compat::sigtimedwait(&CP, s, info, timeout))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigpending(set: Option<&mut sigset_t>) -> c_int {
    ret::store(set, signal_compat::sigpending(&CSys))
}}

export! { "GLIBC_2.7";
pub extern "C" fn signalfd(fd: c_int, mask: Option<&sigset_t>, flags: c_int) -> c_int {
    ret::fd(conv::req(mask).and_then(|m| CSys.signalfd(conv::opt_fd(fd), &signal_compat::kernel_set(m), conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigaltstack(ss: Option<&StackT>, oss: Option<&mut StackT>) -> c_int {
    ret::store_if(oss, raw::sigaltstack(ss))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sigstack(ss: Option<&CSigstack>, oss: Option<&mut CSigstack>) -> c_int {
    ret::store_if(oss, signal_compat::sigstack(ss.is_some(), || raw::sigaltstack(None)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strsignal(sig: c_int) -> *mut c_char {
    ret::ptr(signal_compat::strsignal(sig, crate::thread::result_slot(liella_rt::thread::SLOT_STRSIGNAL)))
}}

export! { "GLIBC_2.32";
pub extern "C" fn sigdescr_np(sig: c_int) -> *const c_char {
    ret::ptr(signal_compat::text_addr(signal_compat::sigdescr(sig)))
}}

export! { "GLIBC_2.32";
pub extern "C" fn sigabbrev_np(sig: c_int) -> *const c_char {
    ret::ptr(signal_compat::text_addr(signal_compat::sigabbrev(sig)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn psignal(sig: c_int, s: *const c_char) {
    // UNSAFE(slice) SAFETY: s is NULL or a NUL-terminated string (§20.8).
    unsafe { strz::strz_opt(s) }.map_or((), |p| psignal_stream(sig, p))
}}

/// `psiginfo` writes each piece to descriptor 2 itself, not through the stream (glibc 2.39: ahead of
/// the stream's pending text, even after `fclose(stderr)`, no error flag); a failed write leaves its
/// error in `errno`, a successful one leaves `errno` alone.
fn psiginfo_direct(bytes: &[u8]) {
    if let Err(e) = Sys::write(&CSys, liella_kernel::Fd::new(2), bytes) {
        crate::errno::set(e);
    }
}

/// `psignal`'s text goes through the `stderr` stream, as `perror`'s does: after any text pending
/// in its buffer, in its orientation (an undecided stream becomes byte-oriented), setting its
/// error flag when descriptor 2 cannot be written, and nothing at all once the stream is closed
/// (observed on glibc 2.39). `errno` is kept unless the write failed. One write when memory allows.
fn psignal_stream(sig: c_int, prefix: Option<&[u8]>) {
    let saved = crate::errno::get();
    let mut whole = liella::mem::TryVec::new();
    let mut joined = true;
    signal_compat::psignal(sig, prefix, |b| joined = joined && whole.extend_from_slice(b).is_ok());
    if joined {
        crate::report::emit(&[whole.as_slice()], true, saved);
    } else {
        signal_compat::psignal(sig, prefix, |b| crate::report::emit(&[b], true, saved));
    }
}

export! { "GLIBC_2.10";
pub extern "C" fn psiginfo(info: Option<&SigInfo>, s: *const c_char) {
    // UNSAFE(slice) SAFETY: s is NULL or a NUL-terminated string (§20.8).
    unsafe { strz::strz_opt(s) }.ok().zip(info).map_or((), |(p, i)| signal_compat::psiginfo(i, p, psiginfo_direct))
}}
