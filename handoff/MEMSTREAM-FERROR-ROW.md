# MEMSTREAM-FERROR-ROW (ported by MEMSTREAM-FERROR-ROW-2)

State: branch task/memstream-ferror-row-2, stacked on PERROR-HERROR-REFERENCE-6. Docs plus one test comment.
- Differences row for the open_memstream growth failure reworded (POSIX rule, gain, ruling #2066, pinning test cited).
- Compat flag registered as COMPAT-NEW (placeholder: COMPAT-11 is now taken twice on next; the coordinator assigns the number before landing).
- Test comment in crates/core/tests/enomem_stream_grow.rs names the flag.
Verified: direct gate rc 0, clippy, static, s12_stdio_symbols (sym_perror) 355 passed.
Not this story's part: the sym_perror failure in trains 234/236 (this change touches no perror code; comment-only in code). Not verified against the train logs.
No plants: no product code changed.
Next step: coordinator replaces COMPAT-NEW with a free id in compat-flags.md, differences row and the test comment.
