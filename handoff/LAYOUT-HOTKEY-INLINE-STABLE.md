# LAYOUT-HOTKEY-INLINE-STABLE (-3): handoff

State: branch `task/layout-hotkey-inline-stable-3` (base next d1b3a596c, not merged), head in
handoff/reports/LAYOUT-HOTKEY-INLINE-STABLE-3.json. Build role keke:~/w/n1 (Zen 1). Scratch scripts on keke:
`~/work/LAYOUT-HOTKEY-INLINE-STABLE-3/` (probe-job.sh, faultdiag.sh, faultrun.sh, dummy-bessel.rs, fmap-*/syms-*/sec-* outputs).
-1/-2 history: the -2 version of this file (on `task/layout-hotkey-inline-stable-2`).

## Decisions in force

- Rulings #4997, #5024 (the check covers every tier; ratchet), #5025 (record pending:
  `records/pending/LAYOUT-HOTKEY-INLINE-STABLE-3-1.toml`, D-X series, id to be assigned by the coordinator).
- `cargo xtask static` (`hottext`, `check_pinned`): every Rust key of EVERY tier names an `#[inline(never)]` workspace
  function or has a line `<key>\t<tier>\t<reason>` in `crates/libc-so/hot-text.unpinned`; a stale line (boundary
  gained, key gone, tier changed) fails. One file with a tier column (not one per tier): one check, one stale rule,
  and a key changing tier shows as one edited line that has to be justified again.
- Pinned in -3: `locale::builtin`, `nscache::real_secs`, `iconv::load` (the keys a dummy libm growth killed).
- hot-text.unpinned: 30 start-up lines from -1/-2, plus 41 closure/other-crate lines and 298 lines for the newly
  checked tiers (stdio 26, locale 42, tool 50, extra 180, generics included): plain functions say "boundary cost
  not measured yet", generics give the generic reason. The ratchet's starting point, not settled reasons (RULING below).

## Done in -3 (evidence, keke logs `~/logs/<label>.log`)

- `lhis3-lint0` rc 0: clippy + static on the inherited head (green).
- #5026 root cause (`lhis3-fd-base`, `lhis3-fd-head`, `lhis3-fr-rel`, `lhis3-fr-fast`: perf page-fault map of
  the `tool` probe, tools/hottext/faultmap.py): release base 52 / head 53 recorded faults, the only difference is
  ONE `.text` page: `liella_sys_fstat` (stdio tier, hot-text.list position 104, called for the locale archive and
  stdout). The text VMA starts at 0x408000, so the kernel's first fault-around window is 0x408000..0x417fff (clipped
  to the VMA start, then 16 pages). Base puts fstat at 0x417e00 (inside), head at 0x418040 (0x240 bytes of start-up
  growth from -2's pins: one page past the window), so head takes a fault at 0x418000. NOT a data object (the -2
  note's "data phases" were the test's labels; the extra page is text at every phase). Fast: base = head (51).
  strace -k: both call fstat through `liella_sys_fstat` (same syscalls).
- Check extended + tier column + unpinned lines + pins + pending record (commits 8be97fb77, 69f19a54d).
  `lhis3-lint1` rc 0 (xtask hottext tests 19 pass, clippy -D warnings, static), `lhis3-lint2` rc 0 (records check,
  static) on the committed tree.
- Dummy growth on the new code (`lhis3-probe-headdummy` rc 0, head + dummy-bessel.rs): fast AND release `so` pass
  (no dead key in any tier), hot_text_products 23/23 in both. Tool probe: fast 52, release 54 (bound 56).

## Exact next step

1. #5026 (must be fixed before landing, ALWAYS FORWARD): give back the 0x240+ bytes of the release start-up text that
   pushed `liella_sys_fstat` out of the first window, or place fstat inside a window the tool run already touches.
   The cliff is structural (whatever sits at 0x418000 jitters): the robust fix is to pin the first window's end, e.g.
   the start-up group sized/aligned so the keys first touched by every probe end before VMA start + 64 KiB, with a
   test that `liella_sys_fstat` (and the start-up group) lie below it. Check the fault map with faultrun.sh at each
   phase, both profiles; the counts must equal base (release tool 53 in the relay harness, fast 51/52).
2. Counts for the 3 new pins (setlocale rows for `locale::builtin`, NSS cache rows for `real_secs`, iconv rows for
   `iconv::load`): `cargo xtask bench --compare --counters <bench>`; revert any pin that costs.
3. Then the direct gate, plants (check_pinned tier rules: stale tier line, extended tier) via `cargo xtask plants`,
   hot_text_products per phase on the final head.

## Open / RULING NEEDED

- The 241 plain locale/tool/extra/stdio functions in the ratchet with "boundary cost not measured yet": accept as the
  ratchet's start (recommended: pin them in batches later, each with counts), or require counts for each before landing.
- Pinning `iconv::load` adds a call per iconv() call: counts pending (step 2).
