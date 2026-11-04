//! The per-architecture login record (`liella::users::UtmpShape`, ARCH-UTMP-LAYOUT): the shapes
//! agree with the probed layout tables, and a record encodes and decodes in either shape on any
//! host. Spec clause: §17.6 (utmp and wtmp records); the aarch64 numbers are the compile-only
//! probes in `spec/abi/layouts-aarch64.tsv` (no spec clause for the aarch64 shape itself).

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::users::{Utmp, UtmpShape, UT_SEC_MAX_32};

/// `(size, offset of member)` from a layout table: the `struct utmp` rows.
fn table(name: &str) -> (usize, impl Fn(&str) -> usize) {
    let path = format!("{}/../../spec/abi/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let rows: Vec<Vec<String>> = text.lines().filter(|l| l.starts_with("struct utmp\t")).map(|l| l.split('\t').map(str::to_string).collect()).collect();
    let size = rows.iter().find(|r| r[1].is_empty()).map(|r| r[2].parse().unwrap()).unwrap();
    let off = move |m: &str| -> usize { rows.iter().find(|r| r[1] == m).map(|r| r[2].parse().unwrap()).unwrap() };
    (size, off)
}

fn check(shape: &UtmpShape, file: &str) {
    let (size, off) = table(file);
    assert_eq!(shape.size, size, "{file}: sizeof(struct utmp)");
    assert_eq!(shape.session_at, off("ut_session"), "{file}: ut_session");
    assert_eq!(shape.sec_at, off("ut_tv.tv_sec"), "{file}: ut_tv.tv_sec");
    assert_eq!(shape.usec_at, off("ut_tv.tv_usec"), "{file}: ut_tv.tv_usec");
    // Widths: each member runs to the next (ut_session to ut_tv, tv_sec to tv_usec, tv_usec to ut_addr_v6).
    assert_eq!(shape.session_len, off("ut_tv") - off("ut_session"), "{file}: ut_session width");
    assert_eq!(shape.sec_len, off("ut_tv.tv_usec") - off("ut_tv.tv_sec"), "{file}: tv_sec width");
    assert_eq!(shape.usec_len, off("ut_addr_v6") - off("ut_tv.tv_usec"), "{file}: tv_usec width");
    assert_eq!(shape.addr_at, off("ut_addr_v6"), "{file}: ut_addr_v6");
    for (m, at) in [("ut_type", 0), ("ut_pid", 4), ("ut_line", 8), ("ut_id", 40), ("ut_user", 44), ("ut_host", 76), ("ut_exit", 332)] {
        assert_eq!(off(m), at, "{file}: {m} is shared by both shapes");
    }
}

#[test]
fn shapes_match_the_layout_tables() {
    // no spec clause: ARCH-UTMP-LAYOUT, the record layouts of spec/abi/layouts*.tsv.
    check(&UtmpShape::X86_64, "layouts.tsv");
    check(&UtmpShape::AARCH64, "layouts-aarch64.tsv");
}

fn sample() -> Utmp {
    let mut u = Utmp { kind: 7, pid: 4242, exit: [3, 4], session: 99, time: (1_700_000_000, 123_456), addr: [1, 2, 3, 4], ..Utmp::default() };
    liella::users::set_field(&mut u.line, b"pts/7");
    liella::users::set_field(&mut u.id, b"ts/7");
    liella::users::set_field(&mut u.user, b"mallory");
    liella::users::set_field(&mut u.host, b"attacker.example");
    u
}

#[test]
fn aarch64_record_encodes_at_the_probed_offsets_and_round_trips() {
    // no spec clause: ARCH-UTMP-LAYOUT, the record layouts of spec/abi/layouts*.tsv.
    let (size, off) = table("layouts-aarch64.tsv");
    let mut u = sample();
    // Values the 32-bit record cannot hold: 64-bit seconds past 2106, a wide session and microseconds.
    u.session = 0x1_2345_6789;
    u.time = (UT_SEC_MAX_32 + 12_345, 0x2_0000_0001);
    let mut b = vec![0u8; size];
    u.encode(&UtmpShape::AARCH64, &mut b);
    let at = |m: &str| off(m);
    assert_eq!(&b[at("ut_line")..at("ut_line") + 5], b"pts/7");
    assert_eq!(&b[at("ut_host")..at("ut_host") + 16], b"attacker.example");
    assert_eq!(i16::from_ne_bytes(b[at("ut_exit.e_termination")..][..2].try_into().unwrap()), 3);
    assert_eq!(i16::from_ne_bytes(b[at("ut_exit.e_exit")..][..2].try_into().unwrap()), 4);
    assert_eq!(i64::from_ne_bytes(b[at("ut_session")..][..8].try_into().unwrap()), 0x1_2345_6789);
    assert_eq!(i64::from_ne_bytes(b[at("ut_tv.tv_sec")..][..8].try_into().unwrap()), UT_SEC_MAX_32 + 12_345);
    assert_eq!(i64::from_ne_bytes(b[at("ut_tv.tv_usec")..][..8].try_into().unwrap()), 0x2_0000_0001);
    assert_eq!(i32::from_ne_bytes(b[at("ut_addr_v6")..][..4].try_into().unwrap()), 1);
    assert_eq!(i32::from_ne_bytes(b[at("ut_addr_v6") + 12..][..4].try_into().unwrap()), 4);
    assert!(b[at("__glibc_reserved")..].iter().all(|&c| c == 0), "reserved tail zero");
    assert_eq!(Utmp::decode(&UtmpShape::AARCH64, &b), u, "nothing is clamped");
}

#[test]
fn x86_64_record_is_unchanged_and_never_wraps() {
    // no spec clause: ARCH-UTMP-LAYOUT, the record layouts of spec/abi/layouts*.tsv.
    let u = sample();
    let mut b = vec![0u8; 384];
    u.encode(&UtmpShape::X86_64, &mut b);
    assert_eq!(i32::from_ne_bytes(b[336..340].try_into().unwrap()), 99);
    assert_eq!(u32::from_ne_bytes(b[340..344].try_into().unwrap()), 1_700_000_000);
    assert_eq!(i32::from_ne_bytes(b[344..348].try_into().unwrap()), 123_456);
    assert_eq!(i32::from_ne_bytes(b[348..352].try_into().unwrap()), 1);
    assert_eq!(Utmp::decode(&UtmpShape::X86_64, &b), u);
    // Unsigned seconds to 2106 are kept; a wider time or session is stored as nothing, not wrapped.
    let late = Utmp { time: (UT_SEC_MAX_32, 5), ..u };
    late.encode(&UtmpShape::X86_64, &mut b);
    assert_eq!(Utmp::decode(&UtmpShape::X86_64, &b).time, (UT_SEC_MAX_32, 5));
    let wide = Utmp { time: (UT_SEC_MAX_32 + 1, 5), session: 1 << 40, ..u };
    wide.encode(&UtmpShape::X86_64, &mut b);
    let back = Utmp::decode(&UtmpShape::X86_64, &b);
    assert_eq!((back.time, back.session), ((0, 0), 0));
}

#[test]
fn native_codec_is_the_native_shape() {
    // no spec clause: ARCH-UTMP-LAYOUT, the record layouts of spec/abi/layouts*.tsv.
    let u = sample();
    let mut b = vec![0u8; UtmpShape::NATIVE.size];
    u.encode(&UtmpShape::NATIVE, &mut b);
    assert_eq!(b, u.to_bytes().to_vec());
    assert_eq!(liella::users::UTMP_SIZE, UtmpShape::NATIVE.size);
}
