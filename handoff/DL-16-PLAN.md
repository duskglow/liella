# DL-16-PLAN (+ DL-16-PLAN-2): handoff

**State:** branch `task/dl-16-plan-2` (stacked on `task/dl-16-plan`, cut from origin/next `d1b3a596c`). Work commit
`f386ce300` on top of the plan's WIP commits; this note and `handoff/reports/DL-16-PLAN-2.json` on top. Planning
only, no product code. Planning is COMPLETE: the code stories can be cut.

**Deliverable:** `plan/ph14/dl-16-dlopen.md` (§1-§7 the plan, §6 the story table and probes P-1..P-13, §8 the story
briefs' "The change" sections: DL-16P, DL-16H, DL-16a, DL-16b, DL-16c, DL-16d, and the later DL-16S). Phase-14 item
20 (`plan/06-phases/17-phase-14-dynamic-linking.md`) points at it.

**Decisions in force:** D-27.1, D-27.2, D-27.11, D-27.15..D-27.20, D-27.22; R14-4, R14-5, R14-11, R14-15; #2607;
coordinator rulings #5005 (static `dlopen` deferred to DL-16S, after DL-17/DL-20; a differences row; a registered
compat placeholder) and #5006 (unknown handles refused with a dlerror text by one lookup, never a scan; one
differences row shared with DL-17; no compat flag).

**Pending records (ids to assign, CI holds the branch until then):** `records/pending/DL-16-PLAN-1.toml` (chain,
writer lock, publication point), `-2` (a failed `dlopen` leaves no trace), `-3` (handles and counts, #5006), `-4`
(static programs deferred, #5005), all series D-27; `-5` the registered compat placeholder (COMPAT-NEW). On
assignment, also update the references: the plan (intro, §2 `dlsym`, §7 static programs, §8 DL-16S), the two new
rows at the end of the Behaviour table of `docs/differences-from-glibc.md`, and `-5`'s `decisions`, `story` and `date` (left
out on purpose; story and date go together, and a date needs `decisions`: `records check` resolves `decisions` only against records that have ids, so `-5`'s body names `-4`).

**Jobs:** shiki sh1: `dl-16-plan-2-lint0` (inherited head `b74a3e77f`, clippy + static, rc 0);
`dl-16-plan-2-static` (final head: clippy, static, records check).

**Open questions:** none for DL-16. DL-16S decides its own design (probe the reference first).

**Do not redo:** the reading (design §5-§11, rulings, both folded probe notes, `spec/behaviour/dynlink.md`), the plan
text, the records.

**Next step:** the coordinator assigns the five ids and fixes the references above; then cut DL-16P now (parallel to
DL-14), DL-16H and DL-16a after DL-14, then DL-16b and DL-16c, DL-16d after DL-15; DL-16S after DL-17 and DL-20.
Briefs: §8 of the plan.
