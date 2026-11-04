# ARCH-REF-PROBE (A-5, OD-10): layouts-aarch64 generated, abi-gen per-arch; aarch64 cross check green

State: branch `task/arch-ref-probe-3` (run 3; carries runs 1-2 from `task/arch-ref-probe-2`, base origin/next 00a8aa87).
Code head 7f235823 (handoff/report commit on top). Plan item: plan/06-phases/20 A-5. Rulings in force: RL-2, RL-3, coordinator
note #2574 (arm64 linux-libc-dev 6.8.0-147.147 unpacked into keke:~/ref/aarch64). No D-row.

## Done (runs 1 and 2)

* `spec/tools/abi_extract.py --arch aarch64` (compile-only cross mode): versions-aarch64.tsv, exports-diff-aarch64.tsv,
  layouts-aarch64.tsv (160 types, 1313 rows), initializers-aarch64.tsv, PROVENANCE-aarch64. Type set = x86_64's minus
  `struct user_fpregs_struct`.
* Probe-settled: plain `char` and `wchar_t` unsigned on aarch64; `__jmp_buf` 22 x 8-byte words; `mcontext_t.__reserved`
  at 288, aligned 16.
* xtask/src/abi.rs per-arch generator; crates/abi/src/layout_aarch64.rs and tests/appendix_c_layouts_aarch64.rs
  generated; x86_64 layout.rs byte-identical.

## Done (run 3)

* crates/abi/src/ctypes.rs: `c_char` (u8) and `wchar_t` (u32) per `target_arch`; x86_64 unchanged (i8, i32).
* crates/abi/src/pack.rs: char arrays through `c_char`; `ut_tv`/`ut_session` through per-arch helpers (x86_64: the same
  D-17.5 code as before; aarch64: `struct timeval` and `long`, narrowed back to `liella::users::Utmp`'s i32 fields with a
  clamp). crates/legacy/src/printf_registry.rs builds `printf_info.spec/pad` as `ctypes::wchar_t`.
* docs/arch-seams.md: new row: the utmp record (384-byte x86_64 file codec in core/src/users.rs) is not ported to
  aarch64's 400-byte record; story not yet assigned.
* `cargo xtask stubs` does not rewrite appendix_c_layouts.rs: `emit` (xtask/src/spec.rs:414) keeps an existing file
  that has `#[test]` unless `--force`.
* Jobs (keke): arch-ref-probe-3-cross2 (abi-gen --check + arch-check, rc 0), arch-ref-probe-3-static (clippy -D warnings
  + xtask static, rc 0), arch-ref-probe-3-gate (direct tier, --base origin/next), arch-ref-probe-3-plants (2 plants on
  pack.rs's x86_64 utmp helpers, s17_users_clauses). Results: see the report json.
* Superseded: arch-ref-probe-3-cross (rc 1: liella-legacy printf_registry wchar_t, fixed in 7f235823).

## Open

* The aarch64 layout tests (appendix_c_layouts_aarch64 under qemu-aarch64) cannot link on keke: no aarch64 libc crt
  files (Scrt1.o, crti.o) under /usr/aarch64-linux-gnu; only the arm64 kernel headers are installed. This is why run 2
  never produced a passing qemu.log. Needs libc6-dev-arm64-cross (root install: owner ruling) or another aarch64 sysroot.
* aarch64 utmp record (see arch-seams row): needs a story.

## Next step

Ruling on the aarch64 libc sysroot for qemu-user tests; then run
`cargo test --target aarch64-unknown-linux-gnu -p liella-abi --test appendix_c_layouts_aarch64` with
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc` and `..._RUNNER="qemu-aarch64 -L <sysroot>"`.

Not to redo: the probes (tables are final), the signedness probe, the generator's arch split, pack.rs port.
