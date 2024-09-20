# bk

bk is a terminal EPUB reader, written in Rust. Forked from <https://github.com/aeosynth/bk>. I use this as a way to learn Rust (expecting ugly code :( ). Added couple of features,
image support by [viuer](https://github.com/atanunq/viuer).

# Features

- Cross platform - Linux, macOS and Windows support
- Single binary, instant startup
- EPUB 2/3 support
- Vim bindings
- Incremental search
- Bookmarks

# Install

from github:

    git clone https://github.com/leochencipher/bk
    cargo install --path bk

# Usage

    Usage: bk [<path>] [-m] [-t] [-w <width>]

    read a book

    Options:
      --bg              background color (eg 282a36)
      --fg              foreground color (eg f8f8f2)
      -m, --meta        print metadata and exit
      -t, --toc         start with table of contents open
      -w, --width       characters per line
      --help            display usage information

Running `bk` without a path will load the most recent EPUB.

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
