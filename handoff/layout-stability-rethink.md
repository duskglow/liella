# LAYOUT-STABILITY-RETHINK: fault and window contracts that unrelated changes cannot move

**State (2026-11-10):** planning story, no product code. Branch `task/layout-stability-rethink` from origin/next
e31d71962. Measured on mei (Skylake-SP, the CI host class), role `~/w/s4`, job `lsr-jitter-2` (rc 0; products of
both profiles built by `lsr-jitter-1`, which was stopped after its builds, see Jobs). Scripts and the full matrix:
mei `~/work/LAYOUT-STABILITY-RETHINK/` (`measure.py`, `analyze.py`, `fa.c`, `matrix.txt`, `summary.txt`,
`results.jsonl`).

## 1. Method

`page_faults.c` linked exactly as `hot_text_products.rs` links it, against a copy of the products in which
`liella-crt1.o` is `ld -r`-merged with a pad object. crt1 is the first object of every link, so its pad sits at the
head of its output section and shifts everything after it. Six regions: `pre` (`.text.unlikely.*`, before the hot
group), `grp` (`.text.sorted.h00040.~`, inside the group right after the start-up tier), `cold` (`.text`, after the
group), `ro` (`.rodata`), `data` (`.data`), `bss` (`.bss`). 19 pads each: 0, 0x30, 0x800, then 4 KiB to 64 KiB in
4 KiB steps. Two anchor variants, each with a cold pad of 0 to 60 KiB: `roalign` (an empty 64 KiB-aligned section
at the head of `.rodata`) and `dataalign` (the same at the head of `.data`). Per configuration: every probe mode
through the relay harness (one warm-up, five runs, lowest kept, `ADDR_NO_RANDOMIZE`, the test's fixed path length,
argv0, cwd and environment; **every run of every configuration was identical**), `perf record -e page-faults -d` of
the true / locale / tool modes attributed to sections (T text, R read-only data, Dr relro, D .data, B .bss/.lbss,
O everything else), and `cargo xtask layout-check` on 3 of the 19 phases (it disassembles all of `libc.a`, about
2 minutes per run under load 30). 294 configurations, both profiles.

## 2. Measurements

**Fault-around on mei's kernel** (6.8.0-149-generic; `fa.c`, pagemap present bits): a read fault on a file mapping
maps 16 pages (64 KiB) starting at the **64 KiB-aligned virtual address** below the fault, or at the mapping's start
when that is later (a mapping at address = 4 KiB mod 64 KiB: a read of its page 5 maps its pages 0-15, of its page 20
pages 15-30); the file offset plays no part (offset 4 KiB, address aligned: pages 0-15). A write fault on a private
file mapping maps one page, no fault-around; anonymous memory one page; read then write of one page costs 2 faults.
So text, read-only data and *read* accesses of the data segment count 64 KiB virtual windows; written data and .bss
count 4 KiB pages. Linker: GNU ld 2.42 (Ubuntu).

**Baseline (origin/next e31d71962, minor faults):**

| profile | true | locale | tool | fork | mktime | localtime | lt+mktime | float | txtW | roW | rwP |
|---|---|---|---|---|---|---|---|---|---|---|---|
| fast | 16 | 52 | 55 | 7 | 0 | 20 | 20 | 2 | 5 | 3 | 16 |
| release | 15 | 54 | 57 | 6 | 0 | 19 | 19 | 4 | 4 | 4 | 21 |

Attribution (fast tool): T 7, R 2, Dr 1, D 4, B 7, O 33 (stack, heap, locale archive, vDSO; not layout).

**Jitter: range of each count over the 19 pads of one region** (fast; release in the matrix, same pattern):

