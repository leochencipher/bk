use crossterm::style::{Attribute, Attributes};
use roxmltree::{Document, Node, ParsingOptions};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read},
};

pub struct Chapter {
    pub title: String,
    // single string for search
    pub text: String,
    pub lines: Vec<(usize, usize)>,
    // crossterm gives us a bitset but doesn't let us diff it, so store the state transition
    pub attrs: Vec<(usize, Attribute, Attributes)>,
    pub links: Vec<(usize, usize, String)>,
    frag: Vec<(String, usize)>,
    state: Attributes,
}

pub struct Epub {
    container: zip::ZipArchive<File>,
    rootdir: String,
    pub chapters: Vec<Chapter>,
    pub links: HashMap<String, (usize, usize)>,
    pub meta: String,
    pub imgs: HashMap<String, Vec<u8>>,
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
        };
        let chapters = epub.get_spine();
        if !meta {
            epub.get_chapters(chapters);
        }
        Ok(epub)
    }
    fn get_text(&mut self, name: &str) -> String {
        let mut text = String::new();
        self.container
            .by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }
    fn get_chapters(&mut self, spine: Vec<(String, String)>) {
        for (title, path) in spine {
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
            let state = Attributes::default();
            let mut c = Chapter {
                title,
                text: String::new(),
                lines: Vec::new(),
                attrs: vec![(0, Attribute::Reset, state)],
                state,
                links: Vec::new(),
                frag: Vec::new(),
            };
            render(body, &mut c, self, chapterpath);
            if c.text.trim().is_empty() {
                continue;
            }
            let relative = path.rsplit('/').next().unwrap();
            self.links
                .insert(relative.to_string(), (self.chapters.len(), 0));
            for (id, pos) in c.frag.drain(..) {
                let url = format!("{}#{}", relative, id);
                self.links.insert(url, (self.chapters.len(), pos));
            }
            for link in c.links.iter_mut() {
                if link.2.starts_with('#') {
                    link.2.insert_str(0, relative);
                }
            }
            self.chapters.push(c);
        }
    }
    fn get_spine(&mut self) -> Vec<(String, String)> {
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
        if doc.root_element().attribute("version") == Some("3.0") {
            let path = manifest_node
                .children()
                .find(|n| n.attribute("properties") == Some("nav"))
                .unwrap()
                .attribute("href")
                .unwrap();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub3(doc, &mut nav);
        } else {
            let id = spine_node.attribute("toc").unwrap_or("ncx");
            let path = manifest.get(id).unwrap();
            let xml = self.get_text(&format!("{}{}", self.rootdir, path));
            let doc = Document::parse(&xml).unwrap();
            epub2(doc, &mut nav);
        }
        spine_node
            .children()
            .filter(Node::is_element)
            .enumerate()
            .map(|(i, n)| {
                let id = n.attribute("idref").unwrap();
                let path = manifest.remove(id).unwrap();
                let label = nav.remove(path).unwrap_or_else(|| i.to_string());
                (label, path.to_string())
            })
            .collect()
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
                        ea.imgs.insert(String::from(url), buffer);
                        c.text.push_str(&format!("\n[IMG][{}]\n", url));
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
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            c.text.push_str("\n ");
            c.render(
                n,
                Attribute::Bold,
                Attribute::NormalIntensity,
                ea,
                chapterpath,
            );
            c.text.push_str("\n--------------\n");
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

fn epub2(doc: Document, nav: &mut HashMap<String, String>) {
    doc.descendants()
        .find(|n| n.has_tag_name("navMap"))
        .unwrap()
        .descendants()
        .filter(|n| n.has_tag_name("navPoint"))
        .for_each(|n| {
            let path = n
                .descendants()
                .find(|n| n.has_tag_name("content"))
                .unwrap()
                .attribute("src")
                .unwrap()
                .split('#')
                .next()
                .unwrap()
                .to_string();
            let text = n
                .descendants()
                .find(|n| n.has_tag_name("text"))
                .unwrap()
                .text()
                .unwrap_or("")
                .to_string();
            // TODO subsections
            nav.entry(path).or_insert(text);
        });
}
fn epub3(doc: Document, nav: &mut HashMap<String, String>) {
    doc.descendants()
        .find(|n| n.has_tag_name("nav"))
        .unwrap()
        .children()
        .find(|n| n.has_tag_name("ol"))
        .unwrap()
        .descendants()
        .filter(|n| n.has_tag_name("a"))
        .for_each(|n| {
            let path = n
                .attribute("href")
                .unwrap()
                .split('#')
                .next()
                .unwrap()
                .to_string();
            let text = n
                .descendants()
                .filter(Node::is_text)
                .map(|n| n.text().unwrap())
                .collect();
            nav.insert(path, text);
        });
}
