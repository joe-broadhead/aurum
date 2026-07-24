//! On-device rule-based cleanup.
//!
//! No network, no ML weights — pure string/regex transforms.

use super::{CleanupProviderKind, CleanupResult, CleanupStyle, TextCleanup};
use crate::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

/// Local, deterministic text cleanup.
#[derive(Debug, Default, Clone)]
pub struct RulesCleanup;

impl RulesCleanup {
    pub fn new() -> Self {
        Self
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
        let trimmed = text.trim();
        let body = if trimmed.is_empty() {
            String::new()
        } else {
            match style {
                CleanupStyle::Raw => trimmed.to_string(),
                CleanupStyle::Clean => clean(trimmed),
                CleanupStyle::Bullets => to_bullets(&clean(trimmed)),
                CleanupStyle::Professional => professional(&clean(trimmed)),
                CleanupStyle::Summary => summarize(&clean(trimmed)),
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

static RE_WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("regex"));
static RE_WS2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").expect("regex"));
static RE_SPACE_PUNCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+([,.!?;:])").expect("regex"));
static RE_SENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?]+\s+").expect("regex"));

fn clean(text: &str) -> String {
    let mut result = RE_WS.replace_all(text, " ").into_owned();

    let multi = [
        "you know", "i mean", "sort of", "kind of", "okay so", "so yeah",
    ];
    for filler in multi {
        let pat = format!(r"(?i)\b{}\b[,.]?", regex::escape(filler));
        result = Regex::new(&pat)
            .expect("filler regex")
            .replace_all(&result, "")
            .into_owned();
    }

    let single = ["um", "uh", "uhm", "erm", "ah", "eh"];
    for filler in single {
        let pat = format!(r"(?i)\b{}\b[,.]?", regex::escape(filler));
        result = Regex::new(&pat)
            .expect("filler regex")
            .replace_all(&result, "")
            .into_owned();
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

fn professional(text: &str) -> String {
    let replacements: &[(&str, &str)] = &[
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
        (r"(?i)\bI'd\b", "I would"),
        (r"(?i)\bwe're\b", "we are"),
        (r"(?i)\bwe've\b", "we have"),
        (r"(?i)\bwe'll\b", "we will"),
        (r"(?i)\bthey're\b", "they are"),
        (r"(?i)\bthey've\b", "they have"),
        (r"(?i)\bit's\b", "it is"),
        (r"(?i)\bthat's\b", "that is"),
        (r"(?i)\bthere's\b", "there is"),
        (r"(?i)\bgonna\b", "going to"),
        (r"(?i)\bwanna\b", "want to"),
        (r"(?i)\bkinda\b", "kind of"),
        (r"(?i)\byeah\b", "yes"),
        (r"(?i)\byep\b", "yes"),
        (r"(?i)\bnope\b", "no"),
        (r"(?i)\bokay\b", "all right"),
        (r"(?i)\bok\b", "all right"),
        (r"(?i)\bhey\b", "hello"),
        (r"(?i)\basap\b", "as soon as possible"),
        (r"(?i)\bfyi\b", "for your information"),
    ];

    let mut result = text.to_string();
    for (pat, rep) in replacements {
        result = Regex::new(pat)
            .expect("pro regex")
            .replace_all(&result, *rep)
            .into_owned();
    }
    result = RE_WS2.replace_all(&result, " ").into_owned();
    capitalize_sentences(result.trim())
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
    // Simple split on .!? followed by whitespace — good enough for v0.
    let mut out: Vec<String> = RE_SENT_SPLIT
        .split(text)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if out.is_empty() {
        out.push(text.to_string());
    }
    // Re-attach terminal punctuation roughly
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
