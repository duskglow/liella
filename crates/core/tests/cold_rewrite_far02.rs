//! Equivalence tests written before the COLD-REWRITE-W-far-02 rewrite of `search`, `random` and
//! `config`: they pin the observable behaviour of the previous implementation (Clauses: s14_1_c_0,
//! s14_4_c_3, s14_5_c_0, s14_6_c_0, s9_3_c_0, s9_4_c_0, s1_8_c_0; D-14.1–D-14.3, D-9.3, OD-13, D-1.8).

#![allow(clippy::type_complexity, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::cast_possible_truncation, missing_docs)]

use std::cmp::Ordering;
use std::collections::HashMap;

use liella::config::{fields, lines, trim, Tunables};
use liella::random::{rand_r, AdditiveFeedback, Rng48, SIZES};
use liella::search::*;

struct Gen(u64);
impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn fnv(words: &[u32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for w in words {
        for b in w.to_le_bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

// ---------------------------------------------------------------- random

/// The documented additive-feedback generator, stated again from its doc comment, as a model.
#[test]
fn additive_feedback_is_pinned_per_state_size() {
    // Clauses: s9_2_c_0
    // (size, seed) -> (digest of state after seeding, first four outputs, digest of 3000 outputs, state digest after 37 outputs)
    let mut got = Vec::new();
    for size in SIZES {
        for seed in [0u32, 1, 42, 0xdead_beef] {
            let t = AdditiveFeedback::type_for(size).unwrap();
            let mut s = vec![0u32; size / 4];
            AdditiveFeedback::seed(s.as_mut_slice(), t, seed);
            assert_eq!(AdditiveFeedback::type_of(s.as_slice()), t);
            let after_seed = fnv(&s);
            let first: Vec<u32> = (0..4).map(|_| AdditiveFeedback::next(s.as_mut_slice())).collect();
            let mut s2 = vec![0u32; size / 4];
            AdditiveFeedback::seed(s2.as_mut_slice(), t, seed);
            for _ in 0..37 {
                AdditiveFeedback::next(s2.as_mut_slice());
            }
            let st37 = fnv(&s2);
            let all: Vec<u32> = (0..3000).map(|_| AdditiveFeedback::next(s.as_mut_slice())).collect();
            got.push(format!("{size}/{seed:x}: {after_seed:x} {first:?} {:x} {st37:x}", fnv(&all)));
        }
    }
    if let Ok(path) = std::env::var("COLD_GEN") {
        std::fs::write(path, got.join("\n") + "\n").unwrap();
        return;
    }
    let want = include_str!("cold_rewrite_far02_af.txt");
    assert_eq!(got.join("\n") + "\n", want);
}

#[test]
fn additive_feedback_state_is_the_documented_layout() {
    // Clauses: s9_2_c_0
    // Header word 0: bits 0-2 type, 8-15 front, 16-23 rear; resuming from a copied state continues the sequence.
    for size in SIZES {
        let t = AdditiveFeedback::type_for(size).unwrap();
        let mut a = vec![0u32; size / 4];
        AdditiveFeedback::seed(a.as_mut_slice(), t, 7);
        for _ in 0..11 {
            AdditiveFeedback::next(a.as_mut_slice());
        }
        assert_eq!(a[0] & 7, t);
        let mut b = a.clone();
        let xa: Vec<u32> = (0..50).map(|_| AdditiveFeedback::next(a.as_mut_slice())).collect();
        let xb: Vec<u32> = (0..50).map(|_| AdditiveFeedback::next(b.as_mut_slice())).collect();
        assert_eq!(xa, xb);
        assert_eq!(a, b);
        assert!(xa.iter().all(|&v| v < 1 << 31));
    }
    // A buffer of n >= 8 bytes uses the largest type that fits.
    let ts: Vec<Option<u32>> = [0usize, 7, 8, 31, 32, 63, 64, 127, 128, 255, 256, 4096].iter().map(|&n| AdditiveFeedback::type_for(n)).collect();
    assert_eq!(ts, [None, None, Some(0), Some(0), Some(1), Some(1), Some(2), Some(2), Some(3), Some(3), Some(4), Some(4)]);
}

#[test]
fn rand_r_follows_the_posix_example() {
    // Clauses: s9_3_c_0
    // Three steps of s = s*1103515245 + 12345 contribute 11, 10 and 10 bits (bits 16 and up).
    for start in [0u32, 1, 12345, 0xffff_ffff, 0x8000_0000] {
        let mut model = start;
        let mut s = start;
        for _ in 0..1000 {
            let mut out = 0u32;
            for bits in [11u32, 10, 10] {
                model = model.wrapping_mul(1_103_515_245).wrapping_add(12345);
                out = (out << bits) | ((model >> 16) & ((1 << bits) - 1));
            }
            assert_eq!(rand_r(&mut s), out & 0x7fff_ffff);
            assert_eq!(s, model);
        }
    }
}

#[test]
fn rng48_matches_the_posix_recurrence_and_known_vectors() {
    // Clauses: s9_4_c_0
    // srand48(1): lrand48() = 89400484 ... is not claimed; the model is the recurrence itself.
    let model = |x: u64, a: u64, c: u64| (u128::from(a) * u128::from(x) + u128::from(c)) as u64 & ((1 << 48) - 1);
    for seed in [0i64, 1, -1, 0x1234_5678_9abc_def0u64 as i64, i64::MIN] {
        let mut r = Rng48::UNSEEDED;
        r.srand48(seed);
        let mut x = ((seed as u64 & 0xffff_ffff) << 16) | 0x330e;
        assert_eq!(r.x, [x as u16, (x >> 16) as u16, (x >> 32) as u16]);
        for _ in 0..200 {
            x = model(x, 0x5_DEEC_E66D, 0xb);
            assert_eq!(r.next(), x);
            assert_eq!(r.x, [x as u16, (x >> 16) as u16, (x >> 32) as u16]);
        }
    }
    // The classic first outputs after srand48(1).
    let mut r = Rng48::UNSEEDED;
    r.srand48(1);
    assert_eq!(Rng48::to_nonneg(r.next()), 89_400_484);
    assert_eq!(Rng48::to_nonneg(r.next()), 976_015_093);
    // seed48 returns the previous state and restores default parameters; lcong48 sets a, c.
    let mut r = Rng48::UNSEEDED;
    r.lcong48([1, 2, 3, 0x0005, 0x0004, 0x0003, 0x77]);
    assert_eq!(r.x, [1, 2, 3]);
    assert_eq!((r.a, r.c), (0x0003_0004_0005, 0x77));
    let x = r.next();
    assert_eq!(x, model(0x0003_0002_0001, 0x0003_0004_0005, 0x77));
    let prev = r.seed48([9, 8, 7]);
    assert_eq!(prev, [x as u16, (x >> 16) as u16, (x >> 32) as u16]);
    assert_eq!((r.x, r.a, r.c), ([9, 8, 7], 0x5_DEEC_E66D, 0xb));
    let mut st = [9u16, 8, 7];
    let v = r.step(&mut st);
    assert_eq!(v, model(0x0007_0008_0009, 0x5_DEEC_E66D, 0xb));
    assert_eq!(st, [v as u16, (v >> 16) as u16, (v >> 32) as u16]);
    assert_eq!(r.x, [9, 8, 7], "step leaves the generator's own state alone");
    assert_eq!(Rng48::to_double(1 << 47), 0.5);
    assert_eq!(Rng48::to_double(0), 0.0);
    assert_eq!(Rng48::to_nonneg(1 << 47), 1 << 30);
    assert_eq!(Rng48::to_signed((1 << 48) - 1), -1);
    assert_eq!(Rng48::to_signed(1 << 47), -(1 << 31));
    assert_eq!(Rng48::to_signed((1 << 47) - 1), (1 << 31) - 1);
}

// ---------------------------------------------------------------- config

#[test]
fn tunables_parse_shapes() {
    // no spec clause: pins the tunable parser (D-1.8)
    let p = |s: &str| Tunables::parse(s.as_bytes(), false);
    let d = Tunables::default();
    assert_eq!(p(""), d);
    assert_eq!(p(":::"), d);
    assert_eq!(p("liella.cpu.hwcaps=AVX2"), Tunables { avx2: Some(true), ..d });
    assert_eq!(p("liella.cpu.hwcaps=-ERMS,AVX2"), Tunables { avx2: Some(true), erms: Some(false), ..d });
    assert_eq!(p("glibc.cpu.hwcaps=-AVX2,AVX2"), Tunables { avx2: Some(true), ..d }, "the later item wins");
    assert_eq!(p("glibc.cpu.x86_rep_stosb_threshold=0x10"), Tunables { rep_stosb_threshold: Some(16), ..d });
    assert_eq!(p("glibc.cpu.x86_rep_stosb_threshold=16:glibc.cpu.x86_rep_stosb_threshold=32"), Tunables { rep_stosb_threshold: Some(32), ..d }, "the later setting wins");
    assert_eq!(p("glibc.cpu.x86_rep_stosb_threshold=16:glibc.cpu.x86_rep_stosb_threshold=zz"), Tunables { rep_stosb_threshold: Some(16), ..d }, "an invalid later value is ignored");
    assert_eq!(p("liella.pthread.rseq=1"), Tunables { rseq: Some(true), ..d });
    assert_eq!(p("liella.pthread.rseq=0"), Tunables { rseq: Some(false), ..d });
    assert_eq!(p("nonsense"), d);
    assert_eq!(p("=1:glibc.pthread.rseq=0"), Tunables { rseq: Some(false), ..d });
    assert_eq!(p("glibc.pthread.rseq"), d, "no value");
    assert_eq!(p("glibc.cpu.x86_rep_movsb_threshold="), d);
    assert_eq!(Tunables::parse(b"glibc.pthread.rseq=0", true), d);
    assert_eq!(Tunables::malloc_perturb(b"liella.malloc.perturb=255", false), Some(255));
    assert_eq!(Tunables::malloc_perturb(b"liella.malloc.perturb=256", false), None);
    assert_eq!(Tunables::malloc_perturb(b"", false), None);
}

/// Black-box probed against the previous implementation: (input, avx2, erms, rep_movsb_threshold, rseq, perturb).
#[test]
fn tunables_probed_shapes() {
    // no spec clause: pins the tunable parser (D-1.8)
    const N: Option<bool> = None;
    let t = Some(true);
    let f = Some(false);
    let rows: &[(&str, Option<bool>, Option<bool>, Option<usize>, Option<bool>, Option<u8>)] = &[
        ("glibc.cpu.hwcaps=-avx2", f, N, None, N, None),
        ("glibc.cpu.hwcaps=-Avx2", f, N, None, N, None),
        ("glibc.cpu.hwcaps=AVX2_Usable", t, N, None, N, None),
        ("glibc.cpu.hwcaps=-AVX2_X", N, N, None, N, None),
        ("glibc.cpu.hwcaps=-", N, N, None, N, None),
        ("glibc.cpu.hwcaps=", N, N, None, N, None),
        ("glibc.cpu.hwcaps=ERMS_Usable", N, t, None, N, None),
        ("glibc.cpu.hwcaps=-ERMS,,AVX2", t, f, None, N, None),
        ("glibc.cpu.hwcaps= AVX2", N, N, None, N, None),
        ("glibc.cpu.hwcaps=-AVX2_Usable,ERMS", f, t, None, N, None),
        ("glibc.cpu.hwcaps=-Fast_Unaligned_Load", N, N, None, N, None),
        ("glibc.cpu.hwcaps=-AVX2:glibc.cpu.hwcaps=ERMS", f, t, None, N, None),
        ("GLIBC.cpu.hwcaps=-AVX2", N, N, None, N, None),
        ("glibc.cpu.hwcaps=-AVX2=3", N, N, None, N, None),
        // Probe-settled against glibc 2.39 (LEGACY-TUNABLES-POLICY): base 0, leading blanks and
        // a sign accepted, the listed minimum 0x100 for this threshold.
        ("glibc.cpu.x86_rep_movsb_threshold=0X1000", N, N, Some(4096), N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=0X10", N, N, None, N, None),
        ("glibc.cpu.x86_rep_movsb_threshold= 4096", N, N, Some(4096), N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=16 ", N, N, None, N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=+4096", N, N, Some(4096), N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=0x", N, N, None, N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=010000", N, N, Some(4096), N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=1_000", N, N, None, N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=99999999999999999999", N, N, None, N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=0xffffffffffffffff", N, N, Some(usize::MAX), N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=0", N, N, None, N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=-1", N, N, None, N, None),
        ("liella.cpu.rep_movsb_threshold=0x700:glibc.cpu.x86_rep_movsb_threshold=0x900", N, N, Some(0x900), N, None),
        ("glibc.cpu.x86_rep_movsb_threshold=0x900:liella.cpu.rep_movsb_threshold=0x700", N, N, Some(0x700), N, None),
        ("glibc.cpu.rep_movsb_threshold=5", N, N, None, N, None),
        ("liella.cpu.x86_rep_movsb_threshold=5", N, N, None, N, None),
        ("glibc.pthread.rseq=0x0", N, N, None, f, None),
        ("glibc.pthread.rseq=00", N, N, None, f, None),
        ("glibc.pthread.rseq=true", N, N, None, N, None),
        ("glibc.pthread.rseq=0:glibc.pthread.rseq=1", N, N, None, t, None),
        ("glibc.pthread.rseq= 0", N, N, None, f, None),
        ("glibc.pthread.rseq=2", N, N, None, N, None),
        ("glibc.pthread.rseq=0=1", N, N, None, N, None),
        // Re-probed on glibc 2.39 (keke, TUNABLES-WARNING-3, #1374): the original row expected
        // rseq=0 to still apply after the malformed item. glibc drops the WHOLE string:
        //   GLIBC_TUNABLES='a=b=c:glibc.pthread.rseq=0' /lib64/ld.so.1 --list-tunables
        // printed the "WARNING: ld.so: invalid GLIBC_TUNABLES ...: ignored." line and
        // glibc.pthread.rseq stayed at 1 (not 0). Hence all None here.
        ("a=b=c:glibc.pthread.rseq=0", N, N, None, N, None),
        ("glibc.pthread.rseq=0:", N, N, None, N, None),
        (":glibc.pthread.rseq=0", N, N, None, f, None),
        ("glibc.malloc.perturb=0x", N, N, None, N, Some(0)),
        ("glibc.malloc.perturb=-1", N, N, None, N, None),
        ("glibc.malloc.perturb= 5", N, N, None, N, Some(5)),
        ("glibc.malloc.perturb=0377", N, N, None, N, Some(255)),
        ("glibc.malloc.perturb=0xFF", N, N, None, N, Some(255)),
        ("glibc.malloc.perturb=+5", N, N, None, N, Some(5)),
        ("glibc.malloc.perturb=", N, N, None, N, Some(0)),
        ("glibc.malloc.perturb=5:liella.malloc.perturb=6", N, N, None, N, Some(6)),
        ("liella.malloc.perturb=6:glibc.malloc.perturb=5", N, N, None, N, Some(5)),
    ];
    for &(input, avx2, erms, movsb, rseq, perturb) in rows {
        let p = Tunables::parse(input.as_bytes(), false);
        assert_eq!((p.avx2, p.erms, p.rep_movsb_threshold, p.rseq, p.rep_stosb_threshold), (avx2, erms, movsb, rseq, None), "{input}");
        assert_eq!(Tunables::malloc_perturb(input.as_bytes(), false), perturb, "{input}");
    }
}

#[test]
fn config_lines_trim_fields() {
    // no spec clause: the internal line reader (IP-21)
    let got: Vec<&[u8]> = lines(b"# comment\n\n  host  a b # tail\r\n\tx\ty\t\nlast").collect();
    assert_eq!(got, [&b"host  a b"[..], b"x\ty", b"last"]);
    assert_eq!(lines(b"").count(), 0);
    assert_eq!(lines(b"\n\n  \n#\n").count(), 0);
    assert_eq!(lines(b"a#b#c\n").next().unwrap(), b"a");
    assert_eq!(trim(b" \t\r x y \r\t "), b"x y");
    assert_eq!(trim(b""), b"");
    assert_eq!(trim(b"   "), b"");
    let f: Vec<&[u8]> = fields(b"  a \t b\tc  ").collect();
    assert_eq!(f, [&b"a"[..], b"b", b"c"]);
    assert_eq!(fields(b"").count(), 0);
    assert_eq!(fields(b"   ").count(), 0);
}

// ---------------------------------------------------------------- search

fn sort_ref(v: &[u8], size: usize, key: impl Fn(&[u8]) -> u32) -> Vec<u8> {
    let mut items: Vec<&[u8]> = v.chunks(size).collect();
    items.sort_by_key(|e| key(e));
    items.concat()
}

#[test]
fn sorts_agree_with_a_reference_for_many_shapes() {
    // Clauses: s14_1_c_0, s14_1_g_1
    let mut g = Gen(5);
    for size in [1usize, 2, 3, 4, 7, 8, 12, 24, 33, 100] {
        for n in (0..70usize).chain([127, 128, 129, 255, 256, 257, 1000]) {
            for modulus in [1u64, 3, 1 << 20] {
                let mut v = Vec::new();
                for _ in 0..n {
                    let k = (g.next() % modulus) as u32;
                    let mut e = vec![0u8; size];
                    for (i, b) in e.iter_mut().enumerate() {
                        *b = if i < 4 && size >= 4 { k.to_le_bytes()[i] } else { (g.next() & 0xff) as u8 };
                    }
                    if size < 4 {
                        e[0] = (k & 0xff) as u8;
                    }
                    v.extend_from_slice(&e);
                }
                let key = |e: &[u8]| if size >= 4 { u32::from_le_bytes(e[..4].try_into().unwrap()) } else { u32::from(e[0]) };
                let want = sort_ref(&v, size, key);
                let cmp = |a: &[u8], b: &[u8]| key(a).cmp(&key(b));
                let mut a = v.clone();
                sort_stable(&mut a, size, &mut { cmp });
                assert_eq!(a, want, "sort_stable size {size} n {n} mod {modulus}");
                let mut b = v.clone();
                sort_stable_in_place(&mut b, size, &mut { cmp });
                assert_eq!(b, want, "sort_stable_in_place size {size} n {n} mod {modulus}");
            }
        }
    }
}

#[test]
fn sort_comparison_counts_stay_near_the_minimum() {
    // Clauses: s14_1_g_2
    let mut g = Gen(77);
    for n in [100usize, 1000, 5000] {
        let mut v: Vec<u8> = (0..n).flat_map(|_| ((g.next() % 1_000_000) as u32).to_le_bytes()).collect();
        let mut calls = 0usize;
        sort_stable(&mut v, 4, &mut |a: &[u8], b: &[u8]| {
            calls += 1;
            u32::from_le_bytes(a.try_into().unwrap()).cmp(&u32::from_le_bytes(b.try_into().unwrap()))
        });
        let bound = n * (usize::BITS - (n - 1).leading_zeros()) as usize;
        assert!(calls <= bound, "n {n}: {calls} comparisons > n*ceil(log2 n) = {bound}");
        // Reverse-ordered and already-ordered input.
        let mut asc: Vec<u8> = (0..n as u32).flat_map(u32::to_le_bytes).collect();
        let mut c2 = 0usize;
        sort_stable(&mut asc, 4, &mut |a: &[u8], b: &[u8]| {
            c2 += 1;
            a.cmp(b).reverse().reverse()
        });
        assert!(c2 <= bound);
    }
    let mut desc: Vec<u8> = (0..1000u32).rev().flat_map(u32::to_le_bytes).collect();
    sort_stable(&mut desc, 4, &mut |a: &[u8], b: &[u8]| u32::from_le_bytes(a.try_into().unwrap()).cmp(&u32::from_le_bytes(b.try_into().unwrap())));
    assert_eq!(desc, (0..1000u32).flat_map(u32::to_le_bytes).collect::<Vec<u8>>());
}

#[test]
fn bsearch_and_lfind_edges() {
    // Clauses: s14_2_c_0, s14_3_c_0
    let v: Vec<u8> = [2u32, 4, 4, 4, 9, 11, 30].iter().flat_map(|x| x.to_le_bytes()).collect();
    let at = |k: u32| binary_search(&v, 4, &mut |e| k.cmp(&u32::from_le_bytes(e.try_into().unwrap())));
    for (k, found) in [(2u32, true), (4, true), (9, true), (11, true), (30, true), (1, false), (3, false), (10, false), (31, false)] {
        assert_eq!(at(k).is_some(), found, "key {k}");
        if let Some(i) = at(k) {
            assert_eq!(u32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap()), k);
        }
    }
    assert_eq!(binary_search(&[], 4, &mut |_| Ordering::Equal), None);
    assert_eq!(linear_find(&[], 4, &mut |_| true), None);
    assert_eq!(linear_find(&v, 4, &mut |e| u32::from_le_bytes(e.try_into().unwrap()) == 4), Some(1), "the first match");
    // Comparator call counts: bsearch is logarithmic.
    let big: Vec<u8> = (0..100_000u32).flat_map(u32::to_le_bytes).collect();
    let mut calls = 0;
    binary_search(&big, 4, &mut |e| {
        calls += 1;
        77_777u32.cmp(&u32::from_le_bytes(e.try_into().unwrap()))
    })
    .unwrap();
    assert!(calls <= 17, "{calls}");
}

// Tree storage over a map; the node layout is {key, left, right, height}.
#[derive(Default)]
struct MapTree {
    root: usize,
    nodes: HashMap<usize, (usize, usize, usize, u32)>,
    next: usize,
}
impl TreeNodes for MapTree {
    fn root(&self) -> usize {
        self.root
    }
    fn set_root(&mut self, n: usize) {
        self.root = n
    }
    fn key(&self, n: usize) -> usize {
        self.nodes[&n].0
    }
    fn link(&self, n: usize, right: bool) -> usize {
        let e = &self.nodes[&n];
        if right { e.2 } else { e.1 }
    }
    fn set_link(&mut self, n: usize, right: bool, c: usize) {
        let e = self.nodes.get_mut(&n).unwrap();
        if right { e.2 = c } else { e.1 = c }
    }
    fn height(&self, n: usize) -> u32 {
        self.nodes[&n].3
    }
    fn set_height(&mut self, n: usize, h: u32) {
        self.nodes.get_mut(&n).unwrap().3 = h
    }
    fn alloc(&mut self, key: usize) -> Option<usize> {
        self.next += 16;
        self.nodes.insert(self.next, (key, 0, 0, 1));
        Some(self.next)
    }
    fn free(&mut self, n: usize) {
        self.nodes.remove(&n).unwrap();
    }
}

fn parent_of(t: &MapTree, target: usize) -> usize {
    let mut n = t.root;
    let mut parent = 0;
    while n != target {
        parent = n;
        n = t.link(n, t.key(target) > t.key(n));
    }
    parent
}

#[test]
fn tdelete_reports_the_parent_the_new_root_or_empty() {
    // Clauses: s14_4_c_3
    let mut t = MapTree::default();
    let mut cmp = |a: usize, b: usize| a.cmp(&b);
    let mut g = Gen(3);
    let mut live: Vec<usize> = Vec::new();
    for round in 0..300 {
        // Fill, then drain in a random order; every result is checked against the tree before deletion.
        let n = 1 + (g.next() % 60) as usize;
        for _ in 0..n {
            let k = (g.next() % 500) as usize + 1;
            if !live.contains(&k) {
                tree_search(&mut t, k, true, &mut cmp).unwrap().unwrap();
                live.push(k);
            }
        }
        while !live.is_empty() {
            let i = (g.next() as usize) % live.len();
            let k = live.swap_remove(i);
            let node = tree_search(&mut t, k, false, &mut cmp).unwrap().unwrap();
            let par = parent_of(&t, node);
            let was_root = par == 0;
            let r = tree_delete(&mut t, k, &mut cmp);
            if live.is_empty() {
                assert_eq!(r, Deleted::Empty, "round {round}");
                assert_eq!(t.root, 0);
            } else if was_root {
                assert_eq!(r, Deleted::NewRoot(t.root), "round {round}");
            } else {
                assert_eq!(r, Deleted::Parent(par), "round {round}: the old parent");
                assert!(t.nodes.contains_key(&par));
            }
            assert_eq!(tree_delete(&mut t, k, &mut cmp), Deleted::Absent);
            if round % 50 == 0 {
                assert_eq!(t.nodes.len(), live.len());
            }
        }
        assert!(t.nodes.is_empty());
    }
}

#[test]
fn walk_and_destroy_order() {
    // Clauses: s14_4_c_4, s14_4_c_6
    let mut t = MapTree::default();
    let mut cmp = |a: usize, b: usize| a.cmp(&b);
    for k in [50usize, 20, 80, 10, 30, 70, 90, 5, 25, 35, 60] {
        tree_search(&mut t, k, true, &mut cmp).unwrap();
    }
    // Reference walk from the links.
    fn go(t: &MapTree, n: usize, d: u32, out: &mut Vec<(usize, u8, u32)>) {
        let (l, r) = (t.link(n, false), t.link(n, true));
        if l == 0 && r == 0 {
            out.push((t.key(n), 3, d));
            return;
        }
        out.push((t.key(n), 0, d));
        if l != 0 {
            go(t, l, d + 1, out);
        }
        out.push((t.key(n), 1, d));
        if r != 0 {
            go(t, r, d + 1, out);
        }
        out.push((t.key(n), 2, d));
    }
    let mut want = Vec::new();
    go(&t, t.root, 0, &mut want);
    let mut got = Vec::new();
    tree_walk(&t, t.root, 0, &mut |n, v, d| got.push((t.key(n), v as u8, d)));
    assert_eq!(got, want);
    assert_eq!(got.iter().filter(|e| e.1 == 3).count() + got.iter().filter(|e| e.1 == 0).count(), 11);
    // tdestroy: every key once; a node's children come before it.
    let mut order = Vec::new();
    let mut pos = HashMap::new();
    let mut parent_key = HashMap::new();
    fn par(t: &MapTree, n: usize, out: &mut HashMap<usize, usize>) {
        for r in [false, true] {
            let c = t.link(n, r);
            if c != 0 {
                out.insert(t.key(c), t.key(n));
                par(t, c, out);
            }
        }
    }
    par(&t, t.root, &mut parent_key);
    let root = t.root;
    tree_destroy(&mut t, root, &mut |k| {
        pos.insert(k, order.len());
        order.push(k);
    });
    assert_eq!(order.len(), 11);
    for (c, p) in &parent_key {
        assert!(pos[c] < pos[p], "child {c} before parent {p}");
    }
    assert!(t.nodes.is_empty());
    // Destroying nothing is fine.
    let mut none = 0;
    tree_destroy(&mut t, 0, &mut |_| none += 1);
    assert_eq!(none, 0);
}

#[test]
fn ordered_and_adversarial_inserts_stay_balanced() {
    // Clauses: s14_4_g_0
    for pattern in 0..4 {
        let mut t = MapTree::default();
        let mut cmp = |a: usize, b: usize| a.cmp(&b);
        let n = 5000usize;
        for i in 0..n {
            let k = match pattern {
                0 => i + 1,
                1 => n - i,
                2 => if i % 2 == 0 { i / 2 + 1 } else { n - i / 2 },
                _ => (i * 7919) % n + 1,
            };
            tree_search(&mut t, k, true, &mut cmp).unwrap();
        }
        assert!(t.height(t.root) <= 18, "pattern {pattern}: height {}", t.height(t.root));
        // Delete half and check the height bound still holds.
        for k in (1..=n).step_by(2) {
            tree_delete(&mut t, k, &mut cmp);
        }
        assert!(t.height(t.root) <= 17);
        assert_eq!(t.nodes.len(), n / 2);
    }
}

// Hash storage over a map of segments.
#[derive(Default)]
struct MapHash {
    head: usize,
    segs: HashMap<usize, (usize, usize, Vec<(usize, usize)>)>,
    keys: HashMap<usize, Vec<u8>>,
    next: usize,
}
impl HashMem for MapHash {
    fn head(&self) -> usize {
        self.head
    }
    fn set_head(&mut self, s: usize) {
        self.head = s
    }
    fn next(&self, s: usize) -> usize {
        self.segs[&s].0
    }
    fn capacity(&self, s: usize) -> usize {
        self.segs[&s].2.len()
    }
    fn used(&self, s: usize) -> usize {
        self.segs[&s].1
    }
    fn set_used(&mut self, s: usize, n: usize) {
        self.segs.get_mut(&s).unwrap().1 = n
    }
    fn key(&self, s: usize, i: usize) -> usize {
        self.segs[&s].2[i].0
    }
    fn set_entry(&mut self, s: usize, i: usize, k: usize, d: usize) {
        self.segs.get_mut(&s).unwrap().2[i] = (k, d)
    }
    fn entry_addr(&self, s: usize, i: usize) -> usize {
        s * 1_000_000 + i * 16 + 8
    }
    fn alloc_segment(&mut self, cap: usize, next: usize) -> Option<usize> {
        self.next += 1;
        self.segs.insert(self.next, (next, 0, vec![(0, 0); cap]));
        Some(self.next)
    }
    fn free_segment(&mut self, s: usize) {
        self.segs.remove(&s).unwrap();
    }
    fn text(&self, k: usize) -> &[u8] {
        &self.keys[&k]
    }
}

impl MapHash {
    fn data(&self, addr: usize) -> usize {
        let (s, i) = (addr / 1_000_000, (addr % 1_000_000 - 8) / 16);
        self.segs[&s].2[i].1
    }
}

#[test]
fn hash_keys_of_every_length_and_prefix_are_distinct() {
    // Clauses: s14_5_c_0
    let mut m = MapHash::default();
    let seg = m.alloc_segment(prime_at_least(2), 0).unwrap();
    m.set_head(seg);
    let mut g = Gen(9);
    let mut texts: Vec<Vec<u8>> = vec![b"".to_vec()];
    for len in 1..=80usize {
        let mut t: Vec<u8> = (0..len).map(|_| b'a' + (g.next() % 3) as u8).collect();
        texts.push(t.clone());
        t[len - 1] ^= 1;
        texts.push(t);
        texts.push(vec![b'x'; len]);
    }
    texts.sort();
    texts.dedup();
    let mut addr = Vec::new();
    for (i, t) in texts.iter().enumerate() {
        m.keys.insert(i + 1, t.clone());
        let a = hash_search(&mut m, i + 1, i * 3, true).unwrap().unwrap();
        addr.push(a);
    }
    let mut sorted = addr.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), texts.len(), "one entry per distinct text");
    for (i, t) in texts.iter().enumerate() {
        m.keys.insert(1_000 + i, t.clone());
        // A different pointer with equal text finds the entry; entering again keeps the first data.
        assert_eq!(hash_search(&mut m, 1_000 + i, 0, false).unwrap(), Some(addr[i]));
        assert_eq!(hash_search(&mut m, 1_000 + i, 999_999, true).unwrap(), Some(addr[i]));
        assert_eq!(m.data(addr[i]), i * 3, "an existing entry keeps its data");
    }
    m.keys.insert(5_000, b"zz-not-there".to_vec());
    assert_eq!(hash_search(&mut m, 5_000, 0, false).unwrap(), None);
    hash_destroy(&mut m);
    assert!(m.segs.is_empty());
}

#[test]
fn prime_sizes() {
    // Clauses: s14_5_i_0
    let mut want = vec![];
    for n in [0usize, 1, 2, 3, 4, 5, 8, 11, 12, 13, 100, 1000, 7919, 7920, 65_536] {
        want.push(prime_at_least(n));
    }
    assert_eq!(want, [3, 3, 3, 3, 5, 5, 11, 11, 13, 13, 101, 1009, 7919, 7927, 65_537]);
}

#[test]
fn queue_edges() {
    // Clauses: s14_6_c_0
    #[derive(Default)]
    struct L(HashMap<usize, (usize, usize)>);
    impl Links for L {
        fn next(&self, e: usize) -> usize {
            self.0[&e].0
        }
        fn prev(&self, e: usize) -> usize {
            self.0[&e].1
        }
        fn set_next(&mut self, e: usize, v: usize) {
            self.0.entry(e).or_default().0 = v
        }
        fn set_prev(&mut self, e: usize, v: usize) {
            self.0.entry(e).or_default().1 = v
        }
    }
    let mut l = L::default();
    queue_insert(&mut l, 1, 0);
    assert_eq!((l.next(1), l.prev(1)), (0, 0), "a new linear list");
    queue_insert(&mut l, 2, 1);
    queue_insert(&mut l, 3, 2);
    assert_eq!([l.next(1), l.next(2), l.next(3)], [2, 3, 0]);
    assert_eq!([l.prev(1), l.prev(2), l.prev(3)], [0, 1, 2]);
    queue_remove(&mut l, 1);
    assert_eq!((l.prev(2), l.next(2)), (0, 3), "head removed");
    queue_remove(&mut l, 3);
    assert_eq!(l.next(2), 0, "tail removed");
    queue_remove(&mut l, 2);
    // A circular element (next = prev = itself) removes cleanly.
    l.set_next(7, 7);
    l.set_prev(7, 7);
    queue_insert(&mut l, 8, 7);
    assert_eq!([l.next(7), l.next(8), l.prev(7), l.prev(8)], [8, 7, 8, 7]);
    queue_remove(&mut l, 8);
    assert_eq!((l.next(7), l.prev(7)), (7, 7));
}
