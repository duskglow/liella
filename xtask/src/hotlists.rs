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
        let r = resolve(root, &Mode::from_env());
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
    use super::{differing, rel_in, resolve, tree_hash, Mode, GENERATED, OUT, STAMP, TRACKED};
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
}
