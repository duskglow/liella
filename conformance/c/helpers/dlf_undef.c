/* s27_dlopen_failures: calls a function no object defines (RTLD_NOW refuses it). */
int dlf_undefined_xyz(void);
int dlf_use_undef(void) { return dlf_undefined_xyz(); }
