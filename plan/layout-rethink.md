# Layout rethink: a start-up layout whose inputs cannot vanish under the compiler

LAYOUT-RETHINK (2026-11-13), planning story, no product change. Proposal; the owner rules before any
implementation story is briefed. Pending record: `records/pending/LAYOUT-RETHINK-1.toml` (series D-X).
Reads with: D-X.20, D-X.47, D-X.53, D-X.56, D-X.63, D-X.77, D-X.78, `handoff/layout-stability-rethink.md`,
`handoff/HOTLIST-TRAIN-REGEN-2.md` (on its branch). Measurements: section 3; scripts and logs in the story's
scratch on the Haswell-EP build host (`~/work/LAYOUT-RETHINK/`, results in `out-summary/` there).

## 1. Frame

### 1.1 What the layout must achieve

The text a short program runs at start-up and in its first library calls sits in as few 64 KiB fault-around
windows as possible (a read fault on a file mapping maps the 16 pages of its 64 KiB-aligned virtual window,
`handoff/layout-stability-rethink.md` section 2), the start-up read-only data in one window, and the written
start-up state in few 4 KiB pages. It is carried by the **archive**: users link `libc.a` with their own linker,
so the only lever is the names and alignment of the archive's input sections, which GNU ld's default script
orders (`.text.sorted.*` sorted by name, after the start-up/exit/unlikely groups; D-X.20, D-X.63). `libc.so.6`
is linked from the same archive and inherits the grouping.

Programs and contracts that must stay green (both profiles, fast and release; counts deterministic under
`ADDR_NO_RANDOMIZE`, the relay harness of `crates/core/tests/hot_text_products.rs`):

| contract | today |
|---|---|
| `page_faults true` (start-up) | bound 17 (count + at most 1, D-X.53) |
| `page_faults locale` (setlocale + printf) | bound 51 |
| `page_faults tool` (coreutils-shaped) | bound 56 |
| fork child / localtime / localtime + mktime / float | bounds 6 / 20 / 20 / 5 |
| zero-fault windows: mktime after localtime_r, string calls after printf, second bindtextdomain | 0 at every text phase (pad tests, D-X.63) |
| STARTUP-TEXT-WINDOWS / STARTUP-DATA-WINDOWS (`layout/contracts.tsv`) | text budget 4 per profile; data windows |
| TEXT-PREFIX, CLUSTER-WINDOW (D-X.63) | no library text ahead of the group; each 0-fault cluster in one window |

