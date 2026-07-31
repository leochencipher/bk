use crossterm::{
    event::{
        KeyCode::{self, *},
        MouseEvent, MouseEventKind,
    },
    style::{Attribute::*, Color, SetBackgroundColor, SetForegroundColor},
};
use std::cmp::{min, Ordering};
use unicode_width::UnicodeWidthChar;

use crate::{Bk, Direction, SearchArgs, TocItem, ensure_chapter_visible, rebuild_toc_visible};

pub trait View {
    fn render(&self, bk: &Bk) -> Vec<String>;
    fn on_key(&self, bk: &mut Bk, kc: KeyCode);
    fn on_mouse(&self, _: &mut Bk, _: MouseEvent) {}
    fn on_resize(&self, _: &mut Bk) {}
}

// TODO render something useful?
struct Mark;
impl View for Mark {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        if let Char(c) = kc {
            bk.mark(c)
        }
        bk.view = &Page
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Jump;
impl View for Jump {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        if let Char(c) = kc {
            if let Some(&pos) = bk.mark.get(&c) {
                bk.jump(pos);
            }
        }
        bk.view = &Page;
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Metadata;
impl View for Metadata {
    fn on_key(&self, bk: &mut Bk, _: KeyCode) {
        bk.view = &Page;
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        if bk.chapters.is_empty() {
            return vec![String::from("(empty book)")];
        }
        let lines: Vec<usize> = bk.chapters.iter().map(|c| c.lines.len()).collect();
        let current = lines[..bk.chapter].iter().sum::<usize>() + bk.line;
        let total = lines.iter().sum::<usize>();
        let progress = current as f32 / total as f32 * 100.0;

        let pages = (lines[bk.chapter] as f32 / bk.rows as f32).ceil() as usize;
        // if the last line is visible we're on the last page. first page is the short one
        let page = pages - (lines[bk.chapter] - 1 - bk.line) / bk.rows;

        let mut vec = vec![
            format!("chapter: {}/{}", page, pages),
            format!("total: {:.0}%", progress),
            format!(
                "tts: {}",
                if bk.tts_engine.is_some() { "loaded" } else { "not loaded" }
            ),
            String::new(),
        ];
        vec.extend_from_slice(&bk.meta);
        vec
    }
}

struct Help;
impl View for Help {
    fn on_key(&self, bk: &mut Bk, _: KeyCode) {
        bk.view = &Page;
    }
    fn render(&self, _: &Bk) -> Vec<String> {
        let text = r#"
                   Esc q  Quit
                      Fn  Help
                     Tab  Table of Contents
                       i  Progress and Metadata
                       B  Toggle Bionic Reading
                       F  Toggle Line Focus
                       s  Speak current sentence (TTS)

PageDown Right Space f l  Page Down
         PageUp Left b h  Page Up
                       d  Half Page Down
                       u  Half Page Up
                  Down j  Line Down
                    Up k  Line Up
                  Home g  Chapter Start
                   End G  Chapter End
                       [  Previous Chapter
                       ]  Next Chapter

                       /  Search Forward
                       ?  Search Backward
                       n  Repeat search forward
                       N  Repeat search backward
                      mx  Set mark x
                      'x  Jump to mark x

               TOC View
          Enter l Space  Expand/Collapse or Go
              Left h     Collapse parent
              Right l    Expand or Go
                   "#;

        text.lines().map(String::from).collect()
    }
}

fn toc_prefix(item: &TocItem) -> String {
    let mut s = String::new();
    for last in &item.ancestors_last {
        if *last {
            s.push_str("    ");
        } else {
            s.push_str("│   ");
        }
    }
    if item.is_last {
        s.push_str("└── ");
    } else {
        s.push_str("├── ");
    }
    s
}

pub struct Toc;
impl Toc {
    fn prev(&self, bk: &mut Bk, n: usize) {
        bk.toc_cursor = bk.toc_cursor.saturating_sub(n);
    }
    fn next(&self, bk: &mut Bk, n: usize) {
        if bk.toc_visible.is_empty() {
            return;
        }
        bk.toc_cursor = min(bk.toc_visible.len() - 1, bk.toc_cursor + n);
    }
    fn cursor(&self, bk: &mut Bk) {
        ensure_chapter_visible(bk);
        let pos = bk
            .toc_visible
            .iter()
            .position(|item| item.chapter == bk.chapter)
            .unwrap_or(0);
        bk.toc_cursor = min(bk.rows / 2, pos);
    }
    fn toggle(&self, bk: &mut Bk) {
        if bk.toc_visible.is_empty() {
            return;
        }
        let item = &bk.toc_visible[bk.toc_cursor];
        if !item.has_children {
            bk.chapter = item.chapter;
            bk.line = 0;
            bk.view = &Page;
            return;
        }
        let idx = item.toc_idx;
        bk.toc_expanded[idx] = !bk.toc_expanded[idx];
        bk.toc_visible =
            rebuild_toc_visible(&bk.toc_tree, &bk.toc_expanded, &bk.path_to_chapter);
        bk.toc_cursor = min(bk.toc_cursor, bk.toc_visible.len().saturating_sub(1));
    }
    fn collapse_parent(&self, bk: &mut Bk) {
        if bk.toc_visible.is_empty() {
            return;
        }
        let item = &bk.toc_visible[bk.toc_cursor];
        if item.depth == 0 {
            return;
        }
        for i in (0..bk.toc_cursor).rev() {
            if bk.toc_visible[i].depth < item.depth {
                bk.toc_expanded[bk.toc_visible[i].toc_idx] = false;
                bk.toc_visible =
                    rebuild_toc_visible(&bk.toc_tree, &bk.toc_expanded, &bk.path_to_chapter);
                bk.toc_cursor = min(i, bk.toc_visible.len().saturating_sub(1));
                return;
            }
        }
    }
    fn click(&self, bk: &mut Bk, row: usize) {
        let idx = bk.toc_cursor + row;
        if idx < bk.toc_visible.len() {
            bk.chapter = bk.toc_visible[idx].chapter;
            bk.line = 0;
            bk.view = &Page;
        }
    }
}
impl View for Toc {
    fn on_resize(&self, bk: &mut Bk) {
        self.cursor(bk);
    }
    fn on_mouse(&self, bk: &mut Bk, e: MouseEvent) {
        match e.kind {
            MouseEventKind::Down(_) => self.click(bk, e.row as usize),
            MouseEventKind::ScrollDown => self.next(bk, 3),
            MouseEventKind::ScrollUp => self.prev(bk, 3),
            _ => {
                bk.dirty = false;
            }
        }
    }
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc | Tab | Char('q') => {
                bk.jump_reset();
                bk.toc_cursor = 0;
                bk.view = &Page;
            }
            Left | Char('h') => {
                if bk.toc_visible.is_empty() {
                    return;
                }
                if bk.toc_visible[bk.toc_cursor].depth == 0 {
                    bk.jump_reset();
                    bk.toc_cursor = 0;
                    bk.view = &Page;
                } else {
                    self.collapse_parent(bk);
                }
            }
            Enter | Right | Char('l' | ' ') => self.toggle(bk),
            Down | Char('j') => self.next(bk, 1),
            Up | Char('k') => self.prev(bk, 1),
            Home | Char('g') => self.prev(bk, bk.toc_visible.len()),
            End | Char('G') => self.next(bk, bk.toc_visible.len()),
            PageDown | Char('f') => self.next(bk, bk.rows),
            PageUp | Char('b') => self.prev(bk, bk.rows),
            Char('d') => self.next(bk, bk.rows / 2),
            Char('u') => self.prev(bk, bk.rows / 2),
            _ => {
                bk.dirty = false;
            }
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        if bk.toc_visible.is_empty() {
            return vec![String::from("(empty book)")];
        }
        let start = bk.toc_cursor.saturating_sub(bk.rows / 2);
        let end = min(bk.toc_visible.len(), start + bk.rows);
        let actual_start = if end - start < bk.rows && start > 0 {
            start.saturating_sub(bk.rows - (end - start))
        } else {
            start
        };
        let actual_end = min(bk.toc_visible.len(), actual_start + bk.rows);

