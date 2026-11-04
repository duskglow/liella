# MATH-M6-F32-REST-2 table generator: EXP2_LO[j] = RN(2^(j/128) - EXP2_TAB[j]), CBRT_T[i] = RN(LOG_INV[i]^(-1/3)),
# CBRT_S[j] = RN(2^(j/3)). 90-digit decimal arithmetic; RN by float(str(Decimal)) (correctly rounded).
import re, struct, sys
from decimal import Decimal, getcontext
getcontext().prec = 90
src = open(sys.argv[1]).read()
def table(name):
    body = src.split("const %s: [f64; 128] = [" % name)[1].split("];")[0]
    return [struct.unpack("<d", struct.pack("<Q", int(h, 16)))[0] for h in re.findall(r"0x([0-9a-f]{16})", body)]
def bits(v): return "f64::from_bits(0x%016x)" % struct.unpack("<Q", struct.pack("<d", v))[0]
def rn(d): return float(str(d))
ln2 = Decimal(2).ln()
T = table("EXP2_TAB"); L = table("LOG_INV")
assert len(T) == 128 and len(L) == 128
lo = [rn((ln2 * j / 128).exp() - Decimal(T[j])) for j in range(128)]
ct = [rn((-Decimal(L[i]).ln() / 3).exp()) for i in range(128)]
cs = [rn((ln2 * j / 3).exp()) for j in range(3)]
def emit(name, vals):
    print("const %s: [f64; %d] = [" % (name, len(vals)))
    for k in range(0, len(vals), 3):
        print("    " + ", ".join(bits(v) for v in vals[k:k + 3]) + ",")
    print("];")
emit("EXP2_LO", lo); emit("CBRT_T", ct); emit("CBRT_S", cs)
