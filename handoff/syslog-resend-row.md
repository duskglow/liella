# SYSLOG-RESEND-ROW(-2) handoff

State: branch task/syslog-resend-row-2. Docs only: one differences row (stream /dev/log, peer closes mid-record: Liella resends the cut record whole on the new connection, §26.1 retry-once; test s26_1_c_2 `syslog_stream_reader_closing_mid_record`) and one compat-flags row.
Decisions: D-26.1 (#2052). The compat id is the placeholder COMPAT-NEW in both rows: next already has COMPAT-11 twice (freeres, fnmatch), so the coordinator assigns the number before landing (replace in both files).
Ported from the historic branch by hand (the two doc rows), not merged; the other historic commits were only handoff/report files.
Not to redo: the test already exists on next and pins the behaviour.
Next step: coordinator assigns the COMPAT id; CI gate.
