//! KittenTTS IPA character vocabulary → token ids.
//!
//! Vocabulary order matches the KittenTTS / StyleTTS2 TextCleaner list so the
//! ONNX graph receives the same integer sequences as the reference pipeline.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

const PAD: char = '$';

/// Punctuation string (matches Python `_punctuation`).
const PUNCTUATION: &str = ";:,.!?¡¿—…\u{201C}«»\u{201D}\" ";

/// ASCII letters A–Z a–z.
const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// IPA characters (matches Python `_letters_ipa`).
const IPA_LETTERS: &str =
    "ɑɐɒæɓʙβɔɕçɗɖðʤəɘɚɛɜɝɞɟʄɡɠɢʛɦɧħɥʜɨɪʝɭɬɫɮʟɱɯɰŋɳɲɴøɵɸθœɶʘɹɺɾɻʀʁɽʂʃʈʧʉʊʋⱱʌɣɤʍχʎʏʑʐʒʔʡʕʢǀǁǂǃˈˌːˑʼʴʰʱʲʷˠˤ˞↓↑→↗↘\u{2019}\u{0329}\u{2018}ᵻ";

static VOCAB: Lazy<HashMap<char, i64>> = Lazy::new(|| {
    let symbols: Vec<char> = std::iter::once(PAD)
        .chain(PUNCTUATION.chars())
        .chain(LETTERS.chars())
        .chain(IPA_LETTERS.chars())
        .collect();
    symbols
        .into_iter()
        .enumerate()
        .map(|(i, c)| (c, i as i64))
        .collect()
});

static RE_TOKENIZE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\w+|[^\w\s]").expect("regex"));

/// Full pipeline: IPA string → padded token ID vector (`[0, …, 0]`).
pub fn ipa_to_ids(ipa: &str) -> Vec<i64> {
    let tokenized = RE_TOKENIZE
        .find_iter(ipa)
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut ids = vec![0i64];
    for ch in tokenized.chars() {
        if let Some(id) = VOCAB.get(&ch) {
            ids.push(*id);
        }
    }
    ids.push(0i64);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_present() {
        let ids = ipa_to_ids("hɛloʊ");
        assert_eq!(ids[0], 0);
        assert_eq!(*ids.last().unwrap(), 0);
        assert!(ids.len() > 2);
    }

    #[test]
    fn pad_char_is_zero() {
        assert_eq!(VOCAB.get(&'$').copied(), Some(0));
    }
}
