//! Conversion-vocabulary tests (implementation-plan.md §8.3).
//!
//! Every conversion and return-convention function in `liella_abi::{conv, ret, strz, pack, va}`
//! gets exhaustive boundary tests here before any translation function may use it: NULL
//! handling, maximum lengths, int/size_t range edges, errno preservation on success (spec C0.1).
