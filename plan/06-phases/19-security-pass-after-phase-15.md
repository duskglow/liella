<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Security pass (after phase 15, before release preparation)

Owner requirement (2026-10-03). Like the optimization pass, a systematic review of the finished
library rather than a per-feature one.

* **Threat model:** `spec/security/threat-model.md` states, for each trust boundary, the attacker,
  what they control, and what they must not be able to achieve. Boundaries: the name-service
  cache region (D-16.10); the secure allocator and zeroing (D-X.11); setuid/`AT_SECURE` handling;
  credential changes; the C layer and the boundary crate; filename, pattern and word expansion
  (`glob`, `wordexp`, `fnmatch`, regex); parsers of system files and network data; the dynamic
  loader; environment-variable handling.
* **Review:** each boundary against its threat model; every finding gets a test, or a documented
  limitation with its reason.
* **Fuzzing:** every parser of untrusted input: `nsswitch.conf`, `resolv.conf`, the hosts and
  passwd files, the shared cache region, DNS responses, regex compilation, printf/scanf formats,
  locale and iconv data, glob and wordexp patterns. Report corpus size, crashes found and fixes.
* **Independence:** part of the review by a fresh agent given only the spec, the threat model
  and the code, not the implementation history.
* **Decisions** go into this plan. Implementation-level fixes are decided and reported; changes
  to a trust boundary, a security default or a public interface go to the owner first, with a
  recommendation (only that change is held; the rest continues).
* **Exit:** every boundary reviewed, every finding tested or documented, the fuzzing report
  written.

---
