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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(title: &str, path: &str, children: Vec<epub::TocEntry>) -> epub::TocEntry {
        epub::TocEntry {
            title: title.to_string(),
            path: path.to_string(),
            children,
        }
    }

    #[test]
    fn test_count_toc_empty() {
        assert_eq!(count_toc(&[]), 0);
    }

    #[test]
    fn test_count_toc_single() {
        let tree = vec![make_entry("Ch1", "ch1.xhtml", vec![])];
        assert_eq!(count_toc(&tree), 1);
    }

    #[test]
    fn test_count_toc_nested() {
        let tree = vec![
            make_entry("Ch1", "ch1.xhtml", vec![
                make_entry("Sub1", "sub1.xhtml", vec![]),
                make_entry("Sub2", "sub2.xhtml", vec![]),
            ]),
            make_entry("Ch2", "ch2.xhtml", vec![]),
        ];
        assert_eq!(count_toc(&tree), 4);
    }

    #[test]
    fn test_rebuild_toc_visible_all_expanded() {
        let tree = vec![
            make_entry("Ch1", "ch1.xhtml", vec![
                make_entry("Sub1", "sub1.xhtml", vec![]),
            ]),
        ];
        let expanded = vec![true, true];
        let path_to_chapter = HashMap::from([
            ("ch1.xhtml".to_string(), 0usize),
            ("sub1.xhtml".to_string(), 1usize),
        ]);
        let visible = rebuild_toc_visible(&tree, &expanded, &path_to_chapter);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].title, "Ch1");
        assert_eq!(visible[0].depth, 0);
        assert_eq!(visible[0].chapter, 0);
        assert_eq!(visible[0].has_children, true);
        assert_eq!(visible[0].is_expanded, true);
        assert_eq!(visible[1].title, "Sub1");
        assert_eq!(visible[1].depth, 1);
        assert_eq!(visible[1].chapter, 1);
    }

    #[test]
    fn test_rebuild_toc_visible_collapsed() {
        let tree = vec![
            make_entry("Ch1", "ch1.xhtml", vec![
                make_entry("Sub1", "sub1.xhtml", vec![]),
            ]),
        ];
        let expanded = vec![false]; // Ch1 collapsed, Sub1's expanded state is never accessed
        let path_to_chapter = HashMap::from([
            ("ch1.xhtml".to_string(), 0usize),
        ]);
        let visible = rebuild_toc_visible(&tree, &expanded, &path_to_chapter);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].title, "Ch1");
        assert_eq!(visible[0].is_expanded, false);
    }

    #[test]
    fn test_rebuild_toc_visible_empty() {
        let visible = rebuild_toc_visible(&[], &[], &HashMap::new());
        assert!(visible.is_empty());
    }
}