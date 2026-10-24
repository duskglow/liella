//! Backtrace unwinder (IP-27; spec §2.7; D-2.7) through its public interface only: `trace`,
//! `caller`, `Object`, `Image`, `Regs`, `Memory` and the program-header helpers. Hand-written
//! (an internal primitive has no spec clause of its own); `.eh_frame` and `.eh_frame_hdr`
//! bytes are built here from the LSB's record layout and DWARF 5 §6.4 / §2.5, so the tests do
//! not depend on how the unwinder is written (COLD-REWRITE2-W-far-03 wrote them before the
//! rewrite and kept them unchanged across it).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use std::collections::HashMap;

use liella_rt::unwind::{self, Image, Memory, Object, Regs, RA, RBP, RBX, RSP};

/// Memory as a map of words.
struct Words(HashMap<u64, u64>);

impl Memory for Words {
    fn word(&self, addr: u64) -> Option<u64> {
        self.0.get(&addr).copied()
    }
}

fn words(pairs: &[(u64, u64)]) -> Words {
    Words(pairs.iter().copied().collect())
}

fn uleb(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// An `.eh_frame` section being built at address `base`.
struct Eh {
    base: usize,
    out: Vec<u8>,
}

impl Eh {
    fn new(base: usize) -> Self {
        Eh { base, out: Vec::new() }
    }

    fn at(&self) -> usize {
        self.base + self.out.len()
    }

    /// A record (32-bit length) whose body is padded with `DW_CFA_nop` to a multiple of 8.
    fn record(&mut self, mut body: Vec<u8>) -> usize {
        let at = self.at();
        while (body.len() + 4) % 8 != 0 {
            body.push(0);
        }
        self.out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        self.out.extend_from_slice(&body);
        at
    }

    /// A CIE: id 0, then `rest` (version onward).
    fn cie(&mut self, rest: &[u8]) -> usize {
        let mut body = vec![0, 0, 0, 0];
        body.extend_from_slice(rest);
        self.record(body)
    }

    /// The usual CIE: version 1, "zR" (or "zRS"), code factor 1, data factor -8, return
    /// address column 16, FDE pointers pcrel sdata4; CFA = rsp + 8, return address at CFA - 8.
    fn std_cie(&mut self, signal: bool) -> usize {
        let mut rest = vec![1];
        rest.extend_from_slice(if signal { b"zRS\0" } else { b"zR\0" });
        rest.extend_from_slice(&[1, 0x78, 16, 1, 0x1b, 0x0c, 7, 8, 0x90, 1]);
        self.cie(&rest)
    }

    /// An FDE of the CIE at `cie`; `tail(field)` gives what follows the CIE pointer, where
    /// `field` is the address of the first byte after it.
    fn fde(&mut self, cie: usize, tail: impl Fn(usize) -> Vec<u8>) -> usize {
        let at = self.at();
        let mut body = ((at + 4 - cie) as u32).to_le_bytes().to_vec();
        body.extend(tail(at + 8));
        self.record(body)
    }

    /// An FDE for the usual CIE: [begin, begin + len), no augmentation data, `insns`.
    fn fde4(&mut self, cie: usize, begin: usize, len: usize, insns: &[u8]) -> usize {
        self.fde(cie, |field| {
            let mut t = ((begin as i64 - field as i64) as i32).to_le_bytes().to_vec();
            t.extend_from_slice(&(len as u32).to_le_bytes());
            t.push(0);
            t.extend_from_slice(insns);
            t
        })
    }

    /// The bytes with the zero terminator.
    fn finish(mut self) -> Vec<u8> {
        self.out.extend_from_slice(&[0, 0, 0, 0]);
        self.out
    }
}

/// Where the test `.eh_frame` sections live.
const EH: usize = 0x10_0000;
/// Code: 0x1000 to 0x9000 is described; 0x9000 and up is in no object.
static CODE: [u8; 0x8000] = [0; 0x8000];

/// An object over `frames` (at `EH`, scanned without a header) and the code range.
fn object(frames: &[u8]) -> Object<'_> {
    let mut o = Object::new();
    assert!(o.add_segment(Image { addr: EH, bytes: frames }));
    assert!(o.add_segment(Image { addr: 0x1000, bytes: &CODE }));
    o.set_frames(EH, EH + frames.len());
    o
}

fn regs(set: &[(usize, u64)]) -> Regs {
    let mut r = Regs::new();
    for &(reg, v) in set {
        r.set(reg, Some(v));
    }
    r
}

