# BK — Codebase Analysis

> Terminal-based EPUB reader written in Rust.

## Architecture Overview

```
src/
├── main.rs    ← CLI args, state management, main loop, rendering
├── view.rs    ← View trait + Page/Toc/Mark/Jump/Search/Help views
├── epub.rs    ← EPUB parsing (zip/xml), chapter extraction, image loading
└── Cargo.toml
```

**Core data flow:**
1. `init()` — parse CLI args, load/save state from RON file
2. `Epub::new()` — unpack ZIP, parse OPF/NCX/NAV, extract text+images+links
3. `Bk::new()` — wrap text to terminal width, build line offset table
4. `Bk::run()` — event loop (keyboard/mouse/resize) → render → repeat

---

## Key Components

### `Bk` struct (`main.rs`)
The central application state. Holds:
- **Book data**: chapters, images, links, metadata
- **Position**: current chapter, line, cursor row
- **Marks**: `HashMap<char, (chapter, line)>` for bookmarking
- **Layout**: terminal size, max width, colors
- **View**: references a `&'a dyn View` (Page, Toc, etc.)

### `View` trait (`view.rs`)
State machine abstraction for different UI modes:
| View      | Purpose                     |
|-----------|-----------------------------|
| `Page`    | Main reading view           |
| `Toc`     | Table of contents           |
| `Search`  | Incremental search          |
| `Help`    | Keybindings reference       |
| `Metadata`| Progress/metadata display   |
| `Mark`    | Set mark (transient)        |
| `Jump`    | Jump to mark (transient)    |

### `Chapter` struct (`epub.rs`)
- `text: String` — flattened plain text
- `lines: Vec<(usize, usize)>` — byte offset pairs for each wrapped line
- `attrs: Vec<(usize, Attribute, Attributes)>` — style transitions
- `links: Vec<(usize, usize, String)>` — clickable link regions
- `frag: Vec<(String, usize)>` — fragment IDs for internal navigation

---

## Critical Code Paths

### Text Wrapping (`main.rs:30–77`)
```rust
fn wrap(text: &str, max_cols: usize) -> Vec<(usize, usize)>
```
Breaks text into `(byte_start, byte_end)` pairs respecting terminal width.
Handles CJK characters, hyphenation, and forced breaks on long words.

### Rendering Loop (`main.rs:118–200`)
- Uses crossterm's `queue!` for batched terminal writes
- `Page::render()` produces lines with embedded crossterm ANSI attributes
- Special `[IMG][url][width]` markers trigger `viuer::print()` for inline images
- Search highlights use `Reverse`/`NoReverse` attribute pairs

### Image Display (`main.rs:158–187`)
- Images are extracted from EPUB ZIP during parsing
- Rendered via `viuer` library with aspect-ratio-aware sizing
- Positioned in a right-side panel (`x: bk.max_width + 10`)
- **BUG**: `println!` on lines 167-168 leaks image info to stdout (should be suppressed)

### Mouse Click (`view.rs:200–240`)
Clicking a word:
1. Converts mouse position to byte offset
2. If word is a link, navigates to target

---

## Issues & Bugs

### 🔴 Critical
| # | Location | Issue | Status |
|---|----------|-------|--------|
| 1 | `main.rs:167-168` | `println!` leaks image URL/width to stdout during rendering | ✅ Fixed — removed `println!`, added graceful skip for missing images |
| 2 | `main.rs:180` | `println!` corrupts terminal output | ✅ Fixed — removed |
| 3 | `main.rs:178` | viuer screen management conflict | ⚠️ Known — viuer uses its own screen management; no easy fix without rewriting image rendering |

### 🟡 Moderate
| # | Location | Issue | Status |
|---|----------|-------|--------|
| 4 | `main.rs:160` | `unwrap()` on `bk.imgs.get(url)` — panics if image URL in text not found in imgs map | ✅ Fixed — `match` with graceful skip |
| 5 | `main.rs:154` | `unwrap()` on `image::load_from_memory` — panics on corrupt image data | ✅ Fixed — `match` with graceful skip |
| 6 | `main.rs:173` | `ratio: u32 = 2` is hardcoded — should be configurable | ✅ Fixed — extracted to `const IMAGE_RATIO` |
| 7 | `main.rs:192` | `last_y` never resets between chapters | ⚠️ Known — would require restructuring image rendering |

| 10 | `view.rs:138` | `unwrap()` on `bk.links.get(url)` — panics | ✅ Fixed — `if let Some(...)` check |
| 11 | `epub.rs:58` | `unwrap()` on `self.container.by_name(name)` — panics | ⚠️ Known — would require changing `get_text` return type throughout |
| 12 | `epub.rs:102` | `unwrap()` on `n.text()` — panics | ✅ Fixed — `if let Some(text)` pattern |

### 🟢 Minor / Tech Debt
| # | Location | Issue | Status |
|---|----------|-------|--------|
| 13 | `main.rs:7` | `Color::{self, Rgb}` — unused `Rgb` import | ✅ Fixed — changed to `Color` |
| 14 | `main.rs:19` | `use std::i16` — unnecessary | ✅ Fixed — removed |
| 15 | `main.rs:141` | `let mut conf;` — late initialization | ✅ Fixed — `let conf = if ... { ... } else { ... }` |
| 16 | `view.rs:11` | Unused import: `Attribute::*` | ✅ Fixed — explicit imports |
| 17 | `epub.rs:134` | Empty match arms `();` | ✅ Fixed — cleaned up match arms |
| 18 | `view.rs:246` | `// lazy` comment — resize handler | ✅ Fixed — `saturating_sub(1)` guard |
| 19 | `main.rs:288` | `// XXX oh god what` — ugly color parsing | ✅ Fixed — inlined with `as_deref()` |
| 20 | `main.rs:101` | `// XXX marks aren't updated` — resize invalidates marks | ⚠️ Known — out of scope for this pass |

---

## External Dependencies

| Crate | Purpose |
|-------|---------|
| `crossterm` | Terminal control (raw mode, cursor, colors, events) |
| `viuer` | Inline image rendering in terminal |
| `image` | Image loading/decoding |
| `roxmltree` | XML parsing for EPUB (OPF, NCX, NAV) |
| `zip` | ZIP archive reading for EPUB files |
| `serde` / `ron` | State serialization (save/restore) |
| `argh` | CLI argument parsing |
| `unicode-width` | CJK character width calculation |


---

## Notable Design Decisions

1. **Flat text model**: Chapters are flattened to a single `String` with `(start, end)` byte pairs for lines. This simplifies wrapping but makes attribute tracking manual.

2. **View as trait object**: `&'a dyn View` avoids enum dispatch overhead and makes adding views easy, but loses exhaustiveness checking.

3. **Inline image rendering**: Uses `viuer` to print images in a side panel. This is fragile — it mixes viuer's screen management with crossterm's.

4. **Save format**: RON (Rusty Object Notation) for human-readable state persistence. One save file tracks all books.

5. **CJK support**: `unicode-width` for display width.

---

## Suggested Improvements

1. **Reset `last_y` per image group** (#7): Reset when entering new chapter or section
2. **Add tests**: `wrap()` function is pure and easily testable
3. **Handle missing chapter files gracefully** (#11): Change `get_text` to return `io::Result<String>` and propagate errors
4. **Configurable image ratio**: Make `IMAGE_RATIO` a CLI option
5. **Update marks on resize** (#20): Recalculate mark positions when terminal resizes
6. **Consider `Result` return types** for `Epub::new` to propagate parsing errors

