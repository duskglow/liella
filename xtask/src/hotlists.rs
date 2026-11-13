//! The generated start-up lists are train-owned (HOTLIST-TRAIN-REGEN, records/pending/HOTLIST-TRAIN-REGEN-2-1.toml).
//!
//! [`GENERATED`] (`hot-text.list`, `startup-text.keys`, `probe-text.keys`, `startup-data.keys` under
//! `crates/libc-so/`) are written by `tools/hottext/regen.sh` from gdb traces of the built products. The
//! hand-written inputs next to them (`hot-text.clusters`, `hot-text.absent`, `hot-text.unpinned`,
//! `startup-state.list`) stay on story branches and are always read from the tracked tree.
//!
//! `cargo xtask hot-lists` regenerates the four for both profiles (`regen.sh . --both --allow-drop`, merge mode
//! #4839: the tracked copies are the merge base) into [`OUT`] and writes [`STAMP`] there: the [`tree_hash`] of
//! the tree they were generated for. `--in-place` (or `LIELLA_TRAIN=1`, the train) rewrites the tracked copies
//! instead, and keeps a copy and the stamp in [`OUT`].
//!
//! Every reader (the `so` grouping, the start-up read-only group, the layout contracts, `hottext-recheck`,
//! `hottext-repair`, static's `hottext` check) asks [`path`] where to read a generated list:
//! * `LIELLA_HOT_LISTS=<dir>` (set by `hot-lists` for its own regen run, so the regen's builds read the lists it
//!   is rewriting): that directory;
//! * off the train, when [`OUT`] holds a stamp for this tree: [`OUT`], with one info line naming the files that
//!   differ from the tracked copies (a branch never commits them; the train regenerates them);
//! * on the train (`LIELLA_TRAIN=1`): the tracked copies, and a tracked copy that differs from a stamped
//!   regeneration of this tree is an error (the only check that moved to the train: "the committed file is stale");
//! * otherwise the tracked copies.
//!
//! Every real check still runs on the copy that is read: a dead key, an unpinned key, a broken layout contract and
//! the page-fault bounds fail on a branch as before.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The tracked directory of the lists, relative to the workspace root.
pub const TRACKED: &str = "crates/libc-so";
/// Where `cargo xtask hot-lists` writes them off the train, relative to the workspace root.
pub const OUT: &str = "target/hot-lists";
/// The generated (train-owned) lists. Everything else in [`TRACKED`] is a hand-written input.
pub const GENERATED: [&str; 4] = ["hot-text.list", "startup-text.keys", "probe-text.keys", "startup-data.keys"];
/// The file in [`OUT`] that names the tree state its lists were generated for (one line, [`tree_hash`]).
pub const STAMP: &str = "STAMP";
/// The override `hot-lists` sets for its regen run: the directory the lists are read from and written to.
pub const ENV: &str = "LIELLA_HOT_LISTS";

/// The CI train's gate (`LIELLA_TRAIN=1`), where a stale tracked list is fatal.
pub fn train() -> bool {
    std::env::var("LIELLA_TRAIN").is_ok_and(|v| v == "1")
}

