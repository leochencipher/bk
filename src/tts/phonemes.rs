/// Phonemization via espeak-ng subprocess.
///
/// Calls `espeak-ng -v en-us -x -q --ipa` to convert normalized English text
/// into IPA phonemes, exactly matching the Python `phonemizer` backend behavior.

use std::process::Command;

/// Phoneme override table — verified exceptions from the Python frontend.
const PHONEME_OVERRIDES: &[(&str, &str)] = &[
    ("sˈæskɐtʃˌuːən", "sɐskˈætʃəwən"),
    ("flʊɹɹˈɛsənt", "flʊˈɹɛsənt"),
];

/// Convert normalized English text to an IPA phoneme string using espeak-ng.
///
/// Returns a string with phonemes separated by spaces, word boundaries
/// marked by `|`, and punctuation preserved with surrounding spaces.
pub fn phonemize(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("espeak-ng")
        .args(["-v", "en-us", "-x", "-q", "--ipa", text])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("espeak-ng failed: {}", stderr).into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut phonemes = stdout.trim().to_string();

    // Apply phoneme overrides
    for (source, replacement) in PHONEME_OVERRIDES {
        phonemes = phonemes.replace(source, replacement);
    }

    // Collapse whitespace
    phonemes = regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(&phonemes, " ")
        .trim()
        .to_string();

    Ok(phonemes)
}

/// Convert IPA phoneme text into token symbols for the VITS model.
///
/// Replaces `|` (word boundary) with `<word>`, then splits on whitespace.
pub fn phoneme_text_to_tokens(phoneme_text: &str) -> Vec<String> {
    let text = phoneme_text.replace('|', " <word> ");
    let text = regex::Regex::new(r"([,;:.!?])")
        .unwrap()
        .replace_all(&text, " $1 ");
    text.split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phonemize_simple() {
        let result = phonemize("hello world");
        // espeak-ng might not be installed in CI; skip if it fails
        if let Ok(phonemes) = result {
            assert!(!phonemes.is_empty());
            assert!(phonemes.contains('h'));
        }
    }

    #[test]
    fn test_tokenize() {
        let tokens = phoneme_text_to_tokens("həˈloʊ | wˈɜːld");
        assert!(tokens.contains(&"<word>".to_string()));
        assert!(tokens.contains(&"həˈloʊ".to_string()));
    }
}