/* s27_dlopen_tls_surplus: an object opened by dlopen with TLS in the global-dynamic,
 * local-dynamic and initial-exec models, a .tbss array and a 64-aligned variable. Its constructor
 * hands the program its accessor (the program exports dlt_register), so no dlsym is needed; the
 * accessor also reaches the program's own TLS variable dlt_prog_tls. */
__attribute__((tls_model("global-dynamic"))) __thread int dlt_gd = 7;
static __attribute__((tls_model("local-dynamic"))) __thread int dlt_ld = 9;
__attribute__((tls_model("initial-exec"))) __thread int dlt_ie = 11;
__thread char dlt_bss[64];
__thread long dlt_al __attribute__((aligned(64))) = 5;

/* The program's own TLS variable (exported): a dlopen'd object's relocation against it. */
extern __thread int dlt_prog_tls;

void dlt_register(int module, void *(*get)(int));

static void *get(int k)
{
    switch (k) {
    case 0: return &dlt_gd;
    case 1: return &dlt_ld;
    case 2: return &dlt_ie;
    case 3: return dlt_bss;
    case 4: return &dlt_al;
    case 5: return &dlt_prog_tls;
    }
    return 0;
}

__attribute__((constructor)) static void reg(void)
{
    dlt_register(0, get);
}