        let mut arr = Vec::new();
        for (i, item) in bk.toc_visible[actual_start..actual_end].iter().enumerate() {
            let prefix = toc_prefix(item);
            let indicator = if item.has_children {
                if item.is_expanded {
                    "▼ "
                } else {
                    "▶ "
                }
            } else {
                "  "
            };
            let line = format!("{}{}{}", prefix, indicator, item.title);
            if actual_start + i == bk.toc_cursor {
                arr.push(format!("{}{}{}", Reverse, line, NoReverse));
            } else {
                arr.push(line);
            }
        }
        arr
    }
}

pub struct Page;
impl Page {
    fn next_chapter(&self, bk: &mut Bk) {
        if bk.chapters.is_empty() {
            return;
        }
        if bk.chapter < bk.chapters.len() - 1 {
            bk.chapter += 1;
            bk.line = 0;
        }
    }
    fn prev_chapter(&self, bk: &mut Bk) {
        if bk.chapter > 0 {
            bk.chapter -= 1;
            bk.line = 0;
        }
    }
    fn scroll_down(&self, bk: &mut Bk, n: usize) {
        if bk.chapters.is_empty() {
            return;
        }
        if bk.line + bk.rows < bk.chapters[bk.chapter].lines.len() {
            bk.line += n;
        } else {
            self.next_chapter(bk);
        }
    }
    fn scroll_up(&self, bk: &mut Bk, n: usize) {
        if bk.chapters.is_empty() {
            return;
        }
        if bk.line > 0 {
            bk.line = bk.line.saturating_sub(n);
        } else if bk.chapter > 0 {
            bk.chapter -= 1;
            bk.line = bk.chapters[bk.chapter].lines.len().saturating_sub(bk.rows);
        }
    }
    fn click(&self, bk: &mut Bk, e: MouseEvent) {
        if bk.chapters.is_empty() {
            return;
        }
        let c = &bk.chapters[bk.chapter];
        let line = bk.line + e.row as usize;

        if line >= c.lines.len() {
            return;
        }
        let (start, end) = c.lines[line];
        let line_col = (e.column - 5) as usize;

        let mut cols = 0;
        let mut found = false;
        let mut byte = start;
        for (i, c) in c.text[start..end].char_indices() {
            cols += c.width().unwrap();
            if cols > line_col {
                byte += i;
                found = true;
                break;
            }
        }
        if !found {
            return;
        }

        let r = c.links.binary_search_by(|&(start, end, _)| {
            if start > byte {
                Ordering::Greater
            } else if end <= byte {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });

        if let Ok(i) = r {
            let url = &c.links[i].2;
            let &(c, byte) = bk.links.get(url).unwrap();
            bk.mark('\'');
            bk.jump_byte(c, byte);
        }
    }
    fn start_search(&self, bk: &mut Bk, dir: Direction) {
        bk.mark('\'');
        bk.query.clear();
        bk.dir = dir;
        bk.view = &Search;
    }
}
impl View for Page {
    fn on_mouse(&self, bk: &mut Bk, e: MouseEvent) {
        match e.kind {
            MouseEventKind::Down(_) => self.click(bk, e),
            MouseEventKind::ScrollDown => self.scroll_down(bk, 3),
            MouseEventKind::ScrollUp => self.scroll_up(bk, 3),
            _ => { bk.dirty = false; }
        }
    }
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc | Char('q') => bk.quit = true,
            Tab => {
                bk.mark('\'');
                Toc.cursor(bk);
                bk.view = &Toc;
            }
            F(_) => bk.view = &Help,
            Char('T') => bk.next_theme(),
            Char('m') => bk.view = &Mark,
            Char('\'') => bk.view = &Jump,
            Char('i') => bk.view = &Metadata,
            Char('?') => self.start_search(bk, Direction::Prev),
            Char('/') => self.start_search(bk, Direction::Next),
            Char('N') => {
                bk.search(SearchArgs {
                    dir: Direction::Prev,
                    skip: true,
                });
            }
            Char('n') => {
                bk.search(SearchArgs {
                    dir: Direction::Next,
                    skip: true,
                });
            }
            End | Char('G') => {
                if bk.chapters.is_empty() {
                    return;
                }
                bk.mark('\'');
                bk.line = bk.chapters[bk.chapter].lines.len().saturating_sub(bk.rows);
            }
            Home | Char('g') => {
                bk.mark('\'');
                bk.line = 0;
            }
            Char('d') => self.scroll_down(bk, bk.rows / 2),
            Char('u') => self.scroll_up(bk, bk.rows / 2),
            Up | Char('k') => self.scroll_up(bk, 3),
            Left | PageUp | Char('b' | 'h') => {
                self.scroll_up(bk, bk.rows);
            }
            Down | Char('j') => self.scroll_down(bk, 3),
            Right | PageDown | Char('f' | 'l' | ' ') => self.scroll_down(bk, bk.rows),
            Char('[') => self.prev_chapter(bk),
            Char(']') => self.next_chapter(bk),
            Char('B') => { bk.bionic = !bk.bionic; },
            Char('F') => { bk.focus = !bk.focus; },
            Char('s') => {
                if bk.tts_engine.is_some() && !bk.chapters.is_empty() {
                    let chapter = &bk.chapters[bk.chapter];
                    let line_start = chapter.lines.get(bk.line).map(|r| r.0).unwrap_or(0);
                    bk.tts_sentences = split_into_sentences(&chapter.text);
                    bk.tts_sentence_idx = find_sentence_index(&bk.tts_sentences, &chapter.text, line_start);
                    bk.tts_active = true;
                    bk.view = &TtsView;
                }
            }
            _ => { bk.dirty = false; }
        }
    }
    fn on_resize(&self, bk: &mut Bk) {
        if bk.chapters.is_empty() {
            return;
        }
        // lazy
        bk.line = min(bk.line, bk.chapters[bk.chapter].lines.len() - 1);
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        if bk.chapters.is_empty() {
            return vec![String::from("(empty book)")];
        }
        let c = &bk.chapters[bk.chapter];
        let last_line = min(bk.line + bk.rows, c.lines.len());
        let text_start = c.lines[bk.line].0;
        let text_end = c.lines[last_line - 1].1;

        // ── search highlights ──
        let search_attrs: Vec<(usize, String)> = {
            let mut v = Vec::new();
            if !bk.query.is_empty() {
                let len = bk.query.len();
                let hl_on =
                    SetBackgroundColor(bk.theme.search_highlight).to_string();
                let hl_off = format!(
                    "{}{}",
                    SetForegroundColor(bk.colors.foreground.unwrap_or(Color::Reset)),
                    SetBackgroundColor(bk.colors.background.unwrap_or(Color::Reset))
                );
                for (pos, _) in c.text[text_start..text_end].match_indices(&bk.query) {
                    v.push((text_start + pos, hl_on.clone()));
                    v.push((text_start + pos + len, hl_off.clone()));
                }
            }
            v
        };

        // ── base formatting ──
        let base_attrs: Vec<(usize, String)> = {
            let start = match c.attrs.binary_search_by_key(&text_start, |&x| x.0) {
                Ok(n) => n,
                Err(n) => n - 1,
            };

            let map = c.attrs[start].2;
            let mut head: Vec<(usize, String)> = Vec::new();
            for attr in [Bold, Italic, Underlined] {
                if map.has(attr) {
                    head.push((text_start, attr.to_string()));
                }
            }
            let tail = c
                .attrs[start + 1..]
                .iter()
                .take_while(|x| x.0 <= text_end)
                .map(|x| (x.0, x.1.to_string()));
            let colors = c
                .color_attrs
                .iter()
                .filter(|(pos, _)| *pos >= text_start && *pos <= text_end)
                .cloned();
            let heading_colors: Vec<(usize, String)> = c
                .heading_spans
                .iter()
                .filter(|(s, e, _)| *s <= text_end && *e >= text_start)
                .flat_map(|(s, e, level)| {
                    let on =
                        SetForegroundColor(bk.theme.heading_colors[*level]).to_string();
                    let off = SetForegroundColor(bk.theme.fg).to_string();
                    vec![(*s, on), (*e, off)]
                })
                .collect();
            let mut all: Vec<(usize, String)> =
                head.into_iter().chain(tail).chain(colors).chain(heading_colors).collect();
            all.sort_by_key(|x| x.0);
            all
        };

        // ── bionic reading (pivot letter color) ──
        let bionic_attrs: Vec<(usize, String)> = {
            let mut v = Vec::new();
            if bk.bionic {
                let bionic_on = SetForegroundColor(bk.theme.bionic_fg).to_string();
                let bionic_off = SetForegroundColor(bk.theme.fg).to_string();
                let mut byte = text_start;
                while byte < text_end {
                    let ch = c.text.as_bytes()[byte];
                    if ch.is_ascii_whitespace() || ch.is_ascii_punctuation() {
                        byte += 1;
                        continue;
                    }
                    let word_start = byte;
                    let mut letter_count = 0usize;
                    while byte < text_end {
                        let ch = c.text.as_bytes()[byte];
                        if ch.is_ascii_whitespace() {
                            break;
                        }
                        if ch.is_ascii_alphabetic() || ch.is_ascii_digit() {
                            letter_count += 1;
                        }
                        byte += 1;
                    }
                    if letter_count == 0 {
                        continue;
                    }
                    // RSVP pivot: find the pivot letter index within the word
                    let pivot_offset = match letter_count {
                        1 => 0,
                        2..=5 => 1,
                        6..=9 => 2,
                        10..=13 => 3,
                        _ => 4,
                    };
                    // Walk forward from word_start to find the pivot letter
                    let mut pivot_pos = word_start;
                    let mut counted = 0;
                    while pivot_pos < byte && counted < pivot_offset {
                        let ch = c.text.as_bytes()[pivot_pos];
                        if ch.is_ascii_alphabetic() || ch.is_ascii_digit() {
                            counted += 1;
                        }
                        pivot_pos += 1;
                    }
                    // Color just the pivot character (handle multi-byte UTF-8)
                    let pivot_len = c.text[pivot_pos..]
                        .chars()
                        .next()
                        .map(|ch| ch.len_utf8())
                        .unwrap_or(1);
                    let pivot_end = pivot_pos + pivot_len;
                    v.push((pivot_pos, bionic_on.clone()));
                    v.push((pivot_end, bionic_off.clone()));
                }
            }
            v
        };

        // ── merge all attribute streams ──
        let mut sorted: Vec<(usize, String)> = search_attrs
            .into_iter()
            .chain(base_attrs)
            .chain(bionic_attrs)
            .collect();
        sorted.sort_by_key(|x| x.0);
        let mut attrs = sorted.into_iter().peekable();

        // ── build lines ──
        let mut buf = Vec::with_capacity(last_line - bk.line);
        for &(mut pos, line_end) in &c.lines[bk.line..last_line] {
            let mut s = String::new();
            while let Some((attr_pos, attr)) = attrs.next_if(|a| a.0 <= line_end) {
                if attr_pos >= pos {
                    s.push_str(&c.text[pos..attr_pos]);
                }
                s.push_str(&attr);
                if attr_pos >= pos {
                    pos = attr_pos;
                }
            }
            s.push_str(&c.text[pos..line_end]);
            buf.push(s);
        }

        // ── line focus ──
        if bk.focus && !buf.is_empty() {
            let focus_line = buf.len() / 2;
            let dim_on = Dim.to_string();
            let dim_off = NormalIntensity.to_string();
            for (i, line) in buf.iter_mut().enumerate() {
                if i != focus_line {
                    *line = format!("{}{}{}", dim_on, line, dim_off);
                }
            }
        }

        buf
    }
}

