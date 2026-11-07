# LAYOUT-CONTRACTS-PLAN: declare, check and isolate every deliberate code and data layout

**State (2026-11-07):** planning story, verdict COMPLETE. Plan branch `task/layout-contracts-plan` (this file, the
briefs in `handoff/layout-contracts-plan/`, `handoff/reports/LAYOUT-CONTRACTS-PLAN.json`), cut from origin/next
9643c81b0. Prototype branch `task/layout-contracts-plan-proto` (427871c8f, from the same base: `tools/layout/`):
**evidence only, do not land it**. The production check is brief LAYOUT-STARTUP-WINDOWS.
Role mei:~/w/s1 (Skylake-SP). Products and measurements kept in mei:~/work/LAYOUT-CONTRACTS-PLAN/{a-next,b-wire}/.
Decisions in force that this plan touches: D-X.20, D-X.22, D-X.32, D-X.34, D-4.33, D-4.53, D-4.56, D-4.63, D-4.67.

## 0. Findings first

1. **Cause of the LIBM-F64-LOG-FAMILY-WIRE-2 startup fault, measured.** `liella_boundary::runtime::start::<false>`
   (5.1 KiB, the first Rust function start-up runs) is **not in the hot group on origin/next**: the hot-text list
   still holds the key of the non-generic `runtime::start` (line 4 of `crates/libc-so/hot-text.list`), which matches
   no section since the function became generic, so `hottext::group` silently grouped nothing for it. It sits in
   cold `.text` about 2.9 MB after the group, where any growth of the code before it moves it. On next it lies in
   one 64 KiB window (0x6f9680-0x6faaf6); on the WIRE-2 head the new libm code moves it to 0x6ef200-0x6f0676,
   across the window boundary 0x6f0000: one more text window, one more fault. `runtime::apply_irelative` is
   another start-up function outside the group (not in the list at all).
2. Measurements (Skylake-SP role, test-profile products of each head built in the same role path, the probe of
   `hot_text_products.rs` linked as that test links it, posix_spawn launcher, `ADDR_NO_RANDOMIZE`, one warm-up then
   5 runs, every run identical): minor faults startup / setlocale+printf / tool **21 / 59 / 61 (next)** against
   **22 / 62 / 64 (WIRE-2)**. `perf record -e page-faults -d` address diff (startup mode): the extra fault is the
   second window of `runtime::start::<false>`; .data/.lbss faults move by one window number but keep their count.
   CI read 24 against bound 23 on another role path in a composed train; the +1 here is consistent with it (CI's
   own next baseline was not re-read). The +3 in the
   setlocale and tool modes was NOT analysed (outside this story).
3. **Prototype** (`tools/layout/startup_windows.py` on the proto branch, job `lcp-proto-check-2`): holds on next
   (9 windows, budget 9: 7 windows of the hot group plus 0x6f and 0x71 outside it, 110 of 110 start-up functions
   found) and fires on WIRE-2 (10 > 9), printing
   `runtime5startKb0_ (0x6ef200-0x6f0676, outside the hot group) straddles the 64 KiB boundary 0x6f0000`.
   Products and counters only, no timing; the check itself takes about a second after the products exist.
