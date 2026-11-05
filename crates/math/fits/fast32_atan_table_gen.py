# LIBM-F32-DOUBLE-KERNELS table generator (D-10.3): ATAN32[i] = RN(atan(i/32)) for i = 0..32 and
# the constants PIO2 = RN(pi/2), PI = RN(pi). 100-digit decimal arithmetic: atan by three argument
# halvings atan(x) = 2 atan(x / (1 + sqrt(1 + x^2))) (|x| <= 1 becomes <= tan(pi/32) < 0.1)
# and then its Taylor series to 10^-100; pi = 4 atan(1). RN by float(str(Decimal)) (correctly
# rounded). The tests re-check every entry against the accurate double-double path.
import struct
from decimal import Decimal, getcontext
getcontext().prec = 100
def atan(x):
    k = 0
    while abs(x) > Decimal("0.1"):
        x = x / (1 + (1 + x * x).sqrt())
        k += 1
    s, term, n, x2 = Decimal(0), x, 1, x * x
    while abs(term) > Decimal(10) ** -105:
        s += term / n
        term = -term * x2
        n += 2
    return s * (2 ** k)
def bits(v): return "f64::from_bits(0x%016x)" % struct.unpack("<Q", struct.pack("<d", v))[0]
def rn(d): return float(str(d))
pi = 4 * atan(Decimal(1))
vals = [rn(atan(Decimal(i) / 32)) for i in range(33)]
print("const ATAN32: [f64; 33] = [")
for k in range(0, 33, 3):
    print("    " + ", ".join(bits(v) for v in vals[k:k + 3]) + ",")
print("];")
print("const PIO2: f64 = %s; // RN(pi/2)" % bits(rn(pi / 2)))
print("const PI: f64 = %s; // RN(pi)" % bits(rn(pi)))
