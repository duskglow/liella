# DL-LDSO-PRODUCT (phase 14 DL-5): the ld.so product, its entry and the L14-1 link checks

**State:** branch `task/dl-ldso-product-2` (DL-LDSO-PRODUCT's WIP b8c1c7d0 on origin/next e0c6b146, plus 9a9ab2e8
and this handoff). Role mei:~/w/s2. Decisions in force: D-27.13 (plan/07-decisions/s26-29.md), coordinator
ruling #3810 (keep Liella's `--version`/`--help` text; row in docs/differences-from-glibc.md).

## Done (DL-LDSO-PRODUCT-2)
* **The start crash is fixed by design, not flags.** `liella_machine_ld_entry` (entry.S, `-DLIELLA_LD`) does
  `leaq __ehdr_start(%rip), %rsi` (hidden) and calls `liella_ld_start(sp, ehdr)`; `bootstrap::relocate_image(ehdr)`
  is `#[inline(never)]`. `adapter/image_anchor.c` is back to libc-only (`LIELLA_RUNTIME`).
* **Automated pre-relocation check** (`cargo xtask so` → `check_interp_prerelocation`, xtask/src/so.rs): from the
  `objdump -d` listing, every function reached by direct calls/jumps from `liella_machine_ld_entry`, with
  `liella_ld_start` cut after its call of `relocate_image`, must not read (non-`lea` RIP-relative operand) a word
  a dynamic relocation writes (`liella_ld::link::relocated_words`: RELA/JMPREL offsets and RELR addresses); a
  register-indirect call there is refused as unfollowable; an inlined relocator is refused. ARCH(x86_64) parser.
  Unit test `interp_prerelocation_reads_are_refused` (xtask). Limit: reads through a computed pointer are not
  visible to a listing check (recorded in D-27.13).
* Product (mei, step2): ld.so links, passes L14-1 and the new check; `--version` exits 0; all 9 `ldso_link`
  tests pass, including `ldso_product_runs_at_random_bases` (100 starts, half `setarch -R`) and
  `ldso_product_refuses_as_interpreter`.
* Inherited reds fixed: clippy (3 test `Arch` impls lacked `INTERP`; unused `c_int` under `export`; one `as`
  with a reasoned allow); static: unsafe audit (policy.rs lists `src/ld.rs` for export/slice/allocator and
  `src/arch/` for extern; tags moved above `#[unsafe(no_mangle)]`), arch tag on the entry declaration, test
  clause tags (`no spec clause`), differences row `no clause ID`.
* Dead code in the product-only boundary build: `null_folded`, `fold_resolved`, `liella_machine_scan_level_get`
  are now `cfg(feature = "selftest")` (their only readers). `heap::c_malloc` stays: it warns in the libc and libm
  product builds too (seen in the same `cargo xtask so` log), so it is not ld-specific; left for its owner.

## Jobs on the final head (mei; labels in jobs.tsv)
`dl-ldso-product-2-gate` (affected --run --base origin/next), `dl-ldso-product-2-counts` (cbench_startup
counters; compare with shiki's origin/next startup_instr_c 2149 / printf 71852 / setloc 68058 / wctype 69108:
mei is another CPU class, so a difference there needs an origin/next run on mei before it is read as a move),
`dl-ldso-product-2-plants2` (handoff/DL-LDSO-PRODUCT-2-plants.txt, 13 plants; run 1 had a too-weak textrel plant: it left the DF_TEXTREL half, which the linker also sets, so the test rightly passed; fixed and rerun; run 1 `dl-ldso-product-2-plants` was stopped and is superseded). The gate escalated to
FULL (xtask changed). Results: see the report json and the hand-back.

## Open / next step
1. If any job above is red: fix the cause, rerun only that job.
2. RULING NEEDED: the unsafe-audit file lists now include `src/ld.rs` and `src/arch/` (a trust-boundary list);
   recommend accept (same categories as the libc start's `runtime.rs`/`bootstrap.rs`).
3. INFORMATIONAL: ld.so's code keeps ~62 GOT-indirect calls (rustc's foreign-function calls are not relaxed in
   this `-shared` link). Correct after relocation; a later performance story may remove them. On a failed
   self-relocation the error message write goes through the GOT and may crash instead of printing (status
   still non-zero).
4. Next phase work: DL-6 (load the program).

## Do not redo
Linker flags for the GOT call (`-Bsymbolic`, `-Crelocation-model=pie`: two attempts, no effect).

Process note: one empty heredoc slipped into a local command early in this run (owner rule: no heredocs).
