# RECORD-D10-95-BODY

State: branch task/record-d10-95-body, code commit 616268efc (records only), report commit follows.

Done:
- records/decisions/D-10/D-10.95.toml: the last sentence ("raise no overflow, underflow or deliberate inexact
  flag today ... a separate change") is kept as history, past tense, followed by "Superseded by D-8.12
  (STRTO-FENV-FLAGS, #4856) for that sentence", with the current behaviour in one sentence.
- records/decisions/D-8/D-8.12.toml: `amends = ["D-10.95"]` (design/records.md section 3: the link lives on the
  amending record; the view then appends "(Amended by D-8.12.)" to D-10.95's row).
- Searched all records/ for the same claim (strto/wcsto/scanf records, "no overflow/underflow/inexact"): only D-10.95
  made it. docs/differences-from-glibc.md lines 348/361 are libm rows, not strto*.

Decisions in force: D-8.12 (current strto* flag behaviour), D-10.95 (history).
Open questions: none. Not redone: views are train-owned and untouched; `cargo xtask records-gen` rendered all 38
views under target/records-views/ (1 stale = s10, which the train regenerates; its D-10.95 row shows both notes).
Not edited: the sentence in handoff/reports/LIBM-INVALID-CONVERT-SCAN-2.json (a frozen report).
Next step: land. Nothing further.
