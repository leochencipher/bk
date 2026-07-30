use crossterm::{
    cursor,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    queue,
    style::{
        self,
        Color::{self, Rgb},
        Colors, Print, ResetColor, SetColors,
    },
    terminal,
};
use serde::{Deserialize, Serialize};
use chrono::Local;
use sha2::{Sha256, Digest};
use std::{
    cmp::min,
    collections::HashMap,
    env, fs, i16,
    io::{self, Write},
    iter,
    process::{exit, Child},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
    u16, u32,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use viuer::Config;

mod view;
use view::{Page, Toc, View};

mod epub;
mod theme;
mod tts;
use theme::{Theme, THEMES, find_theme};

fn wrap(text: &str, max_cols: usize) -> Vec<(usize, usize)> {
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

fn compute_line_offsets(chapters: &[epub::Chapter]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(chapters.len() + 1);
    let mut acc = 0;
    for c in chapters {
        offsets.push(acc);
        acc += c.lines.len();
    }
    offsets.push(acc); // offsets[chapters.len()] = total line count
    offsets
}

// ── nested TOC ──

#[derive(Clone)]
pub(crate) struct TocItem {
    pub title: String,
    pub chapter: usize,
    pub depth: usize,
    pub has_children: bool,
    pub is_expanded: bool,
    pub is_last: bool,
    pub ancestors_last: Vec<bool>,
    pub toc_idx: usize,
}

fn count_toc(tree: &[epub::TocEntry]) -> usize {
    tree.iter().map(|e| 1 + count_toc(&e.children)).sum()
}

pub(crate) fn rebuild_toc_visible(
    tree: &[epub::TocEntry],
    expanded: &[bool],
    path_to_chapter: &HashMap<String, usize>,
) -> Vec<TocItem> {
    fn dfs(
        entries: &[epub::TocEntry],
        expanded: &[bool],
        path_to_chapter: &HashMap<String, usize>,
        idx: &mut usize,
        depth: usize,
        ancestors_last: Vec<bool>,
        visible: &mut Vec<TocItem>,
    ) {
        for (i, entry) in entries.iter().enumerate() {
            let my_idx = *idx;
            *idx += 1;
            let is_last = i == entries.len() - 1;
            let chapter = path_to_chapter.get(&entry.path).copied().unwrap_or(0);
            let mut my_ancestors = ancestors_last.clone();
            my_ancestors.push(is_last);
            let is_expanded = expanded.get(my_idx).copied().unwrap_or(true);
            visible.push(TocItem {
                title: entry.title.clone(),
                chapter,
                depth,
                has_children: !entry.children.is_empty(),
                is_expanded,
                is_last,
                ancestors_last: ancestors_last.clone(),
                toc_idx: my_idx,
            });
            if is_expanded && !entry.children.is_empty() {
                dfs(&entry.children, expanded, path_to_chapter, idx, depth + 1, my_ancestors, visible);
            }
        }
    }
    let mut visible = Vec::new();
    let mut idx = 0;
    dfs(tree, expanded, path_to_chapter, &mut idx, 0, vec![], &mut visible);
    visible
}

/// Expand ancestors of the current chapter so it is visible in the TOC.
pub(crate) fn ensure_chapter_visible(bk: &mut Bk) {
    fn visit(
        tree: &[epub::TocEntry],
        expanded: &mut [bool],
        path_to_chapter: &HashMap<String, usize>,
        target: usize,
        idx: &mut usize,
    ) -> bool {
        for entry in tree {
            let my_idx = *idx;
            *idx += 1;
            if path_to_chapter.get(&entry.path).copied() == Some(target) {
                return true;
            }
            if !entry.children.is_empty()
                && visit(&entry.children, expanded, path_to_chapter, target, idx)
            {
                expanded[my_idx] = true;
                return true;
            }
        }
        false
    }
    let mut idx = 0;
    if visit(&bk.toc_tree, &mut bk.toc_expanded, &bk.path_to_chapter, bk.chapter, &mut idx) {
        bk.toc_visible =
            rebuild_toc_visible(&bk.toc_tree, &bk.toc_expanded, &bk.path_to_chapter);
    }
}

/// Compute SHA-256 hash of a file, returned as a hex string.
fn hash_file(path: &str) -> io::Result<String> {
    use std::fs::File;
    let mut hasher = Sha256::new();
    let mut reader = File::open(path)?;
    std::io::copy(&mut reader, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

/// Returns (left_section, right_section, total_width) for the status bar.
/// Caller is responsible for padding and coloring.
fn build_status(bk: &Bk) -> (String, String) {
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
    let right = format!("📄 {}/{} │ 📊 {}% │ 🕐 {} ", page, total_pages, pct, now);
    (left, right)
}

struct SearchArgs {
    dir: Direction,
    skip: bool,
}

#[derive(Clone)]
enum Direction {
    Next,
    Prev,
}

pub struct Bk<'a> {
    quit: bool,
    dirty: bool,
    chapters: Vec<epub::Chapter>,
    // position in the book
    chapter: usize,
    line: usize,
    mark: HashMap<char, (usize, usize)>,
    links: HashMap<String, (usize, usize)>,
    // layout
    colors: Colors,
    cli_fg: Option<Color>,
    cli_bg: Option<Color>,
    cols: u16,
    rows: usize,
    max_width: u16,
    theme: Theme,
    // view state
    view: &'a dyn View,
    toc_cursor: usize,
    dir: Direction,
    meta: Vec<String>,
    query: String,
    imgs: HashMap<String, Vec<u8>>,
    chapter_line_offsets: Vec<usize>,
    // nested TOC
    toc_tree: Vec<epub::TocEntry>,
    toc_expanded: Vec<bool>,
    toc_visible: Vec<TocItem>,
    path_to_chapter: HashMap<String, usize>,
    // reading mode toggles
    pub bionic: bool,
    pub focus: bool,
    tts_engine: Option<Arc<Mutex<tts::InflectTts>>>,
    // TTS mode state
    pub tts_active: bool,
    pub tts_sentences: Vec<String>,
    pub tts_sentence_idx: usize,
    pub tts_child: Option<Child>,
    pub tts_buffer: Option<(usize, String)>,  // (idx, wav_path) of pre-synthesized next sentence
}

impl Bk<'_> {
    fn new(epub: epub::Epub, args: Props) -> Self {
        let (cols, rows) = terminal::size().unwrap();
        let width = min(cols, args.width) as usize;
        let meta = wrap(&epub.meta, width)
            .into_iter()
            .map(|(a, b)| String::from(&epub.meta[a..b]))
            .collect();

        let mut chapters = epub.chapters;
        let imgs = epub.imgs;
        for c in &mut chapters {
            c.lines = wrap(&c.text, width);
            if c.title.chars().count() > width {
                c.title = c
                    .title
                    .chars()
                    .take(width - 1)
                    .chain(std::iter::once('…'))
                    .collect();
            }
        }

        let chapter_line_offsets = compute_line_offsets(&chapters);

        let fg = args.cli_fg.unwrap_or(args.theme.fg);
        let bg = args.cli_bg.unwrap_or(args.theme.bg);

        let toc_tree = epub.toc_tree;
        let path_to_chapter = epub.path_to_chapter;
        let toc_count = count_toc(&toc_tree);
        let toc_expanded = vec![true; toc_count.max(1)];

        // Initialize TTS engine if model directory is available
        let tts_engine = args.tts_model_dir.as_ref().and_then(|dir| {
            let model_dir = std::path::Path::new(dir);
            if model_dir.join("onnx/duration.onnx").exists() {
                tts::InflectTts::new(model_dir).ok().map(|e| Arc::new(Mutex::new(e)))
            } else {
                None
            }
        });
        let toc_visible = rebuild_toc_visible(&toc_tree, &toc_expanded, &path_to_chapter);

        let mut bk = Bk {
            quit: false,
            dirty: true,
            chapters,
            chapter: 0,
            line: 0,
            mark: HashMap::new(),
            links: epub.links,
            colors: Colors::new(fg, bg),
            cli_fg: args.cli_fg,
            cli_bg: args.cli_bg,
            cols,
            rows: (rows as usize).saturating_sub(1).max(1),
            max_width: args.width,
            theme: args.theme,
            view: if args.toc { &Toc } else { &Page },
            toc_cursor: 0,
            dir: Direction::Next,
            meta,
            query: String::new(),
            imgs,
            chapter_line_offsets,
            toc_tree,
            toc_expanded,
            toc_visible,
            path_to_chapter,
            bionic: false,
            focus: false,
            tts_engine,
            tts_active: false,
            tts_sentences: Vec::new(),
            tts_sentence_idx: 0,
            tts_child: None,
            tts_buffer: None,
        };

        bk.jump_byte(args.chapter, args.byte);
        bk.mark('\'');

        bk
    }
    fn run(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        queue!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            EnableMouseCapture,
        )?;
        terminal::enable_raw_mode()?;

        // Helper to strip ANSI escape sequences from a line for IMG marker detection
        fn strip_ansi(s: &str) -> String {
            let mut out = String::with_capacity(s.len());
            let mut chars = s.chars();
            while let Some(c) = chars.next() {
                if c == '\x1b' && chars.clone().next() == Some('[') {
                    // Skip the escape sequence: \x1b[...m (or other terminators)
                    chars.next(); // consume '['
                    while let Some(d) = chars.next() {
                        if d.is_ascii_alphabetic() { break; }
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }

        let mut render = |bk: &Bk| {
            queue!(
                stdout,
                Print(style::Attribute::Reset),
                SetColors(bk.colors),
                terminal::Clear(terminal::ClearType::All),
            )
            .unwrap();
            let mut img_index = 1;
            let mut last_y: i16 = 5;
            for (i, line) in bk.view.render(bk).iter().enumerate() {
                let clean = strip_ansi(line);
                if !clean.starts_with("[IMG][") {
                    let curlen = clean.width_cjk();
                    if clean.starts_with(" ") {
                        queue!(
                            stdout,
                            cursor::MoveTo(5, i as u16),
                            SetColors(Colors::new(
                                bk.theme.heading_accent_fg,
                                bk.theme.heading_accent_bg,
                            )),
                            Print(format!(
                                "{}{}",
                                &line[3..],
                                " ".repeat((bk.max_width as usize + 11).saturating_sub(curlen))
                            )),
                            SetColors(bk.colors)
                        )
                        .unwrap();
                    } else {
                        queue!(stdout, cursor::MoveTo(5, i as u16), Print(line)).unwrap();
                    }
                } else {
                    queue!(
                        stdout,
                        cursor::MoveTo(5, i as u16),
                        Print(format!("[{}]", img_index))
                    )
                    .unwrap();
                    // [IMG][url][width] — use cleaned line for parsing
                    // format: "[IMG][" (6 chars) + url + "][" + width + "]"
                    if clean.len() < 9 {
                        // Malformed IMG marker; skip
                        img_index += 1;
                        continue;
                    }
                    let inner = &clean[6..clean.len() - 1]; // strip "[IMG][" prefix and trailing "]"
                    let parts: Vec<&str> = inner.split("][").collect();
                    if parts.len() < 2 {
                        // Marker wrapped across lines; skip
                        img_index += 1;
                        continue;
                    }
                    let (url, width_str) = (parts[0], parts[1]);
                    let width: u32 = width_str.trim_end_matches(|c: char| !c.is_ascii_digit())
                        .parse()
                        .unwrap_or(100);
                    let width = min(width, 100);
                    let buf = match bk.imgs.get(url) {
                        Some(b) => b,
                        None => {
                            img_index += 1;
                            continue;
                        }
                    };
                    let img = match image::load_from_memory(buf) {
                        Ok(img) => img,
                        Err(_) => {
                            img_index += 1;
                            continue;
                        }
                    };
                    let avail_cols = bk.cols.saturating_sub(bk.max_width + 10) as u32;
                    let natural_cols = (img.width() / 8).max(1);
                    let target_cols = (avail_cols * width / 100).min(natural_cols).max(1);
                    let remaining_rows = (bk.rows as i16 - last_y).max(1) as u32;
                    // pre-scale image height to correct for viuer's fixed 1:2 cell assumption;
                    // tune this value: < 1.0 = shorter, > 1.0 = taller
                    let height_correction: f32 = 0.6;
                    let corrected_h = ((img.height() as f32 * height_correction) as u32).max(1);
                    let display_img = img.resize_exact(
                        img.width(),
                        corrected_h,
                        image::imageops::FilterType::Triangle,
                    );
                    let est_rows = if img.width() > 0 {
                        corrected_h * target_cols / img.width() / 2
                    } else {
                        remaining_rows
                    };
                    let conf = if est_rows <= remaining_rows {
                        Config {
                            x: bk.max_width + 10,
                            y: last_y,
                            width: Some(target_cols),
                            ..Default::default()
                        }
                    } else {
                        Config {
                            x: bk.max_width + 10,
                            y: last_y,
                            height: Some(remaining_rows),
                            ..Default::default()
                        }
                    };
                    match viuer::print(&display_img, &conf) {
                        Ok((_print_width, print_height)) => {
                            queue!(
                                stdout,
                                cursor::MoveTo(bk.max_width + 7, last_y as u16),
                                Print(format!("[{}]", img_index))
                            )
                            .unwrap();
                            img_index += 1;
                            last_y = last_y + print_height as i16 + 2;
                        }
                        Err(_) => {
                            img_index += 1;
                            continue;
                        }
                    }
                }
            }
            // Status bar on the last terminal row
            let (left, right) = build_status(bk);
            let width = bk.cols as usize;
            // emoji are double-width; count display width for padding
            let left_w = left.width_cjk();
            let right_w = right.width_cjk();
            let pad = width.saturating_sub(left_w + right_w);
            // truncate title if needed
            let left_display = if left_w > width.saturating_sub(right_w + 1) {
                let avail = width.saturating_sub(right_w + 2);
                left.chars()
                    .scan(0usize, |acc, c| {
                        *acc += c.width_cjk().unwrap_or(1);
                        Some((*acc, c))
                    })
                    .take_while(|(w, _)| *w <= avail)
                    .map(|(_, c)| c)
                    .collect::<String>()
                    + "…"
            } else {
                left.clone()
            };
            queue!(
                stdout,
                cursor::MoveTo(0, bk.rows as u16),
                SetColors(Colors::new(
                    bk.theme.status_left_fg,
                    bk.theme.status_left_bg,
                )),
                Print(&left_display),
                Print(" ".repeat(pad)),
                SetColors(Colors::new(
                    bk.theme.status_right_fg,
                    bk.theme.status_right_bg,
                )),
                Print(&right),
                ResetColor,
            )
            .unwrap();

            queue!(stdout, cursor::MoveTo(5, bk.toc_cursor as u16)).unwrap();
            stdout.flush().unwrap();
        };

        render(self);
        loop {
            // ── TTS playback check ──
            if self.tts_active {
                if let Some(child) = &mut self.tts_child {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            self.tts_child = None;
                            self.advance_tts();
                            self.dirty = true;
                        }
                        Ok(None) => {}
                        Err(_) => {
                            self.tts_child = None;
                            self.advance_tts();
                            self.dirty = true;
                        }
                    }
                } else if self.tts_sentence_idx < self.tts_sentences.len() {
                    self.start_tts();
                    self.buffer_next_sentence();
                    self.dirty = true;
                }
            }

            // ── read event (poll when TTS active) ──
            let event = if self.tts_active {
                if event::poll(Duration::from_millis(100))? {
                    Some(event::read()?)
                } else {
                    None
                }
            } else {
                Some(event::read()?)
            };

            match event {
                Some(Event::Key(e)) => {
                    self.dirty = true;
                    self.view.on_key(self, e.code);
                }
                Some(Event::Mouse(e)) => {
                    if e.kind == event::MouseEventKind::Moved {
                        continue;
                    }
                    self.dirty = true;
                    self.view.on_mouse(self, e);
                }
                Some(Event::Resize(cols, rows)) => {
                    self.dirty = true;
                    self.rows = (rows as usize).saturating_sub(1).max(1);
                    if cols != self.cols {
                        self.cols = cols;
                        let width = min(cols, self.max_width) as usize;
                        for c in &mut self.chapters {
                            c.lines = wrap(&c.text, width);
                        }
                    }
                    self.chapter_line_offsets = compute_line_offsets(&self.chapters);
                    self.view.on_resize(self);
                }
                None => {}
            }
            if self.quit {
                break;
            }
            if self.dirty {
                render(self);
                self.dirty = false;
            }
        }

        queue!(
            stdout,
            terminal::LeaveAlternateScreen,
            cursor::Show,
            DisableMouseCapture
        )?;
        terminal::disable_raw_mode()
    }
    fn jump(&mut self, (c, l): (usize, usize)) {
        if self.chapters.is_empty() {
            return;
        }
        self.mark('\'');
        self.chapter = c.min(self.chapters.len() - 1);
        self.line = l.min(self.chapters[self.chapter].lines.len().saturating_sub(1));
    }
    fn jump_byte(&mut self, c: usize, byte: usize) {
        if self.chapters.is_empty() {
            self.chapter = 0;
            self.line = 0;
            return;
        }
        self.chapter = c.min(self.chapters.len() - 1);
        self.line = match self.chapters[self.chapter]
            .lines
            .binary_search_by_key(&byte, |&(a, _)| a)
        {
            Ok(n) => n,
            Err(n) => n - 1,
        }
    }
    fn jump_reset(&mut self) {
        if self.chapters.is_empty() {
            return;
        }
        let &(c, l) = self.mark.get(&'\'').unwrap();
        self.chapter = c.min(self.chapters.len() - 1);
        self.line = l.min(self.chapters[self.chapter].lines.len().saturating_sub(1));
    }
    fn mark(&mut self, c: char) {
        self.mark.insert(c, (self.chapter, self.line));
    }

    fn next_theme(&mut self) {
        let idx = THEMES
            .iter()
            .position(|t| t.name == self.theme.name)
            .unwrap_or(0);
        self.theme = THEMES[(idx + 1) % THEMES.len()];
        self.apply_colors();
    }

    fn apply_colors(&mut self) {
        self.colors = Colors::new(
            self.cli_fg.unwrap_or(self.theme.fg),
            self.cli_bg.unwrap_or(self.theme.bg),
        );
    }

    fn search(&mut self, args: SearchArgs) -> bool {
        if self.chapters.is_empty() {
            return false;
        }
        let (start, end) = self.chapters[self.chapter].lines[self.line];
        match args.dir {
            Direction::Next => {
                let byte = if args.skip { end } else { start };
                let head = (self.chapter, byte);
                let tail = (self.chapter + 1..self.chapters.len() - 1).map(|n| (n, 0));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = self.chapters[c].text[byte..].find(&self.query) {
                        self.jump_byte(c, index + byte);
                        return true;
                    }
                }
                false
            }
            Direction::Prev => {
                let byte = if args.skip { start } else { end };
                let head = (self.chapter, byte);
                let tail = (0..self.chapter)
                    .rev()
                    .map(|c| (c, self.chapters[c].text.len()));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = self.chapters[c].text[..byte].rfind(&self.query) {
                        self.jump_byte(c, index);
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Synthesize and play the current TTS sentence.
    fn start_tts(&mut self) {
        if self.tts_sentences.is_empty() {
            return;
        }
        let idx = self.tts_sentence_idx;
        if idx >= self.tts_sentences.len() {
            return;
        }
        let sentence = self.tts_sentences[idx].clone();
        let params = tts::SynthesizeParams::default();
        let output = format!("/tmp/bk_tts_{}.wav", idx);
        if let Some(engine) = &self.tts_engine {
            let mut tts = engine.lock().unwrap();
            if tts.save(&sentence, std::path::Path::new(&output), &params).is_ok() {
                self.tts_child = std::process::Command::new("afplay")
                    .arg(&output)
                    .spawn()
                    .ok();
            }
        }
    }

    /// Synthesize the next sentence in a background thread.
    fn buffer_next_sentence(&mut self) {
        let next_idx = self.tts_sentence_idx + 1;
        if next_idx >= self.tts_sentences.len() {
            return;
        }
        if let Some((buf_idx, _)) = &self.tts_buffer {
            if *buf_idx == next_idx {
                return;
            }
        }
        let sentence = self.tts_sentences[next_idx].clone();
        let output = format!("/tmp/bk_tts_{}.wav", next_idx);
        let engine = self.tts_engine.clone();

        self.tts_buffer = Some((next_idx, output.clone()));

        if let Some(engine) = engine {
            thread::spawn(move || {
                let mut tts = engine.lock().unwrap();
                let params = tts::SynthesizeParams::default();
                let _ = tts.save(&sentence, std::path::Path::new(&output), &params);
            });
        }
    }

    /// Advance to the next sentence. If at end of chapter, exit TTS mode.
    fn advance_tts(&mut self) {
        self.tts_sentence_idx += 1;
        if self.tts_sentence_idx >= self.tts_sentences.len() {
            self.tts_child = None;
            self.tts_buffer = None;
            self.tts_active = false;
            self.view = &Page;
            return;
        }

        let buffered = self.tts_buffer.as_ref().and_then(|(idx, path)| {
            if *idx == self.tts_sentence_idx { Some(path.clone()) } else { None }
        });

        if let Some(path) = buffered {
            self.tts_buffer = None;
            self.tts_child = std::process::Command::new("afplay")
                .arg(&path)
                .spawn()
                .ok();
        } else {
            self.start_tts();
        }

        self.buffer_next_sentence();
    }

}

#[derive(argh::FromArgs)]
/// read a book
struct Args {
    #[argh(positional)]
    path: Option<String>,

    /// background color (eg 282a36)
    #[argh(option)]
    bg: Option<String>,

    /// foreground color (eg f8f8f2)
    #[argh(option)]
    fg: Option<String>,

    /// print metadata and exit
    #[argh(switch, short = 'm')]
    meta: bool,

    /// start with table of contents open
    #[argh(switch, short = 't')]
    toc: bool,

    /// characters per line
    #[argh(option, short = 'w', default = "75")]
    width: u16,

    /// save path for reading progress (default: ~/.local/share/bk)
    #[argh(option, short = 's')]
    save_path: Option<String>,

    /// color theme: catppuccin-mocha, catppuccin-latte, solarized-dark, nord, gruvbox-dark
    #[argh(option, short = 'T')]
    theme: Option<String>,
    /// directory containing onnx/ subdirectory with TTS models
    #[argh(option)]
    tts_model_dir: Option<String>,
}

struct Props {
    cli_fg: Option<Color>,
    cli_bg: Option<Color>,
    chapter: usize,
    byte: usize,
    tts_model_dir: Option<String>,
    width: u16,
    toc: bool,
    theme: Theme,
}

#[derive(Default, Deserialize, Serialize)]
struct Save {
    last: String,  // book hash of last opened book
    files: HashMap<String, (usize, usize)>,  // book_hash → (chapter, byte_offset)
}

struct State {
    save: Save,
    save_path: String,
    path: String,
    meta: bool,
    bk: Props,
}

fn init() -> Result<State, Box<dyn std::error::Error>> {
    let args: Args = argh::from_env();

    let save_path = args
        .save_path
        .clone()
        .unwrap_or_else(|| {
            if cfg!(windows) {
                format!("{}\\bk", env::var("APPDATA").unwrap())
            } else {
                format!("{}/book/bk", env::var("HOME").unwrap())
            }
        });

    // XXX will silently create a new default save if ron errors but path arg works.
    // revisit if/when stabilizing. ez file format upgrades
    let save: io::Result<Save> = fs::read_to_string(&save_path).and_then(|s| {
        ron::from_str(&s)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid save file"))
    });

    let path = match args.path {
        Some(p) => Some(fs::canonicalize(p)?.to_str().unwrap().to_string()),
        None => None,
    };

    let (path, save, chapter, byte) = match (save, path) {
        (Err(e), None) => return Err(Box::new(e)),
        (Err(_), Some(p)) => (p, Save::default(), 0, 0),
        (Ok(s), None) => {
            let &(chapter, byte) = s.files.get(&s.last).unwrap();
            (s.last.clone(), s, chapter, byte)
        }
        (Ok(s), Some(p)) => {
            // 1) Try hash-based lookup (new format, sync-friendly)
            let book_hash = hash_file(&p).unwrap_or_default();
            if let Some(&(chapter, byte)) = s.files.get(&book_hash) {
                (p, s, chapter, byte)
            }
            // 2) Fall back to path-based lookup (old format, backward compat)
            else if s.files.contains_key(&p) {
                let &(chapter, byte) = s.files.get(&p).unwrap();
                (p, s, chapter, byte)
            }
            // 3) New book, start from beginning
            else {
                (p, s, 0, 0)
            }
        }
    };

    // XXX oh god what
    let cli_fg = args.fg.map(|s| Rgb {
        r: u8::from_str_radix(&s[0..2], 16).unwrap(),
        g: u8::from_str_radix(&s[2..4], 16).unwrap(),
        b: u8::from_str_radix(&s[4..6], 16).unwrap(),
    });
    let cli_bg = args.bg.map(|s| Rgb {
        r: u8::from_str_radix(&s[0..2], 16).unwrap(),
        g: u8::from_str_radix(&s[2..4], 16).unwrap(),
        b: u8::from_str_radix(&s[4..6], 16).unwrap(),
    });

    let theme_name = args.theme.as_deref().unwrap_or("catppuccin-mocha");
    let theme = THEMES[find_theme(theme_name).unwrap_or(0)];

    Ok(State {
        path,
        save,
        save_path,
        meta: args.meta,
        bk: Props {
            cli_fg,
            cli_bg,
            chapter,
            byte,
            width: args.width,
            toc: args.toc,
            theme,
            tts_model_dir: args.tts_model_dir,
        },
    })
}

fn main() {
    let mut state = init().unwrap_or_else(|e| {
        println!("init error: {}", e);
        exit(1);
    });
    let epub = epub::Epub::new(&state.path, state.meta).unwrap_or_else(|e| {
        println!("epub error: {}", e);
        exit(1);
    });
    if state.meta {
        println!("{}", epub.meta);
        exit(0);
    }
    let mut bk = Bk::new(epub, state.bk);
    bk.run().unwrap_or_else(|e| {
        println!("run error: {}", e);
        exit(1);
    });

    if !bk.chapters.is_empty() {
        let byte = bk.chapters[bk.chapter].lines[bk.line].0;
        let book_hash = hash_file(&state.path).unwrap_or_default();
        state
            .save
            .files
            .insert(book_hash.clone(), (bk.chapter, byte));
        state.save.last = book_hash;
    }
    let serialized = ron::to_string(&state.save).unwrap();
    fs::write(state.save_path, serialized).unwrap_or_else(|e| {
        println!("error saving state: {}", e);
        exit(1);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Page;

    fn make_test_bk(epub: epub::Epub) -> Bk<'static> {
        let chapters = epub.chapters;
        let imgs = epub.imgs;
        let mut chapters: Vec<_> = chapters;
        let width = 80usize;
        for c in &mut chapters {
            c.lines = wrap(&c.text, width);
        }
        let chapter_line_offsets = compute_line_offsets(&chapters);
        let fg = Rgb { r: 255, g: 255, b: 255 };
        let bg = Rgb { r: 0, g: 0, b: 0 };
        let theme = THEMES[0];
        Bk {
            quit: false,
            dirty: true,
            chapters,
            chapter: 0,
            line: 0,
            mark: HashMap::new(),
            links: HashMap::new(),
            colors: Colors::new(fg, bg),
            cli_fg: None,
            cli_bg: None,
            cols: 80,
            rows: 24,
            max_width: 80,
            theme,
            view: &Page,
            toc_cursor: 0,
            dir: Direction::Next,
            meta: Vec::new(),
            query: String::new(),
            imgs,
            chapter_line_offsets,
            toc_tree: Vec::new(),
            toc_expanded: Vec::new(),
            toc_visible: Vec::new(),
            path_to_chapter: HashMap::new(),
            bionic: false,
            focus: false,
            tts_engine: None,
            tts_active: false,
            tts_sentences: Vec::new(),
            tts_sentence_idx: 0,
            tts_child: None,
            tts_buffer: None,
        }
    }

    #[test]
    fn test_load_and_render_all_chapters() {
        let epub = epub::Epub::new("test/test.epub", false)
            .expect("failed to load test EPUB");
        assert!(!epub.chapters.is_empty(), "EPUB has no chapters");
        let mut bk = make_test_bk(epub);

        for ch_idx in 0..bk.chapters.len() {
            bk.chapter = ch_idx;
            bk.line = 0;
            let rendered = Page.render(&bk);
            assert!(
                !rendered.is_empty(),
                "chapter {} '{}' rendered empty (lines: {:?})",
                ch_idx,
                bk.chapters[ch_idx].title,
                bk.chapters[ch_idx].lines.len(),
            );
        }
    }

    #[test]
    fn test_scroll_all_chapters() {
        let epub = epub::Epub::new("test/test.epub", false)
            .expect("failed to load test EPUB");
        let mut bk = make_test_bk(epub);

        for ch_idx in 0..bk.chapters.len() {
            bk.chapter = ch_idx;
            let line_count = bk.chapters[ch_idx].lines.len();
            assert!(line_count > 0, "chapter {} has no lines", ch_idx);
            for line in 0..line_count {
                bk.line = line;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Page.render(&bk)
                }));
                if let Err(e) = result {
                    let msg = if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = e.downcast_ref::<&str>() {
                        s.to_string()
                    } else {
                        "unknown panic".to_string()
                    };
                    panic!(
                        "chapter {} '{}': panic at line {}/{}: {}",
                        ch_idx, bk.chapters[ch_idx].title, line, line_count, msg
                    );
                }
            }
        }
    }

    #[test]
    fn test_chapter_navigation() {
        let epub = epub::Epub::new("test/test.epub", false)
            .expect("failed to load test EPUB");
        let mut bk = make_test_bk(epub);

        // Simulate pressing ] to go through all chapters
        for ch_idx in 0..bk.chapters.len() {
            assert_eq!(bk.chapter, ch_idx, "chapter index mismatch");
            bk.line = 0;
            let _ = Page.render(&bk);
            // Simulate next_chapter
            if bk.chapter < bk.chapters.len() - 1 {
                bk.chapter += 1;
                bk.line = 0;
            }
        }
    }
}
