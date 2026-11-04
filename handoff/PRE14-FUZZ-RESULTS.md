# PRE14-FUZZ-RESULTS

State: branch task/pre14-fuzz-results. docs/fuzzing.md "Results" and "Findings" filled from keke's
`~/work/pre14-fuzz-campaign/run-20261023T065548Z-a` and `run-20261023T065839Z-b` (`results.tsv`, `*.stats`).

Facts: 20 targets x 30 min; 16 clean; time-outs on regex (762 s), regex_dfa (67 s), glob (367 s), locale (232 s).
Not recorded by the campaign: coverage (cov/ft is `?`), head sha. charmap's stats file has two stat blocks (peak RSS 258 and 253); 258 is shown.
Decisions: none. No jobs run (docs only; results were read, not rerun).
Not to redo: do not re-search the logs; they are on keke in ~/work/pre14-fuzz-campaign.
Next: fix stories for the four time-outs (artifacts in run-*-b/artifacts/); a rerun that records cov and head would close the coverage gap.

PRE14-FUZZ-RESULTS-2: removed the lab host name from docs/fuzzing.md (now "the lab VM", "7-core"); the CPU model was never recorded, so none is named. cargo is not installed on this agent host, so `cargo xtask static` was not run here; grep for the lab names finds none. CI precheck is the check.
