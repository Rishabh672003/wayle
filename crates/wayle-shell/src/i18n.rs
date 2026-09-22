//! Internationalization for wayle-shell runtime labels.

use std::sync::OnceLock;

use i18n_embed::{
    LanguageLoader,
    fluent::{FluentLanguageLoader, fluent_language_loader},
    unic_langid::LanguageIdentifier,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "locales/"]
struct Localizations;

static LOADER: OnceLock<FluentLanguageLoader> = OnceLock::new();

/// Requested UI languages from the environment.
///
/// Parsed here rather than via `DesktopLanguageRequester` because that logs a
/// hard error for POSIX locales like `C`/`C.UTF-8`; we skip anything that
/// isn't a valid language tag and fall back to the default language.
fn requested_languages() -> Vec<LanguageIdentifier> {
    let raw = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|var| std::env::var(var).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();

    raw.split(':')
        .filter_map(|entry| {
            // Strip encoding (`.UTF-8`) and modifier (`@euro`), normalize `_`.
            let tag = entry.split(['.', '@']).next().unwrap_or(entry).replace('_', "-");
            if tag.is_empty() || tag.eq_ignore_ascii_case("C") || tag.eq_ignore_ascii_case("POSIX") {
                return None;
            }
            tag.parse().ok()
        })
        .collect()
}

#[allow(clippy::expect_used)]
pub fn loader() -> &'static FluentLanguageLoader {
    LOADER.get_or_init(|| {
        let loader = fluent_language_loader!();
        loader
            .load_fallback_language(&Localizations)
            .expect("embedded FTL resources are valid");

        let _ = i18n_embed::select(&loader, &Localizations, &requested_languages());

        loader
    })
}

macro_rules! t {
    ($message_id:literal) => {{
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id)
    }};
    ($message_id:literal, $($args:tt)*) => {{
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id, $($args)*)
    }};
}

pub(crate) use t;

macro_rules! td {
    ($message_id:expr) => {{ $crate::i18n::loader().get($message_id) }};
}

pub(crate) use td;
