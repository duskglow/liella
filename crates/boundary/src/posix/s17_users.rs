//! Translation for spec §17: users and groups (phase 5: §17.1, the credential queries; phase 6:
//! §17.2, changing them; phase 10: §17.3–17.6, the user, group, shadow and gshadow databases,
//! the login name, login shells and login accounting, and `initgroups`). The C conventions of
//! the phase-10 functions are in `crate::users`.
//!
//! The ID queries cannot fail and return the `Sys` values directly; `getgroups`' count query
//! and `group_member` are in `liella_legacy::cred_compat`.

use liella_abi::ctypes::{c_char, c_int, c_uint, c_void, size_t};
use liella_abi::layout::{struct_group, struct_passwd, struct_sgrp, struct_spwd, struct_utmp};
use liella_legacy::nss_files::Key;
use liella_kernel::sys::Sys;
use liella_legacy::cred_compat;

use crate::conv;
use crate::export;
use crate::nr;
use crate::setxid;
use crate::ret;
use crate::strz;
use crate::sys::CSys;
use crate::users;

export! { "GLIBC_2.2.5";
pub extern "C" fn getuid() -> c_uint {
    CSys.getuid()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn geteuid() -> c_uint {
    CSys.geteuid()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgid() -> c_uint {
    CSys.getgid()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getegid() -> c_uint {
    CSys.getegid()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getresuid(r: Option<&mut c_uint>, e: Option<&mut c_uint>, s: Option<&mut c_uint>) -> c_int {
    ret::triple(r, e, s, CSys.getresuid())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getresgid(r: Option<&mut c_uint>, e: Option<&mut c_uint>, s: Option<&mut c_uint>) -> c_int {
    ret::triple(r, e, s, CSys.getresgid())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgroups(size: c_int, list: *mut c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: list holds size gid_t entries (§17.1).
    ret::count(unsafe { conv::array_mut::<u32>(list.cast(), i64::from(size)) }.and_then(|l| cred_compat::getgroups(&CSys, size, l)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __getgroups_chk(size: c_int, list: *mut c_uint, listlen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: as getgroups, after the fortify check.
    ret::count(unsafe { conv::array_mut::<u32>(list.cast(), conv::groups_chk(size, listlen)) }.and_then(|l| cred_compat::getgroups(&CSys, size, l)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn group_member(gid: c_uint) -> c_int {
    ret::int(cred_compat::group_member(&CSys, gid).map(i64::from))
}}

// ------------------------------------------------------------------------------ changing credentials (§17.2)
// Each change applies to every thread the runtime runs (D-17.1, D-17.7).

export! { "GLIBC_2.2.5";
pub extern "C" fn setuid(uid: u32) -> c_int {
    ret::zero(setxid::change(nr::SETUID, setxid::id(uid), 0, 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setgid(gid: u32) -> c_int {
    ret::zero(setxid::change(nr::SETGID, setxid::id(gid), 0, 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn seteuid(uid: u32) -> c_int {
    ret::zero(setxid::set_effective(nr::SETRESUID, uid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setegid(gid: u32) -> c_int {
    ret::zero(setxid::set_effective(nr::SETRESGID, gid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setreuid(ruid: u32, euid: u32) -> c_int {
    ret::zero(setxid::change(nr::SETREUID, setxid::id(ruid), setxid::id(euid), 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setregid(rgid: u32, egid: u32) -> c_int {
    ret::zero(setxid::change(nr::SETREGID, setxid::id(rgid), setxid::id(egid), 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setresuid(ruid: u32, euid: u32, suid: u32) -> c_int {
    ret::zero(setxid::change(nr::SETRESUID, setxid::id(ruid), setxid::id(euid), setxid::id(suid)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setresgid(rgid: u32, egid: u32, sgid: u32) -> c_int {
    ret::zero(setxid::change(nr::SETRESGID, setxid::id(rgid), setxid::id(egid), setxid::id(sgid)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setfsuid(uid: u32) -> c_int {
    ret::id_int(setxid::set_fs(nr::SETFSUID, uid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setfsgid(gid: u32) -> c_int {
    ret::id_int(setxid::set_fs(nr::SETFSGID, gid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setgroups(n: usize, list: *const u32) -> c_int {
    // UNSAFE(slice) SAFETY: list holds n group IDs (§17.2).
    ret::zero(setxid::groups(unsafe { conv::array(list.cast(), conv::count_sz(n)) }))
}}


// ------------------------------------------------------------------------------ the user and group databases (§17.3)
// The conventions are in `crate::users` (D-17.3); lookups read the `files` databases (D-15.6).

export! { "GLIBC_2.2.5";
pub extern "C" fn setpwent() {
    users::PASSWD.rewind()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endpwent() {
    users::PASSWD.end()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpwent() -> *mut struct_passwd {
    ret::null(users::PASSWD.next())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpwent_r(out: Option<&mut struct_passwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.3).
    users::PASSWD.next_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetpwent(stream: *mut c_void) -> *mut struct_passwd {
    ret::null(users::PASSWD.stream(conv::addr(stream)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetpwent_r(stream: *mut c_void, out: Option<&mut struct_passwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.3).
    users::PASSWD.stream_r(conv::addr(stream), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpwnam(name: *const c_char) -> *mut struct_passwd {
    // UNSAFE(slice) SAFETY: name is a string (§17.3).
    ret::null(users::pw_by(unsafe { strz::strz(name) }.map(Key::Name)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpwuid(uid: c_uint) -> *mut struct_passwd {
    ret::null(users::pw_by(Ok(Key::Id(uid))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpwnam_r(name: *const c_char, out: Option<&mut struct_passwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§17.3).
    users::pw_by_r(unsafe { strz::strz(name) }.map(Key::Name), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpwuid_r(uid: c_uint, out: Option<&mut struct_passwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.3).
    users::pw_by_r(Ok(Key::Id(uid)), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putpwent(p: Option<&struct_passwd>, stream: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: the record's strings are NULL or strings (§17.3).
    ret::zero(users::putpwent(p, conv::addr(stream), &|a| unsafe { strz::strz_opt(core::ptr::with_exposed_provenance(a)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpw(uid: c_uint, buf: *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for the line, whatever its length (the caller's contract, §17.3 ⚑ HIST).
    users::getpw(uid, |n| unsafe { conv::buf_mut(buf.cast(), n) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setgrent() {
    users::GROUP.rewind()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endgrent() {
    users::GROUP.end()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgrent() -> *mut struct_group {
    ret::null(users::GROUP.next())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgrent_r(out: Option<&mut struct_group>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.3).
    users::GROUP.next_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetgrent(stream: *mut c_void) -> *mut struct_group {
    ret::null(users::GROUP.stream(conv::addr(stream)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetgrent_r(stream: *mut c_void, out: Option<&mut struct_group>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.3).
    users::GROUP.stream_r(conv::addr(stream), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgrnam(name: *const c_char) -> *mut struct_group {
    // UNSAFE(slice) SAFETY: name is a string (§17.3).
    ret::null(users::gr_by(unsafe { strz::strz(name) }.map(Key::Name)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgrgid(gid: c_uint) -> *mut struct_group {
    ret::null(users::gr_by(Ok(Key::Id(gid))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgrnam_r(name: *const c_char, out: Option<&mut struct_group>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§17.3).
    users::gr_by_r(unsafe { strz::strz(name) }.map(Key::Name), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgrgid_r(gid: c_uint, out: Option<&mut struct_group>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.3).
    users::gr_by_r(Ok(Key::Id(gid)), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putgrent(g: Option<&struct_group>, stream: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: the record's strings are NULL or strings, its member list NULL or a NULL-terminated array of strings (§17.3).
    ret::zero(users::putgrent(g, conv::addr(stream), &|a| unsafe { strz::strz_opt(core::ptr::with_exposed_provenance(a)) }, &|a| unsafe { conv::argv(core::ptr::with_exposed_provenance(a)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getgrouplist(user: *const c_char, group: c_uint, groups: *mut c_uint, ngroups: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: user is a string; groups has room for *ngroups GIDs (§17.3).
    users::getgrouplist(unsafe { strz::strz(user) }, group, ngroups, |n| unsafe { conv::array_mut::<u32>(groups.cast(), conv::count_sz(n)) })
}}

// ------------------------------------------------------------------------------ the shadow databases (§17.4)

export! { "GLIBC_2.2.5";
pub extern "C" fn setspent() {
    users::SHADOW.rewind()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endspent() {
    users::SHADOW.end()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getspent() -> *mut struct_spwd {
    ret::null(users::SHADOW.next())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getspent_r(out: Option<&mut struct_spwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.4).
    users::SHADOW.next_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetspent(stream: *mut c_void) -> *mut struct_spwd {
    ret::null(users::SHADOW.stream(conv::addr(stream)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fgetspent_r(stream: *mut c_void, out: Option<&mut struct_spwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.4).
    users::SHADOW.stream_r(conv::addr(stream), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sgetspent(s: *const c_char) -> *mut struct_spwd {
    // UNSAFE(slice) SAFETY: s is a string (§17.4).
    ret::null(users::SHADOW.text(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sgetspent_r(s: *const c_char, out: Option<&mut struct_spwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: s is a string; buf holds buflen bytes (§17.4).
    users::SHADOW.text_r(unsafe { strz::strz(s) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getspnam(name: *const c_char) -> *mut struct_spwd {
    // UNSAFE(slice) SAFETY: name is a string (§17.4).
    ret::null(users::sp_by(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getspnam_r(name: *const c_char, out: Option<&mut struct_spwd>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§17.4).
    users::sp_by_r(unsafe { strz::strz(name) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn putspent(p: Option<&struct_spwd>, stream: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: the record's strings are NULL or strings (§17.4).
    ret::zero(users::putspent(p, conv::addr(stream), &|a| unsafe { strz::strz_opt(core::ptr::with_exposed_provenance(a)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lckpwdf() -> c_int {
    users::lckpwdf()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ulckpwdf() -> c_int {
    users::ulckpwdf()
}}

export! { "GLIBC_2.10";
pub extern "C" fn setsgent() {
    users::GSHADOW.rewind()
}}

export! { "GLIBC_2.10";
pub extern "C" fn endsgent() {
    users::GSHADOW.end()
}}

export! { "GLIBC_2.10";
pub extern "C" fn getsgent() -> *mut struct_sgrp {
    ret::null(users::GSHADOW.next())
}}

export! { "GLIBC_2.10";
pub extern "C" fn getsgent_r(out: Option<&mut struct_sgrp>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.4).
    users::GSHADOW.next_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.10";
pub extern "C" fn fgetsgent(stream: *mut c_void) -> *mut struct_sgrp {
    ret::null(users::GSHADOW.stream(conv::addr(stream)))
}}

export! { "GLIBC_2.10";
pub extern "C" fn fgetsgent_r(stream: *mut c_void, out: Option<&mut struct_sgrp>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§17.4).
    users::GSHADOW.stream_r(conv::addr(stream), out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.10";
pub extern "C" fn sgetsgent(s: *const c_char) -> *mut struct_sgrp {
    // UNSAFE(slice) SAFETY: s is a string (§17.4).
    ret::null(users::GSHADOW.text(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.10";
pub extern "C" fn sgetsgent_r(s: *const c_char, out: Option<&mut struct_sgrp>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: s is a string; buf holds buflen bytes (§17.4).
    users::GSHADOW.text_r(unsafe { strz::strz(s) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.10";
pub extern "C" fn getsgnam(name: *const c_char) -> *mut struct_sgrp {
    // UNSAFE(slice) SAFETY: name is a string (§17.4).
    ret::null(users::sg_by(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.10";
pub extern "C" fn getsgnam_r(name: *const c_char, out: Option<&mut struct_sgrp>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§17.4).
    users::sg_by_r(unsafe { strz::strz(name) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.10";
pub extern "C" fn putsgent(g: Option<&struct_sgrp>, stream: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: the record's strings are NULL or strings, its lists NULL or NULL-terminated arrays of strings (§17.4).
    ret::zero(users::putsgent(g, conv::addr(stream), &|a| unsafe { strz::strz_opt(core::ptr::with_exposed_provenance(a)) }, &|a| unsafe { conv::argv(core::ptr::with_exposed_provenance(a)) }))
}}

// ------------------------------------------------------------------------------ initgroups (§17.2)

export! { "GLIBC_2.2.5";
pub extern "C" fn initgroups(user: *const c_char, group: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: user is a string (§17.2).
    ret::zero(users::initgroups(unsafe { strz::strz(user) }, group))
}}

// ------------------------------------------------------------------------------ login name, terminal session (§17.5)

export! { "GLIBC_2.2.5";
pub extern "C" fn getlogin() -> *mut c_char {
    ret::null(users::getlogin())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getlogin_r(buf: *mut c_char, size: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds size bytes (§17.5).
    users::getlogin_r(unsafe { conv::buf_mut(buf.cast(), size) })
}}

export! { "GLIBC_2.4";
pub extern "C" fn __getlogin_r_chk(buf: *mut c_char, size: size_t, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds size bytes, checked against its object size buflen (C0.5).
    users::getlogin_r(unsafe { conv::buf_mut(buf.cast(), conv::chk(size, buflen)) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setlogin(name: *const c_char) -> c_int {
    ret::zero(users::setlogin(conv::addr(name)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ttyslot() -> c_int {
    users::ttyslot()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpass(prompt: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: prompt is a string (§17.5).
    ret::null(users::getpass(unsafe { strz::strz(prompt) }))
}}

// ------------------------------------------------------------------------------ login shells (§17.5a)

export! { "GLIBC_2.2.5";
pub extern "C" fn getusershell() -> *mut c_char {
    ret::ptr(users::getusershell())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setusershell() {
    users::rewind_shells()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endusershell() {
    users::rewind_shells()
}}

// ------------------------------------------------------------------------------ login accounting (§17.6)
// `struct utmpx` has the layout of `struct utmp` (Appendix C), so the `x` names share the
// translations.

export! { "GLIBC_2.2.5";
pub extern "C" fn setutent() {
    users::setutent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endutent() {
    users::endutent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutent() -> *mut struct_utmp {
    ret::null(users::getutent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutid(key: Option<&struct_utmp>) -> *mut struct_utmp {
    ret::null(users::getutid(key))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutline(key: Option<&struct_utmp>) -> *mut struct_utmp {
    ret::null(users::getutline(key))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pututline(rec: Option<&struct_utmp>) -> *mut struct_utmp {
    ret::null(users::pututline(rec))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn utmpname(file: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: file is a string (§17.6).
    ret::zero(users::utmpname(unsafe { strz::strz(file) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn updwtmp(file: *const c_char, rec: Option<&struct_utmp>) {
    // UNSAFE(slice) SAFETY: file is a string (§17.6).
    ret::errno_only(users::updwtmp(unsafe { strz::strz(file) }, rec))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setutxent() {
    users::setutent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endutxent() {
    users::endutent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutxent() -> *mut struct_utmp {
    ret::null(users::getutent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutxid(key: Option<&struct_utmp>) -> *mut struct_utmp {
    ret::null(users::getutid(key))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutxline(key: Option<&struct_utmp>) -> *mut struct_utmp {
    ret::null(users::getutline(key))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pututxline(rec: Option<&struct_utmp>) -> *mut struct_utmp {
    ret::null(users::pututline(rec))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn utmpxname(file: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: file is a string (§17.6).
    ret::zero(users::utmpname(unsafe { strz::strz(file) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn updwtmpx(file: *const c_char, rec: Option<&struct_utmp>) {
    // UNSAFE(slice) SAFETY: file is a string (§17.6).
    ret::errno_only(users::updwtmp(unsafe { strz::strz(file) }, rec))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutent_r(out: Option<&mut struct_utmp>, result: Option<&mut usize>) -> c_int {
    users::getutent_r(out, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutid_r(key: Option<&struct_utmp>, out: Option<&mut struct_utmp>, result: Option<&mut usize>) -> c_int {
    users::getutid_r(key, out, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutline_r(key: Option<&struct_utmp>, out: Option<&mut struct_utmp>, result: Option<&mut usize>) -> c_int {
    users::getutline_r(key, out, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutmp(ux: Option<&struct_utmp>, u: Option<&mut struct_utmp>) {
    ret::unit(users::copy_utmp(ux, u))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getutmpx(u: Option<&struct_utmp>, ux: Option<&mut struct_utmp>) {
    ret::unit(users::copy_utmp(u, ux))
}}

export! { "GLIBC_2.34";
pub extern "C" fn login(rec: Option<&struct_utmp>) {
    ret::unit(users::login(rec))
}}

export! { "GLIBC_2.34";
pub extern "C" fn logout(line: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: line is a string (§17.6).
    users::logout(unsafe { strz::strz(line) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn logwtmp(line: *const c_char, name: *const c_char, host: *const c_char) {
    // UNSAFE(slice) SAFETY: the three arguments are strings (§17.6).
    ret::errno_only(users::logwtmp(unsafe { strz::strz(line) }, unsafe { strz::strz(name) }, unsafe { strz::strz(host) }))
}}

// ------------------------------------------------------------------------------ login_tty (§19.4)
// Phase 10 for `getlogin`'s needs; the terminal work is `liella::term::login` (phase 11): a
// caller that already leads a session keeps it, and nothing changes when `fd` is no terminal.

export! { "GLIBC_2.34";
pub extern "C" fn login_tty(fd: c_int) -> c_int {
    ret::zero(liella::term::login(&CSys, conv::fd(fd)))
}}
