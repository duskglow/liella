# SCOPE-REVIEW-LEGACY handoff (2026-11-04)

State: branch `task/scope-review-legacy`, planning only, no code changes.
Deliverable: `plan/scope-review-legacy.md`, a table of 34 families, rulings R0 to R14, and standing-check text.
Evidence: job `scope-review-legacy-usage` on mei. It read the dynamic imports of 2,867 ELFs and mapped them with
`dpkg -S`. The script and its output are in the scratchpad at `work/SCOPE-REVIEW-LEGACY/` (legacy_users.py,
names.txt, usage.tsv), and on mei in `~/work/scope-review-legacy/`.
Gate: `scope-review-legacy-gate` on mei:~/w/s2 (clippy, static, affected).
Decisions in force: LEGACY-RPC-NIS-STUBS (RPC/NIS stubbed). Everything else here is RULING NEEDED.
Do not redo: I skipped Debian Code Search because its results include glibc source.
Next step: the owner rules on R0 to R14. Then re-scope phase 15 stories LG-8, LG-10, LG-12 to LG-18, LG-2 and
LG-3, and add the standing-check text to the brief template and CONTRIBUTING.md.
Plan rows to read: plan/06-phases/18-phase-15-legacy-tier-and.md (LG list), R15-3, R15-8.
