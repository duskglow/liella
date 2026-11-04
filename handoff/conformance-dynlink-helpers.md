# CONFORMANCE-DYNLINK-HELPERS
State: branch task/conformance-dynlink-helpers, code head 58f34899 (shiki:~/w/sh1).
Done: LINK-HELPERS directive in conformance/run.sh (helpers in conformance/c/helpers/<n>.c built as .so, program linked
dynamically with --no-as-needed, run with LD_LIBRARY_PATH, reference only, Liella reported "SKIPPED ... for Liella"),
test s27_dynlink_helper_dlsym + helper dlh_add, conformance/README.md, affected-selection rules for the README and helper
sources (they escalated to FULL before).
Gate: conformance-dynlink-helpers-gate2 (so, clippy, static, affected): 181 passed 0 failed, conformance 116 passed, rc 0.
Plants (conformance-dynlink-helpers-plants): drop helper from link, no LD_LIBRARY_PATH, no --no-as-needed, helper not shared: all CAUGHT.
No decisions/compat/differences rows needed (harness only). Not redone: nothing.
Next: phase 14 dlfcn clause tests can declare LINK-HELPERS; when Liella has ld.so, drop the reference-only branch.
Superseded: conformance-dynlink-helpers-gate (escalated to FULL on README/helper paths; killed).