4. Three `hot_text_products.rs` bound raises are recorded as "layout-only delta, owner rule" (TRUE 20 -> 23,
   LOCALE 65 -> 68, TOOL 68 -> 71; TRUE 18 -> 20 before them for the run path), and two more bounds carry a
   "plus 2 for layout-only deltas" margin. The diagnosed ones were a symbol that moved across a window or page
   (env names in .rodata, NSCACHE in .data; WIRE-2's is in .text), found by symptom, then by hand with perf. A layout check names the
   symbol at build time instead.

## 1. Inventory of layout assumptions

Legend for "checked": B = at build (`cargo xtask static` or `so`), G = gate test on products, T = timing only,
C = compile-time assert, - = not checked.

| # | Assumption | Where | Protects | Checked today | How a change elsewhere breaks it |
|---|---|---|---|---|---|
| L1 | Hot text grouped by list (D-X.20), first-touch order (`.text.sorted.h<n>.<f>`, HOT-ORDER) | `xtask/src/hottext.rs`, `crates/libc-so/hot-text.list`, `tools/hottext/` | text faults of short programs (one fault per 64 KiB window) | G: fault bounds (`init_short_programs_...`, `init_coreutils_shaped_...`); G: `init_cold_startup_functions_are_grouped` (hand list of ~25 names) | a renamed / newly generic / newly split function drops out of the list **silently** (finding 1); an unlisted start-up function in cold text moves with any growth |
| L2 | Fork child path in the hot group (D-X.32) | `tools/hottext/probes/p_fork.c` | fork child refaults | G: `init_fork_child_stays_within_its_page_fault_bound` | as L1 |
| L3 | GNU ld / gold default scripts place `.text.hot.*`, `SORT(.text.sorted.*)`, `.rodata1`, `.lbss` as assumed (binutils >= 2.36 for `.text.sorted`) | the user's linker | L1, L5, L6 | - (assumed; lld's default handling NOT VERIFIED) | a program linked by another linker loses the grouping (no wrong behaviour, only faults) |
| L4 | Path-independent products (PRODUCT-PATH-REMAP) | `xtask/src/align.rs` `remap_flags`, `remap::check` | layout identical across roles/hosts (role path was in .rodata) | B: `remap::check` in `so` | a new build-path string |
| L5 | Start-up environment names in one 512-byte object, `.rodata1`, inside one 64 KiB window | `crates/boundary/src/startup_names.rs`, `hottext.rs` `STARTUP_SECTION` | the TIMER-SIGEV +1 (names straddled 0x7a0000) | C: size/alignment assert; G: `init_startup_names_sit_in_one_window` | a new start-up rodata constant outside the object |
| L6 | Large zero-initialised tables in `.lbss` after `.bss` (D-X.22) | `xtask/src/coldbss.rs` | start-up writes pack into few RW pages | G: `abi_libc_a_cold_bss_tables_are_lbss` | a small hot word added in the middle of `.data`/`.bss` (UTMP-Y2038: NSCACHE moved to its own page, +1) - **not checked** |
| L7 | Every exported function entry 64-byte aligned (D-4.33) | `PRODUCT_RUSTFLAGS`, `.p2align 6` before every exported `.S` label | entry not straddling a line (MEM-K5c: memset_16 13% slower) | B: `align::check` on libc.so.6 dynsym | internal (`liella_machine_*`) entries are not checked |
| L8 | Classed pages: two 4096-byte pages, same slot layout, page 1's out-of-page refs = page 0's + 4096 (D-4.53, D-X.34) | `csys/src/machine/fold.S` `LIELLA_SLOT_*`, `.org` | the CPU-class swap at start-up | B: `.org` fails the assembly on overflow; `classed::check_file` in `static` and `so` | already build-checked |
| L9 | No jump crosses or ends on a 32-byte boundary in fold.S / scan.S (D-4.67, JCC erratum) | `crates/boundary/jcc_pad.rs`, `xtask/src/jcc.rs` | Skylake-family uop-cache delivery | B: `jcc-check` in `static` and `so` | Rust-generated hot code is not padded or checked (open question Q3) |
| L10 | memchr/rawmemchr SSE2 path pinned at its section offset (`.org 0x231`, `0x1ed`; D-4.56) | `scan.S` subsection 1 | Ivy Bridge memchr_16_mix 14% slower when it moved | B (assembler: `.org` backwards fails); the offset's class mod 32/64 holds only while the section stays >= 64-aligned: - | a section alignment change; growth of the head past the `.org` is caught by the assembler |
| L11 | rawmemchr SSE2 128-byte loop 32-byte aligned (D-4.63); other `.p2align 4/5` before hot loops (scan.S 91 alignment directives, fold.S 9 plus 6 `.org`) | `scan.S`, `fold.S` | Ivy Bridge 1.3% (rawmemchr_4k) and loop-entry timing | T only (assembler keeps the directive; survives the link while the section alignment >= the directive) | dropping a directive in an edit; section alignment lowered |
| L12 | gettext resolution slots in one 4096-byte page (`#[repr(C, align(4096))] Slots`) | `crates/legacy/src/intl.rs` | one fault per first lookup, not two (GETTEXT-HIT-3) | C: size assert; alignment by attribute | none from outside (attribute travels) |
| L13 | Heap segment registry on its own 128-byte pair of lines; TLS / thread blocks 64-aligned | `crates/rt/src/heap.rs`, `tls.rs`, `thread.rs` | false sharing | attribute only | none from outside |
| L14 | Startup data touched once per program: AUXV words, SYSINFO_EHDR, cpu dispatch words, NSCACHE, EXIT_HANDLERS, HEAP (from the perf trace) | .data / .bss / .lbss | RW page count at start-up | G: fault bounds only | **object order in .data is the compiler's**; UTMP-Y2038 and this run show the pages move with unrelated objects |
| L15 | Bench operands at fixed page offsets (one page-aligned block, 64 bytes in) | `bench/c/cbench_hot.c` | strcasecmp and other rows measure the kernel, not where the operands sat (BENCH-CASECMP-ALIGN) | - (comment) | a new buffer in the block; the struct reordered |
| L16 | Kernels whose cost depends on operand page offsets (head page tests, page-end back-up) | `scan.S` casecmp, strcmp, strlen families | instructions per call independent of placement | G: neighbour-page safety tests (correctness), counters at chosen offsets; no sweep of cost over offsets | a kernel edit reintroduces an offset-dependent slow path (BENCH-CASECMP-ALIGN-2: 554-1949 instructions at page offsets > 4064) |
| L17 | Fault-bound tests on products (`hot_text_products.rs` 11 tests, `heap_meta_products.rs`) | crates/core/tests | the outcome of L1-L6, L14 | G (25+ minutes into a CI train when it fires) | everything above; each bound absorbs drift until it is raised |

