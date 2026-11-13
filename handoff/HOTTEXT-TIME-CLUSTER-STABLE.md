# HOTTEXT-TIME-CLUSTER-STABLE: handoff

State: branch `task/hottext-time-cluster-stable` from next bc146ccec; head in handoff/reports/HOTTEXT-TIME-CLUSTER-STABLE.json.
Build role kanon:~/w/kn1. Scratch on kanon: `~/work/HOTTEXT-TIME-CLUSTER-STABLE/` (probe-job.sh, proof-job.sh,
measure.py = LAYOUT-STABILITY-RETHINK's with layout-check on the unpadded link of each profile only, verdict.py,
per-tree results.jsonl under `sincos/`, `pow/`, `next/`).

## Finding: the hypothesis was wrong

next merged with task/libm-cplx-sincos-5 and with task/libm-cplx-pow-5 (fast profile, before the fix):
- every hot-text cluster boundary key present; the **time cluster byte-identical to next** (positions 271..=280,
  10 sections, 17,130 bytes, aligned 32 KiB). Only the rebind cluster changed (16 sections, 15,406 B against
  next's 17 / 20,654 B) and rebind stayed at 0.
- mktime after localtime_r 1 at every pre/cold pad (`htcs-probe-sincos-fast`, `htcs-probe-pow-fast`), localtime 20,
  localtime_r + mktime 21 (bound 20).
- perf page-faults, pages of the mktime mode absent from the localtime mode: one address, the entry of an
  out-of-line `liella_legacy::recheck::TscGate::fresh` instantiated for `time_compat::select`'s closure (sincos
  0x70b640, pow 0x62cd00). The fast profile (thin LTO, 16 codegen units) stopped inlining it into `select` (a key)
  when libm grew; the copy has no key, so it sits in cold text.
- release profile of the same sincos tree: all `recheck` code inlined, mktime 0, localtime_r + mktime 19: passes.
- Why a boundary cannot be the cause on next: a cluster's range comes from list positions (`parse_clusters`), not
  from the archive, and a key missing from the archive fails the plain `so` (dead-key check, D-X.56). LAYOUT-RETHINK
  3.2 measured with `--hottext-regen` (the check off), where executed-but-unlisted functions also rose (7 / 26).

## Fix (pending record records/pending/HOTTEXT-TIME-CLUSTER-STABLE-1.toml, series D-X)

`#[inline(always)]` on the unkeyed functions of `select`'s lock-free path: `TscGate::fresh`, `recheck::tsc_holds`,
`time_compat::Entry::zone`, `zone::Glue::entry_at`, `tb::zone_entry_at`. Per-call counts: see the report json.

## Proof (both profiles, measure.py: 147 configurations per profile = orig + 6 regions x 19 pads + 2 anchors x 16)

| tree | fast mktime | fast lt+mktime | release mktime | release lt+mktime | three tests | layout-check |
|---|---|---|---|---|---|---|
| next + sincos-5 + fix (`htcs-proof-sincos`) | 0 at all | 18-19 | 0 at all | 17-19 | pass both | rc 0 both |
| next + pow-5 + fix (`htcs-proof-pow`) | 0 at all | 18-19 | 0 at all | 17-19 | pass both | rc 0 both |
| next + fix (`htcs-proof-next`) | see report json | | | | | |

strings and rebind 0 in every configuration of every tree.

## Not done, with reasons (RULING NEEDED)

- Brief item 1 (fixed cluster boundaries plus a check that every boundary is fixed): not done, because the
  boundaries were not the cause (above). The internal boundary keys (time both, rebind last, rebind-tail both,
  float both) carry the same risk as the other ~240 unpinned keys: a loud dead-key build failure, not a silent
  misalignment. Recommend pinning them with the #5068 batches (counts per pin), not as a special case. The brief's
  plant (internal boundary) therefore has no test to catch it; the plants here move each always-inlined function
  back out of line instead.
- The class itself (a function on a 0-fault path that a codegen change outlines without a key) stays open for other
  paths until LT-1 (trace at build) lands. Recommend raising LT-1's priority over more per-path inline rules.

## Next step

Coordinator: land after the jobs in the report json verify; rule on the two items above.
