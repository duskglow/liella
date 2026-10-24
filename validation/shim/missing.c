/* Validation stand-in (validation/README.md, "Workarounds"): NOT part of Liella.
 *
 * Track M's libm is not a product yet (T3), and findutils' find needs modf. This thin stand-in,
 * written from C17 7.12.6.12, is linked into the validation builds that use workarounds. The
 * first runs' stand-ins for the system-identity calls of spec §24.5–24.6 (T2) are gone: Liella
 * implements them since milestone 3.
 */

/* modf (C17 7.12.6.12). */
double modf(double x, double *iptr)
{
    if (x != x) {
        *iptr = x;
        return x;
    }
    if (x >= 0x1p52 || x <= -0x1p52) {
        *iptr = x;
        return x > 0 ? 0.0 : -0.0;
    }
    double i = (double)(long long)x;
    *iptr = i;
    double f = x - i;
    return (f == 0 && x < 0) ? -0.0 : f;
}
