# PRE14-LIBC-FMOD (branch task/pre14-libc-fmod)

State: libc.a and libm.a already had the right split on `next` (LIBC-A-PLACEMENT, 2026-10-23: `localize` step in
`xtask/src/so.rs` and `check_placement`, run at the end of `cargo xtask so`). T34 was stale on `next`.
Verified on keke from `cargo xtask so`: libc.a defines no libm.so.6-only name except the 28 glibc's libc.a also defines
(`LIBC_A_LIBM_NAMES`: copysignf128, frexpf32, ... the `*f32/f64/f128` helpers); libc.so.6 exports none; libm.a has no
libc.so.6 name.
How they got in (earlier): libc's math crates export fmod etc. with C names into libc.a; the build localized them in the
archive and routes libc's own calls to private `liella_libc_<name>` copies.

Change here: `check_placement` now also checks the shared objects (`nm -D`): libc.so.6 must export no libm.so.6-only name,
libm.so.6 no libc.so.6-only name (`dynamic_names` helper). Passes on the current build.

gawk 5.4.1 (static, scripts/liella-cc, job pre14-libc-fmod-gawk): `checking for fmod... yes`, `LIBS='-lm '`, Makefile
`LIBS = -lm`, links, and sin/exp/log/atan2/% run. (Configure + build + smoke only; the 594-test suite not re-run.)
Plant (job pre14-libc-fmod-plants2): planted copy that stops localizing libm-only names -> `cargo xtask so` fails with
"libc.a defines fmod/trunc/... which glibc's libc.a does not": CAUGHT. No plant for the new .so check (it passes on all
real builds; a plant would need a differently-linked libc.so.6): not planted, say so.
clippy --workspace --all-targets --all-features -D warnings: clean; `cargo xtask static`: rc 0 (keke n1).
Not done: update validation/README.md T34 row to "fixed" (next re-run of validation).
Next step: land.
