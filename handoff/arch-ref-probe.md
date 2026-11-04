# ARCH-REF-PROBE (A-5, OD-10): layouts-aarch64 generated, abi-gen per-arch; aarch64 cross check RED in pack.rs

State: branch `task/arch-ref-probe-2` (run 2; carries run 1's commits from `task/arch-ref-probe`, base origin/next
00a8aa87). HEAD: see `git log -1` (WIP commit). Plan item: plan/06-phases/20 A-5. Rulings in force: RL-2, RL-3, and the
coordinator note #2574 (arm64 linux-libc-dev 6.8.0-147.147 unpacked into keke:~/ref/aarch64). No D-row yet.

## Done (runs 1 and 2)

* Run 1: `spec/tools/abi_extract.py --arch aarch64` (compile-only cross mode); versions-aarch64.tsv,
  exports-diff-aarch64.tsv. Rerun in run 2: both byte-identical.
* Run 2: `python3 -I spec/tools/abi_extract.py --arch aarch64 --sysroot ~/ref/aarch64` on keke (scratch copy of spec/ in
  ~/work/arch-ref-probe-2): `spec/abi/layouts-aarch64.tsv` (160 types, 1313 rows), `initializers-aarch64.tsv` (10),
  PROVENANCE-aarch64 updated. Skips: user_fpregs_struct (x86_64 only), plus ustat, vtimes, vlimit and cpu_features, which
  x86_64 also skips. Type set = x86_64's minus `struct user_fpregs_struct`.
* Probe-settled (compile-only constants, aarch64-linux-gnu-gcc --sysroot ref): plain `char` unsigned, `wchar_t` unsigned,
  `__jmp_buf` element 8 bytes unsigned (array 176 bytes: 22 words). `mcontext_t.__reserved` is at 288 with the type
  aligned 16 (past the natural end 280 of `pstate`).
* xtask/src/abi.rs: `Arch` table (x86_64, aarch64); `load`/`render` per arch; `scalar_on`/`alias_on` (aarch64 char -> u8,
  wchar_t -> u32, __jmp_buf -> [u64; 22]); `ALIGNED_MEMBERS` (newtype with repr(align)) for mcontext_t.__reserved; the test
  header item count is computed; each test file carries `#![cfg(target_arch = ...)]`. Generated:
  crates/abi/src/layout_aarch64.rs, crates/abi/tests/appendix_c_layouts_aarch64.rs. x86_64 layout.rs byte-identical;
  appendix_c_layouts.rs gains only the cfg line and ", x86_64" in the count line. `abi-gen --check` up to date on keke.
* crates/abi/src/lib.rs `#[cfg_attr(target_arch = "aarch64", path = "layout_aarch64.rs")] pub mod layout;`;
  appendix_c/table.rs includes the per-arch tsv; xtask/src/arch.rs allows "cfg" in crates/abi/.

## RED: `cargo xtask arch-check` (liella-abi, aarch64): 15 errors, all in crates/abi/src/pack.rs

pack.rs is hand-written against the x86_64 layout: `use crate::layout::struct_utmp__ut_tv` (no such type on aarch64),
`chars()` returns `[i8; N]` where aarch64 `char[N]` is `[u8; N]` (ut_line/ut_id/ut_user/ut_host), `ut_session` i64 on
aarch64 vs i32. Only the first 6 errors were read (log: keke:~/work/arch-ref-probe-2/cross.log).

## Next step

1. Read cross.log fully; make pack.rs arch-neutral: char arrays through a per-arch `c_char` (check ctypes.rs), utmp's
   ut_tv/ut_session per `target_arch` (generated names differ), each site `ARCH(x86_64):`-tagged or allowed.
2. keke: `sh ~/work/arch-ref-probe-2/gen.sh` (the role is not a git repo: drop its `git diff` lines; diff locally after
   scp). It runs abi-gen, arch-check, and the aarch64 layout tests under `qemu-aarch64 -L /usr/aarch64-linux-gnu`
   (that run never produced qemu.log in run 2, so check why). Those tests are the real proof of the layouts.
3. Check `cargo xtask stubs` does not rewrite appendix_c_layouts.rs (xtask/src/spec.rs:599 emits that path too).
4. Jobs: arch-ref-probe-2-static, -cross (`cargo xtask arch-check`), the direct-tier gate; clippy and static in the foreground.

Not to redo: the probes (tables are final), the signedness probe, the generator's arch split.
