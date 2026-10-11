//! Document model and the primitive editing operations on it.

use crate::fonts;
use std::collections::BTreeMap;
use std::sync::Arc;

pub type Rgba = [u8; 4];

pub const BLACK: Rgba = [0, 0, 0, 255];
pub const NO_COLOR: Rgba = [0, 0, 0, 0];

/// Object replacement character used for inline pictures.
pub const OBJ: char = '\u{FFFC}';
/// Manual page break (Ctrl+Enter).
pub const PAGE_BREAK: char = '\u{000C}';
/// Manual line break (Shift+Enter).
pub const LINE_BREAK: char = '\u{000B}';

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CharFmt {
    pub font: u16,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// 0 = normal, 1 = superscript, -1 = subscript
    pub script: i8,
    pub color: Rgba,
    /// Alpha 0 means no highlight.
    pub highlight: Rgba,
    /// Image id for `OBJ` characters, 0 otherwise.
    pub image: u32,
}

impl CharFmt {
    pub fn body() -> Self {
        CharFmt {
            font: fonts::font_id(fonts::default_font()),
            size: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            script: 0,
            color: BLACK,
            highlight: NO_COLOR,
            image: 0,
        }
    }

    pub fn without_object(mut self) -> Self {
        self.image = 0;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Style {
    Normal,
    Title,
    Subtitle,
    Heading1,
    Heading2,
    Heading3,
    Quote,
    Code,
}

impl Style {
    pub const ALL: [Style; 8] = [
        Style::Normal,
        Style::Heading1,
        Style::Heading2,
        Style::Heading3,
        Style::Title,
        Style::Subtitle,
        Style::Quote,
        Style::Code,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Style::Normal => "Normal",
            Style::Title => "Title",
            Style::Subtitle => "Subtitle",
            Style::Heading1 => "Heading 1",
            Style::Heading2 => "Heading 2",
            Style::Heading3 => "Heading 3",
            Style::Quote => "Quote",
            Style::Code => "Code",
        }
    }

    /// Word style id used in .docx files.
    pub fn docx_id(self) -> &'static str {
        match self {
            Style::Normal => "Normal",
            Style::Title => "Title",
            Style::Subtitle => "Subtitle",
            Style::Heading1 => "Heading1",
            Style::Heading2 => "Heading2",
            Style::Heading3 => "Heading3",
            Style::Quote => "Quote",
            Style::Code => "Code",
        }
    }

    pub fn from_docx(id: &str, name: Option<&str>) -> Style {
        let key = name.unwrap_or(id).to_ascii_lowercase().replace(' ', "");
        match key.as_str() {
            "title" => Style::Title,
            "subtitle" => Style::Subtitle,
            "heading1" => Style::Heading1,
            "heading2" => Style::Heading2,
            "heading3" | "heading4" | "heading5" | "heading6" => Style::Heading3,
            "quote" | "intensequote" => Style::Quote,
            "code" | "htmlpreformatted" | "plaintext" => Style::Code,
            _ => Style::Normal,
        }
    }

