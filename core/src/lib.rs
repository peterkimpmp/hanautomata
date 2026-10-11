//! Hanautomata shared engine — a port of the Windows app's `src/Core.cs` for the macOS (InputMethodKit) and Windows (TSF)
//! input-method shells.
//!
//! Parity contract (gate G1): every public operation returns what the C# engine returns on the golden suite that
//! `tests/dotnet/Hanautomata.Golden` extracts and `tests/parity.rs` replays. Where the C# code depends on UTF-16 code
//! units (lengths, indices, scoring) this crate works on UTF-16 too, and the .NET character semantics it needs
//! (`char.IsLetter`, `ToLowerInvariant`, `OrdinalIgnoreCase`) come from generated tables. See `core/README.md` for the
//! deliberate deviations (inputs the C# engine rejects with an exception).

mod composition;
mod detector;
mod focus;
pub mod hangul;
mod lists;
mod prefs;
mod resources;
mod scorer;
mod session;
mod store;
mod unicode;
#[rustfmt::skip]
mod unicode_tables;

pub use composition::Composition;
pub use detector::{Confidence, Decision, Detector, MIXED_KEY_LIMIT, MIXED_MARGIN, MIXED_PART_LIMIT, WORD_LIST_MARGIN};
pub use focus::can_capture;
pub use prefs::{context_key, LearnedWord, Lookup, WordPreferences, CAPACITY};
pub use scorer::{
    preset_thresholds, LanguageScorer, LanguageVerdict, CONTINUITY_PRIOR, DEFAULT_ENGLISH_THRESHOLD, DEFAULT_KOREAN_THRESHOLD,
    LEXICON_BONUS,
};
pub use session::{Key, Mode, Response, Session, MAX_WORD};
pub use store::{Store, MAX_FILE_BYTES};
