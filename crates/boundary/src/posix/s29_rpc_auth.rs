//! §29.1 ONC RPC authentication helpers (LG-16, D-29.1, R15-4 provisional): `ecb_crypt`,
//! `cbc_crypt` (FIPS 46 DES), `des_setparity`, `passwd2des`, `xencrypt`, `xdecrypt` (the
//! reference's key and hex handling, bit for bit, owner ruling #1675; DES-based legacy functions
//! kept for existing encrypted keys, with no meaningful security today: new software should use
//! modern authentication), the `key_*` and `authdes_*` calls as they behave
//! with no key server running (probed), and the `netname` helpers.
//!
//! A DES-authenticated RPC path is NOT built (R15-4 is the owner's): it would live in
//! `liella_compat_authdes_pk_create` (session key from `key_gendes`, encrypted with
//! `key_encryptsession_pk`, verifier timestamps through `ecb_crypt`) and its server side in
//! `authdes_getucred`'s credential cache.

use core::ffi::CStr;
use core::ptr;

use liella_abi::ctypes::{c_char, c_int, c_uint, c_void};
use liella_legacy::des;

use super::s17_users::{getegid, geteuid, getgroups};
use super::s23_time::time;
use super::s24_system::{getdomainname, gethostname};
use super::s29_legacy::Xdr;
use super::s29_rpc_clnt::{AuthC, AuthOps, put_bytes};
use super::s29_rpc_svc::OpaqueAuth;
use crate::{heap, ret};

const ENOENT: i32 = 2;
const EACCES: i32 = 13;
/// `MAXNETNAMELEN`.
const MAXNETNAMELEN: usize = 255;
/// The longest host or domain part `host2netname` keeps.
const NAME_PART: usize = 64;

// UNSAFE(slice): the caller's 8-byte key and `len`-byte buffer.
#[allow(unsafe_code)]
fn crypt(key: *mut c_char, buf: *mut c_char, len: c_uint, mode: c_uint, iv: *mut c_char) -> c_int {
    let n = usize::try_from(len).unwrap_or(usize::MAX);
    let rc = match des::status(n, mode) {
        Ok(rc) => rc,
        Err(e) => return e,
    };
    if key.is_null() || (n != 0 && buf.is_null()) {
        return des::DESERR_BADPARAM;
    }
    // UNSAFE(slice) SAFETY: key points at 8 bytes (`des_block`, the C contract).
    let k: [u8; 8] = unsafe { key.cast::<[u8; 8]>().read_unaligned() };
    let data: &mut [u8] = if n == 0 {
        &mut []
    } else {
        // UNSAFE(slice) SAFETY: buf holds len bytes (the C contract), checked non-null above.
        unsafe { core::slice::from_raw_parts_mut(buf.cast::<u8>(), n) }
    };
    let decrypt = mode & des::DES_DECRYPT != 0;
    if iv.is_null() {
        des::ecb(&k, data, decrypt);
    } else {
        // UNSAFE(slice) SAFETY: ivec points at 8 bytes (the C contract).
        let mut v: [u8; 8] = unsafe { iv.cast::<[u8; 8]>().read_unaligned() };
        des::cbc(&k, data, &mut v, decrypt);
        // UNSAFE(slice) SAFETY: as above.
        unsafe { iv.cast::<[u8; 8]>().write_unaligned(v) };
    }
    rc
}

crate::internal_entry! {
/// `ecb_crypt(key, buf, len, mode)`.
pub extern "C" fn liella_compat_ecb_crypt(key: *mut c_char, buf: *mut c_char, len: c_uint, mode: c_uint) -> c_int {
    crypt(key, buf, len, mode, ptr::null_mut())
}}

crate::internal_entry! {
/// `cbc_crypt(key, buf, len, mode, ivec)`: `ivec` ends as the last ciphertext block.
pub extern "C" fn liella_compat_cbc_crypt(key: *mut c_char, buf: *mut c_char, len: c_uint, mode: c_uint, ivec: *mut c_char) -> c_int {
    if ivec.is_null() {
        return des::DESERR_BADPARAM;
    }
    crypt(key, buf, len, mode, ivec)
}}

crate::internal_entry! {
/// `des_setparity(key)`: as the reference, bit 0x80 of each byte cleared, odd parity in its low bit.
pub extern "C" fn liella_compat_des_setparity(key: *mut c_char) {
    if key.is_null() {
        return;
    }
    // UNSAFE(slice) SAFETY: key points at 8 bytes (the C contract).
    let mut k: [u8; 8] = unsafe { key.cast::<[u8; 8]>().read_unaligned() };
    des::setparity(&mut k);
    // UNSAFE(slice) SAFETY: as above.
    unsafe { key.cast::<[u8; 8]>().write_unaligned(k) };
}}

