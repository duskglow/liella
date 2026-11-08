# Saturation register: calls at or near their largest achievable gain

The register is one file per section under `docs/saturation/`, so that performance stories do not edit the same
file. Put a new row in the section file it belongs to; a new section is a new file `docs/saturation/<nn>-<slug>.md`
starting with `## <nn>. <title>` and a line here. Do not renumber by hand: `cargo xtask register --check` (also part
of `cargo xtask static`) requires every section file to be listed here, section numbers to be unique and contiguous,
and every row id (first cell of a `Row` table) to be unique across files.

- [Introduction and sources](saturation/00-intro.md): sources, caveats and how to read the tables
