# LAYOUT-TRACE-DEFAULT: trace-at-build is the default of every product build

State: branch `task/layout-trace-default` (HEAD in handoff/reports/LAYOUT-TRACE-DEFAULT.json). Plan:
`plan/layout-rethink.md` section 4, rulings #5190-#5194; LT-1 is `handoff/LAYOUT-TRACE-AT-BUILD.md` (D-X.88).
Decision: `records/pending/LAYOUT-TRACE-DEFAULT-1.toml` (D-NEW, series D-X, amends D-X.88). Plants:
`handoff/LAYOUT-TRACE-DEFAULT-plants.txt`. First run stopped at the context checkpoint with its jobs running.

## What changed (code done, pushed)

* `Layout::parse(value, regen)` (xtask/src/hotlists.rs): unset / empty / `1` -> Traced (the default);
  `ungrouped` -> Ungrouped; `0` -> Committed (list tooling until LT-5); anything else an error. A regen build
  (`--hottext-regen`) with the switch unset or `0` reads the committed lists; with `1` / `ungrouped` it fails.
* `run_so_as` prints which layout it builds; trace messages say `trace-at-build` (no `=1`); the no-tracer
  message is kept and names the opt-out.
* `LIELLA_HOT_LISTS` + default trace: error naming `LIELLA_LAYOUT_TRACE=0`.
* `tools/hottext/regen.sh --build`: its check of the rewritten list builds with `LIELLA_LAYOUT_TRACE=0`.
* Tests: `layout_switch_defaults_to_the_trace_and_is_strict`, `regen_builds_never_rely_on_the_default_layout`
  (xtask), `init_products_are_grouped_from_a_trace_of_their_own_build` (hot_text_products: marker present,
  list hashes match, hot list and start-up keys non-empty).
* Decision on `0`: keep it as "the committed lists" for regen / repair / comparisons until LT-5 (refusing it
  would break regen.sh's rewritten-list check, which must build from the committed list).

## Tracer availability (brief step 2, checked 13:30Z with `gdb -nx -batch -ex 'python print(1)'`)

mei (Skylake-SP, 48 threads), keke (Zen 1, 60), shiki (Haswell-EP, 26), kanon (Broadwell-EP, 35): gdb 15.1
with Python, rc 0. nico, kotori, nozomi (timing hosts): no gdb; they have no compiler and never build products,
so nothing to install. `~/bin/gate` (keke) adds mount/IPC namespaces only; ptrace_scope 1 (gdb traces its own
child): the gate job below is the first traced build inside the gate.
`ops host-available` is a setter (host 0/1), not a lister: not run.

## Jobs started (not yet read)

Superseded: `layout-trace-default-lint2` (stopped: static's hottext check refused the literal regen flag in
hotlists.rs, found by handback-check; fixed in 4e3ac0709, comments and messages only),
`layout-trace-default-gate` (stopped while queued: it would have tested 97c1112e7), `-lint` / `-kproof` (stopped
while queued, before a resync). The role still holds 97c1112e7: sync it (rc 0, not 4) once kproof2 and the
plants end, then run lint (clippy + static + `cargo test -p xtask hotlists`) and the gate on the final head.

keke (role n3, head 97c1112e7, products identical to the final head):
* `layout-trace-default-kproof2`: `~/work/LAYOUT-TRACE-DEFAULT/proof.sh ~/w/n3 /dev/shm/claude/LAYOUT-TRACE-DEFAULT/out-n3`:
  default `so` + `layout-check` + `hot_text_products --nocapture` twice (each probe warm-up + 5 runs), fast then
  release; lists and libc.a kept in the out dir.
* `layout-trace-default-plants`: 5 plants, `--dir /dev/shm/claude/LAYOUT-TRACE-DEFAULT/plants-1`.
* `layout-trace-default-kbesl`: proof.sh on `/dev/shm/claude/LAYOUT-TRACE-DEFAULT/besl` = head cc19fd05e merged
  (git merge-tree, no branch changed) with `origin/task/libm-f32-bessel-large-5` (libm-only; -6 is not pushed).
mei (zram, no role): `layout-trace-default-mbase` (proof.sh on base = cc19fd05e, plus a second release `so` +
hot_text_products on the unchanged tree, timed, libc.a compared) and `layout-trace-default-mbesl` (the libm
merge), both under `/mnt/zram/claude/LAYOUT-TRACE-DEFAULT/` (equal-length paths; outputs in `out-base`, `out-besl`).
Products are the same at cc19fd05e and the final head (later commits: an xtask unit test and the plants file).

## Exact next step

1. `~/bin/jobwait` the jobs above (keke and mei); fix any red. Read the fault table:
   `grep -a "PROOF\|minor faults" ~/logs/layout-trace-default-<label>.log`.
2. Step 3: compare base vs besl per host and profile (fault lines, `cmp` libc.a, diff the four lists in out-*/).
3. Step 4 ratchet: every bound in hot_text_products.rs (TRUE 17, LOCALE 51, TOOL 56, FORK 6, LOCALTIME 20,
   LOCALTIME_MKTIME 20, FLOAT 5) and layout/contracts.tsv budgets (STARTUP-TEXT-WINDOWS 4/4,
   STARTUP-DATA-WINDOWS ro 1/1, rw 20/21) tightened to the highest value measured over mei and keke, both
   profiles; never raised. Bound history comments follow the file's style; add the numbers to the pending record.
4. Step 5: the release check for the CI gate, after the fast `affected --tier full --run` in the same tree:
   `LIELLA_FULL_PROFILE=1 cargo xtask so && LIELLA_FULL_PROFILE=1 cargo test -p liella --test hot_text_products`
   (the `so` runs layout-check's contracts itself). Run time on mei: from mbase's `release-again` lines (warm
   tree) and the first release lines (cold); note the fast products are replaced, so it runs last.
5. Rerun lint + gate + plants on the final head after the ratchet commit; hand back.

Do not redo: the code change, the host check, the plants file (unless a plant site moves).