crate::internal_entry! {
/// `passwd2des(passwd, key)`: the DES key of a password (first eight bytes, shifted left by one,
/// then `des_setparity`). A legacy DES function kept for existing encrypted keys.
pub extern "C" fn liella_compat_passwd2des(passwd: *mut c_char, key: *mut c_char) {
    if key.is_null() {
        return;
    }
    let k = des::passwd2des(text(passwd).unwrap_or(&[]));
    // UNSAFE(slice) SAFETY: key points at 8 bytes (the C contract).
    unsafe { key.cast::<[u8; 8]>().write_unaligned(k) };
}}

// UNSAFE(slice): the caller's NUL-terminated hex text, rewritten in place.
#[allow(unsafe_code)]
fn xcrypt(secret: *mut c_char, passwd: *mut c_char, decrypt: bool) -> c_int {
    if secret.is_null() {
        return 0;
    }
    // UNSAFE(slice) SAFETY: secret is a NUL-terminated string (the C contract), checked non-null.
    let len = unsafe { CStr::from_ptr(secret) }.to_bytes().len();
    // UNSAFE(slice) SAFETY: the len bytes before its NUL, writable (the C contract).
    let s = unsafe { core::slice::from_raw_parts_mut(secret.cast::<u8>(), len) };
    match des::xcrypt(s, text(passwd).unwrap_or(&[]), decrypt) {
        Some(end) => {
            if let Some(c) = s.get_mut(end) {
                *c = 0;
            }
            1
        }
        None => 0,
    }
}

crate::internal_entry! {
/// `xencrypt(secret, passwd)`: 1 when the hex text was encrypted in place, 0 (text untouched,
/// `errno` untouched) when half its length is not a multiple of 8 or above `DES_MAXDATA`.
pub extern "C" fn liella_compat_xencrypt(secret: *mut c_char, passwd: *mut c_char) -> c_int {
    xcrypt(secret, passwd, false)
}}

crate::internal_entry! {
/// `xdecrypt(secret, passwd)`: as `xencrypt`, decrypting.
pub extern "C" fn liella_compat_xdecrypt(secret: *mut c_char, passwd: *mut c_char) -> c_int {
    xcrypt(secret, passwd, true)
}}

/// A key server call that fails as with no key server running: -1 and `errno`.
fn no_keyserv(errno: i32) -> c_int {
    crate::errno::set_raw(errno);
    -1
}