/// The state of everything a regen's output depends on, as a string: an FNV-1a hash over the relative path and the
/// bytes of `Cargo.lock` and every file under `crates/`, `csys/`, `include/`, `layout/` and `tools/hottext/` (no
/// `target` directory), the [`GENERATED`] lists left out (they are the output; the train rewrites them).
pub fn tree_hash(root: &Path) -> String {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "target") {
                    walk(&p, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    let mut files = vec![root.join("Cargo.lock")];
    for d in ["crates", "csys", "include", "layout", "tools/hottext"] {
        walk(&root.join(d), &mut files);
    }
    let generated: Vec<PathBuf> = GENERATED.iter().map(|g| root.join(TRACKED).join(g)).collect();
    files.retain(|f| !generated.contains(f));
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes.iter().chain(&[0u8]) {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for f in &files {
        eat(f.strip_prefix(root).unwrap_or(f).to_string_lossy().as_bytes());
        eat(&fs::read(f).unwrap_or_default());
    }
    format!("tree-{h:016x}")
}

/// The [`GENERATED`] lists whose bytes differ between directories `a` and `b` (a missing file differs).
pub fn differing(a: &Path, b: &Path) -> Vec<&'static str> {
    GENERATED.iter().copied().filter(|g| fs::read(a.join(g)).ok() != fs::read(b.join(g)).ok()).collect()
}

/// LAYOUT-TRACE-AT-BUILD (LT-1, rulings #5190/#5191) and LAYOUT-TRACE-DEFAULT: the layout switch of `cargo xtask
/// so`. Unset, empty or `1`: the build links the probes against the archive it built, traces them and groups from
/// that trace ([`Layout::Traced`], the default of every product build since LAYOUT-TRACE-DEFAULT); `ungrouped`: the
/// explicit opt-out for a builder without a tracer, an ungrouped product ([`Layout::Ungrouped`]); `0`: the committed
/// lists ([`Layout::Committed`]), kept for the list tooling (regen, repair, bench comparisons) until LT-5 deletes
/// those lists. A regen build ([`crate::hottext::REGEN_FLAG`]) reads the committed lists whatever the default.
pub const TRACE_ENV: &str = "LIELLA_LAYOUT_TRACE";
/// The marker `so` writes next to products it grouped from a trace of their own build (or left ungrouped): line 1
/// the directory of the lists it used (`target/product/<profile>`), then `<list> <hash>` per list (so the layout
/// verdict cache, which hashes the products directory, sees a list change). Every later reader of the generated
/// lists (`layout-check`, the tests) reads that directory. A build from the committed lists removes it first.
pub const MARKER: &str = ".hot-lists";

/// Where the start-up layout of a `cargo xtask so` comes from ([`TRACE_ENV`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The committed (or train-regenerated) lists: regen builds, and `0` (list tooling until LT-5).
    Committed,
    /// A trace of the probes linked against this build's un-grouped archive: the default.
    Traced,
    /// No grouping at all (no tracer on this builder; explicit).
    Ungrouped,
}

impl Layout {
    /// The layout `value` of [`TRACE_ENV`] selects for a product build; `regen`: the build is a regen build
    /// ([`crate::hottext::REGEN_FLAG`]), which traces nothing itself and reads the committed lists (it is what
    /// `tools/hottext/regen.sh` traces), so with it only an unset (or empty) switch or `0` is accepted.
    pub fn parse(value: Option<&str>, regen: bool) -> Result<Layout, String> {
        let layout = match value.unwrap_or("") {
            "" if regen => return Ok(Layout::Committed),
            "" | "1" => Layout::Traced,
            "0" => Layout::Committed,
            "ungrouped" => Layout::Ungrouped,
            v => return Err(format!("{TRACE_ENV}={v:?}: want unset or 1 (trace at build, the default), ungrouped (no tracer: ungrouped product) or 0 (the committed lists: list tooling until LT-5)")),
        };
        if regen && layout != Layout::Committed {
            return Err(format!("so: {TRACE_ENV}={} and {} together: a regen build reads the committed lists", value.unwrap_or(""), crate::hottext::REGEN_FLAG));
        }
        Ok(layout)
    }

    /// This process's layout ([`TRACE_ENV`]) for a product build (`regen`: see [`Layout::parse`]).
    pub fn from_env(regen: bool) -> Result<Layout, String> {
        Layout::parse(std::env::var(TRACE_ENV).ok().as_deref(), regen)
    }
}

/// The lists directory a trace-at-build `so` run uses from its start ([`force_dir`]).
static FORCED: OnceLock<PathBuf> = OnceLock::new();

/// Make `dir` the directory of the generated lists for the rest of this process (a trace-at-build `so`, before any
/// reader resolves them). Fails when a reader already resolved another directory or [`ENV`] is set.
pub fn force_dir(root: &Path, dir: &Path) -> Result<(), String> {
    if Mode::from_env().over.is_some() {
        return Err(format!(
            "{ENV} is set, but this build writes its own lists ({TRACE_ENV} unset or 1: trace at build, the default; or ungrouped): unset {ENV}, or set {TRACE_ENV}=0 to build from the lists it names"
        ));
    }
    FORCED.set(dir.to_path_buf()).map_err(|_| "hot-lists: the lists directory was forced twice".to_string())?;
    match self::dir(root) {
        Ok(d) if d == dir => Ok(()),
        Ok(d) => Err(format!("hot-lists: {} resolved before the trace-at-build directory {}", d.display(), dir.display())),
        Err(e) => Err(e),
    }
}

/// The lists directory named by the [`MARKER`] of the products in `products` (`None`: no marker).
pub fn marked_dir(products: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(products.join(MARKER)).ok()?;
    text.lines().next().filter(|l| !l.is_empty()).map(PathBuf::from)
}

