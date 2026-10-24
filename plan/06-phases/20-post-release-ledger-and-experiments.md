<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Post-release ledger and experiments

* **Other architectures** (owner, OD-10, 2026-10-04): aarch64 first.
* **`libmvec` placeholder** (owner, OD-11, 2026-10-04): a `libmvec.so.1` with scalar fallbacks, so
  that binaries listing it as a dependency still load. When the owner's Devuan mirror is complete,
  its binaries' dependencies are scanned to see whether any package needs it.


Owner direction (2026-10-04): native interface only, opt-in, never behind standard symbols.
GPU-assisted batch math and large matrix multiplication, probably as a companion crate, with a
complete CPU fallback, documented determinism differences, a reported crossover size, and no
effect on the core's zero-dependency policy. Batch math functions over slices are new native
interface: an owner ruling before any is added.

#### Stories (LEDGER-PLAN, 2026-10-23)

This is a plan only; no code was written for it. The tree was checked at `6abb25d6` (origin/next). It covers the three ledger items above and
the open rows of `docs/saturation-register.md` (§7 "not near the limit (for the brainstorm)", §8
unmeasured claims, §9 gaps; the register has no section literally titled "out of the box", so
those three are its idea rows). Each story fits one 50-call run unless sized **L**; split an L
story before dispatch. **Every story names its verified jobs.** The floor for all of them, docs-only
ones included, is `cargo xtask static` (job `<story>-static`); code stories add the direct-tier
gate (`<story>-gate`) and plants on their own code (`<story>-plants`, every plant CAUGHT or
EQUIVALENT with a reason). Owner rule (2026-10-23): rulings never block work that is cheap to
unwind, so each story says what it can do before its ruling.

**What exists today for a second architecture.** Checked in the tree, and with `command -v` on
sumire and keke on 2026-10-23:

* **Assembly.** 21 files, 8416 lines, all in `csys/src/machine/*.S` and all x86_64 (K2: no
  logic in assembly). `scan.S` (6150 lines, 117 labels) and `fold.S` (1026) are the SIMD string
  kernels. The rest covers process entry, the signal restorer, setjmp and context switching,
  thread start, the thread pointer (`tp.S`), vDSO calls, vfork, calling code at an address,
  barriers and once, CPUID/XGETBV, MXCSR/x87 (`fpu.S`), PKRU, RDTSC and traps.
* **Rust side.** `crates/` has nine `target_arch` sites, and only one of them is a non-x86_64
  branch (`boundary/src/sys.rs`). Everything else assumes x86_64 without saying so.
* **System calls.** `csys/syscalls.tsv` is the single source for `csys/src/sys/*.c`,
  `liella_sysnr.h` and `ffi_sys.rs` (`cargo xtask csys-gen`). Its numbers come from
  `asm/unistd_64.h`. aarch64 uses the kernel's generic table, which has none of these entries:
  `arch_prctl`, `ioperm`, `iopl` and `modify_ldt` (all x86-only); `time`, `alarm`, `pause`,
  `getpgrp`, `dup2` and `ustat` (legacy calls). A-3 checks this list against the UAPI header.
  The argument order of `clone` is set per architecture in the kernel.
* **ABI layouts.** `spec/abi/layouts.tsv` and the generated `crates/abi` (`cargo xtask abi-gen`)
  describe x86_64 only (Appendix C). These types are expected to differ on aarch64 and are
  probed, not assumed: `struct stat`, `ucontext_t`/`mcontext_t`, `jmp_buf`, `va_list`,
  `fenv_t`, the pthread object sizes, the packing of `struct epoll_event`, and `long double`
  (IEEE binary128 under AAPCS64, against x87 80-bit on x86_64).
* **`long double`.** 63 files in `crates/` handle the x87 80-bit format and 38 handle binary128
  (the `_Float128` work). On aarch64 the binary128 code becomes the `long double` path (`*l`
  math, `strtold`, printf `%L`), and the x87 code is x86-only.
* **TLS and loader.** Static TLS is variant II with glibc's `tcbhead_t` prefix (D-22.10, in
  `rt/src/tls.rs`), and the thread pointer is set through `arch_prctl`. aarch64 uses variant I
  (`TPIDR_EL0` points at a 16-byte TCB with the TLS blocks after it), and TLS descriptors are its
  default dynamic model (the public AAELF64 document). By the GCC documentation
  (`-mstack-protector-guard`), the stack-protector canary is a global symbol, not a TCB slot.
  `crates/ld` is a 49-line skeleton. `plan/ph14/design.md` names only `R_X86_64_*` relocations
  and `ld-linux-x86-64.so.2`; the aarch64 loader is `ld-linux-aarch64.so.1`.
