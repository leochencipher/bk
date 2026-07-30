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