//! On-device rule-based cleanup (JOE-1610).
//!
//! No network, no ML weights — pure string/regex transforms.
//! English filler/contraction heuristics apply only when language is English
//! (or `auto` defaulting to English). Other languages receive whitespace-safe
//! normalization only for `clean`/`professional` styles.

use super::{CleanupProviderKind, CleanupResult, CleanupStyle, TextCleanup};
use crate::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

/// Local, deterministic text cleanup.
#[derive(Debug, Default, Clone)]
pub struct RulesCleanup {
    /// Optional BCP-47 / ISO language hint (e.g. `en`, `fr`, `auto`).
    pub language: Option<String>,
}

impl RulesCleanup {
    pub fn new() -> Self {
        Self { language: None }
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    fn is_english(&self) -> bool {
        match self
            .language
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
        {
            None => true, // historical default: English heuristics
            Some(l)
                if l.is_empty()
                    || l == "auto"
                    || l == "en"
                    || l.starts_with("en-")
                    || l == "eng" =>
            {
                true
            }
            _ => false,
        }
    }
}

#[async_trait]
impl TextCleanup for RulesCleanup {
    fn name(&self) -> &'static str {
        "rules"
    }

    fn kind(&self) -> CleanupProviderKind {
        CleanupProviderKind::Rules
    }

    async fn cleanup(&self, text: &str, style: CleanupStyle) -> Result<CleanupResult> {
        let original = text.to_string();
        let english = self.is_english();
        let body = if text.trim().is_empty() {
            String::new()
        } else {
            match style {
                CleanupStyle::Raw => text.trim().to_string(),
                CleanupStyle::Clean => {
                    if english {
                        clean_english(text)
                    } else {
                        whitespace_safe(text)
                    }
                }
                CleanupStyle::Bullets => {
                    let base = if english {
                        clean_english(text)
                    } else {
                        whitespace_safe(text)
                    };
                    to_bullets(&base)
                }
                CleanupStyle::Professional => {
                    if english {
                        professional_english(&clean_english(text))
                    } else {
                        whitespace_safe(text)
                    }
                }
                CleanupStyle::Summary => {
                    let base = if english {
                        clean_english(text)
                    } else {
                        whitespace_safe(text)
                    };
                    summarize(&base)
                }
            }
        };
        Ok(CleanupResult {
            text: body,
            style,
            provider: CleanupProviderKind::Rules,
            original_text: original,
        })
    }
}

// --- Precompiled patterns (no per-call Regex::new for hot paths) ---

static RE_WS2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").expect("regex"));
static RE_SPACE_PUNCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]+([,.!?;:])").expect("regex"));
static RE_SENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?]+\s+").expect("regex"));

// English fillers (precompiled).
static RE_FILLER_MULTI: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        "you know", "i mean", "sort of", "kind of", "okay so", "so yeah",
    ]
    .into_iter()
    .map(|f| Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(f))).expect("filler"))
    .collect()
});
static RE_FILLER_SINGLE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    ["um", "uh", "uhm", "erm", "ah", "eh"]
        .into_iter()
        .map(|f| Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(f))).expect("filler"))
        .collect()
});

// Safe professional expansions only (no ambiguous I'd / it's).
static RE_PRO_SAFE: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?i)\bcan't\b", "cannot"),
        (r"(?i)\bwon't\b", "will not"),
        (r"(?i)\bdon't\b", "do not"),
        (r"(?i)\bdidn't\b", "did not"),
        (r"(?i)\bisn't\b", "is not"),
        (r"(?i)\baren't\b", "are not"),
        (r"(?i)\bwasn't\b", "was not"),
        (r"(?i)\bweren't\b", "were not"),
        (r"(?i)\bhaven't\b", "have not"),
        (r"(?i)\bhasn't\b", "has not"),
        (r"(?i)\bhadn't\b", "had not"),
        (r"(?i)\bI'm\b", "I am"),
        (r"(?i)\bI've\b", "I have"),
        (r"(?i)\bI'll\b", "I will"),
        // Intentionally omitted: I'd, it's, that's, there's (ambiguous).
        (r"(?i)\bwe're\b", "we are"),
        (r"(?i)\bwe've\b", "we have"),
        (r"(?i)\bwe'll\b", "we will"),
        (r"(?i)\bthey're\b", "they are"),
        (r"(?i)\bthey've\b", "they have"),
        (r"(?i)\bgonna\b", "going to"),
        (r"(?i)\bwanna\b", "want to"),
        (r"(?i)\bkinda\b", "kind of"),
        (r"(?i)\byeah\b", "yes"),
        (r"(?i)\byep\b", "yes"),
        (r"(?i)\bnope\b", "no"),
        (r"(?i)\basap\b", "as soon as possible"),
        (r"(?i)\bfyi\b", "for your information"),
    ]
    .into_iter()
    .map(|(p, r)| (Regex::new(p).expect("pro"), r))
    .collect()
});

