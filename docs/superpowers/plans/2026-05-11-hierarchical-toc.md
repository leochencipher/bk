# Hierarchical TOC Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add depth-aware hierarchical TOC display with indented sub-sections, each independently navigable via the existing `bk.links` fragment map.

**Architecture:** Add `TocEntry { title, depth, href }` to `epub.rs`; rewrite `epub2()`/`epub3()` to recurse through nested nav structures and return `Vec<TocEntry>`; add `toc: Vec<TocEntry>` and `toc_pos: usize` to `Bk`; update the `Toc` view to render from `bk.toc` with indentation and navigate via `bk.links`.

**Tech Stack:** Rust, roxmltree (XML traversal), crossterm (terminal rendering). No test framework — verify with `cargo check`, `cargo build`, and manual testing with an EPUB file.

---

## File Map

| File | Changes |
|------|---------|
| `src/epub.rs` | Add `TocEntry` struct; rewrite `epub2()` and `epub3()` to recurse and return `Vec<TocEntry>`; update `Epub` struct with `pub toc` field; update `get_spine()` to populate `self.toc` |
| `src/main.rs` | Add `toc: Vec<epub::TocEntry>` and `toc_pos: usize` to `Bk`; update `Bk::new()`; add `resolve_toc_href()` and `toc_pos_for_chapter()` helpers; update `Page::on_key` Tab handler |
| `src/view.rs` | Rewrite `Toc::prev()`, `next()`, `cursor()`, `click()`, `render()`; add `Toc::select()`; update key handlers |

---

### Task 1: Create feature branch

- [ ] **Step 1: Create and switch to feature branch**

```bash
cd /Users/schen/sideproject/bk
git checkout -b feature/hierarchical-toc
```

Expected: `Switched to a new branch 'feature/hierarchical-toc'`

---

### Task 2: Add `TocEntry` struct and rewrite `epub2()` / `epub3()`

**Files:**
- Modify: `src/epub.rs`

- [ ] **Step 1: Add `TocEntry` struct to `epub.rs`**

After the `Chapter` struct definition (after line 22), add:

```rust
#[derive(Clone)]
pub struct TocEntry {
    pub title: String,
    pub depth: usize,
    pub href: String,
}
```

- [ ] **Step 2: Add `pub toc` field to `Epub` struct**

Change the `Epub` struct definition (around line 24) to add the new field:

```rust
pub struct Epub {
    container: zip::ZipArchive<File>,
    rootdir: String,
    pub chapters: Vec<Chapter>,
    pub toc: Vec<TocEntry>,
    pub links: HashMap<String, (usize, usize)>,
    pub meta: String,
    pub imgs: HashMap<String, Vec<u8>>,
}
```

- [ ] **Step 3: Initialize `toc` field in `Epub::new()`**

In the `Epub::new()` function body (around line 36), add `toc: Vec::new()` to the struct literal:

```rust
let mut epub = Epub {
    container: zip::ZipArchive::new(file)?,
    rootdir: String::new(),
    chapters: Vec::new(),
    toc: Vec::new(),
    links: HashMap::new(),
    meta: String::new(),
    imgs: HashMap::new(),
};
```

- [ ] **Step 4: Replace `epub2()` with a recursive version**

Replace the entire `epub2()` function (lines 341–368) with:

```rust
fn epub2_navpoints(node: roxmltree::Node, depth: usize, entries: &mut Vec<TocEntry>) {
    for child in node.children().filter(|n| n.has_tag_name("navPoint")) {
        let href = child
            .descendants()
            .find(|n| n.has_tag_name("content"))
            .unwrap()
            .attribute("src")
            .unwrap()
            .to_string();
        let title = child
            .descendants()
            .find(|n| n.has_tag_name("text"))
            .unwrap()
            .text()
            .unwrap_or("")
            .to_string();
        entries.push(TocEntry { title, depth, href });
        epub2_navpoints(child, depth + 1, entries);
    }
}

fn epub2(doc: Document) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    if let Some(navmap) = doc.descendants().find(|n| n.has_tag_name("navMap")) {
        epub2_navpoints(navmap, 0, &mut entries);
    }
    entries
}
```

