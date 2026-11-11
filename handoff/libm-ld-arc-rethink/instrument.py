#!/usr/bin/env python3
"""LIBM-LD-ARC-RETHINK scratch instrumentation, never landed: phase markers (lfence; rdtsc; lfence, stored at the
page arcprobe.c maps) in a Liella tree. usage: instrument.py apply|restore <tree> <backup dir>.
Restore writes the saved text back (fresh mtimes, so cargo rebuilds)."""
import os
import sys

MARK = '''
/// LIBM-LD-ARC-RETHINK scratch probe (never landed): serialized TSC stamp `k` at the page arcprobe maps.
#[inline(always)]
pub fn phase_mark(k: usize) {
    unsafe {
        core::arch::x86_64::_mm_lfence();
        let t = core::arch::x86_64::_rdtsc();
        core::ptr::write_volatile((0x100_0000_0000usize as *mut u64).add(k), t);
        core::arch::x86_64::_mm_lfence();
    }
}
'''
ROOT = "    let (s, is, below) = root_one_minus_sq(a, hw)?;\n"
EDITS = {
    'crates/math/Cargo.toml': [('[lints]\nworkspace = true\n', '[lints.rust]\nunsafe_code = "allow"\n')],
    'crates/math/src/lib.rs': [('#![forbid(unsafe_code)]\n', '#![allow(unsafe_code)]\n')],
    'crates/math/src/wide_int/arc.rs': [
        ("    let ia = inv_top(a);\n" + ROOT + "    Some((n.neg, if below { atan_le(a, s, is) } else { sub_from(PIO2, atan_le(s, a, ia)) }))\n",
         "    crate::phase_mark(2);\n    let ia = inv_top(a);\n" + ROOT + "    crate::phase_mark(3);\n"
         "    let r = Some((n.neg, if below { atan_le(a, s, is) } else { sub_from(PIO2, atan_le(s, a, ia)) }));\n"
         "    crate::phase_mark(4);\n    r\n"),
        ("    let ia = inv_top(a);\n" + ROOT + "    let t = if below",
         "    crate::phase_mark(2);\n    let ia = inv_top(a);\n" + ROOT + "    crate::phase_mark(3);\n    let t = if below"),
        ("    Some((false, if n.neg { sub_from(PI, t) } else { t }))\n",
         "    let r = Some((false, if n.neg { sub_from(PI, t) } else { t }));\n    crate::phase_mark(4);\n    r\n"),
        ("    let t = atan2_pos(val(b)?, val(a)?);\n    Some((b.neg, if a.neg { sub_from(PI, t) } else { t }))\n",
         "    let (vb, va) = (val(b)?, val(a)?);\n    crate::phase_mark(2);\n    crate::phase_mark(3);\n"
         "    let t = atan2_pos(vb, va);\n    let r = Some((b.neg, if a.neg { sub_from(PI, t) } else { t }));\n"
         "    crate::phase_mark(4);\n    r\n"),
    ],
    'crates/boundary/src/mathret.rs': [
        ("-> (R, Status)) -> R::C {\n    let (e, mode) = enter(unit);\n    let x = core::hint::black_box(e.operand(x));\n"
         "    let (v, mut st) = core::hint::black_box(f(x, mode));\n    let v = e.result(v, &mut st);\n    exit(e, st);\n    v.c()\n",
         "-> (R, Status)) -> R::C {\n    liella_math::phase_mark(0);\n    let (e, mode) = enter(unit);\n"
         "    liella_math::phase_mark(1);\n"
         "    let x = core::hint::black_box(e.operand(x));\n    let (v, mut st) = core::hint::black_box(f(x, mode));\n"
         "    liella_math::phase_mark(5);\n    let v = e.result(v, &mut st);\n    exit(e, st);\n"
         "    liella_math::phase_mark(6);\n    v.c()\n"),
    ],
}


def main():
    op, tree, bak = sys.argv[1], sys.argv[2], sys.argv[3]
    # `math-only`: leave the boundary crate untouched (no entry/exit marks 0, 1, 5, 6).
    math_only = len(sys.argv) > 4 and sys.argv[4] == 'math-only'
    for rel, edits in EDITS.items():
        path, saved = os.path.join(tree, rel), os.path.join(bak, rel.replace('/', '__'))
        if op == 'restore' and not os.path.exists(saved):
            continue
        if op != 'restore' and math_only and rel.startswith('crates/boundary/'):
            continue
        if op == 'restore':
            with open(saved) as f, open(path, 'w') as g:
                g.write(f.read())
            continue
        with open(path) as f:
            text = f.read()
        os.makedirs(bak, exist_ok=True)
        with open(saved, 'w') as g:
            g.write(text)
        for old, new in edits:
            if text.count(old) != 1:
                sys.exit(f'instrument: {rel}: pattern found {text.count(old)} times: {old[:60]!r}')
            text = text.replace(old, new)
        if rel.endswith('math/src/lib.rs'):
            text += MARK
        with open(path, 'w') as g:
            g.write(text)
    print(f'instrument: {op} done')


main()
