# One entry per CPU class, chosen once at load, read-only afterwards (design)

Story LOADTIME-ENTRY-DESIGN (owner ruling 11-05, #3069, option b). Design only: this page changes no product code.
Base: `origin/next` at `06cefa93`. Sources: the ELF gABI and the x86-64 psABI (IFUNC, `R_X86_64_IRELATIVE`,
`PT_GNU_RELRO`), the Linux man pages (`mprotect(2)`, `mremap(2)`, `mmap(2)`), the Intel SDM (instruction set), and the
tree as cited. No C library's source was consulted.

## 0. The problem

On the AVX2 class, `memset` 16-128 B runs the SSE2 code (PARITY-MEMSET-SMALL-AVX2). That code is already short; an
AVX2 build is shorter (one `vpbroadcastb` instead of a four-instruction byte broadcast, and 32-byte stores), but a
per-call "which CPU?" test costs about what the AVX2 build saves (D-4.25 measured the fill's AVX2 build as slower at
33-128 B with the test and a Rust hop in front of it). A writable code pointer on the hot path is rejected (owner): it
gives any memory-write bug a target to redirect. So the code has to be chosen per CPU class with no test on each
call, and whatever holds that choice has to be read-only once the choice is made.

## 1. How a Liella entry is bound today

| Link | What the caller's `call memset` reaches | Evidence |
|---|---|---|
| Static (`libc.a`, the only mode that runs programs today, and the mode the benches compare) | A direct `call rel32` to the exported label, which **is** the kernel: `memset`, `memcpy`/`memmove`, `strcmp` are second labels on `liella_machine_*` under `LIELLA_EXPORT` | `csys/src/machine/fold.S:28-60` (memmove/memcpy), `:269-284` (memset); `csys/src/machine/scan.S:3225-3242` (strcmp) |
| Dynamic (`libc.so.6`) | Nothing runs yet: `crates/ld` is a placeholder (phase 14, `crates/ld/src/reloc.rs:3` "Not yet implemented"). When it runs, calls from a program go through its PLT/GOT (or `call *GOT` with `-fno-plt`) as for any shared object | `docs/architecture.md:559` |
| Inside libc | rustc's and the C layer's implicit `memcpy`/`memset`/`memmove` calls bind straight to `liella_internal_*` (D-X.15); other internal users call the hidden `liella_machine_*` names directly | `plan/07-decisions/x-implementation-decisions.md` D-X.15; `crates/boundary/src/hot.rs:743,809` |

How the CPU class is applied today (D-4.14, D-4.25, D-4.45): a **per-call data test**. `cpu::init_with` (static
start-up step 4, `crates/boundary/src/runtime.rs`, the `// 4. Tunables and the CPU paths.` step; `cpu.rs:54-75`)
writes `liella_machine_avx2` (a byte in `.bss`, `fold.S:1351-1356`) and `liella_machine_scan_level` (`.data`,
`scan.S:18-25`). The kernels test them on the classes that have an AVX2 build: `cmpb $0, liella_machine_avx2(%rip)`
then a branch (memset 129-256 at `fold.S` `.Lset_129`, the copy's 65-256, the compare classes through the
`liella_machine_cmp_*` words), and `cmpl $0, liella_machine_scan_level(%rip); jne` at the head of **every** `strcmp`
call (`scan.S:3241-3242`). The words are 0 (the SSE2 baseline, correct on any x86-64) until step 4, so calls before it
are correct. They stay writable for the whole process, but they hold data, not code addresses.

When a load-time choice can be made:

* **Static:** at step 4, right after `cpu::init_with` (the tunables, `glibc.cpu.hwcaps=-AVX2` included, are decided
  there). The process has one thread, no user code has run, and the program's own IFUNC resolvers
  (`apply_irelative`) and every constructor run after it. Libc code that runs before step 4 runs the baseline. The
  program's RELRO is protected after it (`protect_relro`, D-1.13). **This is the only binding that can take the
  choice today.**
* **Dynamic (phase 14):** either Liella's `ld.so`, while it relocates `libc.so.6` (before RELRO and before any
  constructor; IFUNC resolvers run then, but with no tunables unless the loader passes them, which is the problem
  D-10.50 had for libm), or libc's own initialisation, which Liella's loader runs before any other object's
  constructors. The second gets the same decision as the static start-up.