crate::internal_entry! {
/// `key_setsecret`: no key server (-1, `ENOENT`).
pub extern "C" fn liella_compat_key_setsecret(_secret: *const c_char) -> c_int { no_keyserv(ENOENT) }}
crate::internal_entry! {
/// `key_secretkey_is_set`: 0 (no key server; `errno` `ENOENT`).
pub extern "C" fn liella_compat_key_secretkey_is_set() -> c_int { crate::errno::set_raw(ENOENT); 0 }}
crate::internal_entry! {
/// `key_gendes`: no key server (-1, `EACCES`; the block is untouched).
pub extern "C" fn liella_compat_key_gendes(_key: *mut c_void) -> c_int { no_keyserv(EACCES) }}
crate::internal_entry! {
/// `key_encryptsession`: no key server (-1, `ENOENT`).
pub extern "C" fn liella_compat_key_encryptsession(_name: *const c_char, _key: *mut c_void) -> c_int { no_keyserv(ENOENT) }}
crate::internal_entry! {
/// `key_decryptsession`: no key server (-1, `ENOENT`).
pub extern "C" fn liella_compat_key_decryptsession(_name: *const c_char, _key: *mut c_void) -> c_int { no_keyserv(ENOENT) }}
crate::internal_entry! {
/// `key_encryptsession_pk`: no key server (-1, `ENOENT`).
pub extern "C" fn liella_compat_key_encryptsession_pk(_name: *const c_char, _pk: *mut c_void, _key: *mut c_void) -> c_int { no_keyserv(ENOENT) }}
crate::internal_entry! {
/// `key_decryptsession_pk`: no key server (-1, `ENOENT`).
pub extern "C" fn liella_compat_key_decryptsession_pk(_name: *const c_char, _pk: *mut c_void, _key: *mut c_void) -> c_int { no_keyserv(ENOENT) }}
crate::internal_entry! {
/// `key_get_conv`: no key server (-1, `ENOENT`).
pub extern "C" fn liella_compat_key_get_conv(_pk: *const c_char, _key: *mut c_void) -> c_int { no_keyserv(ENOENT) }}
crate::internal_entry! {
/// `key_setnet`: no key server (-1, `ENOENT`).
pub extern "C" fn liella_compat_key_setnet(_arg: *mut c_void) -> c_int { no_keyserv(ENOENT) }}
crate::internal_entry! {
/// `getpublickey`: no `publickey` source answers (0; the buffer is untouched).
pub extern "C" fn liella_compat_getpublickey(_name: *const c_char, _key: *mut c_char) -> c_int { 0 }}
crate::internal_entry! {
/// `getsecretkey`: no `publickey` source answers (0; the buffer is untouched).
pub extern "C" fn liella_compat_getsecretkey(_name: *const c_char, _key: *mut c_char, _passwd: *const c_char) -> c_int { 0 }}
crate::internal_entry! {
/// `authdes_create`: NULL (no public key for the server; `errno` untouched).
pub extern "C" fn liella_compat_authdes_create(_name: *const c_char, _window: c_uint, _addr: *mut c_void, _key: *mut c_void) -> *mut c_void { ptr::null_mut() }}
crate::internal_entry! {
/// `authdes_pk_create`: NULL (no key server: `EACCES`).
pub extern "C" fn liella_compat_authdes_pk_create(_name: *const c_char, _pk: *mut c_void, _window: c_uint, _addr: *mut c_void, _key: *mut c_void) -> *mut c_void {
    crate::errno::set_raw(EACCES);
    ptr::null_mut()
}}
crate::internal_entry! {
/// `authdes_getucred`: 0 (no credential is cached; the outputs are untouched).
pub extern "C" fn liella_compat_authdes_getucred(_cred: *const c_void, _uid: *mut c_uint, _gid: *mut c_uint, _len: *mut i16, _groups: *mut c_uint) -> c_int { 0 }}

// UNSAFE(slice): a C string argument.
#[allow(unsafe_code)]
fn text<'a>(p: *const c_char) -> Option<&'a [u8]> {
    // UNSAFE(slice) SAFETY: p is NULL or a NUL-terminated string (the C contract).
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_bytes())
}

/// The system's NIS domain name (`(none)` when unset, as the kernel reports it).
fn domain(buf: &mut [u8; 65]) -> &[u8] {
    if getdomainname(buf.as_mut_ptr().cast(), buf.len()) != 0 {
        return &[];
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    buf.get(..n).unwrap_or(&[])
}

// UNSAFE(slice): the caller's `MAXNETNAMELEN + 1`-byte result buffer.
#[allow(unsafe_code)]
fn put(out: *mut c_char, parts: &[&[u8]]) -> c_int {
    let n = parts.iter().map(|p| p.len()).fold(0usize, usize::saturating_add);
    if out.is_null() || n > MAXNETNAMELEN {
        return 0;
    }
    // UNSAFE(slice) SAFETY: out holds MAXNETNAMELEN + 1 bytes (the C contract) and n <= MAXNETNAMELEN.
    let dst = unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), n.saturating_add(1)) };
    let mut at = 0usize;
    for p in parts {
        if let Some(d) = dst.get_mut(at..at.saturating_add(p.len())) {
            d.copy_from_slice(p);
        }
        at = at.saturating_add(p.len());
    }
    if let Some(z) = dst.get_mut(at) {
        *z = 0;
    }
    1
}

/// The decimal text of `v` (signed, as the reference prints a uid).
fn decimal(v: i32, buf: &mut [u8; 12]) -> &[u8] {
    let mut at = buf.len();
    let mut m = v.unsigned_abs();
    loop {
        at = at.saturating_sub(1);
        if let Some(d) = buf.get_mut(at) {
            *d = b'0'.saturating_add(u8::try_from(m % 10).unwrap_or(0));
        }
        m /= 10;
        if m == 0 {
            break;
        }
    }
    if v < 0 {
        at = at.saturating_sub(1);
        if let Some(d) = buf.get_mut(at) {
            *d = b'-';
        }
    }
    buf.get(at..).unwrap_or(&[])
}

