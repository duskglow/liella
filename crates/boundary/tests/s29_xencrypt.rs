// §29.1 ONC RPC DES key helpers (LG-16, D-29.1, owner ruling #1675): Liella against the reference, both ways.
//! `des_setparity`, `passwd2des`, `ecb_crypt`/`cbc_crypt` key handling and `xencrypt`/`xdecrypt`
//! byte for byte against the host C library's own versions: a small C oracle (built with `cc`,
//! symbols looked up with `dlvsym` at `GLIBC_2.2.5`) answers seeded random and edge cases, and
//! for each successful text call also runs the inverse on its own output (= Liella's output,
//! checked equal first): reference encrypts / Liella decrypts, Liella encrypts / reference
//! decrypts, and Liella alone. These are DES-based legacy functions kept for existing encrypted
//! keys; DES provides no meaningful security today.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, unreachable_pub)]

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::{Command, Stdio};

use liella_boundary::errno;
use liella_boundary::posix::s29_rpc_auth as a;

const ORACLE: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef int (*ecb_t)(char *, char *, unsigned, unsigned);
typedef int (*cbc_t)(char *, char *, unsigned, unsigned, char *);
typedef void (*par_t)(char *);
typedef void (*p2d_t)(char *, char *);
typedef int (*xc_t)(char *, char *);
static unsigned char A[20000], B[20000], C[20000];
static int unhex(const char *s, unsigned char *o) {
  int n = 0; unsigned v;
  if (*s == '-') return 0;
  while (s[0] && s[1]) { sscanf(s, "%2x", &v); o[n++] = (unsigned char)v; s += 2; }
  return n;
}
static void hex(const unsigned char *p, int n) { if (!n) putchar('-'); for (int i = 0; i < n; i++) printf("%02x", p[i]); }
static void *sym(const char *n) { void *p = dlvsym(RTLD_DEFAULT, n, "GLIBC_2.2.5"); if (!p) { printf("NOSYM %s\n", n); exit(3); } return p; }
int main(void) {
  ecb_t ecb = (ecb_t)sym("ecb_crypt"); cbc_t cbc = (cbc_t)sym("cbc_crypt"); par_t par = (par_t)sym("des_setparity");
  p2d_t p2d = (p2d_t)sym("passwd2des"); xc_t xenc = (xc_t)sym("xencrypt"), xdec = (xc_t)sym("xdecrypt");
  char *line = NULL; size_t cap = 0;
  while (getline(&line, &cap, stdin) > 0) {
    char *op = strtok(line, " \n"), *t1 = strtok(NULL, " \n"), *t2 = strtok(NULL, " \n"), *t3 = strtok(NULL, " \n"), *t4 = strtok(NULL, " \n");
    if (!op || !t1) return 4;
    if (*op == 'P') { unhex(t1, A); par((char *)A); hex(A, 8); }
    else if (*op == 'D') { int n = unhex(t1, A); A[n] = 0; memset(B, 0xAA, 8); p2d((char *)A, (char *)B); hex(B, 8); }
    else if (*op == 'E') {
      unhex(t1, A); int n = unhex(t3, B), cb = *t4 != '-'; unsigned mode = (unsigned)atoi(t2);
      if (cb) unhex(t4, C);
      int rc = cb ? cbc((char *)A, (char *)B, (unsigned)n, mode, (char *)C) : ecb((char *)A, (char *)B, (unsigned)n, mode);
      printf("%d ", rc); hex(B, n); putchar(' '); if (cb) hex(C, 8); else putchar('-'); putchar(' '); hex(A, 8);
    } else {
      int n = unhex(t1, A), m = unhex(t2, B), dec = *op == 'Y';
      A[n] = 0; memset(A + n + 1, 0x5A, 3); B[m] = 0;
      errno = 4321; int rc = (dec ? xdec : xenc)((char *)A, (char *)B); int e = errno;
      printf("%d %d ", rc, e); hex(A, n + 4);
      if (rc == 1) {
        int len = (int)strlen((char *)A); memcpy(C, A, (size_t)len); C[len] = 0; memset(C + len + 1, 0x5A, 3);
        errno = 4321; int r2 = (dec ? xenc : xdec)((char *)C, (char *)B); e = errno;
        printf(" %d %d ", r2, e); hex(C, len + 4);
      } else printf(" - - -");
    }
    putchar('\n');
  }
  return 0;
}
"#;

