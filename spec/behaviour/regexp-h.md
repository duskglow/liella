# `<regexp.h>`: `step`, `advance`, `loc1`, `loc2`, `locs` (compat, `GLIBC_2.2.5`)

Plan row LG-10 (`REGEXP-H`). Black-box probes against glibc 2.39 (Ubuntu 24.04, x86_64): a C program
that `regcomp`s an expression into a `regex_t` and calls `step`/`advance` with `expbuf` at chosen
offsets, as PIE and non-PIE executables (both behave the same). The probe's printed lines are the
values below; there is no separate `.glibc.txt` record (the values here are the record).

## What is observed

- **`expbuf`.** The `regex_t` is read at the first 8-aligned byte AFTER `expbuf[0]`. `expbuf` =
  `re-1` .. `re-8` all work; `expbuf == re` and `re-9` or lower crash. Bytes before the `regex_t`
  are ignored.
- **`step(string, expbuf)`.** Leftmost match: returns 1, `loc1` = start, `loc2` = end. No match
  returns 0 and leaves both unchanged.
- **End of subject.** `$` never matches at the end of the subject (`REG_NOTEOL`): `c$` on `abc` is
  0, `c$` on `abc\n` is 0; `bc` on `abc` is 1 with 1..3; `b$` compiled `REG_NEWLINE` on `ab\nc` is
  1 with 1..2. `^` works. ERE and `REG_ICASE` compile flags are honoured.
- **Empty matches.** Empty pattern on `abc`: 1, 0..0. `x*` on the empty string: 1, 0..0.
- **`advance(string, expbuf)`.** The match must start at `string`; it sets only `loc2` (`loc1` is
  untouched): `abc` on `abcyy` is 1 with `loc2` 3; `abc` on `xabc` is 0; `x*abc` on `xxabcyy` is 1
  with `loc2` 5; `abc$` on `abc` is 0.
- **`locs`.** Never read or changed, whatever its value (set to 0, 2, 3 offsets: no effect).
- **`loc1`, `loc2`, `locs`** are exported data objects (`GLIBC_2.2.5`, 8 bytes). A non-PIE
  executable gets `R_X86_64_COPY` relocations for all three, so the library's writes must go to the
  object the dynamic linker binds, not to a private copy.

## Liella

`step`/`advance` are compat-only (`xtask/src/compat.rs`: `BEHAVIOUR`, `DATA`, `LOADS`). The generated
stubs load the addresses of `loc1`/`loc2` through the GOT and pass them to
`liella_compat_step`/`liella_compat_advance` (`crates/boundary/src/posix/s14_search.rs`), which run
`regexec` with `REG_NOTEOL`.

Known gap: an expression compiled `REG_NOSUB` makes glibc's `step` return 1 with `loc1` = the real
start; Liella leaves `loc1` unchanged there (the engine reports no offsets) and sets `loc2` to the
subject's start. The old `compile()` flags are unknown; no program is known to depend on it.
`advance` runs an unanchored `regexec` and rejects a match that does not start at `string`, so its
cost grows with the subject when nothing matches at its start.

**Clauses:** `s29_4_c_0` (§29.4: the removed `<regexp.h>` interfaces keep the contract they had; the observed values above are that contract).
