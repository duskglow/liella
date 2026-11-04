# ARCH-REF-PROBE (A-5, OD-10): aarch64 symbol versions done; layouts BLOCKED on missing UAPI headers

State: branch `task/arch-ref-probe` (from origin/next 00a8aa87). Plan item: plan/06-phases/20-post-release-ledger-and-experiments.md
A-5 (marked partly done). Rulings in force: RL-2 (cross tools on keke), RL-3 (reference = Ubuntu arm64 libc6 + libc6-dev
2.39-0ubuntu8.9, unpacked at keke:~/ref/aarch64). No D-row (the plan item records it).

## What was done

* `spec/tools/abi_extract.py --arch aarch64 --sysroot DIR [--symbols-only]`: a cross mode. Everything is compile-only:
  sizes and alignments are a `const unsigned long long[]` in its own section and initializer bytes are one object per
  section, read back with `readelf -x`; members come from DWARF (`readelf --debug-dump`), the same as x86_64. Symbols come
  from `readelf --dyn-syms` of the reference's shared objects (the loader is `ld-linux-aarch64.so.1`). Before probing any
  type, the headers must compile as a set. Without that check a missing header drops every type without an error (the
  first run skipped all 165).
  The default mode (no `--arch`) is unchanged: it was rerun on keke and its output compared with the committed tables
  (see the report).
* Generated (keke, `--symbols-only`): `spec/abi/versions-aarch64.tsv` (4486 rows, same shape as versions.tsv),
  `spec/abi/exports-diff-aarch64.tsv` (60 exports found on only one arch, by library and name; the loader name is
  normalised), `spec/abi/PROVENANCE-aarch64`.

## Probe-settled facts

* The oldest aarch64 version node is `GLIBC_2.17` (x86_64: `GLIBC_2.2.5`).
* 57 exports exist only on x86_64, among them `ioperm`, `iopl`, `modify_ldt` and `arch_prctl` (RL-8), and 3 exist only on aarch64
  (see the tsv). This is the export list A-3/RL-8 asked for.

## BLOCKED: part (a), layouts-aarch64.tsv

The arm64 reference headers do not compile: `bits/sigcontext.h` includes `asm/sigcontext.h`, which is kernel UAPI
(package linux-libc-dev), not libc6-dev. keke has no arm64 UAPI headers (not in ~/ref-arm64, not under
/usr/aarch64-linux-gnu). The brief forbids downloading. Using the x86_64 or generic `asm/` headers would give wrong
ucontext/sigcontext layouts, so it was not done. RULING NEEDED: add Ubuntu noble `linux-libc-dev_<ver>_arm64.deb` (the
version that pairs with the x86_64 reference's linux-libc-dev), verified like RL-3, unpacked into the same root.

## Next step (after the ruling)

1. keke: `python3 -I spec/tools/abi_extract.py --arch aarch64 --sysroot ~/ref/aarch64` (from a scratch copy of spec/;
   it writes layouts-aarch64.tsv and initializers-aarch64.tsv next to the script's ../abi).
2. abi-gen: parameterise `render()` in xtask/src/abi.rs by arch. Emit `crates/abi/src/layout_aarch64.rs` (lib.rs picks it
   with `#[cfg(target_arch)] #[path]`) and `tests/appendix_c_layouts_aarch64.rs` (`#![cfg(target_arch = "aarch64")]`, and
   the x86_64 file gets `x86_64`). appendix_c/table.rs `include_str!`s the per-arch tsv. Add "cfg" to the crates/abi/
   ALLOWED entry in xtask/src/arch.rs. Check that layout_init.rs builds against the aarch64 types.
3. Jobs: arch-ref-probe-static, arch-ref-probe-cross (`cargo xtask arch-check`), the direct-tier gate.

Not to redo: the cross probe mechanics, the symbol tables, the UAPI search on keke.
