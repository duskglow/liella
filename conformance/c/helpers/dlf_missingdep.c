/* s27_dlopen_failures: needs libdlf_absent.so (dlf_gone's soname), which is never found. */
int dlf_fn_gone(void);
int dlf_fn_missingdep(void) { return dlf_fn_gone(); }
