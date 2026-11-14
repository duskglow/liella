/* Helper of s27_dlinfo (DL-18 DL-DLADDR): dla_syms without TLS, linked with a DT_RPATH of /rp
 * and no DT_RUNPATH (its HELPER-FLAGS line in s27_dlinfo.c). */
#define NOTLS 1
#include "dla_syms.c"
