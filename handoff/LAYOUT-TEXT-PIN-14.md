# LAYOUT-TEXT-PIN-14: merged with next, lists regenerated, start-up +1 gone; static red on 29 unpinned keys (budget checkpoint)

**State (2026-11-13):** branch `task/layout-text-pin-14` = task/layout-text-pin-13 6bff2ffed + 9ae17f5f1 (merge of
origin/next 7b1d895b0) + c62331b46 (regenerated lists) + daef8fb39 (fork pre-pad test) + this note. Role kanon:~/w/kn1
(Broadwell-EP). Scripts on kanon in ~/work/LAYOUT-TEXT-PIN-14/: regen.sh (both one-profile regens, copies the lists to
out/, padmap head), padmap.sh (from -13, modes fork true tool locale, 17 pre pads), classify.py. Next reference built
on kanon: /mnt/zram/claude/LAYOUT-TEXT-PIN-14/next (git archive of 7b1d895b0), padmap in ~/work/LAYOUT-TEXT-PIN-14/next/.
Decisions in force: as -11/-12/-13 (D-X.63, D-X.56, D-X.47, D-X.49, D-4.53), owner ruling #5193 (dead keys warn).

## Done
1. Inherited head 6bff2ffed: clippy and static clean (job layout-text-pin-14-lint0, rc 0).
2. **Merge of origin/next** (9ae17f5f1), two conflicts. hottext.rs: next made a dead key a warning except under
   LIELLA_PROMOTE=1 (#5193, `DeadKeys`). The #4778 unreferenced-key check now follows the same modes through the
   new `unreferenced_verdict`: a warning by default, an error in the promotion run, a note in the regen build. The unit
   test now also covers 10e20c108's profile filter (a key listed for the other profile only; a bare key) and the three
   modes. hot_text_products.rs: comments combined, FORK_BOUND stays 6.
3. **Regen on the merged tree** (job layout-text-pin-14-regen, rc 0; c62331b46): `__libc_single_threaded` is in
   startup-data.keys. Padmap, head vs next on kanon (jobs -regen and -padmap-next, min of 3 runs):
   true equal at every pad (11 at 0 and 0x8000, 12 elsewhere); tool 50-51 vs 53-55; locale 46-48 vs 49-51;
   fork 5 everywhere except 6 at pre 0x2000-0x5000 (next: 6 there and at 0xd000-0xe000). **The start-up +1 is gone.**
4. daef8fb39: at the pre pads the fork child is held to FORK_BOUND only (class `fork pre`), not to the other pads'
   equal count (it reads 6 at 0x2000-0x5000 on next too; next does not measure fork at pre pads at all).
5. Plants file handoff/LAYOUT-TEXT-PIN-14-plants.txt (the -12 plants, ltp13-unref-profile-filter for 10e20c108, two
   for unreferenced_verdict, ltp14-startup-key-dropped). NOT RUN to completion (job stopped, see below).

## Open: static red (job layout-text-pin-14-lint, rc 1)
`cargo xtask static`'s hottext check (next's LAYOUT-HOTKEY-INLINE-STABLE `check_pinned`) refuses 29 keys the regen
added that name functions without `#[inline(never)]`: names/cache.rs Table::{init,put,bump,set_file,set_stamp_files,
file_in_change}, Payload::next; legacy nss.rs passwd, passwd_cached, group_line, group_cached; nscache.rs put,
config_epoch, lock_region, cached, account; boundary users.rs own (x2), patterns.rs passwd_home; core text.rs
builtin_charset, locale/name.rs candidates, locale/ctype.rs CtypeFile::wide, iconv Conv::to_words, getopt next,
system.rs find_line, names/nss.rs run; rt heap.rs Heap::locate_in; legacy intl.rs bind, textdomain.
hot-text.unpinned is a ratchet (rulings #4997, #5024: lines may only be removed), so they cannot go there.
The gate and plants were started on daef8fb39 and stopped (kill-label) once static was red: superseded.
Also: the fast rebuild warns that 13 listed keys are unreferenced in fast (mostly `-`/bare extra keys that the
per-profile regen keeps); a warning under #5193, not a failure.

## Next step (exact)
1. Regenerate with the path next now uses, `bash tools/hottext/regen.sh . --both --allow-drop` (ruling #5241; the
   -13 recipe of two `--build` runs predates it), as a jobrun job, and check whether it still adds the 29 keys.
2. For keys that stay: mark the function `#[inline(never)]` where a call boundary is free (hot_text_products and the
   affected tests catch a cost), else leave the key out of the list. Never add hot-text.unpinned lines.
3. Re-run padmap head vs next (kanon next copy above): true <= next at every pad, fork <= 6, tool/locale <= next.
4. clippy + static, then gate `cargo xtask affected --run --base origin/next` and plants
   handoff/LAYOUT-TEXT-PIN-14-plants.txt (check the ltp14-startup-key-dropped literal against the new startup-data.keys).

## Not to redo
The merge and its conflict resolution, the spot-check, the exec/run split, the -12 per-pad matrices, the next padmap.
