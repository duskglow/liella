//! The names of the interim `files` lookups of phase 10, parts 1 and 2 (D-15.6), kept as aliases
//! of the `files` source (`crate::nss_files`) and the cursors (`crate::cursors`) for the clause
//! tests that name them. Every library caller now goes through the NSS engine (`crate::nss`,
//! `crate::netdb_compat`); nothing here reads `nsswitch.conf`, and nothing new should use
//! this module.

pub use crate::cursors::{line_at, Cursor, Step};
pub use crate::nss_files::{ether_by_addr, ether_by_name, group_by, group_list, gshadow_by, passwd_by, shadow_by, Key, ETHERS, GROUP, GSHADOW, PASSWD, SHADOW};
pub use liella::names::{Family, Ip};
