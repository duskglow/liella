# INTEGRATE-HCONF handoff

State: branch task/integrate-hconf. Composes HOSTCONF-DIAGNOSTICS-2, HOSTCONF-OPTIONS-PROBE-3, TIME-SPEC-GAPS-SETTLE-3
(merged in that order from origin/task/*) and NS-NETWORKS-DNS-2 (ported as one patch of its final changes, from
liella-historic task/ns-networks-dns-2, base 3f84e8cd .. tip).

## What was done
- Merge 1 (diagnostics) and 3 (time gaps) clean. Merge 2 (options-probe-3) conflicted in crates/core/src/names/hostconf.rs
  (add/add) and crates/legacy/src/nss.rs: both stories began from the same first host.conf parser; the diagnostics branch
  is a strict superset of it (probe-3's side added nothing beyond that parser), so the diagnostics side is kept.
- NS-NETWORKS-DNS-2 ported with `git apply --3way`; the only conflict was docs/compat-flags.md.
- COMPAT id: the story's COMPAT-11 became COMPAT-13 (COMPAT-11 and COMPAT-12 are taken on next); text in
  compat-flags.md, differences-from-glibc.md, spec/behaviour/ns-gaps.md and the NS handoff/report files renamed.

## Causes of train 228's failures
1. clippy error `hconf` missing in `liella_legacy::nss::Ctx`: the hostconf work added `pub hconf: Option<Settings>` to
   `Ctx`; the NS-NETWORKS-DNS-2 test crates/core/tests/ns_gaps_probe.rs builds `Ctx` with a struct literal written
   before that field existed, so it did not compile (clippy --all-targets). Fix: `hconf: None` in that literal.
2. test `ns_gaps`: it lives in the same file (ns_gaps_probe.rs), so it failed with the same compile error.
   After the fix it passes in the gate (below). I did not separately reproduce it before the fix.

## Results
Clippy (workspace, all targets, all features, -D warnings) clean; `cargo xtask static` rc 0; direct-tier gate
(`--base origin/next`): 8 suites ok, 4981 passed, 0 failed, 15 conformance passed.

## Notes
- No plants run: the change is a merge of already planted stories plus one field in a test literal; the product
  code of each story was not changed by me.
- origin/next already has two rows named COMPAT-11 (`__libc_freeres`, `fnmatch` FNM_EXTMATCH+FNM_LEADING_DIR);
  static does not reject it. Not mine; flagged.
- Next step: coordinator closes the four stories as landed through this branch.
