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

## INTEGRATE-HCONF-2 (2026-11-04)
Merged origin/next into task/integrate-hconf-2. Only conflict: crates/core/tests/time_gaps_probe.rs (add/add); next's version
(fixed test instant, midnight test) taken whole, since this branch held only the older copy of the same code. No hconf change
was in that file. Jobs on keke:n1: integrate-hconf-2-clippy (clippy --all-features -D warnings + xtask static) rc 0;
integrate-hconf-2-gate (affected --base origin/next --run) rc 0. No plants: the change is a merge resolution in a test file, no product code.
Next step: none; CI.

## INTEGRATE-HCONF-3 (2026-11-04)
CI red was lab-names: hostconf_probe.rs used 10.0.100.x in the reorder test (pure function on address arrays). Replaced with
the documentation range 192.0.2.0/24 (and 192.0.3.x for the other subnet); no collision with the 203.0.113/198.51.100 test
addresses. Merged origin/next (one conflict, docs/compat-flags.md: kept both rows; COMPAT-13 here, COMPAT-14 from next).
On shiki:sh1: xtask static rc 0, clippy --all-features -D warnings clean, job integrate-hconf-3-gate rc 0.
No plants: test constants only. Next step: CI.
