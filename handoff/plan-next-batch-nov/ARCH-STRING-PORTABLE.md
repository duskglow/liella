# ARCH-STRING-PORTABLE: portable Rust versions of the scan/fold kernels (ledger A-8)

Id: ARCH-STRING-PORTABLE
Kind: code
Priority: 60
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 50 tool calls / 120k

## The change

Plan row: `plan/06-phases/20-post-release-ledger-and-experiments.md`, A-8 (after A-2; ARCH-RUST-CHECK landed 2026-11-04,
ARCH-SEAMS-2 landed 2026-10-23). No other open story touches these files (checked against `ops status`, 2026-11-04).

Scope: for every `csys/src/machine/scan.S` / `fold.S` kernel that ARCH-SEAMS-2's seam list names without a portable
version, add a portable Rust (SWAR, word-at-a-time) version, selected only on targets without a tuned kernel. On x86_64
the tuned kernels stay the default, so no instruction count changes there. NEON waits for hardware (A-13).
Files: the portable module next to the seam (find it with `docs/codemap/README.md`), the kernel test suites
`liella-boundary --test small_copy` and `--test machine_scanners` (add a way to force the portable path in tests only).
Rules: portable reads never cross a page the byte-wise kernel would not touch (align to the word, as the .S kernels do);
results equal the tuned kernels bit for bit (owner 2026-11-04: all paths give the same result, test both paths).
Proof: both kernel suites pass with the portable path forced, and pass as they are; plants on each portable kernel
(an off-by-one in the tail, a wrong zero-byte mask, a missed page-boundary alignment), each CAUGHT by small_copy or
machine_scanners named explicitly (`affected` selects nothing for .S-adjacent code); `cargo xtask clippy`.
JOBS: arch-string-portable-gate, arch-string-portable-plants, ARCH-STRING-PORTABLE-static