/// Whitespace-safe normalization preserving paragraph breaks.
fn whitespace_safe(text: &str) -> String {
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| {
            let line = p
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            RE_WS2.replace_all(&line, " ").into_owned()
        })
        .filter(|p| !p.trim().is_empty())
        .collect();
    paragraphs.join("\n\n")
}

fn clean_english(text: &str) -> String {
    // Preserve paragraphs: clean each block independently.
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(clean_english_paragraph)
        .filter(|p| !p.is_empty())
        .collect();
    paragraphs.join("\n\n")
}

fn clean_english_paragraph(text: &str) -> String {
    // Collapse horizontal whitespace and single newlines inside a paragraph.
    let mut result = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    result = RE_WS2.replace_all(&result, " ").into_owned();

    for re in RE_FILLER_MULTI.iter() {
        result = re.replace_all(&result, "").into_owned();
    }
    for re in RE_FILLER_SINGLE.iter() {
        result = re.replace_all(&result, "").into_owned();
    }

    result = RE_WS2.replace_all(&result, " ").into_owned();
    result = RE_SPACE_PUNCT.replace_all(&result, "$1").into_owned();
    result = capitalize_sentences(result.trim());

    if result.chars().count() > 12 {
        if let Some(last) = result.chars().last() {
            if !".!?".contains(last) {
                result.push('.');
            }
        }
    }
    result
}

fn professional_english(text: &str) -> String {
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| {
            let mut result = p.to_string();
            for (re, rep) in RE_PRO_SAFE.iter() {
                result = re.replace_all(&result, *rep).into_owned();
            }
            result = RE_WS2.replace_all(&result, " ").into_owned();
            capitalize_sentences(result.trim())
        })
        .filter(|p| !p.is_empty())
        .collect();
    paragraphs.join("\n\n")
}

fn to_bullets(text: &str) -> String {
    let mut parts = split_sentences(text);
    if parts.len() == 1 {
        let commas: Vec<_> = text
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if commas.len() >= 3 {
            parts = commas;
        }
    }
    if parts.len() <= 1 {
        return format!("• {text}");
    }
    parts
        .into_iter()
        .map(|p| p.trim_matches(|c: char| ".!?;: ".contains(c)).to_string())
        .filter(|p| !p.is_empty())
        .map(|p| format!("• {}", capitalize_first(&p)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn summarize(text: &str) -> String {
    let sentences = split_sentences(text);
    if sentences.len() <= 2 {
        return text.to_string();
    }
    let first = sentences[0].clone();
    let rest = &sentences[1..];
    let longest = rest
        .iter()
        .max_by_key(|s| s.chars().count())
        .cloned()
        .unwrap_or_default();
    if longest.is_empty() || longest == first {
        first
    } else {
        format!("{first} {longest}")
    }
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out: Vec<String> = RE_SENT_SPLIT
        .split(text)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if out.is_empty() {
        out.push(text.to_string());
    }
    out
}

fn capitalize_sentences(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut capitalize_next = true;
    for ch in text.chars() {
        if capitalize_next && ch.is_alphabetic() {
            for c in ch.to_uppercase() {
                result.push(c);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
            if ".!?".contains(ch) {
                capitalize_next = true;
            }
        }
    }
    result
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn english_clean_strips_um() {
        let r = RulesCleanup::new()
            .with_language("en")
            .cleanup("um, hello there", CleanupStyle::Clean)
            .await
            .unwrap();
        assert!(r.text.to_ascii_lowercase().contains("hello"));
        assert!(!r.text.to_ascii_lowercase().contains("um"));
    }

    #[tokio::test]
    async fn french_does_not_strip_english_fillers_as_english() {
        // "um" as substring of French should not be English-filler processed.
        // We only apply whitespace-safe path — content preserved aside from trim.
        let r = RulesCleanup::new()
            .with_language("fr")
            .cleanup("Bonjour, um, le monde", CleanupStyle::Clean)
            .await
            .unwrap();
        // Whitespace-safe keeps words; may keep "um" as a token.
        assert!(r.text.contains("Bonjour") || r.text.to_ascii_lowercase().contains("bonjour"));
        assert!(r.text.contains("monde"));
    }

    #[tokio::test]
    async fn professional_preserves_id_contraction() {
        let r = RulesCleanup::new()
            .with_language("en")
            .cleanup("I'd like that", CleanupStyle::Professional)
            .await
            .unwrap();
        // Must not force "I would" (could have been "I had").
        assert!(
            r.text.contains("I'd") || r.text.contains("I d"),
            "got: {}",
            r.text
        );
        assert!(!r.text.contains("I would"));
    }

    #[tokio::test]
    async fn paragraphs_preserved_in_clean() {
        let input = "First paragraph um here.\n\nSecond paragraph uh there.";
        let r = RulesCleanup::new()
            .with_language("en")
            .cleanup(input, CleanupStyle::Clean)
            .await
            .unwrap();
        assert!(
            r.text.contains("\n\n"),
            "paragraph break lost: {:?}",
            r.text
        );
    }
}
