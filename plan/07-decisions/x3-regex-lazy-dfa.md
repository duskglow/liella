# X-3a `REGEX-LAZY-DFA`: profile of the regex rows and the lazy-DFA design note

Status: PROPOSED (X-3a, planning; no matcher code). Ledger item: X-3 in
`plan/06-phases/20-post-release-ledger-and-experiments.md`. Builds on D-14.4 (matcher), D-14.12
(byte classes, per-thread store, serial), D-14.13 (existence DFA) and D-14.16 (span passes).
Sources: the spec (§14.9), POSIX `regexec` (leftmost-longest, subexpression rules), published
automata literature (below) and black-box counts. No other C library's source was used.

## 1. Profile (counters decide)

Job `regex-lazy-dfa-profile`: `cargo xtask bench --compare --counters --runs 3 cbench_patterns
cbench_opt` on the 60-vCPU x86-64 build host, then `perf record -e instructions:u` of the
**Liella** static bench binary only (`CBENCH_ROWS=<row>`). Instructions per call, user space:

| Row | Liella | glibc | ratio | time ratio (same host) |
|---|---|---|---|---|
| `regexec_backref` (`\(b[a-z]*\)@\1`, 13-byte subject, nmatch 2) | 24,460 | 21,950 | **1.11** | 1.13 |
| `regexec_class_nomatch_1k` | 8,647 | 8,665 | 1.00 | 0.75 |
| `regcomp_regexec_regfree_email` | 144,404 | ~523k | 0.27 | 0.27 |
| `regexec_submatch_1k` (`([a-z]+)@([a-z]+)\.com`, nmatch 3) | 31,202 | 439,117 | 0.07 | 0.07 |
| `regexec_literal_1k` | 1,129 | 28,878 | 0.04 | 0.04 |
| `regexec_pathological_4k` | 1,293 | (glibc about 65 ms) | ~0.00 | 0.00 |
| `re_sed_kv` (per line) | 2,704 | 12,885 | 0.21 | 0.20 |
| `re_sed_swap` | 3,223 | 40,658 | 0.08 | 0.08 |
| `re_grep_dotstar` | 713 | 2,912 | 0.24 | 0.26 |
| `re_grep_ipv4` | 1,287 | 18,511 | 0.07 | 0.08 |

The ledger's "capture/back-reference rows at t 3.1 to 5.8" predates D-14.13/D-14.16: the
existence and span DFAs already took the capture rows to 0.07 to 0.27 of glibc. **Only
`regexec_backref` is above parity.** (The Zen 3 time ratio given in the brief, 1.08, agrees.)

Where `regexec_backref` spends its instructions (perf, self time): `Matcher<Posix>::run` 32.7%,
`attempt` 14.3%, `insert` 13.8%, `walk` 7.0%, `key_*` 3.5%, `succ` 3.4% (matcher total 74.7%);
scratch clearing and copies (`internal_memset`, `machine_copy_avx2`, `conv::internal_move`)
7.7%. On the capture rows the remaining matcher share is small: `re_sed_kv` `one_*` 23.2% plus
`search` 2.6%; `re_sed_swap` `one_*` 21.2% plus `search` 4.3%; both rows are already at 0.21 and
0.08 of glibc.

## 2. Which rows a lazy DFA would help: negative

* **Back-references** (`regexec_backref`, the only row above parity): a finite automaton cannot
  decide `\n` (the language is not regular), so a DFA can only act as a filter. The strongest
  filter is a DFA over the **relaxed** pattern, where each `\n` is replaced by a copy of group n's
  subexpression (a superset language: every match of the pattern matches the relaxed pattern), used
  to reject a subject or to bound the span the matcher runs on. On this row the subject is 13
  bytes and it matches; the positions outside the relaxed span (`xx `, ` yy`) hold no live thread
  after one step, so the matcher work a span would remove is a few steps of the 74.7%. Estimate:
  at most about 10% of the row, not measured, and the extra pass costs O(subject) of its own.
  It would not bring the row to parity. **Recorded as a negative** in
  `docs/saturation/06-printf-regex-wordexp-iconv.md`.
* **Capture rows**: a capture DFA (section 3) could replace the capture pass on [s*, e*]
  (21 to 26% of `re_sed_*`), but every capture row is already 0.07 to 0.27 of glibc. Counters
  give no parity reason; it stays a deferred option (X-3d/X-3e below), started only if a capture
  row rises above parity or a workload count shows the capture pass is a large share.
* What would pay on `regexec_backref` is the matcher's fixed cost per call (`attempt` 14.3%,
  scratch clearing 7.7%), not a DFA: story X-3b below.

## 3. Design for a capture DFA, if it is started (DoS bound, fallback)

