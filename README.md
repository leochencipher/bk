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
- Cross-device sync (via content-hash + configurable save path)

# Install

from github:

    git clone https://github.com/leochencipher/bk
    cd bk
    cargo install --path .

# Usage

    Usage: bk [<path>] [-m] [-t] [-s <save_path>] [-w <width>]

    read a book

    Options:
      --bg              background color (eg 282a36)
      --fg              foreground color (eg f8f8f2)
      -m, --meta        print metadata and exit
      -t, --toc         start with table of contents open
      -s, --save-path   save path for reading progress (default: ~/book/bk)
      -w, --width       characters per line
      --help            display usage information

Running `bk` without a path will load the most recent EPUB.

## Cross-device sync

`bk` identifies books by SHA-256 content hash instead of file path, so the same EPUB opened from different directories on different machines is recognized as the same book. Set a shared sync directory for the save file:

    bk --save-path ~/Sync/bk/books.ron /path/to/book.epub

Then point Syncthing (or any sync tool) at `~/Sync/bk/` and your reading progress follows you everywhere. The save file is in human-readable RON format, so you can inspect or manually merge it if needed.

Type any function key (eg <kbd>F1</kbd>) to see the keybinds.

Check if your terminal supports italics:

    echo -e "\e[3mitalic\e[0m"

# Comparison

|   | bk | epr/epy |
| - | - | - |
| runtime deps | ❌ | python, curses |
| wide characters | ✔️ | ❌ |
| incremental search | ✔️ | ❌ |
| multi line search | ✔️ | ❌ |
| regex search | ❌ | ✔️ |
| links | ✔️ | ❌ |
| images | ✔️  | ✔️ |
| themes | ✔️ | ✔️ |
| choose file from history | ❌ | ✔️ |
| additional formats | ❌ | FictionBook, Mobi, AZW3 |
| external integration | see 1 | dictionary |

1: you can use the `--meta` switch to use `bk` as a file previewer with eg [nnn](https://github.com/jarun/nnn/)

# Inspiration

<https://github.com/wustho/epr>
