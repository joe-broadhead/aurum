//! Sentence-aware, phoneme-token-safe chunking for local TTS.
//!
//! Policy: **complete-or-error**. Text is split at sentence boundaries, then
//! clauses/whitespace, so every chunk stays within the model token capacity.
//! An unsplittable unit that exceeds the limit is rejected with size guidance.

use super::kokoro_vocab;
use super::tokenize;
use crate::error::{ProviderError, Result, UserError};

/// Phoneme→id map selected by the active TTS adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PhonemeCodec {
    /// KittenTTS / StyleTTS2 cleaner vocabulary.
    Kitten,
    /// Kokoro-82M config vocab (JOE-1618).
    Kokoro,
}

/// One synthesis unit: source text + padded phoneme token ids.
#[derive(Debug, Clone)]
pub(super) struct TtsChunk {
    pub(super) text: String,
    pub(super) ids: Vec<i64>,
}

/// Split `text` into model-safe chunks for the given adapter codec.
///
/// `max_tokens` is the maximum phoneme-token sequence length accepted by the
/// loaded voice pack (including start/end pads).
pub(super) fn prepare_tts_chunks_with(
    text: &str,
    max_tokens: usize,
    codec: PhonemeCodec,
) -> Result<Vec<TtsChunk>> {
    if max_tokens <= 2 {
        return Err(ProviderError::Other {
            message: format!("TTS model token capacity is too small ({max_tokens}); need > 2"),
        }
        .into());
    }

    let g2p = misaki_rs::G2P::new(misaki_rs::Language::EnglishUS);
    let mut chunks = Vec::new();
    let mut pending = String::new();

    for sentence in sentence_segments(text) {
        let candidate = join_text(&pending, sentence);
        if phoneme_ids(&g2p, &candidate, codec)?.len() <= max_tokens {
            pending = candidate;
            continue;
        }
        if !pending.is_empty() {
            chunks.push(make_chunk(&g2p, &pending, max_tokens, codec)?);
            pending.clear();
        }
        chunks.extend(split_oversize_segment(&g2p, sentence, max_tokens, codec)?);
    }
    if !pending.is_empty() {
        chunks.push(make_chunk(&g2p, &pending, max_tokens, codec)?);
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
    codec: PhonemeCodec,
) -> Result<Vec<TtsChunk>> {
    let mut chunks = Vec::new();
    let mut pending = String::new();
    for word in text.split_whitespace() {
        let candidate = join_text(&pending, word);
        if phoneme_ids(g2p, &candidate, codec)?.len() <= max_tokens {
            pending = candidate;
            continue;
        }
        if !pending.is_empty() {
            chunks.push(make_chunk(g2p, &pending, max_tokens, codec)?);
        }
        pending = word.to_string();
        let word_len = phoneme_ids(g2p, &pending, codec)?.len();
        if word_len > max_tokens {
            return Err(UserError::Other {
                message: format!(
                    "a single TTS unit exceeds the model's phoneme-token limit \
                     ({word_len} > {max_tokens}): {:?}\n  \
                     Hint: shorten or split the word; silent truncation is not supported.",
                    truncate_for_msg(word, 80)
                ),
            }
            .into());
        }
    }
    if !pending.is_empty() {
        chunks.push(make_chunk(g2p, &pending, max_tokens, codec)?);
    }
    Ok(chunks)
}

fn make_chunk(
    g2p: &misaki_rs::G2P,
    text: &str,
    max_tokens: usize,
    codec: PhonemeCodec,
) -> Result<TtsChunk> {
    let ids = phoneme_ids(g2p, text, codec)?;
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

fn phoneme_ids(g2p: &misaki_rs::G2P, text: &str, codec: PhonemeCodec) -> Result<Vec<i64>> {
    let (ipa, _) = g2p.g2p(text).map_err(|e| ProviderError::Other {
        message: format!("G2P failed: {e}"),
    })?;
    // Strip unknown markers that misaki may emit without espeak fallback.
    let cleaned = ipa.replace('❓', "");
    Ok(match codec {
        PhonemeCodec::Kitten => tokenize::ipa_to_ids(&cleaned),
        PhonemeCodec::Kokoro => kokoro_vocab::ipa_to_ids(&cleaned),
    })
}

fn sentence_segments(text: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if matches!(ch, '.' | '!' | '?' | ';' | '\n') {
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
    if segments.is_empty() {
        let t = text.trim();
        if !t.is_empty() {
            segments.push(t);
        }
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

fn truncate_for_msg(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!("{head}…")
}

/// Documented inter-chunk pause (milliseconds of silence).
pub const CHUNK_PAUSE_MS: u64 = 150;

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

        let chunks = prepare_tts_chunks_with(text, 399, PhonemeCodec::Kitten).unwrap();

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.ids.len() <= 399));
        assert!(chunks.iter().all(|chunk| chunk.ids.len() > 2));
        assert!(chunks.iter().any(|chunk| chunk.text.contains("Pogačar")));
        assert!(chunks.iter().any(|chunk| chunk.text.contains("Liège")));
        // Complete-or-error: concatenated chunk text covers the full input (modulo
        // whitespace joining). Every non-space char from input should appear.
        let joined: String = chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for word in ["Pogačar", "Liège", "Merckx", "Hinault"] {
            assert!(joined.contains(word), "missing {word} in {joined}");
        }
    }

    #[test]
    fn punctuation_free_text_falls_back_to_word_boundaries() {
        let text = "one two three four five six seven eight nine ten eleven twelve";

        let chunks = prepare_tts_chunks_with(text, 25, PhonemeCodec::Kitten).unwrap();

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

    #[test]
    fn unsplittable_unit_is_user_error() {
        // Force a tiny limit so a single word cannot fit.
        let err = prepare_tts_chunks_with(
            "supercalifragilisticexpialidocious",
            3,
            PhonemeCodec::Kitten,
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 2, "{err}");
        assert!(err.to_string().contains("exceeds"));
    }

    #[test]
    fn just_under_limit_single_chunk() {
        let chunks = prepare_tts_chunks_with("Hello world.", 399, PhonemeCodec::Kitten).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].ids.len() <= 399);
    }

    #[test]
    fn kokoro_codec_produces_padded_ids() {
        let chunks = prepare_tts_chunks_with("Hello world.", 509, PhonemeCodec::Kokoro).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ids[0], 0);
        assert_eq!(*chunks[0].ids.last().unwrap(), 0);
        assert!(chunks[0].ids.len() > 2);
    }
}
