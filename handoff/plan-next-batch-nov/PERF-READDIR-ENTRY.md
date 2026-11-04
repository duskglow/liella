# PERF-READDIR-ENTRY: readdir per-entry user instructions (1.18x)

Id: PERF-READDIR-ENTRY
Kind: code
Priority: 40
Size: S
Host: keke
Stacks-on: PERF-FSWALK-PHYS
Model: opus
Effort: high
BUDGET: 45 tool calls / 100k

## The change

Measured gap: section 17 of the register, `readdir_entry_flat`: 54.3 vs 46.1 user instructions per entry (1.18;
the older note's 1.17 on another tree agrees). readdir is reached by ls, find, du, tar and every fts/nftw walk.
NOT independent: `readdir` lives in `crates/boundary/src/dirtree.rs`, which PERF-FSWALK-PHYS (running) may change, so
this stacks on it; cut from next after it lands and re-count first (its change may close the gap).
Scope: find where the extra instructions per entry go (the record walk, the `d_reclen` check, the locking, the copy)
with the per-call counts, and remove them without weakening the bounds checks on `getdents64` records (a record
length that runs past the buffer is still rejected). Files: the readdir path in `dirtree.rs` and its tests.
Proof: the dirent clause tests and the direct tier; plants on the changed record walk (skip one record, accept a short
`d_reclen`), each CAUGHT; before/after/glibc instructions for `readdir_entry_flat` and the fts and nftw rows (no row
slower); `cargo xtask clippy`.
JOBS: perf-readdir-entry-gate, perf-readdir-entry-plants, perf-readdir-entry-percall, PERF-READDIR-ENTRY-static