* **Internal calls:** they never go through the exported entry (D-X.15), so the choice does not reach them unless
  the internal entries use the same mechanism. They are correct with either body.

## 2. Mechanisms

Instruction counts are on the hot path of one call, relative to today's direct call into the kernel. The "+1 jmp"
of M2 is an unconditional direct `jmp` (cheap, but the instruction counter counts it).

| | M0 today: data flag tested per call | M1 IFUNC + RELRO | M2 patched direct `jmp` at the entry | M3 read-only table, indexed once | M4 per-class body installed in the entry slot (recommended) |
|---|---|---|---|---|---|
| Per call, SSE2 class | +2 (`cmp`+`jcc`) on each class with an AVX2 build, 0 elsewhere | static: +1 (`jmp *GOT` in the `.iplt` stub) on **every** class; dynamic: +0 (the PLT `jmp *GOT` is paid anyway) | 0 (the entry is the SSE2 body; nothing is patched) | +1 (`jmp *word(%rip)`) on every class; +2 if the index is loaded per call | 0 |
| Per call, AVX2 class | +2 on those classes | as SSE2 | +1 on **every** class, including those with no AVX2 gain (memset < 16, copy 1-64) | as SSE2 | 0 |
| What is writable, how long | the flag bytes, the whole process (data: they can switch to the AVX2 code on a CPU without it, a crash, never a redirect) | the GOT word until RELRO is protected (static: step 4 + `protect_relro`; dynamic: the loader, provided IFUNC slots are bound eagerly, never lazily) | one text page, RW during start-up, RX after | the pointer word until RELRO | the live text page is **never** writable: a private copy is patched, made RX, then `mremap`ed over it (section 2.1) |
| Early calls (before the choice) | baseline: correct | static: a call before `apply_irelative` jumps through an unfilled word (none happen today: internal calls bind to `liella_internal_*`; constructors run after); dynamic: resolved before any code runs | baseline: correct | static non-PIE: the word's link-time value is the SSE2 body: correct; dynamic: needs the loader to write it before RELRO, i.e. an IFUNC by another name | baseline: correct |
| CPU without the feature | flag stays 0 | resolver returns SSE2 | not patched | word stays SSE2 | not installed |
| Tunables (`-AVX2`) | honoured (step 4) | static: yes (resolver runs after step 4); dynamic: only if the loader passes them | honoured | honoured (static) | honoured |
| Static binary | works | works (`apply_irelative` exists), +1 per call | works | works (non-PIE: no relocation needed) | works |
| Policy failures | none | none | `mprotect` refused (seccomp, PaX MPROTECT, SELinux `execmod`) leaves SSE2; **a refusal of the second `mprotect` (back to RX) after the first succeeded leaves the page non-executable: the next call crashes** | none | any refused step leaves the live page untouched: SSE2 |
| Observable ABI | none | symbol type becomes `STT_GNU_IFUNC`; `&memset` in a static program is the PLT stub (NOT VERIFIED against the reference: no `readelf` on the build host) | none | none | none |
| Ivy Bridge rows | today's | **rise by 1** on every static row: excluded by this story's rule | fall by 2 on the flag-tested classes | rise by 1: excluded | fall by 2 on the flag-tested classes |

