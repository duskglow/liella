# REGISTER-REFS
Branch task/register-refs, code head 852a8977. Done: `cargo xtask register --check` (part of `static`) now walks the tree (skipping target/, hidden dirs, docs/saturation/, spec/, *.tsv, the functional spec) and fails with file:line when "register N.M", "register §N" or "register section N" cites a section above the highest existing one. Unit test `current_and_stale_references` plants a stale reference; plants job plants a file citing section 99 and the check catches it with file:line.
Found: no stale "register 13.5"-style references on this branch (the plan/ citations of §7, §8, §11, §12 match current sections); the stale ones were on another branch. Nothing to renumber.
Limits: a citation split across lines, or "13.5 #8" without the word register, is not seen; a number that exists but is the wrong section is not detected.
Note: any xtask change makes `affected` escalate to FULL, so the gate was clippy + `cargo test -p xtask register` + `cargo xtask static` instead (register-refs-gate3).
Next: none.