Bounds only tighten (owner ruling #4079). Nothing in this proposal changes a bound.

### 1.2 What a key has to identify

The grouping needs, for **the archive being built**, the set of function sections the probes execute, the
order they are first touched in, and their tier (start-up first) and cluster. That is a property of this
build's code. A name is needed only to carry a trace taken on build N into build N + 1; under fat LTO the
carry breaks whenever the inliner, monomorphisation or a refactor changes which functions exist out of line.
So the question is not "which names are stable" but "why carry anything across builds at all".

A key going stale has three distinct causes, and the current check (D-X.53: every dead key fails `so`)
treats them inversely to their cost:

1. **Inlined away.** The function's code now sits inside a caller. That caller ran too, so it is traced and
   grouped: the bytes are already in the hot group. Expected cost: about 0 faults. Today: `so` fails.
2. **Renamed** (became generic, moved module, changed signature): the code is out of line under a new name,
   which the list does not hold, so it lands in cold text. Can cost a window. Today: `so` fails (correct
   signal, but only a regeneration fixes it).
3. **Newly outlined** (the inliner stops inlining a function, or a new helper appears): a section the list
   never held. Can cost a window. Today: **nothing fires**; no key is dead. Only a fault bound or window budget
   notices, and only when the shift crosses a window boundary.

The dead-key check therefore fails on the harmless case and is blind to one of the two harmful ones; the outcome
contracts are what really protect the counts. Section 3.3 measures this with a deliberate compiler change.

## 2. Options

Each: mechanism; what it depends on; failure mode; cost. Licences and availability on our build hosts
(checked on the Haswell-EP build host, Ubuntu 24.04, GNU ld 2.42: gdb, perf, ld.gold present; no llvm-bolt, no system
lld, no valgrind, no llvm-profdata; `rust-lld` ships in the rustup toolchain).

**(a) Keys only on things that cannot disappear.**
- *a1, exported roots + static closure* (prototyped as P2). Keys are the exported C functions the traces hit
  (ABI names: they exist as long as the symbol is exported). At `so`, the hot set is every function section
  reachable from them through direct-call / PC-relative relocations in the archive, skipping `#[cold]`
  (`.text.unlikely.*`) sections, in depth-first order from the roots in first-touch order. Depends on: the ABI
  names and the archive's relocations only. Failure mode: precision. The closure includes every directly
  reachable function (error paths not marked cold, options a probe never takes) and misses everything reached
  indirectly (function pointers, vtables, IFUNC resolvers' targets, assembly entry labels). Cost: section 3.2.
- *a2, structural guarantees for internal keys*: `#[inline(never)]` on every keyed function (today's D-X.56 /
  D-X.77 pins). A fixed boundary costs instructions (3 to 45 per start-up row, D-X.56; iconv +34 per call,
  D-X.77), so 367 of 412 Rust keys are in the `hot-text.unpinned` ratchet instead. Does not cover class 3.
- *a3, assembly labels*: functions written in `.S` files keep their names (no inlining across them). Stable
  keys for the assembly families, including the IFUNC variants. Small part of the set (most keys are Rust).

**(b) Compiler-owned placement.**
- *PGO* (`-Cprofile-generate`, training run, `-Cprofile-use`; LLVM names sections `.text.hot.` /
  `.text.unlikely.` from profile counts). Depends on: the profiling runtime linked into an instrumented libc
  (it calls libc functions itself), `llvm-profdata` of rustc's LLVM (rustup `llvm-tools`, not installed), a
  committed profile keyed by function name plus a control-flow hash. Failure mode: the same vanishing, but
  silent: a function whose name or CFG changed loses its profile with a warning. The buckets are count-based:
  start-up code runs once, so it is lukewarm and lands in plain `.text.*`, scattered; there is no first-touch
  order, tier or cluster, so TEXT-PREFIX / CLUSTER-WINDOW cannot be expressed. It also changes code generation
  everywhere (inlining, block layout): a speed question of its own. Cost: an instrumented build and a training
  run per profile per refresh. Licence: LLVM, Apache-2.0 WITH LLVM-exception. Not prototyped (cost, and it
  cannot express the contracts).
- *Attributes*: `#[cold]` is already honoured (`.text.unlikely.`); stable Rust has no hot attribute;
  `#[link_section]` was rejected in D-X.20 (cannot reach `core` or generic instantiations).

**(c) Reorder the linked output from traces of the built binary (addresses, not names).**
- *libc.so.6 / ld.so*: we do the final link, so a post-link tool or a second link can order by observed
  addresses. BOLT (`llvm-bolt`, Apache-2.0 WITH LLVM-exception; not on our hosts; needs an `--emit-relocs`
  link; rewrites the whole object, a large trust step for a libc) or our own two-pass link (trace the first
  link, map addresses to input sections with the link map, relink with an order: gold
  `--section-ordering-file` (present, deprecated upstream), `rust-lld --symbol-ordering-file`, GNU ld's
  `--section-ordering-file` from 2.43 (hosts have 2.42)). Not measured here: the contracts are static-link
  contracts.
- *libc.a*: impossible as such, because the final link is the user's. But the same idea works one step
  earlier, and that is option (e).

**(d) Baseline: the current design with the train regeneration.** Generated `hot-text.list` (564 lines),
`startup-text.keys`, `probe-text.keys`, `startup-data.keys`; hand-kept `hot-text.clusters`, `hot-text.absent`,
`hot-text.unpinned`; gdb traces of probe programs (`tools/hottext/regen.sh`, about 5 min for both profiles,
D-X.78); a dead key of any tier fails `so` (D-X.53); `--hottext-regen`, `hottext-repair`, `hottext-recheck`;
45 pins; the one-at-a-time lane; HOTLIST-TRAIN-REGEN-2 (not landed) makes the four generated files
train-owned (`cargo xtask hot-lists`, 284 s). About 6,000 lines of tooling (`tools/hottext/*`, `xtask` hottext,
startup_ro, coldbss, layout). Failure mode: classes 1 and 2 stop builds of changes that did not cause them;
class 3 is unguarded.

**(e) New: the build traces itself ("trace at build").** `cargo xtask so` already has the un-grouped archive
in hand (`hot-text.pre-group.a`, D-X.78). It links each probe program against it with a link map, runs each
under a tracer in a fixed environment, maps every executed address to its input section through the map, and
groups exactly those sections, in first-touch order, tier by probe. This is (c) applied to the archive: the
"built binary" is the probe linked against this archive, and the result is written back as section names.
The committed inputs are the probe programs (C, calling only ABI functions) and their tier; clusters become
probe phases delimited by ABI calls (for example: the time cluster is what `page_faults localtime_mktime`
first touches from its first `localtime_r` call on). No function name crosses a build, so nothing can go
dead, and class 3 is grouped automatically. Tracer variants: gdb one-shot breakpoints (today's
`trace_gdb.py`); a small ptrace tracer (one-shot `int3` at every function start of the link map, no gdb); or an
"exploded" link (every function section 64 KiB aligned) under `perf record -e page-faults -d`, whose fault
addresses name one section each, in first-touch order. Depends on: running the just-built products on the
build host (native builds only, which Liella already is), ptrace or perf permission, and a hermetic probe
environment. Failure modes: no tracer (sandboxed builder): fall back to an ungrouped product with a loud
warning, or fail; **host dependence**: the executed set can follow the build host's CPU (IFUNC / CPU-class
selection) and its locale data, so two hosts could produce different section names unless the trace is
canonicalised (section 4, LT-4). Cost: section 3.

**(f) Patch on (d): dead keys judged by DWARF.** A dead key whose function appears as an inlined subroutine of
a grouped function (release builds keep inline records in their line tables) is class 1 and accepted
automatically; only a key found nowhere fails. Removes most false stops without pins; still blind to class 3;
still names; depends on debug info in both profiles (release has `line-tables-only`; fast to verify). Not
prototyped.

## 3. Measurements

Haswell-EP build host (26 threads, Linux 6.8.0-147, GNU ld 2.42); a scratch copy of the branch base
b31418181 (next when the branch was cut). `page_faults.c` linked exactly as `hot_text_products.rs` links it,
each mode through `relay <mode>` from a fixed-length path, `ADDR_NO_RANDOMIZE`, argv0 `page_faults`, cwd `/`,
`LANG` only; one warm-up and five runs: **every run identical in every variant**. `puts` and `cat` are
`tools/hottext/probes/p_puts.c` / `p_cat.c` (64 KiB of input), `perf stat -e minor-faults` from exec. In
brackets: text-segment faults of one `perf record -e page-faults -d` run. Every variant is measured the same
way, so the deltas compare; 3.4 gives the contract tests' own run on the P1 products.

### 3.1 P1 = option (e): the list traced from the build being grouped, nothing inherited

`regen.sh . --replace --build` per profile.

| probe | next fast | P1 fast | next release | P1 release |
|---|---|---|---|---|
| start-up (`true`) | 11 [2] | 11 [2] | 10 [2] | 10 [2] |
| setlocale + printf (`locale`) | 49 [8] | 49 [8] | 51 [9] | 50 [8] |
| `tool` | 52 [10] | **53 [11]** | 54 [11] | 53 [10] |
| fork child | 5 | 5 | 5 | 5 |
| localtime / localtime + mktime | 19 / 19 | 17 / 17 | 18 / 18 | 18 / 18 |
| float | 4 | 3 | 3 | 3 |
| 0-fault: mktime, strings, rebind | 0, 0, 0 | 0, 0, 0 | 0, 0, 0 | 0, 0, 0 |
| puts | 18 [3] | 17 [2] | 17 [2] | 17 [2] |
| cat | 24 [5] | 24 [5] | 22 [5] | 22 [5] |
| hot group in the probe link | 546 sections, 520,588 B | 480, 507,266 B | 458, 562,564 B | 427, 545,589 B |

21 of 22 rows equal or lower. Fast `tool` is one text window higher. Not diagnosed: the group is 13 KiB smaller
and ordered by the current first touch only, so the tool path's window phase moved; LT-1 must bring it back
before the default changes. The committed list holds 561 keys; the fresh traces 477 (fast) and 427 (release).
Every committed key a fresh trace lacks (86 fast, 134 release) is one no current trace of that profile hits
(45 carry `-`, history kept by merge mode): bytes in the group that no probe runs. The committed list also
lacks 2 functions the fast probes execute today (extra tier): class 3 is already present on next.

### 3.2 What a stale list costs when the compiler changes its mind (fast profile)

The same tree built with LLVM's inline threshold moved (`RUSTFLAGS=-Cllvm-args=-inline-threshold=N`; `so`
appends it to its own flags), once with next's committed list (stale; `--hottext-regen` so the dead-key check
reports instead of failing) and once with a list traced from that build (fresh).

