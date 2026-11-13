# LAYOUT-TRACE-HOST-CANONICAL (LT-4): handoff (run 4, LAYOUT-TRACE-HOST-CANONICAL-4, COMPLETE)

State: branch `task/layout-trace-host-canonical-4` (on top of -3's head c47ca50, no merge), role keke:~/w/n1 (Zen 1).
Spec: `~/state/briefs/LAYOUT-TRACE-HOST-CANONICAL.md` "The change"; plan `plan/layout-rethink.md` (branch
task/layout-rethink) 2(e), 3, 4; rulings #5191, #5222, #5223, #5241 (prove with `--both`; no contract relief), #5261
(prove under a UTF-8 job as well as under the gate). Decision: pending record
`records/pending/LAYOUT-TRACE-HOST-CANONICAL-2-1.toml` (D-NEW, series D-X; the coordinator assigns the id). Code:
tools/hottext and xtask/src/hottext.rs (tests, one message); xtask/src/so.rs untouched (LT-1 owns it).

## What the four runs did (details: the commits on the branch, the -2/-3 reports in handoff/reports/)

- Run 1-2: the tracer removes every host input a trace showed (virtual CPU per class, fixed environment, tree-supplied
  host files, fresh nscache region, syscall stops told by %orig_rax); 17/17 identical traces on three hosts.
- Run 3: lthc2-gate red explained and fixed (runner test); `--both` documented as the both-profile path (#5241);
  `--replace` prunes stale hot-text.absent lines; alias order no longer follows the caller's locale (nm LC_ALL=C,
  names at one address in byte order).
- Run 4 (this run): no code change. The pending record now names the tool locale, `--both` and the `--replace`
  pruning, and the proof below; the cross-host proof was rerun in both locales; the gate and plants were rerun on the head.

## Proof (run 4: tree = git archive of adbea33 at one path on every host, `regen.sh . --replace --build` then `regen.sh . --both`, then `cargo xtask so` in both profiles)

One job per host (`~/work/LAYOUT-TRACE-HOST-CANONICAL-4/proof4-run.sh`): pass `gate` under `~/bin/gate` (LANG unset,
LC_COLLATE POSIX), then pass `utf8` as a plain job with LANG=en_US.UTF-8, no LC_ALL (LC_COLLATE en_US.UTF-8); the
locale is printed by each pass. sha256 (first 16 hex digits), identical on Zen 1 (EPYC 7551), Skylake-SP (Platinum
8151) and Haswell-EP (E5-2697 v3), in both passes:

| step | digests |
|---|---|
| fast lists (hot-text.list startup-text.keys probe-text.keys startup-data.keys; absent) | fd7e54b5 3242d4f0 c8d5c8c0 2d993846; b2c3f708 |
| fast products after `--replace --build` (libc.a libc.so.6) | c5c5a4c2 0f81862d |
| `--both` lists (same order; absent) | fcc9ddee fdcc38fd dc503dd5 8033f217; b2c3f708 |
| `--both` regen fast (libc.a liella-crt1.o) | c5c5a4c2 0d112d38 |
| `--both` regen release (libc.a liella-crt1.o) | 06c9d74d e0e062cc |
| `cargo xtask so` fast with the `--both` lists (libc.a libc.so.6 crt1) | 694fc7d7 aec11a0d 0d112d38 |
| `cargo xtask so` release (LIELLA_FULL_PROFILE=1) | 06f2b44b c31c6fb6 e0e062cc |

The fast lists equal run 2's and run 3's, and the products equal run 3's at another tree path. Bounds (keke, both
passes): layout contracts (`cargo xtask so`) and `cargo test -p liella --test hot_text_products` green in fast and
release. Trace cost (reported, not a gate): fast regen 669-1131 s, `--both` 522-989 s per host under the hosts' load.

## Jobs on the final head

keke: `lthc4-lint0` (clippy all-features + static on the inherited head), `lthc4-prod` (RAM products for the plants),
`lthc4-gate` (affected --run --base origin/next: 32 suites, 11692 passed, 0 failed; conformance 130/0; peak 11G),
`lthc4-plants` (handoff/LAYOUT-TRACE-HOST-CANONICAL-4-plants.txt), `lthc4-proof-keke`; mei: `lthc4-proof-mei`; shiki:
`lthc4-proof-shiki`; `lthc4-lint1` (clippy all-features + static on the hand-back head, which adds only handoff/ files to
the gate's and plants' head 0e2b127; the proof tree adbea33 has the same code).

## Open questions / next step

None for this story. Next: the coordinator assigns the pending record's id; LT-1 (trace-at-build) can then rely on
host-independent lists. Later speed goals (owner 11-13): LT-3's tracer or a trace cache keyed by its inputs.

Things not to redo: namespaces for data (nscache private table); `files`-only nsswitch or empty /usr/share/locale;
detecting syscall stops from gdb's printed output or from the pc; the one-profile release regen (#5241: use --both);
a proof only under `~/bin/gate` (its C locale hid the alias order, #5261).
