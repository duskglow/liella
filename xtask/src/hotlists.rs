//! The start-up lists of a product build (LAYOUT-TRACE-AT-BUILD, D-X.88; LAYOUT-TRACE-DEFAULT, D-X.92;
//! LAYOUT-DELETE-LISTS, LT-5).
//!
//! [`GENERATED`] (`hot-text.list`, `startup-text.keys`, `probe-text.keys`, `startup-data.keys`) are written by
//! every product build from a trace of that build (`tools/hottext/regen.sh`, run by `cargo xtask so`) into
//! `target/product/<profile>`; none is committed (`cargo xtask static` refuses one in the tree,
//! `crate::hottext::static_check`). The hand-written inputs (`hot-text.clusters`, `hot-text.first-window`,
//! `startup-state.list`) stay in [`TRACKED`] and are always read from the tree.
//!
//! Every reader (the `so` grouping, the start-up read-only group, the layout contracts) asks [`dir`] where the
//! lists are: the directory this process's build wrote them to ([`force_dir`]), or the one the checked products'
//! [`MARKER`] names ([`follow_products`]). There is no other source: without a build there are no lists.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The tracked directory of the hand-written layout inputs, relative to the workspace root.
pub const TRACKED: &str = "crates/libc-so";
/// The lists every product build writes from its own trace (never tracked).
pub const GENERATED: [&str; 4] = ["hot-text.list", "startup-text.keys", "probe-text.keys", "startup-data.keys"];
/// The directory `tools/hottext/regen.sh` (and `probe_text.sh`, `startup_data.sh`) writes the lists to; `so` sets
/// it for the trace it runs.
pub const ENV: &str = "LIELLA_HOT_LISTS";

/// LAYOUT-TRACE-AT-BUILD (LT-1, rulings #5190/#5191) and LAYOUT-TRACE-DEFAULT: the layout switch of `cargo xtask
/// so`. Unset, empty or `1`: the build links the probes against the archive it built, traces them and groups from
/// that trace ([`Layout::Traced`], the default of every product build); `ungrouped`: the explicit opt-out for a
/// builder without a tracer, an ungrouped product ([`Layout::Ungrouped`]). `0` (the committed lists) was removed
/// with them (LT-5) and is an error.
pub const TRACE_ENV: &str = "LIELLA_LAYOUT_TRACE";
/// The marker `so` writes next to products it grouped from a trace of their own build (or left ungrouped): line 1
/// the directory of the lists it used (`target/product/<profile>`), then `<list> <hash>` per list (so the layout
/// verdict cache, which hashes the products directory, sees a list change). Every later reader of the lists
/// (`layout-check`, the tests) reads that directory.
pub const MARKER: &str = ".hot-lists";

/// Where the start-up layout of a `cargo xtask so` comes from ([`TRACE_ENV`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// A trace of the probes linked against this build's un-grouped archive: the default.
    Traced,
    /// No grouping at all (no tracer on this builder; explicit).
    Ungrouped,
}

impl Layout {
    /// The layout `value` of [`TRACE_ENV`] selects for a product build.
    pub fn parse(value: Option<&str>) -> Result<Layout, String> {
        match value.unwrap_or("") {
            "" | "1" => Ok(Layout::Traced),
            "ungrouped" => Ok(Layout::Ungrouped),
            "0" => Err(format!(
                "{TRACE_ENV}=0: the committed lists were removed (LT-5, LAYOUT-DELETE-LISTS): every product build traces its own layout; unset {TRACE_ENV}, or set it to ungrouped (no tracer: ungrouped product)"
            )),
            v => Err(format!("{TRACE_ENV}={v:?}: want unset or 1 (trace at build, the default) or ungrouped (no tracer: ungrouped product)")),
        }
    }

    /// This process's layout ([`TRACE_ENV`]) for a product build.
    pub fn from_env() -> Result<Layout, String> {
        Layout::parse(std::env::var(TRACE_ENV).ok().as_deref())
    }
}

/// The lists directory of the build this process ran last, or of the products it checks ([`force_dir`]).
static FORCED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Make `dir` the directory of the lists for the readers that follow (a product build, before it groups; a
/// later build in the same process, such as `bench --v3` after the products, sets its own).
pub fn force_dir(dir: &Path) {
    *FORCED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir.to_path_buf());
}

/// The lists directory named by the [`MARKER`] of the products in `products` (`None`: no marker).
pub fn marked_dir(products: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(products.join(MARKER)).ok()?;
    text.lines().next().filter(|l| !l.is_empty()).map(PathBuf::from)
}