Considered and set aside: **M5**, mapping the AVX2 page of the executable file over the entry page (no writes at all,
zero cost, page stays shared), fails because the AVX2 copy's PC-relative references were linked for its own address;
it would need a page-aligned file layout and position-independent bodies (M4's fixups, plus an fd of the own file).
**M3 with a per-call index** (`and $1` + `jmp *table(,%rax,8)`) costs as much as M0 and adds an indirect branch.

### 2.1 M4 in detail

* **Layout.** The entries that have per-class builds live in one page-aligned section, `.text.liella_classed`: each
  exported label is the start of a fixed-size **slot** holding the SSE2 body (the slots keep D-4.33's 64-byte
  alignment). The AVX2 bodies sit in ordinary text with the same slot sizes; `.org`/`.ifne` checks in the assembly
  fail the build when a body outgrows its slot. Neither body tests a flag: each is straight code for its class.
* **Fixups.** A body moved from its own address A to the slot at S keeps its internal jumps but its PC-relative
  references outside the slot (`liella_machine_fold_limit`, the tail jumps to `liella_hot_fill_rest` and friends)
  are off by A - S. The assembly emits, next to each AVX2 body, a `.rodata` table of the offsets of those `rel32`
  fields (a label after each such instruction, `.long label - body - 4`); the installer adds A - S to each. Nothing
  else in a body is position-dependent (no absolute addresses: `libc.so.6` is PIC).
* **Install protocol** (start-up step 4, after `cpu::init_with`, one thread, before `apply_irelative` and every
  constructor): (1) open the object's own file (static: `/proc/self/exe`; `libc.so.6`: its path from the loader),
  (2) `mmap` the page holding `.text.liella_classed` from that file, `MAP_PRIVATE`, `PROT_READ|PROT_WRITE`, at a new
  address, (3) copy each AVX2 body into its slot and apply its fixups, (4) `mprotect` it `PROT_READ|PROT_EXEC`,
  (5) `mremap(copy, 4096, 4096, MREMAP_MAYMOVE|MREMAP_FIXED, live)` (one call replaces the live page), (6) close.
  Any failure before (5) unmaps the copy and leaves the live page as it was (SSE2, correct). W^X holds throughout:
  the copy is writable only before it is executable, and the live page is never writable. The copy stays
  file-backed, so profilers still name `memset` in it. Cost on the AVX2 class only: about 6 system calls and one
  page of private memory per process; the SSE2 class pays nothing. Under `ld.so` for another C library (not a
  supported configuration) the same code runs in libc's initialisation.
* **FP and feature state.** The bodies use integer and vector moves only (no MXCSR or x87 dependence). A body that
  touches `ymm` ends with `vzeroupper` (no SSE/AVX transition penalty in the caller); the 16-32 B class uses VEX-128
  only and needs none. AVX2 is installed only when `liella_rt::cpu::Features` reports AVX2 **and** XCR0 shows the OS
  saves the YMM state (the existing D-4.25 proof obligation, now on the installer). AVX-512 CPUs take the AVX2 class.
* **Threads and signals.** Installation happens before any thread or handler can exist (static step 4). If a future
  path ever finds more than one thread, it does not install.

### 2.2 Testing, both entries on every host

1. **Test builds** (no `LIELLA_EXPORT`) name both bodies, `liella_machine_<f>_sse2` and `liella_machine_<f>_avx2`;
   the existing kernel tests (`crates/boundary/tests/small_copy.rs`, `small_compare.rs`, the memset cases) run every
   case on each body. The AVX2 body is skipped, with the reason printed, only on a CPU without AVX2.
2. **The installed export**: a static conformance program runs its sweep twice, by default and under
   `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2`, and asks a test-only selftest query which body each slot holds (the
   slot's bytes against the AVX2 body with fixups applied). On an AVX2 host the default run must report AVX2 and the
   tunable run SSE2; on Ivy Bridge both report SSE2. So every host exercises the SSE2 export, and every AVX2 host
   both.
3. **Failure injection**: a test hook fails step (2), (4) or (5) in turn; the program must still run, report SSE2
   and give correct results.
4. **Early calls**: `conformance/c/s04_strings_early_copies.c` (constructor sweep, static) stays; a call made before
   step 4 is covered by the start-up's own copies.

## 3. Scope and predicted effects

Counted from today's paths in `fold.S`/`scan.S` (instructions per call inside the entry; the bench rows add the
loop). AVX2-class figures are the latest in `handoff/parity-rows-avx2/summary.md` (Skylake-SP class) and the
PARITY-MEMSET-SMALL-AVX2 Haswell-EP ladder; glibc in brackets.

| Function, class | Today | With M4, AVX2 class | With M4, SSE2 / Ivy Bridge | Row prediction |
|---|---|---|---|---|
| memset 16-32 | 14 | 12 (`vmovd`+`vpbroadcastb xmm` for 4, VEX-128) | 14 | ladder 16/31: 27 -> 25 [24], 1.04 |
| memset 33-64 | 14 | 11 (two 32-byte stores + `vzeroupper`) | 14 | `memset_64` 20.3 -> 17.3 [18.3], 0.95 |
| memset 65-128 | 21 | 16 (four 32-byte stores + `vzeroupper`) | 21 | `memset_128` 27.4 -> 22.4 [24.4], 0.92; ladder 65/128 34 -> 29 [32] |
| memset 129-256 | flag test + body | -2 (no test) | -2 | ladder 129/256 37 -> 35 [39] |
| memset < 16, > 256 | | 0 | 0 | unchanged |
| memcpy/memmove 65-256 | flag test + body | -2 | -2 | `memcpy`/`memmove` mid rows -2 both classes |
| memcpy/memmove 1-64 | | 0 (stays SSE2: D-4.25 measured AVX2 slower at 33-64) | 0 | unchanged |
| strcmp, every size | `cmpl`+`jne` at the head | -2 | -2 | `strcmp_equal_16` 31.0 -> 29.0 [27.0]; `strcmp_16_mix` 52.5 -> 50.5 [42.8]; `strcmp_names` 67.0 -> 65.0 [57.3] |
| memcmp/bcmp/`__memcmpeq` 65-2048 | class words combine the fold and AVX2 decisions (D-4.52) | 0 to -2 (the words keep the fold part, which is decided lazily) | 0 to -2 | NOT VERIFIED: settle in the story |
| strlen and the other scanners | the level test is folded into the page-limit compare (D-4.45) | 0 | 0 | no gain: out of scope |

No Ivy Bridge row rises under M4: the SSE2 bodies are today's minus the flag tests. Rows still above parity after M4
(strcmp mix, memcmp) are the shape of the bodies, for PARITY-STRCMP-AVX2 and PARITY-MEMCMP-AVX2, which can then add
AVX2 code with no per-call test. Counts are a prediction: D-4.25 timed the fill's AVX2 build slower at 33-128 B (with
the test and a Rust hop), so the memset story also times cycles per call and keeps only rows that do not get slower.

