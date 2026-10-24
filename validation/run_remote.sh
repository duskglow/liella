#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Run validation suites on keke from the workstation, two at a time, each as its own queued job
# (~/bin/shared, one slot, under the 150-minute cap) (validation/README.md, milestone 6).
#
#   validation/run_remote.sh TAG PRODUCTS COMMIT PACKAGE:LIBC...
#
# TAG names the run's logs (~/validation/TAG-LIBC-PACKAGE.out); PRODUCTS is the directory of
# libc.a and liella-crt1.o on keke; COMMIT is recorded as the built-from line of each summary.
# Prints one line per finished job.
set -u
tag=$1 products=$2 commit=$3
shift 3
host=claude@10.0.100.53
printf '%s\n' "$@" | xargs -P2 -I{} sh -c '
    pkg=${1%%:*}; libc=${1#*:}
    ssh -o BatchMode=yes "$2" "~/bin/shared bash -lc \"cd ~/liella-test && SUITE_COMMIT=\\\"$4\\\" LIELLA_PRODUCTS=$3 bash validation/suite.sh $pkg $libc 6 > ~/validation/$5-$libc-$pkg.out 2>&1; echo $libc-$pkg rc=\\\$?\""
' _ {} "$host" "$products" "$commit" "$tag"
