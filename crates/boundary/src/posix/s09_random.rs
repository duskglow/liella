//! Translation for spec §9: random numbers.
//!
//! The `arc4random` family takes every value from `getrandom` and terminates the process if
//! the kernel cannot supply entropy. `rand`/`random` and the `_r` forms use Liella's documented
//! additive-feedback generator (OD-13); the 48-bit family is bit-exact (§9.4). The global
//! states are process-wide under one lock until the phase-4 thread blocks.

use liella::random;
use liella_abi::ctypes::{c_char, c_int, c_long, c_uint, c_ushort, c_void, size_t, ssize_t};
use liella_abi::layout::{struct_drand48_data, struct_random_data};
use liella_kernel::sys::Sys;
use liella_legacy::prng_global;
use liella_legacy::prng_r;

use crate::cancel;
use crate::conv;
use crate::export;
use crate::ret;
use crate::sys::{CSys, CP};

export! { "GLIBC_2.25";
pub extern "C" fn getrandom(buf: *mut c_void, n: size_t, flags: c_uint) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf is writable for n bytes (§9.1).
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| CP.getrandom(b, flags))))
}}

export! { "GLIBC_2.25";
pub extern "C" fn getentropy(buf: *mut c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf is writable for n bytes (§9.1).
    ret::zero(unsafe { conv::buf_mut(buf, n) }.and_then(|b| random::entropy(&CSys, b)))
}}

export! { "GLIBC_2.36";
pub extern "C" fn arc4random() -> u32 {
    ret::arc4(random::u32_random(&CSys))
}}

export! { "GLIBC_2.36";
pub extern "C" fn arc4random_buf(buf: *mut c_void, n: size_t) {
    // UNSAFE(slice) SAFETY: buf is writable for n bytes (§9.1).
    ret::unit(unsafe { conv::buf_mut(buf, n) }.map(|b| ret::arc4(random::fill(&CSys, b).ok())))
}}