fn run(objects: &[Object<'_>], mem: &Words, start: Regs, entry: usize, max: usize) -> Vec<usize> {
    let mut got = Vec::new();
    let n = unwind::trace(objects, mem, start, entry, max, |pc| got.push(pc));
    assert_eq!(n, got.len(), "the count is the number of addresses stored");
    assert!(n <= max);
    got
}

/// f at 0x1000 (FDE `f_insns`) returns to g at 0x2000, whose CFA is `g_reg` + 8 and whose
/// return address is at that register's value; g returns into h at 0x3000, the outermost
/// frame (return address undefined). A correct row for f therefore traces [0x2010, 0x3010].
fn through_f(f_cie: Option<&[u8]>, f_insns: &[u8], g_reg: u8, start: Regs, mem: &[(u64, u64)]) -> Vec<usize> {
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    let fc = f_cie.map_or(c, |rest| eh.cie(rest));
    eh.fde4(fc, 0x1000, 0x100, f_insns);
    eh.fde4(c, 0x2000, 0x100, &[0x0c, g_reg, 8]);
    eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
    let frames = eh.finish();
    run(&[object(&frames)], &words(mem), start, 0, 16)
}

/// Starting in f at 0x1010 with rsp 0x7000 and the return address 0x2010 at 0x7000 (the
/// usual CIE's row), with rbx = 0x7800 holding g's return address.
fn plain_start() -> (Regs, Vec<(u64, u64)>) {
    (regs(&[(RA, 0x1010), (RSP, 0x7000), (RBX, 0x7800)]), vec![(0x7000, 0x2010), (0x7800, 0x3010)])
}

#[test]
fn ip27_frame_pointer_chain() {
    // Trace: s2_7_c_1, s2_7_f_2 (frames without unwind information); IP-27, D-2.7.
    // push %rbp (+1: CFA rsp+16, rbp at CFA-16); mov %rsp,%rbp (+4: CFA rbp+16).
    let insns = [0x41, 0x0e, 16, 0x86, 2, 0x43, 0x0d, 6];
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    eh.fde4(c, 0x1000, 0x100, &insns);
    eh.fde4(c, 0x2000, 0x100, &insns);
    eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
    let frames = eh.finish();
    let mem = words(&[(0x7100, 0x7200), (0x7108, 0x2010), (0x7200, 0x7300), (0x7208, 0x3010)]);
    let start = regs(&[(RA, 0x1010), (RSP, 0x7000), (RBP, 0x7100)]);
    assert_eq!(run(&[object(&frames)], &mem, start, 0, 16), [0x2010, 0x3010]);
    // At 0x1001 (looked up at 0x1000) the CIE's row holds: the return address is at rsp.
    // rbp is still g's frame pointer, so g's return address is at 0x7108.
    let mem = words(&[(0x7000, 0x2010), (0x7100, 0x7200), (0x7108, 0x3010), (0x7208, 0x5555)]);
    let start = regs(&[(RA, 0x1001), (RSP, 0x7000), (RBP, 0x7100)]);
    assert_eq!(run(&[object(&frames)], &mem, start, 0, 16), [0x2010, 0x3010]);
}

#[test]
fn ip27_advance_encodings_and_code_factor() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    // advance_loc1 0x10: CFA rsp+16; advance_loc2 0x2f0 (to +0x300): rsp+24; advance_loc4
    // 0x7000 (to +0x7300): rsp+32.
    let insns = [0x02, 0x10, 0x0e, 16, 0x03, 0xf0, 0x02, 0x0e, 24, 0x04, 0, 0x70, 0, 0, 0x0e, 32];
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    eh.fde4(c, 0x1000, 0x7f00, &insns);
    // Code factor 4: advance_loc 1 moves 4 bytes.
    let c4 = eh.cie(&[1, b'z', b'R', 0, 4, 0x78, 16, 1, 0x1b, 0x0c, 7, 8, 0x90, 1]);
    eh.fde4(c4, 0x8f00, 0x100, &[0x41, 0x0e, 16]);
    let frames = eh.finish();
    let o = object(&frames);
    let first = |pc: u64| {
        let mem = words(&[(0x7000, 0x9000), (0x7008, 0x9008), (0x7010, 0x9010), (0x7018, 0x9018)]);
        run(&[o], &mem, regs(&[(RA, pc), (RSP, 0x7000)]), 0, 1)
    };
    assert_eq!(first(0x1010), [0x9000]);
    assert_eq!(first(0x1011), [0x9008]);
    assert_eq!(first(0x1300), [0x9008]);
    assert_eq!(first(0x1301), [0x9010]);
    assert_eq!(first(0x8301), [0x9018]);
    assert_eq!(first(0x8f04), [0x9000]);
    assert_eq!(first(0x8f05), [0x9008]);
}

#[test]
fn ip27_cfa_rules() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    let (start, mem) = plain_start();
    // def_cfa_sf rsp, -2 (x -8 = 16); def_cfa_offset_sf -1 (8).
    let mut m = mem.clone();
    m.push((0x7008, 0x2010));
    assert_eq!(through_f(None, &[0x12, 7, 0x7e], 3, start, &m), [0x2010, 0x3010]);
    assert_eq!(through_f(None, &[0x12, 7, 0x7e, 0x13, 0x7f], 3, start, &mem), [0x2010, 0x3010]);
    // def_cfa_register rbx then def_cfa_offset: CFA = rbx + 0x10 (return address at 0x7808).
    let m = vec![(0x7808, 0x2010), (0x7800, 0x3010)];
    assert_eq!(through_f(None, &[0x0d, 3, 0x0e, 0x10], 3, start, &m), [0x2010, 0x3010]);
    // def_cfa_expression: CFA = rsp + 8.
    assert_eq!(through_f(None, &[0x0f, 2, 0x77, 8], 3, start, &mem), [0x2010, 0x3010]);
}

#[test]
fn ip27_register_rules() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    let (start, mem) = plain_start();
    let ok = [0x2010, 0x3010];
    // offset rbx at CFA-16 (0x6ff8 holds g's frame), and the same with offset_extended and
    // offset_extended_sf; GNU_negative_offset_extended 2 is CFA+16 (0x7018).
    let mut m = mem.clone();
    m.extend([(0x6ff8, 0x7a00), (0x7a00, 0x3010), (0x7018, 0x7a00)]);
    assert_eq!(through_f(None, &[0x83, 2], 3, start, &m), ok);
    assert_eq!(through_f(None, &[0x05, 3, 2], 3, start, &m), ok);
    assert_eq!(through_f(None, &[0x11, 3, 2], 3, start, &m), ok);
    assert_eq!(through_f(None, &[0x2f, 3, 2], 3, start, &m), ok);
    // restore and restore_extended bring back the CIE's rule (rbx unchanged: 0x7800).
    assert_eq!(through_f(None, &[0x83, 2, 0xc3], 3, start, &mem), ok);
    assert_eq!(through_f(None, &[0x83, 2, 0x06, 3], 3, start, &mem), ok);
    // same_value, nop and GNU_args_size change nothing.
    assert_eq!(through_f(None, &[0x08, 3, 0x00, 0x2e, 16], 3, start, &mem), ok);
    // register: rbx's value is in rbp.
    let both = regs(&[(RA, 0x1010), (RSP, 0x7000), (RBP, 0x7800), (RBX, 1)]);
    assert_eq!(through_f(None, &[0x09, 3, 6], 3, both, &mem), ok);
    // val_offset: rbx = CFA + 16 x -8 (0x6f88).
    let s = regs(&[(RA, 0x1010), (RSP, 0x7000)]);
    let mut low = mem.clone();
    low.push((0x6f88, 0x3010));
    assert_eq!(through_f(None, &[0x14, 3, 0x10], 3, s, &low), ok);
    // val_offset_sf: rbx = CFA + (-255 x -8) = 0x7008 + 0x7f8 = 0x7800.
    assert_eq!(through_f(None, &[0x15, 3, 0x81, 0x7e], 3, s, &mem), ok);
    // expression: rbx at the address CFA - 16; val_expression: rbx = CFA + 0x7f8.
    assert_eq!(through_f(None, &[0x10, 3, 3, 0x08, 16, 0x1c], 3, start, &m), ok);
    assert_eq!(through_f(None, &[0x16, 3, 3, 0x23, 0xf8, 0x0f], 3, s, &mem), ok);
    // The return address in a register (rbp), and undefined: the trace ends at once.
    let in_rbp = regs(&[(RA, 0x1010), (RSP, 0x7000), (RBP, 0x2010), (RBX, 0x7800)]);
    assert_eq!(through_f(None, &[0x09, 16, 6], 3, in_rbp, &mem), ok);
    assert_eq!(through_f(None, &[0x07, 16], 3, start, &mem), Vec::<usize>::new());
    // The stack pointer is the CFA unless a rule names it: g's CFA is rsp + 8 = 0x7010.
    let m = vec![(0x7000, 0x2010), (0x7008, 0x3010)];
    assert_eq!(through_f(None, &[], 7, start, &m), ok);
    // remember_state / restore_state: +1 CFA rsp+16, +2 remember, CFA rsp+8, +3 restore.
    let insns = [0x41, 0x0e, 16, 0x41, 0x0a, 0x0e, 8, 0x41, 0x0b];
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    eh.fde4(c, 0x1000, 0x40, &insns);
    let frames = eh.finish();
    let o = object(&frames);
    let mem = words(&[(0x7000, 0x9001), (0x7008, 0x9002)]);
    let first = |pc: u64| run(&[o], &mem, regs(&[(RA, pc), (RSP, 0x7000)]), 0, 1);
    assert_eq!(first(0x1002), [0x9002]);
    assert_eq!(first(0x1003), [0x9001]);
    assert_eq!(first(0x1004), [0x9002]);
}

