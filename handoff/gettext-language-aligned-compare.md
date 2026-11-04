# GETTEXT-LANGUAGE-ALIGNED-COMPARE (branch task/gettext-language-aligned-compare, code head 16a7668e)
State: implemented, NOT yet verified. Baseline count (job gettext-language-aligned-compare-before, rc 0): steady row 778.7 i vs glibc 452.6 (1.72); the
strlen+compare share is about 55 of 780 (7%, over the 3% bar; STEADY-2 profile: strlen 9.1% for two calls).
Change: boundary strz::text_equals_words (one UNSAFE(slice) site, ruling 2409), Host::{language_probe,text_at,text_equals} (defaults keep other hosts),
Slot::list_text + Key.list_checked in legacy intl.rs: translate compares the LANGUAGE text with R_LAST slot's stored list word for word; matches skips the list compare then.
Tests: crates/boundary/tests/text_equals_words.rs (len 0..64, 16 alignments, page-end before unmapped page, NUL-look-alike bytes): passed on an earlier head.
Plants (earlier head, job ...-plants rc 0): read-one-byte-past CAUGHT (fault), skip-terminator CAUGHT, no-low-mask CAUGHT, wrong-shift CAUGHT.
Superseded: ...-t1 (killed queued), ...-t2 (test file not synced), ...-t3 (clippy red as-casts), ...-gate and ...-after (stale head; killed), ...-plants (older head).
Final job ...-final runs on the final head in order: clippy, test, static, plants, affected gate, after-count (-> ~/work/gettext-language-aligned-compare/fe.txt, after.txt).
NOT DONE (out of calls): register row in docs/saturation/14 with the after count; read after.txt and add the row.
Also: one pending decision: whether the two unsafe blocks in text_equals_words (read + from_raw_parts) count as one site for the audit.

## Run 2 (GETTEXT-LANGUAGE-ALIGNED-COMPARE-2)
Clippy red cause: identity_op (0x00 + 0x80) in tests/text_equals_words.rs; fixed. Job gettext-language-aligned-compare-2-final (keke role n3): clippy, test, static, plants (4 CAUGHT), affected gate, after-count all ran, rc 0.
RESULT: after-count 862.9 i (1.91 vs glibc 452.6); paired base at merge-base (job ...-2-basecount, rc 0) 731.6 i. The change is +131 instructions SLOWER. Register row written (docs/saturation/14, "run 3").
Recommendation: do not land the code; if retried, design a closure-free short-value kernel and measure instructions first.
Process note: I used sed (not -i) on remote scratch scripts and one `sed -i` on a scratch plants.sh copy on the host (not repo files).
