# SCOPE-APPLY-STUBS handoff
State: branch task/scope-apply-stubs, code head 9a4ab5f. Decisions: D-29.3 (new), D-3.5 (unchanged). Rulings R6, R9.
Done: step/advance return 0 and write nothing (s14_search.rs; generated stubs and loc1/loc2/locs unchanged); malloc_get_state/set_state were already failing stubs (NULL/-1, ENOSYS): s3_3_c_4 now a table with errno and untouched-buffer checks. Differences row (clause s29_4_c_0), D-29.3, regexp-h.md note.
No COMPAT flag for R6 (row says why: obsolete, regcomp/regexec exist); not asked for by the ruling. Ask if wanted.
Gate (clippy, static, clippy --all-features, affected --run): scope-apply-stubs-gate2 rc 0 on shiki sh2. Plants: 4 CAUGHT (step/advance return 1, step writes loc1, set_state errno).
Next: none. Left over: spec/behaviour/regexp-h.md "Liella" body describes the deferred matcher (marked as such).
