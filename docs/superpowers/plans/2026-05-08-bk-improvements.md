# bk Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the README, add a permanent status bar, replace reverse-video search highlight with amber, and add a dirty flag to skip unnecessary redraws.

**Architecture:** `bk.rows` is reduced by 1 to reserve the last terminal row for a status bar drawn directly in the `main.rs` render closure. `chapter_line_offsets` is cached on `Bk` for O(1) progress lookup. Search highlight changes are contained entirely in `Page::render()`. The dirty flag is set before every event dispatch and cleared by no-op handlers.

**Tech Stack:** Rust, crossterm 0.22, cargo

---

## File Map

| File | What changes |
|------|-------------|
| `README.md` | Description, install command, features list |
| `src/main.rs` | `Bk` struct (add `chapter_line_offsets`, `dirty`), `Bk::new()` (init), resize handler, `compute_line_offsets()`, `build_status()`, render closure (status bar), event loop (dirty flag) |
| `src/view.rs` | imports, `Page::render()` (amber highlight), `_ => ()` arms in `Toc`, `Page`, `Search` (dirty = false) |

---

## Task 1: README fixes

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Fix the opening description, install command, and features list**

Replace the entire opening paragraph and features list. Edit `README.md`:

```markdown
# bk

bk is a terminal EPUB reader, written in Rust. Forked from <https://github.com/aeosynth/bk>.
Supports EPUB 2/3 with vim-style navigation, images, incremental search, and bookmarks.

# Features

- Cross platform - Linux, macOS and Windows support
- Single binary, instant startup
- EPUB 2/3 support
- Vim bindings
- Incremental search
- Bookmarks
- Image display
```

Also fix the install command in the `# Install` section:

```
    git clone https://github.com/leochencipher/bk
    cd bk
    cargo install --path .
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: fix README description, install command, add image display feature"
```

---

## Task 2: chapter_line_offsets cache

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add `compute_line_offsets` free function**

Add this function in `src/main.rs` directly after the `wrap()` function (after line 91):

```rust
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
```

- [ ] **Step 2: Add `chapter_line_offsets` field to `Bk` struct**

In the `Bk` struct, add after `imgs`:

```rust
    imgs: HashMap<String, Vec<u8>>,
    chapter_line_offsets: Vec<usize>,
```

- [ ] **Step 3: Initialize field in `Bk::new()`**

In `Bk::new()`, after the loop that wraps chapters:

```rust
        for c in &mut chapters {
            c.lines = wrap(&c.text, width);
            // (title truncation code stays)
        }

        let chapter_line_offsets = compute_line_offsets(&chapters);
```

Then add `chapter_line_offsets` to the struct literal:

```rust
        let mut bk = Bk {
            // ... existing fields ...
            imgs,
            chapter_line_offsets,
        };
```

- [ ] **Step 4: Rebuild on resize**

In the `Event::Resize` arm of the `run()` event loop, add after the re-wrap loop:

```rust
                Event::Resize(cols, rows) => {
                    self.rows = rows as usize;
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
```

- [ ] **Step 5: cargo check**

```bash
cargo check
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: cache chapter line offsets for O(1) progress lookup"
```

---

## Task 3: Status bar

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Reduce `bk.rows` by 1 on init**

In `Bk::new()`, find:

```rust
        let (cols, rows) = terminal::size().unwrap();
```

Change the struct literal line:

```rust
            rows: rows as usize,
```

to:

```rust
            rows: (rows as usize).saturating_sub(1).max(1),
```

- [ ] **Step 2: Reduce `bk.rows` by 1 on resize**

In the `Event::Resize` arm, change:

```rust
                    self.rows = rows as usize;
```

to:

```rust
                    self.rows = (rows as usize).saturating_sub(1).max(1);
```

- [ ] **Step 3: Add `build_status` free function**

Add this after `compute_line_offsets()`:

```rust
fn build_status(bk: &Bk) -> String {
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

    let title = &bk.chapters[bk.chapter].title;
    let right = format!("  pg {}/{}  {}%", page, total_pages, pct);
    let width = bk.max_width as usize;
    let max_title = width.saturating_sub(right.len());
    let title_display = if title.chars().count() > max_title {
        title
            .chars()
            .take(max_title.saturating_sub(1))
            .collect::<String>()
            + "…"
    } else {
        title.clone()
    };
    let pad = width.saturating_sub(title_display.chars().count() + right.len());
    format!("{}{}{}", title_display, " ".repeat(pad), right)
}
```

- [ ] **Step 4: Draw status bar in render closure**

In the render closure inside `run()`, after the content rendering loop (and before the cursor position line), add:

