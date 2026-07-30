use std::cmp::min;
use std::collections::HashMap;
use std::io::{self, Write};
use std::iter;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::{
    cursor,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    queue,
    style::{
        self,
        Color, Colors, Print, ResetColor, SetColors,
    },
    terminal,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use viuer::Config;

use crate::epub;
use crate::status::build_status;
use crate::theme::{Theme, THEMES};
use crate::toc::{self, TocItem};
use crate::tts;
use crate::view::{Page, Toc, View};
use crate::wrap;

#[derive(Clone)]
pub(crate) enum Direction {
    Next,
    Prev,
}

pub(crate) struct SearchArgs {
    pub dir: Direction,
    pub skip: bool,
}

pub struct Bk<'a> {
    pub(crate) quit: bool,
    pub(crate) dirty: bool,
    pub(crate) chapters: Vec<epub::Chapter>,
    // position in the book
    pub(crate) chapter: usize,
    pub(crate) line: usize,
    pub(crate) mark: HashMap<char, (usize, usize)>,
    pub(crate) links: HashMap<String, (usize, usize)>,
    // layout
    pub(crate) colors: Colors,
    pub(crate) cli_fg: Option<Color>,
    pub(crate) cli_bg: Option<Color>,
    pub(crate) cols: u16,
    pub(crate) rows: usize,
    pub(crate) max_width: u16,
    pub(crate) theme: Theme,
    // view state
    pub(crate) view: &'a dyn View,
    pub(crate) toc_cursor: usize,
    pub(crate) dir: Direction,
    pub(crate) meta: Vec<String>,
    pub(crate) query: String,
    pub(crate) imgs: HashMap<String, Vec<u8>>,
    pub(crate) chapter_line_offsets: Vec<usize>,
    // nested TOC
    pub(crate) toc_tree: Vec<epub::TocEntry>,
    pub(crate) toc_expanded: Vec<bool>,
    pub(crate) toc_visible: Vec<TocItem>,
    pub(crate) path_to_chapter: HashMap<String, usize>,
    // reading mode toggles
    pub bionic: bool,
    pub focus: bool,
    pub(crate) tts_engine: Option<Arc<Mutex<tts::InflectTts>>>,
    // TTS mode state
    pub tts_active: bool,
    pub tts_sentences: Vec<String>,
    pub tts_sentence_idx: usize,
    pub tts_child: Option<Child>,
    pub tts_buffer: Option<(usize, String)>, // (idx, wav_path) of pre-synthesized next sentence
}

pub(crate) struct Props {
    pub cli_fg: Option<Color>,
    pub cli_bg: Option<Color>,
    pub chapter: usize,
    pub byte: usize,
    pub tts_model_dir: Option<String>,
    pub width: u16,
    pub toc: bool,
    pub theme: Theme,
}

