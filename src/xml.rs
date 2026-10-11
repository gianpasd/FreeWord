//! A tiny DOM on top of quick-xml, enough for reading .docx parts.
//! Element and attribute names are stored without namespace prefixes.

use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Default)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug)]
pub enum Node {
    El(El),
    Text(String),
}

impl El {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn elems(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| match n {
            Node::El(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&El> {
        self.elems().find(|e| e.name == name)
    }

    /// First descendant (depth-first) with the given name.
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

    /// `w:val` style attribute value of a child element.
    pub fn child_val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.attr("val"))
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_string()
}

#[allow(deprecated)]
fn element(e: &quick_xml::events::BytesStart) -> Result<El, String> {
    let mut el = El { name: local(e.name().as_ref()), ..Default::default() };
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let v = a.unescape_value().map_err(|e| e.to_string())?.into_owned();
        el.attrs.push((local(a.key.as_ref()), v));
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

pub fn parse(src: &str) -> Result<El, String> {
    let mut reader = Reader::from_str(src);
    let mut stack: Vec<El> = vec![El { name: "#root".into(), ..Default::default() }];
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
            Event::CData(t) => push_text(&mut stack, &String::from_utf8_lossy(t.as_ref().as_bytes())),
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
    let mut root = stack.pop().unwrap();
    // Return the document element.
    let idx = root.children.iter().position(|n| matches!(n, Node::El(_)));
    match idx {
        Some(i) => match root.children.swap_remove(i) {
            Node::El(e) => Ok(e),
            Node::Text(_) => unreachable!(),
        },
        None => Err("empty XML document".into()),
    }
}

/// Escapes text for XML content and attribute values.
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
            c => o.push(c),
        }
    }
    o
}
