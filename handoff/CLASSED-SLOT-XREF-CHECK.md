# CLASSED-SLOT-XREF-CHECK handoff (one page)

**State:** branch `task/classed-slot-xref-check` from origin/next `957f58bd`. Code commits `8d89bf08` (the check, wiring,
D-X.34, plants), `5f410dcf` (clippy type alias). Role keke:~/w/n1 (Zen 1 host).
**Decisions in force:** D-X.34 (new: the XREF bias is checked statically), D-4.53 (classed pages).

**What was done**
* `xtask/src/classed.rs`: pairs the n-th out-of-page reference of page 1 with the n-th of page 0 (one macro expands both
  pages, so the order is the same) and requires the same instruction and symbol and page 1's target = page 0's + 4096.
  An unpaired reference, a relocation outside the two pages, or nothing to check is an error (never a skip). Objects and
  archives: `readelf` relocations of the section defining `liella_classed_page0` (sorted by offset; readelf groups
  them by type). Linked objects: `objdump -d` of the two pages from `liella_classed_page0` (RIP-relative `# addr`
  comments and direct branch targets). Each mismatch names both offsets and both targets.
* Runs in `cargo xtask static` (assembles `csys/src/machine/fold.S` with `-DLIELLA_EXPORT -DLIELLA_RUNTIME`, no
  product needed), at the end of `cargo xtask so` on `target/liella/libc.a` and `libc.so.6`, and as
  `cargo xtask classed-xref [<file>...]`.
* Probe-settled detail: GAS writes page 0's `jz sym` as R_X86_64_PLT32 and page 1's `jz sym+4096` as R_X86_64_PC32
  (seen in libc.a: all 7 page 0 jumps PLT32, every page 1 relocation PC32 with addend 0xffc/0xffb); the two compare as
  one kind.
* On origin/next's tree (job `classed-slot-xref-check-probe1`, keke): fold.o, libc.a and libc.so.6 each 11 pairs, all
  following the rule. Unit tests `classed::tests` (synthetic two-page object, as .o and linked .so: correct pair,
  biasless load, biasless jump, unpaired reference; the readers' parsing; the empty case) pass.
* Plants (`handoff/classed-slot-infra-plants.txt`): `xref-bias-set-mid` is a real plant again (command
  `cargo xtask classed-xref`, `equivalent:` dropped); `xref-bias-fold-limit` (run 1's biasless fold-limit read, only
  mentioned in a comment before) added as a real plant; six plants on the check's own code against its unit tests.

**Results:** see handoff/CLASSED-SLOT-XREF-CHECK-report.md (gate and plants on `5f410dcf`).

**Not to redo:** the layout study above; `classed-slot-xref-check-gate` (rc 1, clippy type_complexity, fixed in
`5f410dcf`) and `classed-slot-xref-check-plants` (stopped: older head) are superseded.

**Open questions:** none. The pairing assumes both pages make their out-of-page references in the same order, which
holds while one macro expands both bodies; a later slot whose pages differ in out-of-page references would fail the
check by design and needs a pairing by slot (D-X.34 says so through "unpaired reference is an error").

**Exact next step:** none for this story. Read: D-X.34, D-4.53.
