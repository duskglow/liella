/* s27_dlopen_failures: needs dlf_fn_ver@VER_2 of libdlf_ver.so, whose run-time file has VER_1 only. */
int dlf_fn_ver(void);
int dlf_fn_w(void) { return dlf_fn_ver(); }
