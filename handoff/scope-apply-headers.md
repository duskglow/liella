# SCOPE-APPLY-HEADERS handoff (stopped for a ruling; no code change)

State: branch `task/scope-apply-headers`, cut from `origin/next` at f3b3584. The only commit is this handoff and
`handoff/reports/SCOPE-APPLY-HEADERS.json`. No product, test or doc file changed: `cargo xtask so` output and the
ABI manifest are unchanged by construction.

## Why nothing was applied

The brief asks Liella's headers to stop declaring `gets`, `getpw`, `getwd`, `sigstack` (and any `_IO_*` or
malloc-hook declarations), to add a COMPAT flag that brings the declarations back, and to put a deprecation attribute
on `tmpnam`/`tmpnam_r`. Liella does not ship any C library headers:

* `include/` holds only `include/liella/native.h`, the native ABI header (D-X.10, `cargo xtask native-gen`). It
  declares none of these names.
* `csys/include/*.h` are internal to the C layer (syscall numbers, wrapper declarations), not installed.
* C programs build against the **system headers**: plan/04-test-methodology.md ("C conformance: real C programs,
  compiled against system headers and statically linked to `libc.a`"), docs/interfaces.md layer 1 ("the system
  headers"), plan/01-ground-rules.md and OD-1 (installed headers are read only as ABI data, distilled into
  Appendix C). Nothing in xtask passes `-nostdinc`, `-isystem` or an include directory of Liella's to a C build.

So no Liella header declares these names, and none can stop declaring them. R0's "Liella's headers do not declare it"
assumes a header set that does not exist. Building one would be a new public interface and a large new scope, so the
task stopped at the ruling instead (CLAUDE.md rule 1: when the spec cannot decide, raise the gap).

## RULING NEEDED

1. **Where COMPAT-ONLY lives while Liella has no headers.** Options:
   (a) Defer R0/R1/R4/R7/R8's header part, and R2's warning, until Liella ships its own C headers (a separate
       story that needs its own ruling). Until then the names are exported exactly as they are now. There is no
       difference from glibc, so no differences row and no COMPAT id are needed yet. **Recommended**: it costs
       nothing and changes no interface.
   (b) Ship an opt-in lint header now (for example `include/liella/legacy-guard.h`, used with `-include`) that marks
       the COMPAT-ONLY names with `#pragma GCC poison` and adds `__attribute__((deprecated("use mkstemp or
       tmpfile")))` to `tmpnam`/`tmpnam_r` by redeclaring them. It is opt-in, so the ruling's default ("not declared")
       is reversed. It is also a new public interface.
   (c) A link-time warning for `tmpnam`/`tmpnam_r` (and the COMPAT-ONLY names) from `.gnu.warning.<symbol>`
       sections in `libc.a`/`libc.so.6`, a mechanism documented in GNU binutils. It needs no headers, but it changes
       what users see at link time, so a black-box probe of the reference's link output should come first. It also
       needs a differences row if the outputs differ.
2. **The flag's name and kind** if (b) is chosen. It would be a compile-time macro (for example
   `_LIELLA_COMPAT_LEGACY_DECLS`), not a `GLIBC_TUNABLES` entry, and would take the next free id, COMPAT-13.

## INFORMATIONAL

* `docs/compat-flags.md` uses **COMPAT-11 twice** (`__libc_freeres` and `fnmatch` extmatch). CLAUDE.md says "a
  static check rejects a duplicate id", but no such check exists in `xtask/src` (no compat-flags or duplicate-id
  code found). That needs a renumber and a check, as a separate story.
* The brief cites `plan/scope-review-legacy.md`. It is not in this tree (`origin/next` at f3b3584) or in liella-ops.
  The owner rulings section in `state/PH14-OWNER-RULINGS.md` was read instead.

## Do not redo

* Do not search for Liella's stdio/unistd/signal/malloc headers: there are none (checked with `find . -name '*.h'`
  outside `target/`).

## Next step

Wait for the owner's ruling on item 1. Under (a), close the story as deferred and attach the header part of R0/R1/R2/
R4/R7/R8 to a future "Liella C headers" story. Under (b) or (c), re-brief with the chosen mechanism and flag name.

Plan rows to read: OD-1 (plan/09), plan/04-test-methodology.md (C conformance row), D-X.10 (native header).
