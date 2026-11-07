//! Verdict cache of the math accuracy sweeps (TVA-MATH-VERDICT-CACHE, D-10.57): a shard whose
//! complete input key equals the key of its last green run prints `verdict cached: <name> <key>`
//! and returns without checking; any other shard runs as it always did and records its key only
//! after it passed. A cached verdict is therefore only ever a green one for identical inputs.
//!
//! The key ([`Key`]) is the content of everything a shard's outcome depends on:
//! - always: the test binary's own bytes (`/proc/self/exe`, the image this process runs: the
//!   checker, the mathtools oracle and, for the pure-Rust sweeps, every Liella byte it calls),
//!   the CPU feature flags the dispatch reads (the `flags` line of `/proc/cpuinfo`), the kernel
//!   release, and the shard's name;
//! - the C-oracle sweeps add the shard index and count, the bytes of both built programs (the
//!   static Liella program holds every Liella byte it runs) and of every shared object the
//!   reference program loads (resolved by the dynamic loader itself, `LD_TRACE_LOADED_OBJECTS`:
//!   the host's libm.so.6, libc.so.6 and the loader; `Key::loaded_by` in `verdict_programs.rs`);
//! - a sweep whose size an environment variable sets adds that variable ([`Key::env`]).
//!
//! An input that cannot be read makes the key incomplete: such a shard always runs and records
//! nothing. Each file is hashed by two SipHash streams with different prefixes (128 bits) and
//! its length. The records are `target/test-verdicts/<name>`, one file per shard, written to a
//! temporary name and renamed, so they stay per tree and per host. `LIELLA_NO_VERDICT_CACHE=1`
//! runs every shard (the nightly and release runs); a shard that runs removes its record first,
//! so a failing shard leaves none.
//!
//! Use: `let Some(v) = key.begin() else { return };` before the check, `v.pass();` after its
//! last assertion. Included with `#[path]` by the sweep tests. This module runs no program and
//! touches no C code, so a pure-Rust sweep that includes it stays pure Rust (xtask
//! `nextest::PASS0_INCLUDES` scans it); what starts a program is in `verdict_programs.rs`,
//! which only the C-oracle sweeps include.
#![allow(dead_code)]

use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Lists the files that make up an input (for example the shared objects a program loads);
/// `None` when they cannot be resolved.
pub(crate) type Resolve = fn(&Path) -> Option<Vec<PathBuf>>;

/// The environment variable that runs every shard, cached or not.
pub(crate) const NO_CACHE: &str = "LIELLA_NO_VERDICT_CACHE";

/// Where the per-host inputs come from (the real files, or a test's stand-ins).
#[derive(Clone, Debug)]
pub(crate) struct Sources {
    /// The test binary.
    pub(crate) exe: PathBuf,
    /// The CPU description holding the `flags` line.
    pub(crate) cpuinfo: PathBuf,
    /// The kernel release.
    pub(crate) osrelease: PathBuf,
}

impl Sources {
    /// The running process's image, `/proc/cpuinfo` and the kernel release.
    pub(crate) fn host() -> Self {
        Self { exe: PathBuf::from("/proc/self/exe"), cpuinfo: PathBuf::from("/proc/cpuinfo"), osrelease: PathBuf::from("/proc/sys/kernel/osrelease") }
    }
}