- [ ] **Step 5: Replace `epub3()` with a recursive version**

Replace the entire `epub3()` function (lines 369–393) with:

```rust
fn epub3_items(node: roxmltree::Node, depth: usize, entries: &mut Vec<TocEntry>) {
    for li in node.children().filter(|n| n.has_tag_name("li")) {
        if let Some(a) = li.children().find(|n| n.has_tag_name("a")) {
            let href = a.attribute("href").unwrap_or("").to_string();
            let title: String = a
                .descendants()
                .filter(Node::is_text)
                .map(|n| n.text().unwrap())
                .collect();
            entries.push(TocEntry { title, depth, href });
        }
        if let Some(ol) = li.children().find(|n| n.has_tag_name("ol")) {
            epub3_items(ol, depth + 1, entries);
        }
    }
}

fn epub3(doc: Document) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    if let Some(nav) = doc.descendants().find(|n| n.has_tag_name("nav")) {
        if let Some(ol) = nav.children().find(|n| n.has_tag_name("ol")) {
            epub3_items(ol, 0, &mut entries);
        }
    }
    entries
}
```

- [ ] **Step 6: Verify it compiles**

```bash
cargo check 2>&1 | head -40
```

Expected: errors about `epub2` / `epub3` call sites in `get_spine` (signature changed) — that's fine, fixed in the next task.

---

### Task 3: Update `get_spine()` to populate `self.toc`

**Files:**
- Modify: `src/epub.rs`

The current `get_spine()` passes a `&mut HashMap` to `epub2`/`epub3`. We change it to receive `Vec<TocEntry>` and derive the HashMap from it.

- [ ] **Step 1: Replace the nav-population block in `get_spine()`**

Find this section in `get_spine()` (around lines 145–161):

```rust
        if doc.root_element().attribute("version") == Some("3.0") {
            let path = manifest_node
                .children()
                .find(|n| n.attribute("properties") == Some("nav"))
                .unwrap()
                .attribute("href")
                .unwrap();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub3(doc, &mut nav);
        } else {
            let id = spine_node.attribute("toc").unwrap_or("ncx");
            let path = manifest.get(id).unwrap();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub2(doc, &mut nav);
        }
```

Replace it with:

```rust
        let toc_entries = if doc.root_element().attribute("version") == Some("3.0") {
            let path = manifest_node
                .children()
                .find(|n| n.attribute("properties") == Some("nav"))
                .unwrap()
                .attribute("href")
                .unwrap();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub3(doc)
        } else {
            let id = spine_node.attribute("toc").unwrap_or("ncx");
            let path = manifest.get(id).unwrap();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub2(doc)
        };

        // Build aux map (bare path → title) for chapter-boundary detection.
        // Use first match per path; strip fragment.
        let mut nav: HashMap<String, String> = HashMap::new();
        for entry in &toc_entries {
            let bare = entry.href.split('#').next().unwrap_or("").to_string();
            nav.entry(bare).or_insert_with(|| entry.title.clone());
        }

        self.toc = toc_entries;
```

- [ ] **Step 2: Remove the `let mut nav = HashMap::new();` line that was above the old block**

The `let mut nav = HashMap::new();` declaration that appears before the `if` block (around line 125) is now redundant — `nav` is declared inside the new block. Remove this line:

```rust
        let mut nav = HashMap::new();
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo check 2>&1 | head -40
```

Expected: errors about `bk.toc` / `bk.toc_pos` not existing yet — that's fine.

---

### Task 4: Add `toc` and `toc_pos` to `Bk`, update `Bk::new()`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add fields to `Bk` struct**

In the `Bk` struct definition (around line 146), add two fields after `chapter_line_offsets`:

```rust
    chapter_line_offsets: Vec<usize>,
    toc: Vec<epub::TocEntry>,
    toc_pos: usize,
```

