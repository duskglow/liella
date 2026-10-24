/* dllib.c - one source for every shared object of the dynamic-loading probe corpus (DL-0b).
 *
 * Built many times by run.sh with different -D options; each object prints what runs in it
 * with write(2), so the order is the order of execution whatever stdout's buffering is.
 *   -DNAME=X          the object's name (required); defines fn_X()
 *   -DWITH_INIT       lib_init/lib_fini (linked with -Wl,-init=lib_init,-fini=lib_fini: DT_INIT/DT_FINI)
 *   -DWITH_PRIO       constructors and destructors with priorities 200 and 300
 *   -DWITH_ATEXIT     the constructor registers an atexit handler
 *   -DCALL=Y          the constructor calls fn_Y() of a dependency
 *   -DUNDEF           references an undefined function (undefined_symbol_xyz)
 *   -DWITH_VER        fn_ver() (linked with a version script)
 *   -DTLS_IE_SIZE=N   an N-byte initial-exec TLS array (DF_STATIC_TLS)
 * Public interfaces only.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#ifndef NAME
#error NAME required
#endif
#define STR2(x) #x
#define STR(x) STR2(x)
#define CAT2(a, b) a##b
#define CAT(a, b) CAT2(a, b)

static void say(const char *what)
{
    char b[128];
    int n = snprintf(b, sizeof b, "%s %s\n", what, STR(NAME));
    if (n > 0)
        (void)!write(1, b, (size_t)n);
}

#ifdef CALL
int CAT(fn_, CALL)(void);
#endif

#ifdef WITH_ATEXIT
static void ax(void) { say("atexit-from-ctor"); }
#endif

__attribute__((constructor)) static void ctor(void)
{
    say("ctor");
#ifdef CALL
    CAT(fn_, CALL)();
#endif
#ifdef WITH_ATEXIT
    atexit(ax);
#endif
}
__attribute__((destructor)) static void dtor(void) { say("dtor"); }

#ifdef WITH_INIT
void lib_init(void) { say("DT_INIT"); }
void lib_fini(void) { say("DT_FINI"); }
#endif

#ifdef WITH_PRIO
__attribute__((constructor(200))) static void ctor200(void) { say("ctor200"); }
__attribute__((constructor(300))) static void ctor300(void) { say("ctor300"); }
__attribute__((destructor(200))) static void dtor200(void) { say("dtor200"); }
__attribute__((destructor(300))) static void dtor300(void) { say("dtor300"); }
#endif

int CAT(fn_, NAME)(void) { return 1; }

#ifdef UNDEF
extern int undefined_symbol_xyz(void);
int use_undef(void) { return undefined_symbol_xyz(); }
#endif

#ifdef WITH_VER
int fn_ver(void) { return 2; }
#endif

#ifdef TLS_IE_SIZE
__thread char tlsbuf[TLS_IE_SIZE] __attribute__((tls_model("initial-exec")));
char *tls_addr(void) { return tlsbuf; }
#endif