/// One input added to a key after the host's (kept so the key can be read again).
#[derive(Clone, Debug)]
enum Input {
    Shard(usize, usize),
    File(PathBuf),
    /// The files `resolve` lists for `path`, under `label`.
    Resolved(&'static str, PathBuf, Resolve),
    Env(String),
}

/// The complete input key of one shard: one line per input (label, length, 128-bit content hash).
#[derive(Clone, Debug)]
pub(crate) struct Key {
    name: String,
    src: Sources,
    dir: PathBuf,
    inputs: Vec<Input>,
    lines: Vec<String>,
    complete: bool,
}

/// Two keys are equal when they read the same inputs with the same contents.
impl PartialEq for Key {
    fn eq(&self, o: &Self) -> bool {
        self.complete == o.complete && self.lines == o.lines
    }
}

/// The 128-bit content hash of `bytes`.
fn digest(bytes: &[u8]) -> String {
    let mut a = DefaultHasher::new();
    let mut b = DefaultHasher::new();
    a.write_u8(0x5a);
    b.write_u8(0xa5);
    a.write(bytes);
    b.write(bytes);
    format!("{:016x}{:016x}", a.finish(), b.finish())
}

/// The digest of the running image, computed once per process (it cannot change under it).
fn own_image() -> Option<&'static (usize, String)> {
    static IMAGE: OnceLock<Option<(usize, String)>> = OnceLock::new();
    IMAGE.get_or_init(|| std::fs::read("/proc/self/exe").ok().map(|b| (b.len(), digest(&b)))).as_ref()
}

/// Writes `line` straight to the process's stderr: libtest captures `eprintln!` of a passing test
/// and hides it, so the log would not say that a cached shard did not run (as `skip_note`).
fn say(line: &str) {
    use std::io::Write as _;
    let _ = std::io::stderr().write_all(line.as_bytes());
}

/// The `flags` line of a `/proc/cpuinfo` text (the first; every CPU of the host lists the same).
pub(crate) fn cpu_flags(cpuinfo: &str) -> Option<&str> {
    cpuinfo.lines().find(|l| l.starts_with("flags")).map(str::trim_end)
}

