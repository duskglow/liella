#!/bin/sh
# WCTYPE-TRIE-BUILTIN: new test, then the direct-tier gate, then the after counts (one build at a time in the role).
set -u
~/bin/jobrun wctype-trie-builtin-test check -- sh -c '. ~/.cargo/env; cd ~/w/n1 && cargo test -p liella --lib builtin_class_tables_equal_wclasses && cargo test -p liella --lib view_equals_the_per_call_reads' || exit 1
~/bin/jobrun wctype-trie-builtin-gate gate -- sh -c '. ~/.cargo/env; ~/bin/gate 16G sh -c "cd ~/w/n1 && cargo xtask affected --run --base origin/next"'
~/bin/jobrun wctype-trie-builtin-count-after count -- sh ~/work/WCTYPE-TRIE-BUILTIN/count.sh after
