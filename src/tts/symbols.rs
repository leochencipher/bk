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