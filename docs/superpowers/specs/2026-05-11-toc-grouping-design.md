# TOC Grouping: Merge Unlabeled Spine Items

**Date:** 2026-05-11  
**Status:** Approved

## Problem

Many EPUBs (especially Calibre-generated ones) split large chapters into many small HTML files in the spine, but only label the "start" of each chapter in the NCX/nav TOC. For example, `test.epub` (The 4-Hour Body) has 153 spine items but only 68 NCX labels. The 85 unlabeled spine items currently fall back to their numeric spine index as their chapter title (`"0"`, `"1"`, `"2"`, …), making the TOC nearly unusable.

**Root cause:** `epub.rs:get_spine()` line 166:
```rust
let label = nav.remove(path).unwrap_or_else(|| i.to_string());
```

## Goal

Reduce 153 noisy TOC entries to 68 meaningful chapters by merging each unlabeled spine item into the preceding NCX-labeled chapter.

## Design

### `get_spine()` — change return type

From: `Vec<(String, String)>`  
To: `Vec<(String, Vec<String>)>` — each entry is `(label, [path, ...continuations])`

**Algorithm:**
1. Walk spine items in order.
2. Maintain a `pending: Vec<String>` buffer for leading unlabeled items (before any NCX entry is seen).
3. For each spine item:
   - Look up its path in `nav`.
   - **Labeled:** start a new group. Prepend `pending` paths (if any) to this group's path list, then clear `pending`.
   - **Unlabeled, group exists:** push path onto the current group's path list.
   - **Unlabeled, no group yet:** push path onto `pending`.
4. Return accumulated groups.

**Result for test.epub:** 153 spine items → 68 groups. The 9 pre-NCX files (titlepage, split_000–004, split_006–008) get prepended to the "LIST OF ILLUSTRATIONS" group.

### `get_chapters()` — iterate multiple paths per group

Signature change: accepts `Vec<(String, Vec<String>)>` instead of `Vec<(String, String)>`.

For each `(label, paths)` group:
1. Create one `Chapter` with `title = label`.
2. For each path in `paths`, render the HTML body into the same `Chapter` (same render logic as today, called in a loop).
3. Link/fragment registration happens inside the inner loop — each path registers its `relative` filename and `id` fragments as before.
4. Skip the entire group if all paths render empty text (preserves existing empty-skip behavior).

### Link/fragment map — no logic change

Each path in a group still inserts:
- `relative → (chapter_idx, 0)` into `self.links`
- `relative#id → (chapter_idx, byte_pos)` for each fragment anchor

The chapter index is the group index, not the path index. This is correct because all paths in a group are now one chapter.

## Files Changed

- `src/epub.rs` only:
  - `get_spine()` return type and implementation
  - `get_chapters()` parameter type and inner loop

## Non-Goals

- No change to how NCX/nav labels are parsed (epub2/epub3 functions unchanged).
- No HTML heading extraction fallback (out of scope for this change).
- No change to view.rs or main.rs.