There are no linker scripts in the tree: the static products rely only on section names and alignment, which
the user's linker interprets (L3); `libc.so.6` is linked by xtask with version scripts, so its link order is
under our control.

## 2. Design

### 2a. One manifest of layout contracts

* **A1 (recommended): `layout/contracts.tsv`**, one row per contract: `id`, `kind` (the check function), `subjects`
  (symbols, sections or files), `rule` (kind-specific parameters, e.g. `windows<=9`), `reason`, `decision` (D-row).
  TSV like `spec/clauses.tsv` and `symbols.tsv`, so `grep -P '^STARTUP-TEXT-WINDOWS\t'` works. Data the check
  needs and that is generated (the traced start-up keys) stays in its own generated file named by the row.
* **A2: contracts as Rust code** in `xtask/src/layout.rs` (a `const CONTRACTS: &[Contract]`), with a generated
  `docs/layout-contracts.md`. Type-checked, but a budget change is a code change, and the doc is a second copy.
* **A3: site tags only** (`// LAYOUT(<id>)` / `/* LAYOUT(<id>) */` next to the attribute or `.p2align`), collected by
  `static` like the UNSAFE audit. Finds the sites, but no place for budgets or products-level rules.
* Recommendation: **A1 plus A3's tags as a cross-check**: every tag names a row; every row of a kind with a source
  site (alignment, `.org`, object packing) has at least one tag; `static` fails either way. Deleting a `.p2align`
  that carries a tag then fails `static`, which no product check could see (L11).

### 2b. A check when the products are built

* **B1 (recommended): `cargo xtask layout-check [<products dir>]`**, run at the end of `cargo xtask so` (both
  profiles, as `classed` and `jcc` are today) and by hand on any products. Reads the built `libc.a`, `libc.so.6`,
  `ld.so` with `readelf`/`nm`/`objdump`, and for program-level contracts links the canonical probe
  (`page_faults.c`, plus `p_fork.c` for L2) statically, as `hot_text_products.rs` does, with `-Wl,-Map`. Every
  failure prints `layout: <ID> BROKEN: <symbol> <old place?> -> <new place>: <rule>` and the decision row.
  Deterministic, no perf, no timing, about a second per profile after the build. Existing checks (`align`,
  `classed`, `jcc`, `coldbss` test, startup-names test) become kinds of the manifest without changing what they test.
* **B2: golden layout snapshot** (commit the link map or symbol address list, diff it). Catches everything, but every
  code change moves addresses, so the snapshot is regenerated on every commit and reviewers learn to ignore it.
  Rejected.
* **B3: counter check at build** (perf page-fault addresses of the probe). Precise for faults, but needs perf and a
  quiet host, and is host-dependent (Skylake-SP vs Zen 1 read differently). Keep it where it is: the gate tests of
  L17 are the backstop and the source of budgets; regen uses perf / gdb to *produce* the start-up sets.
* Plants (each kind gets at least one; the brief lists them): budget minus one; a 64 KiB filler `.text` object linked
  ahead of a listed function; a hot key removed from the list; a `.p2align` deleted (tag check); the classed bias
  broken (existing plant); a JCC padding flag removed (existing plant).

### 2c. Structural isolation

* **C1 (recommended now): complete, guarded grouping.** Keep D-X.20's name-based grouping (works in the user's
  static link, keeps `--gc-sections`), and close its hole: the list gets a tier per key (startup / fork / stdio /
  locale / tool) and the profile(s) it was traced in; `hottext::group` FAILS when a startup- or fork-tier key of the
  profile being built matches no function section, naming the key ("renamed or now generic: run regen"). Then
  require *every* traced start-up function to be in the group (rule STARTUP-TEXT-GROUPED; fails on origin/next
  today for `runtime::start::<false>` and `apply_irelative`, so it is enforced after the list is regenerated). Cost:
  none in size. A new hot function is listed by `tools/hottext/regen.sh`; a new start-up function that is not traced
  shows up as a STARTUP-TEXT-WINDOWS failure naming it.