pub struct Search;
impl View for Search {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc => {
                bk.jump_reset();
                bk.query.clear();
                bk.view = &Page;
            }
            Enter => {
                bk.view = &Page;
            }
            Backspace => {
                bk.query.pop();
                bk.jump_reset();
                bk.search(SearchArgs {
                    dir: bk.dir.clone(),
                    skip: false,
                });
            }
            Char(c) => {
                bk.query.push(c);
                let args = SearchArgs {
                    dir: bk.dir.clone(),
                    skip: false,
                };
                bk.search(args);
            }
            _ => {}
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let mut buf = Page::render(&Page, bk);
        if buf.len() == bk.rows {
            buf.pop();
        } else {
            for _ in buf.len()..bk.rows - 1 {
                buf.push(String::new());
            }
        }
        let prefix = match bk.dir {
            Direction::Next => '/',
            Direction::Prev => '?',
        };
        buf.push(format!("{}{}", prefix, bk.query));
        buf
    }
}

/// Normalize text for TTS: collapse whitespace, strip quotes and noise chars.
/// Returns (normalized_string, byte_map) where byte_map[i] = original byte position
/// for each char in the normalized string (or None for inserted spaces).
fn normalize_for_tts(text: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len());
    let mut in_ws = false;
    for (byte_pos, ch) in text.char_indices() {
        match ch {
            // Strip all double-quote-like characters
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}'
            | '\u{00AB}' | '\u{00BB}'
            | '"' => continue,
            ' ' | '\n' | '\r' | '\t' => {
                if !in_ws {
                    out.push(' ');
                    map.push(byte_pos);
                    in_ws = true;
                }
            }
            _ => {
                out.push(ch);
                map.push(byte_pos);
                in_ws = false;
            }
        }
    }
    (out.trim().to_string(), map)
}

