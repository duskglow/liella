//! Translation for spec §1.1–§1.5: program startup, termination, environment, auxiliary vector.
//!
//! The process-lifecycle symbols (`__libc_start_main`, `exit`, the handler registrations) are
//! `runtime` exports: only the products define them, because only a process Liella started has
//! Liella's runtime (D-4.13). `atexit` and `at_quick_exit` belong to the static library only, as
//! glibc's `libc_nonshared.a` provides them (`nonshared`: not in the shared object's version
//! script). The data objects `environ`, `__environ`, `_environ`, `program_invocation_name`,
//! `__progname_full`, `program_invocation_short_name`, `__progname` and `__libc_stack_end` are
//! defined in `csys/src/adapter/runtime.c`, as are argp's program-wide variables (weakly, so
//! that a program's own definitions replace them; §1.7). `getopt`/`argp` (§1.6–1.7) arrived with
//! stdio (phase 8); the variadic `argp_error` and `argp_failure` are C adapters forwarding to
//! `liella_argp_error3` and `liella_argp_failure5`.

use liella_abi::ctypes::{c_char, c_int, c_uint, c_ulong, c_void};
use liella_legacy::exit_handlers::{ATEXIT, CXA, ON_EXIT, QUICK};

use crate::argp;
use crate::conv;
use crate::export;
use crate::options;
use crate::process;
use crate::ret;
use crate::strz;

#[cfg(feature = "runtime")]
use crate::runtime::{Fn0, Main};

// ------------------------------------------------------------------------------ §1.1 startup