| | next | 300 stale | 300 fresh | 200 stale | 200 fresh |
|---|---|---|---|---|---|
| dead keys of the committed list | 0 | 9 (plain `so` fails) | - | 2 (plain `so` fails) | - |
| executed, missing from the list | 2 | 7 | 0 | 26 (9 start-up) | 0 |
| start-up | 11 [2] | 12 [4] | 11 [2] | 12 [6] | 11 [2] |
| setlocale + printf | 49 [8] | 52 [11] | 49 [8] | **58 [17]** | 50 [9] |
| tool | 52 [10] | 55 [13] | 52 [10] | **59 [17]** | 52 [10] |
| fork / localtime / + mktime | 5 / 19 / 19 | 6 / 18 / 18 | 5 / 17 / 18 | 6 / 22 / 22 | 6 / 17 / 17 |
| float; mktime after localtime_r | 4; 0 | 3; 0 | 3; **1** | 4; 0 | 3; 0 |
| puts / cat | 18 / 24 | 19 / 25 | 18 / 23 | 21 / 27 | 17 / 25 |

Readings. (1) The loss follows the functions that newly exist out of line (class 3: 7 of them, +3 on locale;
26, +8 on locale, +7 on tool, +5 on localtime), not the dead keys: the check fired hardest (9 keys) where the loss
was smaller and barely (2 keys) where it was largest. (2) A trace of the build being grouped restores next's
counts in both directions (within 1: fork and locale at 200, puts / cat). (3) At 300 the fresh list traced
cleanly (463 keys, none dead), yet its rebuild failed: the hand-kept `hot-text.clusters` names
`intl::translate::{closure}` as the rebind cluster's end, and that closure was inlined. The fallback build
(`--hottext-regen`) leaves that cluster unaligned and reads 1 for mktime after localtime_r (not diagnosed
further). Any committed file that names an internal function breaks this way, the cluster file included.

