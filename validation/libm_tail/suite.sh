#!/bin/bash
# LAB-ONLY: the LIBM-TAIL-PROBE measurement set (no spec clause), in groups that run as separate jobs:
#   bash validation/libm_tail/suite.sh <group> <liella-tree> <outdir>
#   x32a x32b x32c  every f32 input of the unary f32 functions (batches of 1024 consecutive inputs)
#   s32             per-input f32 sample (one input per 1024) for percentiles; f32 binary 2-D samples
#   f64             f64 unary (64 sub-blocks per binade, 256 inputs each, batched; 4 per input), hard points, binary
#   analyze         validation/libm_tail/analyze.py over <outdir> -> <outdir>/report-*.tsv
# Every group resumes (run.sh skips finished specs).
set -u
g=$1 tree=$2 out=$3
here=$(cd "$(dirname "$0")" && pwd)
F32A="lgammaf acosf acoshf asinf asinhf atanf atanhf cbrtf cosf sqrtf"
F32B="tgammaf coshf erff erfcf expf exp10f exp2f expm1f logf log10f"
F32C="j0f j1f y0f y1f log1pf log2f sinf sinhf tanf tanhf sincosf"
F32BIN="atan2f powf hypotf fmodf remainderf"
F64="acos acosh asin asinh atan atanh cbrt cos cosh erf erfc exp exp10 exp2 expm1 j0 j1 lgamma log log10 log1p log2
     sin sinh tan tanh tgamma y0 y1 sqrt sincos"
F64BIN="atan2 pow hypot fmod remainder"
# Hard points (f64, mode n64): zeros, poles, overflow thresholds, large trig arguments.
declare -A P=(
    [lgamma]="1/2/-2.4570247382208006/-2.7476826467276127/-3.1435473364170004/-3.9552942845622999/-4.0393618397405361/-4.9915446405600472/-5.0082181689591050/-1/-2/-3/-10/-20/-100.5/0/2.5e305"
    [tgamma]="-1/-2/-3/-10/-20/-100.5/-170.5/-184/0/171.6243769563027/1/2"
    [sin]="1.5707963267948966/3.141592653589793/1e22/7.5e292/1e300/6.2831853071795862/314159.26535897932"
    [cos]="1.5707963267948966/3.141592653589793/1e22/7.5e292/1e300/6.2831853071795862/314159.26535897932"
    [tan]="1.5707963267948966/3.141592653589793/1e22/7.5e292/1e300/6.2831853071795862/314159.26535897932"
    [log]="1/0.5/2/1e-300"
    [log2]="1/0.5/2"
    [log10]="1/0.1/10"
    [log1p]="-1/0/1e-300/-0.5"
    [acos]="1/-1/0.5/0"
    [asin]="1/-1/0.5/0"
    [atanh]="1/-1/0.5"
    [acosh]="1/1.0000000001"
    [erf]="0/5.9215871957945083/-5.9215871957945083/0.84375"
    [erfc]="0/26.543258430632299/27.226017111108365/-5.9/0.84375"
    [exp]="709.782712893384/-745.13321910194122/-708.39641853226408/0"
    [expm1]="709.782712893384/-37.42994775023705/0"
    [exp2]="1024/-1074/-1022/0"
    [exp10]="308.25471555991675/-323.60724533877976/0"
    [cosh]="710.47586007394394/-710.47586007394394/0"
    [sinh]="710.47586007394394/-710.47586007394394/0"
    [j0]="2.4048255576957728/5.5200781102863106/8.6537279129110125/11.791534439014281/1e4/1e100"
    [j1]="3.8317059702075123/7.0155866698156188/10.173468135062722/1e4/1e100"
    [y0]="0.89357696627916752/3.9576784193148579/7.0860510603017727/1e4/1e100"
    [y1]="2.197141326031017/5.4296810407941351/8.5960058683311689/1e4/1e100"
)
case $g in
x32a | x32b | x32c)
    case $g in x32a) L=$F32A ;; x32b) L=$F32B ;; *) L=$F32C ;; esac
    specs=()
    for f in $L; do specs+=("x32:$f:1024"); done
    bash "$here/run.sh" "$tree" "$out" "${specs[@]}"
    ;;
s32)
    specs=()
    for f in $F32A $F32B $F32C; do specs+=("u32:$f:1024"); done
    for f in $F32BIN; do specs+=("B32:$f:256" "b32:$f:4"); done
    bash "$here/run.sh" "$tree" "$out" "${specs[@]}"
    ;;
f64)
    specs=()
    for f in $F64; do
        specs+=("U64:$f:256" "u64:$f:4")
        [ -n "${P[$f]:-}" ] && specs+=("n64:$f:256:${P[$f]}")
    done
    for f in $F64BIN; do specs+=("B64:$f:64" "b64:$f:4"); done
    bash "$here/run.sh" "$tree" "$out" "${specs[@]}"
    ;;
analyze)
    python3 "$here/analyze.py" "$out" "$out/report"
    ;;
*)
    echo "suite.sh: unknown group $g" >&2
    exit 2
    ;;
esac
