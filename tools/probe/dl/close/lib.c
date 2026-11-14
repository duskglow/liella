/* lib.c - the objects of the DL-17 dlclose probe (probe.c), one per -D: NAME is the object's tag.
 * Public interfaces only. */
#include <dlfcn.h>
#include <stdio.h>

#define S2(x) #x
#define S(x) S2(x)

#ifdef GDEF
int g_val = 42;
#endif
#ifdef GUSE
extern int g_val; /* no DT_NEEDED on the definer: found through the global scope */
int use_g(void) { return g_val; }
#endif
#ifdef TLS_GD
__thread int t_val = 7;
int *t_addr(void) { return &t_val; }
#endif
#ifdef TLS_IE
__attribute__((tls_model("initial-exec"))) __thread int t_ie = 9;
int *t_addr(void) { return &t_ie; }
#endif
#ifdef UNIQ
__asm__(".globl u_obj\n .section .data.u_obj,\"aw\"\n .type u_obj, @gnu_unique_object\n .size u_obj, 4\nu_obj: .long 5\n .text\n");
#endif
#ifdef OPENER
/* Opens libY in its constructor and closes it in its destructor. */
static void *y;
__attribute__((constructor)) static void c(void) { y = dlopen(S(YPATH), RTLD_NOW); }
__attribute__((destructor)) static void d(void) {
    printf("dtor %s start\n", S(NAME));
    int r = dlclose(y);
    printf("dtor %s dlclose(Y)=%d\n", S(NAME), r);
}
#else
__attribute__((destructor)) static void d(void) { printf("dtor %s\n", S(NAME)); }
#endif
__attribute__((constructor)) static void c2(void) { printf("ctor %s\n", S(NAME)); }
int tag_fn(void) { return 1; }
