/// Text normalization pipeline — port of `inflect_nano_v2_frontend.py::normalize_text`.
///
/// Transforms raw English text into a pronunciation-friendly normalized form
/// before phonemization.

use regex::Regex;

use super::numbers;

// ── static lookup tables ──────────────────────────────────────────────────

const MONTHS: &[&str] = &[
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];

fn word_overrides() -> Vec<(Regex, &'static str)> {
    let overrides: &[(&str, &str)] = &[
        ("Qwen3", "Qwen three"),
        ("Qwen", "Qwen"),
        ("PyTorch", "pie torch"),
        ("SQLite", "ess cue lite"),
        ("USB-C", "you ess bee see"),
        ("RTX 3060", "ar tee ex thirty sixty"),
        ("RTX 3090", "ar tee ex thirty ninety"),
        ("RTX 4090", "ar tee ex forty ninety"),
        ("RTX 5080", "ar tee ex fifty eighty"),
        ("RTX 5090", "ar tee ex fifty ninety"),
    ];
    overrides
        .iter()
        .map(|(src, dst)| (Regex::new(&format!(r"\b{}\b", regex::escape(src))).unwrap(), *dst))
        .collect()
}

fn abbreviations() -> Vec<(Regex, &'static str)> {
    let abbrs: &[(&str, &str)] = &[
        ("Dr\\.", "doctor"),
        ("Mr\\.", "mister"),
        ("Mrs\\.", "missus"),
        ("Ms\\.", "miss"),
        ("Prof\\.", "professor"),
        ("St\\.", "saint"),
        ("vs\\.", "versus"),
        ("etc\\.", "et cetera"),
        ("e\\.g\\.", "for example"),
        ("i\\.e\\.", "that is"),
    ];
    abbrs
        .iter()
        .map(|(src, dst)| (Regex::new(&format!(r"(?i)\b{}", src)).unwrap(), *dst))
        .collect()
}

const LETTER_NAMES: &[(&str, &str)] = &[
    ("A", "ay"), ("B", "bee"), ("C", "see"), ("D", "dee"), ("E", "ee"),
    ("F", "eff"), ("G", "gee"), ("H", "aitch"), ("I", "eye"), ("J", "jay"),
    ("K", "kay"), ("L", "ell"), ("M", "em"), ("N", "en"), ("O", "oh"),
    ("P", "pee"), ("Q", "cue"), ("R", "ar"), ("S", "ess"), ("T", "tee"),
    ("U", "you"), ("V", "vee"), ("W", "double you"), ("X", "ex"),
    ("Y", "why"), ("Z", "zee"),
];

// ── punctuation normalization ──────────────────────────────────────────────

fn normalize_punctuation(text: &str) -> String {
    text.replace('\u{2018}', "'")   // left single quote → apostrophe
        .replace('\u{2019}', "'")   // right single quote → apostrophe
        .replace('\u{201c}', "")    // left double quote → remove
        .replace('\u{201d}', "")    // right double quote → remove
        .replace('"', "")           // straight double quote → remove
        .replace('\u{2013}', "-")
        .replace('\u{2014}', ", ")
        .replace('\u{2026}', "...")
        .replace('(', ", ")
        .replace(')', ", ")
        .replace('[', ", ")
        .replace(']', ", ")
        .replace('{', ", ")
        .replace('}', ", ")
}

// ── regex-based expanders ──────────────────────────────────────────────────