fn user_netname(out: *mut c_char, uid: c_uint, dom: Option<&[u8]>) -> c_int {
    let mut db = [0u8; 65];
    let dom = match dom {
        Some(d) => d,
        None => domain(&mut db),
    };
    // The reference reserves room for the longest uid: "unix." + 11 digits + "@" + NUL.
    if dom.len().saturating_add(18) > MAXNETNAMELEN {
        return 0;
    }
    let mut nb = [0u8; 12];
    put(out, &[b"unix.", decimal(i32::from_ne_bytes(uid.to_ne_bytes()), &mut nb), b"@", dom])
}

crate::internal_entry! {
/// `user2netname(out, uid, domain)`: `unix.<uid>@<domain>`, the system domain when NULL.
pub extern "C" fn liella_compat_user2netname(out: *mut c_char, uid: c_uint, dom: *const c_char) -> c_int {
    user_netname(out, uid, text(dom))
}}

crate::internal_entry! {
/// `host2netname(out, host, domain)`: `unix.<host>@<domain>`; the host is cut at its first dot
/// (the rest is the domain when none is given), each part at 64 bytes; an empty domain fails.
pub extern "C" fn liella_compat_host2netname(out: *mut c_char, host: *const c_char, dom: *const c_char) -> c_int {
    let mut hb = [0u8; 65];
    let host = match text(host) {
        Some(h) => h,
        None => {
            if gethostname(hb.as_mut_ptr().cast(), hb.len().saturating_sub(1)) != 0 {
                return 0;
            }
            let n = hb.iter().position(|&c| c == 0).unwrap_or(0);
            hb.get(..n).unwrap_or(&[])
        }
    };
    let (name, rest) = match host.iter().position(|&c| c == b'.') {
        Some(i) => (host.get(..i).unwrap_or(&[]), host.get(i.saturating_add(1)..)),
        None => (host, None),
    };
    let mut db = [0u8; 65];
    let dom = match (text(dom), rest) {
        (Some(d), _) => d,
        (None, Some(r)) => r,
        (None, None) => domain(&mut db),
    };
    if dom.is_empty() {
        return 0;
    }
    put(out, &[b"unix.", name.get(..name.len().min(NAME_PART)).unwrap_or(&[]), b"@", dom.get(..dom.len().min(NAME_PART)).unwrap_or(&[])])
}}

crate::internal_entry! {
/// `getnetname(out)`: the host's netname for the superuser, the user's otherwise.
pub extern "C" fn liella_compat_getnetname(out: *mut c_char) -> c_int {
    let uid = geteuid();
    if uid == 0 { liella_compat_host2netname(out, ptr::null(), ptr::null()) } else { user_netname(out, uid, None) }
}}

crate::internal_entry! {
/// `netname2user`: 0 (no `netid` source answers).
pub extern "C" fn liella_compat_netname2user(_name: *const c_char, _uid: *mut c_uint, _gid: *mut c_uint, _len: *mut c_int, _groups: *mut c_uint) -> c_int { 0 }}

crate::internal_entry! {
/// `netname2host`: 0 (no `netid` source answers; the buffer is untouched).
pub extern "C" fn liella_compat_netname2host(_name: *const c_char, _host: *mut c_char, _len: c_int) -> c_int { 0 }}

/// `NGRPS`: the most groups an `AUTH_UNIX` credential carries.
const NGRPS: usize = 16;
/// `MAX_MACHINE_NAME`: the longest machine name an `AUTH_UNIX` credential carries.
const MAX_MACHINE_NAME: usize = 255;
/// The largest credential body: stamp, the name (length word, 256 padded bytes), uid, gid, the
/// group count and 16 groups.
const UNIX_BODY: usize = 340;

/// An `AUTH_UNIX` handle and its encoded credential body, one heap block (its destroy frees it).
#[repr(C)]
struct UnixAuth {
    auth: AuthC,
    body: [u8; UNIX_BODY],
}

/// Writes the RFC 5531 `authsys_parms` body into `out`: stamp, machine name (XDR string, zero
/// padded to 4 bytes), uid, gid, then the groups (counted array). `None` past 255 name bytes or
/// when the groups do not fit.
fn unix_body(out: &mut [u8; UNIX_BODY], stamp: u32, name: &[u8], uid: u32, gid: u32, gids: &[u32]) -> Option<usize> {
    if name.len() > MAX_MACHINE_NAME {
        return None;
    }
    let mut at = 0usize;
    let mut put = |b: &[u8]| -> Option<()> {
        let end = at.checked_add(b.len())?;
        out.get_mut(at..end)?.copy_from_slice(b);
        at = end;
        Some(())
    };
    put(&stamp.to_be_bytes())?;
    put(&u32::try_from(name.len()).ok()?.to_be_bytes())?;
    put(name)?;
    put([0u8; 3].get(..name.len().wrapping_neg() & 3)?)?;
    put(&uid.to_be_bytes())?;
    put(&gid.to_be_bytes())?;
    put(&u32::try_from(gids.len()).ok()?.to_be_bytes())?;
    for g in gids {
        put(&g.to_be_bytes())?;
    }
    Some(at)
}