* **C2 (measure, then decide): a 64 KiB-aligned anchor at the start of the group and between tiers.** An empty input
  section with `sh_addralign = 65536` whose name sorts first in `SORT(.text.sorted.*)` (e.g. `.text.sorted.h0`,
  shorter than `.text.sorted.h00000.*`) fixes the group start to a window boundary, so drift before the group (seen
  here: the group starts at 0x40e440 on next, 0x40e0c0 on WIRE-2) no longer re-cuts the windows inside it; a second
  anchor before the first non-startup key keeps the start-up tier in its own windows whatever the later tiers do.
  Cost: up to 64 KiB of file padding per anchor in every static program and in `libc.so.6` (never touched, so no
  faults in principle; NOT MEASURED), and the padding window may hold crt/`.init` code. Code after the group cannot
  shift it in either design: cold `.text` is placed after the sorted sections by the default scripts.
* **C3: `libc.so.6` only, link-order control.** We link `libc.so.6` ourselves: a linker script fragment with
  `. = ALIGN(0x10000)` around the hot input sections (works with GNU ld 2.42), or `--section-ordering-file`
  (GNU ld 2.43+, not on the reference toolchain; gold has one but gold is deprecated). Static programs do not get it.
* **C4: `ld -r` pre-link of the hot objects into one section.** Rejected in D-X.20 (defeats `--gc-sections`), still
  rejected.
* Recommendation: **C1 now, C2 behind measurement** (instructions, faults, file size; keep only if no measured row is
  slower), C3 only if `libc.so.6` start-up shows window drift that C2 does not remove.

### 2d. The rule

A contract is changed on purpose only with a decision row, like a deliberate difference: the manifest row names its
D-row; `cargo xtask static` fails when a row names a D-row that does not exist, when a tagged site has no row, and
(using `.changed`, as `affected` does) when `layout/contracts.tsv` changes a rule or budget without a change under
`plan/07-decisions/`. Tightening a budget is a local decision (one-line row). Loosening one, removing a contract or
raising a `hot_text_products.rs` fault bound for a "layout-only delta" needs the layout check's output naming the
moved symbol in the row (no more raise-by-symptom), and a maintainer ruling when it is a loosening.

### What each option would have caught

| Failure | A1+B1 (manifest + build check) | C1 (guarded grouping) | C2 (anchors) | Today |
|---|---|---|---|---|
| LIBM-F64-LOG-FAMILY-WIRE-2, startup 24 > 23 | **build time** in that story's own `cargo xtask so`, naming `runtime::start::<false>` (prototype shows it) | **prevented**: the stale `runtime::start` key would have failed `so` in the story that made it generic; listed, it sits in the group where cold growth cannot reach it | not by itself (the function was not listed) | gate, 25 min into a CI train, symptom only |
| strcasecmp cost by operand placement (BENCH-CASECMP-ALIGN-2) | only with the L16 kind (placement sweep, counters, gate time) and L15 (bench operand offsets, bench build time) | no | no | timing, then a by-hand offset sweep |
| JCC erratum (MACHINE-JCC-PAD) | already build time for fold.S/scan.S (D-4.67); the manifest registers it; Rust hot code stays unchecked (Q3) | no | no (32-byte rule is inside functions) | build time since D-4.67 |
| Classed pages' 4096 bias (D-4.53) | already build time (D-X.34); registered | no | no | build time |
| TIMER-SIGEV +1 (.rodata names straddle) | build time with STARTUP-DATA-WINDOWS (brief 5) | no (data) | no | gate; fixed by L5 |
| UTMP-Y2038 +1 (NSCACHE alone on a page) | build time with STARTUP-DATA-WINDOWS (pages of RW start-up objects) | no | no | gate |

## 3. Prototype (step 3)

