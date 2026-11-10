# HOTTEXT-ISOLATE-64K (INCOMPLETE, budget)

Branch task/hottext-isolate-64k, decision-row head c7be174a8 (code head 8beaf85fa). Role kanon:~/w/kn1. Brief: ~/state/briefs/HOTTEXT-ISOLATE-64K.md.

## Done
- xtask/src/hottext.rs: empty anchor section .text.sorted.h0 (align 65536, SHF_GNU_RETAIN, both written in the section header by xtask) added to the libc.a member holding list position 0; removed from any other member; idempotent.
- xtask/src/layout.rs: kind group-anchor (one member, empty, align >= 65536, flags AX+R, sorts before every .text.sorted.*); row HOT-GROUP-ANCHOR in layout/contracts.tsv; D-X.51 row (verdict pending).
- Probe (GNU ld 2.42, toy link): unretained empty section is dropped by --gc-sections; retained one survives, sorts first, aligns the group, keeps nothing else alive. objcopy 2.42 has no retain flag and ignored --set-section-alignment on the section it added (the new contract caught alignment 1 on the first build).
- Second anchor (start-up tier) NOT built: it could cost a fault for stdio programs (start-up tier last window no longer shared); measure separately.

## Base numbers (ddc2dbdaf, kanon, job hottext-isolate-64k-measure-base2), fast / release
- minor faults: startup 18/17, setlocale+printf 57/58, tool 60/61, fork child 7/6.
- probe hot group start 0x40dbc0 / 0x413280 (mod 64K 0xdbc0 / 0x3280), 10 windows each; start-up tier 2 / 1 windows.
- probe file 5598312 / 4290504, text 4282971 / 3650131; libc.so.6 file 5639856 / 5102232, text 4005537 / 3774693.

## NOT VERIFIED / next step
- Change numbers: job hottext-isolate-64k-measure-8beaf85 (second run, same label as a failed first run; tree synced with the D-X.51 row uncommitted, same content as c7be174a8) = clippy + static + measure.sh. Plants hottext-isolate-64k-plants-8beaf85, count hottext-isolate-64k-wlc-8beaf85 (base ddc2dbdaf; seq printf-loop du wc sed-s cut-tr-uniq gawk-sum diff make-n tar-tv). Scripts: kanon ~/work/HOTTEXT-ISOLATE-64K/measure.sh, groupmap.py.
- Next: read those logs; keep only if no row has more faults or instructions; write sizes into D-X.51 (or revert + NEGATIVE saturation row); decide C3 for libc.so.6 from its __libc_start_main address mod 64K and LOAD p_align; rerun measure on the final head with a fresh label; ops handback-check (not run).
- Process: one python heredoc used (slip); the base job first ran with a wrong package name (-p liella-core), cost one rerun.
