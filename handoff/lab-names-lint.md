# LAB-NAMES-LINT handoff

State: branch task/lab-names-lint, code head 50c1bd8. New static check lab-names (xtask/src/labnames.rs, run by cargo xtask static and as cargo xtask lab-names).

- Scans every tracked file outside handoff/ for the 13 lab host names (whole words, case-insensitive) and 10.0.100.x / 10.0.0.x.
- Exemptions: ALLOW (16 files, reasons in source: tool host maps, host-keyed table, the check itself, the pending list), lines marked LAB-ONLY, file names (never checked).
- Ratchet: xtask/lab-names-pending.tsv lists 92 other files with 1174 hits as path and max count. A file may only lose hits; a new file or a higher count fails.
- Not done: the prose cleanup. About 1900 hits in 107 files is far over a 30-call budget (bench/*.tsv alone is ~1000 lines). Follow-up LAB-NAMES-CLEAN: reword each file to the processor class, delete its line from the pending list, keep going until the list is empty.
- Tests (cargo test -p xtask labnames): planted name, planted address, longer words, allowlisted path/marker/handoff. Plants: 11 of 11 CAUGHT on plants2.
- Not verified: the tracked-file list was only exercised in walk mode (role tree has no .git); the git ls-files path is the same code feeding the same scan.
