# DL-PLACEHOLDERS (phase 14 DL-1): handoff

**State.** Branch `task/dl-placeholders` (from `origin/next` 8357b11d), HEAD: see the report json. Role keke:~/w/n2.
Decision row **D-27.9** (plan/07-decisions/s26-29.md). Rulings in force: R14-12 (a) (no GLIBC_PRIVATE anywhere),
R14-3 (pinned sonames; nothing here assumes Liella is or is not the only libc).

## What was done
* `xtask/src/placeholders.rs` (new), called from `cargo xtask so` step 5 (`so.rs`): builds `libpthread.so.0`,
  `libdl.so.2`, `librt.so.1`, `libutil.so.1`, `libanl.so.1`, `libc_malloc_debug.so.0` from `spec/abi/versions.tsv`
  and empty `libpthread.a`, `libdl.a`, `librt.a`, `libutil.a`, `libanl.a`.
  * Version nodes = the library's row versions, numeric order, chained (same shape as the reference's `readelf -V`:
    libpthread 12 nodes + base, libdl 3, librt 5, libutil 1, libanl 1, libc_malloc_debug 4).
  * Exports = the library's rows whose name `libc.so.6` has no row for (rule in D-27.9). Placeholders: only
    `__lib<x>_version_placeholder@<each version>` (body returns). `libc_malloc_debug.so.0`: only
    `malloc_get_state`/`malloc_set_state@GLIBC_2.2.5` (bodies in `crates/libc-so/placeholders/libc_malloc_debug.c`);
    every other name it lists is libc.so.6's and reached through `DT_NEEDED libc.so.6`.
  * Linked `-nostdlib` against `target/liella/libc.so.6` with `--no-as-needed`, so each has `DT_NEEDED libc.so.6`.
* **The check** (`placeholders::product_errors`), run on each library before it is published: soname, DT_NEEDED,
  version definitions vs the table, every defined dynamic symbol (unversioned / another library's row / a libc.so.6
  name / binding / missing export), and `check_no_private` over libc.so.6, libm.so.6 and the six: no GLIBC_PRIVATE
  verdef or export (R14-12). Wired into the existing symbol-version checks of `cargo xtask so` (the design names
  neither `static` nor `so`; the products are needed, so `so`).
* `remap.rs` product list includes the new files. `s22_n_1` now expects `NEEDED libc.so.6`; new
  `s22_n_1_placeholders_link` links `-lpthread -ldl -lrt -lutil -lanl` statically (runs, prints) and dynamically
  against libc.so.6 (links; `readelf -d` shows all five + libc.so.6 NEEDED; it cannot run before DL-14: NOT VERIFIED).

## Arch seam (§0a checklist)
arch seam: ticked. (1) no `GLIBC_2.2.5` or other arch literal: nodes and exports come from versions.tsv, the unit
tests use a synthetic `V_1.x` table; (2)-(6) not touched (no relocations, TLS, assembly, features, paths);
(7) the tests depend on no x86_64 fact beyond the products; (8) nothing new per arch.

## Open / for the coordinator
* **RULING NEEDED:** `libc_malloc_debug.so.0`'s two bodies are a two-statement C stub (NULL / -1, errno ENOSYS) with
  the same results as libc.a's Rust definitions, not a forwarding call: libc.so.6 does not export these names (the
  reference's does not either) and a forwarding target would be a new libc.so.6 export. Recommend: keep the stub now;
  DL-13 makes it one forwarding call to `malloc_get_state@LIELLA_PRIVATE` once that node exists.
* INFORMATIONAL: the reference's `libc_malloc_debug.so.0` also needs `ld-linux-x86-64.so.2`; ours only libc.so.6
  until ld.so exists (DL-5).
* INFORMATIONAL: `libc_malloc_debug.so.0` defines all four of its nodes (GLIBC_2.2.5/2.10/2.16/2.33) while exporting
  only two GLIBC_2.2.5 names, so a binary's version requirement on it still resolves.

## Do not redo
Reference shape probed (readelf -d/-V/--dyn-syms of the host libraries): NEEDED libc.so.6, base verdef = soname,
nodes chained in numeric order, placeholder FUNC size 5. Multiple `.symver` of one symbol with `, remove` on the
last works with binutils 2.42.

## Next step
Coordinator: land after gate + plants. Then DL-13 replaces the malloc_debug stub with a LIELLA_PRIVATE forward.
Read: plan/ph14/design.md §16, §0a; D-27.9.
