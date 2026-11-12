# DL-16-PLAN: handoff (WIP, context checkpoint)

**State:** branch `task/dl-16-plan` (cut from origin/next `d1b3a596c`). One WIP commit: `plan/ph14/dl-16-dlopen.md`
(the DL-16 plan: sections 1-8 written, including the six story briefs' "The change" sections) and this note. No
product code.

**Decisions in force:** D-27.1, D-27.2, D-27.11, D-27.15..D-27.20, D-27.22; R14-4, R14-5, R14-11, R14-15; #2607.

**Not done (resume, about 12 calls):**
1. Write the three pending records the plan names (`cargo xtask records new decision D-NEW --series D-27 --story
   DL-16-PLAN`, or by hand in the schema's key order): -1 object list/lock/one publication point before constructors;
   -2 a failed dlopen leaves no trace (two-pass relocation, no foreign code before the last failure point, adds/subs
   as probed); -3 handles (validation of unknown handles, shared differences row with DL-17's dlclose(NULL); `opens`
   saturates and pins). Then `cargo xtask records check`.
2. Fix two clause mentions in the briefs (section 8): DL-16a's proof adds `s27_1_c_5` (dlerror); DL-16d's proof drops
   `s27_1_c_4` (dlclose, DL-17's). The table in section 6 already has both fixes.
3. Point item 20 of `plan/06-phases/17-phase-14-dynamic-linking.md` at the plan (one line).
4. Sync to shiki sh1, run the static job (`cargo xtask static` + `records check`), `ops handback-check`.

**RULING NEEDED (proposed in the plan):** dlopen in static programs (`libc.a`) is out of DL-16 (recommend: defer;
the decision is public); unknown-handle validation in dlsym/dlvsym (recommend: refuse with a dlerror text, one
differences row shared with DL-17).

**Do not redo:** the reading (design §5-§11, rulings, both folded probe notes, `spec/behaviour/dynlink.md`).