## 4. Recommendation and plan

**Recommendation: M4** for `libc.a` now, and the same installer in `libc.so.6`'s initialisation once phase 14
loads it (one mechanism, one test matrix). It is the only mechanism with zero per-call cost on both classes, no Ivy
Bridge rise, no change to symbol types, correct early calls, and no writable code at any time in the live mapping.
M1 (IFUNC + RELRO) stays the fallback for `libc.so.6` if the start-up system calls measure too dear there (a PLT
call already pays its `jmp *GOT`); it is excluded for `libc.a` by the Ivy Bridge rule. M2 is excluded by its +1 on
classes without a gain and its non-executable-page failure mode.

Stories (the coordinator splits further as needed). JOBS labels follow the story id.

1. **CLASSED-SLOT-INFRA** (wiring, M): the `.text.liella_classed` section, slot size checks, fixup tables, the
   installer (protocol 2.1) at step 4, the selftest query and the failure-injection hook; `memset` moves into a slot
   with its SSE2 body unchanged except the 129-256 flag test, and an AVX2 body identical to it (so behaviour and
   counts are unchanged except -2 at 129-256). JOBS: `-gate` (direct tier), `-plants`, `-count-avx2`, `-count-ivy`
   (`cargo xtask bench --compare --counters --runs 3 cbench_memref` and a start-up row, syscalls counted). Plants: drop
   one fixup entry (the AVX2-host export sweep must fail); drop the XCR0 condition from the install decision (the
   test that forces `Features` without the YMM state and expects SSE2 must fail); skip the `mremap` but report installed (the selftest's byte comparison must
   catch it); make step (4) failure leave the copy mapped over the live page (failure-injection test must catch).
