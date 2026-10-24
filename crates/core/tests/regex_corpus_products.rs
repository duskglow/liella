//! The regex corpus (validation item C): every regcomp/regexec and re_compile_pattern/re_search/
//! re_match call the coreutils, sed, bash and findutils test suites made against glibc,
//! deduplicated (`validation/corpus/regex.corpus`, `validation/harvest/`), replayed against the
//! host C library and against Liella's products (`tests/oracle/regex_corpus.c`). Disagreements
//! the optimization thread has not fixed yet are recorded in `validation/corpus/regex.known`.
#![allow(missing_docs)]

#[path = "support/corpus.rs"]
mod corpus_support;

#[test]
fn regex_corpus_equals_the_reference() {
    // Trace: s14_9_c_1, s14_9_c_2.
    corpus_support::replay("regex", &["en_US.UTF-8"]);
}