    pub fn char_fmt(self) -> CharFmt {
        let mut f = CharFmt::body();
        let blue = [0x2F, 0x54, 0x96, 255];
        match self {
            Style::Normal => {}
            Style::Title => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 28.0;
            }
            Style::Subtitle => {
                f.size = 15.0;
                f.color = [0x59, 0x59, 0x59, 255];
            }
            Style::Heading1 => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 16.0;
                f.color = blue;
            }
            Style::Heading2 => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 13.0;
                f.color = blue;
            }
            Style::Heading3 => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 12.0;
                f.color = [0x1F, 0x37, 0x63, 255];
            }
            Style::Quote => {
                f.italic = true;
                f.color = [0x40, 0x40, 0x40, 255];
            }
            Style::Code => {
                f.font = fonts::font_id(fonts::mono_font());
                f.size = 10.0;
            }
        }
        f
    }

    /// (space before, space after) in points.
    pub fn spacing(self) -> (f32, f32) {
        match self {
            Style::Normal => (0.0, 8.0),
            Style::Title => (0.0, 4.0),
            Style::Subtitle => (0.0, 8.0),
            Style::Heading1 => (12.0, 4.0),
            Style::Heading2 => (8.0, 4.0),
            Style::Heading3 => (6.0, 2.0),
            Style::Quote => (10.0, 8.0),
            Style::Code => (0.0, 0.0),
        }
    }

    pub fn line_spacing(self) -> f32 {
        match self {
            Style::Title | Style::Code => 1.0,
            _ => 1.15,
        }
    }

    /// Styles after which Enter starts a Normal paragraph.
    pub fn next_is_normal(self) -> bool {
        matches!(
            self,
            Style::Title | Style::Subtitle | Style::Heading1 | Style::Heading2 | Style::Heading3
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ListKind {
    None,
    Bullet,
    Number,
}

#[derive(Clone, Debug)]
pub struct Para {
    pub chars: Vec<char>,
    pub fmts: Vec<CharFmt>,
    pub style: Style,
    pub align: Align,
    pub list: ListKind,
    pub level: u8,
    pub line_spacing: f32,
    pub space_before: f32,
    pub space_after: f32,
    /// Format of the paragraph mark (used when the paragraph is empty).
    pub end_fmt: CharFmt,
}

impl Para {
    pub fn new(style: Style) -> Self {
        let (b, a) = style.spacing();
        Para {
            chars: Vec::new(),
            fmts: Vec::new(),
            style,
            align: Align::Left,
            list: ListKind::None,
            level: 0,
            line_spacing: style.line_spacing(),
            space_before: b,
            space_after: a,
            end_fmt: style.char_fmt(),
        }
    }

    pub fn with_text(text: &str, style: Style) -> Self {
        let mut p = Para::new(style);
        let f = style.char_fmt();
        p.chars = text.chars().collect();
        p.fmts = vec![f; p.chars.len()];
        p
    }

    /// Copy of the paragraph properties without text.
    pub fn empty_like(&self) -> Self {
        Para { chars: Vec::new(), fmts: Vec::new(), ..self.clone() }
    }

    pub fn copy_props_from(&mut self, o: &Para) {
        self.style = o.style;
        self.align = o.align;
        self.list = o.list;
        self.level = o.level;
        self.line_spacing = o.line_spacing;
        self.space_before = o.space_before;
        self.space_after = o.space_after;
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Format that newly typed text at offset `o` should get.
    pub fn fmt_at(&self, o: usize) -> CharFmt {
        let f = if o > 0 && o <= self.fmts.len() {
            self.fmts[o - 1]
        } else if !self.fmts.is_empty() {
            self.fmts[0]
        } else {
            self.end_fmt
        };
        f.without_object()
    }

    pub fn set_style(&mut self, s: Style) {
        self.style = s;
        let (b, a) = s.spacing();
        self.space_before = b;
        self.space_after = a;
        self.line_spacing = s.line_spacing();
        let base = s.char_fmt();
        for f in &mut self.fmts {
            let img = f.image;
            *f = base;
            f.image = img;
        }
        self.end_fmt = base;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub p: usize,
    pub o: usize,
}

impl Pos {
    pub fn new(p: usize, o: usize) -> Self {
        Pos { p, o }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PageSetup {
    /// Points.
    pub width: f32,
    pub height: f32,
    /// top, right, bottom, left in points.
    pub margins: [f32; 4],
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup { width: 612.0, height: 792.0, margins: [72.0; 4] }
    }
}

#[derive(Clone, Debug)]
pub struct ImageData {
    pub bytes: Arc<Vec<u8>>,
    pub px_w: u32,
    pub px_h: u32,
    /// Display size in points.
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub paras: Vec<Para>,
    pub page: PageSetup,
    pub header: String,
    pub footer: String,
    pub page_numbers: bool,
    pub images: BTreeMap<u32, ImageData>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> CharClass {
    if c.is_whitespace() || c == PAGE_BREAK || c == LINE_BREAK {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' || c == '\'' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

impl Document {
    pub fn new() -> Self {
        Document {
            paras: vec![Para::new(Style::Normal)],
            page: PageSetup::default(),
            header: String::new(),
            footer: String::new(),
            page_numbers: false,
            images: BTreeMap::new(),
        }
    }

    pub fn next_image_id(&self) -> u32 {
        self.images.keys().next_back().copied().unwrap_or(0) + 1
    }

    pub fn end(&self) -> Pos {
        let p = self.paras.len() - 1;
        Pos::new(p, self.paras[p].len())
    }

    pub fn clamp(&self, pos: Pos) -> Pos {
        let p = pos.p.min(self.paras.len() - 1);
        Pos::new(p, pos.o.min(self.paras[p].len()))
    }

    pub fn prev(&self, pos: Pos) -> Pos {
        if pos.o > 0 {
            Pos::new(pos.p, pos.o - 1)
        } else if pos.p > 0 {
            Pos::new(pos.p - 1, self.paras[pos.p - 1].len())
        } else {
            pos
        }
    }

    pub fn next(&self, pos: Pos) -> Pos {
        if pos.o < self.paras[pos.p].len() {
            Pos::new(pos.p, pos.o + 1)
        } else if pos.p + 1 < self.paras.len() {
            Pos::new(pos.p + 1, 0)
        } else {
            pos
        }
    }

    pub fn word_left(&self, pos: Pos) -> Pos {
        if pos.o == 0 {
            return self.prev(pos);
        }
        let c = &self.paras[pos.p].chars;
        let mut o = pos.o;
        while o > 0 && class(c[o - 1]) == CharClass::Space {
            o -= 1;
        }
        if o > 0 {
            let k = class(c[o - 1]);
            while o > 0 && class(c[o - 1]) == k {
                o -= 1;
            }
        }
        Pos::new(pos.p, o)
    }

    pub fn word_right(&self, pos: Pos) -> Pos {
        let c = &self.paras[pos.p].chars;
        if pos.o >= c.len() {
            return self.next(pos);
        }
        let mut o = pos.o;
        let k = class(c[o]);
        if k != CharClass::Space {
            while o < c.len() && class(c[o]) == k {
                o += 1;
            }
        }
        while o < c.len() && class(c[o]) == CharClass::Space {
            o += 1;
        }
        Pos::new(pos.p, o)
    }

    /// Range of the word (or run of same-class characters) around `pos`.
    pub fn word_at(&self, pos: Pos) -> (Pos, Pos) {
        let c = &self.paras[pos.p].chars;
        if c.is_empty() {
            return (pos, pos);
        }
        let i = pos.o.min(c.len() - 1);
        let k = class(c[i]);
        let mut a = i;
        while a > 0 && class(c[a - 1]) == k {
            a -= 1;
        }
        let mut b = i;
        while b < c.len() && class(c[b]) == k {
            b += 1;
        }
        // Like Word, include trailing spaces after a word.
        if k == CharClass::Word {
            while b < c.len() && c[b] == ' ' {
                b += 1;
            }
        }
        (Pos::new(pos.p, a), Pos::new(pos.p, b))
    }

    /// Splits the paragraph at `pos`, returning the start of the new paragraph.
    pub fn split(&mut self, pos: Pos) -> Pos {
        let para = &mut self.paras[pos.p];
        let tail_chars = para.chars.split_off(pos.o);
        let tail_fmts = para.fmts.split_off(pos.o);
        let mut np = para.empty_like();
        np.chars = tail_chars;
        np.fmts = tail_fmts;
        if np.chars.is_empty() && para.style.next_is_normal() && !para.chars.is_empty() {
            np.set_style(Style::Normal);
        } else if np.chars.is_empty() {
            np.end_fmt = para.fmt_at(para.len());
        }
        if para.chars.is_empty() && !np.chars.is_empty() {
            para.end_fmt = np.fmts[0].without_object();
        }
        self.paras.insert(pos.p + 1, np);
        Pos::new(pos.p + 1, 0)
    }

    /// Inserts text (which may contain newlines) with a single format.
    pub fn insert_text(&mut self, pos: Pos, text: &str, fmt: CharFmt) -> Pos {
        let mut pos = pos;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                pos = self.split(pos);
            }
            let chars: Vec<char> = line.chars().filter(|c| *c == '\t' || !c.is_control() || *c == PAGE_BREAK || *c == LINE_BREAK).collect();
            let n = chars.len();
            let para = &mut self.paras[pos.p];
            para.chars.splice(pos.o..pos.o, chars);
            para.fmts.splice(pos.o..pos.o, std::iter::repeat_n(fmt, n));
            pos.o += n;
        }
        pos
    }

    pub fn delete(&mut self, a: Pos, b: Pos) {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            return;
        }
        if a.p == b.p {
            let para = &mut self.paras[a.p];
            para.chars.drain(a.o..b.o);
            para.fmts.drain(a.o..b.o);
            return;
        }
        let last = self.paras[b.p].clone();
        let first = &mut self.paras[a.p];
        first.chars.truncate(a.o);
        first.fmts.truncate(a.o);
        if first.chars.is_empty() && a.o == 0 && b.o == 0 {
            // Deleting whole paragraphs: keep the properties of the surviving one.
            first.copy_props_from(&last);
            first.end_fmt = last.end_fmt;
        }
        first.chars.extend_from_slice(&last.chars[b.o..]);
        first.fmts.extend_from_slice(&last.fmts[b.o..]);
        if b.o < last.len() || first.chars.is_empty() {
            first.end_fmt = last.end_fmt;
        }
        self.paras.drain(a.p + 1..=b.p);
    }

    /// Copies a range as a list of paragraphs (first and last may be partial).
    pub fn fragment(&self, a: Pos, b: Pos) -> Vec<Para> {
        let mut out = Vec::new();
        for p in a.p..=b.p {
            let para = &self.paras[p];
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            let mut np = para.empty_like();
            np.chars = para.chars[s..e].to_vec();
            np.fmts = para.fmts[s..e].to_vec();
            out.push(np);
        }
        out
    }

    pub fn insert_fragment(&mut self, pos: Pos, frag: &[Para]) -> Pos {
        if frag.is_empty() {
            return pos;
        }
        if frag.len() == 1 {
            let para = &mut self.paras[pos.p];
            let n = frag[0].len();
            para.chars.splice(pos.o..pos.o, frag[0].chars.iter().copied());
            para.fmts.splice(pos.o..pos.o, frag[0].fmts.iter().copied());
            return Pos::new(pos.p, pos.o + n);
        }
        let right = self.split(pos);
        {
            let left = &mut self.paras[pos.p];
            if left.chars.is_empty() {
                left.copy_props_from(&frag[0]);
                left.end_fmt = frag[0].end_fmt;
            }
            left.chars.extend_from_slice(&frag[0].chars);
            left.fmts.extend_from_slice(&frag[0].fmts);
        }
        let middle = &frag[1..frag.len() - 1];
        for (i, m) in middle.iter().enumerate() {
            self.paras.insert(right.p + i, m.clone());
        }
        let r = right.p + middle.len();
        let last = frag.last().unwrap();
        let rp = &mut self.paras[r];
        if rp.chars.is_empty() {
            rp.copy_props_from(last);
            rp.end_fmt = last.end_fmt;
        }
        rp.chars.splice(0..0, last.chars.iter().copied());
        rp.fmts.splice(0..0, last.fmts.iter().copied());
        Pos::new(r, last.len())
    }

    pub fn plain_text(&self, a: Pos, b: Pos) -> String {
        let mut s = String::new();
        for p in a.p..=b.p {
            let para = &self.paras[p];
            let st = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            for &c in &para.chars[st..e] {
                match c {
                    OBJ => {}
                    LINE_BREAK => s.push('\n'),
                    PAGE_BREAK => s.push('\n'),
                    c => s.push(c),
                }
            }
            if p != b.p {
                s.push('\n');
            }
        }
        s
    }

    /// Applies `f` to every character format in `[a, b)`.
    pub fn apply_fmt(&mut self, a: Pos, b: Pos, f: &dyn Fn(&mut CharFmt)) {
        for p in a.p..=b.p {
            let para = &mut self.paras[p];
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            for fm in &mut para.fmts[s..e] {
                f(fm);
            }
            if p < b.p || e == para.len() {
                f(&mut para.end_fmt);
                para.end_fmt.image = 0;
            }
        }
    }

    pub fn find_all(&self, query: &str, match_case: bool, whole_word: bool) -> Vec<(Pos, Pos)> {
        let fold = |c: char| if match_case { c } else { c.to_lowercase().next().unwrap_or(c) };
        let q: Vec<char> = query.chars().map(fold).collect();
        let mut out = Vec::new();
        if q.is_empty() {
            return out;
        }
        for (pi, para) in self.paras.iter().enumerate() {
            let c = &para.chars;
            if c.len() < q.len() {
                continue;
            }
            let mut i = 0;
            while i + q.len() <= c.len() {
                if (0..q.len()).all(|k| fold(c[i + k]) == q[k]) {
                    let ok = !whole_word
                        || ((i == 0 || class(c[i - 1]) != CharClass::Word)
                            && (i + q.len() == c.len() || class(c[i + q.len()]) != CharClass::Word));
                    if ok {
                        out.push((Pos::new(pi, i), Pos::new(pi, i + q.len())));
                        i += q.len();
                        continue;
                    }
                }
                i += 1;
            }
        }
        out
    }

    pub fn stats(&self) -> Stats {
        let mut st = Stats { paragraphs: 0, ..Default::default() };
        for p in &self.paras {
            let t: String = p.chars.iter().filter(|c| **c != OBJ).collect();
            if !t.trim().is_empty() {
                st.paragraphs += 1;
            }
            st.words += t.split_whitespace().count();
            st.chars += t.chars().filter(|c| !c.is_control()).count();
            st.chars_no_spaces += t.chars().filter(|c| !c.is_whitespace() && !c.is_control()).count();
        }
        st
    }

    /// Drops image data no longer referenced by any character.
    pub fn gc_images(&mut self) {
        let mut used = std::collections::BTreeSet::new();
        for p in &self.paras {
            for f in &p.fmts {
                if f.image != 0 {
                    used.insert(f.image);
                }
            }
        }
        self.images.retain(|k, _| used.contains(k));
    }
}

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub words: usize,
    pub chars: usize,
    pub chars_no_spaces: usize,
    pub paragraphs: usize,
}

/// Text for list markers: 1. a. i. depending on level.
pub fn number_label(n: u32, level: u8) -> String {
    match level % 3 {
        0 => format!("{n}."),
        1 => format!("{}.", alpha(n)),
        _ => format!("{}.", roman(n)),
    }
}

pub fn bullet_char(level: u8) -> char {
    ['•', '◦', '▪'][(level % 3) as usize]
}

fn alpha(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}

fn roman(mut n: u32) -> String {
    let table = [
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
        (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
    ];
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// Computes the list label for every paragraph (None for non-list paragraphs).
pub fn list_labels(paras: &[Para]) -> Vec<Option<String>> {
    let mut counters = [0u32; 10];
    let mut out = Vec::with_capacity(paras.len());
    for p in paras {
        let lvl = (p.level as usize).min(9);
        match p.list {
            ListKind::None => {
                counters = [0; 10];
                out.push(None);
            }
            ListKind::Bullet => {
                for c in counters.iter_mut().skip(lvl + 1) {
                    *c = 0;
                }
                out.push(Some(bullet_char(p.level).to_string()));
            }
            ListKind::Number => {
                for c in counters.iter_mut().skip(lvl + 1) {
                    *c = 0;
                }
                counters[lvl] += 1;
                out.push(Some(number_label(counters[lvl], p.level)));
            }
        }
    }
    out
}
