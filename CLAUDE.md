# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**bk** is a terminal EPUB reader written in Rust. It renders EPUB 2/3 books in the terminal with vim-style navigation, incremental search, bookmarks, and image display.

## Commands

```bash
cargo build                  # debug build
cargo build --release        # optimized build
cargo check                  # fast type/borrow check (no binary)
cargo install --path .       # install `bk` binary locally

# Run with an EPUB file
bk book.epub
bk -t book.epub              # open table of contents immediately
bk -w 80 book.epub           # set line width
bk --meta book.epub          # print metadata and exit
```

There are no automated tests in this project.

## Architecture

Three source files with clean separation of concerns:

### `src/epub.rs` — EPUB parsing & rendering
- `Epub` opens the ZIP archive, parses the OPF manifest/spine, and exposes chapters, metadata, images, and a link map.
- `Chapter` holds the plain-text rendering of a chapter plus line-boundary offsets, formatting spans (bold/italic/underline), and internal link anchors.
- `render()` converts XML/HTML elements to plain text with inline formatting codes.

### `src/main.rs` — Application state & event loop
- `Bk` struct is the top-level state: current chapter/line, chapters vector, terminal dimensions, colors, bookmarks, and search state.
- `wrap()` does Unicode-aware line wrapping (CJK double-width aware).
- The event loop dispatches keyboard, mouse, and resize events to the active `View`.
- Reading progress (chapter index, line offset, bookmarks) is persisted to `~/.local/share/bk` using RON serialization.

### `src/view.rs` — UI views
- `View` trait: `render()`, `on_key()`, `on_mouse()`, `on_resize()`.
- Views: `Page` (main reading), `Toc`, `Search`, `Help`, `Mark`/`Jump` (bookmarks), `Metadata`.
- `Page` handles image rendering via `viuer` and single-click link navigation.
