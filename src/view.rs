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

        let mut search: Vec<(usize, String)> = Vec::new();
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
                search.push((text_start + pos, hl_on.clone()));
                search.push((text_start + pos + len, hl_off.clone()));
            }
        }
        let mut search = search.into_iter().peekable();

        let mut base = {
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
            all.into_iter().peekable()
        };

        let mut attrs: Vec<(usize, String)> = Vec::new();
        loop {
            match (search.peek(), base.peek()) {
                (None, None) => break,
                (Some(_), None) => {
                    attrs.extend(search);
                    break;
                }
                (None, Some(_)) => {
                    attrs.extend(base);
                    break;
                }
                (Some(_), Some(_)) => {
                    let s_pos = search.peek().unwrap().0;
                    let b_pos = base.peek().unwrap().0;
                    if s_pos < b_pos {
                        attrs.push(search.next().unwrap());
                    } else {
                        attrs.push(base.next().unwrap());
                    }
                }
            }
        }
        let mut attrs = attrs.into_iter().peekable();

        let mut buf = Vec::with_capacity(last_line - bk.line);
        for &(mut pos, line_end) in &c.lines[bk.line..last_line] {
            let mut s = String::new();
            while let Some((attr_pos, attr)) = attrs.next_if(|a| a.0 <= line_end) {
                s.push_str(&c.text[pos..attr_pos]);
                s.push_str(&attr);
                pos = attr_pos;
            }
            s.push_str(&c.text[pos..line_end]);
            buf.push(s);
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
                if !bk.search(args) {
                    bk.jump_reset();
                }
            }
            _ => { bk.dirty = false; }
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
