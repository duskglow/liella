//! Hidden conversion states (spec §6.2: the non-restartable forms and the `NULL`-state forms
//! keep "hidden static storage"). One state per function family, as atomics; per-thread slots
//! replace them with the phase-4 thread blocks (D-6.2, IP-10).

use core::sync::atomic::{AtomicU64, Ordering};

use liella::text::MbState;

/// Which hidden state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    /// `mblen`.
    Mblen = 0,
    /// `mbtowc`.
    Mbtowc = 1,
    /// `wctomb`.
    Wctomb = 2,
    /// `mbrlen` with a NULL state.
    Mbrlen = 3,
    /// `mbrtowc` with a NULL state.
    Mbrtowc = 4,
    /// `wcrtomb` with a NULL state.
    Wcrtomb = 5,
    /// `mbsrtowcs`/`mbsnrtowcs` with a NULL state.
    Mbsrtowcs = 6,
    /// `wcsrtombs`/`wcsnrtombs` with a NULL state.
    Wcsrtombs = 7,
    /// `mbrtoc16`/`c16rtomb` with a NULL state.
    C16 = 8,
    /// `mbrtoc32`/`c32rtomb` with a NULL state.
    C32 = 9,
    /// `mbrtoc8`/`c8rtomb` with a NULL state.
    C8 = 10,
}

static STATES: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];

/// The array index of a slot (its discriminant).
#[allow(clippy::as_conversions)] // a fieldless enum's discriminant
fn index(slot: Slot) -> usize {
    slot as usize
}

/// Load a hidden state.
pub fn load(slot: Slot) -> MbState {
    let v = STATES.get(index(slot)).map_or(0, |s| s.load(Ordering::Relaxed));
    MbState::from_bytes(v.to_le_bytes())
}

/// True when a hidden state holds all zero bytes (the initial state as stored after any complete
/// character): the single-character fast path of the boundary needs nothing else (WL-HOT-SEQ3).
#[inline]
pub fn is_zero(slot: Slot) -> bool {
    STATES.get(index(slot)).is_some_and(|s| s.load(Ordering::Relaxed) == 0)
}

/// Store a hidden state.
pub fn store(slot: Slot, st: MbState) {
    if let Some(s) = STATES.get(index(slot)) {
        s.store(u64::from_le_bytes(st.to_bytes()), Ordering::Relaxed);
    }
}

/// Serialise into an `mbstate_t` (8 bytes, D-6.1).
pub fn to_abi(st: MbState) -> [u8; 8] {
    st.to_bytes()
}

/// Read an `mbstate_t`.
pub fn from_abi(b: [u8; 8]) -> MbState {
    MbState::from_bytes(b)
}