static AUTHUNIX_OPS: AuthOps = AuthOps { ah_nextverf: Some(au_nextverf), ah_marshal: Some(au_marshal), ah_validate: Some(au_validate), ah_refresh: Some(au_refresh), ah_destroy: Some(au_destroy) };

extern "C" fn au_nextverf(_a: *mut AuthC) {}

// UNSAFE(slice): the handle's credential body.
#[allow(unsafe_code)]
/// Writes the credential (flavor `AUTH_UNIX`, length, body) and the empty `AUTH_NONE` verifier;
/// 0 when the stream has no room.
extern "C" fn au_marshal(a: *mut AuthC, x: *mut Xdr) -> c_int {
    // UNSAFE(slice) SAFETY: a is NULL or a handle made by authunix_create.
    let Some(au) = (unsafe { a.as_ref() }) else { return 0 };
    let n = usize::try_from(au.ah_cred.oa_length).unwrap_or(0).min(UNIX_BODY);
    // UNSAFE(slice) SAFETY: oa_base points at the handle's UNIX_BODY-byte body and n <= UNIX_BODY.
    let body = unsafe { core::slice::from_raw_parts(au.ah_cred.oa_base.cast::<u8>(), n) };
    let ok = put_bytes(x, &1u32.to_be_bytes()) && put_bytes(x, &au.ah_cred.oa_length.to_be_bytes()) && put_bytes(x, body) && put_bytes(x, &[0u8; 8]);
    c_int::from(ok)
}

extern "C" fn au_validate(_a: *mut AuthC, _v: *mut OpaqueAuth) -> c_int {
    1
}

extern "C" fn au_refresh(_a: *mut AuthC) -> c_int {
    0
}

/// Frees the handle and its credential (one block).
extern "C" fn au_destroy(a: *mut AuthC) {
    if !a.is_null() {
        heap::free_entry(a.expose_provenance());
    }
}

/// The handle for an encoded credential; aborts (a caller error, as the reference) when the name
/// is missing or too long.
// UNSAFE(slice): a fresh block of UnixAuth's size.
#[allow(unsafe_code)]
fn unix_create(name: Option<&[u8]>, uid: c_int, gid: c_int, gids: &[u32]) -> *mut AuthC {
    let Some(name) = name else { ret::abort() };
    let stamp = u32::try_from(time(None).rem_euclid(0x1_0000_0000)).unwrap_or(0);
    let mut body = [0u8; UNIX_BODY];
    let Some(n) = unix_body(&mut body, stamp, name, u32::from_ne_bytes(uid.to_ne_bytes()), u32::from_ne_bytes(gid.to_ne_bytes()), gids) else { ret::abort() };
    let p = heap::malloc_entry(core::mem::size_of::<UnixAuth>()).cast::<UnixAuth>();
    if p.is_null() {
        return ptr::null_mut();
    }
    let len = c_uint::try_from(n).unwrap_or(0);
    let none = OpaqueAuth { oa_flavor: 0, oa_base: ptr::null_mut(), oa_length: 0 };
    let cred = OpaqueAuth { oa_flavor: 1, oa_base: ptr::null_mut(), oa_length: len };
    let auth = AuthC { ah_cred: cred, ah_verf: none, ah_key: [0; 8], ah_ops: &AUTHUNIX_OPS, ah_private: ptr::null_mut() };
    // UNSAFE(slice) SAFETY: p is a fresh block of UnixAuth's size, malloc-aligned (16).
    unsafe { p.write(UnixAuth { auth, body }) };
    // UNSAFE(slice) SAFETY: p was just written and is not shared yet.
    let au = unsafe { &mut *p };
    au.auth.ah_cred.oa_base = au.body.as_mut_ptr().cast();
    au.auth.ah_private = au.auth.ah_cred.oa_base;
    p.cast::<AuthC>()
}

