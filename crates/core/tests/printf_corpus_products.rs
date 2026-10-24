//! The printf corpus (validation item C): the printf-family calls (format, typed arguments,
//! locale, errno) the coreutils, sed, bash and findutils test suites made against glibc,
//! deduplicated (`validation/corpus/printf.corpus`, `validation/harvest/`), formatted again with
//! snprintf against the host C library and against Liella's products
//! (`tests/oracle/printf_corpus.c`). Disagreements not fixed yet are recorded in
//! `validation/corpus/printf.known`.
#![allow(missing_docs)]

#[path = "support/corpus.rs"]
mod corpus_support;

#[test]
fn printf_corpus_equals_the_reference() {
    // Trace: s12_6_c_0, s12_6_c_2, s12_6_c_3, s12_6_c_5, s12_6_c_6.
    corpus_support::replay("printf", &["en_US.UTF-8"]);
}
