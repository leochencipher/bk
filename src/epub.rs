use crossterm::style::{Attribute, Attributes};
use roxmltree::{Document, Node, ParsingOptions};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read},
};

#[derive(Clone)]
pub struct Chapter {
    pub title: String,
    // single string for search
    pub text: String,
    pub lines: Vec<(usize, usize)>,
    // crossterm gives us a bitset but doesn't let us diff it, so store the state transition
    pub attrs: Vec<(usize, Attribute, Attributes)>,
    // raw ANSI color-change sequences keyed by byte position
    pub color_attrs: Vec<(usize, String)>,
    pub links: Vec<(usize, usize, String)>,
    // (start, end, level 0-5) for heading spans
    pub heading_spans: Vec<(usize, usize, usize)>,
    frag: Vec<(String, usize)>,
    state: Attributes,
}

#[derive(Clone, Debug)]
pub struct TocEntry {
    pub title: String,
    pub path: String,
    pub children: Vec<TocEntry>,
}

pub struct Epub {
    container: zip::ZipArchive<File>,
    rootdir: String,
    pub chapters: Vec<Chapter>,
    pub links: HashMap<String, (usize, usize)>,
    pub meta: String,
    pub imgs: HashMap<String, Vec<u8>>,
    pub toc_tree: Vec<TocEntry>,
    pub path_to_chapter: HashMap<String, usize>,
}

impl Epub {
    pub fn new(path: &str, meta: bool) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut epub = Epub {
            container: zip::ZipArchive::new(file)?,
            rootdir: String::new(),
            chapters: Vec::new(),
            links: HashMap::new(),
            meta: String::new(),
            imgs: HashMap::new(),
            toc_tree: Vec::new(),
            path_to_chapter: HashMap::new(),
        };
        let chapters = epub.get_spine();
        if !meta {
            epub.get_chapters(chapters);
        }
        Ok(epub)
    }
    fn get_text(&mut self, name: &str) -> String {
        let mut bytes = Vec::new();
        self.container
            .by_name(name)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        decode_text(&bytes)
    }
    fn get_chapters(&mut self, spine: Vec<(String, Vec<String>)>) {
        for (title, paths) in spine {
            let state = Attributes::default();
            let mut c = Chapter {
                title,
                text: String::new(),
                lines: Vec::new(),
                attrs: vec![(0, Attribute::Reset, state)],
                color_attrs: Vec::new(),
                state,
                links: Vec::new(),
                heading_spans: Vec::new(),
                frag: Vec::new(),
            };
            for path in &paths {
                // https://github.com/RazrFalcon/roxmltree/issues/12
                // UnknownEntityReference for HTML entities
                let cpath = format!("{}{}", self.rootdir, path);
                let xml = self.get_text(&cpath);
                let (chapterpath, _) = cpath.rsplit_once('/').unwrap_or(("", ""));
                let opt = ParsingOptions { allow_dtd: true };
                let doc = Document::parse_with_options(&xml, opt);
                let doc = match doc {
                    Ok(v) => v,
                    Err(_e) => continue,
                };
                let body = doc.root_element().last_element_child().unwrap();
                let link_start = c.links.len();
                render(body, &mut c, self, chapterpath);
                let relative = path.rsplit('/').next().unwrap();
                self.links
                    .insert(relative.to_string(), (self.chapters.len(), 0));
                for (id, pos) in c.frag.drain(..) {
                    let url = format!("{}#{}", relative, id);
                    self.links.insert(url, (self.chapters.len(), pos));
                }
                for link in c.links[link_start..].iter_mut() {
                    if link.2.starts_with('#') {
                        link.2.insert_str(0, relative);
                    }
                }
            }
            if c.text.trim().is_empty() {
                continue;
            }
            self.chapters.push(c);
        }
    }
    fn get_spine(&mut self) -> Vec<(String, Vec<String>)> {
        let xml = self.get_text("META-INF/container.xml");
        let doc = Document::parse(&xml).unwrap();
        let path = doc
            .descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .unwrap()
            .attribute("full-path")
            .unwrap();
        let xml = self.get_text(path);
        let doc = Document::parse(&xml).unwrap();

        // zip expects unix path even on windows
        self.rootdir = match path.rfind('/') {
            Some(n) => &path[..=n],
            None => "",
        }
        .to_string();
        let mut manifest = HashMap::new();
        let mut nav = HashMap::new();
        let mut children = doc.root_element().children().filter(Node::is_element);
        let meta_node = children.next().unwrap();
        let manifest_node = children.next().unwrap();
        let spine_node = children.next().unwrap();

        meta_node.children().filter(Node::is_element).for_each(|n| {
            let name = n.tag_name().name();
            let text = n.text();
            if text.is_some() && name != "meta" {
                self.meta
                    .push_str(&format!("{}: {}\n", name, text.unwrap()));
            }
        });
        manifest_node
            .children()
            .filter(Node::is_element)
            .for_each(|n| {
                manifest.insert(n.attribute("id").unwrap(), n.attribute("href").unwrap());
            });
        let toc_path = if doc.root_element().attribute("version") == Some("3.0") {
            let path = manifest_node
                .children()
                .find(|n| n.attribute("properties") == Some("nav"))
                .unwrap()
                .attribute("href")
                .unwrap()
                .to_string();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub3(doc, &mut self.toc_tree, &mut nav);
            path
        } else {
            let id = spine_node.attribute("toc").unwrap_or("ncx");
            let path = manifest.get(id).unwrap().to_string();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub2(doc, &mut self.toc_tree, &mut nav);
            path
        };
        // Resolve nav paths relative to the nav/NCX document's directory.
        // Nav hrefs are relative to the nav doc, manifest hrefs are relative to rootdir.
        if let Some(n) = toc_path.rfind('/') {
            let base = &toc_path[..=n];
            nav = nav
                .into_iter()
                .map(|(k, v)| (format!("{}{}", base, k), v))
                .collect();
            fn resolve_toc_paths(tree: &mut Vec<TocEntry>, prefix: &str) {
                for entry in tree {
                    entry.path = format!("{}{}", prefix, entry.path);
                    resolve_toc_paths(&mut entry.children, prefix);
                }
            }
            resolve_toc_paths(&mut self.toc_tree, base);
        }
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        let mut pending: Vec<String> = Vec::new();
        let mut chapter_idx = 0usize;

        for n in spine_node.children().filter(Node::is_element) {
            let id = n.attribute("idref").unwrap();
            let path = manifest.remove(id).unwrap().to_string();
            match nav.remove(path.as_str()) {
                Some(label) => {
                    let mut paths: Vec<String> = pending.drain(..).collect();
                    paths.push(path.clone());
                    groups.push((label, paths));
                    for p in &groups.last().unwrap().1 {
                        self.path_to_chapter.insert(p.clone(), chapter_idx);
                    }
                    chapter_idx += 1;
                }
                None => {
                    if groups.is_empty() {
                        pending.push(path.clone());
                    } else {
                        groups.last_mut().unwrap().1.push(path.clone());
                        self.path_to_chapter.insert(path, chapter_idx.saturating_sub(1));
                    }
                }
            }
        }
        groups
    }
}

