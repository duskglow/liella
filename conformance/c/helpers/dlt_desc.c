/* s27_dlopen_tls_surplus: an object opened by dlopen whose TLS is reached through TLS
 * descriptors (built with -mtls-dialect=gnu2): an initialised int and a .tbss array. */
__thread int dlt_dg = 13;
__thread char dlt_dz[200];

void dlt_register(int module, void *(*get)(int));

static void *get(int k)
{
    return k == 0 ? (void *)&dlt_dg : k == 1 ? (void *)dlt_dz : 0;
}

__attribute__((constructor)) static void reg(void)
{
    dlt_register(1, get);
}
