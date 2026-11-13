# DLOPEN-INTERNAL-BITS

State: branch task/dlopen-internal-bits; docs-only change, no Liella code touched.

Done:
- spec/behaviour/dynlink.md row "P-1 unknown bits": the eight high bits are glibc-internal (maintainers, 2026-11-13), unsupported, refused like any unknown bit, excluded from the comparison. Existing raw `.glibc.txt` lines kept; none added.
- tools/probe/dl/dlopen/probe.c p1: sweep skips 0xff000000 (all eight) with a comment; the "0x80000000 alone" child is dropped.
- docs/differences-from-glibc.md: one row (no compat flag, owner ruling).
- records/pending/DLOPEN-INTERNAL-BITS-1.toml (D-27 series, id D-NEW).

Checked: no Rust differential test compares dlopen modes with the reference (only the probe does). Liella's refusal tests (crates/ld DL-16a sweep, conformance s27_dlopen_failures) are unchanged and still sweep every undefined bit.

Gate: clippy + static + `affected --run --base origin/next` green on shiki:sh1 (jobs dib-static2, dib-gate2). The first pair failed on bare `|` in the pending record; fixed in the second commit.
Not run: plants (no code), percall (no code), probe re-run on the reference (comment/skip change only; NOT VERIFIED beyond compile in the gate).

Next step: none; coordinator assigns the decision id.