/// Split text into TTS chunks: normalize whitespace, split on sentence boundaries,
/// then merge short sentences into longer chunks for natural speech.
fn split_into_sentences(text: &str) -> Vec<String> {
    let (normalized, _) = normalize_for_tts(text);
    if normalized.is_empty() {
        return Vec::new();
    }

    // 2. Split into raw sentences at .!? followed by space/end
    let bytes = normalized.as_bytes();
    let mut raw: Vec<String> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'.' | b'!' | b'?') {
            let after = i + 1;
            if after >= bytes.len() || bytes[after] == b' ' {
                let s = normalized[start..=i].to_string();
                if !s.is_empty() {
                    raw.push(s);
                }
                start = after + 1; // skip punctuation + space
                i = start;
                continue;
            }
        }
        i += 1;
    }
    if start < bytes.len() {
        let s = normalized[start..].to_string();
        if !s.is_empty() {
            raw.push(s);
        }
    }

    // 3. Merge short sentences into longer chunks (target ~120 chars, max ~280)
    let target = 120usize;
    let max_chunk = 280usize;
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for s in raw {
        if cur.is_empty() {
            cur = s;
        } else if cur.len() + 1 + s.len() <= max_chunk && cur.len() < target {
            cur.push(' ');
            cur.push_str(&s);
        } else {
            chunks.push(cur);
            cur = s;
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }

    chunks
}

