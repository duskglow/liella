//! `nsswitch.conf` (spec §16.1, D-16.1) and `host.conf`'s `multi` (D-16.2): the configuration
//! parser, the encoded form kept in atomics, and the action engine over the parsed rules.
//! Input: `nsswitch.conf text NUL host.conf text NUL answers` (one byte per source asked).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::names::nss::{self, Config, Database, LookupError, Spec};
use liella_fuzz::Input;

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let conf = input.part();
    let host_conf = input.part();
    let answers = input.rest();

    let config = Config::parse(conf);
    for db in Database::ALL {
        let (spec, _) = config.resolve(db);
        // The encoded form round-trips.
        let decoded = Spec::decode(&spec.encode());
        assert_eq!(decoded, Some(spec), "encode/decode of {db:?}");
        // The action engine, with answers from the input.
        let mut k = 0usize;
        let mut next = || {
            let b = answers.get(k % answers.len().max(1)).copied().unwrap_or(0);
            k += 1;
            b
        };
        let r: nss::Answer<u32> = nss::run(
            &spec,
            |_| match next() % 6 {
                0 => Ok(1),
                1 => Err(LookupError::NotFound),
                2 => Err(LookupError::NoData),
                3 => Err(LookupError::TryAgain),
                4 => Err(LookupError::Unavailable),
                _ => Err(LookupError::Os(liella::error::Errno::ENOENT)),
            },
            |a: &mut u32, b: u32| {
                *a = a.wrapping_add(b);
                Ok(())
            },
        );
        let _ = r;
        nss::gather(&spec, db == Database::Initgroups, |_| if next() & 1 == 0 { Ok(()) } else { Err(LookupError::NotFound) });
    }
    // Each line on its own, as `__nss_configure_lookup` passes a source list.
    for line in conf.split(|&c| c == b'\n') {
        if let Some(spec) = Spec::parse(line) {
            assert_eq!(Spec::decode(&spec.encode()), Some(spec));
        }
    }
    let _ = liella_legacy::nss::multi_of(host_conf);
});