impl Chapter {
    fn render(
        &mut self,
        n: Node,
        open: Attribute,
        close: Attribute,
        ea: &mut Epub,
        chapterpath: &str,
    ) {
        self.state.set(open);
        self.attrs.push((self.text.len(), open, self.state));
        self.render_text(n, ea, chapterpath);
        self.state.unset(open);
        self.attrs.push((self.text.len(), close, self.state));
    }
    fn render_text(&mut self, n: Node, ea: &mut Epub, chapterpath: &str) {
        for child in n.children() {
            render(child, self, ea, chapterpath);
        }
    }
}

fn render(n: Node, c: &mut Chapter, ea: &mut Epub, chapterpath: &str) {
    if n.is_text() {
        let text = n.text().unwrap();
        let content: Vec<_> = text.split_ascii_whitespace().collect();

        if text.starts_with(char::is_whitespace) {
            c.text.push(' ');
        }
        c.text.push_str(&content.join(" "));
        if text.ends_with(char::is_whitespace) {
            c.text.push(' ');
        }
        return;
    }

    if let Some(id) = n.attribute("id") {
        c.frag.push((id.to_string(), c.text.len()));
    }

    match n.tag_name().name() {
        "br" => c.text.push('\n'),
        "hr" => c.text.push_str("\n* * *\n"),
        "img" => match n.attribute("src") {
            Some(url) => {
                let mut ipath: Vec<&str> = Vec::new();
                let psplit = &format!("{}/{}", chapterpath, url);
                psplit.split('/').for_each(|comp| {
                    match comp {
                        "" => (),
                        "." => (),
                        ".." => {
                            ipath.pop();
                            ();
                        }
                        pcomp => {
                            ipath.push(pcomp);
                            ();
                        }
                    };
                });
                let mut buffer = Vec::new();
                match ea.container.by_name(ipath.join("/").as_str()) {
                    Ok(mut f) => {
                        f.read_to_end(&mut buffer).unwrap();
                        let width = n.attribute("width")
                            .or_else(|| n.attribute("style")
                                .and_then(|s| s.split(';')
                                    .find(|p| p.trim().starts_with("width:"))
                                    .map(|w| w.split(':').nth(1).unwrap().trim())))
                            .unwrap_or("auto");
                        ea.imgs.insert(String::from(url), buffer);
                        c.text.push_str(&format!("\n[IMG][{}][{}]\n", url, width));
                    }
                    Err(_) => c.text.push_str("\n[IMG_MISSING]\n"),
                }
            }
            _ => c.text.push_str("\n[IMG_MISSING]\n"),
        },
        "a" => {
            match n.attribute("href") {
                // TODO open external urls in browser
                Some(url) if !url.starts_with("http") => {
                    let start = c.text.len();
                    c.render(
                        n,
                        Attribute::Underlined,
                        Attribute::NoUnderline,
                        ea,
                        chapterpath,
                    );
                    c.links.push((start, c.text.len(), url.to_string()));
                }
                _ => c.render_text(n, ea, chapterpath),
            }
        }
        "em" => c.render(n, Attribute::Italic, Attribute::NoItalic, ea, chapterpath),
        "strong" => c.render(
            n,
            Attribute::Bold,
            Attribute::NormalIntensity,
            ea,
            chapterpath,
        ),
        tag @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
            c.text.push_str("\n ");
            let start = c.text.len();
            c.render(
                n,
                Attribute::Bold,
                Attribute::NormalIntensity,
                ea,
                chapterpath,
            );
            let end = c.text.len();
            c.text.push_str("\n\n");
            let level = match tag {
                "h1" => 0,
                "h2" => 1,
                "h3" => 2,
                "h4" => 3,
                "h5" => 4,
                _ => 5,
            };
            c.heading_spans.push((start, end, level));
        }
        "blockquote" | "div" | "p" | "tr" => {
            // TODO compress newlines
            c.text.push('\n');
            c.render_text(n, ea, chapterpath);
            c.text.push('\n');
        }
        "li" => {
            c.text.push_str("\n ");
            c.render_text(n, ea, chapterpath);
            c.text.push('\n');
        }
        "pre" => {
            c.text.push_str("\n  ");
            n.descendants()
                .filter(Node::is_text)
                .map(|n| n.text().unwrap().replace('\n', "\n  "))
                .for_each(|s| c.text.push_str(&s));
            c.text.push('\n');
        }
        _ => c.render_text(n, ea, chapterpath),
    }
}