/// A checker of built products (`layout-check`) reads the lists the products in `products` were grouped from (the
/// directory their [`MARKER`] names). Products without a marker have no lists ([`dir`] then fails when a contract
/// needs one).
pub fn follow_products(products: &Path) {
    if let Some(d) = marked_dir(products) {
        println!("hot-lists: {}/{MARKER}: the products were grouped from the lists in {}", products.display(), d.display());
        force_dir(&d);
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

/// The directory of the lists ([`force_dir`]); an error when this process neither built products nor follows
/// built ones.
pub fn dir() -> Result<PathBuf, String> {
    FORCED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone().ok_or_else(|| {
        format!(
            "hot-lists: no start-up lists: they are written by a product build from a trace of itself (cargo xtask so) and read through the products' {MARKER} marker; build the products first"
        )
    })
}

/// The path to read list `name` (one of [`GENERATED`]) from.
pub fn path(name: &str) -> Result<PathBuf, String> {
    if !GENERATED.contains(&name) {
        return Err(format!("hot-lists: {name} is no generated list (hand-written inputs are read from {TRACKED})"));
    }
    Ok(dir()?.join(name))
}

/// A path as a reader opens it: a bare [`GENERATED`] name (the layout contracts name their key files this way) in
/// [`dir`], anything else as a workspace-relative path under `root`.
pub fn rel(root: &Path, rel: &str) -> Result<PathBuf, String> {
    rel_in(root, rel, dir)
}

/// [`rel`] with the directory of the lists given (`lists`, called only for a generated list).
fn rel_in(root: &Path, rel: &str, lists: impl FnOnce() -> Result<PathBuf, String>) -> Result<PathBuf, String> {
    if GENERATED.contains(&rel) {
        return Ok(lists()?.join(rel));
    }
    if rel.strip_prefix(TRACKED).and_then(|r| r.strip_prefix('/')).is_some_and(|n| GENERATED.contains(&n)) {
        return Err(format!("hot-lists: {rel}: the committed lists were removed (LT-5); name the list bare ({}) to read this build's trace", &rel[TRACKED.len() + 1..]));
    }
    Ok(root.join(rel))
}

#[cfg(test)]
mod tests {
    use super::{marked_dir, marker_text, rel_in, Layout, GENERATED, MARKER, TRACKED};
    use std::fs;
    use std::path::PathBuf;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("liella-hotlists-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// LAYOUT-TRACE-DEFAULT, LAYOUT-DELETE-LISTS (no spec clause: build tooling, rulings #5190-#5194): a product
    /// build traces its own layout unless told otherwise (unset, empty or `1`); `ungrouped` is the explicit
    /// opt-out; `0` (the committed lists, removed in LT-5) and any other value are errors, never a silent default.
    #[test]
    fn layout_switch_defaults_to_the_trace_and_is_strict() {
        assert_eq!(Layout::parse(None), Ok(Layout::Traced));
        assert_eq!(Layout::parse(Some("")), Ok(Layout::Traced));
        assert_eq!(Layout::parse(Some("1")), Ok(Layout::Traced));
        assert_eq!(Layout::parse(Some("ungrouped")), Ok(Layout::Ungrouped));
        assert!(Layout::parse(Some("0")).unwrap_err().contains("committed lists were removed"));
        for bad in ["yes", "true", "2", "Ungrouped", " 1"] {
            assert!(Layout::parse(Some(bad)).unwrap_err().contains("LIELLA_LAYOUT_TRACE"), "{bad}");
        }
    }

    /// LAYOUT-DELETE-LISTS (no spec clause: build tooling): a contract's bare list name reads the build's
    /// lists, a hand-written input reads the tree, and the old tracked path of a list is refused by name
    /// (it would read a file no tree has any more).
    #[test]
    fn rel_reads_the_traced_lists_and_refuses_the_committed_paths() {
        let root = tree("rel");
        let lists = || Ok(root.join("lists"));
        assert_eq!(rel_in(&root, "startup-text.keys", lists).unwrap(), root.join("lists/startup-text.keys"));
        assert_eq!(rel_in(&root, "crates/libc-so/hot-text.clusters", lists).unwrap(), root.join(TRACKED).join("hot-text.clusters"));
        for g in GENERATED {
            let e = rel_in(&root, &format!("{TRACKED}/{g}"), lists).unwrap_err();
            assert!(e.contains("committed lists were removed") && e.contains(g), "{e}");
        }
        let none = || Err("no lists".to_string());
        assert_eq!(rel_in(&root, "hot-text.list", none).unwrap_err(), "no lists");
        let _ = fs::remove_dir_all(&root);
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
