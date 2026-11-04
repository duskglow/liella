# DL-THREAT-TEXT (phase 14 DL-0d): loader threat-model rules and environment additions folded in

**State.** Run 1 on `task/dl-threat-text` (docs 30974b1b, 81f978db). Run 2 (DL-THREAT-TEXT-2) on
`task/dl-threat-text-2`, cut from run 1's head 5c773a20. `origin/next` merged first (already up to
date). Run 2's docs commit is 9f6aa513, then this handoff/report commit. Planning-type change: no
loader code.

**Done (run 1).** `spec/security/threat-model.md`, "The dynamic loader": rules 1-12 in checkable
form (draft sections B and C, owner approval R14-15, #2549), with the DL-0a probe answers
(`spec/behaviour/dynlink-secure.md`), "Still open" paragraph, the DL-29 checklist table. Rule 10
(executable stack refused) is a deliberate difference with a differences row and a compat row.

**Done (run 2, owner rulings 2026-11-04).**
- #2661: draft section A folded into "Environment variables in secure-execution mode": the loader
  applies the same `rt::start::UNSECURE` list at bootstrap (libc pass then a no-op); secure mode
  is `AT_SECURE` only in the products; `GLIBC_TUNABLES` row says the variable is not passed on and
  no tunable is honoured by loader or libc; the `TZ` row now says: name or POSIX rule honoured,
  a path only inside `/usr/share/zoneinfo`, a path outside it or any `..` component reads no file
  and gives offset 0; new row `LD_ASSUME_KERNEL`, `LD_PROFILE_OUTPUT`, `LD_TRACE_LOADED_OBJECTS`
  (kept, the loader does not act on them in secure mode); closing sentence on loader variables.
  The "Still open" paragraph and the Status line now say section A is approved and folded in.
- #2660: both `COMPAT-NEW` placeholders (compat-flags and differences rows) and the threat-model
  rule 10 text now say COMPAT-19.
- #2662: the COMPAT-19 row says the future flag is ignored in a secure process, as COMPAT-12.

**Open questions.** Rule 1's set-ID bit for `LD_PRELOAD`/`LD_AUDIT` entries is still Open (it needs
a root probe on the owner's say-so). `LD_USE_LOAD_BIAS` stays in the "kept, not read" row because
section A does not list it; the loader story that reads it adds its row.

**Not to redo.** The DL-0a answers and the execstack probe (`tools/probe/execstack/`, run 1) are in
the text. The compat id and the secure-process rule are settled.

**Next step.** Land. The `TZ` rule is now a spec requirement for §23.6 zone selection in a secure
process: the coordinator has the implementation note (sent privately with the hand-back). DL-29
should add a checklist row for the `TZ` rule (tests `s23_6_c_0`, `s23_6_c_2` in secure mode).
DL-8 may start once this lands.

**Plan rows to read.** R14-13, R14-15 (`plan/ph14/`), `plan/ph14/threat-model-draft.md`.

**Jobs (run 2, kanon kn1).** `dl-threat-text-2-static`: `cargo xtask static`, then
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, on 9f6aa513. Result:
see `handoff/reports/DL-THREAT-TEXT-2.json`. No gate, plants or counts: docs only, no code.
Run 1's jobs (keke r1) cover run 1's head only.
