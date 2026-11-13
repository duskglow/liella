# DLOPEN-INTERNAL-BITS (and -2)

State: branch task/dlopen-internal-bits-2 (continues task/dlopen-internal-bits); docs and one comment, no Liella behaviour change.

Done (run 1):
- spec/behaviour/dynlink.md row "P-1 unknown bits": the eight high bits `0x01000000`-`0x80000000` are glibc-internal (maintainers, 2026-11-13), refused like any unknown bit, excluded from the comparison. Raw `.glibc.txt` lines kept.
- tools/probe/dl/dlopen/probe.c p1: sweep skips 0xff000000; the "0x80000000 alone" child is dropped.
- docs/differences-from-glibc.md: one row (no compat flag, owner ruling).
- records/pending/DLOPEN-INTERNAL-BITS-1.toml (D-27 series, id D-NEW; the coordinator assigns the id).

Done (run 2, owner amendment 2026-11-13): the eight bits are RESERVED FOR LIELLA. Liella owes glibc's internal meaning nothing, refuses them with `invalid mode parameter`, keeps them available for a possible future Liella-defined use (none planned; any use is a new public-interface decision). Said in the dynlink.md row, the differences row, the pending decision record, and a doc comment at `ModeError::Unknown` in crates/ld/src/dlopen.rs naming the range.

Checked: clippy and static green on the inherited head; no Rust differential test compares dlopen modes with the reference.
Not run: plants (comment and docs only), percall, probe re-run on the reference.

Next step: none.