/// A checker of built products (`layout-check`) reads the lists the products in `products` were grouped from: when
/// they carry a [`MARKER`], its directory is forced for this process. Only product checks call this: the static
/// checks, regen and repair work on the committed lists whatever the last build was.
pub fn follow_products(root: &Path, products: &Path) -> Result<(), String> {
    match marked_dir(products) {
        Some(d) if Mode::from_env().over.is_none() => {
            println!("hot-lists: {}/{MARKER}: the products were grouped from the lists in {} ({TRACE_ENV})", products.display(), d.display());
            force_dir(root, &d)
        }
        _ => Ok(()),
    }
}

/// The text of the [`MARKER`] for lists in `dir`: the directory, then each [`GENERATED`] list's FNV-1a hash.
pub fn marker_text(dir: &Path) -> String {
    let mut out = format!("{}\n", dir.display());
    for g in GENERATED {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in &fs::read(dir.join(g)).unwrap_or_default() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        out.push_str(&format!("{g} {h:016x}\n"));
    }
    out
}

/// What decides where the generated lists are read from, besides the tree: the [`ENV`] override directory and
/// whether this is the train. [`resolve`] takes it explicitly so that its tests never depend on the environment
/// they run in (the CI gate itself runs with `LIELLA_TRAIN=1`); only [`dir`] reads this process's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    /// [`ENV`]'s value (unset or empty: none).
    pub over: Option<PathBuf>,
    /// `LIELLA_TRAIN=1` ([`train`]).
    pub train: bool,
}

impl Mode {
    /// The mode of this process: its [`ENV`] and `LIELLA_TRAIN`.
    pub fn from_env() -> Mode {
        Mode { over: std::env::var_os(ENV).filter(|v| !v.is_empty()).map(PathBuf::from), train: train() }
    }
}

/// Where the generated lists of `root` are read from under `mode` (see the module doc), and the info line to print
/// once.
pub fn resolve(root: &Path, mode: &Mode) -> Result<(PathBuf, Option<String>), String> {
    if let Some(d) = &mode.over {
        return Ok((d.clone(), None));
    }
    let train = mode.train;
    let (tracked, out) = (root.join(TRACKED), root.join(OUT));
    let Ok(stamp) = fs::read_to_string(out.join(STAMP)) else { return Ok((tracked, None)) };
    let here = tree_hash(root);
    if stamp.trim() != here {
        return Ok((tracked, Some(format!(
            "hot-lists: {OUT} was generated for another tree state ({}; this tree is {here}): reading the tracked lists (refresh: cargo xtask hot-lists)",
            stamp.trim()
        ))));
    }
    let differ = differing(&tracked, &out);
    if train {
        if differ.is_empty() {
            return Ok((tracked, None));
        }
        return Err(format!(
            "hot-lists: the tracked {} stale: {OUT} holds a regeneration of this tree that differs; the train runs `cargo xtask hot-lists --in-place` before the build",
            differ.iter().map(|g| format!("{TRACKED}/{g}")).collect::<Vec<_>>().join(", ") + if differ.len() == 1 { " is" } else { " are" }
        ));
    }
    let info = if differ.is_empty() {
        format!("hot-lists: reading the generated lists from {OUT} (the same as the tracked copies)")
    } else {
        format!(
            "hot-lists: reading the generated lists from {OUT}; {} differ from the tracked copies (train-owned: the train regenerates them, a branch does not commit them)",
            differ.join(", ")
        )
    };
    Ok((out, Some(info)))
}

/// [`resolve`] for this process ([`Mode::from_env`]), once per root; the info line is printed the first time.
pub fn dir(root: &Path) -> Result<PathBuf, String> {
    type Resolved = (PathBuf, Result<PathBuf, String>);
    static DIR: OnceLock<Resolved> = OnceLock::new();
    let once = || {
        // LAYOUT-TRACE-AT-BUILD: after [`ENV`], the directory a trace-at-build run (or [`follow_products`]) forced.
        let mut mode = Mode::from_env();
        if mode.over.is_none() {
            mode.over = FORCED.get().cloned();
        }
        let r = resolve(root, &mode);
        if let Ok((_, Some(info))) = &r {
            println!("{info}");
        }
        r.map(|(d, _)| d)
    };
    let (first, r) = DIR.get_or_init(|| (root.to_path_buf(), once()));
    // xtask has one root per process; another root is resolved for itself, never given the first one's answer.
    if first == root { r.clone() } else { once() }
}

