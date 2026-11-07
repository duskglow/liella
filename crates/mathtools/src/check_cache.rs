//! The verdict cache of `remez --check` (D-10.58).
//!
//! A check verdict is a pure function of the bytes the check reads: the `remez` executable and
//! every file mapped into its process (the shared objects it runs on), the fit file, the
//! coefficient file the fit names (or its absence), and the arguments that select the mode and
//! name the fit. [`check_key`] hashes all of them, length-prefixed and tagged, with SHA-256; a
//! fit that checks up to date stores its report under that key ([`Cache::store`]), and a later
//! check with the same key replays the report without fitting ([`Cache::lookup`]). A failing fit
//! stores nothing, so a failure is always recomputed. The generating mode never uses the cache.
//!
//! SHA-256 is implemented here from FIPS 180-4 (*Secure Hash Standard*, sections 4.1.2, 4.2.2,
//! 5.1.1, 5.3.3 and 6.2): the workspace has no external dependencies and `std` has no
//! collision-resistant hash; 256 bits make an accidental collision between two different input
//! sets negligible, which a 64-bit `std` hasher (`DefaultHasher`) would not.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The environment variable that turns the cache off (any value but empty or `0`).
pub const NO_CACHE_ENV: &str = "LIELLA_REMEZ_NO_CACHE";
/// The environment variable that moves the stamp directory (tests; default
/// `<target dir>/remez-check`).
pub const CACHE_DIR_ENV: &str = "LIELLA_REMEZ_CACHE_DIR";

/// FIPS 180-4 4.2.2: the first 32 bits of the fractional parts of the cube roots of the first
/// 64 primes.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// FIPS 180-4 5.3.3: the initial hash value.
const H0: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

/// An incremental SHA-256 (FIPS 180-4).
#[derive(Clone, Debug)]
pub struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    used: usize,
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A hash of the empty message so far.
    pub fn new() -> Self {
        Sha256 { h: H0, buf: [0; 64], used: 0, len: 0 }
    }

    fn block(&mut self, b: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (t, c) in b.chunks_exact(4).enumerate() {
            w[t] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
        }
        let [mut a, mut b2, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for t in 0..64 {
            let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(big_s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
            let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b2) ^ (a & c) ^ (b2 & c);
            let t2 = big_s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b2;
            b2 = a;
            a = t1.wrapping_add(t2);
        }
        for (hi, v) in self.h.iter_mut().zip([a, b2, c, d, e, f, g, h]) {
            *hi = hi.wrapping_add(v);
        }
    }

    /// Appends `data` to the message.
    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.used).min(data.len());
            self.buf[self.used..self.used + take].copy_from_slice(&data[..take]);
            self.used += take;
            data = &data[take..];
            if self.used == 64 {
                let b = self.buf;
                self.block(&b);
                self.used = 0;
            }
        }
    }

    /// The digest of the message (FIPS 180-4 5.1.1 padding).
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.used != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut out = [0u8; 32];
        for (o, h) in out.chunks_exact_mut(4).zip(self.h) {
            o.copy_from_slice(&h.to_be_bytes());
        }
        out
    }
}

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(data);
    s.finish()
}

/// Lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Everything one fit's check verdict depends on.
#[derive(Clone, Debug)]
pub struct KeyInputs<'a> {
    /// The process image: (path, SHA-256 of its bytes) of the running executable and of every
    /// file mapped into the process, from [`process_image`].
    pub image: &'a [(String, [u8; 32])],
    /// The mode argument (`--check`).
    pub mode: &'a str,
    /// The fit file argument as given on the command line.
    pub fit_arg: &'a str,
    /// The fit file as the generated header names it (relative to the workspace root).
    pub fit_display: &'a str,
    /// The fit file's bytes.
    pub fit: &'a [u8],
    /// The `output` path the fit names.
    pub coeff_path: &'a str,
    /// The coefficient file's bytes, `None` when it cannot be read.
    pub coeff: Option<&'a [u8]>,
}

fn field(s: &mut Sha256, tag: u8, bytes: &[u8]) {
    s.update(&[tag]);
    s.update(&(bytes.len() as u64).to_le_bytes());
    s.update(bytes);
}