fn hx(b: &[u8]) -> String {
    if b.is_empty() {
        return "-".into();
    }
    b.iter().fold(String::with_capacity(b.len() * 2), |mut s, v| {
        write!(s, "{v:02x}").unwrap();
        s
    })
}

/// The reference's answer line for each request line.
fn reference(requests: &str) -> Vec<String> {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let (src, exe) = (dir.join("s29_xencrypt_oracle.c"), dir.join("s29_xencrypt_oracle"));
    std::fs::write(&src, ORACLE).unwrap();
    let st = Command::new("cc").arg("-O1").arg("-o").arg(&exe).arg(&src).status().unwrap();
    assert!(st.success(), "building the reference oracle (needs cc and a glibc host)");
    let mut child = Command::new(&exe).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = requests.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(out.status.success(), "reference oracle failed: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect()
}

/// xorshift64*, seeded: the same cases on every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> usize {
        (self.next() % n) as usize
    }
    fn bytes8(&mut self) -> [u8; 8] {
        self.next().to_be_bytes()
    }
}

fn check(requests: &[String], mine: &[String]) {
    let theirs = reference(&requests.concat());
    assert_eq!(theirs.len(), mine.len(), "one reference answer per case");
    for ((req, m), t) in requests.iter().zip(mine).zip(&theirs) {
        assert_eq!(m, t, "case {}", req.trim_end());
    }
}

#[test]
fn setparity_and_passwd2des() {
    // Trace: no spec clause (LG-16 des_setparity/passwd2des match the reference, owner ruling #1675).
    let mut rng = Rng(0x5EED_0001);
    let mut keys: Vec<[u8; 8]> = (0..=255u8).map(|v| [v, !v, v ^ 0x80, v, 0, 0xFF, v.rotate_left(3), v]).collect();
    keys.extend((0..4000).map(|_| rng.bytes8()));
    let mut pws: Vec<Vec<u8>> = vec![vec![], b"a".to_vec(), b"abcdefgh".to_vec(), b"abcdefghi".to_vec(), vec![0xFF; 300], vec![0x80], vec![0x7F, 0x40, 0x01]];
    pws.extend((1..=255u8).map(|v| vec![v; 1 + (v as usize % 9)]));
    pws.extend((0..4000).map(|_| {
        let n = rng.below(20);
        (0..n).map(|_| 1 + rng.below(255) as u8).collect()
    }));
    let (mut req, mut mine) = (Vec::new(), Vec::new());
    for k in keys {
        let mut y = k;
        a::liella_compat_des_setparity(y.as_mut_ptr().cast());
        req.push(format!("P {}\n", hx(&k)));
        mine.push(hx(&y));
    }
    for pw in pws {
        let mut c = pw.clone();
        c.push(0);
        let mut y = [0x55u8; 8];
        a::liella_compat_passwd2des(c.as_mut_ptr().cast(), y.as_mut_ptr().cast());
        req.push(format!("D {}\n", hx(&pw)));
        mine.push(hx(&y));
    }
    check(&req, &mine);
}

#[test]
fn ecb_cbc_key_handling() {
    // Trace: no spec clause (LG-16 ecb_crypt/cbc_crypt ignore the key bits the reference ignores).
    let mut rng = Rng(0x5EED_0003);
    let (mut req, mut mine) = (Vec::new(), Vec::new());
    for i in 0..3000 {
        let mut key = rng.bytes8();
        if i % 3 == 0 {
            key[rng.below(8)] ^= 1 << rng.below(8);
        }
        let data: Vec<u8> = (0..1 + rng.below(4)).flat_map(|_| rng.bytes8()).collect();
        let iv = rng.bytes8();
        for mode in [2u32, 3, 0, 1] {
            for cb in [false, true] {
                let (mut k, mut d, mut v) = (key, data.clone(), iv);
                let len = d.len() as u32;
                let rc = if cb {
                    a::liella_compat_cbc_crypt(k.as_mut_ptr().cast(), d.as_mut_ptr().cast(), len, mode, v.as_mut_ptr().cast())
                } else {
                    a::liella_compat_ecb_crypt(k.as_mut_ptr().cast(), d.as_mut_ptr().cast(), len, mode)
                };
                req.push(format!("E {} {mode} {} {}\n", hx(&key), hx(&data), if cb { hx(&iv) } else { "-".into() }));
                mine.push(format!("{rc} {} {} {}", hx(&d), if cb { hx(&v) } else { "-".into() }, hx(&k)));
            }
        }
    }
    check(&req, &mine);
}