/// The path to read generated list `name` (one of [`GENERATED`]) from.
pub fn path(root: &Path, name: &str) -> Result<PathBuf, String> {
    if !GENERATED.contains(&name) {
        return Err(format!("hot-lists: {name} is no generated list (hand-written inputs are read from {TRACKED})"));
    }
    Ok(dir(root)?.join(name))
}

/// A workspace-relative path as a reader opens it: `crates/libc-so/<generated list>` in [`dir`], anything else
/// under `root` (the layout contracts name their key files this way).
pub fn rel(root: &Path, rel: &str) -> Result<PathBuf, String> {
    rel_in(root, rel, || dir(root))
}

/// [`rel`] with the directory of the generated lists given (`lists`, called only for a generated list).
fn rel_in(root: &Path, rel: &str, lists: impl FnOnce() -> Result<PathBuf, String>) -> Result<PathBuf, String> {
    match rel.strip_prefix(TRACKED).and_then(|r| r.strip_prefix('/')) {
        Some(name) if GENERATED.contains(&name) => Ok(lists()?.join(name)),
        _ => Ok(root.join(rel)),
    }
}

/// `cargo xtask hot-lists [--in-place]`: regenerate the [`GENERATED`] lists of both profiles (see the module doc).
pub fn cmd(root: &Path, args: &[String]) -> Result<(), String> {
    let mut in_place = train();
    for a in args {
        match a.as_str() {
            "--in-place" => in_place = true,
            _ => return Err("usage: cargo xtask hot-lists [--in-place] (in place also under LIELLA_TRAIN=1)".into()),
        }
    }
    let (tracked, out) = (root.join(TRACKED), root.join(OUT));
    fs::create_dir_all(&out).map_err(|e| format!("hot-lists: {}: {e}", out.display()))?;
    // A stamp left by an earlier run must not survive a failed one (the build would read half-written lists).
    match fs::remove_file(out.join(STAMP)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("hot-lists: {OUT}/{STAMP}: {e}")),
    }
    let copy = |from: &Path, to: &Path| -> Result<(), String> {
        for g in GENERATED {
            fs::copy(from.join(g), to.join(g)).map_err(|e| format!("hot-lists: copy {g}: {e}"))?;
        }
        Ok(())
    };
    // Merge mode (#4839): the regen starts from the tracked lists, in place or in a copy of them.
    let target = if in_place { tracked.clone() } else { copy(&tracked, &out)?; out.clone() };
    let st = Command::new("bash")
        .arg(root.join("tools/hottext/regen.sh"))
        .arg(root)
        .args(["--both", "--allow-drop"])
        .env(ENV, &target)
        .current_dir(root)
        .status()
        .map_err(|e| format!("hot-lists: tools/hottext/regen.sh: {e}"))?;
    if !st.success() {
        return Err(format!("hot-lists: tools/hottext/regen.sh . --both --allow-drop failed ({st}); {OUT}/{STAMP} not written"));
    }
    if in_place {
        copy(&tracked, &out)?;
    }
    let hash = tree_hash(root);
    fs::write(out.join(STAMP), format!("{hash}\n")).map_err(|e| format!("hot-lists: {OUT}/{STAMP}: {e}"))?;
    let differ = differing(&tracked, &out);
    if in_place {
        println!("hot-lists: regenerated in place ({TRACKED}), both profiles, for {hash}");
    } else if differ.is_empty() {
        println!("hot-lists: regenerated into {OUT} for {hash}: the same as the tracked copies");
    } else {
        println!(
            "hot-lists: regenerated into {OUT} for {hash}: {} differ from the tracked copies; the build and the tests of this tree read {OUT} (do not commit the lists: the train regenerates them)",
            differ.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{differing, dir, marked_dir, marker_text, rel_in, resolve, tree_hash, Layout, Mode, GENERATED, MARKER, OUT, STAMP, TRACE_ENV, TRACKED};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A scratch tree: the tracked lists, one source file, and optionally a regeneration in `target/hot-lists`.
    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("liella-hotlists-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(TRACKED)).unwrap();
        fs::create_dir_all(root.join(OUT)).unwrap();
        fs::write(root.join("Cargo.lock"), "lock\n").unwrap();
        fs::write(root.join(TRACKED).join("hot-text.clusters"), "# hand-written\n").unwrap();
        for g in GENERATED {
            fs::write(root.join(TRACKED).join(g), format!("# {g}\nkey_a\tstartup\tfast,release\n")).unwrap();
            fs::write(root.join(OUT).join(g), format!("# {g}\nkey_a\tstartup\tfast,release\n")).unwrap();
        }
        root
    }

    /// The modes are explicit: these tests read neither `LIELLA_TRAIN` nor `LIELLA_HOT_LISTS` of the process (the CI
    /// gate runs them with `LIELLA_TRAIN=1`).
    fn branch() -> Mode {
        Mode { over: None, train: false }
    }

    fn train() -> Mode {
        Mode { over: None, train: true }
    }

    fn stamp(root: &Path) {
        fs::write(root.join(OUT).join(STAMP), format!("{}\n", tree_hash(root))).unwrap();
    }

    /// HOTLIST-TRAIN-REGEN (no spec clause): on the train a tracked list that differs from a stamped regeneration of
    /// the same tree is an error naming the file; with equal copies the train reads the tracked lists.
    #[test]
    fn train_fails_on_a_stale_tracked_list() {
        let root = tree("train");
        stamp(&root);
        assert_eq!(resolve(&root, &train()).unwrap(), (root.join(TRACKED), None));
        fs::write(root.join(TRACKED).join("startup-text.keys"), "# old\nkey_old\tstartup\tfast,release\n").unwrap();
        let e = resolve(&root, &train()).unwrap_err();
        assert!(e.contains("crates/libc-so/startup-text.keys is stale"), "{e}");
        let _ = fs::remove_dir_all(&root);
    }

    /// HOTLIST-TRAIN-REGEN (no spec clause): off the train the same stale tracked list passes: the build reads the
    /// regeneration in target/hot-lists and says so in one info line naming the differing file.
    #[test]
    fn branch_reads_the_fresh_copy_with_an_info_line() {
        let root = tree("branch");
        fs::write(root.join(TRACKED).join("hot-text.list"), "# old\nkey_old\tstartup\tfast,release\n").unwrap();
        stamp(&root);
        let (dir, info) = resolve(&root, &branch()).unwrap();
        assert_eq!(dir, root.join(OUT));
        let info = info.expect("an info line");
        assert!(info.contains("hot-text.list differ from the tracked copies"), "{info}");
        assert_eq!(differing(&root.join(TRACKED), &root.join(OUT)), ["hot-text.list"]);
        // `rel` routes a contract's tracked key path to the fresh copy, a hand-written input to the tree.
        let fresh = || resolve(&root, &branch()).map(|(d, _)| d);
        assert_eq!(rel_in(&root, "crates/libc-so/hot-text.list", fresh).unwrap(), root.join(OUT).join("hot-text.list"));
        assert_eq!(rel_in(&root, "crates/libc-so/hot-text.clusters", fresh).unwrap(), root.join(TRACKED).join("hot-text.clusters"));
        let _ = fs::remove_dir_all(&root);
    }

    /// HOTLIST-TRAIN-REGEN (no spec clause): a regeneration made for another tree state (a source changed after it,
    /// or a role reused by another story) is never read: the tracked lists are, with an info line; no stamp, no line.
    /// A change to a generated list alone does not change the tree state; an override directory always wins.
    #[test]
    fn a_regeneration_of_another_tree_is_not_read() {
        let root = tree("other");
        assert_eq!(resolve(&root, &branch()).unwrap(), (root.join(TRACKED), None));
        stamp(&root);
        let before = tree_hash(&root);
        fs::write(root.join(TRACKED).join("probe-text.keys"), "# changed\n").unwrap();
        assert_eq!(tree_hash(&root), before, "the generated lists are not part of the tree state");
        fs::create_dir_all(root.join("crates/core/src")).unwrap();
        fs::write(root.join("crates/core/src/lib.rs"), "fn moved() {}\n").unwrap();
        assert_ne!(tree_hash(&root), before);
        let (dir, info) = resolve(&root, &branch()).unwrap();
        assert_eq!(dir, root.join(TRACKED));
        assert!(info.is_some_and(|i| i.contains("another tree state")));
        assert_eq!(resolve(&root, &train()).unwrap().0, root.join(TRACKED), "the train ignores it too");
        let over = root.join("over");
        assert_eq!(resolve(&root, &Mode { over: Some(over.clone()), train: true }).unwrap(), (over, None));
        let _ = fs::remove_dir_all(&root);
    }

    /// HOTLIST-TRAIN-REGEN (no spec clause): the process-wide [`dir`] answers each root for itself, never with the
    /// first root's cached directory. It compares with [`resolve`] under the same process mode, so it holds whatever
    /// `LIELLA_TRAIN` or `LIELLA_HOT_LISTS` the caller set (neither tree has a stamp, so neither mode can fail).
    #[test]
    fn the_cached_directory_belongs_to_its_root() {
        let (a, b) = (tree("cache-a"), tree("cache-b"));
        let _ = (fs::remove_dir_all(a.join(OUT)), fs::remove_dir_all(b.join(OUT)));
        for root in [&a, &b, &a] {
            assert_eq!(dir(root).unwrap(), resolve(root, &Mode::from_env()).unwrap().0, "{}", root.display());
        }
        let _ = (fs::remove_dir_all(&a), fs::remove_dir_all(&b));
    }

    /// LAYOUT-TRACE-DEFAULT (no spec clause: build tooling, rulings #5190-#5194): a product build traces its own
    /// layout unless told otherwise (unset, empty or `1`); `ungrouped` is the explicit opt-out and `0` the committed
    /// lists; a regen build reads the committed lists and refuses an explicit trace or opt-out; any other value is
    /// an error, never a silent default.
    #[test]
    fn layout_switch_defaults_to_the_trace_and_is_strict() {
        assert_eq!(Layout::parse(None, false), Ok(Layout::Traced));
        assert_eq!(Layout::parse(Some(""), false), Ok(Layout::Traced));
        assert_eq!(Layout::parse(Some("1"), false), Ok(Layout::Traced));
        assert_eq!(Layout::parse(Some("0"), false), Ok(Layout::Committed));
        assert_eq!(Layout::parse(Some("ungrouped"), false), Ok(Layout::Ungrouped));
        assert_eq!(Layout::parse(None, true), Ok(Layout::Committed));
        assert_eq!(Layout::parse(Some(""), true), Ok(Layout::Committed));
        assert_eq!(Layout::parse(Some("0"), true), Ok(Layout::Committed));
        for explicit in ["1", "ungrouped"] {
            assert!(Layout::parse(Some(explicit), true).unwrap_err().contains(crate::hottext::REGEN_FLAG), "{explicit}");
        }
        for bad in ["yes", "true", "2", "Ungrouped", " 1"] {
            for regen in [false, true] {
                assert!(Layout::parse(Some(bad), regen).unwrap_err().contains("LIELLA_LAYOUT_TRACE"), "{bad} {regen}");
            }
        }
    }

    /// LAYOUT-TRACE-DEFAULT (no spec clause: build tooling): the list tooling builds from the committed lists, so
    /// every product build `tools/hottext/regen.sh` runs is a regen build (`crate::hottext::REGEN_FLAG`) or names
    /// `LIELLA_LAYOUT_TRACE=0`; a plain `cargo xtask so` there would trace its own layout and check nothing of the
    /// list it just rewrote.
    #[test]
    fn regen_builds_never_rely_on_the_default_layout() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let sh = fs::read_to_string(root.join("tools/hottext/regen.sh")).unwrap();
        let builds: Vec<&str> = sh.lines().filter(|l| !l.trim_start().starts_with('#') && l.contains("cargo xtask so")).collect();
        assert!(builds.len() >= 3, "regen.sh: found {} product builds, expected the --both, --build and rewritten-list builds", builds.len());
        for l in builds {
            assert!(l.contains(crate::hottext::REGEN_FLAG) || l.contains(&format!("{TRACE_ENV}=0 cargo xtask so")), "regen.sh builds with the default layout (trace at build): {l}");
        }
    }

    /// LAYOUT-TRACE-AT-BUILD (no spec clause: build tooling): the products' marker names the lists directory on its
    /// first line, and its text changes with any list's bytes (the layout verdict cache hashes the products
    /// directory, so a list change must change a product file); no marker, no directory.
    #[test]
    fn marker_names_the_lists_and_hashes_them() {
        let root = tree("marker");
        let (lists, products) = (root.join("lists"), root.join("products"));
        fs::create_dir_all(&lists).unwrap();
        fs::create_dir_all(&products).unwrap();
        assert_eq!(marked_dir(&products), None);
        for g in GENERATED {
            fs::write(lists.join(g), "# empty\n").unwrap();
        }
        let first = marker_text(&lists);
        fs::write(products.join(MARKER), &first).unwrap();
        assert_eq!(marked_dir(&products), Some(lists.clone()));
        fs::write(lists.join("hot-text.list"), "# empty\nkey_a\tstartup\tfast\n").unwrap();
        assert_ne!(marker_text(&lists), first, "a list change changes the marker");
        assert_eq!(marker_text(&lists).lines().count(), 1 + GENERATED.len());
        let _ = fs::remove_dir_all(&root);
    }
}
