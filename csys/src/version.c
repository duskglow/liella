/* Phase 0: the only function of the C layer, proving the build and link path. */
#include "liella_csys.h"

uint32_t liella_csys_abi_version(void)
{
    return LIELLA_CSYS_ABI_VERSION;
}
