# bk Improvements Design

**Date:** 2026-05-08
**Scope:** README fixes, status bar, amber search highlight, dirty-flag render optimization

---

## 1. README Fixes

Three targeted edits to `README.md`:

1. **Opening description** — Remove the self-deprecating "I use this as a way to learn Rust (expecting ugly code)" sentence. Rewrite the intro as a straightforward project description and add image display to it.
2. **Install command** — Change `cargo install --path bk` to `cargo install --path .` (the current command is wrong when already inside the repo directory).
3. **Features list** — Add `Image display` as a bullet point alongside the existing features.

---

## 2. Status Bar (bottom row)

### Goal
Show reading progress permanently at the bottom of the screen without pressing `i`.

### Data to display
```
{chapter_title truncated}  pg {page}/{total_pages}  {overall%}
```
- `chapter_title`: `bk.chapters[bk.chapter].title`, truncated to fit
- `page / total_pages`: page within the current chapter (same logic as `Metadata` view)
- `overall%`: percentage of total book lines read

### Layout approach
`bk.rows` is set to `terminal_rows - 1` on init and on every resize. All existing views (Page, TOC, Help, Search, Metadata) already use `bk.rows` as their content height limit — they automatically use one fewer row with no changes to view logic.

The `main.rs` render closure draws the status bar on the final terminal row (`bk.rows as u16`) with reverse-video styling after rendering the view content.

### Progress computation
Overall progress requires summing line counts across chapters, which is O(chapters). Cache `chapter_line_offsets: Vec<usize>` on `Bk`:
- Built once during `Bk::new()` after chapters are wrapped
- Rebuilt during resize (after re-wrapping chapters)
- `chapter_line_offsets[i]` = total lines in chapters `0..i`
- Overall progress = `(chapter_line_offsets[bk.chapter] + bk.line) / total_lines`

### Status bar render (in `main.rs` render closure)
```
row = terminal_rows - 1 = bk.rows
style = Reverse (swaps fg/bg for a classic status bar look)
content = build_status(bk)  // returns a padded String
```

`build_status` is a free function in `main.rs`:
- Computes page, total_pages, overall% using cached offsets
- Truncates chapter title so the full string fits within `bk.max_width` columns
- Pads with spaces to fill the row

---

## 3. Amber Search Highlight

### Goal
Replace the current "reverse video" search highlight with an amber background (`Rgb{250, 179, 135}`) for clear, theme-independent visibility.

### Current implementation
`Page::render()` builds a merged `Vec<(usize, Attribute)>` from two sources:
- `base`: formatting spans from `c.attrs` (Bold, Italic, Underlined)
- `search`: start/end positions for query matches, using `Reverse`/`NoReverse`

At render time, `attr.to_string()` is called to embed the escape sequence into the output string.

### Change
Change the merged collection type from `Vec<(usize, Attribute)>` to `Vec<(usize, String)>` where each `String` is a pre-serialized ANSI escape sequence.

- **Base attrs**: call `.to_string()` on each `Attribute` upfront when building the iterator (no behavior change)
- **Search start-of-match**: emit `SetBackgroundColor(Color::Rgb{r:250,g:179,b:135}).to_string()`
- **Search end-of-match**: emit `format!("{}{}", SetForegroundColor(Color::Reset), SetBackgroundColor(Color::Reset))` to restore defaults (the frame-level `SetColors(bk.colors)` already set the base colors for this render pass, so a reset returns to them)

This change is contained entirely within `Page::render()`.

---

## 4. Dirty-Flag Render Optimization

### Goal
Skip the `view.render()` call (which allocates a `Vec<String>` every frame) for events that produce no state change — primarily no-op key presses (keys that hit `_ => ()` in a view handler).

### Implementation
Add `dirty: bool` to `Bk`, initialized to `true`.

Every view handler (`on_key`, `on_mouse`, `on_resize`) that actually mutates `Bk` state sets `bk.dirty = true`. Handlers that match `_ => ()` do not set it. The event loop checks the flag before rendering:

```rust
if self.quit { break; }
if self.dirty {
    render(self);
    self.dirty = false;
}
```

Mouse move events already use `continue` before reaching the render call, so they are unaffected. The main win is eliminating spurious redraws from unrecognized key presses.

### Where to set dirty
- `on_key` in every view: set `bk.dirty = true` at the top of each arm that changes state (all arms except `_ => ()`)
- `on_resize` in all views: always dirty (resize always needs redraw)
- `on_mouse` in all views: always dirty when the handler fires (mouse moves already filtered)

---

## Files Changed

| File | Changes |
|------|---------|
| `README.md` | Description, install command, features list |
| `src/main.rs` | `bk.rows = terminal_rows - 1`, add `chapter_line_offsets`, `build_status()`, status bar render, dirty flag |
| `src/view.rs` | `Page::render()` attrs type change for amber highlight |

`src/epub.rs` — no changes needed.

---

## Out of Scope

- Configurable status bar content or colors
- Regex search
- File history picker
