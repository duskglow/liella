//! The freshness check of `support/oracle.rs` (shared test programs under `target/test-oracles`)
//! depends on the contents of the inputs, not on file-system timestamps (owner rule 2026-10-21:
//! nothing depends on timestamp granularity). A header edited to the same length, with its
//! modification time put back to the old value (what a file system with coarse timestamps does to
//! an edit made right after a build), must still force a rebuild; an unchanged input must not.
//!
//! Needs a C compiler (`cc`); without one the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

/// Compile `dir/main.c`; with `include_dir` the directory is also named by `-I`, without it the
/// header is found only because it sits beside the source (a quoted include).
fn compile(dir: &Path, out: &Path, include_dir: bool) -> bool {
    let mut cc = Command::new("cc");
    cc.arg(dir.join("main.c"));
    if include_dir {
        cc.arg("-I").arg(dir);
    }
    oracle::build_quiet(cc, out)
}

fn exit_code(out: &Path) -> i32 {
    Command::new(out).status().unwrap().code().unwrap()
}

#[test]
fn oracle_freshness_follows_content_not_mtime() {
    // Trace: no spec clause (test-harness behaviour; owner rule 2026-10-21, no file-system dependence).
    freshness_follows_content(true, "oracle-fresh-i");
}

/// The header is named by no `-I` and no argument: only its place beside `main.c` ties it to the
/// build, so a freshness check that looks only at the named files and `-I` directories misses it.
#[test]
fn oracle_freshness_covers_headers_beside_the_source() {
    // Trace: no spec clause (test-harness behaviour; owner rule 2026-10-21, no file-system dependence).
    freshness_follows_content(false, "oracle-fresh-beside");
}

fn freshness_follows_content(include_dir: bool, tag: &str) {
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.c"), "#include \"v.h\"\nint main(void) { return V; }\n").unwrap();
    std::fs::write(dir.join("v.h"), "#define V 1\n").unwrap();
    let out = dir.join("prog");

    assert!(compile(&dir, &out, include_dir));
    assert_eq!(exit_code(&out), 1);

    // Unchanged inputs: the program is kept (same file, not a rebuilt one).
    let first = std::fs::metadata(&out).unwrap().ino();
    assert!(compile(&dir, &out, include_dir));
    assert_eq!(std::fs::metadata(&out).unwrap().ino(), first, "rebuilt although no input changed");

    // Same-length edit of the header with its old modification time restored.
    let old = std::fs::metadata(dir.join("v.h")).unwrap().modified().unwrap();
    std::fs::write(dir.join("v.h"), "#define V 2\n").unwrap();
    std::fs::File::options().write(true).open(dir.join("v.h")).unwrap().set_modified(old - std::time::Duration::from_secs(3600)).unwrap();
    assert!(compile(&dir, &out, include_dir));
    assert_eq!(exit_code(&out), 2, "an edited header with an old modification time was taken as built");

    let _ = std::fs::remove_dir_all(&dir);
}
