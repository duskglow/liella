# RELEASE-HANDOFF-SCRUB (handoff, 2026-11-07)

**State.** Branch `task/release-handoff-scrub` (base origin/next 9643c81b0); the head commit is this note's commit.
Decision in force: **D-X.45** (plan/07-decisions/x-implementation-decisions.md).

## What was done
1. The scan was `git grep -nE '10\.0\.[0-9]+\.[0-9]+|/home/[a-z]+|C:[/\\]Users'` over the whole tree. Every lab address and every
   personal home path is now gone. Changes per file (the count is the number of replacements):
   - handoff/: 20 files, 24 replacements. Home paths became `~/...`. Addresses became host names (keke, shiki, nico, nozomi).
     lab-names-clean.md now names the 10/8 test fixtures without full addresses (2). proto_percall.c now includes
     `proto_exp.c` relative to its own directory, or through `-DPROTO_EXP=...` (1).
   - validation/corpus/printf.corpus: 78 home paths of the harvesting account became `~`. A harvested 7-letter user name (4) became `someone`,
     which has the same length. No line was added or removed, so the line numbers in printf.known still hold.
   - validation/results/: 6 files, 21 replacements (home paths became `~`; no measurement changed).
   - xtask/src/remap.rs (2), repro.rs (3): the test paths became `/srv/build/w/...`. The relative lengths are kept, so each
     assertion keeps its meaning.
   - The tool scripts validation/wl_gate.sh, run_remote.sh, wl_alloctrace.sh, wl_fallback.sh, wl_install.sh and
     tools/probe/remote.sh no longer hold addresses. The new helper `validation/lab_host.sh` provides `lab_addr <name>`,
     which looks the address up at run time in this order: `LIELLA_HOST_<NAME>`, then `~/.config/liella/lab-hosts`,
     then the ops checkout's vm_ssh.sh, then the bare name. It was tested locally for all 8 host names and the override.
     The scripts were checked with `bash -n`.
   - Test fixture homes (`/home/alice`, `/home/h`, `/home/cbench`, ...) and resolver test addresses in 10/8 (such as
     `10.0.1.0` and `010.0.0.1`) are not personal or lab data, so they were left as they are.
2. A new static check, `private-paths` (xtask/src/privpaths.rs, registered in `cargo xtask static`), scans every
   tracked file, handoff/ included. It fails on a lab-subnet address (`10.0.100.x` or `10.0.0.x`, the same rule as
   lab-names), on `/home/<name>`, and on `<drive>:/Users`. The one documented placeholder form is `/home/<fixture>`
   with the name in `PLACEHOLDERS`. In a role tree without .git, both lab-names and private-paths now skip `.changed*`,
   `.synced*` and `fuzz/target`; before this change lab-names failed on `.changed.xtask`. The tests in labnames.rs build
   their addresses at run time, so labnames.rs passes the new check.
3. Plants: handoff/release-handoff-scrub.plants. All 4 were CAUGHT, each by the test it names.

## Jobs (keke, role n4)
- rhs-static-3: clippy, `cargo test -p xtask --bin xtask -- privpaths labnames` (8 passed) and `cargo xtask static`.
  All green, rc 0.
- rhs-plants: 4 CAUGHT, 0 SURVIVED, 0 WARN. The log is `~/logs/rhs-plants.log`.
- Superseded runs: rhs-static (array length compile error), rhs-static-2 (the walk read sync files).

## Open / next
- The coordinator lands the branch on next before the snapshot. No ruling is needed (D-X.45 is a local tooling decision).
- INFORMATIONAL: the scripts above still work on this workstation because `lab_addr` falls back to the ops checkout. On
  another machine, set `LIELLA_HOST_KEKE` (or similar) or create `~/.config/liella/lab-hosts`.
- Not redone: the host-name scrub outside handoff/ (lab-names, D-row of LAB-NAMES-CLEAN) was already in force.