| pad region | true | locale | tool | fork | mktime | localtime | float | txtW | roW | moving classes |
|---|---|---|---|---|---|---|---|---|---|---|
| pre (cold fns before group) | 16-17 | 52-55 | 55-57 | 6-8 | **0-1** | 19-21 | 2-4 | 4-5 | 3-4 | T, R, Dr |
| grp (inside the group) | 16-17 | 52-56 | 55-58 | 6-8 | **0-1** | 19-21 | 2-5 | 5 | 3-4 | T, R, Dr |
| cold (text after group) | 16-17 | 52-55 | 55-58 | 7 | 0 | 20-21 | 2-4 | 5 | 3-4 | T, R, Dr |
| ro (.rodata head) | 16 | 52-55 | 55-58 | 7 | 0 | 20-21 | 2-5 | 5 | 3-4 | R, Dr |
| data (.data head) | 15-16 | 51-52 | 54-55 | 7 | 0 | 19-20 | 2 | 5 | 3 | B |
| bss (.bss head) | 16-17 | 52-53 | 55-56 | 7 | 0 | 20 | 2 | 5 | 3 | B (rwP 16-17) |
| roalign + cold 0-60K | 16-17 | 54-55 | 57-58 | 7 | 0 | 20 | 2 | 5 | 3 | T only |

Release: mktime 0-1 (pre, grp), localtime_mktime 18-20, float 4-6, txtW 4-5 (pre, grp), text-class faults of the
true mode 4-6 (grp), rwP 20-21 (data) and 21-22 (bss). All five counted runs were identical in every configuration.

**What moves and where:**
1. **The hot group does not start at a fixed place.** Everything between the start of `.text` and the group is
   libc's own `.text.unlikely.*` (Rust `#[cold]` functions: fast 154 sections, 47,007 B; release 127, 69,623 B; plus
   one 34-byte `.text.startup` and one 7-byte `.text.exit`). GNU ld's default script places them before
   `SORT(.text.sorted.*)`, so any change to any cold function in libc (a new panic path, an error branch in libm)
   moves every window boundary inside the group. `pre` shows it: mktime after localtime_r takes 1 fault at 4 of 19
   phases (0x800-0x3000), the start-up text windows go 5 -> 4, fork 6-8, localtime 19-21 (bound 20: the #4246 red
   reproduced by padding alone).