crate::internal_entry! {
// UNSAFE(slice): the caller's `len`-entry group list.
#[allow(unsafe_code)]
/// `authunix_create`: an `AUTH_UNIX` handle for `machname`, `uid`, `gid` and `len` groups. A NULL
/// or over-255-byte name, more than 16 groups or a negative count abort the process (probed).
pub extern "C" fn liella_compat_authunix_create(machname: *mut c_char, uid: c_int, gid: c_int, len: c_int, gids: *mut c_int) -> *mut AuthC {
    let Some(n) = usize::try_from(len).ok().filter(|&n| n <= NGRPS) else { ret::abort() };
    if n != 0 && gids.is_null() {
        ret::abort();
    }
    let groups: &[u32] = if n == 0 {
        &[]
    } else {
        // UNSAFE(slice) SAFETY: gids points at len (= n) ints (the C contract).
        unsafe { core::slice::from_raw_parts(gids.cast::<u32>(), n) }
    };
    unix_create(text(machname), uid, gid, groups)
}}

crate::internal_entry! {
/// `authunix_create_default`: the handle for this host (`gethostname`), the effective uid and
/// gid and the supplementary groups (`getgroups`, the first 16 when there are more).
pub extern "C" fn liella_compat_authunix_create_default() -> *mut AuthC {
    let mut host = [0u8; 256];
    if gethostname(host.as_mut_ptr().cast(), MAX_MACHINE_NAME) != 0 {
        host.fill(0);
    }
    let hn = host.iter().position(|&c| c == 0).unwrap_or(MAX_MACHINE_NAME);
    let mut first = [0u32; NGRPS];
    let n = default_groups(&mut first);
    unix_create(host.get(..hn), c_int::from_ne_bytes(geteuid().to_ne_bytes()), c_int::from_ne_bytes(getegid().to_ne_bytes()), first.get(..n).unwrap_or(&[]))
}}

/// The first (up to) 16 supplementary groups into `out`; their count. Only a process in more than
/// 16 groups reads its whole list (into a heap block it frees).
// UNSAFE(slice): a heap block of `count` group ids.
#[allow(unsafe_code)]
fn default_groups(out: &mut [u32; NGRPS]) -> usize {
    let count = getgroups(0, ptr::null_mut());
    let Ok(want) = usize::try_from(count) else { return 0 };
    if want <= NGRPS {
        return usize::try_from(getgroups(c_int::try_from(NGRPS).unwrap_or(0), out.as_mut_ptr())).unwrap_or(0);
    }
    let Some(bytes) = want.checked_mul(4) else { return 0 };
    let p = heap::malloc_entry(bytes).cast::<u32>();
    if p.is_null() {
        return 0;
    }
    let got = usize::try_from(getgroups(count, p)).unwrap_or(0).min(want);
    // UNSAFE(slice) SAFETY: p is a fresh block of want ids, the first got <= want written by getgroups.
    let all = unsafe { core::slice::from_raw_parts(p, got) };
    let n = got.min(NGRPS);
    if let (Some(d), Some(s)) = (out.get_mut(..n), all.get(..n)) {
        d.copy_from_slice(s);
    }
    heap::free_entry(p.expose_provenance());
    n
}

/// The fields of an `AUTH` handle a caller reads: credential flavor and length, verifier flavor
/// and length, and whether `ah_private` is set. `a` is NULL or a handle from `authunix_create*`.
// UNSAFE(slice): the caller's handle.
#[allow(unsafe_code, clippy::not_unsafe_ptr_arg_deref)]
pub fn auth_fields(a: *mut AuthC) -> Option<(c_int, c_uint, c_int, c_uint, bool)> {
    // UNSAFE(slice) SAFETY: a is NULL or a live handle (the C contract).
    let au = unsafe { a.as_ref() }?;
    Some((au.ah_cred.oa_flavor, au.ah_cred.oa_length, au.ah_verf.oa_flavor, au.ah_verf.oa_length, !au.ah_private.is_null()))
}

/// The operations table of an `AUTH` handle (`a` NULL or a live handle).
// UNSAFE(slice): the caller's handle and its static table.
#[allow(unsafe_code, clippy::not_unsafe_ptr_arg_deref)]
pub fn auth_ops(a: *mut AuthC) -> Option<&'static AuthOps> {
    // UNSAFE(slice) SAFETY: a is NULL or a live handle (the C contract).
    let au = unsafe { a.as_ref() }?;
    // UNSAFE(slice) SAFETY: ah_ops of a live handle is a static table.
    unsafe { au.ah_ops.as_ref() }
}
