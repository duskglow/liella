/* s27_dlopen_tls_surplus: 128 KiB of initial-exec TLS, more than the static TLS block has room
 * for, so dlopen fails with "<path>: cannot allocate memory in static TLS block". */
__attribute__((tls_model("initial-exec"))) __thread char dlt_big_block[128 * 1024];
char *dlt_big_at(void) { return dlt_big_block; }