#[test]
fn ip27_expressions() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    let (start, mem) = plain_start();
    let mut m = mem.clone();
    m.extend([(0x6000, 0x7008), (0x6100, 0x1_0000_7008)]);
    // Each computes the CFA, rsp + 8 (with breg7 0 / plus where it computes the 8 only).
    let eight = |e: &[u8]| {
        let mut v = e.to_vec();
        v.extend_from_slice(&[0x77, 0, 0x22]);
        v
    };
    let cases: Vec<Vec<u8>> = vec![
        vec![0x77, 8],
        vec![0x92, 7, 8],
        eight(&[0x38]),
        eight(&[0x08, 8]),
        eight(&[0x09, 0xf8, 0x1f]),
        eight(&[0x0a, 8, 0]),
        eight(&[0x0b, 8, 0]),
        eight(&[0x0c, 8, 0, 0, 0]),
        eight(&[0x0d, 8, 0, 0, 0]),
        eight(&[0x0e, 8, 0, 0, 0, 0, 0, 0, 0]),
        eight(&[0x0f, 8, 0, 0, 0, 0, 0, 0, 0]),
        eight(&[0x10, 8]),
        eight(&[0x11, 8]),
        eight(&[0x32, 0x34, 0x1e]),
        eight(&[0x40, 0x32, 0x1b]),
        eight(&[0x09, 0xf0, 0x09, 0xfe, 0x1b]),
        eight(&[0x4a, 0x39, 0x1d]),
        eight(&[0x31, 0x33, 0x24]),
        eight(&[0x40, 0x31, 0x25]),
        eight(&[0x09, 0xf0, 0x31, 0x26, 0x1f]),
        eight(&[0x09, 0xf8, 0x19]),
        eight(&[0x3c, 0x34, 0x27]),
        eight(&[0x38, 0x30, 0x21]),
        eight(&[0x48, 0x3f, 0x1a]),
        eight(&[0x09, 0xf7, 0x20]),
        eight(&[0x3a, 0x32, 0x1c]),
        vec![0x38, 0x77, 0, 0x16, 0x22],
        vec![0x38, 0x77, 0, 0x14, 0x13, 0x22],
        vec![0x38, 0x77, 0, 0x15, 1, 0x22, 0x16, 0x13],
        vec![0x77, 0, 0x30, 0x38, 0x17, 0x22, 0x22],
        vec![0x77, 0, 0x12, 0x1c, 0x77, 8, 0x22],
        vec![0x0c, 0, 0x60, 0, 0, 0x06],
        vec![0x0c, 0, 0x61, 0, 0, 0x94, 4],
        vec![0x2f, 2, 0, 0x30, 0x30, 0x96, 0x77, 8],
        vec![0x31, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
        vec![0x30, 0x28, 1, 0, 0x30, 0x77, 8],
        vec![0x38, 0x38, 0x29, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
        vec![0x38, 0x39, 0x2e, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
        vec![0x39, 0x38, 0x2b, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
        vec![0x38, 0x38, 0x2a, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
        vec![0x38, 0x39, 0x2c, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
        vec![0x38, 0x38, 0x2d, 0x30, 0x29, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
        vec![0x09, 0xff, 0x31, 0x2d, 0x28, 2, 0, 0x30, 0x30, 0x77, 8],
    ];
    for (i, e) in cases.iter().enumerate() {
        let mut insns = vec![0x0f];
        uleb(e.len() as u64, &mut insns);
        insns.extend_from_slice(e);
        assert_eq!(through_f(None, &insns, 3, start, &m), [0x2010, 0x3010], "expression {i}: {e:x?}");
    }
    // Expressions that cannot be evaluated end the trace: an unknown operation, an empty
    // program, a stack underflow, unreadable memory, division by zero, an endless loop, a
    // branch out of the program, a register with no value, a truncated operand.
    let bad: [&[u8]; 10] =
        [&[0xe0], &[], &[0x22], &[0x30, 0x06], &[0x31, 0x30, 0x1b], &[0x2f, 0xfd, 0xff], &[0x77, 8, 0x2f, 0x40, 0], &[0x7f, 0], &[0x0e, 1, 2], &[0x31, 0x30, 0x1d]];
    for e in bad {
        let mut insns = vec![0x0f];
        uleb(e.len() as u64, &mut insns);
        insns.extend_from_slice(e);
        assert_eq!(through_f(None, &insns, 3, start, &m), Vec::<usize>::new(), "expression {e:x?}");
    }
}

#[test]
fn ip27_cie_versions_augmentations_and_encodings() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    let (start, mem) = plain_start();
    let ok = [0x2010, 0x3010];
    // Version 3 (return-address column as a ULEB).
    let v3 = [3, b'z', b'R', 0, 1, 0x78, 16, 1, 0x1b, 0x0c, 7, 8, 0x90, 1];
    assert_eq!(through_f(Some(&v3), &[], 3, start, &mem), ok);
    // "zPLR": a personality (indirect pcrel sdata4) and an LSDA pointer in each FDE.
    let mut zplr = vec![1, b'z', b'P', b'L', b'R', 0, 1, 0x78, 16, 7, 0x9b, 0, 0, 0, 0, 0x1b, 0x1b];
    zplr.extend_from_slice(&[0x0c, 7, 8, 0x90, 1]);
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    let p = eh.cie(&zplr);
    eh.fde(p, |field| {
        let mut t = ((0x1000 - field as i64) as i32).to_le_bytes().to_vec();
        t.extend_from_slice(&0x100u32.to_le_bytes());
        t.push(4);
        t.extend_from_slice(&0x1234u32.to_le_bytes());
        t
    });
    eh.fde4(c, 0x2000, 0x100, &[0x0c, 3, 8]);
    eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
    let frames = eh.finish();
    assert_eq!(run(&[object(&frames)], &words(&mem), start, 0, 16), ok);
    // FDE address encodings: absptr, udata4, sdata4, pcrel sdata8, pcrel absptr, udata8.
    for enc in [0x00u8, 0x03, 0x0b, 0x1c, 0x10, 0x04] {
        let mut eh = Eh::new(EH);
        let c = eh.std_cie(false);
        let e = eh.cie(&[1, b'z', b'R', 0, 1, 0x78, 16, 1, enc, 0x0c, 7, 8, 0x90, 1]);
        eh.fde(e, |field| {
            let pc = if enc & 0x70 == 0x10 { 0x1000i64 - field as i64 } else { 0x1000 };
            let mut t = match enc & 0x0f {
                0x03 | 0x0b => (pc as u32).to_le_bytes().to_vec(),
                _ => (pc as u64).to_le_bytes().to_vec(),
            };
            t.extend(match enc & 0x0f {
                0x03 | 0x0b => 0x100u32.to_le_bytes().to_vec(),
                _ => 0x100u64.to_le_bytes().to_vec(),
            });
            t.push(0);
            t
        });
        eh.fde4(c, 0x2000, 0x100, &[0x0c, 3, 8]);
        eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
        let frames = eh.finish();
        assert_eq!(run(&[object(&frames)], &words(&mem), start, 0, 16), ok, "encoding {enc:#x}");
    }
}

/// `.eh_frame_hdr` at `EH` followed by `.eh_frame` at `EH + 0x100` with FDEs for 0x1000,
/// 0x1020 and 0x2000; the table is datarel sdata4 (the form linkers write).
fn with_header(table: bool) -> Vec<u8> {
    let mut eh = Eh::new(EH + 0x100);
    let c = eh.std_cie(false);
    let fdes = [(0x1000usize, eh.fde4(c, 0x1000, 0x20, &[])), (0x1020, eh.fde4(c, 0x1020, 0x10, &[0x0e, 16])), (0x2000, eh.fde4(c, 0x2000, 4, &[]))];
    let frames = eh.finish();
    let mut hdr = vec![1, 0x1b, if table { 0x03 } else { 0xff }, if table { 0x3b } else { 0xff }];
    hdr.extend_from_slice(&(0x100i32 - 4).to_le_bytes());
    if table {
        hdr.extend_from_slice(&3u32.to_le_bytes());
        for (loc, fde) in fdes {
            hdr.extend_from_slice(&((loc as i64 - EH as i64) as i32).to_le_bytes());
            hdr.extend_from_slice(&((fde - EH) as i32).to_le_bytes());
        }
    }
    hdr.resize(0x100, 0);
    hdr.extend_from_slice(&frames);
    hdr
}

#[test]
fn ip27_finds_fdes_through_the_header_and_by_scanning() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    let mem = words(&[(0x7000, 0x9000), (0x7008, 0x9008)]);
    for table in [true, false] {
        let bytes = with_header(table);
        let mut o = Object::new();
        assert!(o.add_segment(Image { addr: EH, bytes: &bytes }));
        assert!(o.add_segment(Image { addr: 0x1000, bytes: &CODE }));
        assert!(!o.has_cfi());
        o.set_header(EH);
        assert!(o.has_cfi());
        let first = |pc: u64| run(&[o], &mem, regs(&[(RA, pc), (RSP, 0x7000)]), 0, 1);
        assert_eq!(first(0x1001), [0x9000], "table {table}");
        assert_eq!(first(0x1020), [0x9000], "table {table}");
        assert_eq!(first(0x1021), [0x9008], "table {table}");
        assert_eq!(first(0x1030), [0x9008], "table {table}");
        assert_eq!(first(0x2004), [0x9000], "table {table}");
        // No FDE covers 0x1030 (looked up), 0x0fff or 0x2004: the trace ends there.
        assert!(first(0x1031).is_empty(), "table {table}");
        assert!(first(0x2005).is_empty(), "table {table}");
    }
    // A header of a version other than 1 is not read: nothing is found through it.
    let mut bytes = with_header(true);
    if let Some(v) = bytes.first_mut() {
        *v = 2;
    }
    let mut o = Object::new();
    o.add_segment(Image { addr: EH, bytes: &bytes });
    o.add_segment(Image { addr: 0x1000, bytes: &CODE });
    o.set_header(EH);
    assert!(run(&[o], &mem, regs(&[(RA, 0x1001), (RSP, 0x7000)]), 0, 1).is_empty());
    // An object without call-frame information describes nothing.
    let mut o = Object::new();
    o.add_segment(Image { addr: 0x1000, bytes: &CODE });
    assert!(run(&[o], &mem, regs(&[(RA, 0x1001), (RSP, 0x7000)]), 0, 1).is_empty());
}

#[test]
fn ip27_signal_frames() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    // h (0x4000) returns to the restorer at 0x5001, whose FDE (CIE with `S`) starts at the nop
    // before it: CFA = rsp, rip at CFA + 168, rsp at CFA + 160, rbx at CFA + 128 (what the
    // kernel's ucontext_t holds). The interrupted g is at 0x6000, the first byte of its FDE:
    // it must be looked up at the address itself, not one byte before (where e's FDE ends with
    // a row that would read the wrong return address).
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    let s = eh.std_cie(true);
    eh.fde4(c, 0x4000, 0x100, &[]);
    eh.fde4(s, 0x5000, 0x10, &[0x0c, 7, 0, 0x11, 16, 0x6b, 0x11, 7, 0x6c, 0x11, 3, 0x70]);
    eh.fde4(c, 0x5f00, 0x100, &[0x0e, 0x40]);
    eh.fde4(c, 0x6000, 0x100, &[0x0c, 3, 8]);
    let frames = eh.finish();
    let o = object(&frames);
    let uc = 0x7008u64;
    let mem = words(&[(0x7000, 0x5001), (uc + 168, 0x6000), (uc + 160, 0x8000), (uc + 128, 0x8800), (0x8800, 0x9010), (0x8038, 0x5555)]);
    let start = regs(&[(RA, 0x4005), (RSP, 0x7000)]);
    assert_eq!(run(&[o], &mem, start, 0, 16), [0x5001, 0x6000, 0x9010]);
    // Signal frames nest: the interrupted function is itself the restorer's caller again.
    let mem = words(&[
        (0x7000, 0x5001),
        (uc + 168, 0x4010),
        (uc + 160, 0x7400),
        (0x7400, 0x5001),
        (0x7408 + 168, 0x6000),
        (0x7408 + 160, 0x8000),
        (0x7408 + 128, 0x8800),
        (0x8800, 0x9010),
    ]);
    assert_eq!(run(&[o], &mem, start, 0, 16), [0x5001, 0x4010, 0x5001, 0x6000, 0x9010]);
}

#[test]
fn ip27_trace_ends_limits_and_entry() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    for f in 0..4 {
        eh.fde4(c, 0x1000 + f * 0x10, 0x10, &[]);
    }
    let frames = eh.finish();
    let o = object(&frames);
    let start = regs(&[(RA, 0x1005), (RSP, 0x7000)]);
    let chain = |last: u64| words(&[(0x7000, 0x1015), (0x7008, 0x1025), (0x7010, 0x1035), (0x7018, last)]);
    // A zero return address is the end; it is not reported.
    assert_eq!(run(&[o], &chain(0), start, 0, 8), [0x1015, 0x1025, 0x1035]);
    // An address no object describes is reported, then the trace ends.
    assert_eq!(run(&[o], &chain(0x9_0000), start, 0, 8), [0x1015, 0x1025, 0x1035, 0x9_0000]);
    // An unreadable stack ends the trace.
    let m = words(&[(0x7000, 0x1015), (0x7008, 0x1025)]);
    assert_eq!(run(&[o], &m, start, 0, 8), [0x1015, 0x1025]);
    // An unknown stack pointer ends it before anything is read.
    assert!(run(&[o], &chain(0), regs(&[(RA, 0x1005)]), 0, 8).is_empty());
    // max limits the addresses stored; 0 stores none.
    assert_eq!(run(&[o], &chain(0), start, 0, 2), [0x1015, 0x1025]);
    assert!(run(&[o], &chain(0), start, 0, 0).is_empty());
    // The entry point's own frame (identified by its FDE start) is left out when it is the
    // first one reported.
    assert_eq!(run(&[o], &chain(0), start, 0x1010, 8), [0x1025, 0x1035]);
    assert_eq!(run(&[o], &chain(0), start, 0x1020, 8), [0x1015, 0x1025, 0x1035]);
    // A frame that steps to itself (same address, same CFA) makes no progress: the trace ends
    // instead of filling the buffer with one address.
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    eh.fde4(c, 0x1000, 0x10, &[]);
    eh.fde4(c, 0x1010, 0x10, &[0x0c, 7, 0, 0x16, 7, 2, 0x77, 0]);
    let frames = eh.finish();
    let o = object(&frames);
    let m = words(&[(0x7000, 0x1015), (0x7008, 0x1015)]);
    let got = run(&[o], &m, start, 0, 64);
    assert!(!got.is_empty() && got.len() < 4, "{got:x?}");
    // A second object is searched too.
    let mut eh = Eh::new(EH + 0x1000);
    let c = eh.std_cie(false);
    eh.fde4(c, 0x9100, 0x10, &[0x07, 16]);
    let more = eh.finish();
    let mut b = Object::new();
    b.add_segment(Image { addr: EH + 0x1000, bytes: &more });
    b.add_segment(Image { addr: 0x9000, bytes: &CODE[..0x1000] });
    b.set_frames(EH + 0x1000, EH + 0x1000 + more.len());
    let frames = with_header(true);
    let mut a = Object::new();
    a.add_segment(Image { addr: EH, bytes: &frames });
    a.add_segment(Image { addr: 0x1000, bytes: &CODE[..0x1000] });
    a.set_header(EH);
    let m = words(&[(0x7000, 0x9105)]);
    assert_eq!(run(&[a, b], &m, start, 0, 8), [0x9105]);
    assert_eq!(run(&[Object::default(), a, b], &m, start, 0, 8), [0x9105]);
}

#[test]
fn ip27_caller_skips_the_family_and_what_runs_below_it() {
    // Trace: s3_3_c_3 (the mtrace log's caller text); IP-27.
    // c (0x1000, the capturing frame) <- hook (0x1100) <- malloc (0x1200) <- realloc (0x1300)
    // <- main (0x1400) <- start (0x1500, outermost).
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    for f in 0..5 {
        eh.fde4(c, 0x1000 + f * 0x100, 0x100, &[]);
    }
    eh.fde4(c, 0x1500, 0x100, &[0x07, 16]);
    let frames = eh.finish();
    let o = object(&frames);
    let mem = words(&[(0x7000, 0x1105), (0x7008, 0x1205), (0x7010, 0x1305), (0x7018, 0x1405), (0x7020, 0x1505)]);
    let start = regs(&[(RA, 0x1005), (RSP, 0x7000)]);
    let caller = |inside: &[usize], max: usize| unwind::caller(&[o], &mem, start, inside, max);
    // The return address into the first frame whose function is not in `inside` (the caller
    // lists the family and the paths below it).
    assert_eq!(caller(&[0x1100, 0x1200, 0x1300], 32), Some(0x1405));
    assert_eq!(caller(&[0x1100, 0x1200], 32), Some(0x1305));
    assert_eq!(caller(&[0x1200], 32), Some(0x1105));
    assert_eq!(caller(&[], 32), Some(0x1105));
    // `max` bounds the frames looked at; a stack that ends inside the list has no caller.
    assert_eq!(caller(&[0x1100, 0x1200, 0x1300], 5), Some(0x1405));
    assert_eq!(caller(&[0x1100, 0x1200, 0x1300], 3), None);
    assert_eq!(caller(&[0x1100, 0x1200, 0x1300, 0x1400, 0x1500], 32), None);
}

#[test]
fn ip27_damaged_call_frame_information_ends_the_trace() {
    // Trace: s2_7_c_1, s2_7_f_2 (frames whose unwind information is unusable); IP-27, D-2.7.
    // Deterministic mutations of a valid section: every result is a short trace, never a panic,
    // a loop or a read outside the bytes given.
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    let p = eh.cie(&[3, b'z', b'P', b'L', b'R', 0, 1, 0x78, 16, 7, 0x9b, 0, 0, 0, 0, 0x1b, 0x1b, 0x0c, 7, 8, 0x90, 1]);
    eh.fde4(c, 0x1000, 0x100, &[0x41, 0x0e, 16, 0x86, 2, 0x43, 0x0d, 6, 0x0a, 0x0f, 2, 0x77, 8, 0x0b]);
    eh.fde(p, |field| {
        let mut t = ((0x2000 - field as i64) as i32).to_le_bytes().to_vec();
        t.extend_from_slice(&0x100u32.to_le_bytes());
        t.extend_from_slice(&[4, 0, 0, 0, 0, 0x10, 3, 3, 0x08, 16, 0x1c, 0x2e, 8]);
        t
    });
    eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
    let good = eh.finish();
    let hdr = with_header(true);
    let mem = words(&[(0x7000, 0x2010), (0x7008, 0x3010), (0x7100, 0x7200), (0x7108, 0x2010), (0x6ff8, 0x7800), (0x7800, 0x3010)]);
    let start = regs(&[(RA, 0x1050), (RSP, 0x7000), (RBP, 0x7100), (RBX, 0x7800)]);
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..4000 {
        let mut bytes = if round % 4 == 3 { hdr.clone() } else { good.clone() };
        for _ in 0..1 + next() % 4 {
            let at = (next() as usize) % bytes.len();
            bytes[at] = next() as u8;
        }
        let mut o = Object::new();
        o.add_segment(Image { addr: EH, bytes: &bytes });
        o.add_segment(Image { addr: 0x1000, bytes: &CODE });
        if round % 4 == 3 {
            o.set_header(EH);
        } else {
            o.set_frames(EH, EH + bytes.len());
        }
        let got = run(&[o], &mem, start, 0, 32);
        assert!(got.len() <= 32);
    }
    // Truncated sections: every prefix of the good one.
    for len in 0..good.len() {
        let o = object(&good[..len]);
        let _ = run(&[o], &mem, start, 0, 8);
    }
}

#[test]
fn ip27_program_headers_and_objects() {
    // Trace: no spec clause (the unwinder's program-header and object helpers, an internal primitive); IP-27.
    let mut ehdr = [0u8; 64];
    ehdr[..6].copy_from_slice(b"\x7fELF\x02\x01");
    ehdr[32..40].copy_from_slice(&64u64.to_le_bytes());
    ehdr[54..56].copy_from_slice(&56u16.to_le_bytes());
    ehdr[56..58].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(unwind::program_headers(&ehdr), Some((64, 3)));
    assert_eq!(unwind::program_headers(&[0; 64]), None);
    let mut bad = ehdr;
    bad[4] = 1; // 32-bit
    assert_eq!(unwind::program_headers(&bad), None);
    let mut bad = ehdr;
    bad[54] = 32; // another program-header size
    assert_eq!(unwind::program_headers(&bad), None);
    let ph = |t: u64, f: u64, off: u64, va: u64, sz: u64| [t | (f << 32), off, va, va, sz, sz, 0x1000];
    let mut w = Vec::new();
    w.extend(ph(1, 5, 0, 0, 0x1000));
    w.extend(ph(1, 6, 0x1000, 0x2000, 0x100));
    w.extend(ph(0x6474_e550, 4, 0x800, 0x800, 0x20));
    static ZEROS: [u8; 0x1000] = [0; 0x1000];
    let o = unwind::object(&w, 3, 0x5_0000, |_, n| ZEROS.get(..n).unwrap_or(&[]));
    assert!(o.contains(0x5_0000) && o.contains(0x5_0fff));
    assert!(!o.contains(0x5_2000) && !o.contains(0x5_1000));
    assert!(o.has_cfi());
    assert!(unwind::in_object(&w, 3, 0x5_0000, 0x5_2050));
    assert!(!unwind::in_object(&w, 3, 0x5_0000, 0x5_2100));
    assert!(!unwind::in_object(&w, 3, 0x5_0000, 0x4_ffff));
    assert_eq!(unwind::bias_of(&w, 3, 0x5_0000), Some(0x5_0000));
    // No header: no call-frame information until the link's bounds are set.
    let o = unwind::object(&w[..14], 2, 0x5_0000, |_, n| ZEROS.get(..n).unwrap_or(&[]));
    assert!(!o.has_cfi());
    // Segments are bounded: the ninth is refused.
    let mut o = Object::new();
    for i in 0..8 {
        assert!(o.add_segment(Image { addr: i * 0x1000, bytes: &ZEROS }));
    }
    assert!(!o.add_segment(Image { addr: 0x9000, bytes: &ZEROS }));
    // Regs: unknown by default, settable, and registers beyond the tracked ones are ignored.
    let mut r = Regs::new();
    assert_eq!(r.get(RSP), None);
    r.set(RSP, Some(5));
    r.set(99, Some(1));
    assert_eq!((r.get(RSP), r.get(99)), (Some(5), None));
    assert_eq!(Regs::default(), Regs::new());
}

/// The usual initial row (CFA = rsp + 8, return address at CFA - 8).
const ROW: [u8; 5] = [0x0c, 7, 8, 0x90, 1];
/// What `plain_start` traces when f's FDE is usable and gives the usual row.
const THROUGH_F: [usize; 2] = [0x2010, 0x3010];

/// A CIE body from the version on: `aug`, the version 4 address and segment sizes `sizes`,
/// code factor 1, data factor -8, return address column `ra`, the augmentation data (with its
/// length when `aug` starts with `z`) and the initial instructions.
fn cie_body(version: u8, aug: &str, sizes: &[u8], ra: u8, data: &[u8], insns: &[u8]) -> Vec<u8> {
    let mut b = vec![version];
    b.extend_from_slice(aug.as_bytes());
    b.push(0);
    b.extend_from_slice(sizes);
    b.extend_from_slice(&[1, 0x78, ra]);
    if aug.starts_with('z') {
        uleb(data.len() as u64, &mut b);
        b.extend_from_slice(data);
    }
    b.extend_from_slice(insns);
    b
}

/// The rest of f's FDE (after its CIE pointer, `field` being the next byte's address) in pcrel
/// sdata4: [0x1000, 0x1100), augmentation data `aug` when the CIE has some, then `insns`.
fn pcrel4(field: usize, aug: Option<&[u8]>, insns: &[u8]) -> Vec<u8> {
    let mut t = ((0x1000i64 - field as i64) as i32).to_le_bytes().to_vec();
    t.extend_from_slice(&0x100u32.to_le_bytes());
    if let Some(a) = aug {
        uleb(a.len() as u64, &mut t);
        t.extend_from_slice(a);
    }
    t.extend_from_slice(insns);
    t
}

/// As `through_f` from `plain_start` with g's CFA at rbx + 8, but f's FDE (the record body
/// after its CIE pointer: `f_tail(field)`) refers to a CIE whose body is `f_cie`.
fn through_custom_f(f_cie: &[u8], f_tail: impl Fn(usize) -> Vec<u8>) -> Vec<usize> {
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    let fc = eh.cie(f_cie);
    eh.fde(fc, f_tail);
    eh.fde4(c, 0x2000, 0x100, &[0x0c, RBX as u8, 8]);
    eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
    let frames = eh.finish();
    let (start, mem) = plain_start();
    run(&[object(&frames)], &words(&mem), start, 0, 16)
}

#[test]
fn ip27_cie_versions_augmentations_and_return_column_are_read_exactly() {
    // Trace: s2_7_c_1, s2_7_f_2 (frames whose unwind information is unusable); IP-27, D-2.7.
    let zr = |version: u8, sizes: &[u8]| through_custom_f(&cie_body(version, "zR", sizes, 16, &[0x1b], &ROW), |f| pcrel4(f, Some(&[]), &[]));
    // Versions 1, 3 and 4 (8-byte addresses, no segment selector) are read; others are not.
    assert_eq!(zr(1, &[]), THROUGH_F);
    assert_eq!(zr(3, &[]), THROUGH_F);
    assert_eq!(zr(4, &[8, 0]), THROUGH_F);
    assert!(zr(4, &[4, 0]).is_empty(), "4-byte addresses");
    assert!(zr(4, &[8, 1]).is_empty(), "a segment selector");
    assert!(zr(2, &[]).is_empty(), "version 2");
    assert!(zr(5, &[]).is_empty(), "version 5");
    // An unknown letter after "z" rejects the CIE (where its data ends is unknown).
    assert!(through_custom_f(&cie_body(1, "zRX", &[], 16, &[0x1b], &ROW), |f| pcrel4(f, Some(&[]), &[])).is_empty());
    // Without "z": the empty string is read (absolute 8-byte pointers, no FDE augmentation
    // data); any other string is not.
    let abs8 = |_: usize| {
        let mut t = 0x1000u64.to_le_bytes().to_vec();
        t.extend_from_slice(&0x100u64.to_le_bytes());
        t
    };
    assert_eq!(through_custom_f(&cie_body(1, "", &[], 16, &[], &ROW), abs8), THROUGH_F);
    assert!(through_custom_f(&cie_body(1, "x", &[], 16, &[], &ROW), abs8).is_empty());
    // "zLR": L's encoding byte comes before R's, so the FDE pointers are udata4 here, and the
    // FDE's augmentation data (an 8-byte LSDA pointer) is skipped by its length.
    let udata4 = |_: usize| {
        let mut t = 0x1000u32.to_le_bytes().to_vec();
        t.extend_from_slice(&0x100u32.to_le_bytes());
        t.push(8);
        t.extend_from_slice(&[0; 8]);
        t
    };
    assert_eq!(through_custom_f(&cie_body(1, "zLR", &[], 16, &[0x00, 0x03], &ROW), udata4), THROUGH_F);
    // The FDE's augmentation data is not read as instructions: its length (7) and first byte
    // would read as "return address undefined".
    let aug = [16, 0, 0, 0, 0, 0, 0];
    assert_eq!(through_custom_f(&cie_body(1, "zR", &[], 16, &[0x1b], &ROW), |f| pcrel4(f, Some(&aug), &[])), THROUGH_F);
    // A return-address column other than 16 (here rbp's, 6) is where the caller's address is.
    let rbp_ra = cie_body(1, "zR", &[], RBP as u8, &[0x1b], &[0x0c, 7, 8, 0x86, 1]);
    assert_eq!(through_custom_f(&rbp_ra, |f| pcrel4(f, Some(&[]), &[])), THROUGH_F);
}

#[test]
fn ip27_record_forms() {
    // Trace: s2_7_c_1, s2_7_f_2 (frames whose unwind information is unusable); IP-27, D-2.7.
    // f's CIE and FDE in the 64-bit form (length 0xffffffff, then an 8-byte length; the CIE
    // id and pointer stay 4 bytes).
    let ext = |eh: &mut Eh, body: &[u8]| {
        let at = eh.at();
        eh.out.extend_from_slice(&u32::MAX.to_le_bytes());
        eh.out.extend_from_slice(&(body.len() as u64).to_le_bytes());
        eh.out.extend_from_slice(body);
        at
    };
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    let mut cie = vec![0, 0, 0, 0];
    cie.extend(cie_body(1, "zR", &[], 16, &[0x1b], &ROW));
    let fc = ext(&mut eh, &cie);
    let id_at = eh.at() + 12;
    let mut fde = ((id_at - fc) as u32).to_le_bytes().to_vec();
    fde.extend(pcrel4(id_at + 4, Some(&[]), &[]));
    ext(&mut eh, &fde);
    eh.fde4(c, 0x2000, 0x100, &[0x0c, RBX as u8, 8]);
    eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
    let frames = eh.finish();
    let (start, mem) = plain_start();
    assert_eq!(run(&[object(&frames)], &words(&mem), start, 0, 16), THROUGH_F);
    // An FDE whose CIE pointer lands on a record that is not a CIE (its id is not 0) is not
    // used, even when the rest of that record would read as a CIE.
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    let mut not_cie = vec![1, 0, 0, 0];
    not_cie.extend(cie_body(1, "zR", &[], 16, &[0x1b], &ROW));
    let fake = eh.record(not_cie);
    eh.fde(fake, |f| pcrel4(f, Some(&[]), &[]));
    eh.fde4(c, 0x2000, 0x100, &[0x0c, RBX as u8, 8]);
    let frames = eh.finish();
    let (start, mem) = plain_start();
    assert!(run(&[object(&frames)], &words(&mem), start, 0, 16).is_empty());
}

#[test]
fn ip27_header_tables_in_every_fixed_size_encoding() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    // The FDEs follow a terminator, so a scan from the header's frame pointer cannot reach
    // them: only the table finds them. Absolute entries (EH and code fit in 32 bits).
    let build = |enc: u8| {
        let mut eh = Eh::new(EH + 0x100);
        let c = eh.std_cie(false);
        eh.out.extend_from_slice(&[0; 4]);
        let fdes = [(0x1000usize, eh.fde4(c, 0x1000, 0x20, &[])), (0x1020, eh.fde4(c, 0x1020, 0x10, &[0x0e, 16])), (0x2000, eh.fde4(c, 0x2000, 4, &[]))];
        let frames = eh.finish();
        let mut hdr = vec![1, 0x1b, 0x03, enc];
        hdr.extend_from_slice(&(0x100i32 - 4).to_le_bytes());
        hdr.extend_from_slice(&3u32.to_le_bytes());
        for (loc, fde) in fdes {
            for v in [loc, fde] {
                match enc & 0x0f {
                    0x03 | 0x0b => hdr.extend_from_slice(&(v as u32).to_le_bytes()),
                    _ => hdr.extend_from_slice(&(v as u64).to_le_bytes()),
                }
            }
        }
        hdr.resize(0x100, 0);
        hdr.extend_from_slice(&frames);
        hdr
    };
    let mem = words(&[(0x7000, 0x9000), (0x7008, 0x9008)]);
    for (enc, found) in [(0x03u8, true), (0x0b, true), (0x00, true), (0x04, true), (0x0c, true), (0x01, false)] {
        let bytes = build(enc);
        let mut o = Object::new();
        o.add_segment(Image { addr: EH, bytes: &bytes });
        o.add_segment(Image { addr: 0x1000, bytes: &CODE });
        o.set_header(EH);
        let first = |pc: u64| run(&[o], &mem, regs(&[(RA, pc), (RSP, 0x7000)]), 0, 1);
        if found {
            assert_eq!(first(0x1001), [0x9000], "table {enc:#x}");
            assert_eq!(first(0x1021), [0x9008], "table {enc:#x}");
            assert_eq!(first(0x2004), [0x9000], "table {enc:#x}");
        } else {
            // uleb128 is not a fixed size: the header falls back to the scan, which stops at
            // the terminator.
            assert!(first(0x1001).is_empty(), "table {enc:#x}");
        }
    }
}

#[test]
fn ip27_walk_lookup_address_and_progress() {
    // Trace: s2_7_c_1 (backtrace stores the return addresses of the call stack); IP-27, D-2.7.
    // g [0x2000, 0x2010) ends with its call, so its return address 0x2010 is the first byte of
    // another function, whose row would end the walk: an ordinary frame is looked up at its
    // address - 1.
    let mut eh = Eh::new(EH);
    let c = eh.std_cie(false);
    eh.fde4(c, 0x1000, 0x100, &[]);
    eh.fde4(c, 0x2000, 0x10, &[0x0c, RBX as u8, 8]);
    eh.fde4(c, 0x2010, 0xf0, &[0x07, 16]);
    eh.fde4(c, 0x3000, 0x100, &[0x07, 16]);
    let frames = eh.finish();
    let o = object(&frames);
    let (start, mem) = plain_start();
    assert_eq!(run(&[o], &words(&mem), start, 0, 16), THROUGH_F);
    // f calls itself: the same return address with a higher stack pointer is progress.
    let mem = words(&[(0x7000, 0x1010), (0x7008, 0x1010), (0x7010, 0x2010), (0x7800, 0x3010)]);
    assert_eq!(run(&[o], &mem, start, 0, 16), [0x1010, 0x1010, 0x2010, 0x3010]);
}

#[test]
fn ip27_program_header_edge_cases() {
    // Trace: no spec clause (the unwinder's program-header and object helpers, an internal primitive); IP-27.
    // PN_XNUM (0xffff): the real count is elsewhere, so no table is returned; 0xfffe is a count.
    let mut ehdr = [0u8; 64];
    ehdr[..6].copy_from_slice(b"\x7fELF\x02\x01");
    ehdr[32..40].copy_from_slice(&64u64.to_le_bytes());
    ehdr[54..56].copy_from_slice(&56u16.to_le_bytes());
    ehdr[56..58].copy_from_slice(&0xffffu16.to_le_bytes());
    assert_eq!(unwind::program_headers(&ehdr), None);
    ehdr[56..58].copy_from_slice(&0xfffeu16.to_le_bytes());
    assert_eq!(unwind::program_headers(&ehdr), Some((64, 0xfffe)));
    let ph = |t: u64, f: u64, off: u64, va: u64, filesz: u64, memsz: u64| [t | (f << 32), off, va, va, filesz, memsz, 0x1000];
    // A writable segment's zero-filled tail (.bss, past filesz up to memsz) is in the object.
    let mut w = Vec::new();
    w.extend(ph(1, 5, 0, 0, 0x1000, 0x1000));
    w.extend(ph(1, 6, 0x1000, 0x2000, 0x100, 0x300));
    assert!(unwind::in_object(&w, 2, 0x5_0000, 0x5_2180));
    assert!(unwind::in_object(&w, 2, 0x5_0000, 0x5_22ff));
    assert!(!unwind::in_object(&w, 2, 0x5_0000, 0x5_2300));
    // The bias comes from the segment that maps file offset 0 (the ELF header), even when
    // another loadable segment is listed first.
    let mut w = Vec::new();
    w.extend(ph(1, 5, 0x2000, 0x40_2000, 0x100, 0x100));
    w.extend(ph(1, 4, 0, 0x40_0000, 0x1000, 0x1000));
    assert_eq!(unwind::bias_of(&w, 2, 0x7f00_0040_0000), Some(0x7f00_0000_0000));
    assert_eq!(unwind::bias_of(&w[..7], 1, 0x7f00_0040_0000), None);
}
