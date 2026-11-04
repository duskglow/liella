# FMA-DID-COMMENTS handoff
Changed the FMA-build comments from D-10.22 to D-10.23 in 8 files: dd.rs, fast64.rs (x3), config.rs, cpu.rs, dispatch.rs, hot.rs, selftest.rs (x2), tests/math_fma_paths.rs. The sqrt row's references were left alone. Comment-only, so there are no plants.
Gate (`cargo xtask static` + `affected --run`) on shiki: rc=0, label fma-did-comments-gate.
Not done: the decision-row order swap that the earlier handoff mentioned, because the brief says "nothing else".
