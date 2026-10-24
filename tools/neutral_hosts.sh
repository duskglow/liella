#!/bin/sh
# Replace the project's lab host names with neutral CPU-based roles in the files named.
# Mapping: keke -> Zen 1 host, sumire -> Skylake-SP host, shiki -> Haswell-EP host,
# nico -> Ivy Bridge host A, kotori -> Ivy Bridge host B, kanon -> Broadwell-EP host,
# nozomi -> AVX2 timing host. Used once on docs/saturation-register.md after a merge.
set -eu
for f in "$@"; do
  perl -pi -e '
    s/nozomi\/kanon/the AVX2 timing hosts/g;
    s/\bkeke Zen 1\b/Zen 1 host/g;
    s/\bkeke\b/Zen 1 host/g;
    s/\bsumire\b/Skylake-SP host/g;
    s/\bshiki\b/Haswell-EP host/g;
    s/\bnico\b/Ivy Bridge host A/g;
    s/\bkotori\b/Ivy Bridge host B/g;
    s/\bkanon\b/Broadwell-EP host/g;
    s/\bnozomi\b/AVX2 timing host/g;
  ' "$f"
done