* **Symbol versions.** `versions.tsv` is x86_64. aarch64's oldest version node is newer than
  x86_64's `GLIBC_2.2.5`; A-5 reads it from the reference.
* **Tools.** `rust-toolchain.toml` lists only `x86_64-unknown-linux-gnu`. Neither sumire nor keke
  (both x86_64) has `aarch64-linux-gnu-gcc`, clang or lld, qemu (user or system), an aarch64
  sysroot or a binfmt entry, and rustup there has the x86_64 target only. No other lab host was
  checked; all are believed to be x86_64.

**Stories, in order of value** (value for cost, dependencies respected; S < 25 calls,
M one run, L split first):

| # | Story | Size | Needs | Owner needs | Without aarch64 hardware |
|---|---|---|---|---|---|
| 1 | A-1 `ARCH-PH14-NEUTRAL` | S | none | RL-5 (rule after; cheap to unwind) | yes, docs only |
| 2 | A-0 `ARCH-SEAMS` | S | none | none | yes |
| 3 | X-1 `LEDGER-RECHECK-BENCH` | M | release head | a timing booking | yes (x86_64) |
| 4 | X-2 `STRCASE-AVX2` | M | X-1 helps | none | yes (x86_64) |
| 5 | A-2 `ARCH-RUST-CHECK` | M | A-0 | RL-1 | yes |
| 6 | A-3 `ARCH-SYSCALLS` | M | A-0 | none | yes |
| 7 | A-4 `ARCH-TLS-LAYOUT` | M | A-1, DL TLS story | none | yes (host-side logic) |
| 8 | M-0 `LIBMVEC-SCAN` | S | owner's Devuan mirror complete | the mirror | yes |
| 9 | A-5 `ARCH-REF-PROBE` | M | A-2 | RL-2, RL-3 | yes (compile-only probes) |
| 10 | A-6 `ARCH-CSYS-CROSS` | M | A-3, A-5 | RL-2 | yes (assembles, does not run) |
| 11 | A-7 `ARCH-QEMU-RUN` | M | A-6 | RL-2 (qemu-user) | emulation |
| 12 | A-8 `ARCH-STRING-PORTABLE` | M | A-2 | none | yes, and on x86_64 |
| 13 | A-9 `ARCH-FENV` | M | A-7 | none | emulation |
| 14 | A-10 `ARCH-LDOUBLE` | L | A-5, A-7 | none | emulation |
| 15 | A-11 `ARCH-SUITES` | L | A-7..A-10 | none | emulation |
| 16 | X-3 `REGEX-LAZY-DFA` | L | X-1 | none | yes (x86_64) |
| 17 | X-4 `COLD-FIRST-CALLS` | M | X-1 | none | yes (x86_64) |
| 18 | M-1 `LIBMVEC-PLACEHOLDER` | S | M-0 positive, DL-1, phase 14 | RL-6 | yes (x86_64 only) |
| 19 | A-12 `ARCH-DYNLINK` | L | phase 14 done, A-11 | none | emulation |
| 20 | A-13 `ARCH-NATIVE-LANE` | M | A-11 | RL-4 hardware | **no** |
| 21 | X-5 `WORDEXP-PROFILE` | M | X-1 | none | yes (x86_64) |
| 22 | X-6 `SMALL-OPEN-ROWS` | S | X-1 | none | yes (x86_64) |
| 23 | X-7 `BATCH-MATH-DESIGN` | S | none | RL-7 | yes, docs only |
| 24 | X-8 `BATCH-MATH-CPU` / X-9 `BATCH-MATH-GPU` | M / L | X-7 ruled | RL-7, GPU hardware for X-9 | X-8 yes, X-9 no |

**Why this order.** A-1 and A-0 come first because they are the cheapest and the cost of skipping
them grows: every phase-14 story that hard-codes x86_64 is later work for the port. X-1 is a
measurement that turns the register's **recheck** rows into data and may re-rank X-2..X-6. X-2
is the largest ratio gap on a common call (t 5.6 to 9.7). The aarch64 stages then follow the
owner's OD-10 priority in what the hosts can do without new hardware: stage 0 (A-0..A-4) needs
nothing new, stage 1 (A-5, A-6) needs a cross compiler, stage 2 (A-7..A-12) needs emulation, and
stage 3 (A-13) needs hardware. libmvec waits on the scan (M-0). The GPU work comes last: it is an
experiment with no named workload, and it needs a ruling and hardware.

