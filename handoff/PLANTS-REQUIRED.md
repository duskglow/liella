# PLANTS-REQUIRED
Docs, CONTRIBUTING, CLAUDE.md, PRINCIPLES and the libm template now point at cargo xtask plants. Default timeout 600. rc 124 is CAUGHT by timeout (was WARN-NO-MSG). New tests: default_timeout_is_600_seconds, a_plant_that_times_out_is_caught. Plants in handoff/plants/PLANTS-REQUIRED.txt: 2 CAUGHT.
Note: validation/tmpfs-run-plants.sh and validation/wl_stats_plants.sh are older hand scripts, left in place with a note. Running two jobrun commands in one ssh call blocks on the first; start them separately.