`tools/layout/startup_windows.py <products dir>` (proto branch): links `page_faults.c` against the products like
`hot_text_products.rs`, reads `nm -S` (assembly entries without `.size` count their entry byte) and the link map
(hot group = the `.text.sorted.h*` / `.text.hot.*` input sections), maps the keys in `tools/layout/startup-text.keys`
(110 build-stable keys from a `trace_gdb.py` trace of `page_faults true` on next's products, first-touch order) to
address ranges, counts distinct 64 KiB windows, and fails when the count exceeds the budget (9) or a start-up function
outside the group straddles a window boundary. Jobs on the role host: `lcp-wire2-so`, `lcp-next-so` (builds +
copies + fault measurement), `lcp-wire2-measure`, `lcp-trace` (start-up trace), `lcp-proto-check-2` (the check,
rc 0 = holds on next AND fires on WIRE-2). `lcp-proto-check` is superseded (its first version skipped size-less
assembly entries: 107 of 110 keys found; same verdicts).

## 4. Proposed decision rows (numbers for the coordinator)

* **D-X.NEW-1 Layout contracts manifest and build check.** Every deliberate code or data layout is a row of
  `layout/contracts.tsv` (id, kind, subjects, rule, reason, decision); `cargo xtask so` runs `layout-check` on the
  built products and fails naming the contract and the symbol; `static` checks rows against D-rows and `LAYOUT(<id>)`
  site tags. Tooling only, no ABI or behaviour change. (Process rule d needs a maintainer ruling: see RULING 1.)
* **D-X.NEW-2 STARTUP-TEXT-WINDOWS** (extends D-X.20): the functions `page_faults true` executes occupy at most N
  64 KiB windows of the probe linked from the products (N = 9 on origin/next 9643c81b0, test profile; release profile
  to be measured), and none outside the hot group straddles a window boundary. Prototype evidence as in section 3.
* **D-X.NEW-3 Hot-text keys are guarded** (extends D-X.20): list keys carry tier and profile; a startup/fork-tier key
  that matches no function section of its profile fails `so`; rule STARTUP-TEXT-GROUPED (every traced start-up
  function in the group) enforced once the list is regenerated.
* **D-X.NEW-4 Hot group anchored at a 64 KiB boundary** (extends D-X.20): only if HOTTEXT-ISOLATE-64K measures no
  row slower; records file-size cost and fault counts.
* **D-X.NEW-5 STARTUP-DATA-WINDOWS** (extends D-X.22): start-up read-only objects in at most N 64 KiB windows and
  start-up written objects in at most M 4 KiB pages.

## 5. Briefs (one file each in `handoff/layout-contracts-plan/`)

Order: 3 first (fixes a live defect, small), then 1 and 2 (they can run in parallel; 2 needs 1's manifest only at
its end), then 5, 4 and 6.

1. `LAYOUT-MANIFEST.md`: the manifest, `xtask layout-check`, existing checks as kinds, tags, rule d in `static`.
2. `LAYOUT-STARTUP-WINDOWS.md`: the prototype as an xtask kind, keys generated by regen, both profiles, plants.
3. `HOTTEXT-STARTUP-COMPLETE.md`: regenerate the list, the stale-key guard with tiers and profiles, measure.
4. `HOTTEXT-ISOLATE-64K.md`: anchors, measured size and faults, keep or reject.
5. `LAYOUT-STARTUP-DATA.md`: data and read-only start-up objects (pages and windows).
6. `LAYOUT-PLACEMENT-SWEEP.md`: operand placement (bench offsets at build, a counter sweep over page offsets).

## 6. Open questions

* Q1 (RULING 1): rule d as a `static` failure (manifest change without a decision change) vs review-only.
  Recommend the static failure, with tightening allowed by a one-line row.
* Q2: should `hot_text_products.rs` bounds be derived from the manifest budgets (one number, two checks) once the
  data contract exists? Recommend yes, keeping the measured noise margin in the test.
* Q3: Rust hot code is not JCC-padded (`-Cllvm-args=-x86-branches-within-32B-boundaries` exists for rustc); a
  separate measured story, not part of this plan's briefs.
* Q4: the +3 faults of the setlocale and tool modes on WIRE-2 are not analysed; the LIBM continuation should run
  the prototype's per-section fault diff (`faults.py` method, section 3) before raising any bound.
* Q5: lld's handling of `.text.sorted.*` / `.text.hot.*` and `.lbss` is NOT VERIFIED (L3).

## 7. Not to redo

The cause of WIRE-2's startup fault (finding 1) and the next vs WIRE-2 counts are measured; the products and the
perf traces are kept in the role host's `~/work/LAYOUT-CONTRACTS-PLAN/`. Scripts used: scratch
`lcp_build.sh` (build, copy, link, perf), `spawn.py` (launcher matching the test), `faults.py` (fault address ->
section, symbol, window), `lcp_trace.sh`, `lcp_check.sh` (in the role host's `~/work/LAYOUT-CONTRACTS-PLAN/`).
