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
*(LOADTIME-ENTRY-DESIGN-2: M6, section 5, removes that objection. It assembles the AVX2 page for the SSE2 page's
address, so the references are right with no fixups, and it moves the already-mapped page with `mremap`, so no fd is
needed.)*
**M3 with a per-call index** (`and $1` + `jmp *table(,%rax,8)`) costs as much as M0 and adds an indirect branch.

### 2.1 M4 in detail

*Superseded by M6 (section 5). The probes show M4's step (4) is refused under `PR_SET_MDWE` and systemd
`MemoryDenyWriteExecute=`. Kept as the record.*

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

Amended by LOADTIME-ENTRY-DESIGN-2 (section 5). The original recommendation was M4. Sections 2.1 and 2.2 are kept
as the record of M4.

**Recommendation: M6** (section 5): both page variants pre-built in the file, both mapped read+exec, never written.
At start-up on the AVX2 class, a single `mremap` moves the AVX2 page over the SSE2 page, after a guard checks that
neither page has been modified since it was mapped. M4 is dropped. The probes (5.2) show that its step (4)
(`mprotect` to read+exec on the written copy) is refused under `PR_SET_MDWE` and under systemd
`MemoryDenyWriteExecute=yes`. SELinux `execmod` is documented to refuse it too. Under those policies a service would
stay on SSE2 without any message. M6 passes every probed policy. It has the same zero per-call cost as M4, costs 1
system call instead of 5 (4 with the guard, 8 for M4 with the guard), uses no private memory, opens no file, and does
not need `/proc` for the swap itself. Use it for `libc.a` now, and the same code in `libc.so.6`'s initialisation once
phase 14 loads it. M1 (IFUNC + RELRO) stays the fallback for `libc.so.6` only. M2 is excluded (section 4 as before).

Stories (the coordinator splits further as needed). JOBS labels follow the story id.

1. **CLASSED-SLOT-INFRA** (wiring, M): the `.text.liella_classed` section with its two pages (5.3), the `XREF` bias
   macro and the page-size checks, the guard and the swap at step 4 (5.4), the failure handling (5.5), and the
   `classed_id` slot (5.3). `memset` moves into a slot with its SSE2 body unchanged except the 129-256 flag test, and
   an AVX2 page body identical to it (so behaviour and counts are unchanged except -2 at 129-256). JOBS: `-gate`
   (direct tier), `-plants`, `-count-avx2`, `-count-ivy` (`cargo xtask bench --compare --counters --runs 3
   cbench_memref` and a start-up row with system calls counted). Tests: the export sweep by default and under
   `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2`, checked with `classed_id`. A guard test: a parent starts the program under
   `PTRACE_TRACEME`, writes back one unchanged byte of the live page at the exec stop (the page becomes anonymous, its
   content is the same), and the child must report SSE2. A failure-injection test for the `mremap`. A policy test: the
   swap under `prctl(PR_SET_MDWE, PR_MDWE_REFUSE_EXEC_GAIN)` set by a parent before `execve`, which must report AVX2 on
   an AVX2 host. Skip it, with the reason printed, where the kernel has no `PR_SET_MDWE`. Plants: drop the `XREF` bias
   on one out-of-page reference in the AVX2 page (the AVX2-host export sweep must fail); drop the XCR0 condition (the
   forced-`Features` test must report SSE2 and fail); skip the `mremap` but record "installed" (the `classed_id` query
   catches it); invert the guard (the guard test catches it); on an injected `mremap` failure, skip the check that the
   live page is still mapped (the failure-injection test catches it).
2. **MEMSET-AVX2-SLOT** (kernel, S): as before (section 3 bodies, now in the AVX2 page). JOBS: `-gate`, `-plants`,
   `-count-avx2`, `-count-ivy`, `-percall` (`validation/wl_percall.sh` with `validation/percall/avx2_families.c`, before
   and after), plus cycles per call on a booked AVX2 timing host. The plants are unchanged from the M4 plan.
