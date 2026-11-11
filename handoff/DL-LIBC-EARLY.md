# DL-LIBC-EARLY: LIELLA_PRIVATE, `__liella_libc_early_init` (phase 14 DL-13)

**State:** branch `task/dl-libc-early-2` (continues `task/dl-libc-early`; base origin/next 672acd9aa), role
kanon:~/w/kn1. First half (build tag check, early init before constructors) is done and gate-fixed; **the export moves
and rseq publishing are NOT done** (next story, see below). Report: handoff/DL-LIBC-EARLY-2-report.md.

**Decisions in force:** D-27.22 (this story; tests now name `crates/rt/src/start.rs::private_tag_layout`),
D-27.19/D-27.20 (init order, interpreter exports), D-27.18/.21 (shared bias). Plan rows:
plan/06-phases/17-phase-14-dynamic-linking.md DL-13; plan/ph14/design.md §14; R14-12.

## What exists
* `liella_rt::start::private` (moved there in -2): `NODE`, `EARLY_INIT`, `BUILD_TAG`, `TAG_PREFIX`, `TAG_LEN`,
  `TAG` (64 bytes, `LIELLA-BUILD-TAG:` + `LIELLA_BUILD_TAG` set by `cargo xtask so`), `StartInfo` (`repr(C)`).
  `liella_ld::start::private` re-exports them and keeps `find` (lookup of `__liella_libc_early_init@LIELLA_PRIVATE`,
  tag check, `<object>: LIELLA_PRIVATE build tag differs ...`, 127).
* ld.so (`boundary/src/ld.rs`): tag check after the version check, before relocation; `run_init` calls the early
  init before `.preinit_array`. libc: `runtime::early_init` (s01_startup.rs export, tag static in data.rs).
* Tests: crates/ld/tests/reloc.rs `reloc_libc_early_init_before_preinit`, `reloc_libc_build_tag_mismatch_refused`;
  unit `private_tag_layout` (rt). Plants: handoff/DL-LIBC-EARLY-2-plants.txt (4 plants).

## Do not redo
* The -1 gate red (`a_loader_change_does_not_select_the_math_suites`) was libc-side boundary code (data.rs,
  runtime.rs) naming `liella_ld::`: the affected model then puts the loader crate in libc's image, so a loader
  change selects liella-math. Rule: **libc-side code never names `liella_ld::`**; shared data goes in `liella_rt`
  (plant `libc-names-loader-crate` proves the test catches it).

## Next step (DL-13 second half, a continuation story)
Facts found (not built): `__libc_stack_end` is a C definition (csys/src/adapter/runtime.c) read via ffi.rs;
`__rseq_offset/size/flags` are Rust statics in boundary/src/data.rs (exported under `runtime`), set by
`thread.rs::register_kernel_areas` when libc registers the initial thread's rseq area (in `Thread.rseq`).
ld.so exports only `LD_EXPORTS = ["__tls_get_addr"]` (xtask/src/so.rs, version script from versions.tsv: adding the
four names there gives GLIBC_2.2.5 / GLIBC_2.35 nodes). libc.so.6 is linked from libc.a with no `DT_NEEDED` on the
interpreter, so moving the objects needs: (1) libc.so.6 linked against ld.so (DT_NEEDED ld-linux-x86-64.so.2,
references bind to ld.so's objects; libc.a keeps its own definitions); (2) ld.so's `link` resolving that DT_NEEDED
to itself (check crates/ld search/map); (3) the versions.tsv set check in so.rs moving the four names from libc.so.6
to ld.so; (4) rseq: ld.so registers the initial thread's area and publishes the values, or libc keeps registering
and stores into ld.so's objects through the GOT (design §14 says ld.so registers; libc's descriptor owns the area:
decide where the initial thread's rseq area lives under ld.so, a D-27 row). Proofs: `__libc_stack_end`/`__rseq_*`
resolved from ld.so (dlsym/readelf on products), values non-zero in a dynamic program, static programs unchanged;
start-up counters static and dynamic.
