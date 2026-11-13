# SETLOCALE-POSIX-ATTR: handoff

State: branch `task/setlocale-posix-attr` (from origin/next a5636ce6b), head in
handoff/reports/SETLOCALE-POSIX-ATTR.json. Build role mei:~/w/s2 (Skylake-SP). Scratch on mei:
`~/work/SETLOCALE-POSIX-ATTR/spa.c` (percall spec: setlocale_ctype_posix, _ctype_c, _all_c, _all_posix, _query,
_ctype_empty).

## Cause (attributed)

- `"POSIX"` selects the built-in locale that reports itself as `"C"`. The lock-free no-change path of
  `global_locale::setlocale` compared the requested name AS GIVEN with the cached reported name, so
  `setlocale(cat, "POSIX")` never matched `"C"`: every call took the lock, ran `replace_changed`/`replace`, compared
  the result with the interned locales and copied `Locale` values.
- wl_trace of call 11 on next a5636ce6b (`setlocale-posix-attr-trace-head`): 2490 instructions; memmove family
  (`machine_move_avx2` 565, `conv::internal_move` 242, `memmove` 180, `hot::mem::internal_large` 77, `move_mid` 60,
  `internal_memmove` 17) about 1140; `Locale::eq` 278 + `machine_equal` 210; `replace` 424; setlocale 176.
- Why the address matters: the memmove kernels' instruction count depends on the alignment of the source and
  destination (stack buffers), so the slow path's count moved with unrelated layout changes. On a Zen 1 host that
  showed as +170 and sd 7-8 after LAYOUT-HOTKEY-INLINE-STABLE; on mei the same pair goes the other way (pre-merge next
  ee529a28d 2517.8, next a5636ce6b 2488.8, sd 2.5). Nothing in that story touched the path: it moved the stack/data
  layout under a path that copied large values.
- The brief's base 38e278e6f does not build in the release profile on mei (`xtask so`: dead hot-text key
  `stdio_compat::with_list<Glue, flush_all closure>`), so the attribution used ee529a28d (next just before the
  LAYOUT-HOTKEY-INLINE-STABLE merge) as the base (`setlocale-posix-attr-percall-base` red for that reason; superseded).

## Fix

- `liella::locale::reported(name)`: `"POSIX"` -> `"C"`, else the name. The fast path matches `s == n ||
  s == reported(n)` (as given first, so `"C"` costs nothing extra; the first version measured +2 on the C rows).
  `replace_changed` is unchanged (a first call only; changing it shifted inlining of two fast-profile hot-text keys).
- Survives layout growth: the repeated call no longer runs any copy or compare whose cost follows addresses.
- Decision: records/pending/SETLOCALE-POSIX-ATTR-1.toml (D-NEW, series D-X; the coordinator assigns the id).
- Test: crates/boundary/tests/setlocale_posix_repeat.rs (self-counting static program; "POSIX" again at most
  "C" again + 25% + 8/call, LC_CTYPE and LC_ALL; also reports "C" with MB_CUR_MAX 1 after C.UTF-8).
  Plants: handoff/SETLOCALE-POSIX-ATTR-plants.txt.

## Results

See the report json (percall, gate, plants labels and numbers).

## Exact next step

Coordinator: assign the D-X id; land. Optional follow-up: `setlocale(LC_CTYPE, "")` repeated is 8260 vs glibc 2551
(re-resolves from the environment every call); not in this story's scope.