/// The cache key of one fit's check: SHA-256 over every input, each tagged and
/// length-prefixed so that no two different input sets give the same byte stream.
pub fn check_key(k: &KeyInputs) -> String {
    let mut s = Sha256::new();
    field(&mut s, b'V', b"liella-remez-check-v1");
    for (path, digest) in k.image {
        field(&mut s, b'I', path.as_bytes());
        field(&mut s, b'i', digest);
    }
    field(&mut s, b'M', k.mode.as_bytes());
    field(&mut s, b'A', k.fit_arg.as_bytes());
    field(&mut s, b'D', k.fit_display.as_bytes());
    field(&mut s, b'F', k.fit);
    field(&mut s, b'P', k.coeff_path.as_bytes());
    match k.coeff {
        Some(c) => field(&mut s, b'C', c),
        None => field(&mut s, b'N', b""),
    }
    hex(&s.finish())
}

/// The process image the verdict depends on: the running executable (`current_exe`) and every
/// file mapped into this process (`/proc/self/maps`: the executable again and the shared
/// objects, whose code the arithmetic may call), each with the SHA-256 of its bytes, sorted by
/// path. Any read failure is an error (the caller then runs without the cache).
pub fn process_image() -> Result<Vec<(String, [u8; 32])>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut paths: Vec<String> = vec![exe.display().to_string()];
    let maps = std::fs::read_to_string("/proc/self/maps").map_err(|e| format!("/proc/self/maps: {e}"))?;
    for line in maps.lines() {
        // address perms offset dev inode pathname: the path is the sixth field and may hold spaces.
        let path = line.splitn(6, ' ').nth(5).map(str::trim_start).unwrap_or("");
        if path.starts_with('/') {
            if path.ends_with(" (deleted)") {
                return Err(format!("{path}: a mapped file was replaced while running"));
            }
            paths.push(path.to_string());
        }
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).map_err(|e| format!("{p}: {e}"))?;
            Ok((p, sha256(&bytes)))
        })
        .collect()
}

/// Whether [`NO_CACHE_ENV`] turns the cache off.
pub fn disabled() -> bool {
    std::env::var_os(NO_CACHE_ENV).is_some_and(|v| !v.is_empty() && v != "0")
}

/// The stamp directory: [`CACHE_DIR_ENV`], else `<target dir>/remez-check` beside the
/// profile directory the executable runs from (`target/release/remez` -> `target/remez-check`).
pub fn default_dir() -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os(CACHE_DIR_ENV) {
        if !d.is_empty() {
            return Ok(PathBuf::from(d));
        }
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    exe.parent().and_then(Path::parent).map(|t| t.join("remez-check")).ok_or_else(|| format!("{}: no target directory above it", exe.display()))
}

/// The first line of every stamp; the report follows it.
fn header(key: &str) -> String {
    format!("liella-remez-check v1 {key}\n")
}

static SEQ: AtomicUsize = AtomicUsize::new(0);

/// The verdict cache of one `remez --check` run.
#[derive(Debug)]
pub struct Cache {
    /// The stamp directory.
    pub dir: PathBuf,
    /// The process image, hashed once per run.
    pub image: Vec<(String, [u8; 32])>,
}

impl Cache {
    /// The cache in [`default_dir`] for this process.
    pub fn open() -> Result<Cache, String> {
        Ok(Cache { dir: default_dir()?, image: process_image()? })
    }

    /// The stored report of an up-to-date verdict under `key`, if there is one.
    pub fn lookup(&self, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.dir.join(key)).ok()?;
        text.strip_prefix(&header(key)).map(str::to_string)
    }

    /// Records an up-to-date verdict with its report (written to a temporary name, then renamed,
    /// so a reader never sees a partial stamp).
    pub fn store(&self, key: &str, report: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let tmp = self.dir.join(format!("{key}.tmp.{}.{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        let fin = self.dir.join(key);
        std::fs::write(&tmp, format!("{}{report}", header(key))).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &fin).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", fin.display())
        })
    }
}
