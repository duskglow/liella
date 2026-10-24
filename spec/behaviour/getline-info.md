# _IO_getline and _IO_getline_info (item 9)

**Spec:** §12.13 (each libio entry point has the contract of the obvious public function).
These two read a line up to a delimiter into a caller's buffer:

* `size_t _IO_getline(FILE *fp, char *buf, size_t n, int delim, int extract)`;
* `size_t _IO_getline_info(FILE *fp, char *buf, size_t n, int delim, int extract, int *eof)`.

**Clauses:** `s12_13_c_0`.

No installed header declares them; the probe declares them with these prototypes.

**Probe:** `crates/core/tests/oracle/stdio_diff.c`, function `libio`. **Record:**
`stdio.glibc.txt`, section `== libio`.

## What the probe does

The file holds `A`, 70 dashes, then `\nline one\nline two;rest\nlast`. The `getline_info` lines
read from offset 71 (the first `\n`) with `extract` 1, 0 and -1. For each `extract` there are four
reads in turn: a 128-byte buffer with delimiter `\n`; a 4-byte buffer with `\n`; a 128-byte buffer
with `;`; a 128-byte buffer with `\n`. A line shows `extract`, the count returned, the buffer's
bytes up to one past the count (it was filled with `Z` first), the `eof` value (7 before the
call) and the next byte (`_IO_peekc_locked`). The `getline` line is one `_IO_getline` call from
offset 71 with `extract` 1.

The same section also covers `_IO_putc`, `_IO_padn`, `_IO_seekpos`, `_IO_getc`,
`_IO_peekc_locked`, `_IO_sgetn`, `_IO_seekoff`, `_IO_feof`, `_IO_ferror`, the list iterator and
`_IO_flush_all`. Those are not part of item 9.
