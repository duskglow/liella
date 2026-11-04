// §29.1 secure-RPC authentication, key server and DES API as failing stubs (D-29.3, owner ruling 2026-10-27).
//! Every stub in `s29_rpc_auth` is called from one table: each must return its family's failure
//! value, set `errno` to `EOPNOTSUPP`, leave caller buffers as they were (except `passwd2des`,
//! which clears its output key), and open no file descriptor (no socket, no file).
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_uint, c_void};
use core::ptr::null_mut;

use liella_boundary::errno;
use liella_boundary::posix::s29_rpc_auth as a;

const CANARY: u8 = 0xA5;

fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd").map(Iterator::count).unwrap_or(0)
}

/// Runs `f` with a canary buffer; returns (result, errno, buffer after).
fn run(f: &dyn Fn(*mut c_char) -> i64) -> (i64, i32, [u8; 64]) {
    let mut buf = [CANARY; 64];
    errno::set_raw(0);
    let r = f(buf.as_mut_ptr().cast());
    (r, errno::get(), buf)
}

#[test]
fn every_auth_stub_fails_cleanly() {
    // Trace: no spec clause (§29.1 secure-RPC stubs; D-29.3 failure table, owner ruling 2026-10-27).
    let s = c"x".as_ptr();
    let v: *mut c_void = null_mut();
    let u: *mut c_uint = null_mut();
    let p = |q: *mut c_void| i64::from(!q.is_null());
    let table: Vec<(&str, i64, Box<dyn Fn(*mut c_char) -> i64>)> = vec![
        ("ecb_crypt", 2, Box::new(|b| i64::from(a::liella_compat_ecb_crypt(b, b.wrapping_add(8), 8, 0)))),
        ("cbc_crypt", 2, Box::new(|b| i64::from(a::liella_compat_cbc_crypt(b, b.wrapping_add(8), 8, 0, b.wrapping_add(16))))),
        ("des_setparity", 0, Box::new(|b| { a::liella_compat_des_setparity(b); 0 })),
        ("xencrypt", 0, Box::new(|b| i64::from(a::liella_compat_xencrypt(b, b.wrapping_add(32))))),
        ("xdecrypt", 0, Box::new(|b| i64::from(a::liella_compat_xdecrypt(b, b.wrapping_add(32))))),
        ("key_setsecret", -1, Box::new(move |_| i64::from(a::liella_compat_key_setsecret(s)))),
        ("key_secretkey_is_set", 0, Box::new(|_| i64::from(a::liella_compat_key_secretkey_is_set()))),
        ("key_gendes", -1, Box::new(|b| i64::from(a::liella_compat_key_gendes(b.cast())))),
        ("key_encryptsession", -1, Box::new(move |b| i64::from(a::liella_compat_key_encryptsession(s, b.cast())))),
        ("key_decryptsession", -1, Box::new(move |b| i64::from(a::liella_compat_key_decryptsession(s, b.cast())))),
        ("key_encryptsession_pk", -1, Box::new(move |b| i64::from(a::liella_compat_key_encryptsession_pk(s, b.cast(), b.wrapping_add(8).cast())))),
        ("key_decryptsession_pk", -1, Box::new(move |b| i64::from(a::liella_compat_key_decryptsession_pk(s, b.cast(), b.wrapping_add(8).cast())))),
        ("key_get_conv", -1, Box::new(move |b| i64::from(a::liella_compat_key_get_conv(s, b.cast())))),
        ("key_setnet", -1, Box::new(|b| i64::from(a::liella_compat_key_setnet(b.cast())))),
        ("getpublickey", 0, Box::new(move |b| i64::from(a::liella_compat_getpublickey(s, b)))),
        ("getsecretkey", 0, Box::new(move |b| i64::from(a::liella_compat_getsecretkey(s, b, s)))),
        ("authdes_create", 0, Box::new(move |b| p(a::liella_compat_authdes_create(s, 60, v, b.cast())))),
        ("authdes_pk_create", 0, Box::new(move |b| p(a::liella_compat_authdes_pk_create(s, b.cast(), 60, v, b.wrapping_add(8).cast())))),
        ("authdes_getucred", 0, Box::new(move |b| i64::from(a::liella_compat_authdes_getucred(b.cast(), u, u, null_mut(), u)))),
        ("user2netname", 0, Box::new(move |b| i64::from(a::liella_compat_user2netname(b, 0, s)))),
        ("host2netname", 0, Box::new(move |b| i64::from(a::liella_compat_host2netname(b, s, s)))),
        ("getnetname", 0, Box::new(|b| i64::from(a::liella_compat_getnetname(b)))),
        ("netname2user", 0, Box::new(move |b| i64::from(a::liella_compat_netname2user(s, b.cast(), u, null_mut::<c_int>(), u)))),
        ("netname2host", 0, Box::new(move |b| i64::from(a::liella_compat_netname2host(s, b, 64)))),
        ("authunix_create", 0, Box::new(move |b| p(a::liella_compat_authunix_create(b, 0, 0, 0, null_mut()).cast()))),
        ("authunix_create_default", 0, Box::new(move |_| p(a::liella_compat_authunix_create_default().cast()))),
    ];
    let before = open_fds();
    for (name, want, f) in &table {
        let (got, e, buf) = run(f.as_ref());
        assert_eq!(got, *want, "{name}: failure value");
        assert_eq!(e, a::EOPNOTSUPP, "{name}: errno");
        assert!(buf.iter().all(|&c| c == CANARY), "{name}: wrote into the caller's buffer");
        assert_eq!(open_fds(), before, "{name}: opened a descriptor");
    }
}

#[test]
fn passwd2des_clears_its_key_and_fails() {
    // Trace: no spec clause (§29.1 `passwd2des` stub; D-29.3, owner ruling 2026-10-27).
    let mut key = [CANARY; 8];
    let mut pw = *b"secret\0";
    errno::set_raw(0);
    a::liella_compat_passwd2des(pw.as_mut_ptr().cast(), key.as_mut_ptr().cast());
    assert_eq!(key, [0; 8]);
    assert_eq!(errno::get(), a::EOPNOTSUPP);
    assert_eq!(&pw, b"secret\0");
}