- [ ] **Step 2: Add `resolve_toc_href` and `toc_pos_for_chapter` helpers to `impl Bk`**

Add these two methods anywhere in `impl Bk` (e.g., after `pad()`):

```rust
    /// Resolve a TOC href (possibly with fragment) to (chapter, byte_offset).
    /// Takes the last path component of href to match against bk.links keys.
    fn resolve_toc_href(&self, href: &str) -> Option<(usize, usize)> {
        // Get the last path component (filename + optional #fragment)
        let key = href.rsplit('/').next().unwrap_or(href);
        if let Some(&pos) = self.links.get(key) {
            return Some(pos);
        }
        // Retry without fragment
        let bare = key.split('#').next().unwrap_or(key);
        self.links.get(bare).copied()
    }

    /// Find the toc index that best matches the current reading chapter.
    /// Returns the last toc entry whose resolved chapter index ≤ bk.chapter.
    fn toc_pos_for_chapter(&self) -> usize {
        self.toc
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                self.resolve_toc_href(&e.href).map(|(ch, _)| (i, ch))
            })
            .filter(|&(_, ch)| ch <= self.chapter)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
```

- [ ] **Step 3: Initialize `toc` and `toc_pos` in `Bk::new()`**

In `Bk::new()`, the epub is partially moved into locals. Add `epub.toc` extraction alongside the existing `epub.chapters` and `epub.imgs` extraction:

Find (around line 179):
```rust
        let mut chapters = epub.chapters;
        let imgs = epub.imgs;
```

Replace with:
```rust
        let mut chapters = epub.chapters;
        let imgs = epub.imgs;
        let toc = epub.toc;
```

Then in the `Bk { ... }` struct literal (around line 195), add the two new fields:

```rust
            chapter_line_offsets,
            toc,
            toc_pos: 0,
```

- [ ] **Step 4: Verify it compiles cleanly**

```bash
cargo check 2>&1 | head -40
```

Expected: no errors (or only warnings). The view.rs errors about `bk.chapter` in Toc will appear in the next task.

---

### Task 5: Update `Toc` view — render and navigation

**Files:**
- Modify: `src/view.rs`

- [ ] **Step 1: Rewrite `Toc::prev()`, `next()`, and `cursor()`**

Replace the three methods on `Toc` (lines 111–121):

```rust
    fn prev(&self, bk: &mut Bk, n: usize) {
        bk.chapter = bk.chapter.saturating_sub(n);
        self.cursor(bk);
    }
    fn next(&self, bk: &mut Bk, n: usize) {
        bk.chapter = min(bk.chapters.len() - 1, bk.chapter + n);
        self.cursor(bk);
    }
    fn cursor(&self, bk: &mut Bk) {
        bk.cursor = min(bk.rows / 2, bk.chapter);
    }
```

With:

```rust
    fn prev(&self, bk: &mut Bk, n: usize) {
        bk.toc_pos = bk.toc_pos.saturating_sub(n);
        self.cursor(bk);
    }
    fn next(&self, bk: &mut Bk, n: usize) {
        if bk.toc.is_empty() { return; }
        bk.toc_pos = min(bk.toc.len() - 1, bk.toc_pos + n);
        self.cursor(bk);
    }
    fn cursor(&self, bk: &mut Bk) {
        bk.cursor = min(bk.rows / 2, bk.toc_pos);
    }
```

- [ ] **Step 2: Add `Toc::select()` method**

Add a new method after `cursor()`:

```rust
    fn select(&self, bk: &mut Bk) {
        if bk.toc.is_empty() { return; }
        let href = bk.toc[bk.toc_pos].href.clone();
        if let Some((ch, byte)) = bk.resolve_toc_href(&href) {
            bk.jump_byte(ch, byte);
        }
        bk.cursor = 0;
        bk.view = &Page;
    }
```

- [ ] **Step 3: Rewrite `Toc::click()`**

Replace (lines 122–129):

