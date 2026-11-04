# VALIDATION-ROWS-STALE: NOT DONE (blocked on the keke job queue)
State: branch task/validation-rows-stale, no edit to validation/README.md yet.
Job `validation-rows-stale-so` (cargo xtask so on keke:~/w/r4, output ~/work/vrs-so.txt) was queued at 2026-11-04T17:15:37Z and
was still QUEUED at 18:52Z (jobrun log: "QUEUED ... for a slot"); it never started, so no nm/readelf evidence exists.
Rows to refresh once it runs (validation/README.md, lines ~467 T23, ~477 T34; T24 stays as is):
- T23: nm libc.a / libm.a for frexp ldexp modf scalbn finite isinf isnan (+ __, f, l forms), __signbit*, __fpu_control.
- T34: nm libc.a for fmod (expect absent from libc.a, present in libm.a); cite PRE14-LIBC-FMOD / LIBC-A-PLACEMENT.
- Other rows naming a merge/story: line ~423-431 "After V-5" item 0/1 and the provisional-results section mention Track M's libm
  merge and the modf/gawk stand-ins; check against the tree before rewriting. Unsettled rows: "rechecked 2026-11-04: unchanged".
Next step: when the job finishes, `nm -A target/liella/libc.a | grep ...` on keke:~/w/r4, edit, `cargo xtask static`.