fn epub2(doc: Document, tree: &mut Vec<TocEntry>, flat: &mut HashMap<String, String>) {
    fn parse_navpoint(n: Node, entries: &mut Vec<TocEntry>, flat: &mut HashMap<String, String>) {
        let path = n
            .descendants()
            .find(|child| child.has_tag_name("content"))
            .unwrap()
            .attribute("src")
            .unwrap()
            .split('#')
            .next()
            .unwrap()
            .to_string();
        let title = n
            .descendants()
            .find(|child| child.has_tag_name("text"))
            .unwrap()
            .text()
            .unwrap_or("")
            .to_string();
        let mut children = Vec::new();
        for child in n.children().filter(|c| c.has_tag_name("navPoint")) {
            parse_navpoint(child, &mut children, flat);
        }
        flat.entry(path.clone()).or_insert(title.clone());
        entries.push(TocEntry { title, path, children });
    }
    let navmap = doc
        .descendants()
        .find(|n| n.has_tag_name("navMap"))
        .unwrap();
    for n in navmap.children().filter(|n| n.has_tag_name("navPoint")) {
        parse_navpoint(n, tree, flat);
    }
}
fn epub3(doc: Document, tree: &mut Vec<TocEntry>, flat: &mut HashMap<String, String>) {
    fn parse_ol(ol: Node, entries: &mut Vec<TocEntry>, flat: &mut HashMap<String, String>) {
        for li in ol.children().filter(|n| n.has_tag_name("li")) {
            let a = match li.children().find(|n| n.has_tag_name("a")) {
                Some(a) => a,
                None => continue,
            };
            let path = a
                .attribute("href")
                .unwrap()
                .split('#')
                .next()
                .unwrap()
                .to_string();
            let title: String = a
                .descendants()
                .filter(Node::is_text)
                .map(|n| n.text().unwrap())
                .collect();
            let mut children = Vec::new();
            if let Some(child_ol) = li.children().find(|n| n.has_tag_name("ol")) {
                parse_ol(child_ol, &mut children, flat);
            }
            flat.entry(path.clone()).or_insert(title.clone());
            entries.push(TocEntry { title, path, children });
        }
    }
    let nav = doc
        .descendants()
        .find(|n| n.has_tag_name("nav"))
        .unwrap();
    let ol = nav
        .children()
        .find(|n| n.has_tag_name("ol"))
        .unwrap();
    parse_ol(ol, tree, flat);
}