```rust
            // Status bar on the last terminal row
            let status = build_status(bk);
            queue!(
                stdout,
                cursor::MoveTo(5, bk.rows as u16),
                Print(style::Attribute::Reverse),
                Print(&status),
                Print(style::Attribute::NoReverse),
            )
            .unwrap();

            queue!(stdout, cursor::MoveTo(5, bk.cursor as u16)).unwrap();
            stdout.flush().unwrap();
```

(Replace the existing two-line ending of the closure.)

- [ ] **Step 5: cargo check**

```bash
cargo check
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: add status bar showing chapter title, page, and overall progress"
```

---

## Task 4: Amber search highlight

**Files:**
- Modify: `src/view.rs`

- [ ] **Step 1: Update imports**

Change the crossterm style import at the top of `src/view.rs` from:

```rust
use crossterm::{
    event::{
        KeyCode::{self, *},
        MouseEvent, MouseEventKind,
    },
    style::Attribute::*,
};
```

to:

```rust
use crossterm::{
    event::{
        KeyCode::{self, *},
        MouseEvent, MouseEventKind,
    },
    style::{Attribute::*, Color, SetBackgroundColor, SetForegroundColor},
};
```

- [ ] **Step 2: Replace search highlight and update attrs type in `Page::render()`**

Replace the entire body of `Page::render()` in `src/view.rs`:

```rust
    fn render(&self, bk: &Bk) -> Vec<String> {
        let c = &bk.chapters[bk.chapter];
        let last_line = min(bk.line + bk.rows, c.lines.len());
        let text_start = c.lines[bk.line].0;
        let text_end = c.lines[last_line - 1].1;

        let mut search: Vec<(usize, String)> = Vec::new();
        if !bk.query.is_empty() {
            let len = bk.query.len();
            let hl_on =
                SetBackgroundColor(Color::Rgb { r: 250, g: 179, b: 135 }).to_string();
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
            head.into_iter().chain(tail).peekable()
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
```

Note: `bk.colors` is accessed here — `Page::render` receives `bk: &Bk` so this compiles. The `Colors` struct has `foreground: Option<Color>` and `background: Option<Color>`.

- [ ] **Step 3: cargo check**

```bash
cargo check
```

Expected: no errors or warnings.

- [ ] **Step 4: Commit**

```bash
git add src/view.rs
git commit -m "feat: amber background highlight for search matches"
```

---

## Task 5: Dirty flag

**Files:**
- Modify: `src/main.rs`, `src/view.rs`

- [ ] **Step 1: Add `dirty` field to `Bk` struct**

In `src/main.rs`, add `dirty: bool` after `quit`:

```rust
pub struct Bk<'a> {
    quit: bool,
    dirty: bool,
    chapters: Vec<epub::Chapter>,
    // ... rest unchanged
```

Initialize it in `Bk::new()`:

```rust
        let mut bk = Bk {
            quit: false,
            dirty: true,
            // ... rest unchanged
```

- [ ] **Step 2: Update event loop to set dirty before dispatch and check before render**

In `run()`, replace the entire event dispatch and render block:

```rust
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
```

- [ ] **Step 3: Add `bk.dirty = false` to no-op `_ => ()` arms in `src/view.rs`**

There are five `_ => ()` arms that should suppress re-renders. Change each from `_ => ()` to `_ => { bk.dirty = false; }`:

**Toc::on_key** (around line 163):
```rust
            _ => { bk.dirty = false; }
```

**Toc::on_mouse** (around line 142):
```rust
            _ => { bk.dirty = false; }
```

**Page::on_key** (around line 310):
```rust
            _ => { bk.dirty = false; }
```

**Page::on_mouse** (around line 263):
```rust
            _ => { bk.dirty = false; }
```

**Search::on_key** (around line 424):
```rust
            _ => { bk.dirty = false; }
```

- [ ] **Step 4: cargo check**

```bash
cargo check
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/view.rs
git commit -m "perf: skip render on no-op key events with dirty flag"
```

---

## Task 6: Build and smoke test

- [ ] **Step 1: Release build**

```bash
cargo build --release
```

Expected: compiles cleanly with no warnings.

- [ ] **Step 2: Manual smoke test**

```bash
./target/release/bk <any-epub-file>
```

Verify:
- Status bar visible at bottom with chapter title, page, and %
- Searching (`/`) highlights matches in amber
- All navigation keys work (j/k, space, [, ], Tab for TOC)
- Resize terminal — status bar stays at bottom

- [ ] **Step 3: Final commit if any fixups needed**

```bash
git add -p
git commit -m "fix: <description of any fixup>"
```
