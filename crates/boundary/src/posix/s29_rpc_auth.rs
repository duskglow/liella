//! §29.1 ONC RPC authentication helpers as failing stubs (D-29.3, owner ruling 2026-10-27).
//!
//! Liella does not implement secure-RPC authentication, the key server client or the DES
//! API: `ecb_crypt`, `cbc_crypt`, `des_setparity`, `passwd2des`, `xencrypt`, `xdecrypt`,
//! the `key_*` calls, `getpublickey`, `getsecretkey`, `authdes_*`, `authunix_*` and the
//! `netname` helpers. Each name stays exported at its compat version so an old program still
//! loads; a call fails with its family's failure value (the table in D-29.3), sets `errno` to
//! `EOPNOTSUPP`, and never touches the network, a file, a key server or the memory its arguments
//! point at. Every entry is an `internal_entry!`: `cargo xtask so` binds each compat version to
//! the `liella_compat_<name>` symbol, so a plain `pub extern "C" fn` (no symbol) fails the build.

use liella_abi::ctypes::{c_char, c_int, c_uint, c_void};

use super::s29_rpc_clnt::AuthC;

/// `EOPNOTSUPP`: the errno every stub here sets.
pub const EOPNOTSUPP: i32 = 95;
/// `DESERR_HWERROR`: what `ecb_crypt` and `cbc_crypt` return (`DES_FAILED` is true for it).
pub const DESERR_HWERROR: c_int = 2;

fn unsupported() {
    crate::errno::set_raw(EOPNOTSUPP);
}

/// Fails with `-1` (the `key_*` calls).
fn fail_neg() -> c_int {
    unsupported();
    -1
}

/// Fails with `0` (`FALSE`, the netname and key-lookup calls).
fn fail_zero() -> c_int {
    unsupported();
    0
}

crate::internal_entry! {
/// `ecb_crypt`: `DESERR_HWERROR`, the buffer untouched.
pub extern "C" fn liella_compat_ecb_crypt(_key: *mut c_char, _buf: *mut c_char, _len: c_uint, _mode: c_uint) -> c_int {
    unsupported();
    DESERR_HWERROR
}
}

crate::internal_entry! {
/// `cbc_crypt`: `DESERR_HWERROR`, the buffer and vector untouched.
pub extern "C" fn liella_compat_cbc_crypt(_key: *mut c_char, _buf: *mut c_char, _len: c_uint, _mode: c_uint, _ivec: *mut c_char) -> c_int {
    unsupported();
    DESERR_HWERROR
}
}

crate::internal_entry! {
/// `des_setparity`: no effect.
pub extern "C" fn liella_compat_des_setparity(_key: *mut c_char) {
    unsupported();
}
}

crate::internal_entry! {
/// `passwd2des`: no effect, the key untouched.
pub extern "C" fn liella_compat_passwd2des(_passwd: *mut c_char, _key: *mut c_char) {
    unsupported();
}
}

crate::internal_entry! {
/// `xencrypt`: `0` (failure), the text untouched.
pub extern "C" fn liella_compat_xencrypt(_secret: *mut c_char, _passwd: *mut c_char) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `xdecrypt`: `0` (failure), the text untouched.
pub extern "C" fn liella_compat_xdecrypt(_secret: *mut c_char, _passwd: *mut c_char) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `key_setsecret`: `-1`.
pub extern "C" fn liella_compat_key_setsecret(_secret: *const c_char) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `key_secretkey_is_set`: `0` (no key is set).
pub extern "C" fn liella_compat_key_secretkey_is_set() -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `key_gendes`: `-1`.
pub extern "C" fn liella_compat_key_gendes(_key: *mut c_void) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `key_encryptsession`: `-1`.
pub extern "C" fn liella_compat_key_encryptsession(_name: *const c_char, _key: *mut c_void) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `key_decryptsession`: `-1`.
pub extern "C" fn liella_compat_key_decryptsession(_name: *const c_char, _key: *mut c_void) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `key_encryptsession_pk`: `-1`.
pub extern "C" fn liella_compat_key_encryptsession_pk(_name: *const c_char, _pk: *mut c_void, _key: *mut c_void) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `key_decryptsession_pk`: `-1`.
pub extern "C" fn liella_compat_key_decryptsession_pk(_name: *const c_char, _pk: *mut c_void, _key: *mut c_void) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `key_get_conv`: `-1`.
pub extern "C" fn liella_compat_key_get_conv(_pk: *const c_char, _key: *mut c_void) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `key_setnet`: `-1`.
pub extern "C" fn liella_compat_key_setnet(_arg: *mut c_void) -> c_int {
    fail_neg()
}
}

crate::internal_entry! {
/// `getpublickey`: `0` (not found).
pub extern "C" fn liella_compat_getpublickey(_name: *const c_char, _key: *mut c_char) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `getsecretkey`: `0` (not found).
pub extern "C" fn liella_compat_getsecretkey(_name: *const c_char, _key: *mut c_char, _passwd: *const c_char) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `authdes_create`: `NULL`.
pub extern "C" fn liella_compat_authdes_create(_name: *const c_char, _window: c_uint, _addr: *mut c_void, _key: *mut c_void) -> *mut c_void {
    unsupported();
    core::ptr::null_mut()
}
}

crate::internal_entry! {
/// `authdes_pk_create`: `NULL`.
pub extern "C" fn liella_compat_authdes_pk_create(_name: *const c_char, _pk: *mut c_void, _window: c_uint, _addr: *mut c_void, _key: *mut c_void) -> *mut c_void {
    unsupported();
    core::ptr::null_mut()
}
}

crate::internal_entry! {
/// `authdes_getucred`: `0` (no credential).
pub extern "C" fn liella_compat_authdes_getucred(_cred: *const c_void, _uid: *mut c_uint, _gid: *mut c_uint, _len: *mut i16, _groups: *mut c_uint) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `user2netname`: `0` (failure), the output untouched.
pub extern "C" fn liella_compat_user2netname(_out: *mut c_char, _uid: c_uint, _dom: *const c_char) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `host2netname`: `0` (failure), the output untouched.
pub extern "C" fn liella_compat_host2netname(_out: *mut c_char, _host: *const c_char, _dom: *const c_char) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `getnetname`: `0` (failure), the output untouched.
pub extern "C" fn liella_compat_getnetname(_out: *mut c_char) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `netname2user`: `0` (failure).
pub extern "C" fn liella_compat_netname2user(_name: *const c_char, _uid: *mut c_uint, _gid: *mut c_uint, _len: *mut c_int, _groups: *mut c_uint) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `netname2host`: `0` (failure).
pub extern "C" fn liella_compat_netname2host(_name: *const c_char, _host: *mut c_char, _len: c_int) -> c_int {
    fail_zero()
}
}

crate::internal_entry! {
/// `authunix_create`: `NULL`.
pub extern "C" fn liella_compat_authunix_create(_machname: *mut c_char, _uid: c_int, _gid: c_int, _len: c_int, _gids: *mut c_int) -> *mut AuthC {
    unsupported();
    core::ptr::null_mut()
}
}

crate::internal_entry! {
/// `authunix_create_default`: `NULL`.
pub extern "C" fn liella_compat_authunix_create_default() -> *mut AuthC {
    unsupported();
    core::ptr::null_mut()
}
}
