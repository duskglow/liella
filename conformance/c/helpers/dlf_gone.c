/* s27_dlopen_failures: linked by dlf_missingdep, under the soname libdlf_absent.so, which no file has. */
int dlf_fn_gone(void) { return 1; }
