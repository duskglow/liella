//! `resolv.conf`, `LOCALDOMAIN`, `RES_OPTIONS` and the `HOSTALIASES` file (spec §16.4, D-16.3):
//! the configuration parser, then the search rules over the list it gives.
//! Input: `resolv.conf NUL LOCALDOMAIN NUL RES_OPTIONS NUL hostname NUL HOSTALIASES text NUL
//! name to search` (an absent part is an unset variable or no file).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::names::dns::{client, conf::Config};
use liella_fuzz::Input;

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let text = input.opt_part();
    let localdomain = input.opt_part();
    let res_options = input.opt_part();
    let hostname = input.part();
    let aliases = input.part();
    let name = input.part();

    let zone = |z: &[u8]| (z.first() == Some(&b'e')).then_some(u32::try_from(z.len()).unwrap_or(0));
    let Ok(mut c) = Config::parse(text, localdomain, res_options, hostname, zone) else { return };
    assert!(!c.servers.is_empty(), "a configuration always has a server");
    let _ = liella_legacy::nscache::config_fingerprint(&c);
    for s in c.servers.iter() {
        let _ = client::sockaddr(s);
        let _ = liella_legacy::resolv_compat::sockaddr_in6(s);
    }

    // The search rules (res_nsearch), every candidate refused, then one accepted.
    let name = liella_legacy::resolv_compat::hostalias(name, aliases).unwrap_or(name);
    let domains: Vec<&[u8]> = c.search.iter().map(|d| d.as_slice()).collect();
    let mut asked = 0usize;
    let _ = client::search(name, c.ndots, &domains, c.options, |cand| {
        asked += 1;
        let _ = cand;
        Err::<(), _>(client::Miss::NotFound)
    });
    let _ = client::search(name, c.ndots, &domains, c.options, |cand| Ok::<usize, client::Miss>(cand.len()));
    for d in domains.iter().copied().chain([&b""[..]]) {
        let _ = client::join(name, Some(d));
    }

    // Applying the options and search text again (res_init after a program change).
    if let Some(o) = res_options {
        c.apply_options(o);
    }
    let _ = c.set_search(hostname);
});