**Approach.** The span is known before the capture pass (D-14.16), so the capture pass is an
*anchored* match of [s*, e*]. For patterns that are **one-pass** on that anchored run (at every
node and byte class at most one consuming successor, and no two epsilon paths to the same
consuming node with different capture effects: the one-pass condition described by R. Cox,
"Regular Expression Matching in the Wild", 2010), exactly one path spans [s*, e*], so its
capture vector is the POSIX answer (POSIX chooses among parses of that one span; there is one).
The bench capture patterns `([a-z]+)@([a-z]+)\.com`, `^\([a-z0-9_]*\) = \(.*\)$` and
`\([a-z]*\)_\([0-9]*\)` are one-pass when anchored at s* with e* known. General patterns would
need a tagged DFA with POSIX disambiguation (Laurikari, "NFAs with tagged transitions", 2000;
Trofimovich, "Tagged deterministic finite automata with lookahead", 2017; Borsotti and
Trofimovich, "Efficient POSIX submatch extraction on NFA", 2021). That is out of scope until
counts show a need.

**Memory cap (fixed, not proportional to input).** The one-pass table is built at `regcomp`
inside the compiled block: one row per node of at most `NODE_CAP` (1024) nodes, a byte-class
successor per entry plus a capture-slot action word. A hard cap of **16 KiB per program**
applies; a program whose table would exceed it gets no table. `regexec` allocates nothing new:
the table is read-only and shared, and the existence and span passes keep the D-14.13 store
(per thread, 32 KiB: 2 LRU entries of 3,436 words, at most 192 states each, kernel at most 256
nodes).

**When the cap is hit.** At `regcomp`: no table; the program uses today's capture pass. At
`regexec`: the existence and span passes keep the D-14.13 rules (a full entry is flushed; a third
flush in one call, or a second after fewer than 4 bytes per state built, falls back to the
matcher, exact).

**Worst-case time.** The one-pass run costs O(e* - s*) with one table lookup per byte and no
state construction. If the run fails to reach `Match` exactly at e*, the D-14.16 exactness
guard hands the search to the matcher. That cannot happen for a correct table, and tests count
it as 0. The bound stays today's: attempt plus fallback is at most about 2x the matcher plus
O(subject) on hostile patterns, the D-14.13 bound, and the one-pass run adds O(span) at most once.

**When the DFA cannot be used, chosen per pattern at `regcomp`** (a block-head flag, set once):
back-references (`has_backrefs`); more than `NODE_CAP` nodes; no predecessor section; encodings
other than single bytes and UTF-8; the one-pass check fails; the table would exceed the cap.
Per call, the D-14.13/D-14.16 run-time fallbacks still apply: a backward or partial window, or a
UTF-8 byte >= 0x80 in the span (LEAVE).

## 4. Proof plan for X-3b onwards

* Regex suites: `cargo xtask affected --run` on the direct tier (the `crates/core/tests/regex_*`
  suites, `crates/boundary/tests/regex_dfa.rs`, `regex_paths.rs`, `regex_reentry.rs`,
  `regex_dfa_signal_fork.rs`, `regex_enomem.rs`), plus a differential test of the new path
  against the matcher, in the style of `dfa::tests::the_spans_answer_as_the_matcher`: BRE/ERE x
  ICASE x NEWLINE x NOTBOL/NOTEOL x C/UTF-8, every bench pattern, and the D-14.8 stacks.
* Fuzz: `fuzz/fuzz_targets/regex_dfa.rs` and `regex.rs`, a fixed number of runs (`-runs=N`,
  never a time limit), on a RAM copy.
* Counters: `-percall` for every regex row in both benches; keep the change only if no row is
  slower and the target row falls.
* Plants (each must be CAUGHT, `msg:` naming the test): (a) **cap**: ignore the 16 KiB check, so
  a program just over the cap must still report "no table" (a counter test); (b) one-pass check
  accepts an ambiguous pattern (`(a|ab)(c|bcd)`), so the differential test must fail; (c) the
  capture-slot action is skipped on one transition; (d) for X-3b, the trimmed scratch clearing
  leaves a stale slot, so `regex_backref_*` must fail.

## 5. Story split

| Story | Size | What | JOBS |
|---|---|---|---|
| X-3b `REGEX-BACKREF-FIXED` | S | Cut the back-reference matcher's fixed cost per call (`attempt` setup, scratch clearing 7.7%) on `regexec_backref`; no DFA | `-gate`, `-plants`, `-percall`, `-static` |
| X-3c `REGEX-ONEPASS-TABLE` | M | **Deferred** (trigger: a capture row above parity). The one-pass check and the capped table at `regcomp` (program section, block-head flag), unused by `regexec`; cap plant (a) | `-gate`, `-plants`, `-static` |
| X-3d `REGEX-ONEPASS-RUN` | M | **Deferred**, after X-3c. Use the table for the capture pass on [s*, e*]; differential test; plants (b), (c); fuzz fixed-run | `-gate`, `-plants`, `-percall`, `-fuzz`, `-static` |
| X-3e `REGEX-ONEPASS-DOCS` | S | **Deferred**, after X-3d. Register, `docs/benchmarks.md` and D-row with the counts | `-static` |

No decision row is needed for X-3a: the note proposes, and each implementation story records its
own D-14.x row.
