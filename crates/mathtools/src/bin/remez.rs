//! `remez`: fits minimax polynomials described by fit files and writes the generated Rust
//! coefficient files (D-10.3). See `liella_mathtools::remez` for the algorithm and the fit file
//! format.
//!
//! Usage: `cargo run --release -p liella-mathtools --bin remez -- [--check] <fit files...>`.
//! With `--check`, nothing is written: each output is regenerated in memory and compared with
//! the file on disk, and the exit status is 1 if any differs.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use liella_mathtools::remez::{generate, parse_fit};

/// The workspace root (this crate lives in `crates/mathtools`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// How the generated header names the fit file: relative to the workspace root when possible,
/// with `/` separators, so the output does not depend on the invoking directory.
fn display_path(fit: &Path, root: &Path) -> String {
    let rel = match (fit.canonicalize(), root.canonicalize()) {
        (Ok(f), Ok(r)) => f.strip_prefix(&r).map(Path::to_path_buf).unwrap_or(f),
        _ => fit.to_path_buf(),
    };
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn run(path: &str, check: bool, root: &Path) -> Result<bool, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = parse_fit(&text).map_err(|e| format!("{path}: {e}"))?;
    let g = generate(&spec, &display_path(Path::new(path), root)).map_err(|e| format!("{path}: {e}"))?;
    print!("{}", g.report);
    let out = root.join(&g.output);
    if check {
        let on_disk = std::fs::read_to_string(&out).unwrap_or_default();
        if on_disk != g.source {
            eprintln!("{}: out of date (regenerate with `cargo run --release -p liella-mathtools --bin remez -- {path}`)", g.output);
            return Ok(false);
        }
        println!("  {}: up to date", g.output);
    } else {
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&out, g.source.as_bytes()).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("  wrote {}", g.output);
    }
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let files: Vec<&String> = args.iter().filter(|a| *a != "--check").collect();
    if files.is_empty() {
        eprintln!("usage: remez [--check] <fit files...>");
        return ExitCode::from(2);
    }
    let root = workspace_root();
    let mut ok = true;
    for f in files {
        match run(f, check, &root) {
            Ok(good) => ok &= good,
            Err(e) => {
                eprintln!("error: {e}");
                ok = false;
            }
        }
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
