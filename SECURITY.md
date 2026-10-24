# Security policy

Liella is a C library meant to sit underneath other programs, so memory safety and predictable
behaviour on hostile input matter. It is public as a work in progress and has not had a
security audit (see [README.md](README.md)).

## Supported versions

Only the `main` branch is supported. There are no maintained release branches.

## Reporting a vulnerability

Please report privately, not in a public issue or pull request. Use GitHub private
vulnerability reporting on this repository (the "Report a vulnerability" button under the
Security tab).

Include the function or component, the input, the Liella commit and, if you can, a minimal C
program. Please do not paste glibc source into a report.

## What to expect

You should get an acknowledgement. Liella is a soft release maintained at the owner's pace, so
there is no promised response or fix timeline. Please keep the report private until a fix is
available or we have agreed otherwise.

## Scope

In scope: Liella's own code (the Rust crates, the C layer in `csys/`, the headers) and its build
and test tooling (`xtask`, `tools/`, `scripts/`), for example memory-safety bugs, crashes or
unbounded resource use on crafted input, and `unsafe` code that breaks its stated contract.

Not in scope here: anything that looks like an issue in GNU glibc itself. Please send that to
the glibc project's own security process, described at
<https://sourceware.org/glibc/security.html>, not to Liella's tracker.

If code Liella already has may share the weakness, that part is in scope here: report it to us
privately as above, however minor it looks, and do not open a public fix first. We coordinate
the fix and its timing, and nothing public names the glibc weakness until glibc's fix is
public. Where Liella does not have the affected functionality yet, implementing it without the
weakness, worded neutrally, is enough. See [docs/PRINCIPLES.md](docs/PRINCIPLES.md) section 7.

Some tests and fuzzers deliberately feed malformed input to the library; a crash that a test
documents as expected is not a vulnerability.