```rust
    fn click(&self, bk: &mut Bk, row: usize) {
        let start = bk.chapter - bk.cursor;
        if start + row < bk.chapters.len() {
            bk.chapter = start + row;
            bk.line = 0;
            bk.view = &Page;
        }
    }
```

With:

```rust
    fn click(&self, bk: &mut Bk, row: usize) {
        let start = bk.toc_pos - bk.cursor;
        if start + row < bk.toc.len() {
            bk.toc_pos = start + row;
            self.select(bk);
        }
    }
```

- [ ] **Step 4: Update key handler for Enter/Right**

In `Toc::on_key()`, replace (around line 150):

```rust
            Enter | Right | Char('l') => {
                bk.line = 0;
                bk.cursor = 0;
                bk.view = &Page;
            }
```

With:

```rust
            Enter | Right | Char('l') => {
                self.select(bk);
            }
```

- [ ] **Step 5: Rewrite `Toc::render()`**

Replace (lines 166–177):

```rust
    fn render(&self, bk: &Bk) -> Vec<String> {
        let start = bk.chapter - bk.cursor;
        let end = min(bk.chapters.len(), start + bk.rows);

        let mut arr = bk.chapters[start..end]
            .iter()
            .map(|c| c.title.clone())
            .collect::<Vec<String>>();
        arr[bk.cursor] = format!("{}{}{}", Reverse, arr[bk.cursor], NoReverse);
        arr
    }
```

With:

```rust
    fn render(&self, bk: &Bk) -> Vec<String> {
        if bk.toc.is_empty() {
            return vec![String::from("(no table of contents)")];
        }
        let start = bk.toc_pos - bk.cursor;
        let end = min(bk.toc.len(), start + bk.rows);

        let mut arr = bk.toc[start..end]
            .iter()
            .map(|e| format!("{}{}", "  ".repeat(e.depth), e.title))
            .collect::<Vec<String>>();
        arr[bk.cursor] = format!("{}{}{}", Reverse, arr[bk.cursor], NoReverse);
        arr
    }
```

- [ ] **Step 6: Verify it compiles**

```bash
cargo check 2>&1 | head -40
```

Expected: clean (or warnings only).

---

### Task 6: Initialize `toc_pos` when opening the TOC

**Files:**
- Modify: `src/main.rs`

When the user presses Tab to open the TOC, `toc_pos` should be pre-set to the best match for the current reading position.

- [ ] **Step 1: Update the Tab handler in `Page::on_key()`**

In `src/view.rs`, find the Tab case in `Page::on_key()` (around line 269):

```rust
            Tab => {
                bk.mark('\'');
                Toc.cursor(bk);
                bk.view = &Toc;
            }
```

Replace with:

```rust
            Tab => {
                bk.mark('\'');
                bk.toc_pos = bk.toc_pos_for_chapter();
                Toc.cursor(bk);
                bk.view = &Toc;
            }
```

- [ ] **Step 2: Final compile check**

```bash
cargo check 2>&1
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/epub.rs src/main.rs src/view.rs
git commit -m "feat: hierarchical TOC with depth indentation and sub-section navigation"
```

---

### Task 7: Build and manually test

- [ ] **Step 1: Build release binary**

```bash
cargo build --release 2>&1
```

Expected: `Finished release` with no errors.

- [ ] **Step 2: Test with an EPUB that has sub-sections**

```bash
./target/release/bk test.epub
```

Open the TOC with Tab. Verify:
- Top-level chapters appear at column 0
- Sub-sections are indented by 2 spaces per depth level
- Navigating to a sub-section (Enter) jumps to the correct position in the chapter
- Pressing Tab again re-opens TOC with cursor near the current position
- Esc / q / left-arrow / Tab returns to reading without moving position

- [ ] **Step 3: Test with an EPUB 2 (NCX) file if available**

Repeat Step 2 with an EPUB 2 file to verify `epub2()` recursion works correctly.

- [ ] **Step 4: Test edge case — EPUB with no TOC**

If an EPUB with an empty or missing nav is available, verify the TOC shows `(no table of contents)` instead of panicking.