2. **Read-only data and relro windows move with text size.** The read-only segment starts at the first page after
   the text, the data segment after that, so text growth rotates both through the 64 KiB grid in 4 KiB steps: `cold`
   gives R 2 -> 3 from +0x5000, Dr 1 -> 2 from +0x8000, text 7 -> 8 from +0xB000 (the probe's own main moves), tool
   55 -> 58 at +0xB000 to +0xE000; +0x10000 returns R and Dr to the baseline (period 64 KiB, as the kernel's
   virtual-address windows predict). This is #4320 (global_locale::C) and #4297 (+0x30 rodata).
3. **Inside .rodata, link order decides.** `ro` pads move R 2 -> 4: the start-up objects are scattered over 880 KiB.
4. **Written data counts pages, and only shifts within .data/.bss move it** (rwP 16-17, 20-22; B 6-7).
5. **Anchoring without grouping fixes the count at an arbitrary value.** `roalign` made R and Dr constant over the
   whole text range (R 3, Dr 2), but higher than the baseline's lucky phase (R 2, Dr 1): locale 54-55 against 52,
   tool 57-58 against 55. Same lesson as D-X.51 (text anchor) and LAYOUT-STARTUP-RODATA (start-up rodata alone): a
   window boundary at a fixed place only helps when the hot bytes behind it are contiguous. File cost of the anchor:
   the static probe grows by 49,152 B (fast) / 28,672 B (release) at pad 0, and by 0-60 KiB as the text phase varies.

## 3. Design

**Principle: one contiguous hot cluster per fault class, each either at the head of its mapping or aligned to the
smallest power of two not below its size (at most 64 KiB); everything cold after it.** A cluster of size S aligned to
A >= S (A a power of two, A <= 64 KiB) can never straddle a 64 KiB window (64 KiB boundaries are multiples of A), so
it costs exactly one window wherever the rest of the program puts it; a cluster above 64 KiB aligned to 64 KiB costs
exactly ceil(S / 64 KiB). Clusters that follow each other with nothing cold between them share windows by their own
sizes only. Every count then depends on the hot code and data, and unrelated growth (cold code, cold data, other
segments) can only move the clusters as a whole, which the alignment absorbs. Padding is at most A - 1 bytes per
cluster (file and address space), not a fixed 64 KiB.

| class | today | design | cost | expected (fast tool) |
|---|---|---|---|---|
| text, start-up tier | group start = 0x401000 + 47-70 KiB of libc cold code | no libc input section before the group (rename libc's `.text.unlikely/.startup/.exit/.hot.*` to `.text.*`); start fixed by the text segment start, .init and .iplt | none (reorder) | T 6-7, fixed by hot sizes (one-off change of 0 or -1 when the cold block leaves the first window) |
| text, 0-fault paths (time, strings, rebind, float) | any group shift can split them (mktime 0-1) | each is a window-safe cluster: first section's `sh_addralign` = pow2 >= cluster size, written by xtask as for D-X.51 | < cluster size each (time path: a few KiB) | 0 in every phase |
| read-only data | scattered over .rodata; R 2-4 | every fault-probed read-only object (all probes' traces, tiered like the text) in one `.rodata1` cluster aligned to pow2 >= its size (LAYOUT-STARTUP-RODATA-2 is this, with 64 KiB) | < cluster size; 64 KiB anchor measured +28-49 KiB file | R = ceil(S_ro / 64 KiB) = 1 if the cluster <= 64 KiB |
| relro read | Dr 1-2 with text size | start-up-read relro objects first in `.data.rel.ro` (`.data.rel.ro.local.*` sorts first in the default script), aligned pow2 >= size | < 4 KiB if the cluster fits a page | Dr 1 |
| written data and .bss | rwP 16-21 pages, scattered | written start-up state in one `.data1` cluster (extends move_startup_state), aligned pow2 >= size (>= 4 KiB) | < cluster size; zero-initialised members become file bytes | ceil(S_w / 4 KiB): NOT MEASURED (S_w unknown; expected well under today's D 4 + B 7) |

Expected tool total, fast: O 33 + T 6-7 + R 1 + Dr 1 + written pages (estimate 3-6) = 44-48 against 55 today, the
same in every phase. The estimate is not a measurement: brief B measures it.

**Contracts on the outcome, per class.** Today the window contracts map keys (`startup-text.keys`,
`startup-data.keys`) to symbols, so a key lost to inlining or renaming (#4302, #4283) or a function no probe traces
(getrusage, #4327) breaks or blinds them. Instead: `layout-check` runs the contract programs once under
`perf record -e page-faults -d` (0.1 s each; perf works for the claude user on mei) and counts faults per class and
per cluster. Bounds per class: text windows of the hot group, read-only windows, relro windows, written pages, and
"faults outside every cluster = 0 apart from O". A rise names the class and the symbol at each new fault address. The
key lists stay as placement input only. Phase stability is itself checked: a product test links the probe at four
`pre`/`cold` pads (as LAYOUT-STARTUP-RODATA's 16-pad test did) and requires identical per-class counts.

**Regen workflow (brief update, #4291, #4318, #4326-#4328, WCTYPE-TRIE-FILE-6).**
1. One list per profile (`hot-text.fast.list`, `hot-text.release.list`), each written only from its own profile's
   trace; a fast regen never reorders release keys (the position 50 -> 255 move).
2. Regen traces the contract programs themselves (`page_faults <mode>` for every mode, the tool program), so a
   function only the measuring code calls (getrusage) gets its real tier instead of the list's tail.
3. The regen build reports contract and dead-key failures without failing (as D-X.56 does for dead keys); the
   plain `so` after it enforces. A contract never blocks the tool that restores it.
4. Hermetic trace: `env -i` with fixed LANG/LC_ALL, `ADDR_NO_RANDOMIZE`, fixed path (HOTTEXT-REGEN-HOSTS);
   `python3 -B` (no `__pycache__` in the role, #4326); call `bash tools/hottext/regen.sh` (exec bit, #4328).

**Alternatives considered.**
- 64 KiB anchors at segment or group heads (D-X.51, `roalign` here): invariant but at a worse count unless the hot
  bytes are grouped first; costs up to 60 KiB of file each. Kept only where a cluster exceeds 32 KiB.
- `-z max-page-size=0x10000` or a linker script: not available for users' static links (libc cannot pass link
  flags); possible for `libc.so.6` only (C3 of the contracts plan), and worth it there for dynamic programs.
- Phase-free bounds without moving code (bound = worst phase): stable, but each bound loosens by 1-3 and the
  ratchet loses its teeth; rejected as the end state, usable as an interim (rule below).
- Hot read-only data in the text segment or in notes: rejected (data in executable pages; note abuse).

## 4. Code briefs

**A. LAYOUT-TEXT-PIN (size M).** `cargo xtask so`: rename every libc.a `.text.unlikely.*`, `.text.startup.*`,
`.text.exit.*` and unlisted `.text.hot.*` section to `.text.<f>`; give the first section of each 0-fault cluster
(time path: localtime_r/mktime keys; strings after printf; rebind; first float conversion) `sh_addralign` = pow2 >=
cluster size. Contracts: `TEXT-PREFIX` (no libc.a input section between `.text` start and the group except .plt/.iplt)
and `CLUSTER-WINDOW` (each named cluster inside one 64 KiB window). Product test: the probe at pads 0x800, 0x2000,
0x8000, 0xF000 of `pre` and `cold` (crt1 `ld -r` as here) gives the same mktime / strings / rebind (0) and the
same text-class count. Plants: drop the rename (TEXT-PREFIX fails); drop the cluster alignment (the pad test fails,
mktime 1 at 0x800); rename only `.text.unlikely.*` (TEXT-PREFIX must still name any other libc section left before
the group; first find which object owns the 34-byte `.text.startup` and 7-byte `.text.exit` of the probe link).
Counts: hot_text_products both profiles, before/after, every bound.
**B. LAYOUT-DATA-CLUSTERS (size M, after A and LAYOUT-STARTUP-RODATA-2).** Written start-up state into one
pow2-aligned `.data1` cluster; start-up-read relro objects to the head of `.data.rel.ro`; LAYOUT-STARTUP-RODATA-2's
`.rodata1` alignment from 64 KiB to pow2 >= its size. Measure S_w and the per-class counts first. Plants: skip the
.data1 move for one object (rwP rises, names it); drop the relro rename (Dr moves at the `cold` pads).
**C. LAYOUT-FAULT-CONTRACTS (size M, after A).** Per-class, fault-address contracts in `layout-check` (perf), the
phase test, and the regen workflow items 1-4. Plants: a start-up function moved out of the group (named at its
fault address); a fast regen that rewrites the release list; regen build failing on a contract.

**Which stories wait.** HEAP-FIRST-USE-6 (time-path 0-fault bound, #4319/#4320) waits for A. TB-FOREIGN-SAFETY-4
waits for LAYOUT-STARTUP-RODATA-2 (#4297). WCTYPE-TRIE-FILE-6 and HOTTEXT-REGEN-HOSTS should land the hermetic
regen first; C then adds per-profile lists. LIBM and other code stories need not wait for anything: until A lands,
apply the interim rule below.

**Interim rule (RULING NEEDED).** Until A and B land, a fault-bound or window-budget red on a change that touches no
start-up code is judged with this experiment's tool: if `measure.py` at origin/next shows the same count at some pad
of `cold` or `pre`, the delta is phase jitter, not a regression (record the pad). Recommend accepting.

## 5. Open questions / not verified
- S_w and the expected written-page count (brief B measures them first).
- The one-off text count after A (0 or -1 expected; measured by A).
- `libc.so.6` and dynamic programs: not measured here (static probe only).
- Rulings: (1) the interim rule; (2) per-class bounds re-derived once after A/B (each may move by 1 once while the
  totals do not rise), then ratchet; (3) contracts need perf on the build hosts (fallback: key-based check).

## Jobs
- `lsr-jitter-2` (mei, rc 0, 490 s): the measurement of section 2.
- `lsr-jitter-1` (mei): built both product profiles (fast 326 s, release 362 s) and the fault-around probe, then
  stopped by me (`kill-label`): `layout-check` on all 294 configurations would have taken 2 hours. Superseded.
