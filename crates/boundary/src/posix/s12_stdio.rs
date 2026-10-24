//! Translation for spec §12: buffered streams (D-12.11). Each export converts its arguments and
//! calls `crate::stdio`, which runs `liella_legacy::stdio_compat` on the stream.

use core::ffi::c_void;

use liella_abi::ctypes::{c_char, c_int, off_t, size_t, ssize_t, wchar_t, wint_t};
use liella_abi::layout::{cookie_io_functions_t, fpos_t, struct_obstack, struct_printf_info, FILE};
use liella_legacy::printf_registry;
use liella_legacy::stdio_compat::{EOF_SEEN, ERR_SEEN};

use crate::conv;
use crate::export;
use crate::printf;
use crate::ret;
use crate::scanf;
use crate::stdio::{self, Locking};
use crate::strz;

// ------------------------------------------------------------------------------ §12.2

export! { "GLIBC_2.2.5";
pub extern "C" fn fopen(path: *const c_char, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: path and mode are strings (§12.2).
    ret::null(stdio::fopen(unsafe { strz::path(path) }, unsafe { strz::strz(mode) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fopen64(path: *const c_char, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: as fopen.
    ret::null(stdio::fopen(unsafe { strz::path(path) }, unsafe { strz::strz(mode) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fdopen(fd: c_int, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: mode is a string (§12.2).
    ret::null(stdio::fdopen(fd, unsafe { strz::strz(mode) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn freopen(path: *const c_char, mode: *const c_char, fp: *mut FILE) -> *mut FILE {
    // UNSAFE(slice) SAFETY: path is NULL or a string; mode is a string (§12.2).
    ret::null(stdio::freopen(unsafe { strz::opt_path(path) }, unsafe { strz::strz(mode) }, conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn freopen64(path: *const c_char, mode: *const c_char, fp: *mut FILE) -> *mut FILE {
    // UNSAFE(slice) SAFETY: as freopen.
    ret::null(stdio::freopen(unsafe { strz::opt_path(path) }, unsafe { strz::strz(mode) }, conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fclose(fp: *mut FILE) -> c_int {
    ret::zero(stdio::fclose(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fcloseall() -> c_int {
    ret::zero(stdio::fcloseall())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fileno(fp: *mut FILE) -> c_int {
    ret::int(stdio::fileno(conv::addr(fp), Locking::Locked).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fileno_unlocked(fp: *mut FILE) -> c_int {
    ret::int(stdio::fileno(conv::addr(fp), Locking::Unlocked).map(i64::from))
}}

// ------------------------------------------------------------------------------ §12.4

export! { "GLIBC_2.2.5";
pub extern "C" fn fflush(fp: *mut FILE) -> c_int {
    ret::zero(stdio::fflush(conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fflush_unlocked(fp: *mut FILE) -> c_int {
    ret::zero(stdio::fflush(conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setvbuf(fp: *mut FILE, buf: *mut c_char, mode: c_int, size: size_t) -> c_int {
    ret::zero(stdio::setvbuf(conv::addr(fp), conv::addr(buf), mode, size))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setbuf(fp: *mut FILE, buf: *mut c_char) {
    ret::unit(stdio::setvbuf(conv::addr(fp), conv::addr(buf), conv::flag_mode(conv::addr(buf), 0, 2), liella_legacy::stdio_compat::BUFSIZ))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setbuffer(fp: *mut FILE, buf: *mut c_char, size: size_t) {
    ret::unit(stdio::setvbuf(conv::addr(fp), conv::addr(buf), conv::flag_mode(conv::addr(buf), 0, 2), size))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setlinebuf(fp: *mut FILE) {
    ret::unit(stdio::setvbuf(conv::addr(fp), 0, 1, 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fpurge(fp: *mut FILE) {
    stdio::fpurge(conv::addr(fp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _flushlbf() {
    stdio::flushlbf()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fbufsize(fp: *mut FILE) -> size_t {
    ret::or_zero(stdio::info(conv::addr(fp)).map(|i| i.bufsize))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fpending(fp: *mut FILE) -> size_t {
    ret::or_zero(stdio::info(conv::addr(fp)).map(|i| i.pending))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __flbf(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::info(conv::addr(fp)).map(|i| i.line_buffered)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __freading(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::info(conv::addr(fp)).map(|i| i.reading)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fwriting(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::info(conv::addr(fp)).map(|i| i.writing)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __freadable(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::info(conv::addr(fp)).map(|i| i.readable)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fwritable(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::info(conv::addr(fp)).map(|i| i.writable)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fsetlocking(fp: *mut FILE, how: c_int) -> c_int {
    ret::or_zero(stdio::fsetlocking(conv::addr(fp), how))
}}

// ------------------------------------------------------------------------------ §12.5

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetc(fp: *mut FILE) -> c_int {
    stdio::getc_c(conv::addr(fp), Locking::Locked)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getc(fp: *mut FILE) -> c_int {
    stdio::getc_c(conv::addr(fp), Locking::Locked)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getchar() -> c_int {
    stdio::getc_c(stdio::stdin(), Locking::Locked)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetc_unlocked(fp: *mut FILE) -> c_int {
    stdio::getc_c(conv::addr(fp), Locking::Unlocked)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getc_unlocked(fp: *mut FILE) -> c_int {
    stdio::getc_c(conv::addr(fp), Locking::Unlocked)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getchar_unlocked() -> c_int {
    stdio::getc_c(stdio::stdin(), Locking::Unlocked)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __uflow(fp: *mut FILE) -> c_int {
    ret::byte_or_eof(stdio::getc(conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __underflow(fp: *mut FILE) -> c_int {
    ret::byte_or_eof(stdio::peek(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputc(c: c_int, fp: *mut FILE) -> c_int {
    stdio::putc_c(conv::addr(fp), Locking::Locked, c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putc(c: c_int, fp: *mut FILE) -> c_int {
    stdio::putc_c(conv::addr(fp), Locking::Locked, c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putchar(c: c_int) -> c_int {
    stdio::putc_c(stdio::stdout(), Locking::Locked, c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputc_unlocked(c: c_int, fp: *mut FILE) -> c_int {
    stdio::putc_c(conv::addr(fp), Locking::Unlocked, c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putc_unlocked(c: c_int, fp: *mut FILE) -> c_int {
    stdio::putc_c(conv::addr(fp), Locking::Unlocked, c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putchar_unlocked(c: c_int) -> c_int {
    stdio::putc_c(stdio::stdout(), Locking::Unlocked, c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __overflow(fp: *mut FILE, c: c_int) -> c_int {
    ret::int_or_neg1(stdio::overflow(conv::addr(fp), c))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ungetc(c: c_int, fp: *mut FILE) -> c_int {
    ret::byte_written(stdio::ungetc(conv::addr(fp), c))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgets(s: *mut c_char, n: c_int, fp: *mut FILE) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s has room for n bytes (§12.5).
    ret::buf_or_null(s, stdio::fgets(unsafe { conv::buf_mut(s.cast::<c_void>(), conv::nonneg(n)) }, conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgets_unlocked(s: *mut c_char, n: c_int, fp: *mut FILE) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as fgets.
    ret::buf_or_null(s, stdio::fgets(unsafe { conv::buf_mut(s.cast::<c_void>(), conv::nonneg(n)) }, conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __fgets_chk(s: *mut c_char, size: size_t, n: c_int, fp: *mut FILE) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s has room for size bytes; the slice is at most that (§12.5, C0.5).
    ret::buf_or_null(s, stdio::fgets_chk(unsafe { conv::buf_mut(s.cast::<c_void>(), conv::nonneg(n).min(size)) }, conv::nonneg(n), conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __fgets_unlocked_chk(s: *mut c_char, size: size_t, n: c_int, fp: *mut FILE) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as __fgets_chk.
    ret::buf_or_null(s, stdio::fgets_chk(unsafe { conv::buf_mut(s.cast::<c_void>(), conv::nonneg(n).min(size)) }, conv::nonneg(n), conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputs(s: *const c_char, fp: *mut FILE) -> c_int {
    // UNSAFE(slice) SAFETY: s is a string (§12.5).
    ret::count(stdio::fputs(unsafe { strz::strz(s) }, conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputs_unlocked(s: *const c_char, fp: *mut FILE) -> c_int {
    // UNSAFE(slice) SAFETY: as fputs.
    ret::count(stdio::fputs(unsafe { strz::strz(s) }, conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn puts(s: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: s is a string (§12.5).
    ret::count(stdio::puts(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getline(line: Option<&mut *mut c_char>, n: Option<&mut size_t>, fp: *mut FILE) -> ssize_t {
    ret::record(stdio::getdelim(conv::var(line), conv::var(n), c_int::from(b'\n'), conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getdelim(line: Option<&mut *mut c_char>, n: Option<&mut size_t>, delim: c_int, fp: *mut FILE) -> ssize_t {
    ret::record(stdio::getdelim(conv::var(line), conv::var(n), delim, conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __getdelim(line: Option<&mut *mut c_char>, n: Option<&mut size_t>, delim: c_int, fp: *mut FILE) -> ssize_t {
    ret::record(stdio::getdelim(conv::var(line), conv::var(n), delim, conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gets(s: *mut c_char) -> *mut c_char {
    ret::buf_or_null(s, stdio::gets(conv::addr(s), None))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __gets_chk(s: *mut c_char, size: size_t) -> *mut c_char {
    ret::buf_or_null(s, stdio::gets(conv::addr(s), Some(size)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getw(fp: *mut FILE) -> c_int {
    ret::int_or_eof(stdio::getw(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putw(w: c_int, fp: *mut FILE) -> c_int {
    ret::zero(stdio::putw(w, conv::addr(fp)))
}}

// ------------------------------------------------------------------------------ §12.5a

export! { "GLIBC_2.2.5";
pub extern "C" fn fread(p: *mut c_void, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    // UNSAFE(slice) SAFETY: p has room for n elements of size bytes (§12.5a).
    ret::elements(stdio::fread(unsafe { conv::elements_mut(p, n, size) }, conv::addr(fp), Locking::Locked), size, n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fread_unlocked(p: *mut c_void, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    // UNSAFE(slice) SAFETY: as fread.
    ret::elements(stdio::fread(unsafe { conv::elements_mut(p, n, size) }, conv::addr(fp), Locking::Unlocked), size, n)
}}

export! { "GLIBC_2.7";
pub extern "C" fn __fread_chk(p: *mut c_void, plen: size_t, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    // UNSAFE(slice) SAFETY: p has room for plen bytes; n elements must fit (§12.5a, fortified).
    ret::elements(stdio::fread(unsafe { conv::elements_mut(p, conv::chk_elements(n, size, plen), size) }, conv::addr(fp), Locking::Locked), size, n)
}}

export! { "GLIBC_2.7";
pub extern "C" fn __fread_unlocked_chk(p: *mut c_void, plen: size_t, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    // UNSAFE(slice) SAFETY: as __fread_chk.
    ret::elements(stdio::fread(unsafe { conv::elements_mut(p, conv::chk_elements(n, size, plen), size) }, conv::addr(fp), Locking::Unlocked), size, n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fwrite(p: *const c_void, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    // UNSAFE(slice) SAFETY: p holds n elements of size bytes (§12.5a).
    stdio::fwrite_elements(unsafe { conv::elements(p, n, size) }, size, n, conv::addr(fp), Locking::Locked)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fwrite_unlocked(p: *const c_void, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    // UNSAFE(slice) SAFETY: as fwrite.
    stdio::fwrite_elements(unsafe { conv::elements(p, n, size) }, size, n, conv::addr(fp), Locking::Unlocked)
}}

// ------------------------------------------------------------------------------ §12.8

export! { "GLIBC_2.2.5";
pub extern "C" fn fseek(fp: *mut FILE, offset: i64, whence: c_int) -> c_int {
    ret::zero(stdio::fseek(conv::addr(fp), offset, whence))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fseeko(fp: *mut FILE, offset: off_t, whence: c_int) -> c_int {
    ret::zero(stdio::fseek(conv::addr(fp), offset, whence))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fseeko64(fp: *mut FILE, offset: off_t, whence: c_int) -> c_int {
    ret::zero(stdio::fseek(conv::addr(fp), offset, whence))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftell(fp: *mut FILE) -> i64 {
    ret::off_signed(stdio::ftell(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftello(fp: *mut FILE) -> off_t {
    ret::off_signed(stdio::ftell(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftello64(fp: *mut FILE) -> off_t {
    ret::off_signed(stdio::ftell(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rewind(fp: *mut FILE) {
    stdio::rewind(conv::addr(fp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetpos(fp: *mut FILE, pos: Option<&mut fpos_t>) -> c_int {
    ret::store(pos, stdio::fgetpos(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetpos64(fp: *mut FILE, pos: Option<&mut fpos_t>) -> c_int {
    ret::store(pos, stdio::fgetpos(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fsetpos(fp: *mut FILE, pos: Option<&fpos_t>) -> c_int {
    ret::zero(stdio::fsetpos(conv::addr(fp), pos))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fsetpos64(fp: *mut FILE, pos: Option<&fpos_t>) -> c_int {
    ret::zero(stdio::fsetpos(conv::addr(fp), pos))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn feof(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::indicator(conv::addr(fp), EOF_SEEN)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn feof_unlocked(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::indicator(conv::addr(fp), EOF_SEEN)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ferror(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::indicator(conv::addr(fp), ERR_SEEN)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ferror_unlocked(fp: *mut FILE) -> c_int {
    ret::truth(ret::or_zero(stdio::indicator(conv::addr(fp), ERR_SEEN)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn clearerr(fp: *mut FILE) {
    stdio::clearerr(conv::addr(fp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn clearerr_unlocked(fp: *mut FILE) {
    stdio::clearerr(conv::addr(fp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fwide(fp: *mut FILE, mode: c_int) -> c_int {
    ret::or_zero(stdio::fwide(conv::addr(fp), mode))
}}

// ------------------------------------------------------------------------------ §12.9

export! { "GLIBC_2.2.5";
pub extern "C" fn flockfile(fp: *mut FILE) {
    stdio::flockfile(conv::addr(fp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftrylockfile(fp: *mut FILE) -> c_int {
    ret::truth(!stdio::ftrylockfile(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn funlockfile(fp: *mut FILE) {
    stdio::funlockfile(conv::addr(fp))
}}

// ------------------------------------------------------------------------------ §12.3

export! { "GLIBC_2.22";
pub extern "C" fn fmemopen(buf: *mut c_void, size: size_t, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: mode is a string; buf is NULL or size bytes the stream may use until it is closed (§12.3).
    ret::null(stdio::fmemopen(conv::addr(buf), size, unsafe { strz::strz(mode) }))
}}

crate::internal_entry! {
/// `fmemopen@GLIBC_2.2.5`: the old buffer contract (size 0 is `EINVAL`; reads run to the size;
/// `a` places only the first position; probe-settled, LG-2). Exported only under that compat version.
pub extern "C" fn liella_compat_fmemopen(buf: *mut c_void, size: size_t, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: mode is a string; buf is NULL or size bytes the stream may use until it is closed (§12.3).
    ret::null(stdio::fmemopen_2_2_5(conv::addr(buf), size, unsafe { strz::strz(mode) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn open_memstream(ptr: Option<&mut *mut c_char>, size: Option<&mut size_t>) -> *mut FILE {
    ret::null(stdio::open_memstream(conv::var(ptr), conv::var(size), false))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fopencookie(cookie: *mut c_void, mode: *const c_char, functions: cookie_io_functions_t) -> *mut FILE {
    // UNSAFE(slice) SAFETY: mode is a string (§12.3).
    ret::null(stdio::fopencookie(conv::addr(cookie), unsafe { strz::strz(mode) }, functions))
}}

// ------------------------------------------------------------------------------ §12.12

export! { "GLIBC_2.2.5";
pub extern "C" fn tmpnam(s: *mut c_char) -> *mut c_char {
    ret::null(stdio::tmpnam(conv::addr(s), false))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tmpnam_r(s: *mut c_char) -> *mut c_char {
    ret::null(stdio::tmpnam(conv::addr(s), true))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tempnam(dir: *const c_char, pfx: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: dir and pfx are NULL or strings (§12.12).
    ret::null(stdio::tempnam(unsafe { strz::strz_opt(dir) }, unsafe { strz::strz_opt(pfx) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ctermid(s: *mut c_char) -> *mut c_char {
    ret::ptr(stdio::ctermid(conv::addr(s)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cuserid(s: *mut c_char) -> *mut c_char {
    ret::null(crate::users::cuserid(conv::addr(s)))
}}

// ------------------------------------------------------------------------------ §12.10

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetwc(fp: *mut FILE) -> wint_t {
    ret::wint_or_weof(stdio::getwc(conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getwc(fp: *mut FILE) -> wint_t {
    ret::wint_or_weof(stdio::getwc(conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetwc_unlocked(fp: *mut FILE) -> wint_t {
    ret::wint_or_weof(stdio::getwc(conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getwc_unlocked(fp: *mut FILE) -> wint_t {
    ret::wint_or_weof(stdio::getwc(conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getwchar() -> wint_t {
    ret::wint_or_weof(stdio::getwc(stdio::stdin(), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getwchar_unlocked() -> wint_t {
    ret::wint_or_weof(stdio::getwc(stdio::stdin(), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wuflow(fp: *mut FILE) -> wint_t {
    ret::wint_or_weof(stdio::getwc(conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wunderflow(fp: *mut FILE) -> wint_t {
    ret::wint_or_weof(stdio::peekwc(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputwc(wc: wchar_t, fp: *mut FILE) -> wint_t {
    ret::wint_written(stdio::putwc(conv::addr(fp), Locking::Locked, u32::from_ne_bytes(wc.to_ne_bytes())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putwc(wc: wchar_t, fp: *mut FILE) -> wint_t {
    ret::wint_written(stdio::putwc(conv::addr(fp), Locking::Locked, u32::from_ne_bytes(wc.to_ne_bytes())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputwc_unlocked(wc: wchar_t, fp: *mut FILE) -> wint_t {
    ret::wint_written(stdio::putwc(conv::addr(fp), Locking::Unlocked, u32::from_ne_bytes(wc.to_ne_bytes())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putwc_unlocked(wc: wchar_t, fp: *mut FILE) -> wint_t {
    ret::wint_written(stdio::putwc(conv::addr(fp), Locking::Unlocked, u32::from_ne_bytes(wc.to_ne_bytes())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putwchar(wc: wchar_t) -> wint_t {
    ret::wint_written(stdio::putwc(stdio::stdout(), Locking::Locked, u32::from_ne_bytes(wc.to_ne_bytes())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putwchar_unlocked(wc: wchar_t) -> wint_t {
    ret::wint_written(stdio::putwc(stdio::stdout(), Locking::Unlocked, u32::from_ne_bytes(wc.to_ne_bytes())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __woverflow(fp: *mut FILE, wc: wint_t) -> wint_t {
    ret::wint_written(stdio::woverflow(conv::addr(fp), wc))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ungetwc(wc: wint_t, fp: *mut FILE) -> wint_t {
    ret::wint_written(stdio::ungetwc(conv::addr(fp), Locking::Locked, wc))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_sputbackwc(fp: *mut FILE, wc: wint_t) -> wint_t {
    ret::wint_written(stdio::ungetwc(conv::addr(fp), Locking::Unlocked, wc))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_sungetc(fp: *mut FILE) -> c_int {
    ret::byte_written(stdio::sungetc(conv::addr(fp)))
}}

/// The libio internals whose parameter lists no public document gives (§12.13, F32; ruling
/// #1064 under R15-7, IO-VTABLES): fixed-failure stubs. Each one accepts and ignores the six
/// integer argument registers, never reads or writes memory through them (so an unknown
/// signature cannot corrupt anything), leaves `errno` alone (no public document names an error
/// for them), and returns its fixed result in the whole of `rax`: `-1` reads as `EOF`, as
/// `WEOF` in the low 32 bits and as the `-1` of an `off64_t`/`ssize_t`; `0` reads as `NULL`, a
/// zero count, or nothing (a `void` function's caller ignores it). Differences row "libio
/// internals without a public signature".
macro_rules! libio_stub {
    ($ret:expr => $($name:ident),+ $(,)?) => {$(
        export! { "GLIBC_2.2.5";
        #[doc = concat!("`", stringify!($name), "`: fixed-failure stub, returns ", stringify!($ret), " and touches nothing (ruling #1064).")]
        pub extern "C" fn $name(_a0: usize, _a1: usize, _a2: usize, _a3: usize, _a4: usize, _a5: usize) -> ssize_t {
            $ret
        }}
    )+};
}

/// The fixed result of a stub whose role is a status, a byte or character, a position or a
/// transfer that fails: `EOF` / `WEOF` / `-1`.
pub const LIBIO_STUB_FAIL: ssize_t = -1;
/// The fixed result of a stub whose role is a pointer (`NULL`), a count or column (`0`), or
/// nothing (`void`).
pub const LIBIO_STUB_ZERO: ssize_t = 0;

// Overflow, underflow, uflow and pushback (`EOF`/`WEOF`), sync, write-out, close, seek, read,
// write, stat, buffer allocation and get-mode switches (`EOF`/`-1`).
libio_stub!(LIBIO_STUB_FAIL =>
    _IO_default_pbackfail, _IO_default_uflow, _IO_file_overflow, _IO_file_underflow,
    _IO_str_overflow, _IO_str_pbackfail, _IO_str_underflow, _IO_wdefault_pbackfail,
    _IO_wdefault_uflow, _IO_wfile_overflow, _IO_wfile_underflow, _IO_file_sync, _IO_wfile_sync,
    _IO_do_write, _IO_wdo_write, _IO_file_close, _IO_file_close_it, _IO_proc_close, _IO_file_seek,
    _IO_file_seekoff, _IO_str_seekoff, _IO_wfile_seekoff, _IO_seekmark, _IO_seekwmark,
    _IO_file_read, _IO_file_write, _IO_file_stat, _IO_default_doallocate, _IO_file_doallocate,
    _IO_wdefault_doallocate, _IO_switch_to_get_mode, _IO_switch_to_wget_mode,
);
// Stream openers (`NULL`), transfers (`0` bytes or characters), marker distances and columns
// (`0`), and the set-up, tear-down, list, marker and area operations with no result.
libio_stub!(LIBIO_STUB_ZERO =>
    _IO_file_attach, _IO_file_fopen, _IO_file_open, _IO_file_setbuf, _IO_proc_open,
    _IO_default_xsgetn, _IO_default_xsputn, _IO_file_xsputn, _IO_wdefault_xsgetn,
    _IO_wdefault_xsputn, _IO_wfile_xsputn, _IO_marker_delta, _IO_marker_difference,
    _IO_wmarker_delta, _IO_least_wmarker, _IO_adjust_column, _IO_adjust_wcolumn,
    _IO_default_finish, _IO_doallocbuf, _IO_file_finish, _IO_file_init, _IO_free_backup_area,
    _IO_free_wbackup_area, _IO_init, _IO_init_marker, _IO_init_wmarker, _IO_link_in,
    _IO_remove_marker, _IO_setb, _IO_str_init_readonly, _IO_str_init_static, _IO_un_link,
    _IO_unsave_markers, _IO_unsave_wmarkers, _IO_wdefault_finish, _IO_wdoallocbuf, _IO_wsetb,
    _IO_switch_to_main_wget_area, _IO_switch_to_wbackup_area,
);

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_sungetwc(fp: *mut FILE) -> wint_t {
    ret::wint_written(stdio::sungetwc(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetws(ws: *mut wchar_t, n: c_int, fp: *mut FILE) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: ws has room for n wide characters (§12.10).
    ret::buf_or_null(ws, stdio::fgetws(unsafe { conv::buf_w_mut(ws, conv::nonneg(n)) }, conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetws_unlocked(ws: *mut wchar_t, n: c_int, fp: *mut FILE) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: ws has room for n wide characters (§12.10).
    ret::buf_or_null(ws, stdio::fgetws(unsafe { conv::buf_w_mut(ws, conv::nonneg(n)) }, conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __fgetws_chk(ws: *mut wchar_t, size: size_t, n: c_int, fp: *mut FILE) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: ws has room for size wide characters; the slice is at most that (§12.10, C0.5).
    ret::buf_or_null(ws, stdio::fgetws_chk(unsafe { conv::buf_w_mut(ws, conv::nonneg(n).min(size)) }, conv::nonneg(n), conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __fgetws_unlocked_chk(ws: *mut wchar_t, size: size_t, n: c_int, fp: *mut FILE) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: ws has room for size wide characters; the slice is at most that (§12.10, C0.5).
    ret::buf_or_null(ws, stdio::fgetws_chk(unsafe { conv::buf_w_mut(ws, conv::nonneg(n).min(size)) }, conv::nonneg(n), conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputws(ws: *const wchar_t, fp: *mut FILE) -> c_int {
    // UNSAFE(slice) SAFETY: ws is a wide string (§12.10).
    ret::count(stdio::fputws(unsafe { strz::wcsz(ws) }, conv::addr(fp), Locking::Locked))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fputws_unlocked(ws: *const wchar_t, fp: *mut FILE) -> c_int {
    // UNSAFE(slice) SAFETY: ws is a wide string (§12.10).
    ret::count(stdio::fputws(unsafe { strz::wcsz(ws) }, conv::addr(fp), Locking::Unlocked))
}}

export! { "GLIBC_2.4";
pub extern "C" fn open_wmemstream(ptr: Option<&mut *mut wchar_t>, size: Option<&mut size_t>) -> *mut FILE {
    ret::null(stdio::open_memstream(conv::var(ptr), conv::var(size), true))
}}

// ------------------------------------------------------------------------------ §12.13
// The main libio entry points, each the contract of its public function (spec §12.13). The
// vtable objects and the libio internals are phase 15 (F32).

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_getc(fp: *mut FILE) -> c_int {
    getc(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_putc(c: c_int, fp: *mut FILE) -> c_int {
    putc(c, fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_feof(fp: *mut FILE) -> c_int {
    feof(fp)
}}

export! { "GLIBC_2.2.5";
// The mixed-case name is the glibc ABI symbol, not a style choice.
#[allow(non_snake_case)]
pub extern "C" fn _IO_ferror(fp: *mut FILE) -> c_int {
    ferror(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_flockfile(fp: *mut FILE) {
    flockfile(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_funlockfile(fp: *mut FILE) {
    funlockfile(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_ftrylockfile(fp: *mut FILE) -> c_int {
    ftrylockfile(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fclose(fp: *mut FILE) -> c_int {
    fclose(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fdopen(fd: c_int, mode: *const c_char) -> *mut FILE {
    fdopen(fd, mode)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fflush(fp: *mut FILE) -> c_int {
    fflush(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fgetpos(fp: *mut FILE, pos: Option<&mut fpos_t>) -> c_int {
    fgetpos(fp, pos)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fgetpos64(fp: *mut FILE, pos: Option<&mut fpos_t>) -> c_int {
    fgetpos64(fp, pos)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fsetpos(fp: *mut FILE, pos: Option<&fpos_t>) -> c_int {
    fsetpos(fp, pos)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fsetpos64(fp: *mut FILE, pos: Option<&fpos_t>) -> c_int {
    fsetpos64(fp, pos)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fgets(s: *mut c_char, n: c_int, fp: *mut FILE) -> *mut c_char {
    fgets(s, n, fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fopen(path: *const c_char, mode: *const c_char) -> *mut FILE {
    fopen(path, mode)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fputs(s: *const c_char, fp: *mut FILE) -> c_int {
    fputs(s, fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fread(p: *mut c_void, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    fread(p, size, n, fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_fwrite(p: *const c_void, size: size_t, n: size_t, fp: *mut FILE) -> size_t {
    fwrite(p, size, n, fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_ftell(fp: *mut FILE) -> i64 {
    ftell(fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_gets(s: *mut c_char) -> *mut c_char {
    gets(s)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_puts(s: *const c_char) -> c_int {
    puts(s)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_setbuffer(fp: *mut FILE, buf: *mut c_char, size: size_t) {
    setbuffer(fp, buf, size)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_setvbuf(fp: *mut FILE, buf: *mut c_char, mode: c_int, size: size_t) -> c_int {
    setvbuf(fp, buf, mode, size)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_ungetc(c: c_int, fp: *mut FILE) -> c_int {
    ungetc(c, fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_sputbackc(fp: *mut FILE, c: c_int) -> c_int {
    ungetc(c, fp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_peekc_locked(fp: *mut FILE) -> c_int {
    ret::byte_or_eof(stdio::peek_locked(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_padn(fp: *mut FILE, pad: c_int, count: ssize_t) -> ssize_t {
    ssize_t::try_from(stdio::padn(conv::addr(fp), pad, count)).unwrap_or(ssize_t::MAX)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_sgetn(fp: *mut FILE, data: *mut c_void, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: data has room for n bytes (§12.13).
    stdio::fread(unsafe { conv::buf_mut(data, n) }, conv::addr(fp), Locking::Unlocked).done
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_seekoff(fp: *mut FILE, offset: off_t, dir: c_int, _mode: c_int) -> off_t {
    stdio::seekoff(conv::addr(fp), offset, dir).unwrap_or(-1)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_seekpos(fp: *mut FILE, pos: off_t, _mode: c_int) -> off_t {
    stdio::seekoff(conv::addr(fp), pos, 0).unwrap_or(-1)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_getline(fp: *mut FILE, buf: *mut c_char, n: size_t, delim: c_int, extract: c_int) -> size_t {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (§12.13).
    stdio::getline_info(conv::addr(fp), unsafe { conv::buf_mut(buf.cast::<c_void>(), n) }, delim, extract, 0)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_getline_info(fp: *mut FILE, buf: *mut c_char, n: size_t, delim: c_int, extract: c_int, eof: *mut c_int) -> size_t {
    // UNSAFE(slice) SAFETY: buf has room for n bytes; eof is NULL or an int (§12.13).
    stdio::getline_info(conv::addr(fp), unsafe { conv::buf_mut(buf.cast::<c_void>(), n) }, delim, extract, conv::addr(eof))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_flush_all() -> c_int {
    fflush(core::ptr::null_mut())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_flush_all_linebuffered() {
    stdio::flushlbf()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_list_lock() {
    stdio::list_lock(true)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_list_unlock() {
    stdio::list_lock(false)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_list_resetlock() {
    stdio::list_resetlock()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_iter_begin() -> *mut FILE {
    ret::ptr(stdio::iter_begin())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_iter_end() -> *mut FILE {
    core::ptr::null_mut()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_iter_next(fp: *mut FILE) -> *mut FILE {
    ret::ptr(stdio::iter_next(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_iter_file(fp: *mut FILE) -> *mut FILE {
    fp
}}

// ------------------------------------------------------------------------------ §12.7

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vfscanf4(fp: *mut FILE, flavour: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(conv::addr(fp), scanf::flavour(flavour), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vsscanf4(s: *const c_char, flavour: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vsscanf(unsafe { strz::strz(s) }, scanf::flavour(flavour), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vfwscanf4(fp: *mut FILE, flavour: c_int, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfwscanf(conv::addr(fp), scanf::flavour(flavour), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vswscanf4(s: *const wchar_t, flavour: c_int, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are wide strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vswscanf(unsafe { strz::wcsz(s) }, scanf::flavour(flavour), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vscanf(fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(0, scanf::flavour(0), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vfscanf(fp: *mut FILE, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(conv::addr(fp), scanf::flavour(0), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __vfscanf(fp: *mut FILE, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(conv::addr(fp), scanf::flavour(0), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vsscanf(s: *const c_char, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vsscanf(unsafe { strz::strz(s) }, scanf::flavour(0), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __vsscanf(s: *const c_char, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vsscanf(unsafe { strz::strz(s) }, scanf::flavour(0), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vwscanf(fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfwscanf(0, scanf::flavour(0), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vfwscanf(fp: *mut FILE, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfwscanf(conv::addr(fp), scanf::flavour(0), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vswscanf(s: *const wchar_t, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are wide strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vswscanf(unsafe { strz::wcsz(s) }, scanf::flavour(0), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __isoc99_vscanf(fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(0, scanf::flavour(1), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __isoc99_vfscanf(fp: *mut FILE, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(conv::addr(fp), scanf::flavour(1), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __isoc99_vsscanf(s: *const c_char, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vsscanf(unsafe { strz::strz(s) }, scanf::flavour(1), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __isoc99_vwscanf(fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfwscanf(0, scanf::flavour(1), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __isoc99_vfwscanf(fp: *mut FILE, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfwscanf(conv::addr(fp), scanf::flavour(1), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __isoc99_vswscanf(s: *const wchar_t, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are wide strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vswscanf(unsafe { strz::wcsz(s) }, scanf::flavour(1), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_vscanf(fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(0, scanf::flavour(2), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_vfscanf(fp: *mut FILE, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfscanf(conv::addr(fp), scanf::flavour(2), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_vsscanf(s: *const c_char, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vsscanf(unsafe { strz::strz(s) }, scanf::flavour(2), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_vwscanf(fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfwscanf(0, scanf::flavour(2), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_vfwscanf(fp: *mut FILE, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the pointers it names (§12.7).
    ret::int_or_eof(scanf::vfwscanf(conv::addr(fp), scanf::flavour(2), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_vswscanf(s: *const wchar_t, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: s and fmt are wide strings; ap is the va_list of the pointers fmt names (§12.7).
    ret::int_or_eof(scanf::vswscanf(unsafe { strz::wcsz(s) }, scanf::flavour(2), unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_vfscanf(fp: *mut FILE, fmt: *const c_char, ap: *mut c_void, errp: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the pointers it names (§12.7).
    let r = scanf::vfscanf(conv::addr(fp), scanf::flavour(0), unsafe { strz::strz(fmt) }, conv::addr(ap));
    // `*errp` reports a failure, as the reference library's internal entry does.
    if let Some(e) = errp {
        *e = c_int::from(!matches!(r, Ok(Some(_))));
    }
    ret::int_or_eof(r)
}}

// ------------------------------------------------------------------------------ §12.6

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vfprintf4(fp: *mut FILE, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfprintf(conv::addr(fp), flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vdprintf4(fd: c_int, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vdprintf(fd, flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vsnprintf6(s: *mut c_char, maxlen: size_t, flag: c_int, slen: size_t, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6). s has room for min(maxlen, slen) bytes, or the result when unbounded.
    ret::count(printf::vsnprintf(conv::addr(s), maxlen, flag, slen, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vasprintf4(strp: Option<&mut *mut c_char>, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vasprintf(conv::var(strp), flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_obstack_vprintf4(obs: Option<&mut struct_obstack>, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::obstack_vprintf(obs, flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vfwprintf4(fp: *mut FILE, flag: c_int, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfwprintf(conv::addr(fp), flag, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vswprintf6(s: *mut wchar_t, maxlen: size_t, flag: c_int, slen: size_t, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6). s has room for maxlen units.
    ret::count(printf::vswprintf(conv::addr(s), maxlen, flag, slen, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vprintf(fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfprintf(0, 0, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vfprintf(fp: *mut FILE, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfprintf(conv::addr(fp), 0, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_vfprintf(fp: *mut FILE, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfprintf(conv::addr(fp), 0, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __vprintf_chk(flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfprintf(0, flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __vfprintf_chk(fp: *mut FILE, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfprintf(conv::addr(fp), flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vdprintf(fd: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vdprintf(fd, 0, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.8";
pub extern "C" fn __vdprintf_chk(fd: c_int, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vdprintf(fd, flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vsprintf(s: *mut c_char, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6). s has room for the result (unbounded, HIST).
    ret::count(printf::vsnprintf(conv::addr(s), usize::MAX, 0, usize::MAX, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _IO_vsprintf(s: *mut c_char, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6). s has room for the result (unbounded, HIST).
    ret::count(printf::vsnprintf(conv::addr(s), usize::MAX, 0, usize::MAX, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __vsprintf_chk(s: *mut c_char, flag: c_int, slen: size_t, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6). s has room for slen bytes.
    ret::count(printf::vsnprintf(conv::addr(s), usize::MAX, flag, slen, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vsnprintf(s: *mut c_char, maxlen: size_t, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6). s has room for maxlen bytes.
    ret::count(printf::vsnprintf(conv::addr(s), maxlen, 0, usize::MAX, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __vsnprintf(s: *mut c_char, maxlen: size_t, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6). s has room for maxlen bytes.
    ret::count(printf::vsnprintf(conv::addr(s), maxlen, 0, usize::MAX, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __vsnprintf_chk(s: *mut c_char, maxlen: size_t, flag: c_int, slen: size_t, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6). s has room for slen bytes.
    ret::count(printf::vsnprintf(conv::addr(s), maxlen, flag, slen, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vasprintf(strp: Option<&mut *mut c_char>, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vasprintf(conv::var(strp), 0, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.8";
pub extern "C" fn __vasprintf_chk(strp: Option<&mut *mut c_char>, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vasprintf(conv::var(strp), flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn obstack_vprintf(obs: Option<&mut struct_obstack>, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::obstack_vprintf(obs, 0, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.8";
pub extern "C" fn __obstack_vprintf_chk(obs: Option<&mut struct_obstack>, flag: c_int, fmt: *const c_char, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::obstack_vprintf(obs, flag, unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vwprintf(fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfwprintf(0, 0, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __vwprintf_chk(flag: c_int, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfwprintf(0, flag, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vfwprintf(fp: *mut FILE, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfwprintf(conv::addr(fp), 0, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __vfwprintf_chk(fp: *mut FILE, flag: c_int, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6).
    ret::count(printf::vfwprintf(conv::addr(fp), flag, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vswprintf(s: *mut wchar_t, maxlen: size_t, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6). s has room for maxlen units.
    ret::count(printf::vswprintf(conv::addr(s), maxlen, 0, usize::MAX, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __vswprintf_chk(s: *mut wchar_t, maxlen: size_t, flag: c_int, slen: size_t, fmt: *const wchar_t, ap: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: fmt is a wide string; ap is the va_list of the arguments it names (§12.6). s has room for slen units.
    ret::count(printf::vswprintf(conv::addr(s), maxlen, flag, slen, unsafe { strz::wcsz(fmt) }, conv::addr(ap)))
}}

// ------------------------------------------------------------------------------ §12.11

export! { "GLIBC_2.10";
pub extern "C" fn register_printf_specifier(spec: c_int, handler: usize, arginfo: usize) -> c_int {
    ret::zero(printf_registry::register_specifier(spec, handler, arginfo))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn register_printf_function(spec: c_int, handler: usize, arginfo: usize) -> c_int {
    ret::zero(printf_registry::register_specifier(spec, handler, arginfo))
}}

export! { "GLIBC_2.10";
pub extern "C" fn register_printf_modifier(text: *const wchar_t) -> c_int {
    // UNSAFE(slice) SAFETY: text is a wide string (§12.11).
    ret::int(unsafe { strz::wcsz(text) }.and_then(printf_registry::register_modifier).map(i64::from))
}}

export! { "GLIBC_2.10";
pub extern "C" fn register_printf_type(f: usize) -> c_int {
    ret::int(printf_registry::register_type(f).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn parse_printf_format(fmt: *const c_char, n: size_t, argtypes: *mut c_int) -> size_t {
    // UNSAFE(slice) SAFETY: fmt is a string; argtypes has room for n ints (§12.11).
    ret::size0(printf::parse_format(unsafe { strz::strz(fmt) }, unsafe { conv::array_mut::<u32>(argtypes.cast(), conv::count_sz(n)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn printf_size(fp: *mut FILE, info: Option<&struct_printf_info>, args: *const *const c_void) -> c_int {
    ret::count(printf::formatter(conv::addr(fp), info, conv::addr(args), printf::Formatter::Size))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn printf_size_info(info: Option<&struct_printf_info>, n: size_t, argtypes: Option<&mut c_int>) -> c_int {
    ret::int(printf::size_info(info, n, argtypes))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __printf_fp(fp: *mut FILE, info: Option<&struct_printf_info>, args: *const *const c_void) -> c_int {
    ret::count(printf::formatter(conv::addr(fp), info, conv::addr(args), printf::Formatter::Fp))
}}

// ------------------------------------------------------------------------------ §12.12

export! { "GLIBC_2.2.5";
pub extern "C" fn tmpfile() -> *mut FILE {
    ret::null(stdio::tmpfile())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tmpfile64() -> *mut FILE {
    ret::null(stdio::tmpfile())
}}