##### Other architectures: aarch64 (OD-10)

Common proof for every A-story: `cargo xtask static`, and no x86_64 regression. The x86_64
direct-tier gate stays green, and A-0's seam check reports no new unmarked seam. From A-7 onwards
the story's own tests also run under `qemu-aarch64` (job `<story>-qemu`). Risk is **HIGH** for
A-4, A-6, A-7 and A-12: a wrong thread pointer, entry sequence or relocation means a process that
crashes before `main`, where nothing inside it can be debugged.

1. **A-1 `ARCH-PH14-NEUTRAL`** (S, docs only, RL-5). Amend `plan/ph14/design.md` and the DL
   stories so the parts that change per architecture live behind one table or trait from the
   start: relocation types, the TLS variant, the loader name, the PLT/TLSDESC resolvers (assembly
   in `machine/`) and the auxv/HWCAP reading. x86_64 stays the only implementation. Proof: job
   `arch-ph14-neutral-static`; a review checklist in the design that each DL story ticks.
   Risk: low. Unwind: delete the paragraph.
   *Done (ARCH-PH14-NEUTRAL, 2026-10-23):* design §0a (the `Arch` table and its checklist) and
   L14-13. The phase-14 stories carry a common "Arch seam" rule and per-story **seam:** notes.
2. **A-0 `ARCH-SEAMS`** (S, no deps). Add an xtask check, `arch-audit` (or a clause of
   `static`), that lists every x86_64 dependency: `target_arch`, `core::arch::x86_64`,
   `asm!`, `extern` declarations of `liella_machine_*`, the x87/MXCSR code, the
   `%fs`/`arch_prctl` uses and the `R_X86_64` names. Each must sit in an allowed module or carry an
   `ARCH(x86_64): why` tag. Write `docs/arch-seams.md` from its output: per seam, what replaces
   it on aarch64 and in which story. This also answers which crates build without compiling
   csys, and which string kernels have no portable Rust path. Proof: job `arch-seams-static`, plus a
   plant (an untagged `target_arch` in a test file) that the check must catch, in job
   `arch-seams-plants`. Risk: low.
