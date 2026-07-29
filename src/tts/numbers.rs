/// Minimal number-to-words conversion matching the Python `num2words` behavior
/// used by the Inflect frontend.
///
/// Covers: integer cardinals, ordinals, and digit-by-digit expansion.

const ONES: &[&str] = &[
    "", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
    "seventeen", "eighteen", "nineteen",
];

const TENS: &[&str] = &[
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

const ORDINAL_ONES: &[&str] = &[
    "zeroth", "first", "second", "third", "fourth", "fifth", "sixth", "seventh",
    "eighth", "ninth", "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth",
    "fifteenth", "sixteenth", "seventeenth", "eighteenth", "nineteenth",
];

const ORDINAL_TENS: &[&str] = &[
    "", "", "twentieth", "thirtieth", "fortieth", "fiftieth", "sixtieth",
    "seventieth", "eightieth", "ninetieth",
];

/// Convert an integer to its word representation.
/// e.g. 42 → "forty two", 100 → "one hundred"
pub fn int_to_words(value: u64) -> String {
    if value == 0 {
        return "zero".into();
    }
    int_to_words_impl(value)
}

fn int_to_words_impl(value: u64) -> String {
    match value {
        0 => String::new(),
        1..=19 => ONES[value as usize].into(),
        20..=99 => {
            let t = (value / 10) as usize;
            let o = (value % 10) as usize;
            if o == 0 {
                TENS[t].into()
            } else {
                format!("{} {}", TENS[t], ONES[o])
            }
        }
        100..=999 => {
            let h = (value / 100) as usize;
            let rest = value % 100;
            if rest == 0 {
                format!("{} hundred", ONES[h])
            } else {
                format!("{} hundred {}", ONES[h], int_to_words_impl(rest))
            }
        }
        1000..=999_999 => {
            let th = value / 1000;
            let rest = value % 1000;
            let thousands = if th == 1 {
                "one thousand".into()
            } else {
                format!("{} thousand", int_to_words_impl(th))
            };
            if rest == 0 {
                thousands
            } else {
                format!("{} {}", thousands, int_to_words_impl(rest))
            }
        }
        _ => {
            // For millions and above, use a simpler form.
            // The Python num2words handles these, but for our use case
            // (book text), numbers this large are rare.
            let mil = value / 1_000_000;
            let rest = value % 1_000_000;
            let millions = if mil == 1 {
                "one million".into()
            } else {
                format!("{} million", int_to_words_impl(mil))
            };
            if rest == 0 {
                millions
            } else {
                format!("{} {}", millions, int_to_words_impl(rest))
            }
        }
    }
}

/// Convert an integer to its ordinal word representation.
/// e.g. 42 → "forty second"
pub fn int_to_ordinal(value: u64) -> String {
    if value == 0 {
        return "zeroth".into();
    }
    match value {
        1..=19 => ORDINAL_ONES[value as usize].into(),
        20..=99 => {
            let t = (value / 10) as usize;
            let o = (value % 10) as usize;
            if o == 0 {
                ORDINAL_TENS[t].into()
            } else {
                format!("{} {}", TENS[t], ORDINAL_ONES[o])
            }
        }
        _ => {
            let base = int_to_words(value);
            // Simple suffix: replace "y" → "ieth", else add "th"
            if base.ends_with('y') {
                format!("{}ieth", &base[..base.len() - 1])
            } else {
                format!("{}th", base)
            }
        }
    }
}

/// Expand digits of a number string individually.
/// e.g. "42" → "four two"
pub fn digit_words(digits: &str) -> String {
    digits
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| ONES[c.to_digit(10).unwrap() as usize])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_to_words() {
        assert_eq!(int_to_words(0), "zero");
        assert_eq!(int_to_words(7), "seven");
        assert_eq!(int_to_words(19), "nineteen");
        assert_eq!(int_to_words(42), "forty two");
        assert_eq!(int_to_words(100), "one hundred");
        assert_eq!(int_to_words(101), "one hundred one");
        assert_eq!(int_to_words(1000), "one thousand");
        assert_eq!(int_to_words(2024), "two thousand twenty four");
    }

    #[test]
    fn test_int_to_ordinal() {
        assert_eq!(int_to_ordinal(1), "first");
        assert_eq!(int_to_ordinal(2), "second");
        assert_eq!(int_to_ordinal(3), "third");
        assert_eq!(int_to_ordinal(21), "twenty first");
        assert_eq!(int_to_ordinal(42), "forty second");
        assert_eq!(int_to_ordinal(100), "one hundredth");
    }

    #[test]
    fn test_digit_words() {
        assert_eq!(digit_words("42"), "four two");
        assert_eq!(digit_words("911"), "nine one one");
    }
}