2. **MEMSET-AVX2-SLOT** (kernel, S): the AVX2 body for 16-256 as in section 3. JOBS: `-gate`, `-plants`, `-count-avx2`,
   `-count-ivy`, `-percall` (`validation/wl_percall.sh` with `validation/percall/avx2_families.c`, before and after),
   plus cycles per call on a booked AVX2 timing host. Plants: wrong second store offset in 33-64 (the AVX2 body test
   catches), missing `vzeroupper` (counted, perf-only: EQUIVALENT with reason or a count assertion), broadcast from
   the int's high byte (the fill-value test catches).
3. **COPY-SLOT** (kernel + wiring, S): `memcpy`/`memmove`/`mempcpy` 65-256 into slots, flag tests removed. JOBS as 2.
   Plants: AVX2 body loads after the first store on overlap (overlap tests catch), SSE2 body jumping to the AVX2 129-256
   class (the SSE2-body test with AVX2 reported absent, and the Ivy Bridge run, must fail).
4. **STRCMP-SLOT** (kernel + wiring, S): `strcmp` (and `strncmp`/`strcasecmp` where they test the level) into slots.
   JOBS as 2. Plants: AVX2 body's page test off by one (the page-end tests catch), the SSE2 body jumping to the AVX2
   loop (Ivy Bridge run and the SSE2-forced run catch).
5. **CMP-SLOT** (kernel, M): the compare and equality classes, after settling how the D-4.52 class words split
   between the fold part (stays a word) and the CPU part (goes into the slot). JOBS as 2.
6. **Phase 14 note** (no story now): the loader runs libc's initialisation (and so the installer) before any other
   object's constructors; record it in the phase-14 plan.

### Decision-register entry (next free at this base: D-4.53)

| D-4.53 | §4.1–4.3 | PERF | **One entry per CPU class, installed once at start-up, never writable in place** (LOADTIME-ENTRY-DESIGN, owner ruling #3069 option b; design `docs/design/loadtime-entry.md`). Entries with CPU-variant builds (memset, the copy entries, strcmp, then the compares) live in fixed-size slots in one page-aligned section `.text.liella_classed`, each slot holding the SSE2 body (correct on every x86-64, and what runs before start-up step 4). At step 4, after `cpu::init_with` and before the program's IFUNC resolvers and constructors, on a CPU whose `Features` report AVX2 with the YMM state saved and no `glibc.cpu.hwcaps=-AVX2`, the start-up maps a private copy of that page from the object's own file, writes the AVX2 bodies with their PC-relative fixups, makes it read+exec, and `mremap`s it over the live page; any failure leaves the live page untouched. No per-call CPU test, no code pointer, the live text never writable. Chosen over the D-4.25 flag (two instructions per call on tested classes), IFUNC + RELRO (+1 instruction per static call on every class, Ivy Bridge included, and a symbol-type change), a patched `jmp` (+1 on every AVX2-class call and a non-executable page if the second `mprotect` is refused) and a RELRO pointer (+1 per call everywhere). Amends D-4.25's mechanism for the entries moved. Tests: both bodies in test builds; the installed export by default and with `-AVX2`; failure injection. |

**Compat-flag note:** none needed. Results are bit-identical, symbol names, versions and types do not change, and
`&memset` is the same address before and after installation. The `glibc.cpu.hwcaps=-AVX2` tunable keeps the SSE2
bodies, as it keeps them today.

### RULING NEEDED

1. **Replacing a text page at start-up** (trust boundary): M4 never makes the live page writable, but it does put a
   private, patched copy of libc text in place (file-backed, W^X kept). Recommendation: approve; it meets "chosen
   once, read-only afterwards" with no per-call cost, and every failure falls back to the baseline.
2. **Start-up cost on the AVX2 class** (about 6 system calls and one private page per process, every program,
   whether or not it calls the functions). Recommendation: accept if CLASSED-SLOT-INFRA's start-up row shows no
   measured slowdown beyond those system calls; otherwise fall back to M1 for `libc.so.6` only.
3. **Hardening the remaining start-up words** (informational, optional): `liella_machine_avx2`, `scan_level` and
   the `cmp_*` words stay writable for the process lifetime. Once the entries move into slots they lose readers;
   the ones left (the fold limit is decided lazily, STARTUP-1) could move into RELRO in a static program.
   Recommendation: a small follow-up story after story 5.