export! { "GLIBC_2.36";
pub extern "C" fn arc4random_uniform(bound: u32) -> u32 {
    ret::arc4(random::uniform(&CSys, bound))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rand() -> c_int {
    // UNSAFE(slice) SAFETY: state buffers handed to initstate/setstate stay valid while current (§9.3).
    prng_global::rand(unsafe { conv::ctext() })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn srand(seed: c_uint) {
    // UNSAFE(slice) SAFETY: state buffers handed to initstate/setstate stay valid while current (§9.3).
    prng_global::srandom(seed, unsafe { conv::ctext() })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rand_r(seed: Option<&mut c_uint>) -> c_int {
    ret::must(conv::req(seed).map(prng_global::rand_r))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn random() -> c_long {
    // UNSAFE(slice) SAFETY: state buffers handed to initstate/setstate stay valid while current (§9.3).
    prng_global::random(unsafe { conv::ctext() })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn srandom(seed: c_uint) {
    // UNSAFE(slice) SAFETY: state buffers handed to initstate/setstate stay valid while current (§9.3).
    prng_global::srandom(seed, unsafe { conv::ctext() })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn initstate(seed: c_uint, buf: *mut c_char, n: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf is writable for n bytes and stays valid while current (§9.3).
    ret::null(prng_global::initstate(seed, conv::addr(buf), n, unsafe { conv::ctext() }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setstate(buf: *mut c_char) -> *mut c_char {
    ret::ptr(prng_global::setstate(conv::addr(buf)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn random_r(buf: Option<&mut struct_random_data>, result: Option<&mut i32>) -> c_int {
    // UNSAFE(slice) SAFETY: buf is a valid random_data; its state buffer is valid (§9.3).
    ret::zero(conv::req(buf).and_then(|d| conv::req(result).and_then(|r| prng_r::random_r(d, r, unsafe { conv::ctext() }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn srandom_r(seed: c_uint, buf: Option<&mut struct_random_data>) -> c_int {
    // UNSAFE(slice) SAFETY: buf is a valid random_data; its state buffer is valid (§9.3).
    ret::zero(conv::req(buf).and_then(|d| prng_r::srandom_r(seed, d, unsafe { conv::ctext() })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn initstate_r(seed: c_uint, state: *mut c_char, n: size_t, buf: Option<&mut struct_random_data>) -> c_int {
    // UNSAFE(slice) SAFETY: state is writable for n bytes; buf is a valid random_data (§9.3).
    ret::zero(conv::req(buf).and_then(|d| prng_r::initstate_r(seed, conv::addr(state), n, d, unsafe { conv::ctext() })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setstate_r(state: *mut c_char, buf: Option<&mut struct_random_data>) -> c_int {
    // UNSAFE(slice) SAFETY: state is an initialised state buffer; buf is a valid random_data (§9.3).
    ret::zero(conv::req(buf).and_then(|d| prng_r::setstate_r(conv::addr(state), d, unsafe { conv::ctext() })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn drand48() -> f64 {
    prng_global::drand48()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lrand48() -> c_long {
    prng_global::lrand48()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mrand48() -> c_long {
    prng_global::mrand48()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn erand48(xsubi: Option<&mut [c_ushort; 3]>) -> f64 {
    prng_global::erand48(ret::must(conv::req(xsubi)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn nrand48(xsubi: Option<&mut [c_ushort; 3]>) -> c_long {
    prng_global::nrand48(ret::must(conv::req(xsubi)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn jrand48(xsubi: Option<&mut [c_ushort; 3]>) -> c_long {
    prng_global::jrand48(ret::must(conv::req(xsubi)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn srand48(seed: c_long) {
    prng_global::srand48(seed)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn seed48(seed16v: Option<&[c_ushort; 3]>) -> *mut c_ushort {
    ret::ptr(prng_global::seed48_addr(ret::must(conv::req(seed16v))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lcong48(param: Option<&[c_ushort; 7]>) {
    prng_global::lcong48_from(ret::must(conv::req(param)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn drand48_r(buffer: Option<&mut struct_drand48_data>, result: Option<&mut f64>) -> c_int {
    ret::int(conv::req(buffer).and_then(|d| conv::req(result).map(|r| prng_r::drand48_r(d, r))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lrand48_r(buffer: Option<&mut struct_drand48_data>, result: Option<&mut c_long>) -> c_int {
    ret::int(conv::req(buffer).and_then(|d| conv::req(result).map(|r| prng_r::lrand48_r(d, r))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mrand48_r(buffer: Option<&mut struct_drand48_data>, result: Option<&mut c_long>) -> c_int {
    ret::int(conv::req(buffer).and_then(|d| conv::req(result).map(|r| prng_r::mrand48_r(d, r))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn erand48_r(xsubi: Option<&mut [c_ushort; 3]>, buffer: Option<&mut struct_drand48_data>, result: Option<&mut f64>) -> c_int {
    ret::int(conv::req(xsubi).and_then(|x| conv::req(buffer).and_then(|d| conv::req(result).map(|r| prng_r::erand48_r(x, d, r)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn nrand48_r(xsubi: Option<&mut [c_ushort; 3]>, buffer: Option<&mut struct_drand48_data>, result: Option<&mut c_long>) -> c_int {
    ret::int(conv::req(xsubi).and_then(|x| conv::req(buffer).and_then(|d| conv::req(result).map(|r| prng_r::nrand48_r(x, d, r)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn jrand48_r(xsubi: Option<&mut [c_ushort; 3]>, buffer: Option<&mut struct_drand48_data>, result: Option<&mut c_long>) -> c_int {
    ret::int(conv::req(xsubi).and_then(|x| conv::req(buffer).and_then(|d| conv::req(result).map(|r| prng_r::jrand48_r(x, d, r)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn srand48_r(seed: c_long, buffer: Option<&mut struct_drand48_data>) -> c_int {
    ret::int(conv::req(buffer).map(|d| prng_r::srand48_r(seed, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn seed48_r(seed16v: Option<&[c_ushort; 3]>, buffer: Option<&mut struct_drand48_data>) -> c_int {
    ret::int(conv::req(seed16v).and_then(|s| conv::req(buffer).map(|d| prng_r::seed48_r(s, d))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lcong48_r(param: Option<&[c_ushort; 7]>, buffer: Option<&mut struct_drand48_data>) -> c_int {
    ret::int(conv::req(param).and_then(|p| conv::req(buffer).map(|d| prng_r::lcong48_r(p, d))))
}}
