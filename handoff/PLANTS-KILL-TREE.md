# PLANTS-KILL-TREE

State: branch task/plants-kill-tree. run_logged (xtask/src/plants.rs) starts each step as leader of its own process group (timeout -k 10) and after the step ends sends TERM, then KILL, to the group (kill_group). Pending decision: records/pending/PLANTS-KILL-TREE-1.toml (D-X).
Test: a_timed_out_step_leaves_no_process_of_its_group (a TERM-ignoring grandchild outlives its leader). Plant kill1 (drop kill_group) CAUGHT; results in handoff/PLANTS-KILL-TREE-plants.txt.
Limit: a process that leaves the group (setsid, or gdb's own inferior group) is not reached by a group kill; not tested.
Gate: pkt-lint2 (clippy, static, affected --run --base origin/next) rc 0. Not verified: a real traced so on a Skylake host.
Next: none.
