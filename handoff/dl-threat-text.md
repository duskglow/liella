# DL-THREAT-TEXT (phase 14 DL-0d): loader threat-model rules folded in

**State.** Branch `task/dl-threat-text` from `origin/next` c3424c99. Docs commit 30974b1b, then this
handoff/report commit. Planning story: no loader code.

**Done.**
- `spec/security/threat-model.md`, "The dynamic loader": rules 1-3 and 5-7 in the checkable form of
  `plan/ph14/threat-model-draft.md` section B, rules 9-12 from section C (owner approval R14-15,
  ruling #2549, 2026-11-04), rules 4 and 8 kept, each with the DL-0a probe answers
  (`spec/behaviour/dynlink-secure.md`). The old open-questions paragraph became "Still open"; the
  old test paragraph became the DL-29 checklist table (rule -> test it needs, all 12 rules).
- Rule 1's open point (set-user-ID / set-group-ID bit admitting an `LD_PRELOAD`/`LD_AUDIT` entry)
  is marked **Open**, needs a root probe on the owner's say-so; not guessed.
- Rule 10 (executable-stack object refused) is a deliberate difference: row in
  `docs/differences-from-glibc.md` ("Deliberate user-facing incompatibilities") and a registered row
  in `docs/compat-flags.md`.
- Rule 4 text kept, plus one sentence "For a `PT_GNU_STACK` that asks for an executable stack, rule
  10 applies" (its "`PT_GNU_STACK` honoured" would otherwise contradict rule 10).

**Probe settled here** (`tools/probe/execstack/`, `run.sh DIR` builds `es.c` in DIR; run on the
keke role host, Ubuntu GLIBC 2.39-0ubuntu8.9): library and program linked with `-z execstack`
(GNU_STACK RWE checked with readelf). `dlopen("./libx.so")` succeeds; before it the main stack and a
thread stack are `rw-p`, after it both are `rwxp`. A `-z noexecstack` library changes nothing. A
program linked `-z execstack` starts with `rwxp` main and thread stacks.

**RULING NEEDED.**
1. Compat id: the brief assigns COMPAT-18, but on `origin/next` COMPAT-18 is already the `fnmatch`
   `FNM_EXTMATCH`+`FNM_LEADING_DIR` row (the static check rejects a duplicate). The new row carries
   `COMPAT-NEW` (compat-flags row and differences row). Recommend the coordinator assign COMPAT-19
   (next free) and replace both placeholders.
2. Section A of the draft (environment-section additions: `TZ` treatment, `GLIBC_TUNABLES` removal,
   the loader's new variables in the table) was not in the approval (B and C only), so it is not
   folded in; the `TZ` row still says "to be reviewed with the loader". Recommend approving A as
   drafted: the DL-0a probe shows the reference does exactly what A proposes, no differences row.
3. The compat row recommends that the future flag be ignored in a secure process (as COMPAT-12).

**Not to redo.** The DL-0a probe answers are already in the rules; the execstack probe output is above.

**Next step.** Coordinator: number the compat id, then land. DL-29 uses the checklist table; DL-8 may
start once this lands.

**Jobs.** `dl-threat-text-static` (cargo xtask static on keke r1, via jobrun). No gate, plants or
counts: docs only.