export! { runtime "GLIBC_2.34";
#[cfg(feature = "runtime")]
pub extern "C" fn __libc_start_main(main: Option<Main>, argc: c_int, argv: *mut *mut c_char, init: Option<Fn0>, fini: Option<Fn0>, rtld_fini: Option<Fn0>, stack_end: *mut c_void) -> c_int {
    crate::runtime::start(main, argc, argv, init, fini, rtld_fini, stack_end)
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn __libc_init_first(_argc: c_int, _argv: *mut *mut c_char, _envp: *mut *mut c_char) {
    liella_legacy::stubs::nothing()
}}

// ------------------------------------------------------------------------------ §1.2 termination

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn exit(status: c_int) {
    process::exit(status)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _exit(status: c_int) {
    process::exit_now(status)
}}

export! { "GLIBC_2.2.5";
#[allow(non_snake_case)] // the C name
pub extern "C" fn _Exit(status: c_int) {
    process::exit_now(status)
}}

export! { runtime "GLIBC_2.10";
pub extern "C" fn quick_exit(status: c_int) {
    process::quick_exit(status)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn abort() {
    ret::abort()
}}

export! { runtime "nonshared";
pub extern "C" fn atexit(f: usize) -> c_int {
    ret::registered(process::register(process::entry(ATEXIT, f, 0, 0)))
}}

export! { runtime "nonshared";
pub extern "C" fn at_quick_exit(f: usize) -> c_int {
    ret::registered(process::register_quick(process::entry(QUICK, f, 0, 0)))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn on_exit(f: usize, arg: *mut c_void) -> c_int {
    ret::registered(process::register(process::entry(ON_EXIT, f, arg.expose_provenance(), 0)))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn __cxa_atexit(f: usize, arg: *mut c_void, dso: *mut c_void) -> c_int {
    ret::registered(process::register(process::entry(CXA, f, arg.expose_provenance(), dso.expose_provenance())))
}}

export! { runtime "GLIBC_2.10";
pub extern "C" fn __cxa_at_quick_exit(f: usize, dso: *mut c_void) -> c_int {
    ret::registered(process::register_quick(process::entry(QUICK, f, 0, dso.expose_provenance())))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn __cxa_finalize(dso: *mut c_void) {
    process::finalize(dso.expose_provenance())
}}

export! { runtime "GLIBC_2.18";
pub extern "C" fn __cxa_thread_atexit_impl(f: usize, obj: *mut c_void, dso: *mut c_void) -> c_int {
    ret::registered(process::thread_atexit(f, obj.expose_provenance(), dso.expose_provenance()))
}}

// ------------------------------------------------------------------------------ §1.3 environment

export! { "GLIBC_2.2.5";
pub extern "C" fn getenv(name: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: name is NULL or a NUL-terminated string (§1.3).
    ret::ptr(unsafe { strz::getenv_c(name) })
}}

export! { "GLIBC_2.17";
pub extern "C" fn secure_getenv(name: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as getenv.
    ret::ptr(unsafe { strz::secure_getenv_c(name) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __secure_getenv(name: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as getenv.
    ret::ptr(unsafe { strz::secure_getenv_c(name) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setenv(name: *const c_char, value: *const c_char, overwrite: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: name and value are NUL-terminated strings (§1.3).
    ret::zero(unsafe { strz::strz(name) }.and_then(|n| unsafe { strz::strz(value) }.and_then(|v| process::setenv(n, v, overwrite))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn unsetenv(name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§1.3).
    ret::zero(unsafe { strz::strz(name) }.and_then(process::unsetenv))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putenv(string: *mut c_char) -> c_int {
    ret::zero(process::putenv(string.expose_provenance()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn clearenv() -> c_int {
    ret::zero(process::clearenv())
}}

// ------------------------------------------------------------------------------ §1.5 auxv

export! { "GLIBC_2.16";
pub extern "C" fn getauxval(key: c_ulong) -> c_ulong {
    ret::with_errno(process::getauxval(key))
}}

export! { "GLIBC_2.16";
pub extern "C" fn __getauxval(key: c_ulong) -> c_ulong {
    ret::with_errno(process::getauxval(key))
}}

// ------------------------------------------------------------------------------ §1.6

export! { "GLIBC_2.2.5";
pub extern "C" fn getopt(argc: c_int, argv: *const *mut c_char, optstring: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: argv holds argc string pointers and optstring is a string; longopts is NULL or an all-zero-terminated option table (§1.6).
    options::getopt(argc, conv::addr(argv), unsafe { strz::strz(optstring) }.unwrap_or(b""), 0, 0, false, false)
}}

export! { "GLIBC_2.10";
pub extern "C" fn __posix_getopt(argc: c_int, argv: *const *mut c_char, optstring: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: argv holds argc string pointers and optstring is a string; longopts is NULL or an all-zero-terminated option table (§1.6).
    options::getopt(argc, conv::addr(argv), unsafe { strz::strz(optstring) }.unwrap_or(b""), 0, 0, false, true)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getopt_long(argc: c_int, argv: *const *mut c_char, optstring: *const c_char, longopts: *const c_void, longind: *mut c_int) -> c_int {
    // UNSAFE(slice) SAFETY: argv holds argc string pointers and optstring is a string; longopts is NULL or an all-zero-terminated option table (§1.6).
    options::getopt(argc, conv::addr(argv), unsafe { strz::strz(optstring) }.unwrap_or(b""), conv::addr(longopts), conv::addr(longind), false, false)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getopt_long_only(argc: c_int, argv: *const *mut c_char, optstring: *const c_char, longopts: *const c_void, longind: *mut c_int) -> c_int {
    // UNSAFE(slice) SAFETY: argv holds argc string pointers and optstring is a string; longopts is NULL or an all-zero-terminated option table (§1.6).
    options::getopt(argc, conv::addr(argv), unsafe { strz::strz(optstring) }.unwrap_or(b""), conv::addr(longopts), conv::addr(longind), true, false)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getsubopt(optionp: *mut *mut c_char, tokens: *const *mut c_char, valuep: *mut *mut c_char) -> c_int {
    options::getsubopt(conv::addr(optionp), conv::addr(tokens), conv::addr(valuep))
}}

// ------------------------------------------------------------------------------ §1.7

export! { "GLIBC_2.2.5";
pub extern "C" fn argp_parse(argp: *const c_void, argc: c_int, argv: *mut *mut c_char, flags: c_uint, arg_index: *mut c_int, input: *mut c_void) -> c_int {
    argp::parse(conv::addr(argp), argc, conv::addr(argv), flags, conv::addr(arg_index), conv::addr(input))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argp_help(argp: *const c_void, stream: *mut c_void, flags: c_uint, name: *mut c_char) {
    argp::help(conv::addr(argp), conv::addr(stream), flags, conv::addr(name))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argp_state_help(state: *const c_void, stream: *mut c_void, flags: c_uint) {
    argp::state_help(conv::addr(state), conv::addr(stream), flags)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argp_usage(state: *const c_void) {
    argp::usage(conv::addr(state))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_argp_error3(state: *const c_void, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a string; ap is the va_list of the arguments it names (§1.7).
    argp::error(conv::addr(state), unsafe { strz::strz_opt(fmt) }, conv::addr(ap))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_argp_failure5(state: *const c_void, status: c_int, errnum: c_int, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a string; ap is the va_list of the arguments it names (§1.7).
    argp::failure(conv::addr(state), status, errnum, unsafe { strz::strz_opt(fmt) }, conv::addr(ap))
}}