3. **COPY-SLOT**, 4. **STRCMP-SLOT**, 5. **CMP-SLOT**: as before. Each new slot adds its body to both pages at the
   same offset. The AVX2 page must not jump into the SSE2 page (after the swap, that address holds the AVX2 page).
   Shared helpers live outside the section.
6. **Phase 14 note** (no story now): the loader runs libc's initialisation, and so the swap, before any other
   object's constructors. The swap needs no path or descriptor for `libc.so.6`. Record this in the phase-14 plan.
7. **Debugging note** (docs, XS, with story 1): a user-facing paragraph covering 5.6 (perf annotate, uprobes, core
   files, and how to keep SSE2 with `glibc.cpu.hwcaps=-AVX2`).

### Decision-register entry (reserved: D-4.53)

| D-4.53 | §4.1–4.3 | PERF | **One entry per CPU class: pre-built read-only pages, one swapped in at start-up, never written** (LOADTIME-ENTRY-DESIGN, -2; owner ruling #3069 option b; design `docs/design/loadtime-entry.md` sections 4-5). Entries with CPU-variant builds (memset, the copy entries, strcmp, then the compares) live in one page-aligned input section `.text.liella_classed` of two 4096-byte pages with the same slot layout: page 0 holds the SSE2 bodies at the exported labels (correct on every x86-64, and what runs before start-up step 4), page 1 the AVX2 bodies, assembled for page 0's address (every out-of-page PC-relative reference biased by +4096). At step 4, after `cpu::init_with` and before the program's IFUNC resolvers and constructors, on a CPU whose `Features` report AVX2 with the YMM state saved and no `glibc.cpu.hwcaps=-AVX2`, the start-up reads the two pages' `/proc/self/pagemap` entries and, if neither has become an anonymous (modified) page, runs `mremap(page1, 4096, 4096, MREMAP_MAYMOVE\|MREMAP_FIXED, page0)`. Any refusal or failure keeps page 0 (SSE2). A failed `mremap` that leaves page 0 unmapped ends the process with a message. No write to code, no gain of exec permission, no private memory, no per-call CPU test, no code pointer. Chosen over writing a patched private copy (M4: refused under `PR_SET_MDWE`, systemd `MemoryDenyWriteExecute=` and SELinux `execmod`; 5 system calls and one private page), the D-4.25 flag (two instructions per call on tested classes), IFUNC + RELRO (+1 instruction per static call on every class, Ivy Bridge included, and a symbol-type change), a patched `jmp` and a RELRO pointer. Amends D-4.25's mechanism for the entries moved. Tests: both bodies in test builds; the installed export by default and with `-AVX2`, read back by the `classed_id` slot; the guard under a tracer; `mremap` failure injection; the swap under `PR_SET_MDWE`. |

**Compat-flag note:** none needed. Results are bit-identical. Symbol names, versions and types do not change.
`&memset` is the same address before and after the swap. The `glibc.cpu.hwcaps=-AVX2` tunable keeps the SSE2 page,
as it keeps the SSE2 code today. The diagnostics effects in 5.6 are tooling behaviour, not ABI.

### RULING NEEDED

1. **Swapping a text page at start-up** (trust boundary): M6 moves one read+exec, file-backed page of libc text over
   another. Nothing is written and no permission is gained. Recommendation: approve. It replaces M4's item 1 and is
   strictly weaker: no writable code exists at any point, and it passes MDWE.
2. **Start-up cost on the AVX2 class:** 4 system calls (`openat`, `pread64` and `close` of `/proc/self/pagemap`, then
   `mremap`), no private page, and +2 VMAs. Recommendation: accept if CLASSED-SLOT-INFRA's start-up row shows no
   slowdown beyond those calls. Otherwise drop the guard's `close` by reusing a descriptor the start-up already
   holds, if one exists. The SSE2 class pays nothing.
3. **Guard policy when `/proc` is unavailable** (chroot or container without `/proc`): recommendation: keep SSE2. That
   is always correct, it only loses speed, and the alternative is a crash when a debugger breakpoint was inserted
   before start-up (5.2, the debugger rows).
4. **Diagnostics trade-off** (informational, 5.6): on AVX2 hosts, `perf annotate` and core files show the file's
   SSE2 bytes for a classed slot, and uprobes placed on the exported symbol do not fire after the swap. M4 has the
   last two as well. Recommendation: accept, with the debugging note (story 7).
5. **Hardening the remaining start-up words**: as before (optional follow-up after story 5).

## 5. Addendum: pre-built per-class pages (M6), checked against hardening policies

Story LOADTIME-ENTRY-DESIGN-2 (owner 11-05). Additional sources: `prctl(2)` (`PR_SET_MDWE`, Linux 6.3+), `mremap(2)`,
`mmap(2)`, the kernel's `admin-guide/mm/pagemap.rst` (bit 61 "page is file-page or shared-anon", bit 63 "present"),
`systemd.exec(5)` (`MemoryDenyWriteExecute=`), the SELinux Notebook (file permissions `execmod` and `execmem`), and
the kernel's `trace/uprobetracer.rst`. Black-box probes, described below. No C library's source was consulted.

### 5.1 What hardening policies refuse

M4 maps a private copy of a file-backed text page as read+write, writes into it, then calls `mprotect` to make it
read+exec. That is a gain of exec permission on memory that has been written, which these policies exist to stop:

* `PR_MDWE_REFUSE_EXEC_GAIN` (`prctl(2)`): new mappings cannot be writable and executable, and non-executable mappings
  cannot become executable. It is inherited across `fork` and kept across `execve`.
* systemd `MemoryDenyWriteExecute=yes` (`systemd.exec(5)`): the same rule. The probe shows systemd 255 applies it
  with `PR_SET_MDWE`. On kernels without it, systemd uses a seccomp filter on `mmap` (write and exec together),
  `mprotect`/`pkey_mprotect` (adding exec) and `shmat` (`SHM_EXEC`).
* SELinux `execmod` (SELinux Notebook): making a file mapping executable after it was modified by copy-on-write
  needs `execmod` on the file. Confined domains usually lack it, and each denial is logged as an AVC record.

### 5.2 Probes (black box, unprivileged)

Probe program (scratch, not in the tree): a page-aligned section of two pages. The live page's function returns 410
and the alternative page's returns 420. The alternative page is assembled for the live page's address and holds one
RIP-relative data load and one tail jump out of the page, both biased by +4096. The program is built static non-PIE
and PIE with the system compiler. Modes: **M4** (`/proc/self/exe`, `mmap` RW private copy, write, `mprotect` RX,
`mremap` over the live page); **M6a** `mremap(alt, 4096, 4096, MREMAP_MAYMOVE|MREMAP_FIXED, live)`; **M6b** the same
with `MREMAP_DONTUNMAP`; **M6c** `mmap(live, 4096, PROT_READ|PROT_EXEC, MAP_PRIVATE|MAP_FIXED, fd, alt's offset)`. Each
mode ran under (a) no policy, (b) `systemd-run --user -p MemoryDenyWriteExecute=yes -p NoNewPrivileges=yes`, (c)
`prctl(PR_SET_MDWE, PR_MDWE_REFUSE_EXEC_GAIN)` before the swap, and (b) and (c) together. Two kernels: Linux 6.8.0 (a
build host; Ubuntu 24.04, systemd 255) and Linux 7.0.14 (a second machine; systemd 255 user manager; there the
process also ran under an unrelated seccomp sandbox, `Seccomp: 2`). Results were the same on both kernels, static and
PIE (80 runs in all, none wrong):

| Mode | (a) none | (b) systemd MDWE | (c) prctl MDWE | (b)+(c) | System calls (strace) |
|---|---|---|---|---|---|
| M4 | AVX2 live | step 4 `mprotect` RX: **EACCES**, SSE2 kept | **EACCES**, SSE2 kept | **EACCES**, SSE2 kept | `openat`, `mmap`, `mprotect`, `mremap`, `close` |
| M6a `mremap` | AVX2 live | AVX2 live | AVX2 live | AVX2 live | `mremap` |
| M6b `+DONTUNMAP` | AVX2 live | AVX2 live | AVX2 live | AVX2 live | `mremap` |
| M6c `mmap` FIXED | AVX2 live | AVX2 live | AVX2 live | AVX2 live | `openat`, `mmap`, `close` |

* systemd's user manager accepted the property. In the unit, `PR_GET_MDWE` reads 1 and, on 6.8, `Seccomp: 0`, so it
  is the prctl form. The refusal is silent: the call returns `EACCES` and nothing is logged. Under M4, a hardened
  service would run SSE2 and nothing would show it.
* `/proc/self/maps` after the swap: M6a leaves the live address mapped at the **alternative page's file offset**,
  and an unmapped hole where the alternative page was. M6b and M6c show the alternative offset at both addresses.
  M4 shows the live page's own offset, but its content is private and differs from the file.
* **Debugger (gdb), both mechanisms:** with `break` on a slot set before `run`, the swap discards the inserted
  breakpoint, so it never fires. If the process stops between the swap and the call (any signal or other
  breakpoint), gdb removes the breakpoint by writing the pre-swap byte into the moved page, and the next call faults
  (SIGSEGV observed). M4 replaces the page the same way and has the same failure. **The guard fixes it:** after gdb
  inserted the breakpoint, the live page's pagemap entry read present=1, file=0. The probe then skipped the swap and
  ran the SSE2 page correctly under gdb. Without a debugger, it read present=1, file=1 and swapped, under
  systemd MDWE too. A breakpoint set after the swap fires. gdb disassembles the live AVX2 bytes (it reads memory),
  and the backtrace is correct.
* **perf** (`perf record -e cpu-clock`, unprivileged): samples in the swapped page are named by the live slot's
  symbol (correct name). `perf annotate` disassembles the file's bytes at that symbol (the SSE2 page), not the code
  that ran.

Untested here (needs root, or a kernel without `PR_SET_MDWE`). Each row gives the documented behaviour:

| Policy | M4 | M6 | Source | Command for the owner |
|---|---|---|---|---|
| SELinux, confined domain without `execmod` | `mprotect` RX refused, AVC `execmod` denial per process start | allowed: no write, so no `execmod`. It needs only `execute` on its own file, as any text does | SELinux Notebook, file class | as root on an enforcing host: `install -m755 probe-static /usr/local/libexec/ && chcon -t httpd_exec_t /usr/local/libexec/probe-static && for m in m4 vremap; do systemd-run --wait --pipe -p SELinuxContext=system_u:system_r:httpd_t:s0 /usr/local/libexec/probe-static $m; done; ausearch -m avc -ts recent \| grep -E 'execmod\|execmem'` |
| seccomp-form `MemoryDenyWriteExecute=` (kernel < 6.3) | `mprotect` with exec: EPERM | allowed: `mremap` is not in the documented filter | `systemd.exec(5)` | unprivileged on such a host: the probe's (b) runs |
| PaX/grsecurity MPROTECT | refused (exec gain after write) | no protection change. How PaX treats `mremap` is NOT VERIFIED | PaX documentation | needs a PaX kernel |
| EDR / security review | write then exec on text, flagged by design | no write and no anonymous exec memory. Whether a heuristic flags an `mremap` of an executable mapping is UNKNOWN | none | none |
| seccomp allow-lists without `mremap` | EPERM at some step, SSE2 kept | EPERM, SSE2 kept (correct) | `seccomp(2)` | none |

The probe source and driver are in the story's scratch directory. They are not part of the tree (design story).

### 5.3 Layout and linker placement

* **One input section, two pages.** `.text.liella_classed` (`"ax"`, `.balign 4096`) holds page 0 (the SSE2 bodies)
  and page 1 (the AVX2 bodies). Each page has the same slot offsets: `.org page + SLOT_<fn>` with one set of constants
  for both pages, and `.org page + 4096` at the end of each page. GAS rejects a body that outgrows its slot or page
  (".org backwards"), so both pages are exactly 4096 bytes. The exported labels (`memset`, ...) and the hidden
  `liella_machine_*` names point into page 0. Page 1 carries local symbols (`liella_machine_<fn>_avx2`) at the same
  offsets and with the same sizes, so a tool that symbolises by the moved mapping's file offset (5.6) still finds a
  name.
* **References.** A body in page 1 executes at page 0's address. Jumps within the page need nothing. Every reference
  that leaves the page (RIP-relative data, tail jumps to `liella_hot_fill_rest` and similar) is written
  `XREF(sym)` = `sym + 4096` in page 1. This is fixed at assembly and link time, with no fixup tables and no runtime
  writes. Page 1 must not reference page 0 or itself through `XREF`. The story-1 export sweep on an AVX2 host is the
  proof, and one plant drops a bias. Both bodies are leaf code with no stack adjustment, so one CFI description (CFA
  = rsp+8) holds for both pages and unwinding through either is correct.
* **Static programs.** The section is in one object of `libc.a`. The program's linker (GNU ld, gold, lld, mold)
  places it with the default script's `*(.text .text.*)` and honours the 4096 alignment. An executable's `PT_LOAD`
  has `p_offset` ≡ `p_vaddr` mod the page size, so the file offset is page-aligned too. One input section is never
  split, reordered or dropped while a symbol in it is referenced (start-up uses `memset`), and its contents are
  unique, so ICF cannot fold it. Both pages lie in the text `PT_LOAD`. Cost per program: 4096 bytes for page 1, plus
  0-4095 bytes of alignment padding in the file and the address space. A post-link layout rewriter (BOLT and
  similar) is not supported for this section.
* **`libc.so.6`.** It is the same section in libc's text `PT_LOAD`. The swap moves a page inside libc's own mapping,
  so it needs no path or descriptor. M4 needed libc's path from the loader, and a file replaced on disk by a package
  upgrade between exec and initialisation could have supplied the wrong page. Both kinds of position-independent
  code (PIE, the shared object) work unchanged: the probe's PIE build behaved like the static one.
* **Memory.** The SSE2 class never touches page 1 (page cache at most, shared). The AVX2 class keeps page 1's
  file-backed page in place of page 0, with no private memory. M4 used one private page per process.

### 5.4 Start-up sequence (static step 4; `libc.so.6` initialisation later)

1. After `cpu::init_with`, decide the class: AVX2 if `Features` reports AVX2, XCR0 shows the YMM state saved, and the
   tunables do not say `-AVX2`. Otherwise stop here: **0 system calls**.
2. Guard: `openat("/proc/self/pagemap", O_RDONLY|O_CLOEXEC)`, one `pread64` of the 16 bytes for the two pages
   (offset = page 0 / 4096 × 8), then `close`. If either entry is present (bit 63) and not a file page (bit 61), a
   tracer or uprobe has written into it: keep SSE2. If the open or the read fails, also keep SSE2 (RULING 3). Reading
   one's own pagemap needs no privilege. Since Linux 4.0 the frame numbers read as zero without
   `CAP_SYS_ADMIN`, and the flag bits stay readable.
3. `mremap(page1, 4096, 4096, MREMAP_MAYMOVE|MREMAP_FIXED, page0)`, the M6a form. M6b needs Linux 5.13 or later for
   file mappings (`mremap(2)`) and gains nothing. M6c needs a descriptor and two more system calls, and for
   `libc.so.6` a path.
4. Record the outcome in a test-visible place (the `classed_id` slot, below, already shows it).

Total on the AVX2 class: **4 system calls** (1 without the guard), +2 VMAs (the text mapping splits around page 0;
page 1's range becomes a hole). That compares with M4's 5 system calls, or 8 with the same guard, which M4 also needs.
The process has one thread and no handlers at this point, as in 2.1. The `classed_id` slot: a 6-byte body at a fixed
slot (`mov $0, %eax; ret` in page 0, `mov $1, %eax; ret` in page 1) that the selftest calls to ask which page is live.
It costs nothing in calls to other slots.

### 5.5 Failure handling

Any refusal (`EACCES`, `EPERM`, `EINVAL`, `ENOMEM`) before or by the `mremap` keeps page 0. `mremap(2)` says
`MREMAP_FIXED` unmaps whatever was at the destination, and does not say what is left there when the move then fails.
After an error, the start-up therefore checks that page 0 is still mapped (`mincore(page0, 4096, &v)`; `ENOMEM` means
unmapped). If it is gone, the start-up tries the same `mremap` once more, since page 1 is still at its own address.
If that also fails, the process ends with a message on stderr instead of faulting at its next `memset`. At start-up,
with about 20 mappings and no other threads, the move is not expected to fail. The check makes the rare case
diagnosable instead of silent. A refusal of the guard's `openat` (no `/proc`, or a filter) keeps SSE2.

### 5.6 Debuggers, profilers, core files, per-call cost

* **gdb:** covered by the guard (5.2). Attaching after start-up works normally.
* **perf:** names are right. `perf annotate` on an AVX2 host shows the SSE2 bytes for classed slots (probed). Tools
  that rebuild maps from `/proc/<pid>/maps` (attaching to a running process) see page 0's address at page 1's file
  offset and name the slot with page 1's local symbol. That naming is predicted from the maps line and NOT VERIFIED.
* **uprobes** (`uprobetracer.rst`: probes are placed by file and offset): a uprobe on `memset` (page 0's offset) that
  is registered before start makes the guard keep SSE2. One registered after the swap does not fire, because nothing
  maps that offset any more. It has to target `liella_machine_memset_avx2` (page 1's offset) instead. M4 also loses
  uprobes, because the copy is written. NOT VERIFIED by a probe.
* **Core files:** the default `coredump_filter` does not dump clean file-backed text. A debugger reading a core then
  shows the executable's page 0 bytes at page 0's address, although page 1's code ran there. The shared CFI keeps
  backtraces right. M4's written copy is anonymous and was dumped. Recommendation: the debugging note (story 7).
  NOT VERIFIED by a probe.
* **Huge pages:** if text is mapped with a PMD-sized page (read-only THP for file text), a 4 KiB `mremap` splits it.
  That is functional but loses the large page for that range. NOT VERIFIED.
* **Per-call cost (section 2 table, M6 column = M4's):** 0 on both classes. The exported label is the slot, called
  directly, with no flag test and no indirection. Section 3's predictions stand unchanged. On the SSE2 class and
  Ivy Bridge, no row rises.

### 5.7 Debugging note (story 7, written with CLASSED-SLOT-INFRA)

On a CPU of the AVX2 class, a static program's start-up replaces the page of libc text that holds the classed
entries (today `memset`) with the AVX2 page from the same file. Nothing changes in results, symbols or addresses,
but tools that read the file instead of the process see the other page:

* **gdb** works as usual. A breakpoint on `memset` set before `run` makes the start-up keep the SSE2 page (the
  pagemap guard sees the written page), so the breakpoint fires. Attaching later, or setting it after start-up,
  places it in the live AVX2 page, and gdb disassembles that page.
* **perf**: samples are named correctly (`memset`). `perf annotate` shows the file's SSE2 bytes for that symbol, not
  the AVX2 code that ran.
* **uprobes** are placed by file offset: one on `memset` registered after start-up does not fire. Use
  `liella_machine_memset_avx2` (the AVX2 page's local symbol), or keep the SSE2 page.
* **Core files**: the default `coredump_filter` does not dump clean file-backed text, so a debugger reading a core
  shows the file's SSE2 bytes at `memset`. Backtraces are correct.
* **To run the file's SSE2 page** (the code at every exported label is then the code in the file):
  `GLIBC_TUNABLES=liella.compat.no_classed_swap=1` (COMPAT-20) keeps it and leaves the other AVX2 paths on;
  `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2` keeps it and turns every AVX2 path off. A program without `/proc` also
  keeps it.
