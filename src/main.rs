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
    process::exit,
    u16, u32,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use viuer::Config;

mod view;
use view::{Page, Toc, View};

mod epub;

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
    cols: u16,
    rows: usize,
    max_width: u16,
    // view state
    view: &'a dyn View,
    cursor: usize,
    dir: Direction,
    meta: Vec<String>,
    query: String,
    imgs: HashMap<String, Vec<u8>>,
    chapter_line_offsets: Vec<usize>,
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

        let mut bk = Bk {
            quit: false,
            dirty: true,
            chapters,
            chapter: 0,
            line: 0,
            mark: HashMap::new(),
            links: epub.links,
            colors: args.colors,
            cols,
            rows: (rows as usize).saturating_sub(1).max(1),
            max_width: args.width,
            view: if args.toc { &Toc } else { &Page },
            cursor: 0,
            dir: Direction::Next,
            meta,
            query: String::new(),
            imgs,
            chapter_line_offsets,
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
                if !line.starts_with("[IMG][") {
                    let curlen = line.width_cjk();
                    if line.starts_with(" ") {
                        queue!(
                            stdout,
                            cursor::MoveTo(5, i as u16),
                            SetColors(Colors::new(
                                Color::Rgb {
                                    r: 250,
                                    g: 179,
                                    b: 135
                                },
                                Color::Rgb {
                                    r: 49,
                                    g: 50,
                                    b: 68
                                }
                            )),
                            Print(format!(
                                "{}{}",
                                &line[3..],
                                " ".repeat(bk.max_width as usize - curlen + 11)
                            )),
                            ResetColor
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
                    // [IMG][url][width]
                    let parts: Vec<&str> = line[6..line.len()-1].split("][").collect();
                    let (url, width_str) = (parts[0], parts[1]);
                        let width: u32 = width_str.trim_end_matches(|c: char| !c.is_ascii_digit())
                            .parse()
                            .unwrap_or(100);
                        let width = min(width, 100);
                    let buf = bk.imgs.get(url).unwrap();
                    let img = image::load_from_memory(&buf)
                        .expect("Data from stdin could not be decoded.");
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
                    let (_print_width, print_height) =
                        viuer::print(&display_img, &conf).expect("Image printing failed.");
                    queue!(
                        stdout,
                        cursor::MoveTo(bk.max_width + 7, last_y as u16),
                        Print(format!("[{}]", img_index))
                    )
                    .unwrap();
                    img_index = img_index + 1;
                    last_y = last_y + print_height as i16 + 2;
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
                // Purple/indigo background with bright white text for left (title) section
                SetColors(Colors::new(
                    Color::Rgb { r: 249, g: 226, b: 175 }, // Catppuccin yellow
                    Color::Rgb { r: 30, g: 30, b: 46 },    // dark base
                )),
                Print(&left_display),
                Print(" ".repeat(pad)),
                // Teal/cyan accent for right section
                SetColors(Colors::new(
                    Color::Rgb { r: 137, g: 220, b: 235 }, // Catppuccin sky
                    Color::Rgb { r: 30, g: 30, b: 46 },
                )),
                Print(&right),
                ResetColor,
            )
            .unwrap();

            queue!(stdout, cursor::MoveTo(5, bk.cursor as u16)).unwrap();
            stdout.flush().unwrap();
        };

        render(self);
        loop {
            let event = event::read()?;

            match event {
                Event::Key(e) => {
                    self.dirty = true;
                    self.view.on_key(self, e.code);
                }
                Event::Mouse(e) => {
                    if e.kind == event::MouseEventKind::Moved {
                        continue;
                    }
                    self.dirty = true;
                    self.view.on_mouse(self, e);
                }
                Event::Resize(cols, rows) => {
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
                    // XXX marks aren't updated
                }
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
}

struct Props {
    colors: Colors,
    chapter: usize,
    byte: usize,
    width: u16,
    toc: bool,
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
    let fg = args
        .fg
        .map(|s| Rgb {
            r: u8::from_str_radix(&s[0..2], 16).unwrap(),
            g: u8::from_str_radix(&s[2..4], 16).unwrap(),
            b: u8::from_str_radix(&s[4..6], 16).unwrap(),
        })
        .unwrap_or(style::Color::Reset);
    let bg = args
        .bg
        .map(|s| Rgb {
            r: u8::from_str_radix(&s[0..2], 16).unwrap(),
            g: u8::from_str_radix(&s[2..4], 16).unwrap(),
            b: u8::from_str_radix(&s[4..6], 16).unwrap(),
        })
        .unwrap_or(style::Color::Reset);

    Ok(State {
        path,
        save,
        save_path,
        meta: args.meta,
        bk: Props {
            colors: Colors::new(fg, bg),
            chapter,
            byte,
            width: args.width,
            toc: args.toc,
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
