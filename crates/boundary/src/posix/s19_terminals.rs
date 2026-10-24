//! Translation for spec §19: terminals, job control and pseudo-terminals (phase 11;
//! `crate::terminal`, `liella::term`, `liella_legacy::term_compat`). `ctermid` (§19.2) is in
//! `s12_stdio` and `login_tty` (§19.4) in `s17_users`, where earlier phases put them.
//!
//! The C `struct termios` is a typed parameter (D-X.8), converted to the kernel's settings by
//! `liella_abi::pack` (D-19.1). `tcdrain` is a cancellation point (§19.1); so are the calls that
//! open the pty multiplexer, as `open` is (`posix_openpt`, `getpt`, `openpty`, `forkpty`).
//! `forkpty` forks, so it is exported by the products only, as `fork` is. The compat versions of
//! `openpty`, `forkpty` and `login_tty` (`GLIBC_2.2.5`) wait for phase 15.

use liella_abi::ctypes::{c_char, c_int, c_uint, c_void, size_t};
use liella_abi::layout::{struct_termios, struct_ttyent, struct_winsize};
use liella_kernel::uapi::O_RDWR;
use liella::term;
use liella_legacy::stubs;
use liella_legacy::term_compat as tc;

use crate::cancel;
use crate::conv;
use crate::export;
use crate::ret;
use crate::strz;
use crate::sys::{CSys, CP};
use crate::terminal;

// ------------------------------------------------------------------------------ attributes (§19.1)

export! { "GLIBC_2.2.5";
pub extern "C" fn tcgetattr(fd: c_int, t: Option<&mut struct_termios>) -> c_int {
    ret::store(t, terminal::tcgetattr(fd))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tcsetattr(fd: c_int, action: c_int, t: Option<&struct_termios>) -> c_int {
    ret::zero(terminal::tcsetattr(fd, action, t))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cfgetospeed(t: Option<&struct_termios>) -> c_uint {
    ret::must(conv::req(t).map(tc::ospeed))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cfgetispeed(t: Option<&struct_termios>) -> c_uint {
    ret::must(conv::req(t).map(tc::ispeed))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cfsetospeed(t: Option<&mut struct_termios>, speed: c_uint) -> c_int {
    ret::zero(conv::req(t).and_then(|t| tc::set_ospeed(t, speed)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cfsetispeed(t: Option<&mut struct_termios>, speed: c_uint) -> c_int {
    ret::zero(conv::req(t).and_then(|t| tc::set_ispeed(t, speed)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cfsetspeed(t: Option<&mut struct_termios>, speed: c_uint) -> c_int {
    ret::zero(conv::req(t).and_then(|t| tc::set_speed(t, speed)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cfmakeraw(t: Option<&mut struct_termios>) {
    ret::unit(conv::req(t).map(tc::make_raw))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tcsendbreak(fd: c_int, duration: c_int) -> c_int {
    ret::zero(term::send_break(&CSys, conv::fd(fd), duration))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tcdrain(fd: c_int) -> c_int {
    cancel::point(ret::zero(term::drain(&CP, conv::fd(fd))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tcflush(fd: c_int, queue: c_int) -> c_int {
    ret::zero(term::flush(&CSys, conv::fd(fd), queue))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tcflow(fd: c_int, action: c_int) -> c_int {
    ret::zero(term::flow(&CSys, conv::fd(fd), action))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tcgetsid(fd: c_int) -> c_int {
    ret::int32(term::session(&CSys, conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isatty(fd: c_int) -> c_int {
    ret::nonzero(term::is_terminal(&CSys, conv::fd(fd)))
}}

// ------------------------------------------------------------------------------ job control (§19.2)

export! { "GLIBC_2.2.5";
pub extern "C" fn tcgetpgrp(fd: c_int) -> c_int {
    ret::int32(term::foreground(&CSys, conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tcsetpgrp(fd: c_int, pgrp: c_int) -> c_int {
    ret::zero(term::set_foreground(&CSys, conv::fd(fd), pgrp))
}}

// ------------------------------------------------------------------------------ names (§19.3)

export! { "GLIBC_2.2.5";
pub extern "C" fn ttyname(fd: c_int) -> *mut c_char {
    ret::null(terminal::ttyname(fd))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ttyname_r(fd: c_int, buf: *mut c_char, size: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds size bytes, or is NULL (EINVAL) (§19.3).
    ret::errnum_errno(terminal::ttyname_r(fd, unsafe { conv::opt_buf_mut(buf, size) }))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __ttyname_r_chk(fd: c_int, buf: *mut c_char, size: size_t, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds size bytes, checked against its object size buflen (C0.5).
    ret::errnum_errno(terminal::ttyname_r(fd, unsafe { conv::opt_buf_mut(buf, conv::chk(size, buflen)) }))
}}

// ------------------------------------------------------------------------------ pseudo-terminals (§19.4)

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_openpt(flags: c_int) -> c_int {
    cancel::point(ret::fd(term::open_master(&CP, conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpt() -> c_int {
    cancel::point(ret::fd(term::open_master(&CP, O_RDWR)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn grantpt(fd: c_int) -> c_int {
    ret::zero(term::check_master(&CSys, conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn unlockpt(fd: c_int) -> c_int {
    ret::zero(term::unlock(&CSys, conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ptsname(fd: c_int) -> *mut c_char {
    ret::null(terminal::ptsname(fd))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ptsname_r(fd: c_int, buf: *mut c_char, size: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds size bytes, or is NULL (EINVAL) (§19.4).
    ret::errnum_errno(terminal::ptsname_r(fd, unsafe { conv::opt_buf_mut(buf, size) }))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __ptsname_r_chk(fd: c_int, buf: *mut c_char, size: size_t, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds size bytes, checked against its object size buflen (C0.5).
    ret::errnum_errno(terminal::ptsname_r(fd, unsafe { conv::opt_buf_mut(buf, conv::chk(size, buflen)) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn openpty(amaster: Option<&mut c_int>, aslave: Option<&mut c_int>, name: *mut c_char, termp: Option<&struct_termios>, winp: Option<&struct_winsize>) -> c_int {
    cancel::point(ret::zero(terminal::openpty(amaster, aslave, conv::addr(name), termp, winp)))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn forkpty(amaster: Option<&mut c_int>, name: *mut c_char, termp: Option<&struct_termios>, winp: Option<&struct_winsize>) -> c_int {
    cancel::point(ret::int32(terminal::forkpty(amaster, conv::addr(name), termp, winp)))
}}

// ------------------------------------------------------------------------------ legacy interfaces (§19.5)

export! { "GLIBC_2.2.5";
pub extern "C" fn setttyent() -> c_int {
    ret::nonzero(terminal::setttyent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endttyent() -> c_int {
    terminal::endttyent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getttyent() -> *mut struct_ttyent {
    ret::null(terminal::getttyent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getttynam(name: *const c_char) -> *mut struct_ttyent {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§19.5).
    ret::null(terminal::getttynam(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gtty(_fd: c_int, _params: *mut c_void) -> c_int {
    ret::zero(stubs::bsd_terminal())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn stty(_fd: c_int, _params: *const c_void) -> c_int {
    ret::zero(stubs::bsd_terminal())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn revoke(_file: *const c_char) -> c_int {
    ret::zero(stubs::bsd_terminal())
}}