/// Find which chunk index contains `byte_pos` in the original `text`.
fn find_sentence_index(chunks: &[String], text: &str, byte_pos: usize) -> usize {
    let (normalized, map) = normalize_for_tts(text);

    // Find the normalized position corresponding to byte_pos
    let norm_pos = map.iter().position(|&bp| bp >= byte_pos).unwrap_or(map.len());

    // Search for each chunk in the normalized text to find the containing one
    let mut search_from = 0usize;
    for (i, chunk) in chunks.iter().enumerate() {
        if let Some(pos) = normalized[search_from..].find(chunk.as_str()) {
            let abs_pos = search_from + pos;
            let chunk_end = abs_pos + chunk.len();
            if norm_pos >= abs_pos && norm_pos <= chunk_end {
                return i;
            }
            search_from = chunk_end + 1;
        }
    }
    chunks.len().saturating_sub(1)
}

// ── TTS View ────────────────────────────────────────────────────────────────

pub struct TtsView;

impl View for TtsView {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Char('s') | Esc => {
                // Kill afplay if running
                if let Some(child) = &mut bk.tts_child {
                    let _ = child.kill();
                }
                bk.tts_child = None;
                bk.tts_active = false;
                bk.view = &Page;
            }
            _ => {}
        }
    }

    fn render(&self, bk: &Bk) -> Vec<String> {
        if bk.tts_sentences.is_empty() {
            return vec![
                String::new(),
                String::from("  (no text to speak)"),
                String::new(),
                String::from("  Press s or Esc to exit"),
            ];
        }

        let current = &bk.tts_sentences[bk.tts_sentence_idx];
        let total = bk.tts_sentences.len();
        let idx_display = bk.tts_sentence_idx + 1;
        let max_width = (bk.cols as usize).saturating_sub(2);

        let mut vec = Vec::new();
        vec.push(String::new());

        // Progress line
        vec.push(format!("  \u{1f5e3}  Sentence {} of {}", idx_display, total));
        vec.push(String::new());

        // Current sentence
        let cur_wrapped = crate::wrap(current, max_width);
        for &(a, b) in &cur_wrapped {
            vec.push(format!("  {}", &current[a..b]));
        }
        vec.push(String::new());

        // Next sentence preview
        let next_idx = bk.tts_sentence_idx + 1;
        if next_idx < total {
            let next = &bk.tts_sentences[next_idx];
            let buffered = bk.tts_buffer.as_ref().map_or(false, |(i, _)| *i == next_idx);
            let label = if buffered { "  \u{23f3} Next (buffered):" } else { "  \u{23f3} Next (buffering...):" };
            vec.push(label.to_string());
            // Show first line of next sentence as preview
            let next_wrapped = crate::wrap(next, max_width);
            if let Some(&(a, b)) = next_wrapped.first() {
                let preview = &next[a..b];
                if next_wrapped.len() > 1 {
                    vec.push(format!("  {}...", preview));
                } else {
                    vec.push(format!("  {}", preview));
                }
            }
        }

        // Fill remaining space and add footer
        let used = vec.len();
        let footer = String::from("  Press s or Esc to exit TTS mode");
        let footer_line = if bk.rows > used + 1 { bk.rows - 1 } else { used };
        for _ in used..footer_line {
            vec.push(String::new());
        }
        vec.push(footer);

        vec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bk, Direction, SearchArgs, TocItem};

    #[test]
    fn test_toc_prefix_root_first() {
        let item = TocItem {
            title: "Ch1".into(),
            chapter: 0,
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_last: false,
            ancestors_last: vec![],
            toc_idx: 0,
        };
        assert_eq!(toc_prefix(&item), "\u{251c}\u{2500}\u{2500} ");
    }

    #[test]
    fn test_toc_prefix_root_last() {
        let item = TocItem {
            title: "Ch1".into(),
            chapter: 0,
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_last: true,
            ancestors_last: vec![],
            toc_idx: 0,
        };
        assert_eq!(toc_prefix(&item), "\u{2514}\u{2500}\u{2500} ");
    }

    #[test]
    fn test_toc_prefix_nested() {
        let item = TocItem {
            title: "Sub".into(),
            chapter: 1,
            depth: 1,
            has_children: false,
            is_expanded: false,
            is_last: true,
            ancestors_last: vec![false],
            toc_idx: 1,
        };
        assert_eq!(toc_prefix(&item), "\u{2502}   \u{2514}\u{2500}\u{2500} ");
    }

    #[test]
    fn test_toc_prefix_deeply_nested() {
        let item = TocItem {
            title: "Deep".into(),
            chapter: 2,
            depth: 2,
            has_children: false,
            is_expanded: false,
            is_last: false,
            ancestors_last: vec![false, true],
            toc_idx: 2,
        };
        assert_eq!(toc_prefix(&item), "\u{2502}       \u{251c}\u{2500}\u{2500} ");
    }

    #[test]
    fn test_normalize_for_tts_collapses_whitespace() {
        let (text, map) = normalize_for_tts("hello   world\n\nfoo");
        assert_eq!(text, "hello world foo");
        assert_eq!(map.len(), text.len());
    }

    #[test]
    fn test_normalize_for_tts_strips_quotes() {
        let (text, _) = normalize_for_tts("he said \"hello\" to me");
        assert_eq!(text, "he said hello to me");
    }

    #[test]
    fn test_normalize_for_tts_strips_curly_quotes() {
        let (text, _) = normalize_for_tts("\u{201C}hello\u{201D}");
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_normalize_for_tts_byte_map() {
        let (text, map) = normalize_for_tts("a b");
        assert_eq!(text, "a b");
        // Each char maps to its original byte position
        assert_eq!(map[0], 0); // 'a'
        assert_eq!(map[1], 1); // ' '
        assert_eq!(map[2], 2); // 'b'
    }

    #[test]
    fn test_normalize_for_tts_byte_map_with_skipped_quotes() {
        let (text, map) = normalize_for_tts("\"hi\"");
        assert_eq!(text, "hi");
        assert_eq!(map.len(), 2);
        assert_eq!(map[0], 1); // 'h' at byte 1 (skipped the first quote)
        assert_eq!(map[1], 2); // 'i' at byte 2
    }

    #[test]
    fn test_normalize_for_tts_trims() {
        let (text, _) = normalize_for_tts("  hello  ");
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_split_into_sentences_empty() {
        let chunks = split_into_sentences("");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_split_into_sentences_simple() {
        let chunks = split_into_sentences("Hello world. Second sentence.");
        // Both sentences are short (< 120 chars), so they get merged into one chunk
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world. Second sentence.");
    }

    #[test]
    fn test_split_into_sentences_question_exclamation() {
        let chunks = split_into_sentences("What? Yes!");
        // Both are short, so they get merged
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "What? Yes!");
    }

    #[test]
    fn test_split_into_sentences_merges_short() {
        // Short sentences should be merged into longer chunks
        let chunks = split_into_sentences("A. B. C. D. E. F.");
        assert!(chunks.len() < 6, "short sentences should be merged");
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_find_sentence_index_first() {
        let chunks = vec!["Hello.".into(), "World.".into()];
        let idx = find_sentence_index(&chunks, "Hello. World.", 0);
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_find_sentence_index_second() {
        let chunks = vec!["Hello.".into(), "World.".into()];
        // byte 7 is 'W' in "Hello. World."
        let idx = find_sentence_index(&chunks, "Hello. World.", 7);
        assert_eq!(idx, 1);
    }

    #[test]
    fn test_find_sentence_index_past_end() {
        let chunks = vec!["Hello.".into()];
        let idx = find_sentence_index(&chunks, "Hello.", 100);
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_help_render() {
        let help = Help;
        let lines = help.render(&Bk::default_for_test());
        assert!(!lines.is_empty());
        // Should contain keybindings
        assert!(lines.iter().any(|l| l.contains("Quit")));
        assert!(lines.iter().any(|l| l.contains("Search")));
    }

    #[test]
    fn test_metadata_render_empty() {
        let meta = Metadata;
        let bk = Bk::default_for_test();
        let lines = meta.render(&bk);
        assert_eq!(lines[0], "(empty book)");
    }

    #[test]
    fn test_page_render_empty() {
        let lines = Page.render(&Bk::default_for_test());
        assert_eq!(lines[0], "(empty book)");
    }
}