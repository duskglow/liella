# Compatibility flags (registry)

Liella deliberately differs from glibc 2.39 in a few observable behaviours (the full list of
differences is in `differences-from-glibc.md`). Owner philosophy (2026-10-10): where a program
might depend on glibc's behaviour, even wrongly, give it a way to get that behaviour back when
that costs nothing on the default path. This file registers each such behaviour and how a
flag would restore it. A row here is a promise to design the flag, not an implemented flag.

Mechanism (a design note; COMPAT-12 used it until it was withdrawn): `GLIBC_TUNABLES` is already parsed once at startup
(`runtime.rs`, `liella::config::Tunables`) and `cpu::init_with` already picks code paths from it
once per process. A compat flag would be one more tunable, read there and stored in the same
read-mostly startup state. Whether a flag is free depends on the row: where the restored
behaviour is a different function, the export would need a resolved-once pointer or a branch
on startup state, which is NOT free on a hot call; such a row names a cheaper route (for
example a separate preloadable object) or stays registered only. COMPAT-12 was the first
implemented flag (the tunable `liella.compat.rpc_reply_any_source`); it is withdrawn, because the
RPC client it applied to is now a failing stub (D-29.5), and the tunable is no longer read.

