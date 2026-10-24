//! Message catalogs (spec §7.7, §7.8; Appendix D.2; D-7.5, D-7.6): the file formats and the
//! pure parts of the lookups. The process-wide settings (the default domain, the bindings, the
//! loaded catalogs) are `liella_legacy::intl`; this module only reads bytes.
//!
//! * [`mo`]: GNU message objects (`.mo`), validated whole, searched by binary search.
//! * [`plural`]: `Plural-Forms` rules, compiled to a program that never traps.
//! * [`variants`]: the catalog directories gettext tries for a locale name.
//! * [`xopen`]: X/Open catalogs (`.cat`) and the `NLSPATH` templates of `catopen`.

pub mod mo;
pub mod plural;
pub mod variants;
pub mod xopen;

/// The locale names that never translate: `C`, `POSIX` and `C.UTF-8` (as `C.UTF-8` or
/// `C.utf8`), as observed for both the `LC_MESSAGES` locale and the elements of `LANGUAGE`.
pub fn untranslated_locale(name: &[u8]) -> bool {
    matches!(name, b"C" | b"POSIX" | b"C.UTF-8" | b"C.utf8")
}
