# LAB-NAMES-CLEAN-3 (branch task/lab-names-clean-3)
State: origin/next merged. Conflicts: register edits moved into docs/saturation/13-next-perf-stories-from-the-10.md (index kept as next's);
regex_subpatterns.rs and plan/next-batch-1024.md took next's wording with host names replaced by processor classes; xtask static list
now has both "register" and "lab-names". New host mentions that arrived with next were renamed (last commit); mapping used:
keke=Zen 1 host, shiki=Haswell-EP host, sumire=Skylake-SP host (guess; check docs/plants.md line 14).
Gate: lab-names-clean-3-gate (old head, rc 0, 41 min); lint (red: lab-names hits, fixed); lint2 green (register --check, static, clippy).
Final-head gate lab-names-clean-3-gate2 started detached (the last commit only changed comments/docs).
Next step: dispatcher reads gate2 from shiki jobs.tsv.

## LAB-NAMES-CLEAN-4 (branch task/lab-names-clean-4)
origin/next was already merged (bad2e7ae). Two new lab-name hits from next fixed: strto_internal_group.rs:8 (host name dropped),
spec/behaviour/time-gaps.md:13 (keke -> "Zen 1 host"). Jobs on mei: lab-names-clean-4-lint (static + clippy, rc 0), lab-names-clean-4-gate (direct tier).