impl Bk<'_> {
    pub(crate) fn new(epub: epub::Epub, args: Props) -> Self {
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        let width = min(cols, args.width) as usize;
        let meta = wrap::wrap(&epub.meta, width)
            .into_iter()
            .map(|(a, b)| String::from(&epub.meta[a..b]))
            .collect();

        let mut chapters = epub.chapters;
        let imgs = epub.imgs;
        for c in &mut chapters {
            c.lines = wrap::wrap(&c.text, width);
            if c.title.chars().count() > width {
                c.title = c
                    .title
                    .chars()
                    .take(width - 1)
                    .chain(std::iter::once('…'))
                    .collect();
            }
        }

        let chapter_line_offsets = wrap::compute_line_offsets(&chapters);

        let fg = args.cli_fg.unwrap_or(args.theme.fg);
        let bg = args.cli_bg.unwrap_or(args.theme.bg);

        let toc_tree = epub.toc_tree;
        let path_to_chapter = epub.path_to_chapter;
        let toc_count = toc::count_toc(&toc_tree);
        let toc_expanded = vec![true; toc_count.max(1)];

        // Initialize TTS engine if model directory is available
        let tts_engine = args.tts_model_dir.as_ref().and_then(|dir| {
            let model_dir = std::path::Path::new(dir);
            if model_dir.join("onnx/duration.onnx").exists() {
                tts::InflectTts::new(model_dir)
                    .ok()
                    .map(|e| Arc::new(Mutex::new(e)))
            } else {
                None
            }
        });
        let toc_visible =
            toc::rebuild_toc_visible(&toc_tree, &toc_expanded, &path_to_chapter);

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

    pub(crate) fn run(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        queue!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            EnableMouseCapture,
        )?;
        terminal::enable_raw_mode()?;

        let mut render = |bk: &Bk| {
            if queue!(
                stdout,
                Print(style::Attribute::Reset),
                SetColors(bk.colors),
                terminal::Clear(terminal::ClearType::All),
            )
            .is_err()
            {
                return;
            }
            let mut img_index = 1;
            let mut last_y: i16 = 5;
            for (i, line) in bk.view.render(bk).iter().enumerate() {
                let clean = wrap::strip_ansi(line);
                if !clean.starts_with("[IMG][") {
                    let curlen = line.width_cjk();
                    if clean.starts_with(" ") {
                        let _ = queue!(
                            stdout,
                            cursor::MoveTo(5, i as u16),
                            SetColors(Colors::new(
                                bk.theme.heading_accent_fg,
                                bk.theme.heading_accent_bg,
                            )),
                            Print(format!(
                                "{}{}",
                                &line[3..],
                                " ".repeat(bk.max_width as usize - curlen + 11)
                            )),
                            SetColors(bk.colors)
                        );
                    } else {
                        let _ = queue!(stdout, cursor::MoveTo(5, i as u16), Print(line));
                    }
                } else {
                    let _ = queue!(
                        stdout,
                        cursor::MoveTo(5, i as u16),
                        Print(format!("[{}]", img_index))
                    );
                    // [IMG][url][width] — format: "[IMG][" (6 chars) + url + "][" + width + "]"
                    if clean.len() < 9 {
                        // Malformed IMG marker; skip
                        img_index += 1;
                        continue;
                    }
                    let inner = &clean[6..clean.len() - 1]; // strip "[IMG][" prefix and trailing "]"
                    let parts: Vec<&str> = inner.split("][").collect();
                    if parts.len() < 2 {
                        img_index += 1;
                        continue;
                    }
                    let (url, width_str) = (parts[0], parts[1]);
                    let width: u32 = width_str
                        .trim_end_matches(|c: char| !c.is_ascii_digit())
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
                    let height_correction: f32 = 0.6;
                    let corrected_h =
                        ((img.height() as f32 * height_correction) as u32).max(1);
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
                    if let Ok((_print_width, print_height)) = viuer::print(&display_img, &conf) {
                        let _ = queue!(
                            stdout,
                            cursor::MoveTo(bk.max_width + 7, last_y as u16),
                            Print(format!("[{}]", img_index))
                        );
                        img_index += 1;
                        last_y = last_y + print_height as i16 + 2;
                    }
                }
            }
            // Status bar on the last terminal row
            let (left, right) = build_status(bk);
            let width = bk.cols as usize;
            let left_w = left.width_cjk();
            let right_w = right.width_cjk();
            let pad = width.saturating_sub(left_w + right_w);
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
            let _ = queue!(
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
            );

            let _ = queue!(stdout, cursor::MoveTo(5, bk.toc_cursor as u16));
            let _ = stdout.flush();
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
                            c.lines = wrap::wrap(&c.text, width);
                        }
                    }
                    self.chapter_line_offsets =
                        wrap::compute_line_offsets(&self.chapters);
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

    pub(crate) fn jump(&mut self, (c, l): (usize, usize)) {
        if self.chapters.is_empty() {
            return;
        }
        self.mark('\'');
        self.chapter = c.min(self.chapters.len() - 1);
        self.line = l.min(self.chapters[self.chapter].lines.len().saturating_sub(1));
    }

    pub(crate) fn jump_byte(&mut self, c: usize, byte: usize) {
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
            Err(n) => n.saturating_sub(1),
        }
    }

    pub(crate) fn jump_reset(&mut self) {
        if self.chapters.is_empty() {
            return;
        }
        let &(c, l) = self.mark.get(&'\'').unwrap();
        self.chapter = c.min(self.chapters.len() - 1);
        self.line = l.min(self.chapters[self.chapter].lines.len().saturating_sub(1));
    }

    pub(crate) fn mark(&mut self, c: char) {
        self.mark.insert(c, (self.chapter, self.line));
    }

    pub(crate) fn next_theme(&mut self) {
        let idx = THEMES
            .iter()
            .position(|t| t.name == self.theme.name)
            .unwrap_or(0);
        self.theme = THEMES[(idx + 1) % THEMES.len()];
        self.apply_colors();
    }

    pub(crate) fn apply_colors(&mut self) {
        self.colors = Colors::new(
            self.cli_fg.unwrap_or(self.theme.fg),
            self.cli_bg.unwrap_or(self.theme.bg),
        );
    }

    pub(crate) fn search(&mut self, args: SearchArgs) -> bool {
        if self.chapters.is_empty() {
            return false;
        }
        let (start, end) = self.chapters[self.chapter].lines[self.line];
        match args.dir {
            Direction::Next => {
                let byte = if args.skip { end } else { start };
                let head = (self.chapter, byte);
                let tail = (self.chapter + 1..self.chapters.len()).map(|n| (n, 0));
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

    /// Expand ancestors of the current chapter so it is visible in the TOC.
    pub(crate) fn ensure_chapter_visible(&mut self) {
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
        if visit(
            &self.toc_tree,
            &mut self.toc_expanded,
            &self.path_to_chapter,
            self.chapter,
            &mut idx,
        ) {
            self.toc_visible = toc::rebuild_toc_visible(
                &self.toc_tree,
                &self.toc_expanded,
                &self.path_to_chapter,
            );
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
                self.tts_child = Self::play_audio(&output);
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
            if *idx == self.tts_sentence_idx {
                Some(path.clone())
            } else {
                None
            }
        });

        if let Some(path) = buffered {
            self.tts_buffer = None;
            self.tts_child = Self::play_audio(&path);
        } else {
            self.start_tts();
        }

        self.buffer_next_sentence();
    }

    /// Spawn a cross-platform audio player for the given WAV file.
    fn play_audio(path: &str) -> Option<Child> {
        if cfg!(target_os = "macos") {
            std::process::Command::new("afplay")
                .arg(path)
                .spawn()
                .ok()
        } else if cfg!(target_os = "linux") {
            // Try ffplay first (part of ffmpeg), fall back to aplay
            std::process::Command::new("ffplay")
                .args(["-nodisp", "-autoexit", "-loglevel", "quiet", path])
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("aplay")
                        .arg(path)
                        .spawn()
                })
                .ok()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("powershell")
                .args([
                    "-c",
                    &format!(
                        "Add-Type -AssemblyName PresentationCore; \
                         $player = New-Object System.Windows.Media.MediaPlayer; \
                         $player.Open('{}'); $player.Play(); \
                         Start-Sleep -Seconds 999",
                        path
                    ),
                ])
                .spawn()
                .ok()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub::Chapter;
    use crate::view::Page;
    use crate::wrap;
    use crossterm::style::Color::Rgb;

    fn make_chapter(title: &str, text: &str) -> Chapter {
        let mut c = Chapter {
            title: title.to_string(),
            text: text.to_string(),
            lines: Vec::new(),
            attrs: vec![(0, crossterm::style::Attribute::Reset, crossterm::style::Attributes::default())],
            color_attrs: Vec::new(),
            state: crossterm::style::Attributes::default(),
            links: Vec::new(),
            heading_spans: Vec::new(),
            frag: Vec::new(),
        };
        c.lines = wrap::wrap(&c.text, 80);
        c
    }

    fn make_test_bk(chapters: Vec<Chapter>) -> Bk<'static> {
        let chapter_line_offsets = wrap::compute_line_offsets(&chapters);
        let fg = Rgb { r: 255, g: 255, b: 255 };
        let bg = Rgb { r: 0, g: 0, b: 0 };
        let theme = crate::theme::THEMES[0];
        Bk {
            quit: false,
            dirty: true,
            chapters,
            chapter: 0,
            line: 0,
            mark: std::collections::HashMap::new(),
            links: std::collections::HashMap::new(),
            colors: crossterm::style::Colors::new(fg, bg),
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
            imgs: std::collections::HashMap::new(),
            chapter_line_offsets,
            toc_tree: Vec::new(),
            toc_expanded: Vec::new(),
            toc_visible: Vec::new(),
            path_to_chapter: std::collections::HashMap::new(),
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
    fn test_search_forward_finds_match() {
        let ch = make_chapter("Test", "hello world foo bar baz");
        let mut bk = make_test_bk(vec![ch]);
        bk.query = "foo".to_string();
        let found = bk.search(SearchArgs { dir: Direction::Next, skip: false });
        assert!(found);
        // Should have jumped to the line containing "foo"
        let line_text = &bk.chapters[bk.chapter].text[bk.chapters[bk.chapter].lines[bk.line].0..bk.chapters[bk.chapter].lines[bk.line].1];
        assert!(line_text.contains("foo"));
    }

    #[test]
    fn test_search_no_match() {
        let ch = make_chapter("Test", "hello world");
        let mut bk = make_test_bk(vec![ch]);
        bk.query = "xyzzy".to_string();
        let found = bk.search(SearchArgs { dir: Direction::Next, skip: false });
        assert!(!found);
    }

    #[test]
    fn test_search_backward() {
        let ch = make_chapter("Test", "hello world foo bar baz");
        let mut bk = make_test_bk(vec![ch]);
        // Position at end of chapter
        bk.line = bk.chapters[0].lines.len() - 1;
        bk.query = "hello".to_string();
        let found = bk.search(SearchArgs { dir: Direction::Prev, skip: false });
        assert!(found);
        let line_text = &bk.chapters[bk.chapter].text[bk.chapters[bk.chapter].lines[bk.line].0..bk.chapters[bk.chapter].lines[bk.line].1];
        assert!(line_text.contains("hello"));
    }

    #[test]
    fn test_search_skip_current() {
        // "foo" appears on lines 1 and 3; skip should advance past the current line
        let ch = make_chapter("Test", "foo bar\nbaz qux\nfoo baz");
        let mut bk = make_test_bk(vec![ch]);
        bk.query = "foo".to_string();
        // First find the first "foo" on line 0
        let found = bk.search(SearchArgs { dir: Direction::Next, skip: false });
        assert!(found);
        let first_line = bk.line;
        // Skip to the next "foo" on line 2
        let found = bk.search(SearchArgs { dir: Direction::Next, skip: true });
        assert!(found);
        assert!(bk.line > first_line, "expected to advance past line {}", first_line);
    }

    #[test]
    fn test_search_empty_chapters() {
        let mut bk = make_test_bk(vec![]);
        bk.query = "anything".to_string();
        let found = bk.search(SearchArgs { dir: Direction::Next, skip: false });
        assert!(!found);
    }

    #[test]
    fn test_jump_reset() {
        let ch = make_chapter("Test", "line1\nline2\nline3\nline4");
        let mut bk = make_test_bk(vec![ch]);
        bk.mark('\'');
        bk.line = 2;
        bk.jump_reset();
        assert_eq!(bk.line, 0);
    }
    #[test]
    fn test_render_all_chapters_with_images() {
        // Load the real test EPUB and render every chapter
        let epub = crate::epub::Epub::new("test/test.epub", false)
            .expect("failed to load test EPUB");
        assert!(!epub.chapters.is_empty(), "EPUB has no chapters");
        assert!(!epub.imgs.is_empty(), "EPUB has no images");

        let mut bk = make_test_bk(epub.chapters);
        bk.imgs = epub.imgs;
        // Wrap chapter lines (normally done by Bk::new)
        for c in &mut bk.chapters {
            c.lines = wrap::wrap(&c.text, bk.max_width as usize);
        }

        // Render each chapter at line 0
        for ch_idx in 0..bk.chapters.len() {
            bk.chapter = ch_idx;
            bk.line = 0;
            let rendered = Page.render(&bk);
            assert!(!rendered.is_empty(), "chapter {} rendered empty", ch_idx);
        }
    }

    #[test]
    fn test_navigate_to_chapter_with_image() {
        // Simulate pressing ] to go to next chapter, then ] to return
        let epub = crate::epub::Epub::new("test/test.epub", false)
            .expect("failed to load test EPUB");
        let mut bk = make_test_bk(epub.chapters);
        bk.imgs = epub.imgs;
        // Wrap chapter lines (normally done by Bk::new)
        for c in &mut bk.chapters {
            c.lines = wrap::wrap(&c.text, bk.max_width as usize);
        }

        // Navigate to each chapter that has images
        for ch_idx in 0..bk.chapters.len() {
            bk.chapter = ch_idx;
            bk.line = 0;
            // Render should not panic
            let rendered = Page.render(&bk);
            // Check that any IMG markers are properly formed
            for line in &rendered {
                let clean = crate::wrap::strip_ansi(line);
                if clean.starts_with("[IMG][") {
                    assert!(
                        clean.len() >= 9,
                        "chapter {}: malformed IMG marker: '{}'",
                        ch_idx, clean
                    );
                }
            }
        }
    }

    #[test]
    fn test_exhaustive_scroll_all_chapters() {
        // Load EPUB and scroll through every line of every chapter
        let epub = crate::epub::Epub::new("test/test.epub", false)
            .expect("failed to load test EPUB");
        let mut bk = make_test_bk(epub.chapters);
        bk.imgs = epub.imgs;
        for c in &mut bk.chapters {
            c.lines = wrap::wrap(&c.text, bk.max_width as usize);
        }

        for ch_idx in 0..bk.chapters.len() {
            bk.chapter = ch_idx;
            let line_count = bk.chapters[ch_idx].lines.len();
            // Scroll through every possible line position
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
                        "chapter {} ({}): panic at line {}/{}: {}",
                        ch_idx, bk.chapters[ch_idx].title, line, line_count, msg
                    );
                }
            }
        }
    }

    #[test]
    fn test_img_parsing_all_chapters() {
        // Simulate the render closure's IMG parsing for every chapter
        let epub = crate::epub::Epub::new("test/test.epub", false)
            .expect("failed to load test EPUB");
        let mut bk = make_test_bk(epub.chapters);
        bk.imgs = epub.imgs;
        for c in &mut bk.chapters {
            c.lines = wrap::wrap(&c.text, bk.max_width as usize);
        }

        for ch_idx in 0..bk.chapters.len() {
            bk.chapter = ch_idx;
            bk.line = 0;
            let rendered = Page.render(&bk);
            for line in &rendered {
                let clean = crate::wrap::strip_ansi(line);
                if !clean.starts_with("[IMG][") {
                    continue;
                }
                // Simulate the render closure's IMG parsing
                if clean.len() < 9 {
                    continue;
                }
                let inner = &clean[6..clean.len() - 1];
                let parts: Vec<&str> = inner.split("][").collect();
                if parts.len() < 2 {
                    continue;
                }
                let (url, width_str) = (parts[0], parts[1]);
                let width: u32 = width_str
                    .trim_end_matches(|c: char| !c.is_ascii_digit())
                    .parse()
                    .unwrap_or(100);
                let _width = min(width, 100);

                // Verify the URL exists in imgs
                assert!(
                    bk.imgs.contains_key(url),
                    "chapter {} ({}): IMG URL '{}' not found in imgs (keys: {:?})",
                    ch_idx, bk.chapters[ch_idx].title, url,
                    bk.imgs.keys().take(5).collect::<Vec<_>>()
                );

                // Verify the image data is valid
                let buf = &bk.imgs[url];
                assert!(
                    image::load_from_memory(buf).is_ok(),
                    "chapter {}: failed to decode image '{}'",
                    ch_idx, url
                );
            }
        }
    }
}