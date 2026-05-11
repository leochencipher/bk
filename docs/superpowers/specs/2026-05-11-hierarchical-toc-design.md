# Hierarchical TOC Design

**Date:** 2026-05-11  
**Branch:** feature/hierarchical-toc (separate branch, experimental)

## Overview

Add hierarchical indentation to the TOC view so that sub-sections are visually nested and independently navigable. EPUB 2 (NCX) and EPUB 3 (nav) both encode nesting depth — currently the code discards it. This spec captures all depth and exposes it in the TOC.

## Data Model

### New struct in `epub.rs`

```rust
pub struct TocEntry {
    pub title: String,
    pub depth: usize,   // 0 = top-level, 1 = sub-section, etc.
    pub href: String,   // e.g. "ch01.html" or "ch01.html#section-2"
}
```

### `Epub` gains a new field

```rust
pub toc: Vec<TocEntry>,
```

Populated by `get_spine()` from the parsed nav document. Entries are in document order.

### `Bk` gains two new fields

```rust
toc: Vec<TocEntry>,
toc_pos: usize,   // cursor position within the toc list
```

`bk.chapters` is left entirely untouched — it continues to drive rendering, search, and pagination.

## Parsing Changes (`epub.rs`)

### `epub2()` (NCX format)

Replace the current flat `.descendants()` traversal with a recursive walk of the `navMap` → `navPoint` tree. Depth is determined by the number of `navPoint` ancestors. Returns `Vec<TocEntry>` in document order.

### `epub3()` (nav format)

Replace the current flat `<a>` descendant traversal with a recursive walk of nested `<ol>/<li>` elements. Depth = number of enclosing `<ol>` elements above the top-level one. Returns `Vec<TocEntry>` in document order.

### `get_spine()`

Serves two purposes after this change:

1. **Build `epub.toc`** — assign the `Vec<TocEntry>` returned by `epub2()`/`epub3()`.
2. **Build chapter groups** — same logic as today. Builds an auxiliary `HashMap<path, title>` from all toc entries (first match per path, any depth) to identify chapter boundaries in the spine.

The chapter-grouping logic itself is unchanged.

## TOC View Changes (`view.rs`)

### Rendering

Iterates `bk.toc` instead of `bk.chapters`. Each entry is rendered as:

```
"  ".repeat(entry.depth) + &entry.title
```

Example output:
```
Chapter One
  1.1 The Beginning
  1.2 The Middle
    1.2.1 A Subsection
Chapter Two
```

The cursor highlight (reverse video) is applied to the entry at `bk.toc_pos`.

### Navigation cursor

`bk.toc_pos` replaces `bk.chapter` as the TOC cursor. `prev()`/`next()`/`cursor()` helpers clamp to `bk.toc.len()`. Scroll window logic (`start = toc_pos - cursor`) is unchanged in structure.

### Selecting an entry (Enter / Right / click)

Resolves `entry.href` via `bk.links: HashMap<String, (usize, usize)>` (the existing fragment → (chapter, byte_offset) map) to get the target position, then calls `bk.jump_byte(chapter_idx, byte_offset)`. If `bk.links` has no entry for the full `path#fragment` href, strips the fragment and retries with the bare path. Falls back to chapter 0 if still not found.

### Opening the TOC

`bk.toc_pos` is initialized to the last toc entry whose resolved chapter index is ≤ `bk.chapter` (best-effort current-position hint).

## What Is Not Changed

- `bk.chapters` — untouched, drives all rendering/search/pagination
- Keyboard bindings — same keys, same semantics
- Mouse click navigation — same row-to-position logic, now indexes into `bk.toc`
- Persistence format — `bk.chapter` / `bk.line` / bookmarks unchanged
- Search — unchanged

## Branch Strategy

Implemented on `feature/hierarchical-toc`. Not merged to `master` until manually tested and approved.
