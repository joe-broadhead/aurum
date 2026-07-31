//! Kokoro-82M phoneme → token-id map (JOE-1618).
//!
//! Source: hexgrad/Kokoro-82M `config.json` `vocab` field (pinned with the
//! catalogue config artifact). Pad tokens are **0** at sequence start/end
//! (same convention as Kitten/StyleTTS2-style pipelines).

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Phoneme/token pairs from the reviewed Kokoro config (114 entries).
///
/// Unknown IPA characters are skipped (fail-soft mapping); callers still
/// require a non-empty padded sequence after mapping.
const KOKORO_VOCAB: &[(char, i64)] = &[
    (';', 1),
    (':', 2),
    (',', 3),
    ('.', 4),
    ('!', 5),
    ('?', 6),
    ('—', 9),
    ('…', 10),
    ('"', 11),
    ('(', 12),
    (')', 13),
    ('“', 14),
    ('”', 15),
    (' ', 16),
    ('\u{0303}', 17), // combining tilde
    ('ʣ', 18),
    ('ʥ', 19),
    ('ʦ', 20),
    ('ʨ', 21),
    ('ᵝ', 22),
    ('\u{ab67}', 23), // ʦ
    ('A', 24),
    ('I', 25),
    ('O', 31),
    ('Q', 33),
    ('S', 35),
    ('T', 36),
    ('W', 39),
    ('Y', 41),
    ('ᵊ', 42),
    ('a', 43),
    ('b', 44),
    ('c', 45),
    ('d', 46),
    ('e', 47),
    ('f', 48),
    ('h', 50),
    ('i', 51),
    ('j', 52),
    ('k', 53),
    ('l', 54),
    ('m', 55),
    ('n', 56),
    ('o', 57),
    ('p', 58),
    ('q', 59),
    ('r', 60),
    ('s', 61),
    ('t', 62),
    ('u', 63),
    ('v', 64),
    ('w', 65),
    ('x', 66),
    ('y', 67),
    ('z', 68),
    ('ɑ', 69),
    ('ɐ', 70),
    ('ɒ', 71),
    ('æ', 72),
    ('β', 75),
    ('ɔ', 76),
    ('ɕ', 77),
    ('ç', 78),
    ('ɖ', 80),
    ('ð', 81),
    ('ʤ', 82),
    ('ə', 83),
    ('ɚ', 85),
    ('ɛ', 86),
    ('ɜ', 87),
    ('ɟ', 90),
    ('ɡ', 92),
    ('ɥ', 99),
    ('ɨ', 101),
    ('ɪ', 102),
    ('ʝ', 103),
    ('ɯ', 110),
    ('ɰ', 111),
    ('ŋ', 112),
    ('ɳ', 113),
    ('ɲ', 114),
    ('ɴ', 115),
    ('ø', 116),
    ('ɸ', 118),
    ('θ', 119),
    ('œ', 120),
    ('ɹ', 123),
    ('ɾ', 125),
    ('ɻ', 126),
    ('ʁ', 128),
    ('ɽ', 129),
    ('ʂ', 130),
    ('ʃ', 131),
    ('ʈ', 132),
    ('ʧ', 133),
    ('ʊ', 135),
    ('ʋ', 136),
    ('ʌ', 138),
    ('ɣ', 139),
    ('ɤ', 140),
    ('χ', 142),
    ('ʎ', 143),
    ('ʒ', 147),
    ('ʔ', 148),
    ('ˈ', 156),
    ('ˌ', 157),
    ('ː', 158),
    ('ʰ', 162),
    ('ʲ', 164),
    ('↓', 169),
    ('→', 171),
    ('↗', 172),
    ('↘', 173),
    ('ᵻ', 177),
];

static VOCAB: Lazy<HashMap<char, i64>> = Lazy::new(|| KOKORO_VOCAB.iter().copied().collect());

/// Map misaki IPA phonemes to Kokoro token ids with start/end pad 0.
pub fn ipa_to_ids(ipa: &str) -> Vec<i64> {
    let mut ids = vec![0i64];
    for ch in ipa.chars() {
        if let Some(id) = VOCAB.get(&ch) {
            ids.push(*id);
        }
        // Skip unknown IPA (fail-soft); empty body is rejected by chunking.
    }
    ids.push(0i64);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_and_maps_common_ipa() {
        let ids = ipa_to_ids("həlˈoʊ");
        assert_eq!(ids[0], 0);
        assert_eq!(*ids.last().unwrap(), 0);
        assert!(ids.len() > 2);
        assert!(ids.contains(&50)); // h
        assert!(ids.contains(&83)); // ə
    }

    #[test]
    fn vocab_size_matches_pin() {
        assert_eq!(KOKORO_VOCAB.len(), 114);
        assert_eq!(VOCAB.len(), 114);
    }
}
