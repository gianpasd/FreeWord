//! Process-wide interned strings and opaque objects.
//!
//! `CharFmt` must stay `Copy`, so formatting data that is really a string
//! (unknown `w:rPr` children, hyperlink targets) is interned here and referred
//! to by id. Opaque inline objects (fields, bookmarks, footnote references,
//! equations, shapes, ...) are stored here as well. Id 0 always means "none".

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct Strings {
    list: Vec<String>,
    map: HashMap<String, u32>,
}

static STRINGS: OnceLock<Mutex<Strings>> = OnceLock::new();

fn strings() -> &'static Mutex<Strings> {
    STRINGS.get_or_init(|| Mutex::new(Strings { list: vec![String::new()], map: HashMap::new() }))
}

pub fn intern(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let mut st = strings().lock().unwrap();
    if let Some(id) = st.map.get(s) {
        return *id;
    }
    let id = st.list.len() as u32;
    st.list.push(s.to_string());
    st.map.insert(s.to_string(), id);
    id
}

pub fn get(id: u32) -> String {
    if id == 0 {
        return String::new();
    }
    strings().lock().unwrap().list.get(id as usize).cloned().unwrap_or_default()
}

/// How an opaque inline object is displayed.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ObjKind {
    /// Zero width, invisible (bookmarks, comment anchors, deleted revisions...).
    Hidden,
    /// Shown as text (equations, symbols...).
    Label(String),
    /// Shown as a framed box of the given size in points (charts, shapes...).
    Box {
        label: String,
        w: f32,
        h: f32,
        #[serde(default)]
        anchor: Option<crate::doc::Anchor>,
    },
    FieldBegin,
    FieldSep,
    FieldEnd,
    /// Footnote/endnote reference; the number is computed during layout.
    NoteRef { endnote: bool, id: String },
    /// Start/end tag of a paragraph-level wrapper such as a tracked insertion.
    Wrap { open: bool },
    /// Tracked deletion (`w:del`/`w:moveFrom`) with its deleted text.
    Deleted(String),
}

#[derive(Clone, Debug)]
pub struct RawObj {
    /// XML written back verbatim (a run or other paragraph child).
    pub xml: String,
    pub kind: ObjKind,
    /// Package the XML came from (relationship ids are only valid there).
    pub pkg: u64,
}

static OBJECTS: OnceLock<Mutex<Vec<RawObj>>> = OnceLock::new();

fn objects() -> &'static Mutex<Vec<RawObj>> {
    OBJECTS.get_or_init(|| {
        Mutex::new(vec![RawObj { xml: String::new(), kind: ObjKind::Hidden, pkg: 0 }])
    })
}

pub fn add_obj(o: RawObj) -> u32 {
    let mut v = objects().lock().unwrap();
    v.push(o);
    (v.len() - 1) as u32
}

pub fn obj(id: u32) -> Option<RawObj> {
    if id == 0 {
        return None;
    }
    objects().lock().unwrap().get(id as usize).cloned()
}

/// Whether raw XML refers to package relationships (r:id, r:embed, ...).
pub fn has_rel_refs(xml: &str) -> bool {
    xml.contains(" r:id=") || xml.contains(" r:embed=") || xml.contains(" r:link=") || xml.contains(" r:dm=")
        || xml.contains(" r:pict=") || xml.contains(" r:href=")
}

/// A hyperlink target: external URL and/or internal bookmark anchor.
pub fn link_id(url: &str, anchor: &str) -> u32 {
    if url.is_empty() && anchor.is_empty() {
        return 0;
    }
    intern(&format!("{url}\u{1}{anchor}"))
}

pub fn link(id: u32) -> (String, String) {
    let s = get(id);
    match s.split_once('\u{1}') {
        Some((u, a)) => (u.to_string(), a.to_string()),
        None => (s, String::new()),
    }
}

static NEXT_PKG: OnceLock<Mutex<u64>> = OnceLock::new();

pub fn new_pkg_id() -> u64 {
    let m = NEXT_PKG.get_or_init(|| Mutex::new(1));
    let mut g = m.lock().unwrap();
    *g += 1;
    *g
}