fn expand_acronym(text: &str) -> String {
    let re = Regex::new(r"\b[A-Z]{2,}\b").unwrap();
    re.replace_all(text, |caps: &regex::Captures| {
        let acronym = &caps[0];
        acronym
            .chars()
            .map(|c| {
                let s = c.to_string();
                LETTER_NAMES
                    .iter()
                    .find(|(l, _)| *l == s)
                    .map(|(_, name)| name.to_string())
                    .unwrap_or(s)
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
    .to_string()
}

fn expand_number_inner(caps: &regex::Captures) -> String {
    let value: u64 = caps[0].replace(',', "").parse().unwrap_or(0);
    numbers::int_to_words(value)
}

fn expand_ordinal_inner(caps: &regex::Captures) -> String {
    let value: u64 = caps[1].parse().unwrap_or(0);
    numbers::int_to_ordinal(value)
}

fn expand_decimal_inner(caps: &regex::Captures) -> String {
    let whole: u64 = caps[1].parse().unwrap_or(0);
    let frac = &caps[2];
    format!("{} point {}", numbers::int_to_words(whole), numbers::digit_words(frac))
}

fn expand_version_inner(caps: &regex::Captures) -> String {
    caps[0]
        .split('.')
        .map(|part| numbers::int_to_words(part.parse().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(" point ")
}

fn expand_phone_inner(caps: &regex::Captures) -> String {
    let left = numbers::digit_words(&caps[1]);
    let right = numbers::digit_words(&caps[2]);
    format!("{}, {}", left, right)
}

fn expand_time_inner(caps: &regex::Captures) -> String {
    let hour: u32 = caps[1].parse().unwrap_or(0);
    let minute: u32 = caps[2].parse().unwrap_or(0);
    let ampm = caps.get(3).map(|m| m.as_str().to_lowercase()).unwrap_or_default();

    let mut pieces = vec![
        if minute == 0 {
            format!("{} o'clock", numbers::int_to_words(hour as u64))
        } else {
            format!(
                "{} {}",
                numbers::int_to_words(hour as u64),
                numbers::int_to_words(minute as u64),
            )
        },
    ];

    if !ampm.is_empty() {
        let clean: String = ampm.chars().filter(|c| c.is_alphabetic()).collect();
        if !clean.is_empty() {
            pieces.push(clean);
        }
    }

    pieces.join(" ")
}

fn expand_bare_hour_time_inner(caps: &regex::Captures) -> String {
    let hour: u32 = caps[1].parse().unwrap_or(0);
    let suffix: String = caps[2]
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect::<String>()
        .to_lowercase();
    let suffix_words: String = suffix
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{} {}", numbers::int_to_words(hour as u64), suffix_words)
}

fn expand_date_slash_inner(caps: &regex::Captures) -> String {
    let month: usize = caps[1].parse().unwrap_or(1);
    let day: u32 = caps[2].parse().unwrap_or(1);
    let year: u32 = caps[3].parse().unwrap_or(2000);

    let month_name = if month >= 1 && month <= 12 {
        MONTHS[month - 1]
    } else {
        MONTHS[0]
    };

    format!(
        "{} {} {}",
        month_name,
        numbers::int_to_ordinal(day as u64),
        numbers::int_to_words(year as u64),
    )
}

fn expand_money_inner(caps: &regex::Captures) -> String {
    let raw = caps[1].replace(',', "");
    let value: f64 = raw.parse().unwrap_or(0.0);

    let dollars = value.trunc() as u64;
    let cents = ((value.fract() * 100.0).round()) as u64;

    let mut parts = vec![
        numbers::int_to_words(dollars),
        if dollars == 1 { "dollar" } else { "dollars" }.into(),
    ];

    if cents > 0 {
        parts.push("and".into());
        parts.push(numbers::int_to_words(cents));
        parts.push(if cents == 1 { "cent" } else { "cents" }.into());
    }

    parts.join(" ")
}

fn identifier_digits(text: &str) -> String {
    let mut words = Vec::new();
    let mut digit_buf = String::new();

    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digit_buf.push(ch);
        } else {
            if !digit_buf.is_empty() {
                for d in digit_buf.chars() {
                    words.push(numbers::int_to_words(d.to_digit(10).unwrap() as u64));
                }
                digit_buf.clear();
            }
            words.push(ch.to_string());
        }
    }
    if !digit_buf.is_empty() {
        for d in digit_buf.chars() {
            words.push(numbers::int_to_words(d.to_digit(10).unwrap() as u64));
        }
    }

    words.join(" ")
}

fn expand_identifier_token(token: &str) -> String {
    let re = regex::Regex::new(r"^([A-Za-z]?)(\d+)([A-Za-z]?)$").unwrap();
    if let Some(caps) = re.captures(token) {
        let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let digits = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let suffix = caps.get(3).map(|m| m.as_str()).unwrap_or("");

        let mut pieces = Vec::new();
        if !prefix.is_empty() {
            pieces.push(prefix.to_string());
        }
        for d in digits.chars() {
            pieces.push(numbers::int_to_words(d.to_digit(10).unwrap() as u64));
        }
        if !suffix.is_empty() {
            pieces.push(suffix.to_string());
        }
        pieces.join(" ")
    } else {
        token.to_string()
    }
}

fn expand_labeled_identifier_inner(caps: &regex::Captures) -> String {
    format!("{} {}", &caps[1], expand_identifier_token(&caps[2]))
}

// ── main normalize function ────────────────────────────────────────────────

/// Normalize English text for TTS: expand numbers, acronyms, abbreviations,
/// dates, times, money, and clean punctuation.
pub fn normalize_text(text: &str) -> String {
    let mut text = normalize_punctuation(text);
    text = Regex::new(r"\s+").unwrap().replace_all(&text, " ").trim().to_string();

    // Word overrides
    for (re, replacement) in word_overrides() {
        text = re.replace_all(&text, replacement).to_string();
    }

    // Abbreviations
    for (re, replacement) in abbreviations() {
        text = re.replace_all(&text, replacement).to_string();
    }

    // "U.S.A." → "U S A"
    let acronym_dots = Regex::new(r"\b([A-Z])(?:\.([A-Z]))+\.").unwrap();
    text = acronym_dots.replace_all(&text, |caps: &regex::Captures| {
        let letters: Vec<&str> = caps[0].matches(char::is_uppercase).collect();
        letters.join(" ")
    }).to_string();

    // Labeled identifiers: "apartment 4B" → "apartment four bee"
    let labeled_id = Regex::new(r"(?i)\b(apartment|apt\.?|suite|unit|room|flight|extension|order|invoice|locker|aisle|gate)\s+([A-Za-z]?\d{1,4}[A-Za-z]?)\b").unwrap();
    text = labeled_id.replace_all(&text, |caps: &regex::Captures| {
        expand_labeled_identifier_inner(caps)
    }).to_string();

    // Street numbers: "123 North" → "one two three North"
    let street = Regex::new(r"\b(\d{3})\s+(North|South|East|West)\b").unwrap();
    text = street.replace_all(&text, |caps: &regex::Captures| {
        format!("{} {}", identifier_digits(&caps[1]), &caps[2])
    }).to_string();

    // Money: $42.99 → "forty two dollars and ninety nine cents"
    let money = Regex::new(r"\$(\d[\d,]*(?:\.\d{1,2})?)").unwrap();
    text = money.replace_all(&text, |caps: &regex::Captures| {
        expand_money_inner(caps)
    }).to_string();

    // Dates: 12/25/2024
    let date = Regex::new(r"\b(0?[1-9]|1[0-2])/(0?[1-9]|[12]\d|3[01])/(20\d{2}|19\d{2})\b").unwrap();
    text = date.replace_all(&text, |caps: &regex::Captures| {
        expand_date_slash_inner(caps)
    }).to_string();

    // Time: 3:45 PM
    let time = Regex::new(r"\b(\d{1,2}):(\d{2})\s*([AaPp]\.?\s*[Mm]\.?)?\b").unwrap();
    text = time.replace_all(&text, |caps: &regex::Captures| {
        expand_time_inner(caps)
    }).to_string();

    // Bare hour: 3 PM
    let bare_hour = Regex::new(r"\b(\d{1,2})\s*([AaPp]\.?\s*[Mm]\.?)\b").unwrap();
    text = bare_hour.replace_all(&text, |caps: &regex::Captures| {
        expand_bare_hour_time_inner(caps)
    }).to_string();

    // Phone: 555-1234
    let phone = Regex::new(r"\b(\d{3})-(\d{4})\b").unwrap();
    text = phone.replace_all(&text, |caps: &regex::Captures| {
        expand_phone_inner(caps)
    }).to_string();

    // Version: 1.2.3
    let version = Regex::new(r"\b\d+(?:\.\d+){2,}\b").unwrap();
    text = version.replace_all(&text, |caps: &regex::Captures| {
        expand_version_inner(caps)
    }).to_string();

    // Decimal: 3.14
    let decimal = Regex::new(r"\b(\d+)\.(\d+)\b").unwrap();
    text = decimal.replace_all(&text, |caps: &regex::Captures| {
        expand_decimal_inner(caps)
    }).to_string();

    // Ordinal: 42nd
    let ordinal = Regex::new(r"(?i)\b(\d+)(st|nd|rd|th)\b").unwrap();
    text = ordinal.replace_all(&text, |caps: &regex::Captures| {
        expand_ordinal_inner(caps)
    }).to_string();

    // Plain numbers
    let number = Regex::new(r"\b\d[\d,]*\b").unwrap();
    text = number.replace_all(&text, |caps: &regex::Captures| {
        expand_number_inner(caps)
    }).to_string();

    // Acronyms: NASA → "en ay ess ay"
    text = expand_acronym(&text);

    // Cleanup: collapse multiple commas, fix comma before punctuation, etc.
    text = Regex::new(r",(?:\s*,)+").unwrap().replace_all(&text, ",").to_string();
    text = Regex::new(r",\s*([.!?])").unwrap().replace_all(&text, "$1").to_string();
    text = Regex::new(r"\s+([,;:.!?])").unwrap().replace_all(&text, "$1").to_string();
    text = Regex::new(r"([,;:.!?])(\S)").unwrap().replace_all(&text, "$1 $2").to_string();
    text = Regex::new(r"\s+").unwrap().replace_all(&text, " ").trim().to_string();

    text
}

// ── text splitting for long-form synthesis ─────────────────────────────────

/// Split text into chunks suitable for the VITS duration model (max ~280 chars).
/// Respects sentence boundaries where possible.
pub fn split_text(text: &str) -> Vec<String> {
    let normalized = Regex::new(r"\s+").unwrap().replace_all(text, " ").to_string();
    // Split on sentence-ending punctuation followed by whitespace.
    // Use a sentinel to preserve the punctuation character.
    let with_sentinel = Regex::new(r"([.!?;:])\s+").unwrap()
        .replace_all(&normalized, |caps: &regex::Captures| {
            format!("{}\x00", &caps[1])
        });
    let sentences: Vec<String> = with_sentinel
        .split('\x00')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let limit = 280;
    let mut chunks: Vec<String> = Vec::new();

    for sentence in sentences {
        let mut remaining = sentence;
        while remaining.len() > limit {
            let search = &remaining[..remaining.len().min(limit + 1)];
            let punctuation = [',', ';', ':']
                .iter()
                .map(|&mark| search.rfind(mark).map(|i| i as isize).unwrap_or(-1))
                .max()
                .unwrap_or(-1);

            let split_at = if punctuation as usize >= limit / 2 {
                punctuation as usize + 1
            } else {
                remaining[..limit + 1]
                    .rfind(' ')
                    .unwrap_or(limit)
            };

            let split_at = if split_at < limit / 2 { limit } else { split_at };

            chunks.push(remaining[..split_at].trim().to_string());
            remaining = remaining[split_at..].trim().to_string();
        }
        if !remaining.is_empty() {
            chunks.push(remaining);
        }
    }

    chunks
}

/// Return the silence duration (in seconds) after a chunk, based on its
/// final punctuation.
pub fn boundary_pause_seconds(chunk: &str) -> f32 {
    let ending = chunk.trim_end().chars().last().unwrap_or(' ');
    match ending {
        '?' => 0.28,
        '!' => 0.24,
        '.' => 0.22,
        ';' => 0.16,
        ':' => 0.13,
        ',' => 0.09,
        _ => 0.08,
    }
}

/// Apply a short fade-in/fade-out to avoid clicks at chunk boundaries.
pub fn edge_fade(waveform: &mut [f32], sample_rate: u32, milliseconds: f32) {
    let frames = ((sample_rate as f32 * milliseconds / 1000.0).round() as usize)
        .min(waveform.len() / 2);
    if frames == 0 {
        return;
    }
    for i in 0..frames {
        let ramp = i as f32 / frames as f32;
        waveform[i] *= ramp;
        let j = waveform.len() - 1 - i;
        waveform[j] *= ramp;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_simple() {
        let result = normalize_text("Hello, world!");
        assert!(result.contains("Hello"));
        assert!(!result.contains("  "));
    }

    #[test]
    fn test_normalize_number() {
        let result = normalize_text("I have 42 apples.");
        assert!(result.contains("forty two"));
    }

    #[test]
    fn test_normalize_acronym() {
        let result = normalize_text("NASA launched a rocket.");
        assert!(result.contains("en ay ess ay"));
    }

    #[test]
    fn test_split_text() {
        let chunks = split_text("Short. Sentence two.");
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_boundary_pause() {
        assert!((boundary_pause_seconds("Hello?") - 0.28).abs() < 0.001);
        assert!((boundary_pause_seconds("Hello.") - 0.22).abs() < 0.001);
    }
    #[test]
    fn test_quotes_removed() {
        // normalize_text should strip double quotes so they don't
        // produce unexpected phonemes.
        let result = normalize_text("He said, \"hello\" to me.");
        println!("normalized: {:?}", result);
        assert!(!result.contains('"'), "normalize_text should remove straight quotes");
        assert!(result.contains("hello"), "normalize_text should preserve content");

        // Curly quotes should also be removed
        let result2 = normalize_text("He said, \u{201c}hello\u{201d} to me.");
        println!("normalized curly: {:?}", result2);
        assert!(!result2.contains('"'), "normalize_text should remove curly quotes");
        assert!(!result2.contains('\u{201c}'), "normalize_text should remove left curly quote");
        assert!(!result2.contains('\u{201d}'), "normalize_text should remove right curly quote");
        assert!(result2.contains("hello"), "normalize_text should preserve content");
    }
}