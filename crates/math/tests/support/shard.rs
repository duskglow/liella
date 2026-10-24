//! Sharding of the products tests (GATE-MATH-SHARD): one test's printed lines are split by line
//! index (line `i` goes to shard `i % SHARDS`) over `SHARDS` independent `#[test]`s, so the
//! harness runs them on separate cores. Each shard checks that it took exactly its share of the
//! lines; the shares of shards `0..SHARDS` sum to the whole, so together they check the same
//! lines, modes and rules as the single test did, only partitioned.
//!
//! The D-10.3 tallies of the complex tests are written per shard as raw partial counts and
//! merged (counts summed, largest errors maximised) into the one table the unsharded test wrote,
//! by whichever shard finishes last.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

/// The number of shards of each products test.
pub(crate)const SHARDS: usize = 8;

/// Whether line `i` belongs to shard `shard`.
pub(crate)fn mine(i: usize, shard: usize) -> bool {
    i % SHARDS == shard
}

/// Shard `shard`'s share of `len` lines: ceil((len - shard) / SHARDS).
pub(crate)fn share(len: usize, shard: usize) -> usize {
    (len + SHARDS - 1 - shard) / SHARDS
}

/// The case-count check: shard `shard` took exactly its share of the `len` lines.
pub(crate)fn check_share(name: &str, shard: usize, taken: usize, len: usize) {
    let want = share(len, shard);
    eprintln!("{name} shard {shard}/{SHARDS}: {taken} of {len} lines");
    assert_eq!(taken, want, "{name} shard {shard}/{SHARDS}: checked {taken} lines, its share of {len} is {want}");
}

/// The threads of the oracle pool of one shard of the pooled products tests (special_ld,
/// special_f128) under nextest: the `threads-required` `.config/nextest.toml` gives those tests,
/// so nextest counts each pool against its test threads and starts no more of them than the
/// slot holds (GATE-THREADS-REQUIRED: 16 shards x 8 threads oversubscribed a 12-CPU CI slot).
/// Change both together; `pool_matches_the_nextest_config` (special_ld_products) checks it.
pub(crate)const POOL: usize = 4;

/// The pooled shards running in this process now (`cargo test` runs them as threads of one
/// process; nextest runs each in a process of its own).
static RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One pooled shard running in this process, from its start to its end (drop).
pub(crate)struct Running(());

/// Marks a pooled shard as running in this process until the value is dropped.
pub(crate)fn running() -> Running {
    RUNNING.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Running(())
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The oracle pool size of one shard, from the CPUs this process may use
/// (`available_parallelism` follows the affinity mask and the cgroup CPU quota): under nextest
/// (one process per test) the `POOL` threads nextest reserved for it, never more than those CPUs;
/// under `cargo test` (the shards are threads of one process) those CPUs divided among the pooled
/// shards running in this process, so one shard run alone uses every CPU it may.
pub(crate)fn workers() -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    if std::env::var_os("NEXTEST_EXECUTION_MODE").is_some() || std::env::var_os("NEXTEST_RUN_ID").is_some() {
        cpus.min(POOL)
    } else {
        (cpus / RUNNING.load(std::sync::atomic::Ordering::SeqCst).max(1)).max(1)
    }
}

/// The D-10.3 counts of one function (the complex tests).
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub(crate)struct Tally {
    pub(crate)lines: usize,
    pub(crate)identical: usize,
    pub(crate)identical_other_flags: usize,
    pub(crate)liella_cr: usize,
    pub(crate)glibc_cr: usize,
    pub(crate)glibc_only_wrong: usize,
    pub(crate)liella_only_wrong: usize,
    pub(crate)neither: usize,
    pub(crate)undecided: usize,
    pub(crate)glibc_max_ulp: f64,
    pub(crate)glibc_infinite: usize,
    pub(crate)liella_max_ulp: f64,
}

impl Tally {
    fn add(&mut self, o: &Self) {
        self.lines += o.lines;
        self.identical += o.identical;
        self.identical_other_flags += o.identical_other_flags;
        self.liella_cr += o.liella_cr;
        self.glibc_cr += o.glibc_cr;
        self.glibc_only_wrong += o.glibc_only_wrong;
        self.liella_only_wrong += o.liella_only_wrong;
        self.neither += o.neither;
        self.undecided += o.undecided;
        self.glibc_max_ulp = self.glibc_max_ulp.max(o.glibc_max_ulp);
        self.glibc_infinite += o.glibc_infinite;
        self.liella_max_ulp = self.liella_max_ulp.max(o.liella_max_ulp);
    }

    fn line_of(&self, func: &str) -> String {
        let t = self;
        format!(
            "{func}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\t{:016x}",
            t.lines,
            t.identical,
            t.identical_other_flags,
            t.liella_cr,
            t.glibc_cr,
            t.glibc_only_wrong,
            t.liella_only_wrong,
            t.neither,
            t.undecided,
            t.glibc_infinite,
            t.glibc_max_ulp.to_bits(),
            t.liella_max_ulp.to_bits()
        )
    }

    fn from_line(l: &str) -> Option<(String, Self)> {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 13 {
            return None;
        }
        let n = |i: usize| f[i].parse::<usize>().ok();
        let x = |i: usize| u64::from_str_radix(f[i], 16).ok().map(f64::from_bits);
        let t = Self {
            lines: n(1)?,
            identical: n(2)?,
            identical_other_flags: n(3)?,
            liella_cr: n(4)?,
            glibc_cr: n(5)?,
            glibc_only_wrong: n(6)?,
            liella_only_wrong: n(7)?,
            neither: n(8)?,
            undecided: n(9)?,
            glibc_infinite: n(10)?,
            glibc_max_ulp: x(11)?,
            liella_max_ulp: x(12)?,
        };
        Some((f[0].to_string(), t))
    }
}

/// An identifier of one run's inputs (both programs' outputs): partial tallies of another tree's
/// run are never merged with this one's.
pub(crate)fn run_id(want: &str, got: &str) -> u64 {
    let mut h = DefaultHasher::new();
    want.hash(&mut h);
    got.hash(&mut h);
    h.finish()
}

fn write_atomic(path: &Path, text: &str) {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

/// Write shard `shard`'s partial tallies as `<stem>.s<shard>.part` in `dir`; when the partials of
/// every shard of this run (`id`) are present, return their merge (the whole run's tallies).
pub(crate)fn merge_tally(dir: &Path, stem: &str, shard: usize, id: u64, tally: &BTreeMap<String, Tally>) -> Option<BTreeMap<String, Tally>> {
    let mut text = format!("run {id:016x}\n");
    for (f, t) in tally {
        text.push_str(&t.line_of(f));
        text.push('\n');
    }
    write_atomic(&dir.join(format!("{stem}.s{shard}.part")), &text);
    let header = format!("run {id:016x}");
    let mut all: BTreeMap<String, Tally> = BTreeMap::new();
    for k in 0..SHARDS {
        let part = std::fs::read_to_string(dir.join(format!("{stem}.s{k}.part"))).ok()?;
        let mut lines = part.lines();
        if lines.next() != Some(header.as_str()) {
            return None;
        }
        for l in lines {
            let (f, t) = Tally::from_line(l).unwrap_or_else(|| panic!("{stem}.s{k}.part: bad line {l}"));
            all.entry(f).or_default().add(&t);
        }
    }
    Some(all)
}

/// Write `text` to `path` atomically (shards finishing together may write the same table).
pub(crate)fn write_table(path: &Path, text: &str) {
    write_atomic(path, text);
}
