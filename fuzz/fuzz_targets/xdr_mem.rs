//! XDR over memory (`xdrmem_create` and the filters, spec §29.1, RFC 4506): a program of
//! filters decodes untrusted bytes, then the decoded values are encoded again.
//!
//! Invariants, not only "no crash":
//! * every value decoded encodes again (into a buffer of the same size) at the same stream
//!   positions, and decoding that encoding gives the same values (the round trip);
//! * the filters whose wire form is exact (`int`, `u_int`, `long`, `u_long`, `enum`, `hyper`,
//!   `float`, `double`) re-encode to the very input bytes;
//! * `xdr_bytes`/`xdr_string` never accept a length above `maxsize` (a string read
//!   back as a C string is shorter when the wire held a NUL: the round trip stops before it).
//!
//! Input: the program (one byte per filter, the next byte its argument for `opaque`, `bytes`,
//! `string`) up to a NUL, then the bytes to decode.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only
#![allow(unsafe_code)] // the XDR filters are the C entry points over raw pointers

use core::ffi::{c_char, c_int, c_long, c_uint};
use core::ptr;
use libfuzzer_sys::fuzz_target;
use liella_boundary::posix::s29_legacy as x;
use liella_fuzz::Input;

const ENCODE: c_int = 0;
const DECODE: c_int = 1;
const FREE: c_int = 2;

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Int(c_int),
    UInt(c_uint),
    Long(c_long),
    ULong(u64),
    Short(i16),
    UShort(u16),
    Char(c_char),
    Bool(c_int),
    Enum(c_int),
    Hyper(i64),
    Float(u32),
    Double(u64),
    Opaque(Vec<u8>),
    Bytes(Vec<u8>),
    Str(Vec<u8>),
}

fn stream(buf: &mut [u8], op: c_int) -> x::Xdr {
    let mut s = x::Xdr { x_op: 0, x_ops: ptr::null(), x_public: ptr::null_mut(), x_private: ptr::null_mut(), x_base: ptr::null_mut(), x_handy: 0 };
    x::liella_compat_xdrmem_create(&mut s, buf.as_mut_ptr().cast(), c_uint::try_from(buf.len()).unwrap_or(0), op);
    s
}

fn pos(s: &x::Xdr) -> c_uint {
    // SAFETY: xdrmem_create set x_ops to the memory stream's table.
    let ops = unsafe { &*s.x_ops };
    (ops.x_getpostn.expect("x_getpostn"))(s)
}

