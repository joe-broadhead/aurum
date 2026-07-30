//! Sentence-aware, phoneme-token-safe chunking for local TTS.

use super::tokenize::ipa_to_ids;
use crate::error::{ProviderError, Result};

#[derive(Debug)]
pub(super) struct TtsChunk {
    pub(super) text: String,
    pub(super) ids: Vec<i64>,
}

pub(super) fn prepare_tts_chunks(text: &str, max_tokens: usize) -> Result<Vec<TtsChunk>> {
    let g2p = misaki_rs::G2P::new(misaki_rs::Language::EnglishUS);
    let mut chunks = Vec::new();
    let mut pending = String::new();

    for sentence in sentence_segments(text) {
        let candidate = join_text(&pending, sentence);
        if phoneme_ids(&g2p, &candidate)?.len() <= max_tokens {
            pending = candidate;
            continue;
        }
        if !pending.is_empty() {
            chunks.push(make_chunk(&g2p, &pending, max_tokens)?);
            pending.clear();
        }
        chunks.extend(split_oversize_segment(&g2p, sentence, max_tokens)?);
    }
    if !pending.is_empty() {
        chunks.push(make_chunk(&g2p, &pending, max_tokens)?);
    }
    if chunks.is_empty() {
        return Err(ProviderError::Other {
            message: "G2P produced no tokenizable phonemes for input text".into(),
        }
        .into());
    }
    Ok(chunks)
}

fn split_oversize_segment(
    g2p: &misaki_rs::G2P,
    text: &str,
    max_tokens: usize,
) -> Result<Vec<TtsChunk>> {
    let mut chunks = Vec::new();
    let mut pending = String::new();
    for word in text.split_whitespace() {
        let candidate = join_text(&pending, word);
        if phoneme_ids(g2p, &candidate)?.len() <= max_tokens {
            pending = candidate;
            continue;
        }
        if !pending.is_empty() {
            chunks.push(make_chunk(g2p, &pending, max_tokens)?);
        }
        pending = word.to_string();
        if phoneme_ids(g2p, &pending)?.len() > max_tokens {
            return Err(ProviderError::Other {
                message: format!(
                    "a single TTS word exceeds the model's {max_tokens}-token limit: {word:?}"
                ),
            }
            .into());
        }
    }
    if !pending.is_empty() {
        chunks.push(make_chunk(g2p, &pending, max_tokens)?);
    }
    Ok(chunks)
}

fn make_chunk(g2p: &misaki_rs::G2P, text: &str, max_tokens: usize) -> Result<TtsChunk> {
    let ids = phoneme_ids(g2p, text)?;
    if ids.len() <= 2 || ids.len() > max_tokens {
        return Err(ProviderError::Other {
            message: format!(
                "invalid TTS chunk token count {} (model limit {max_tokens})",
                ids.len()
            ),
        }
        .into());
    }
    Ok(TtsChunk {
        text: text.to_string(),
        ids,
    })
}

fn phoneme_ids(g2p: &misaki_rs::G2P, text: &str) -> Result<Vec<i64>> {
    let (ipa, _) = g2p.g2p(text).map_err(|e| ProviderError::Other {
        message: format!("G2P failed: {e}"),
    })?;
    // Strip unknown markers that misaki may emit without espeak fallback.
    Ok(ipa_to_ids(&ipa.replace('❓', "")))
}

fn sentence_segments(text: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if matches!(ch, '.' | '!' | '?' | '\n') {
            let end = index + ch.len_utf8();
            let segment = text[start..end].trim();
            if !segment.is_empty() {
                segments.push(segment);
            }
            start = end;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        segments.push(tail);
    }
    segments
}

fn join_text(left: &str, right: &str) -> String {
    if left.is_empty() {
        right.to_string()
    } else {
        format!("{left} {right}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_paragraph_is_split_below_voice_pack_limit() {
        let text = "Tadej Pogačar (born 21 September 1998), nicknamed \"Pogi\", is a \
            Slovenian professional cyclist who rides for UCI WorldTeam UAE Team Emirates XRG. \
            His victories include five Tours de France (2020, 2021, 2024, 2025 and 2026), the \
            2024 Giro d'Italia, and thirteen one-day Monuments (Milan–San Remo once, Tour of \
            Flanders three times, Liège–Bastogne–Liège four times and Giro di Lombardia five \
            times), as well as the World Championship Road Race twice. Comfortable in \
            time-trialing, one-day classic riding and grand-tour climbing, he has been compared \
            to all-round cyclists such as Eddy Merckx and Bernard Hinault. Despite his youth, he \
            is considered one of the greatest cyclists of all time.";

        let chunks = prepare_tts_chunks(text, 399).unwrap();

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.ids.len() <= 399));
        assert!(chunks.iter().all(|chunk| chunk.ids.len() > 2));
        assert!(chunks.iter().any(|chunk| chunk.text.contains("Pogačar")));
        assert!(chunks.iter().any(|chunk| chunk.text.contains("Liège")));
    }

    #[test]
    fn punctuation_free_text_falls_back_to_word_boundaries() {
        let text = "one two three four five six seven eight nine ten eleven twelve";

        let chunks = prepare_tts_chunks(text, 25).unwrap();

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.ids.len() <= 25));
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            text
        );
    }

    #[test]
    fn sentence_splitter_preserves_unicode_and_tail() {
        assert_eq!(
            sentence_segments("Pogačar won. Liège too!\nNo final stop"),
            vec!["Pogačar won.", "Liège too!", "No final stop"]
        );
    }
}
