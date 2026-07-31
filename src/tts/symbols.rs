use std::collections::HashMap;
use std::sync::LazyLock;

/// The VITS symbol table — must match the Python `runtime/text/symbols.py`.
/// Order matters: index 0 is the padding symbol, and the rest are indexed
/// sequentially for the model input.
const SYMBOLS: &str = concat!(
    "_",
    ";:,.!?¡¿—…\"«»“” ",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
    "ɑɐɒæɓʙβɔɕçɗɖðʤəɘɚɛɜɝɞɟʄɡɠɢʛɦɧħɥʜɨɪʝɭɬɫɮʟɱɯɰŋɳɲɴøɵɸθœɶʘɹɺɾɻʀʁɽʂʃʈʧʉʊʋⱱʌɣɤʍχʎʏʑʐʒʔʡʕʢǀǁǂǃˈˌːˑʼʴʰʱʲʷˠˤ˞↓↑→↗↘'̩'ᵻ",
);

static SYMBOL_TO_ID: LazyLock<HashMap<char, i64>> = LazyLock::new(|| {
    SYMBOLS.chars().enumerate().map(|(i, c)| (c, i as i64)).collect()
});

/// Convert a phoneme string into interleaved token IDs for the VITS duration model.
///
/// Output shape: `[1, 2 * len(phonemes) + 1]` with blanks (0) at even indices
/// and symbol IDs at odd indices.
pub fn phonemes_to_tokens(phoneme_text: &str) -> Vec<i64> {
    let chars: Vec<char> = phoneme_text.chars().collect();
    let len = chars.len() * 2 + 1;
    let mut tokens = vec![0i64; len];

    for (i, &ch) in chars.iter().enumerate() {
        let id = SYMBOL_TO_ID
            .get(&ch)
            .copied()
            .unwrap_or_else(|| {
                // Fallback: use space ID for unknown symbols
                SYMBOL_TO_ID.get(&' ').copied().unwrap_or(0)
            });
        tokens[i * 2 + 1] = id;
    }

    tokens
}

/// Number of tokens in the interleaved sequence for a phoneme string.
pub fn token_count(phoneme_text: &str) -> usize {
    phoneme_text.chars().count() * 2 + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phonemes_to_tokens_empty() {
        let tokens = phonemes_to_tokens("");
        assert_eq!(tokens, vec![0]);
    }

    #[test]
    fn test_phonemes_to_tokens_single_char() {
        // 'a' should be in the symbol table
        let tokens = phonemes_to_tokens("a");
        assert_eq!(tokens.len(), 3); // 2*1 + 1
        assert_eq!(tokens[0], 0); // blank at even index
        assert!(tokens[1] > 0); // 'a' should have a valid ID
        assert_eq!(tokens[2], 0); // trailing blank
    }

    #[test]
    fn test_phonemes_to_tokens_word() {
        let tokens = phonemes_to_tokens("hello");
        assert_eq!(tokens.len(), 11); // 2*5 + 1
        // Even indices should be blanks (0)
        for i in (0..tokens.len()).step_by(2) {
            assert_eq!(tokens[i], 0, "token[{}] should be blank", i);
        }
        // Odd indices should be non-zero (valid symbol IDs)
        for i in (1..tokens.len()).step_by(2) {
            assert!(tokens[i] > 0, "token[{}] should be a valid symbol ID", i);
        }
    }

    #[test]
    fn test_phonemes_to_tokens_unknown_symbol() {
        // Characters not in the symbol table should map to the space ID
        // Use a character that's unlikely to be in SYMBOLS, like '\u{2603}' (snowman)
        let tokens = phonemes_to_tokens("\u{2603}");
        assert_eq!(tokens.len(), 3);
        // The space ID should be used as fallback
        let space_id = SYMBOL_TO_ID.get(&' ').copied().unwrap_or(0);
        assert_eq!(tokens[1], space_id, "unknown symbol should map to space ID");
    }

    #[test]
    fn test_token_count_empty() {
        assert_eq!(token_count(""), 1);
    }

    #[test]
    fn test_token_count_single() {
        assert_eq!(token_count("a"), 3);
    }

    #[test]
    fn test_token_count_multiple() {
        assert_eq!(token_count("hello"), 11);
    }

    #[test]
    fn test_symbol_table_has_padding() {
        // First symbol should be the padding character '_'
        assert_eq!(SYMBOL_TO_ID.get(&'_'), Some(&0));
    }

    #[test]
    fn test_symbol_table_has_common_phonemes() {
        // Should have common IPA symbols
        assert!(SYMBOL_TO_ID.contains_key(&'ɑ'));
        assert!(SYMBOL_TO_ID.contains_key(&'ə'));
        assert!(SYMBOL_TO_ID.contains_key(&'ɛ'));
        assert!(SYMBOL_TO_ID.contains_key(&'ɔ'));
        assert!(SYMBOL_TO_ID.contains_key(&'θ'));
    }
}