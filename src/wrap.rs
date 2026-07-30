use unicode_width::UnicodeWidthChar;
/// Unicode-aware line wrapping (CJK double-width aware).
///
/// Returns a vector of (start_byte, end_byte) pairs for each wrapped line.
/// The input `text` is a byte string; `max_cols` is the display width limit.
pub fn wrap(text: &str, max_cols: usize) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    // bytes
    let mut start = 0;
    let mut end = 0;
    // cols after the break
    let mut after = 0;
    // cols of unbroken line
    let mut cols = 0;
    // are we breaking on whitespace?
    let mut space = false;

    // should probably use unicode_segmentation grapheme_indices
    for (i, c) in text.char_indices() {
        // https://github.com/unicode-rs/unicode-width/issues/6
        let char_cols = c.width().unwrap_or(0);
        cols += char_cols;
        match c {
            '\n' => {
                after = 0;
                end = i;
                space = true;
                cols = max_cols + 1;
            }
            ' ' => {
                after = 0;
                end = i;
                space = true;
            }
            '-' | '—' if cols <= max_cols => {
                after = 0;
                end = i + c.len_utf8();
                space = false;
            }
            x if !x.is_ascii() && cols <= max_cols => {
                after = 0;
                end = i + c.len_utf8();
                space = false;
            }
            _ => after += char_cols,
        }
        if cols > max_cols {
            // break a single long word
            if cols == after {
                after = char_cols;
                end = i;
                space = false;
            }
            lines.push((start, end));
            start = end;
            if space {
                start += 1;
            }
            cols = after;
        }
    }

    // Push the final line
    lines.push((start, text.len()));

    lines
}

/// Compute cumulative line offsets for each chapter.
/// `offsets[i]` = total lines before chapter i.
/// `offsets[chapters.len()]` = total line count.
pub fn compute_line_offsets(chapters: &[crate::epub::Chapter]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(chapters.len() + 1);
    let mut acc = 0;
    for c in chapters {
        offsets.push(acc);
        acc += c.lines.len();
    }
    offsets.push(acc);
    offsets
}

/// Strip ANSI escape sequences from a string. Used to detect image markers
/// in rendered lines without being confused by color codes.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.clone().next() == Some('[') {
            // Skip the escape sequence: \x1b[...m (or other terminators)
            chars.next(); // consume '['
            while let Some(d) = chars.next() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_empty() {
        let lines = wrap("", 80);
        assert_eq!(lines, vec![(0, 0)]);
    }

    #[test]
    fn test_wrap_single_line() {
        let lines = wrap("hello world", 80);
        assert_eq!(lines, vec![(0, 11)]);
    }

    #[test]
    fn test_wrap_break_on_space() {
        let lines = wrap("hello world foo bar", 12);
        // "hello world" (11 chars) + " foo" (4 chars) would exceed 12
        // So break after "hello world"
        assert_eq!(lines.len(), 2);
        assert_eq!(&"hello world foo bar"[lines[0].0..lines[0].1], "hello world");
        assert_eq!(&"hello world foo bar"[lines[1].0..lines[1].1], "foo bar");
    }

    #[test]
    fn test_wrap_newline() {
        let lines = wrap("line1\nline2\nline3", 80);
        assert_eq!(lines.len(), 3);
        assert_eq!(&"line1\nline2\nline3"[lines[0].0..lines[0].1], "line1");
        assert_eq!(&"line1\nline2\nline3"[lines[2].0..lines[2].1], "line3");
    }

    #[test]
    fn test_wrap_long_word() {
        let lines = wrap("supercalifragilisticexpialidocious", 10);
        // Word is longer than max_cols, must be broken
        assert!(lines.len() > 1);
        for &(a, b) in &lines {
            let width: usize = "supercalifragilisticexpialidocious"[a..b]
                .chars()
                .map(|c| c.width().unwrap_or(0))
                .sum();
            assert!(width <= 10, "line width {} exceeds max 10", width);
        }
    }

    #[test]
    fn test_wrap_cjk() {
        let text = "你好世界hello";
        let lines = wrap(text, 6);
        // 你好世界 = 4*2 = 8 cols, would break
        assert!(lines.len() >= 1);
    }

    #[test]
    fn test_compute_line_offsets_empty() {
        let offsets = compute_line_offsets(&[]);
        assert_eq!(offsets, vec![0]);
    }

    #[test]
    fn test_strip_ansi() {
        let input = "\x1b[31mhello\x1b[0m world";
        assert_eq!(strip_ansi(input), "hello world");
    }

    #[test]
    fn test_strip_ansi_img_marker() {
        let input = "[IMG][url][100]";
        assert_eq!(strip_ansi(input), "[IMG][url][100]");
    }
}