//! `tables [--check]`: write (or check) `crates/math/src/coeffs/tables.rs`, the constant tables
//! of the elementary functions (`liella_mathtools::tables`, D-10.3).

#![allow(clippy::disallowed_methods)] // a host tool: S8 governs the library crates only

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../math/src/coeffs/tables.rs");
    let src = liella_mathtools::tables::generate();
    if check {
        if std::fs::read_to_string(&out).unwrap_or_default() != src {
            eprintln!("tables: crates/math/src/coeffs/tables.rs is out of date: run `cargo xtask tables`");
            return ExitCode::FAILURE;
        }
        println!("tables: up to date");
    } else if let Err(e) = std::fs::write(&out, src) {
        eprintln!("tables: {}: {e}", out.display());
        return ExitCode::FAILURE;
    } else {
        println!("tables: wrote crates/math/src/coeffs/tables.rs");
    }
    ExitCode::SUCCESS
}