/// Decode bytes to String, trying UTF-8 first, then falling back to
/// Windows-1252 (common for older EPUBs with smart quotes as 0x93/0x94),
/// then fixing double-encoded UTF-8 mojibake.
fn decode_text(bytes: &[u8]) -> String {
    // Try UTF-8 first (most EPUBs are UTF-8)
    if let Ok(s) = std::str::from_utf8(bytes) {
        let s = s.to_string();
        // Check for double-encoded UTF-8 and fix it
        return fix_double_encoded(&s);
    }

    // Not valid UTF-8 — try to detect encoding from XML/HTML declaration
    let head = std::str::from_utf8(&bytes[..bytes.len().min(512)]).unwrap_or("");
    let encoding = detect_encoding(head);

    match encoding {
        Some("windows-1252") | Some("iso-8859-1") | Some("latin1") => {
            decode_windows1252(bytes)
        }
        _ => {
            // Fallback: treat as Windows-1252
            decode_windows1252(bytes)
        }
    }
}

/// Detect encoding from XML declaration or HTML meta tag.
fn detect_encoding(head: &str) -> Option<&str> {
    if let Some(pos) = head.find("encoding=") {
        let rest = &head[pos + 9..];
        let delim = rest.chars().next()?;
        if delim == '"' || delim == '\'' {
            let end = rest[1..].find(delim)?;
            return Some(&rest[1..1 + end]);
        }
    }
    if let Some(pos) = head.to_lowercase().find("charset=") {
        let rest = &head[pos + 8..];
        let delim = rest.chars().next()?;
        if delim == '"' || delim == '\'' {
            let end = rest[1..].find(delim)?;
            return Some(&rest[1..1 + end]);
        }
        let end = rest.find(|c: char| c.is_whitespace() || c == '>')?;
        return Some(&rest[..end]);
    }
    None
}

/// Fix double-encoded UTF-8: e.g. "â" (C3 A2 C2 80 C2 9C) → " (E2 80 9C).
/// This happens when UTF-8 bytes are misinterpreted as Latin-1 and re-encoded.
fn fix_double_encoded(text: &str) -> String {
    // Common double-encoded patterns: the first byte of a multi-byte UTF-8
    // char (0xC2-0xF4) followed by continuation bytes (0x80-0xBF) that were
    // also individually encoded as UTF-8.
    // We look for C3 A2 (â) followed by C2 80-C2 BF sequences.
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Check for â (C3 A2) which might be the start of a double-encoded sequence
        if i + 1 < bytes.len() && bytes[i] == 0xC3 && bytes[i + 1] == 0xA2 {
            // Collect following C2 xx sequences
            let mut j = i + 2;
            let mut parts: Vec<u8> = vec![0xE2]; // â came from 0xE2
            while j + 1 < bytes.len() && bytes[j] == 0xC2 && (0x80..=0xBF).contains(&bytes[j + 1]) {
                parts.push(bytes[j + 1]);
                j += 2;
            }
            if parts.len() >= 2 {
                // Reconstruct original UTF-8 sequence
                if let Ok(reconstructed) = std::str::from_utf8(&parts) {
                    let ch = reconstructed.chars().next().unwrap();
                    // Only fix if it's a printable/punctuation char (not a control char)
                    if ch as u32 >= 0x2000 || ch == '\u{201C}' || ch == '\u{201D}' || ch == '\u{2018}' || ch == '\u{2019}' || ch == '\u{2013}' || ch == '\u{2014}' {
                        out.extend_from_slice(reconstructed.as_bytes());
                        i = j;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// Decode Windows-1252 bytes to a UTF-8 String.
fn decode_windows1252(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| windows1252_to_char(b)).collect()
}

fn windows1252_to_char(byte: u8) -> char {
    match byte {
        0x80 => '\u{20AC}', 0x82 => '\u{201A}', 0x83 => '\u{0192}',
        0x84 => '\u{201E}', 0x85 => '\u{2026}', 0x86 => '\u{2020}',
        0x87 => '\u{2021}', 0x88 => '\u{02C6}', 0x89 => '\u{2030}',
        0x8A => '\u{0160}', 0x8B => '\u{2039}', 0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}', 0x92 => '\u{2019}', 0x93 => '\u{201C}',
        0x94 => '\u{201D}', 0x95 => '\u{2022}', 0x96 => '\u{2013}',
        0x97 => '\u{2014}', 0x98 => '\u{02DC}', 0x99 => '\u{2122}',
        0x9A => '\u{0161}', 0x9B => '\u{203A}', 0x9C => '\u{0153}',
        0x9E => '\u{017E}', 0x9F => '\u{0178}',
        _ => byte as char,
    }
}