3. **A-2 `ARCH-RUST-CHECK`** (M, after A-0, RL-1). Run `rustup target add aarch64-unknown-linux-gnu`
   (user-level, in `~claude`), add the target to `rust-toolchain.toml`, and make
   `cargo check --target aarch64-unknown-linux-gnu` pass for the crates that do not compile csys
   (from A-0's list: expected to be `core`, `math`, `abi` and `ld`). x86_64 code goes behind `cfg`, and
   an aarch64 `compile_error!` stub marks each item that is not ported. Proof: job
   `arch-rust-check-cross` (the cross `cargo check`, rc 0), plus `-gate` (x86_64 direct tier
   unchanged). Risk: medium (wide `cfg` churn). Before RL-1 is ruled, use plain armv8.0-a.
4. **A-3 `ARCH-SYSCALLS`** (M, after A-0). Give `csys/syscalls.tsv` an arch column (or a second
   number column) taken from the kernel's `asm-generic/unistd.h` (kernel UAPI, which is allowed).
   `csys-gen` emits per-arch `liella_sysnr.h` and `ffi_sys.rs`. The calls missing on aarch64
   move to generic replacements, chosen by black-box probes of the reference on x86_64 where
   behaviour could differ: `dup2` to `dup3` with its equal-fd case, `pause` to `ppoll`, `alarm` to
   `setitimer`, `time` to `clock_gettime`/vDSO, `getpgrp` to `getpgid(0)`. The x86-only exports
   follow the reference's aarch64 export list (RL-8). Proof: a generator test that every wrapper
   has a number for every arch or an explicit "absent"; the x86_64 output is byte-identical
   (`csys-gen` diff empty); jobs `arch-syscalls-static`, `-gate`. Risk: medium (the `clone`
   argument order: checked against the kernel source, which is allowed).
5. **A-4 `ARCH-TLS-LAYOUT`** (M, after A-1 and the phase-14 TLS story; HIGH). Make the static
   and dynamic TLS layout computation (`rt/src/tls.rs`, `liella_ld::tls`) take the variant as a
   parameter, as pure logic tested on the host for both variants (offsets, alignment,
   `PT_TLS` with large alignment, a zero-size block). Move the stack-protector canary and pointer
   guard access behind the arch table. Proof: host unit tests for variant I against the AAELF64
   formulas, `-gate`, and `-plants` on the offset arithmetic (off-by-alignment plants).
6. **A-5 `ARCH-REF-PROBE`** (M, after A-2; RL-2, RL-3). Unpack the reference's arm64 packages
   (RL-3) under `~claude/ref/aarch64`, without installing them. Then: (a) generate
   `spec/abi/layouts-aarch64.tsv` with **compile-only** probes (sizeof/offsetof as object-file
   constants, read with `readelf`; the headers are compiled, never read, exactly as the x86_64
   probes use the installed reference). (b) Build `versions-aarch64.tsv` and the export list from
   `readelf --dyn-syms` of the reference's arm64 `libc.so.6`/`libm.so.6`. This is symbol-table
   reading, not disassembly. Proof: `abi-gen` builds an aarch64 `crates/abi` that
   `cargo check --target aarch64...` accepts; jobs `arch-ref-probe-static`, `-cross`. Before
   RL-2 is ruled: the LLVM release tarball in `~claude` cross-compiles the probes (freestanding,
   no root).
7. **A-6 `ARCH-CSYS-CROSS`** (M, after A-3 and A-5; HIGH). Write `csys/src/machine/aarch64/*.S`
   for the non-SIMD primitives: entry, signal restorer, setjmp/longjmp, context, thread start,
   `TPIDR_EL0` read/write, vDSO call (`__kernel_*` names per the kernel docs), vfork, call at an
   address, barriers, `CNTVCT_EL0` (for `rdtsc.S`), and FPCR/FPSR (for `fpu.S`). CPUID becomes
   HWCAP from auxv, with no assembly. PKRU has no aarch64 counterpart in v8.0, so pkeys report
   ENOSYS/unsupported as the reference does (probe). `build.rs` selects the directory by
   `CARGO_CFG_TARGET_ARCH`; `csys-audit` covers both. Proof: csys cross-assembles and links into a
   static archive (job `arch-csys-cross-build`); `machine-exports` lists the same names per arch.
   Nothing runs yet.
8. **A-7 `ARCH-QEMU-RUN`** (M, after A-6; RL-2 qemu-user; HIGH). Set a cargo runner
   `qemu-aarch64 -L ~claude/ref/aarch64`, and get a static Liella program through `_start` to
   `main` to `exit`, with argv/envp/auxv correct. Then run the kernel-primitive suite
   (`appendix_a_primitives.rs`) under emulation. Proof: job `arch-qemu-run-qemu` (suite rc 0); a
   plant on the entry stack alignment, in job `-plants`. The x86_64 gate is unchanged. qemu-user does
   not model every kernel detail (signals on alternate stacks, some `/proc` files): record each skip.
9. **A-8 `ARCH-STRING-PORTABLE`** (M, after A-2). Portable Rust (SWAR) versions of every
   `scan.S`/`fold.S` kernel that A-0 lists without one, selected on targets without a tuned
   kernel. They are tested on x86_64 too, by forcing the portable path in the kernel test suites.
   NEON versions wait for hardware (A-13), because their value is a timing claim. Proof: jobs
   `-gate`, and `-plants` on the portable kernels (each plant CAUGHT by `small_copy` /
   `machine_scanners` named explicitly, since `affected` selects nothing for `.S` files).
10. **A-9 `ARCH-FENV`** (M, after A-7). `fenv.h`, `fpu_control` and `__fpu_control` on FPCR/FPSR,
    with the trap-enable bits reported as the reference does on hardware without trap support
    (a probe under qemu, re-probed on hardware in A-13). Proof: the fenv suites under qemu (job
    `-qemu`), plants on the rounding-mode mapping.
11. **A-10 `ARCH-LDOUBLE`** (L: split it into math, strtold/printf, and ABI). Route `long double`
    to binary128 on aarch64: the `*l` functions, `strtold`, printf/scanf `%L`, `nexttowardf`, and
    the `ldbl` exports, against `docs/libm-accuracy.tsv` limits re-measured for binary128.
    Proof: the math and stdio suites under qemu; job `-qemu` per part. Risk: medium. Much of it is
    the existing `_Float128` code, but accuracy limits per type must be re-derived and never copied.
12. **A-11 `ARCH-SUITES`** (L: split by spec area). Run every direct suite under qemu, triage, and
    record arch-specific skips with their reason in the suite. Proof: per part, job
    `<part>-qemu` rc 0 with the skip list.
13. **A-12 `ARCH-DYNLINK`** (L, after phase 14 and A-11; HIGH). Add the aarch64 relocations
    (`R_AARCH64_RELATIVE`, `ABS64`, `GLOB_DAT`, `JUMP_SLOT`, `IRELATIVE`, `TLS_DTPMOD64`,
    `TLS_DTPREL64`, `TLS_TPREL64`, `TLSDESC`), the PLT and TLSDESC resolvers in
    `machine/aarch64`, and `ld-linux-aarch64.so.1`. Same proof ladder as phase 14 (R14-17), under
    qemu. Plants on every bounds check.
14. **A-13 `ARCH-NATIVE-LANE`** (M, RL-4 hardware). Add the native host to the lab: the direct
    tier on hardware, re-probing the qemu-settled behaviour (fenv traps, signals), then the
    workload gate and per-call counts on aarch64 (OWNER rule 4: no workload slower), then NEON
    kernels for the hot rows, one story per kernel group. Proof: jobs `-gate` and `-wlc` on the host.

##### libmvec placeholder (OD-11)

1. **M-0 `LIBMVEC-SCAN`** (S, when the owner's Devuan mirror is complete). Unpack the mirror's
   amd64 packages in a scratch directory and list every ELF whose `DT_NEEDED` names
   `libmvec.so.1` (readelf only), together with the `_ZGV*` symbols it imports. Record the result
   in `docs/libmvec-scan.md`. Proof: job `libmvec-scan-static`, and the scan log in the job. If
   the scan finds nothing, the placeholder stays parked here with the scan result.
2. **M-1 `LIBMVEC-PLACEHOLDER`** (S, after a positive M-0, DL-1's library generator and phase 14;
   RL-6). Export the reference's `_ZGV*` names from `libmvec.so.1`, under the reference's
   version nodes (read from its dynsym), each a scalar loop over the lanes calling Liella's libm.
   The calling convention follows the public x86 Vector Function ABI document (vector registers,
   masked variants). The functions are more accurate but slower than vector code, and the
   differences row states this trade. A vectorised kernel is a later story, and only if M-0 names a
   workload. Proof: a test program that calls each width and mask variant against scalar `libm`
   bit for bit; readelf version check; jobs `-gate`, `-plants` (a lane-order plant, a
   mask-ignored plant).

##### Experiments (this file and the saturation register)

1. **X-1 `LEDGER-RECHECK-BENCH`** (M, a timing booking). A fresh full bench and the per-call
   counts of the release head, to replace the 2026-10-07/08 ratios. This settles §8's unmeasured
   claims where a count can: the glibc side of `qsort` per comparison, the `readdir` instruction
   ratio, the `memcpy`/`memset` 16/64 gap, and `strlen_64`/`strnlen_64`/`rawmemchr_64`. Update
   the register's **recheck** rows and re-rank X-2..X-6. Proof: jobs `-bench` and `-wlc`; the
   register diff cites them.
2. **X-2 `STRCASE-AVX2`** (M). Build the AVX2 `casecmp_ascii` kernel that the register's §7
   names for `strcasecmp`/`strncasecmp` (t 5.6 to 9.7, i 5.3 to 7.0), keeping the locale rules.
   Proof: `-gate`, `-plants` (case-fold boundary plants), and `-percall` showing the row's
   instructions per call fall with no other row slower.
   **Outcome (STRCASE-AVX2, 2026-10-23): already built.** The kernel exists (D-4.27: CASECMP-K1,
   CASECMP, STRCASECMP-SHORT, -2); the register's figures were from before it. Recount
   (`strcase-avx2-percall0`, keke): cycles 0.93 to 1.42x, instructions 1.12 to 1.74x glibc. The
   story added fold-boundary plants on `liella_machine_casecmp_ord_avx2` and corrected the
   register and `docs/benchmarks.md`. What is left is the fixed cost of short calls
   (`strcasecmp_c_16`, `strncasecmp_c_64`). That is a possible small story (S) after X-1, not
   part of this one.
3. **X-3 `REGEX-LAZY-DFA`** (L: X-3a a design note and a profile of the capture/back-reference rows
   at t 3.1 to 5.8; X-3b onwards implementation). Risk: high, because a DFA cache on hostile
   patterns is a DoS surface. Bound its memory, and fall back to the current matcher.
   Proof: the regex suites, the fuzz target for the matcher (a fixed-time run), `-percall`.
4. **X-4 `COLD-FIRST-CALLS`** (M). Profile first, then decide: `second_setlocale` (120.6),
   `second_bindtextdomain_gettext` (19.8) and `first_mktime` (10.6) are one-off costs, but
   short-lived processes (shell pipelines, `make`) pay them on every run. A first story measures
   their share in the workload set. Proof: job `-wlc` plus a profile note; code only if the share
   is real.
5. **X-5 `WORDEXP-PROFILE`** (M). The register says "no design for a larger change is recorded":
   a profile and a design note first. Proof: `-static` and the profile job.
6. **X-6 `SMALL-OPEN-ROWS`** (S). `execvp` makes the `Exec` dyn type generic (about 62 instructions per PATH
   directory, EXECVP-PATH-2), and overlapped `memmove_4k` (t 2.3) takes the checked path outside the
   small-copy kernel. Proof: `-gate`, `-plants`, `-percall` for both rows.
7. **X-7 `BATCH-MATH-DESIGN`** (S, docs only; RL-7). Design the companion crate for the owner
   direction above: its interface over slices, the CPU fallback, the determinism contract
   (bit-identical to the scalar function or a stated ulp bound), how the crossover size is
   measured, and the dependency boundary (the core stays zero-dependency). No exports. Proof: job
   `batch-math-design-static`. **Written** (2026-10-23): [`plan/batch-math/design.md`](../batch-math/design.md),
   PROPOSED. It has local decisions B-1..B-18 and owner questions BQ-1..BQ-4 under RL-7.
8. **X-8 `BATCH-MATH-CPU`** (M, after RL-7). The CPU crate only, using SIMD on x86_64, tested
   against the scalar functions. **X-9 `BATCH-MATH-GPU`** (L, owner GPU hardware): the GPU
   backend and the reported crossover size. Proof: `-gate`, plus a crossover bench job on the
   GPU host.

##### Rulings needed (with recommendations)

* **RL-1 aarch64 baseline.** Recommend armv8.0-a with ASIMD (NEON is mandatory there), and runtime
  HWCAP dispatch for LSE atomics, SVE and the rest, so Raspberry Pi 4-class cores still run.
* **RL-2 cross tools on keke** (root). The owner runs or approves:
  `sudo apt-get install --no-install-recommends gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu qemu-user`.
  This is keke only, never shiki. Recommend yes: it is distro-maintained and cheap to remove. Without
  it, an LLVM release tarball in `~claude` (no root) covers the freestanding csys and the probes,
  and qemu can be built from source in `~claude`.
* **RL-3 the aarch64 reference.** Name the glibc 2.39 arm64 build that is the reference
  (recommend the same distribution's arm64 `libc6`/`libc6-dev` packages as the x86_64 reference,
  unpacked under `~claude`, run only under `qemu -L`).
* **RL-4 aarch64 hardware.** A native host is needed for A-13 (timing, workload gate, NEON) and
  for an aarch64 CI lane; everything before it runs under qemu. Recommend one arm64 machine with
  8 or more cores (a server-class VM, or a Raspberry Pi 5 as a first lane), added once A-11 is green.
* **RL-5 phase-14 arch neutrality** (A-1). Recommend yes, now. It is docs only, costs each DL
  story a few lines, and saves a rewrite of the loader's arch-specific parts.
* **RL-6 libmvec scope.** Recommend: M-0 first. If no package needs `libmvec.so.1`, keep it
  parked. If any does, export exactly the reference's x86_64 `_ZGV*` list as scalar loops, with
  the speed trade in a differences row (the gain: accuracy, and loading at all).
* **RL-7 batch math / GPU.** Recommend X-7's design note only (no new interface yet). Approve
  the native slice interface only when a named workload asks for it. For X-9, the owner provides
  the GPU host and rules on the companion crate's dependency (the GPU API crate).
* **RL-8 x86-only exports.** `ioperm`, `iopl`, `modify_ldt` and `arch_prctl` exist only on
  x86_64. Recommend following the reference's per-arch export list exactly (A-5), and adding nothing
  aarch64 lacks.
