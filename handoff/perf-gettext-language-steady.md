# PERF-GETTEXT-LANGUAGE-STEADY(-2) (branch task/perf-gettext-language-steady-2, role keke:~/w/r1)

State: INCOMPLETE. Head = origin/next 2c1e25b9 merged (ruling #2897) + this story's own work only: the per-call spec
validation/percall/gettext.c (9d0076cd) and docs. No library change on top of next: the run-1 code change (drop of the
fused LANGUAGE compare, 123dfe88) is identical in effect to PERF-REGRESS-1023-1105-2's back-out, which landed; the merge
conflict in crates/boundary/src/catalog.rs was resolved to next's text. `git diff origin/next` = 4 files, no Rust.

## Per-phase split of one call (wl_trace, Zen 1 host keke, code = next + spec; skip 20)
glibc per-call totals (Skylake-SP, run 1): found 446, steady 457, ngettext 586; Zen 1 percall column: see Counts.
- gettext_found 621 i: translate 236 (entry 38, locale/untranslated 26, lock+clock 8+rdtsc 4, key setup 60, between
  `matches` and `hit` 86: binding/codeset/memo_gated/planned, return 18), Resolution::hit 147 (91 + 56 around the
  index probe), matches 70, find_in_hinted 65, same_bytes 21, gettext 30, strlen 11, language 27 (unset).
- gettext_language_steady 727 i = found + 106: Host::language 49 (+22), env_find 41 (the memo's one-entry name
  re-check), strlen of the value 11, matches 79 (+9) + same_part 23 (the list words). No clock read on the traced call.
- ngettext_found 724 i: translate 236, hit 210 (147 + plural form 63), matches 70, find_in_hinted 65, ngettext 39,
  plural::evaluate 31, language 27, same_bytes 21, strlen 11.
Surplus over glibc, steady: ~175 in the shared found path (translate key setup + the 86 between matches and hit,
matches' 70 for a key whose domain and locale match by address, hit 147) and ~95 in LANGUAGE.
LANGUAGE bound (register 14 row 6 runs 2-4, not redone): the value must be read each call (in-place rewrite); length
memo and address compare are unsound; the fused aligned compare is slower (+131) and any unaligned-word variant needs
new unsafe in liella-boundary (RULING). So the next gains are in the shared path, which also moves found/ngettext.

## Ruling #2896 (ngettext_found +3 per call)
See Counts: percall base (3d7e667a, fused compare in) vs head (next, back-out in), same host, same spec.

## Counts (keke, Zen 1)
PENDING at hand-back: job keke:perf-gettext-language-steady-2-percall2 (wl_percall, glibc / base 3d7e667a / head) was
still queued on a full host (28 of 28 cores, libm sweeps) after 35 min. Read it with
`ssh keke 'tail -20 ~/logs/perf-gettext-language-steady-2-percall2.log'`; its ngettext_found base vs head row answers
ruling #2896 (head = next's code, so a +3 there is next's back-out, not this branch). Run-1 numbers (Skylake-SP):
percall glibc / old next / back-out: steady 457 / 909 / 742, alternating 2041.6 / 1511.4 / 1361.5, found 446 / 652.8 /
626, ngettext 586 / 728 / 731. NOT EXPLAINED in this run: there is only a head trace (ngettext 724 above); no base trace was
obtained (see Superseded), so where the +3 sits is not known.

## Jobs (keke)
- perf-gettext-language-steady-2-gate: clippy -D warnings + xtask static + xtask affected --run, rc 0; selected 0 of
  233 targets (no code differs from next).
- -trace-head (ngettext_found), -trace-steady, -trace-found: rc 0 (wl_trace, logs on keke ~/logs).
- -percall2: wl_percall gettext spec, base 3d7e667a.
- Superseded: -percall (killed before it started: its jobrun was in a foreground ssh I stopped; no jobs.tsv row),
  -trace-base (wl_trace refuses a bare ~/wl/src/<sha> dir: "neither a git tree nor a role directory").
- No plants: no product code changed on this branch.

## Next step
Shared found path, in order: (1) `Resolution::matches` 70 i on an address-matched key: read its --list trace
(`wl_trace.sh --list --spec validation/percall/gettext.c . gettext_found gettext`; the I-lines did not reach the job
log in this run, check where --list writes) and find why the address fast path still costs that much (key.lens(),
three length loads, the div_ceil offsets computed before the address test); (2) the 86 instructions between matches and
hit in translate (binding words, codeset, memo_gated, planned); (3) hit's 147. Each with percall + cbench_opt counts,
gate and plants. Target 1.05x is not reached on any row.
