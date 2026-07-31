use std::io;

use chrono::Local;
use sha2::{Digest, Sha256};

use crate::bk::Bk;

/// Compute SHA-256 hash of a file, returned as a hex string.
pub(crate) fn hash_file(path: &str) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut reader = std::fs::File::open(path)?;
    std::io::copy(&mut reader, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

/// Returns (left_section, right_section) for the status bar.
/// Caller is responsible for padding and coloring.
pub(crate) fn build_status(bk: &Bk) -> (String, String) {
    if bk.chapters.is_empty() {
        return (String::from(" 📖 (empty)"), String::new());
    }
    let chapter_lines = bk.chapters[bk.chapter].lines.len();
    let total_pages = ((chapter_lines as f32) / (bk.rows as f32)).ceil() as usize;
    let page = total_pages
        .saturating_sub(chapter_lines.saturating_sub(1).saturating_sub(bk.line) / bk.rows);

    let total_lines = *bk.chapter_line_offsets.last().unwrap_or(&1);
    let current_line = bk.chapter_line_offsets[bk.chapter] + bk.line;
    let pct = if total_lines > 0 {
        current_line * 100 / total_lines
    } else {
        0
    };

    let now = Local::now().format("%H:%M").to_string();
    let title = &bk.chapters[bk.chapter].title;
    let left = format!(" 📖 {}", title);
    let right = format!(
        "📄 {}/{} │ 📊 {}% │ 🕐 {} ",
        page, total_pages, pct, now
    );
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bk;

    #[test]
    fn test_build_status_empty() {
        let bk = Bk::default_for_test();
        let (left, right) = build_status(&bk);
        assert!(left.contains("(empty)"), "left: {:?}", left);
        assert!(right.is_empty());
    }

    #[test]
    fn test_build_status_with_chapters() {
        let ch = crate::epub::Chapter {
            title: "Chapter 1".into(),
            text: "hello world foo bar baz".into(),
            lines: vec![(0, 25)],
            attrs: vec![],
            color_attrs: vec![],
            state: Default::default(),
            links: vec![],
            heading_spans: vec![],
            frag: vec![],
        };
        let mut bk = Bk::default_for_test();
        bk.chapters = vec![ch];
        bk.chapter_line_offsets = vec![0, 1];
        bk.rows = 24;

        let (left, right) = build_status(&bk);
        assert!(left.contains("Chapter 1"), "left: {:?}", left);
        assert!(right.contains("📄"), "right: {:?}", right);
    }

    #[test]
    fn test_hash_file() {
        // hash_file should return a SHA-256 hex string
        let hash = hash_file("test/test.epub").expect("failed to hash test.epub");
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "hash should be hex: {:?}", hash);
    }

    #[test]
    fn test_hash_file_not_found() {
        let result = hash_file("nonexistent.epub");
        assert!(result.is_err());
    }
}