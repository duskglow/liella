#!/bin/bash
# Plant for validation/tmpfs-run.sh (FS-TMPFS-SCRATCH-RUN), run from a RAM copy of the role (no target/):
#   usage: tmpfs-run-plants.sh <copy-dir>   (the copy is made by the caller with rsync -a --exclude /target)
# Baseline: a small existing test must give identical lists. Plant: a test that depends on readdir order (it asserts the
# unsorted listing is NOT newest-first, true on ext4's hash order, false on tmpfs) must make the tool report DIFFERENT.
# Prints CAUGHT / SURVIVED; exits non-zero on a red baseline or a surviving plant.
set -u
copy=${1:?copy dir}
cd "$copy" || exit 2
. "$HOME/.cargo/env"
out=$copy/plant-out; mkdir -p "$out"
cargo xtask so > "$out/so.log" 2>&1 || { echo "BASELINE RED: so"; exit 2; }
cargo nextest run --no-run -p liella -p liella-boundary > "$out/nr.log" 2>&1 || { echo "BASELINE RED: build"; exit 2; }
validation/tmpfs-run.sh --no-build --out "$out/base" -- -E 'binary(=s13_filesystem_clauses) & test(/s13_2_c/)' > "$out/base.txt" 2>&1
rc=$?
tail -3 "$out/base.txt"
[ $rc = 0 ] || { echo "BASELINE RED: rc $rc"; exit 2; }
cat > crates/core/tests/zz_plant_readdir.rs <<'RS'
// PLANT (not committed): depends on readdir order. no spec clause.
#[test]
fn plant_readdir_order() {
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("plant-rd-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let names: Vec<String> = (0..12).map(|i| format!("f{i}")).collect();
    for n in &names { std::fs::write(d.join(n), b"x").unwrap(); }
    let listed: Vec<String> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    let mut newest_first = names.clone(); newest_first.reverse();
    std::fs::remove_dir_all(&d).unwrap();
    assert_ne!(listed, newest_first, "listing came back newest-first");
}
RS
validation/tmpfs-run.sh --out "$out/plant" -- -E 'binary(=zz_plant_readdir)' > "$out/plant.txt" 2>&1
rc=$?
tail -6 "$out/plant.txt"
if [ $rc = 1 ]; then echo "CAUGHT plant-readdir-order (tmpfs-run.sh rc 1, lists differ)"; exit 0; fi
echo "SURVIVED plant-readdir-order (rc $rc)"; exit 1
