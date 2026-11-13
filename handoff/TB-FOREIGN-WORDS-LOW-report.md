# TB-FOREIGN-WORDS-LOW-4 report

Branch `task/tb-foreign-words-low-4`, host keke:~/w/n1. The handoff is handoff/TB-FOREIGN-WORDS-LOW.md.

## Commits (this run)
- 07c81c2b3 Record the fast-profile inlining of five CSysImpl hot-text keys (`cargo xtask hottext-repair`).
- fbf21301f Measure the errno_location percall case through a call (validation/percall/foreign_words.c).
- handoff commits: this report, the handoff, the plants file fix, handoff/reports/TB-FOREIGN-WORDS-LOW-4.json.

## The five fast-profile keys
They are inlined in fast, not renamed (no symbol of theirs in target/product/fast/libc.a; their modules sys.rs,
recheck, intl, nscache are unchanged on this chain). The fix uses the repo's own tool, which built both profiles:
- dropped from hot-text.list (no profile matches; traced in fast only):
  `legacy::recheck::TscGate::fresh<time_compat::select<CSysImpl<false>, zone::Glue>::{closure}>`,
  `legacy::intl::bind<CSysImpl<false>, catalog::Catalogs>`, `legacy::intl::textdomain<CSysImpl<false>, catalog::Catalogs>`;
- recorded in hot-text.absent for fast (release still matches):
  `<CSysImpl<true> as Sys>::wait4`, `legacy::nscache::NsCache::attach<CSysImpl<false>>::{closure}`.
No hand rename was needed (#4849 not used again).

## Jobs (keke)
| label | head | result |
|---|---|---|
| tb-foreign-words-low-4-lint0 | 2ca200b6c | clippy, static ok; fast `so` red (the 5 keys); superseded |
| tb-foreign-words-low-4-repair | 2ca200b6c | rc 0, 3 dropped, 2 recorded |
| tb-foreign-words-low-4-lint1 | 07c81c2b3 | rc 0: clippy, static, fast `so` |
| tb-foreign-words-low-4-plants | 07c81c2b3 | rc 0: 6 CAUGHT, 1 WARN-NO-MSG (errno-inline-registry did not compile: see below) |
| tb-foreign-words-low-4-plants2 | 07c81c2b3 | rc 0: errno-inline-registry (corrected pattern) CAUGHT, conformance s00_errno 0 passed / 1 failed; all 7 plants now CAUGHT |
| tb-foreign-words-low-4-percall2, -percall2-rel, -percall2-relbase | 07c81c2b3 products, spec of fbf21301f | rc 0 (table in the handoff) |
| tb-foreign-words-low-4-gate | 07c81c2b3 | `affected --run --tier full`; see the hand-back |
Superseded: tb-foreign-words-low-4-percall, -percall-rel, -percall-relbase (old spec: the errno case counted the loop only).

Later commits change only validation/percall/ and handoff/, so the gate on 07c81c2b3 covers the code of the final head.

## Plants
`errno-inline-registry` in -3's file replaced `with_exposed_provenance_mut(...)` after a `core::ptr::` prefix and
did not compile (E0433), so the tool's WARN-NO-MSG was a build failure, not a catch. The pattern now includes the
prefix (one site in tb.rs).

## Percall (#4779, ruling #4889: judged on Release)
errno_location +2 (9.0 -> 11.0, the owned `cmpb`/`je`), malloc_free and calloc_free +0 on Release; Fast the
same (calloc 163.9 -> 164.0). The Release base is the merge-base ca8aa835a: exported with `git archive` into
~/work/TB-FOREIGN-WORDS-LOW-4/base-src on keke and built with LIELLA_FULL_PROFILE=1 under --rev ca8aa835a-release,
because ~/wl/products/<sha> holds only the fast build of a sha.

## INFORMATIONAL
- The `c_malloc` (heap.rs) and `hyp_quick` (hot/hyperbolic.rs) dead-code warnings come from the product build. The code is the
  same on next, and other stories' logs on keke show the same warnings: they are not this branch's.
- wl_percall caches products per revision name only, not per profile: a Release measurement needs its own --rev key
  (and a hand-made tree for the base). A `--profile` option in wl_percall/wl_lock would make #4889 checks one command.

## Refused
None.
