# The trace runner of tools/hottext/regen.sh and probe_text.sh (HOTLIST-REGEN-CHEAP), sourced by both.
# The caller sets `work` (the trace directory) and `here` (tools/hottext). HOTTEXT_JOBS caps the traces
# that run at once (default nproc); HOTTEXT_GDB replaces the tracer command (the runner's test in
# xtask/src/hottext.rs). Every trace writes its own files, so the order of the results never depends on
# which trace ends first.
hot_jobs_max=${HOTTEXT_JOBS:-$(nproc)}

run_traced() { # <names out> <log> <env assignment>... -- <program> <args...>: started in the background
    local out=$1 log=$2; shift 2
    local envs=()
    while [ "$1" != -- ]; do envs+=("$1"); shift; done
    shift
    while [ "$(jobs -rp | wc -l)" -ge "$hot_jobs_max" ]; do wait -n || true; done
    # shellcheck disable=SC2086
    ( rc=0; env "${envs[@]}" HOT_TEXT_OUT="$out" timeout 900 ${HOTTEXT_GDB:-gdb} -nx -batch -x "$here/trace_gdb.py" --args "$@" > "$log" 2>&1 || rc=$?; echo $rc > "$log.rc" ) &
}

trace_tagged() { # <tier> <env assignment>... -- <program> <args...>: run_traced into names.<n> (n: the
    # caller's count, in start order) and name it in <work>/tagged, which keys.py --tagged reads in that order
    local tier=$1; shift
    n=$((n + 1))
    printf '%s=%s\n' "$tier" "$work/names.$n" >> "$work/tagged"
    run_traced "$work/names.$n" "$work/gdb.$n.log" "$@"
}

traces_wait() { # wait for every trace started; fail (log tail on stderr) when one failed
    wait
    local r
    for r in "$work"/*.log.rc; do
        [ -e "$r" ] || continue
        [ "$(cat "$r")" = 0 ] || { echo "trace failed (rc $(cat "$r")): ${r%.rc}" >&2; tail -5 "${r%.rc}" >&2; return 1; }
    done
}

put_changed() { # <new file> <tracked file>: replace the tracked file only when the bytes differ
    if cmp -s "$1" "$2"; then rm -f "$1"; else mv "$1" "$2"; fi
}
