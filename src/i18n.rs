// Copyright 2026 git-next contributors. MIT license.
//
// The interface language of every message comes from a locale document.
//
// A locale document is a JSON file in `locales/`. The build embeds all of them,
// so a language is added by dropping in a file - there is no enumeration of the
// supported languages anywhere in the code that could fall out of date.
//
// English is the source language: the message templates in the code are English,
// and a locale document only lists the messages it overrides. A message that a
// locale does not translate is therefore shown in English rather than disappearing.

use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

include!(concat!(env!("OUT_DIR"), "/locales_generated.rs"));

/// The locale code of the source language. It never has a document of its own.
pub(crate) const SOURCE_LANGUAGE: &str = "en";

/// One parsed locale document.
#[derive(Debug, Deserialize)]
struct Locale {
    /// The locale code, for example `zh-Hans`.
    code: String,
    /// The name of the language in that language, for example `简体中文`.
    name: String,
    /// The name of the language in English, for example `Simplified Chinese`.
    #[serde(default)]
    english_name: String,
    /// Message translations, keyed by the English source message.
    #[serde(default)]
    messages: BTreeMap<String, String>,
}

/// Every locale shipped with this build.
///
/// The source language is part of the directory as well. It has no document of
/// its own - its messages are the source text in the code - but it has to be
/// findable, so that a device language such as `en-US` resolves to English
/// instead of falling through to whatever the fallback happens to be.
fn locales() -> &'static [Locale] {
    static LOCALES: OnceLock<Vec<Locale>> = OnceLock::new();
    LOCALES.get_or_init(|| {
        let mut parsed: Vec<Locale> = CATALOGS
            .iter()
            .filter_map(|(code, document)| {
                serde_json::from_str::<Locale>(document)
                    .ok()
                    .map(|mut locale| {
                        // The file name is the identity of a locale, so a document
                        // cannot claim a code it was not published under.
                        locale.code = (*code).to_owned();
                        locale
                    })
            })
            .collect();
        parsed.push(Locale {
            code: SOURCE_LANGUAGE.to_owned(),
            name: "English".to_owned(),
            english_name: "English".to_owned(),
            messages: BTreeMap::new(),
        });
        parsed.sort_by(|left, right| left.code.cmp(&right.code));
        parsed
    })
}

/// Find a locale by its code, comparing case-insensitively.
fn find(code: &str) -> Option<&'static Locale> {
    let wanted = normalize_code(code);
    locales()
        .iter()
        .find(|locale| normalize_code(&locale.code) == wanted)
}

/// Normalize a locale code for comparison: lowercase, `-` separated.
fn normalize_code(code: &str) -> String {
    code.trim().replace('_', "-").to_ascii_lowercase()
}

/// The language that is currently in effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Language {
    /// The code of the active locale.
    code: String,
    /// The name of the active locale in that language.
    name: String,
    /// The name of the active locale in English.
    english_name: String,
}

impl Language {
    /// The code of the active language.
    pub(crate) fn code(&self) -> &str {
        &self.code
    }

    /// The name of the active language in that language.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The name of the active language in English.
    pub(crate) fn english_name(&self) -> &str {
        &self.english_name
    }

    /// Whether this is the source language, which needs no translations.
    fn is_source(&self) -> bool {
        self.code == SOURCE_LANGUAGE
    }

    /// Translate a message, falling back to the English source text.
    ///
    /// The locale documents live for the whole process, so a translated message
    /// is available for as long as the caller needs it. The English fallback is
    /// the source text of the caller, so the result borrows from the input.
    ///
    /// The lifetime of the language is deliberately independent of the lifetime
    /// of the message: a message is a string constant that outlives every
    /// borrow, and tying the two together would force every caller to keep the
    /// language alive for the lifetime of the result.
    fn translate<'a>(&self, english: &'a str) -> &'a str {
        if self.is_source() {
            return english;
        }
        find(&self.code)
            .and_then(|locale| locale.messages.get(english))
            .map(|translated| &**translated)
            .unwrap_or(english)
    }
}
/// The language used when the device language has no matching locale.
///
/// English is the source language, so it is always available and is the only
/// sensible fallback: it is the language the messages are written in.
pub(crate) fn fallback_language() -> Language {
    find(SOURCE_LANGUAGE)
        .map(to_language)
        .unwrap_or_else(|| Language {
            code: SOURCE_LANGUAGE.to_owned(),
            name: "English".to_owned(),
            english_name: "English".to_owned(),
        })
}

/// Build the runtime language value of a locale document.
fn to_language(locale: &Locale) -> Language {
    Language {
        code: locale.code.clone(),
        name: locale.name.clone(),
        english_name: if locale.english_name.is_empty() {
            locale.name.clone()
        } else {
            locale.english_name.clone()
        },
    }
}

