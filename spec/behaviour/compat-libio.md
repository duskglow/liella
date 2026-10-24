# libio compat entries (§12.13, F32): black-box probes

IO-VTABLES, 2026-10-23. Probe: a C program built on keke against the host glibc 2.39
(Ubuntu 2.39-0ubuntu8.9), black box only.

**Clauses:** `s12_13_c_0`.

## `_IO_sungetc(FILE *)`

Same shape as `_IO_sungetwc` (already in the tree). On a file opened `"r"` holding `abc`, after
two `getc` calls (`a`, `b`): `_IO_sungetc` returns `98` (`b`) and the next `getc` returns `b`.
That is the contract Liella keeps: the last byte read comes back and is read again.

Liella's rule beyond that case (not taken from the reference): a byte counts as read only while
it is still in the current read window; with nothing read, on a writing, idle or wide stream,
the result is `EOF` and the stream is unchanged. Liella never moves the read position below
its window.

## `_IO_file_jumps`, `_IO_wfile_jumps`

`/proc/self/maps` shows both objects in a read-only mapping (`r--p`) in the probe process. The
manifest size is 168 bytes each (`spec/abi/versions.tsv`). Liella exports them as 168 zero
bytes in read-only data and never calls through them (R15-3).
