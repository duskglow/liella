//! The dynamic loader's ELF reader (`liella_ld::elf`, IP-24, decisions D-27.7 and D-27.8): the
//! header and program headers, `PT_INTERP`, the notes and their properties, the dynamic section,
//! `DT_NEEDED`, both hash tables and their chains, the symbol version tables and the RELA/RELR
//! iterators. Input: the bytes of one ELF file. Every error is a value; a panic, an out-of-range
//! read or a walk that does not end is a finding. Each walk takes at most `STEPS` items, so a
//! long (but finite) table cannot turn into a time-out.
#![no_main]

use libfuzzer_sys::fuzz_target;
use liella_ld::arch::X86_64;
use liella_ld::elf::{pt, Dynamic, Elf};

/// The most items one walk takes.
const STEPS: usize = 4096;

fn walk_dynamic(d: &Dynamic<'_>) {
    let _ = d.len();
    for e in d.entries().take(STEPS) {
        let _ = d.get(e.tag);
    }
    let _ = d.strtab();
    for n in d.needed().take(STEPS) {
        let _ = n;
    }
    let _ = d.symbol_count();
    if let Ok(Some(h)) = d.sysv_hash() {
        let _ = (h.nbucket(), h.nchain(), h.bucket(0));
        for hash in [0u32, 1, 0x1234_5678, u32::MAX] {
            for s in h.chain(hash).take(STEPS) {
                let _ = s;
            }
        }
    }
    if let Ok(Some(g)) = d.gnu_hash() {
        let _ = (g.nbuckets(), g.symoffset(), g.bloom_size(), g.bloom_shift(), g.bloom_word(0), g.bucket(0));
        let _ = g.symbol_count();
        for hash in [0u32, 1, 0x1234_5678, u32::MAX] {
            for e in g.chain(hash).take(STEPS) {
                let _ = e;
            }
        }
    }
    if let Ok(Some(v)) = d.versym() {
        let _ = v.max_index();
        let n = v.count().unwrap_or(16).min(STEPS as u32);
        for i in 0..n.saturating_add(1) {
            let _ = v.get(i);
        }
        let _ = v.get(u32::MAX);
    }
    if let Ok(Some(defs)) = d.verdef() {
        for def in defs.take(STEPS).flatten() {
            for name in def.names().take(STEPS) {
                let _ = name;
            }
        }
    }
    if let Ok(Some(needs)) = d.verneed() {
        for need in needs.take(STEPS).flatten() {
            for aux in need.auxes().take(STEPS) {
                let _ = aux;
            }
        }
    }
    if let Ok(Some(r)) = d.rela::<X86_64>() {
        let _ = r.entry_count();
        for e in r.take(STEPS) {
            let _ = e;
        }
    }
    if let Ok(Some(r)) = d.jmprel::<X86_64>() {
        for e in r.take(STEPS) {
            let _ = e;
        }
    }
    if let Ok(Some(r)) = d.relr() {
        for e in r.take(STEPS) {
            let _ = e;
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(elf) = Elf::parse::<X86_64>(data) else { return };
    let _ = elf.header();
    let _ = elf.interp();
    for ph in elf.program_headers().take(STEPS) {
        let _ = elf.segment_bytes(&ph);
        let _ = elf.bytes_at(ph.vaddr, ph.filesz);
        let _ = elf.bytes_from(ph.vaddr.saturating_add(ph.filesz));
        if ph.p_type == pt::NOTE
            && let Ok(notes) = elf.notes(&ph)
        {
            for n in notes.take(STEPS).flatten() {
                for p in n.properties().take(STEPS) {
                    let _ = p;
                }
            }
        }
    }
    if let Ok(Some(d)) = elf.dynamic() {
        walk_dynamic(&d);
    }
});