const HEXL: &[u8] = b"0123456789abcdef";
const HEXU: &[u8] = b"0123456789ABCDEF";

/// Liella's call on `text` + NUL + three spare bytes: (return value, errno, the whole buffer).
fn liella_x(text: &[u8], pw: &[u8], decrypt: bool) -> (i32, i32, Vec<u8>) {
    let mut buf = text.to_vec();
    buf.extend_from_slice(&[0, 0x5A, 0x5A, 0x5A]);
    let mut p = pw.to_vec();
    p.push(0);
    errno::set_raw(4321);
    let rc = if decrypt { a::liella_compat_xdecrypt(buf.as_mut_ptr().cast(), p.as_mut_ptr().cast()) } else { a::liella_compat_xencrypt(buf.as_mut_ptr().cast(), p.as_mut_ptr().cast()) };
    (rc, errno::get(), buf)
}

#[test]
fn xencrypt_xdecrypt_both_ways() {
    // Trace: no spec clause (LG-16 xencrypt/xdecrypt match the reference bit for bit, owner ruling #1675).
    let mut rng = Rng(0x5EED_0004);
    let mut cases: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    // Edge cases: empty, odd lengths, lengths that are not whole blocks, the maximum and beyond.
    for len in [0usize, 1, 2, 7, 8, 15, 16, 17, 18, 24, 31, 32, 33, 47, 48, 16383, 16384, 16385, 16386, 16400] {
        cases.push(((0..len).map(|i| HEXL[i % 16]).collect(), b"pw".to_vec()));
    }
    // Every byte value as a high and as a low digit; non-ASCII and long passwords.
    for v in 1..=255u8 {
        let mut t = b"0123456789abcdef".to_vec();
        t[4] = v;
        t[9] = v;
        cases.push((t, vec![v]));
    }
    cases.push((b"0123456789abcdef".to_vec(), vec![]));
    cases.push((b"0123456789abcdef".to_vec(), vec![0xFF, 0x80, b'p', 0x81]));
    cases.push((b"0123456789abcdef".to_vec(), vec![b'z'; 500]));
    for _ in 0..4000 {
        let mut len = rng.below(6) * 16;
        if rng.below(5) == 0 {
            len += rng.below(16);
        }
        let style = rng.below(10);
        let text: Vec<u8> = (0..len)
            .map(|_| match style {
                0 => 1 + rng.below(255) as u8,
                1 => HEXU[rng.below(16)],
                2 if rng.below(8) == 0 => 0x80 | rng.below(128) as u8,
                _ => HEXL[rng.below(16)],
            })
            .collect();
        let pw: Vec<u8> = (0..rng.below(14)).map(|_| if rng.below(4) == 0 { 1 + rng.below(255) as u8 } else { b'a' + rng.below(26) as u8 }).collect();
        cases.push((text, pw));
    }
    let (mut req, mut mine) = (Vec::new(), Vec::new());
    let mut ok = 0usize;
    for (text, pw) in &cases {
        for decrypt in [false, true] {
            let (rc, e, buf) = liella_x(text, pw, decrypt);
            let mut line = format!("{rc} {e} {}", hx(&buf));
            if rc == 1 {
                ok += 1;
                // The inverse on the output: Liella here, the reference in the oracle line.
                let out = &buf[..buf.iter().position(|&b| b == 0).unwrap()];
                let (r2, e2, back) = liella_x(out, pw, !decrypt);
                write!(line, " {r2} {e2} {}", hx(&back)).unwrap();
                if text.len() % 2 == 0 && text.iter().all(|c| HEXL.contains(c)) {
                    assert_eq!(&back[..text.len()], &text[..], "Liella round trip, pw {pw:02x?}");
                }
            } else {
                assert_eq!(&buf[..text.len()], &text[..], "a failure leaves the text untouched");
                line.push_str(" - - -");
            }
            req.push(format!("{} {} {}\n", if decrypt { 'Y' } else { 'X' }, hx(text), hx(pw)));
            mine.push(line);
        }
    }
    assert!(ok > 6000, "enough successful cases ({ok})");
    check(&req, &mine);
}
