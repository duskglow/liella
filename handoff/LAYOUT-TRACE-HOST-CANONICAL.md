# LAYOUT-TRACE-HOST-CANONICAL (LT-4): handoff (run 3, LAYOUT-TRACE-HOST-CANONICAL-3, context checkpoint)

State: branch `task/layout-trace-host-canonical-3` (on top of -2's branch, no merge), role keke:~/w/n1 (Zen 1). Spec:
`~/state/briefs/LAYOUT-TRACE-HOST-CANONICAL.md` "The change"; plan `plan/layout-rethink.md` (branch task/layout-rethink)
2(e), 3, 4; rulings #5191, #5222, #5223, #5241 (prove with `--both`; no contract relief). Decision: pending record
`records/pending/LAYOUT-TRACE-HOST-CANONICAL-2-1.toml` (D-NEW, series D-X). Code: tools/hottext and xtask/src/hottext.rs
(tests, one message); xtask/src/so.rs untouched.

## Run 3 (this run)

1. **Why lthc2-gate (mei) went red:** two causes, both on the branch. (a) `cargo xtask static`: the gate ran on
   ffb4253, before 551efeb removed the build flag's name from the -2 report json (fixed already, static green
   on the inherited head on keke). (b) a real test failure: runner.sh now passes the program's assignments as
   `HOT_TEXT_ENV_<NAME>`, and `regen_runner_runs_traces_together_and_keeps_their_order`'s fake tracer still printed
   its own LANG. Fixed: the fake prints the environment the way trace_gdb.py builds it, and the test gives the
   runner a caller LANG/TZ that must not leak (f18e111). Not layout jitter.
2. **#5241:** regen.sh's usage and the dead-key message name `regen.sh . --both` as the both-profile path and say why
   a release `--build` after a fast `--replace` fails (bc77b8d).
3. **`--replace` then `--both` failed on every host** (lthc2-proof2-mei: `hottext-recheck: release: hot-text.absent:
   20 stale release line(s)`). --replace drops next's relic keys (tier extra, profiles `-`: no trace hits them),
   but their allow-list lines stayed. Fixed: `prune.py --absent` drops allow-list lines whose key left the list;
   regen.sh runs it on the --replace path only (the train's `--both --allow-drop` never touches the hand-written
   file). Drops nothing on next's own lists (f58747b; test `regen_replace_drops_the_allow_list_lines_of_keys_it_drops`).
4. **Host input found: the order of names at one address followed the caller's locale.** trace_gdb.py wrote aliases
   (`bcmp`/`__memcmpeq`/`liella_machine_equal`, `__vfprintf_chk`/`liella_vfprintf4`) in nm's order, and nm sorts by
   locale. A plain jobrun on keke (LANG=en_US.UTF-8) gave startup-text.keys 7ec03f8a / probe-text.keys 76f63692 vs
   3242d4f0 / c8d5c8c0 under `~/bin/gate` (C): same keys, swapped neighbours. Fixed: nm with LC_ALL=C and a sort per
   address (1acfdbc; test `trace_writes_the_names_at_one_address_in_byte_order`). The -2 proof matched only because
   every proof job ran under `~/bin/gate`.
5. **Proof so far (fast `--replace --build`, tree2 = -2's head, all under ~/bin/gate):** Zen 1, Skylake-SP and (run 2)
   Haswell-EP: lists `fd7e54b5 3242d4f0 c8d5c8c0 2d993846`, libc.a 412c17ffe4abfda5, libc.so.6 f55cc779eb7a8c84, all
   identical. Fast regen time: 1019 s Zen 1 (loaded), 596 s Skylake-SP. `--both` was never green before item 3.

## Jobs started on the final head 1acfdbc (collect these)

- keke: `lthc3-lint3` (clippy all-features + static), `lthc3-gate3` (affected --run --base origin/next),
  `lthc3-plants3` (handoff/LAYOUT-TRACE-HOST-CANONICAL-3-plants.txt, 10 plants, --timeout 1500),
  `lthc3-proof2-keke` (proof3.sh bounds).
- mei: `lthc3-proof2-mei`; shiki: `lthc3-proof2-shiki` (`~/work/LAYOUT-TRACE-HOST-CANONICAL-3/proof3.sh`: tree3 =
  git archive of 1acfdbc at the same path on every host, plain jobrun, no gate wrapper, so the caller's locale is
  the host's: this is the test of item 4). Output `~/work/LAYOUT-TRACE-HOST-CANONICAL-3/out3/`; `proof:` lines in
  `~/logs/lthc3-proof2-<host>.log`; keke adds `bounds:` lines (both profiles: `cargo xtask so` contracts and
  `cargo test -p liella --test hot_text_products` with the canonical lists).

## Next step (exact)

1. Collect the six jobs. Compare `proof:` lines across the three hosts: fast lists + fast products, `--both` lists +
   `both fast/release products`. Expected fast lists = the run-2 digests above (C order). Any difference: diff
   `out3/<step>/` between hosts, then the names files under `target/product/regen/*.trace`.
2. `bounds:` lines green on keke = the brief's last proof item. A bound red with lists that only reorder: read
   the start-up layout jitter rule in the brief before acting.
3. Plants: all ten CAUGHT; list WARN lines (WARN-DEV-SHM is expected on a Zen 1 host).
4. Then update the pending record's body (add: tool locale, item 4; `--both` as the both-profile path and the
   `--replace` allow-list pruning, items 2-3; the proof numbers), the report json, and hand back COMPLETE. The record
   was left unchanged in run 3 so the hand-back head's content equals what the jobs tested.

Things not to redo: namespaces for data (nscache private table); `files`-only nsswitch or empty /usr/share/locale;
detecting syscall stops from gdb's printed output or from the pc; the one-profile release regen (#5241: use --both).
