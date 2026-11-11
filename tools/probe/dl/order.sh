#!/bin/bash
# The constructor/destructor order corpus without libc (phase 14 DL-11, D-27.19). Black box: each
# object prints what runs with write(2) on fd 1; each program's _start prints "main", calls the
# termination function the loader passed in %rdx (as libc's exit would, after the atexit handlers),
# then exits 0. Run the same build under two loaders and compare line for line.
#
#   order.sh BUILD_DIR            build (if needed) and run each program directly (its PT_INTERP)
#   order.sh BUILD_DIR LDSO       run each program as `LDSO PROGRAM`
#
# DT_NEEDED entries are absolute paths (no soname), so no library search is involved.
set -u
W=${1:?usage: order.sh BUILD_DIR [LDSO]}
LDSO=${2:-}
CC=${CC:-cc}
mkdir -p "$W" || exit 2
W=$(cd "$W" && pwd)
F="-nostdlib -O1 -fno-stack-protector -fcf-protection=none -Wl,--no-as-needed -Wl,--hash-style=gnu"

say='static void say(const char *s) { long n = 0; while (s[n]) n++; long r; __asm__ volatile("syscall" : "=a"(r) : "a"(1L), "D"(1L), "S"(s), "d"(n) : "rcx", "r11", "memory"); }'

# lib NAME [extra flags...]: libNAME.so with a constructor and a destructor, plus EXTRA C text in $X.
lib() {
    local n=$1; shift
    printf '%s\n__attribute__((constructor)) static void c(void) { say("ctor %s\\n"); }\n__attribute__((destructor)) static void d(void) { say("dtor %s\\n"); }\nvoid %s_fn(void) {}\n%s\n' \
        "$say" "$n" "$n" "$n" "${X:-}" > "$W/$n.c"
    $CC $F -shared -fPIC "$@" -o "$W/lib$n.so" "$W/$n.c" || exit 2
}

# prog NAME LIBS...: a program linked against the given libraries (absolute paths).
prog() {
    local n=$1; shift
    cat > "$W/$n.c" <<EOF
$say
static void pre(void) { say("preinit_array main\\n"); }
__attribute__((section(".preinit_array"), used)) static void (*pa)(void) = pre;
static void ia(void) { say("init_array main\\n"); }
__attribute__((section(".init_array"), used)) static void (*ip)(void) = ia;
static void fa(void) { say("fini_array main\\n"); }
__attribute__((section(".fini_array"), used)) static void (*fp)(void) = fa;
__attribute__((constructor)) static void c(void) { say("ctor main\\n"); }
__attribute__((destructor)) static void d(void) { say("dtor main\\n"); }
__attribute__((used)) void start_c(long *sp, void (*fini)(void)) {
    (void)sp;
    say("main\\n");
    if (fini) { say("call fini\\n"); fini(); } else say("no fini\\n");
    __asm__ volatile("syscall" :: "a"(231L), "D"(0L));
    for (;;);
}
__asm__(".globl _start\\n_start:\\n mov %rsp, %rdi\\n mov %rdx, %rsi\\n and \$-16, %rsp\\n call start_c\\n hlt\\n");
EOF
    $CC $F -fPIE -pie -o "$W/$n" "$W/$n.c" "$@" || exit 2
    # The same program with the C library (crt1, main, exit): the reference's exit order. Only run
    # directly (a loader without a C library of its own cannot start it).
    sed -e '/^__attribute__((used)) void start_c/,$d' "$W/$n.c" > "$W/${n}_libc.c"
    printf 'int main(void) { say("main\\n"); return 0; }\n' >> "$W/${n}_libc.c"
    $CC -O1 -Wl,--no-as-needed -fPIE -pie -o "$W/${n}_libc" "$W/${n}_libc.c" "$@" || exit 2
}

if [ ! -x "$W/p_spre_libc" ]; then
    # The DL-0b graph: main -> A, D; A -> B, C; D -> C; D has DT_INIT/DT_FINI; A has prioritised
    # constructors and destructors.
    lib C
    lib B
    X='__attribute__((constructor(200))) static void c2(void) { say("ctor200 A\n"); }
__attribute__((constructor(300))) static void c3(void) { say("ctor300 A\n"); }
__attribute__((destructor(200))) static void d2(void) { say("dtor200 A\n"); }
__attribute__((destructor(300))) static void d3(void) { say("dtor300 A\n"); }' lib A "$W/libB.so" "$W/libC.so"
    X='void dinit(void) { say("DT_INIT D\n"); }
void dfini(void) { say("DT_FINI D\n"); }' lib D -Wl,-init=dinit -Wl,-fini=dfini "$W/libC.so"
    prog p_order "$W/libA.so" "$W/libD.so"
    # A two-object cycle: main -> H; H -> I; I -> H (I is built first without H, then rebuilt).
    lib I
    lib H "$W/libI.so"
    lib I "$W/libH.so"
    prog p_cycle "$W/libH.so"
    # Ties: main -> R, Q, P (link order); P -> R. Load order main, R, Q, P.
    lib R
    lib Q
    lib P "$W/libR.so"
    prog p_tie "$W/libR.so" "$W/libQ.so" "$W/libP.so"
    # A dependency loaded early through a later sibling: main -> K, L; K -> M; L -> N; N -> K.
    lib M
    lib K "$W/libM.so"
    lib N "$W/libK.so"
    lib L "$W/libN.so"
    prog p_sib "$W/libL.so" "$W/libK.so"
    # A three-object cycle with an outside dependency: main -> X; X -> Y; Y -> Z, W; Z -> X.
    lib W
    lib Z
    lib Y "$W/libZ.so" "$W/libW.so"
    lib X "$W/libY.so"
    lib Z "$W/libX.so"
    prog p_cycle3 "$W/libX.so"
    # A preinit array in a shared object: main -> S. The linker refuses .preinit_array in a DSO, so
    # libS's DT_INIT_ARRAY/DT_INIT_ARRAYSZ entries are retagged DT_PREINIT_ARRAY/DT_PREINIT_ARRAYSZ
    # (25/27 -> 32/33): its constructor is then only reachable as a preinit function.
    lib S
    perl -e '
        open(my $f, "+<", $ARGV[0]) or die; binmode $f; local $/; my $b = <$f>;
        my ($phoff) = unpack("Q<", substr($b, 32, 8)); my ($phent, $phnum) = unpack("S<S<", substr($b, 54, 4));
        for my $i (0 .. $phnum - 1) {
            my ($type, undef, $off, undef, undef, $sz) = unpack("L<L<Q<Q<Q<Q<", substr($b, $phoff + $i * $phent, 40));
            next unless $type == 2;
            for (my $e = $off; $e + 16 <= $off + $sz; $e += 16) {
                my ($tag) = unpack("Q<", substr($b, $e, 8));
                substr($b, $e, 8) = pack("Q<", 32) if $tag == 25;
                substr($b, $e, 8) = pack("Q<", 33) if $tag == 27;
            }
        }
        seek($f, 0, 0); print $f $b; close $f;' "$W/libS.so" || exit 2
    prog p_spre "$W/libS.so"
fi

for p in p_order p_cycle p_tie p_sib p_cycle3 p_spre; do
    for v in "$p" "${p}_libc"; do
        [ -n "$LDSO" ] && [ "$v" != "$p" ] && continue
        [ -x "$W/$v" ] || { echo "== $v: not built"; continue; }
        echo "== $v"
        if [ -n "$LDSO" ]; then "$LDSO" "$W/$v"; else "$W/$v"; fi
        echo "   rc=$?"
    done
done