/// Match a device locale against the shipped locales.
///
/// The device reports locales such as `zh-CN`, `zh-Hans-CN` or `pt-BR`. The
/// matching therefore walks from the most specific form to the least specific
/// one: the full code, then the code without its region, then the bare language.
fn match_locale(locale: &str) -> Option<Language> {
    let normalized = normalize_code(locale);
    if normalized.is_empty() || normalized == "c" || normalized == "posix" {
        return None;
    }
    let parts: Vec<&str> = normalized.split('-').collect();
    let mut candidates: Vec<String> = vec![normalized.clone()];
    if parts.len() > 1 {
        // A region implies a writing system for the languages that distinguish
        // one: `zh-CN` is written in simplified characters and `zh-TW` in
        // traditional ones. The script is therefore resolved before the region
        // is dropped, so `zh-TW` does not end up on a simplified document.
        if let Some(script) = script_for_region(parts[parts.len() - 1]) {
            let with_script = format!("{}-{}", parts[0], script);
            if !candidates.contains(&with_script) {
                candidates.push(with_script);
            }
        }
        let without_region = parts[..parts.len() - 1].join("-");
        if !candidates.contains(&without_region) {
            candidates.push(without_region);
        }
        if parts.len() > 2 {
            let language_only = parts[0].to_owned();
            if !candidates.contains(&language_only) {
                candidates.push(language_only);
            }
        }
    }
    candidates
        .iter()
        .find_map(|candidate| find(candidate).map(to_language))
}

/// The writing system a region implies, for the languages that distinguish one.
fn script_for_region(region: &str) -> Option<&'static str> {
    match region {
        "cn" | "sg" | "my" => Some("hans"),
        "tw" | "hk" | "mo" => Some("hant"),
        _ => None,
    }
}

/// Whether a locale states that the user has no language preference.
fn is_unset_locale(locale: &str) -> bool {
    let normalized = normalize_code(locale);
    normalized.is_empty() || normalized == "c" || normalized == "posix"
}

/// The language of the device, used when the user follows the device language.
pub(crate) fn system_language() -> Language {
    // The native API is the authority on Windows and on Apple platforms: it
    // reports the language the user actually chose for the system, which is
    // what "follow the device language" has to mean there.
    //
    // The environment variables are only consulted where the native API does
    // not exist, because a shell on those platforms can export a locale that
    // has nothing to do with the language the user picked.
    for locale in sys_locale::get_locales() {
        if let Some(language) = match_locale(&locale) {
            return language;
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            let entries: Vec<&str> = value
                .split(':')
                .map(|entry| entry.split(['.', '@']).next().unwrap_or(entry))
                .collect();
            if let Some(language) = entries.iter().find_map(|entry| match_locale(entry)) {
                return language;
            }
            // Either the locale states that the user has no preference, or it
            // names a language no document ships for. The source language is the
            // honest answer in both cases: guessing a related language would
            // show messages the user cannot read.
            let _ = is_unset_locale(entries.first().copied().unwrap_or_default());
            return fallback_language();
        }
    }
    fallback_language()
}

/// Resolve a user supplied language name into the language it selects.
///
/// The name may be a locale code (`zh-Hans`), a legacy alias (`chinese`), or
/// `auto` to follow the device language.
pub(crate) fn resolve(selection: &str) -> Option<Language> {
    let trimmed = selection.trim();
    if trimmed.is_empty() {
        return None;
    }
    if matches!(trimmed.to_ascii_lowercase().as_str(), "auto" | "system") {
        return Some(system_language());
    }
    if let Some(code) = legacy_alias(trimmed) {
        return find(code).map(to_language);
    }
    find(trimmed).map(to_language)
}

/// The canonical name a selection is stored under.
pub(crate) fn canonical(selection: &str) -> String {
    let trimmed = selection.trim();
    if matches!(trimmed.to_ascii_lowercase().as_str(), "auto" | "system") {
        return "auto".to_owned();
    }
    match legacy_alias(trimmed) {
        Some(code) => code.to_owned(),
        None => normalize_code(trimmed),
    }
}

/// Map the language names that earlier releases accepted to a locale code.
///
/// The aliases keep existing configuration files working after the switch to
/// locale files, so a user who pinned `chinese` keeps Simplified Chinese.
fn legacy_alias(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "english" | "en" | "en-us" | "en_us" => Some(SOURCE_LANGUAGE),
        "chinese" | "zh" | "zh-cn" | "zh_cn" | "zh-hans" | "中文" => Some("zh-Hans"),
        _ => None,
    }
}

/// The languages that ship with this build, for the language picker.
pub(crate) fn available() -> Vec<Language> {
    locales().iter().map(to_language).collect()
}

/// Translate a message with the given language.
///
/// The lifetime of the language is independent of the lifetime of the message,
/// so a translated string constant can be stored in the interface without
/// keeping the language value alive.
pub(crate) fn translate<'a>(language: &Language, english: &'a str) -> &'a str {
    language.translate(english)
}
