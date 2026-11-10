/* Spec §1.1 (D-27.10): s01_static_pie.c linked with its relative relocations as RELA entries
 * (DT_RELA, R_X86_64_RELATIVE) instead of RELR; the same checks and the same 100 starts. */
/* LINK-STATIC-PIE: rela */
#include "s01_static_pie.c"
