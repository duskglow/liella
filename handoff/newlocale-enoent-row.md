# NEWLOCALE-ENOENT-ROW (docs; branch task/newlocale-enoent-row)

State: differences row for `newlocale`'s ENOENT on an unknown locale added to `docs/differences-from-glibc.md`
(ruling #5424, clause `s7_3_f_0`, probe rows in `spec/behaviour/newlocale-mask.md` / `.glibc.txt`, base already has
NEWLOCALE-LC-ALL-BIT). Two pending records: `records/pending/NEWLOCALE-ENOENT-ROW-1.toml` (`D-NEW`, series D-10) and
`-2.toml` (`COMPAT-NEW`, registered, not implemented; its `decisions` names D-7.17 because a pending D-NEW cannot be
referenced yet). The row cites `COMPAT-NEW`; the coordinator assigns both ids and replaces the placeholders in the row and
in the D-NEW body.

Open: the row sits among the syslog rows of the Behaviour table (cosmetic; move it if wanted). The compat tunable name is
to be chosen when designed.
Not to redo: the probe rows (owned by NEWLOCALE-LC-ALL-BIT). No code changed, so no plants.
Next: coordinator assigns ids.
