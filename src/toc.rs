use std::collections::HashMap;

use crate::epub;

// ── nested TOC ──

#[derive(Clone)]
pub(crate) struct TocItem {
    pub title: String,
    pub chapter: usize,
    pub depth: usize,
    pub has_children: bool,
    pub is_expanded: bool,
    pub is_last: bool,
    pub ancestors_last: Vec<bool>,
    pub toc_idx: usize,
}

pub(crate) fn count_toc(tree: &[epub::TocEntry]) -> usize {
    tree.iter().map(|e| 1 + count_toc(&e.children)).sum()
}

pub(crate) fn rebuild_toc_visible(
    tree: &[epub::TocEntry],
    expanded: &[bool],
    path_to_chapter: &HashMap<String, usize>,
) -> Vec<TocItem> {
    fn dfs(
        entries: &[epub::TocEntry],
        expanded: &[bool],
        path_to_chapter: &HashMap<String, usize>,
        idx: &mut usize,
        depth: usize,
        ancestors_last: Vec<bool>,
        visible: &mut Vec<TocItem>,
    ) {
        for (i, entry) in entries.iter().enumerate() {
            let my_idx = *idx;
            *idx += 1;
            let is_last = i == entries.len() - 1;
            let chapter = path_to_chapter.get(&entry.path).copied().unwrap_or(0);
            let mut my_ancestors = ancestors_last.clone();
            my_ancestors.push(is_last);
            let is_expanded = expanded.get(my_idx).copied().unwrap_or(true);
            visible.push(TocItem {
                title: entry.title.clone(),
                chapter,
                depth,
                has_children: !entry.children.is_empty(),
                is_expanded,
                is_last,
                ancestors_last: ancestors_last.clone(),
                toc_idx: my_idx,
            });
            if is_expanded && !entry.children.is_empty() {
                dfs(
                    &entry.children,
                    expanded,
                    path_to_chapter,
                    idx,
                    depth + 1,
                    my_ancestors,
                    visible,
                );
            }
        }
    }
    let mut visible = Vec::new();
    let mut idx = 0;
    dfs(tree, expanded, path_to_chapter, &mut idx, 0, vec![], &mut visible);
    visible
}