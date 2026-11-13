# DL-TLS-THREADS (phase 14 DL-15): threads with TLS in dynamically linked programs

Branch `task/dl-tls-threads` (stacked on DL-LIBC-START-10, cdafa3d1c). Code: 0460cc7fe; final code head 1f09927be
(reverts the cost fix bbe7c0530, see the report: hot-text keys). Role keke:~/w/n3 (Zen 1 host). Decision:
records/pending/DL-TLS-THREADS-1.toml (D-NEW, series D-27, amends D-22.10 and D-27.20). Do not redo: the cost fix
exists as bbe7c0530; re-apply it with tools/hottext/regen.sh in both profiles (ruling 1 in the report).

## What changed
- **Interface (design §7/§14 seam, changed):** ld.so passes its per-thread TLS interface in `StartInfo`
  (`tls_size`, `tls_align`, `tls_init`, `tls_release` as addresses), not as `LIELLA_PRIVATE` exports. Reason:
  an import would give `libc.so.6` a version need on ld.so's private node and stop it loading under another
  loader. libc keeps the interface only if it is usable (size > 0, align a power of two <= page, both hooks).
- **ld.so (`crates/boundary/src/ld.rs`):** `install_tls` now records the placed modules
  (`record_thread_tls`) and fills the initial area through `fill_static` (no zero writes on the fresh map).
  `thread_tls_init(tp)`: images, zeros to `p_memsz`, dtv in the lowest words of the area below TP
  (`tls_size` = initial area's bytes below TP + dtv words), the block's dtv word. `thread_tls_release`: no-op
  (static blocks go with libc's mapping; the place for DL-19's dynamic blocks and DL-16's dlerror record).
- **libc (`thread.rs`, `runtime.rs`):** `create_inner` reserves the loader's area as the TLS block (rt
  `Layout`), calls `tls_init` after writing the block and before the clone; a cached mapping is cleared from TP
  up only (tls_init rewrites the blocks; the 64 KiB surplus is not cleared); `finish` calls `tls_release` after
  the destructors. `start_under_liella_ld` forbids threads only without the interface. Under another loader
  nothing changes (D-22.10).
- **TLSDESC:** `R_X86_64_TLSDESC` applied as `[liella_machine_ld_tlsdesc_static, TPOFF]` (`Arch::tls_descriptor`,
  `Image::tlsdesc_static`, default None = still refused for the test doubles). `csys/src/machine/tlsdesc.S`:
  `_static` and `_dynamic` (branch-free; saves rcx..r11 and FXSAVE state, 64-byte aligned frame, calls
  `*(record)` with the record, subtracts %fs:0). Register convention in the file header.

## Tests
- `crates/boundary/tests/dl_tls_threads.rs`: `tlsdesc_functions_keep_every_register_but_rax` (C+asm oracle in
  tests/oracle/tlsdesc_*.{c,S}: both primitives forced by the caller, stack aligned/off by 8, clobbering and C
  resolvers, GPRs/xmm/MXCSR/FCW); `threads_get_static_tls_of_every_initial_module` (library with GD, LD, IE,
  TLSDESC; program LE; 3 rounds x 6 threads, half created by the library; fresh values, zeros, 64-byte alignment,
  per-thread errno, isolation; glibc reference and Liella via PT_INTERP and `ld.so PROGRAM`).
- `crates/ld/tests/tls.rs`: descriptor words in `tls_relocation_arithmetic_from_the_table`.
- `products::liella_link_script` factored out of `conformance_dynamic` (same behaviour).

## Open / not done
- The dynamic trampoline has no product caller yet (DL-19 creates the records and the resolver). FXSAVE suffices
  only while the resolver is baseline SSE2 code (no AVX); an AVX resolver needs an XSAVE variant.
- DL-16d (per plan/ph14/dl-16-dlopen.md §5): needs a live-thread list and a growable module table; today
  `tls_init` reads the start-time table only. Surplus bytes of a reused mapping are not cleared.
- Variant I (A-4): `record_thread_tls` assumes variant II (ARCH comment).

## Next step
Land. Then DL-16d / DL-19 build on `StartInfo::tls_*` and `liella_machine_ld_tlsdesc_dynamic`.

## Results
See handoff/DL-TLS-THREADS-report.md.