impl Key {
    /// The key of shard `name` (unique per test binary and shard) from the host's inputs.
    pub(crate) fn new(name: &str) -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-verdicts");
        Self::with_sources(name, &Sources::host(), &dir)
    }

    /// The key of shard `name` from `src`, recorded in `dir`.
    pub(crate) fn with_sources(name: &str, src: &Sources, dir: &Path) -> Self {
        let mut k = Self { name: name.to_string(), src: src.clone(), dir: dir.to_path_buf(), inputs: Vec::new(), lines: vec![format!("name\t{name}")], complete: true };
        let image = if src.exe == Path::new("/proc/self/exe") { own_image().cloned() } else { std::fs::read(&src.exe).ok().map(|b| (b.len(), digest(&b))) };
        match image {
            Some((len, d)) => k.lines.push(format!("test binary\t{len}\t{d}")),
            None => k.complete = false,
        }
        match std::fs::read_to_string(&src.cpuinfo).ok().as_deref().and_then(cpu_flags) {
            Some(f) => k.lines.push(format!("cpu\t{f}")),
            None => k.complete = false,
        }
        match std::fs::read_to_string(&src.osrelease) {
            Ok(r) => k.lines.push(format!("kernel\t{}", r.trim())),
            Err(_) => k.complete = false,
        }
        k
    }

    /// Reads input `i` into the key.
    fn add(mut self, i: Input) -> Self {
        match &i {
            Input::Shard(k, n) => self.lines.push(format!("shard\t{k}/{n}")),
            Input::File(path) => self.add_file(path),
            Input::Resolved(label, path, resolve) => match resolve(path) {
                Some(objs) => {
                    self.lines.push(format!("{label}\t{}", path.display()));
                    for p in &objs {
                        self.add_file(p);
                    }
                }
                None => self.complete = false,
            },
            Input::Env(var) => self.lines.push(format!("env\t{var}\t{:?}", std::env::var_os(var))),
        }
        self.inputs.push(i);
        self
    }

    fn add_file(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(b) => self.lines.push(format!("file\t{}\t{}\t{}", path.display(), b.len(), digest(&b))),
            Err(_) => self.complete = false,
        }
    }

    /// The key read again from the same inputs now.
    pub(crate) fn reread(&self) -> Self {
        let fresh = Self::with_sources(&self.name, &self.src, &self.dir);
        self.inputs.iter().cloned().fold(fresh, Self::add)
    }

    /// Adds shard `k` of `n`.
    #[must_use]
    pub(crate) fn shard(self, k: usize, n: usize) -> Self {
        self.add(Input::Shard(k, n))
    }

    /// Adds the bytes of `path` (a built program, a library).
    #[must_use]
    pub(crate) fn file(self, path: &Path) -> Self {
        self.add(Input::File(path.to_path_buf()))
    }

    /// Adds `label` and `path`, then the bytes of every file `resolve` lists for `path` (read
    /// again by [`Key::reread`]); the key is incomplete if they cannot be resolved.
    #[must_use]
    pub(crate) fn resolved(self, label: &'static str, path: &Path, resolve: Resolve) -> Self {
        self.add(Input::Resolved(label, path.to_path_buf(), resolve))
    }

    /// Adds the value of environment variable `var` (unset is a value of its own).
    #[must_use]
    pub(crate) fn env(self, var: &str) -> Self {
        self.add(Input::Env(var.to_string()))
    }

    /// Whether every input could be read (an incomplete key is never cached nor recorded).
    pub(crate) fn complete(&self) -> bool {
        self.complete
    }

    /// The key as recorded: one input per line.
    pub(crate) fn text(&self) -> String {
        let mut t = self.lines.join("\n");
        t.push('\n');
        t
    }

    /// A short form of the key for the log.
    pub(crate) fn id(&self) -> String {
        digest(self.text().as_bytes())[..16].to_string()
    }

    fn record_path(&self) -> PathBuf {
        self.dir.join(&self.name)
    }

    /// Whether a green verdict for exactly this key is on record.
    pub(crate) fn cached(&self) -> bool {
        self.complete && std::fs::read_to_string(self.record_path()).is_ok_and(|r| r == self.text())
    }

    /// Records a green verdict for this key (temporary file, then rename).
    pub(crate) fn record(&self) {
        if !self.complete || std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.record_path();
        let tmp = self.dir.join(format!(".{}.tmp{}", self.name, std::process::id()));
        if std::fs::write(&tmp, self.text()).is_err() || std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Removes this shard's record, whatever key it holds.
    pub(crate) fn forget(&self) {
        let _ = std::fs::remove_file(self.record_path());
    }

    /// The start of a shard: `None` (after printing `verdict cached: <name> <key>`) when a green
    /// verdict for this key is on record and `LIELLA_NO_VERDICT_CACHE` is not `1`; otherwise the
    /// record is removed and the shard runs, calling [`Run::pass`] after its last check.
    pub(crate) fn begin(self) -> Option<Run> {
        let forced = std::env::var_os(NO_CACHE).is_some_and(|v| v == "1");
        self.begin_with(forced)
    }

    /// [`Key::begin`] with the override given (`forced` runs the shard whatever is on record).
    pub(crate) fn begin_with(self, forced: bool) -> Option<Run> {
        if !forced && self.cached() {
            say(&format!("verdict cached: {} {}\n", self.name, self.id()));
            return None;
        }
        // Before the check: a shard that fails (panics) leaves no record.
        self.forget();
        Some(Run { key: self })
    }
}

/// A shard that is running: [`Run::pass`] records its verdict.
#[derive(Debug)]
pub(crate) struct Run {
    key: Key,
}

impl Run {
    /// The shard passed: records the key, read again now, if no input changed during the run
    /// (a program rebuilt meanwhile would otherwise get the verdict of the one checked).
    pub(crate) fn pass(self) {
        let now = self.key.reread();
        self.pass_with(&now);
    }

    /// [`Run::pass`] with the key as read again at the end of the run given.
    pub(crate) fn pass_with(self, now: &Key) {
        if *now == self.key {
            self.key.record();
        } else {
            say(&format!("verdict not recorded: {}: an input changed during the run\n", self.key.name));
        }
    }
}
