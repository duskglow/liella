# DL-INIT-ORDER (phase 14 DL-11): init/fini order, destructors at exit, entry transfer

State: branch `task/dl-init-order`, base origin/next 8253a107c. Commits: e6fca8868 (code + tests + probe),
5117d9444 (preinit rule in one place, plants file), a29f73eec (D-27.19, spec/behaviour/dynlink.md), then this note.
Decision in force: **D-27.19** (records/decisions/D-27/D-27.19.toml). Plan rows: DL-11 in
plan/06-phases/17-phase-14-dynamic-linking.md; design plan/ph14/design.md §9 and the start-up table.

## What was done
* `crates/ld/src/init.rs` (pure): `init_order`, `fini_order`, `hooks` (DT_PREINIT_ARRAY only for the program),
  `init_calls` (DT_INIT, then DT_INIT_ARRAY), `fini_calls` (DT_FINI_ARRAY reversed, then DT_FINI). `elf::dt` gains
  the INIT/FINI/array tags.
* `boundary/src/ld.rs` `link`: records the DT_NEEDED edges (including an object reached by name or by a second path),
  then `run_init` after RELRO: program preinit, shared objects' init in order with `(argc, argv, envp)`; the fini
  table goes into statics and `fini_all` (run once) is passed to the program in `%rdx`.
* `csys/src/machine/entry.S` `liella_machine_ld_enter(sp, entry, fini)`: no longer zeroes `%rdx`.
* Probe `tools/probe/dl/order.sh` (black box; -nostdlib corpus + the same programs linked with the C library).

## Probe-settled behaviour (reference loader, kanon, 2026-11-10)
Order rule: depth-first over objects in reverse load order, post-order over DT_NEEDED, a cycle member being walked is
passed over, root last; fini = the same walk over all loaded objects, reversed (program first). Fits: diamond (DL-0b),
ties (R, P, Q), sibling loaded early (M, K, N, L), start cycle H<->I (H, I), 3-cycle (W, Y, X, Z), DL-0b's dlopen cycle
(I, H) and DL-0b's exit with E open. The loader runs the program's preinit only, not its init_array/constructors; a
shared object's DT_PREINIT_ARRAY is not run. The reference's `%rdx` function cannot be called without its C library
(status 127), so exit lines come from the `_libc` variants. Details: spec/behaviour/dynlink.md "Order without a C library".

## Results
See handoff/DL-INIT-ORDER-report.md (gate, plants, counts).

## Not done / next step
1. The DL-0b libc corpus (`dlorder`: atexit and `__cxa_atexit` interplay, dlopen/dlclose) cannot run under Liella's
   ld.so until DL-12 (TLS), DL-13 (libc early init; libc must run the program's DT_INIT/INIT_ARRAY when `init` is NULL
   and must not run its fini itself: `fini_all` covers the program) and the dlopen story. Run `dlorder` under both
   loaders then; the atexit ordering is libc's (`rtld_fini` registered first, so it runs after every atexit handler).
2. dlopen/dlclose: call `init::init_order` over the new set (root = the opened object; the root-last rule is already
   in it) and recompute `fini_order` over all loaded objects at exit (today the table is fixed at start).
3. A dynamic program without DT_NEEDED still enters with `%rdx` = 0 and no preinit (`run`'s second path).

## Don't redo
The order rule (six graphs probed); the retagging trick for a preinit array in a DSO (the linker refuses
`.preinit_array` in a shared object).
