# REGISTER-SPLIT handoff

State: branch task/register-split. Done: docs/saturation-register.md is now an index; sections are docs/saturation/NN-slug.md (00 = intro and sources, 1-15 the old sections, text unchanged).
Verified by script that the files concatenated equal the old body (ignoring blank lines; 368 = 368 non-blank lines). `cargo xtask register --check` (xtask/src/register.rs) is wired into `cargo xtask static`: every file listed, numbers unique and contiguous from 0, `## N.` heading matches the file prefix, row ids (first cell of `Row` tables, 64 today) unique across files.
Links: CLAUDE.md, plan/next-batch-1024.md, plan/06-phases/20-..., CONTRIBUTING.md (performance section sentence). Remaining plain mentions of the index file (plan/next-batch-1024-2.md, tools/neutral_hosts.sh, phase-17) name a file that still exists.
Not changed: the intro text still has the old "Numbering" note (content unchanged by brief); a later story may reword it.
Gate: `cargo xtask affected --base origin/next --run` escalates to FULL because xtask changed; running as register-split-gate-2 on mei (static and workspace clippy passed first in the same job). Plants register-split-plants-2: 4 CAUGHT (unlisted, dupnumber, gap, duprow).
Next: none beyond reading the gate result.