### 3.3 P2 = option (a1): closure of the exported roots

`closure.py` on each profile's pre-group archive (1 s, no trace); roots: the 123 exported functions of the
fresh trace, in its order.

| | fast | release |
|---|---|---|
| traced sections (bytes) | 480 (507,266) | 427 (545,589) |
| closure sections (bytes) | 1,012 (1,056,035) | 875 (982,704) |
| traced, not reachable by direct reference | 61 (47,346 B: regex program, heap refill, locale view, ...) | 45 (44,107 B) |
| start-up tier, closure against traced | 264 sections, 243,459 B against 91, 60,182 B | - |
| `cargo xtask so --hottext-regen` | fails: start-up read-only data 65,659 B > its 64 KiB window | fails: STARTUP-TEXT-WINDOWS |

The closure is 2.1x (fast) and 1.8x (release) the traced bytes, 4x in the start-up tier, and still misses what
is reached through function pointers and trait objects. It does not build under the start-up contracts, so no
fault counts were taken. Rejected.

### 3.4 Cost and determinism

| option | added build time (Haswell-EP host) | determinism |
|---|---|---|
| (d) next | none (`so`: fast 215 s, release 359 s, full products) | a function of the tree |
| (e) as prototyped (gdb tracer) | tracing the built products: fast 59 s, release 26 s (all probes in parallel); no second compile, because the list no longer feeds the build tag (the regen path's second build, 396 / 736 s in all, comes from the list living in `crates/`) | a re-trace of the same products gave a byte-identical list, both profiles (fixed point); with that list, a second plain `cargo xtask so` passed every build check and gave a `libc.a` byte-identical to the first build, both profiles; on it `cargo xtask layout-check` (all 12 contracts, STARTUP-TEXT / DATA-WINDOWS included) and `hot_text_products` (every bound, the 0-fault and pad tests) passed, both profiles |
| (a1) | 1 s | a function of the tree; fails the contracts |

## 4. Recommendation and migration

**Recommend (e): the build traces itself, and no function name crosses a build.** It is the only option
measured to keep the counts when the compiler changes its mind (3.2), it removes the failure class that stops
unrelated branches (nothing can go dead), it groups class 3 automatically, and on next it holds the counts in
21 of 22 rows with a smaller group (3.1). It keeps the D-X.20 / D-X.63 mechanism (`group()`, renames, cluster
alignment, TEXT-PREFIX) and every bound and contract; only the source of the list changes. (a1) fails the
contracts, (b) cannot express order or clusters and silently drops stale profile entries, (c) cannot reach
`libc.a`, (f) keeps names and stays blind to class 3.

Stories, in order:
- **LT-1 TRACE-AT-BUILD (M).** `cargo xtask so` links the probes against the archive it built, traces them
  with today's gdb tracer, merges in replace mode and groups from the result, written to
  `target/product/<profile>/` (`hot-text.list`, `startup-text.keys`, `probe-text.keys`, `startup-data.keys`;
  `layout-check` and the tests read them there). The trace set includes every `page_faults` mode and program
  the contracts measure. Both profiles' bounds and contracts green; fast `tool` back to next's count (3.1).
  Plants: a probe dropped from the trace set; a traced section left out of the rename.
- **LT-2 CLUSTERS-BY-PHASE (S).** A cluster is `<name> <probe mode> <ABI call that opens it>`: what that mode
  first touches from that call on. No committed file names an internal function.
- **LT-3 TRACER (M).** A small ptrace tracer in place of gdb (C built by `so` with `cc`; one-shot `int3` at
  every function start of the probe's link map; follows fork; iteration budget as hang guard). Drops the gdb
  dependency; target a few seconds per profile.
- **LT-4 HOST-CANONICAL (M).** The trace must not depend on the build host: every candidate of a traced IFUNC
  resolver grouped by rule (assembly labels, a3), the probes' locale data from the tree, a fixed environment.
  Check: one tree traced on Haswell-EP, Skylake-SP and Zen 1 class hosts gives identical lists.
- **LT-5 DELETE (M).** Remove the committed `hot-text.list`, `startup-text.keys`, `probe-text.keys`,
  `startup-data.keys`, `hot-text.absent`, `hot-text.unpinned` and `check_pinned`, the dead-key check,
  `--hottext-regen`, `hottext-repair`, `hottext-recheck`, `prune.py`, merge mode, the train's `hot-lists` step
  and the one-at-a-time lane. Review the 45 pins that exist only for keys, one wl_percall each (D-X.56
  measured 3 to 45 instructions per start-up row for pinning).
- **LT-6 (optional, M).** The same trace for `libc.so.6` with dynamic probes (option (c) for the shared
  object), judged on dynamic workloads.

Out of scope: `startup-state.list` (D-X.64, statics declared by hand; a static is not inlined away, and a rename
fails only the change that makes it).

Cost to unwind: `group()` and the contracts do not change. Until LT-5, reverting LT-1 restores the committed
list path; after LT-5, reverting LT-5's commit restores the tools.

## 5. Open questions for the owner

1. May `cargo xtask so` run the products it just built under a tracer (gdb, later ptrace)? When it cannot (a
   sandboxed builder): recommend failing, with an explicit opt-out that builds an ungrouped (correct, slower
   to start) product; not a committed snapshot, which brings names back.
2. Reproducibility across build hosts: byte-identical products on any host need LT-4 first. Is "same tree,
   same host class, same bytes" enough until then? Recommend LT-4 before LT-1 becomes the default.
3. HOTLIST-TRAIN-REGEN-2 (not landed): withdraw it if LT-1 is briefed now; otherwise land it as the interim and
   delete it in LT-5.
4. Interim, until LT-1 lands: make the dead-key failure (D-X.53 (2)) a warning on branches. 3.2 shows it fires
   on the harmless class; the bounds and window budgets stay enforced. Recommend yes.
5. The records schema has no `proposed` status; the pending record is `active` and says "Proposed" in its body.
