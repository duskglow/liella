# COMPAT-SCANF-ERANGE handoff

State: branch `task/compat-scanf-erange` (cut from origin/next, stacked on SCANF-FLOAT-ERANGE), records commit 5eb19eb0d.

Done (ruling #4661):
- `records/compat/COMPAT-21.toml`: status `registered`, `decisions = ["D-12.26"]`, behaviour naming `s12_7_c_3`;
  `restore` = a start-up tunable that puts back the entry `errno` when a later directive meets end of input after a
  floating range error; `cost` = a flag test (and keeping the entry `errno`) on every scan that can end there, on the
  default path, so not implemented now.
- `docs/differences-from-glibc.md`, the D-12.26 row: "No compatibility option" became "Compatibility option COMPAT-21
  registered, not implemented: ...".
- No code. No generated view committed (the train regenerates docs/compat-flags.md).

Jobs (mei, role s3): `compat-scanf-erange-static` (records check + `cargo xtask static`) rc 0:
"records: ok: 505 decision, 21 compat, 87 saturation records (0 pending)".

INFORMATIONAL: the D-12.26 decision record body still says "No compatibility option ... (maintainer ruling requested in
the story's hand-back)". The brief limited the change to the compat record and the row, so it is untouched; a one-line
correction (name COMPAT-21) could go with the next scanf story.

Next step: none for this story; implementing COMPAT-21 only if a flag test turns out free on the default path.
