//! A small DOM on top of quick-xml for reading and re-writing .docx parts.
//!
//! Elements keep their qualified names (`w:p`) so that unknown content can be
//! serialized back verbatim; lookups use local names (`p`).

use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Default, Clone)]
pub struct El {
    /// Local name, e.g. `p`.
    pub name: String,
    /// Qualified name as written in the source, e.g. `w:p`.
    pub qname: String,
    /// Attributes as (qualified name, value).
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    El(El),
    Text(String),
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl El {
    pub fn new(qname: &str) -> Self {
        El { name: local(qname).to_string(), qname: qname.to_string(), ..Default::default() }
    }

    /// Attribute by local name (`val` matches `w:val`).
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| local(k) == key).map(|(_, v)| v.as_str())
    }

    /// Attribute by exact qualified name.
    pub fn qattr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, key: &str, val: &str) {
        if let Some(a) = self.attrs.iter_mut().find(|(k, _)| k == key) {
            a.1 = val.to_string();
        } else {
            self.attrs.push((key.to_string(), val.to_string()));
        }
    }

    pub fn elems(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| match n {
            Node::El(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn elems_mut(&mut self) -> impl Iterator<Item = &mut El> {
        self.children.iter_mut().filter_map(|n| match n {
            Node::El(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&El> {
        self.elems().find(|e| e.name == name)
    }

    /// First descendant (depth-first) with the given local name.
    pub fn find(&self, name: &str) -> Option<&El> {
        for e in self.elems() {
            if e.name == name {
                return Some(e);
            }
            if let Some(f) = e.find(name) {
                return Some(f);
            }
        }
        None
    }

    /// All descendants with the given local name.
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        for e in self.elems() {
            if e.name == name {
                out.push(e);
            }
            e.find_all(name, out);
        }
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        for n in &self.children {
            match n {
                Node::Text(t) => s.push_str(t),
                Node::El(e) => s.push_str(&e.text()),
            }
        }
        s
    }

    /// Text of all `t` / `delText` descendants (Word run text), ignoring markup whitespace.
    pub fn run_text(&self) -> String {
        let mut s = String::new();
        for e in self.elems() {
            match e.name.as_str() {
                "t" | "delText" | "instrText" => s.push_str(&e.text()),
                "tab" => s.push('\t'),
                "br" | "cr" => s.push(' '),
                _ => s.push_str(&e.run_text()),
            }
        }
        s
    }

    /// `w:val` attribute of a child element.
    pub fn child_val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.attr("val"))
    }

    /// Serializes this element (and its subtree) back to XML.
    pub fn to_xml(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }

    pub fn write(&self, s: &mut String) {
        s.push('<');
        s.push_str(&self.qname);
        for (k, v) in &self.attrs {
            s.push(' ');
            s.push_str(k);
            s.push_str("=\"");
            s.push_str(&esc(v));
            s.push('"');
        }
        if self.children.is_empty() {
            s.push_str("/>");
            return;
        }
        s.push('>');
        for c in &self.children {
            match c {
                Node::Text(t) => s.push_str(&esc_text(t)),
                Node::El(e) => e.write(s),
            }
        }
        s.push_str("</");
        s.push_str(&self.qname);
        s.push('>');
    }

}

#[allow(deprecated)]
fn element(e: &quick_xml::events::BytesStart) -> Result<El, String> {
    let qname = e.name().as_ref().to_string();
    let mut el = El::new(&qname);
    for a in e.attributes().with_checks(false) {
        let a = a.map_err(|e| e.to_string())?;
        let v = a.unescape_value().map_err(|e| e.to_string())?.into_owned();
        el.attrs.push((a.key.as_ref().to_string(), v));
    }
    Ok(el)
}

fn push_text(stack: &mut [El], t: &str) {
    if let Some(top) = stack.last_mut() {
        if let Some(Node::Text(prev)) = top.children.last_mut() {
            prev.push_str(t);
        } else {
            top.children.push(Node::Text(t.to_string()));
        }
    }
}

/// Parses a document and returns its root element.
pub fn parse(src: &str) -> Result<El, String> {
    let root = parse_fragment(src)?;
    root.children
        .into_iter()
        .find_map(|n| match n {
            Node::El(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| "empty XML document".into())
}

/// Parses XML that may have several top-level elements; returns a synthetic root.
pub fn parse_fragment(src: &str) -> Result<El, String> {
    let mut reader = Reader::from_str(src);
    let mut stack: Vec<El> = vec![El::new("#root")];
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => stack.push(element(&e)?),
            Event::Empty(e) => {
                let el = element(&e)?;
                stack.last_mut().unwrap().children.push(Node::El(el));
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(Node::El(el));
                }
            }
            Event::Text(t) => push_text(&mut stack, &t.xml10_content()),
            Event::CData(t) => push_text(&mut stack, &t.xml10_content()),
            Event::GeneralRef(r) => {
                let s: String = if r.is_char_ref() {
                    r.resolve_char_ref().ok().flatten().map(|c| c.to_string()).unwrap_or_default()
                } else {
                    match r.as_ref() {
                        "lt" => "<",
                        "gt" => ">",
                        "amp" => "&",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    }
                    .to_string()
                };
                push_text(&mut stack, &s);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    while stack.len() > 1 {
        let el = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(Node::El(el));
    }
    Ok(stack.pop().unwrap())
}

/// Parses a sequence of sibling elements (e.g. stored raw `rPr` children).
pub fn parse_elems(src: &str) -> Vec<El> {
    if src.is_empty() {
        return Vec::new();
    }
    parse_fragment(src)
        .map(|r| {
            r.children
                .into_iter()
                .filter_map(|n| match n {
                    Node::El(e) => Some(e),
                    Node::Text(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Escapes text for XML attribute values.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => o.push(c),
        }
    }
    o
}

/// Escapes text content.
pub fn esc_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '&' => o.push_str("&amp;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => o.push(c),
        }
    }
    o
}
