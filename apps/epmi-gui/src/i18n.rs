//! Minimal UI translation. Source strings are English (the default language); [`crate::de`] holds
//! the German translations. `tr!("Text")` returns the text in the current language,
//! `tr!("Slot {slot} saved", slot = n)` additionally fills named placeholders.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::German];

    /// Language names are shown in their own language.
    pub const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }

    /// Stable code for settings files.
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }

    /// From `LC_ALL` / `LC_MESSAGES` / `LANG`: German for `de*`, otherwise English.
    pub fn from_env() -> Self {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(var)
                && !value.is_empty()
            {
                return if value.starts_with("de") {
                    Self::German
                } else {
                    Self::English
                };
            }
        }
        Self::English
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    CURRENT.store(language as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    if CURRENT.load(Ordering::Relaxed) == Language::German as u8 {
        Language::German
    } else {
        Language::English
    }
}

fn german() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| crate::de::DE.iter().copied().collect())
}

/// `english` in the current language (English if no translation exists).
pub fn tr(english: &'static str) -> &'static str {
    match language() {
        Language::English => english,
        Language::German => german().get(english).copied().unwrap_or(english),
    }
}

/// Like [`tr`] for texts only known at runtime (e.g. labels from `epmi-core`).
pub fn tr_dyn(english: &str) -> String {
    match language() {
        Language::English => english.to_string(),
        Language::German => german()
            .get(english)
            .map_or_else(|| english.to_string(), |s| (*s).to_string()),
    }
}

/// Replaces `{name}` placeholders in `template`.
pub fn fill(template: &str, args: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[macro_export]
macro_rules! tr {
    ($en:literal) => {
        $crate::i18n::tr($en)
    };
    ($en:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::tr($en),
            &[$((stringify!($name), ($value).to_string())),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of every `tr!("…")` in `src` (escape sequences resolved).
    fn tr_texts(src: &str) -> Vec<String> {
        let mut texts = Vec::new();
        let mut rest = src;
        while let Some(pos) = rest.find("tr!(") {
            rest = rest[pos + 4..].trim_start();
            let Some(after_quote) = rest.strip_prefix('"') else {
                continue;
            };
            rest = after_quote;
            let mut text = String::new();
            let mut chars = rest.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => text.push('\n'),
                        Some('\n') => {
                            let skipped: String =
                                chars.clone().take_while(|c| c.is_whitespace()).collect();
                            for _ in skipped.chars() {
                                chars.next();
                            }
                        }
                        Some(other) => text.push(other),
                        None => break,
                    },
                    _ => text.push(c),
                }
            }
            texts.push(text);
        }
        texts
    }

    /// Every `tr!("…")` text in the GUI sources needs a German translation.
    #[test]
    fn all_ui_texts_are_translated() {
        let sources = [include_str!("main.rs"), include_str!("theme.rs")];
        let mut missing = Vec::new();
        for src in sources {
            for text in tr_texts(src) {
                if !german().contains_key(text.as_str()) {
                    missing.push(text);
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "missing German translations: {missing:#?}"
        );
    }

    /// A translation must use exactly the placeholders of the English text.
    #[test]
    fn translations_keep_placeholders() {
        fn names(text: &str) -> Vec<&str> {
            let mut out: Vec<&str> = text
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}').map(|(name, _)| name))
                .collect();
            out.sort_unstable();
            out
        }
        for (en, de) in crate::de::DE {
            assert_eq!(names(en), names(de), "{en:?} -> {de:?}");
        }
    }

    #[test]
    fn placeholders_are_filled() {
        assert_eq!(
            fill(
                "Slot {slot} of {n}",
                &[("slot", "05".into()), ("n", "30".into())]
            ),
            "Slot 05 of 30"
        );
    }
}