/// Runs filter `code` (argument `arg`) in direction `s.x_op`; on encode `v` is the value.
fn filter(s: &mut x::Xdr, code: u8, arg: u8, v: Option<&Val>) -> Option<Val> {
    macro_rules! word {
        ($f:path, $variant:ident, $t:ty) => {{
            let mut val: $t = match v { Some(Val::$variant(a)) => *a, _ => Default::default() };
            ($f(s, &mut val) != 0).then(|| Val::$variant(val))
        }};
    }
    let max = c_uint::from(arg) * 7;
    match code % 15 {
        0 => word!(x::liella_compat_xdr_int, Int, c_int),
        1 => word!(x::liella_compat_xdr_u_int, UInt, c_uint),
        2 => word!(x::liella_compat_xdr_long, Long, c_long),
        3 => word!(x::liella_compat_xdr_u_long, ULong, u64),
        4 => word!(x::liella_compat_xdr_short, Short, i16),
        5 => word!(x::liella_compat_xdr_u_short, UShort, u16),
        6 => word!(x::liella_compat_xdr_char, Char, c_char),
        7 => word!(x::liella_compat_xdr_bool, Bool, c_int),
        8 => word!(x::liella_compat_xdr_enum, Enum, c_int),
        9 => word!(x::liella_compat_xdr_hyper, Hyper, i64),
        10 => {
            let mut f = match v { Some(Val::Float(b)) => f32::from_bits(*b), _ => 0.0 };
            (x::liella_compat_xdr_float(s, &mut f) != 0).then(|| Val::Float(f.to_bits()))
        }
        11 => {
            let mut d = match v { Some(Val::Double(b)) => f64::from_bits(*b), _ => 0.0 };
            (x::liella_compat_xdr_double(s, &mut d) != 0).then(|| Val::Double(d.to_bits()))
        }
        12 => {
            let n = usize::from(arg);
            let mut b = match v { Some(Val::Opaque(b)) => b.clone(), _ => vec![0; n] };
            (x::liella_compat_xdr_opaque(s, b.as_mut_ptr().cast(), c_uint::from(arg)) != 0).then(|| Val::Opaque(b))
        }
        13 => {
            let (mut p, mut n, mut keep) = match v {
                Some(Val::Bytes(b)) => {
                    let mut k = b.clone();
                    (k.as_mut_ptr().cast::<c_char>(), c_uint::try_from(b.len()).unwrap_or(0), Some(k))
                }
                _ => (ptr::null_mut(), 0, None),
            };
            if let Some(k) = keep.as_mut() {
                p = k.as_mut_ptr().cast();
            }
            let ok = x::liella_compat_xdr_bytes(s, &mut p, &mut n, max) != 0;
            if !ok || keep.is_some() {
                return ok.then(|| Val::Bytes(keep.unwrap_or_default()));
            }
            assert!(n <= max, "xdr_bytes accepted {n} bytes over maxsize {max}");
            let out = if n == 0 || p.is_null() { Vec::new() } else {
                // SAFETY: decode stored n fresh bytes at p.
                unsafe { core::slice::from_raw_parts(p.cast::<u8>(), n as usize) }.to_vec()
            };
            let mut fs = x::Xdr { x_op: FREE, ..*s };
            x::liella_compat_xdr_bytes(&mut fs, &mut p, &mut n, max);
            Some(Val::Bytes(out))
        }
        _ => {
            let mut held: Option<Vec<u8>> = match v {
                Some(Val::Str(b)) => Some(b.iter().copied().chain([0]).collect()),
                _ => None,
            };
            let mut p: *mut c_char = held.as_mut().map_or(ptr::null_mut(), |h| h.as_mut_ptr().cast());
            let ok = x::liella_compat_xdr_string(s, &mut p, max) != 0;
            if !ok || held.is_some() {
                return ok.then(|| Val::Str(v.map(|v| if let Val::Str(b) = v { b.clone() } else { Vec::new() }).unwrap_or_default()));
            }
            assert!(!p.is_null(), "xdr_string decode succeeded with no string");
            // SAFETY: decode stored a NUL-terminated string at p.
            let out = unsafe { core::ffi::CStr::from_ptr(p) }.to_bytes().to_vec();
            assert!(out.len() as u64 <= u64::from(max), "xdr_string accepted {} bytes over maxsize {max}", out.len());
            let mut fs = x::Xdr { x_op: FREE, ..*s };
            x::liella_compat_xdr_string(&mut fs, &mut p, max);
            Some(Val::Str(out))
        }
    }
}

fn exact(code: u8) -> bool {
    matches!(code % 15, 0 | 1 | 2 | 3 | 8 | 9 | 10 | 11)
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let prog = input.part();
    let mut wire = input.rest().to_vec();
    if prog.len() > 128 || wire.len() > 8192 {
        return;
    }
    let len = wire.len();

    // Decode the input.
    let mut d = stream(&mut wire, DECODE);
    let mut steps: Vec<(u8, u8, Val, c_uint, c_uint)> = Vec::new();
    let mut i = 0;
    while i < prog.len() {
        let code = prog[i];
        let arg = prog.get(i + 1).copied().unwrap_or(0);
        i += if matches!(code % 15, 12..=14) { 2 } else { 1 };
        let before = pos(&d);
        let Some(v) = filter(&mut d, code, arg, None) else { break };
        let after = pos(&d);
        // A string may hold NUL bytes on the wire; read back as a C string it is shorter than
        // what it consumed, so it cannot encode the same way: the round trip stops before it.
        if let Val::Str(s) = &v
            && (after - before) as usize != 4 + s.len().div_ceil(4) * 4
        {
            break;
        }
        assert!(after >= before && after as usize <= len, "position out of range: {before} -> {after} of {len}");
        assert_eq!((after - before) % 4, 0, "a filter moved by {} bytes", after - before);
        steps.push((code, arg, v, before, after));
    }

    // Encode the values again.
    let mut again = vec![0u8; len];
    let mut e = stream(&mut again, ENCODE);
    for (code, arg, v, before, after) in &steps {
        assert_eq!(pos(&e), *before, "encode position differs before {v:?}");
        assert!(filter(&mut e, *code, *arg, Some(v)).is_some(), "the decoded {v:?} does not encode");
        assert_eq!(pos(&e), *after, "encode position differs after {v:?}");
    }
    for (code, _, v, before, after) in &steps {
        if exact(*code) {
            let r = *before as usize..*after as usize;
            assert_eq!(&again[r.clone()], &wire[r], "{v:?} re-encodes to other bytes");
        }
    }

    // Decode the encoding: the same values.
    let mut d2 = stream(&mut again, DECODE);
    for (code, arg, v, _, _) in &steps {
        let w = filter(&mut d2, *code, *arg, None);
        assert_eq!(w.as_ref(), Some(v), "the round trip changed a value");
    }
});
