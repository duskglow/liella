# PLANTS-KILL-TREE

State: branch task/plants-kill-tree-2 (continues task/plants-kill-tree). run_logged (xtask/src/plants.rs) starts each step as leader of its own process group (timeout -k 10) and after the step ends sends TERM, then KILL, to the group (kill_group). Pending decision: records/pending/PLANTS-KILL-TREE-1.toml (D-X).
Test: a_timed_out_step_leaves_no_process_of_its_group (a TERM-ignoring grandchild outlives its leader; the test polls its PID in /proc, bounded). The test already observed the grandchild when run 2 started; run 1's SURVIVED was the older test.
Plant kill1 (drop kill_group) CAUGHT on the final head (pkt2-plants, shiki); its command is now `cargo test -p xtask --bin xtask a_timed_out_step`, so the products were not rebuilt (baseline reused). Results: handoff/PLANTS-KILL-TREE-plants.txt.
Limit: a process that leaves the group (setsid) is not reached by a group kill; not tested.
Gate: pkt2-lint (clippy, static) rc 0; pkt2-gate (affected --run --base origin/next).
Earlier jobs pkt-plants (rc 1, old test) and pkt-lint, pkt-gate, pkt-lint2, pkt-plants2 are superseded.
Next: none.
