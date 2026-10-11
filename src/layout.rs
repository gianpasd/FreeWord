//! Line breaking, table layout and pagination.
//!
//! All coordinates produced here are in screen points relative to the top-left
//! corner of the page the line lives on.

use crate::doc::{
    self, Align, CharFmt, COLUMN_BREAK, Document, LINE_BREAK, Leader, LineRule, PAGE_BREAK, Pos,
    Rgba, TabKind, VMerge,
};
use crate::fonts;
use crate::pool::{self, ObjKind};
use eframe::egui::{Color32, FontId, epaint::text::FontsView};
use std::collections::HashMap;
use std::ops::Range;

pub const PT_TO_PX: f32 = 96.0 / 72.0;

/// Whether tracked changes are shown with markup (insertions coloured,
/// deletions struck through) rather than as the final text.
pub static SHOW_MARKUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn show_markup() -> bool {
    SHOW_MARKUP.load(std::sync::atomic::Ordering::Relaxed)
}
const SCRIPT_SCALE: f32 = 0.65;
const SMALL_CAPS_SCALE: f32 = 0.8;

#[derive(Clone)]
pub struct Line {
    pub para: usize,
    pub start: usize,
    pub end: usize,
    pub page: usize,
    pub y: f32,
    pub height: f32,
    /// Baseline offset from `y`.
    pub baseline: f32,
    /// Caret x positions for each boundary in `start..=end`.
    pub xs: Vec<f32>,
    pub last_in_para: bool,
    /// Line was ended by a manual line or page break character.
    pub forced: bool,
    /// Tab characters (paragraph offsets) drawn with a leader.
    pub leaders: Vec<(usize, Leader)>,
}

#[derive(Clone)]
pub struct ParaBox {
    pub lines: Range<usize>,
    /// Horizontal extent of the paragraph's text area.
    pub left: f32,
    pub right: f32,
    pub marker: Option<(String, f32)>,
}

/// A table cell's rectangle on a page.
#[derive(Clone)]
pub struct CellBox {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub shade: Option<Rgba>,
    /// top, right, bottom, left
    pub edges: [Option<doc::Border>; 4],
}

#[derive(Clone)]
pub struct Layout {
    pub lines: Vec<Line>,
    pub paras: Vec<ParaBox>,
    pub page_lines: Vec<Range<usize>>,
    pub cells: Vec<CellBox>,
    pub pages: usize,
    pub scale: f32,
    pub page_w: f32,
    pub page_h: f32,
    /// top, right, bottom, left in screen points.
    pub margins: [f32; 4],
    /// Display text for objects that show computed labels (footnote numbers...).
    pub obj_text: HashMap<(usize, usize), String>,
    /// Header text and its width.
    pub header: (String, f32),
    /// Footer text and width for each page.
    pub footers: Vec<(String, f32)>,
    /// Rich header/footer layouts per (section, variant).
    pub hf: Vec<HfBox>,
    /// Section index of each page (the section in effect at the top of the page).
    pub page_section: Vec<usize>,
    /// First page of each section.
    pub section_first_page: Vec<usize>,
    /// Whether rich (laid out) headers / footers are used instead of plain text.
    pub rich_header: bool,
    pub rich_footer: bool,
    /// PAGE / NUMPAGES field results: (paragraph, start, end, is NUMPAGES).
    pub page_fields: Vec<(usize, usize, usize, bool)>,
    /// Footnotes (bottom of their page) and endnotes (after the text).
    pub notes: Vec<NoteBox>,
    /// Note separator lines: (page, y).
    pub note_separators: Vec<(usize, f32)>,
    /// Floating pictures and shapes.
    pub floats: Vec<FloatBox>,
    /// Nested tables: (paragraph, page, x, y, layout).
    pub nested: Vec<(usize, usize, f32, f32, Box<Layout>)>,
    /// Tracked insertions: (paragraph, start, end).
    pub ins_ranges: Vec<(usize, usize, usize)>,
    /// Lines that contain tracked changes (for change bars).
    pub rev_lines: std::collections::HashSet<usize>,
    /// Commented ranges: (comment id, start, end).
    pub comment_ranges: Vec<(String, Pos, Pos)>,
}

/// Value of a `w:id="..."` attribute in raw XML.
fn xml_id(xml: &str) -> Option<String> {
    let i = xml.find(" w:id=\"")? + 7;
    let e = xml[i..].find('"')?;
    Some(xml[i..i + e].to_string())
}

/// Tracked insertions and lines with revisions.
fn find_revisions(doc: &Document) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (pi, p) in doc.paras.iter().enumerate() {
        let mut open: Vec<usize> = Vec::new();
        for (ci, f) in p.fmts.iter().enumerate() {
            let Some(o) = pool::obj(f.obj) else { continue };
            if let ObjKind::Wrap { open: is_open } = o.kind {
                if is_open {
                    if o.xml.starts_with("<w:ins") || o.xml.starts_with("<w:moveTo") {
                        open.push(ci + 1);
                    } else {
                        open.push(usize::MAX);
                    }
                } else if let Some(s) = open.pop() {
                    if s != usize::MAX && ci > s {
                        out.push((pi, s, ci));
                    }
                }
            }
        }
    }
    out
}

/// Commented ranges from commentRangeStart/End (or the reference alone).
fn find_comments(doc: &Document) -> Vec<(String, Pos, Pos)> {
    let mut starts: HashMap<String, Pos> = HashMap::new();
    let mut out: Vec<(String, Pos, Pos)> = Vec::new();
    for (pi, p) in doc.paras.iter().enumerate() {
        for (ci, f) in p.fmts.iter().enumerate() {
            let Some(o) = pool::obj(f.obj) else { continue };
            let Some(id) = xml_id(&o.xml) else { continue };
            if o.xml.contains("commentRangeStart") {
                starts.insert(id, Pos::new(pi, ci));
            } else if o.xml.contains("commentRangeEnd") {
                let s = starts.remove(&id).unwrap_or(Pos::new(pi, ci));
                out.push((id, s, Pos::new(pi, ci)));
            } else if o.xml.contains("commentReference") && !out.iter().any(|(i, _, _)| *i == id) {
                out.push((id, Pos::new(pi, ci), Pos::new(pi, ci)));
            }
        }
    }
    out
}

type BTreeMapPages = std::collections::BTreeMap<usize, Vec<((bool, String), Box<Layout>, f32)>>;

/// A laid-out header or footer. Variants: 0 header, 1 first-page header, 2 footer, 3 first-page footer.
#[derive(Clone)]
pub struct HfBox {
    pub section: usize,
    pub variant: usize,
    pub lay: Box<Layout>,
    pub dy: f32,
}

/// The document of a header/footer variant of a section.
pub fn hf_doc(doc: &Document, section: usize, variant: usize) -> Option<&Document> {
    let h = doc.hf.get(section)?;
    match variant {
        0 => h.header.as_deref(),
        1 => h.header_first.as_deref(),
        2 => h.footer.as_deref(),
        _ => h.footer_first.as_deref(),
    }
}

impl Layout {
    /// The rich header (or footer) to draw on `page`, with its document.
    pub fn hf_for<'a>(&'a self, doc: &'a Document, header: bool, page: usize) -> Option<(&'a HfBox, &'a Document)> {
        let section = *self.page_section.get(page)?;
        let first = self.section_first_page.get(section) == Some(&page) && doc.hf.get(section)?.title_page;
        let variant = match (header, first) {
            (true, false) => 0,
            (true, true) => 1,
            (false, false) => 2,
            (false, true) => 3,
        };
        let d = hf_doc(doc, section, variant)?;
        let b = self.hf.iter().find(|b| b.section == section && b.variant == variant)?;
        Some((b, d))
    }

    /// Section and variant of the header (or footer) shown on `page`, whether or not it has content.
    pub fn hf_slot(&self, doc: &Document, header: bool, page: usize) -> (usize, usize) {
        let section = self.page_section.get(page).copied().unwrap_or(0);
        let first = self.section_first_page.get(section) == Some(&page) && doc.hf.get(section).is_some_and(|h| h.title_page);
        (section, if header { 0 } else { 2 } + first as usize)
    }

    /// Layout of a header/footer placed on `page` of this (body) layout, for editing it in place:
    /// its lines are moved onto `page` and every other page is empty.
    pub fn story_layout(&self, section: usize, variant: usize, page: usize) -> Option<Layout> {
        let b = self.hf.iter().find(|b| b.section == section && b.variant == variant)?;
        let mut l = (*b.lay).clone();
        let dy = b.dy;
        for line in &mut l.lines {
            line.page = page;
            line.y += dy;
        }
        for c in &mut l.cells {
            c.page = page;
            c.y += dy;
        }
        for f in &mut l.floats {
            f.page = page;
            f.y += dy;
        }
        for n in &mut l.nested {
            n.1 = page;
            n.3 += dy;
        }
        l.pages = self.pages;
        l.page_w = self.page_w;
        l.page_h = self.page_h;
        l.page_lines = vec![0..0; self.pages];
        if let Some(r) = l.page_lines.get_mut(page) {
            *r = 0..l.lines.len();
        }
        l.notes.clear();
        l.note_separators.clear();
        l.hf.clear();
        Some(l)
    }
}

/// Finds the results of PAGE and NUMPAGES fields.
fn find_page_fields(doc: &Document) -> Vec<(usize, usize, usize, bool)> {
    let mut out = Vec::new();
    for (pi, p) in doc.paras.iter().enumerate() {
        let mut instr = String::new();
        let mut result_start: Option<(usize, bool)> = None;
        let mut in_instr = false;
        for (ci, f) in p.fmts.iter().enumerate() {
            let Some(o) = pool::obj(f.obj) else { continue };
            match o.kind {
                ObjKind::FieldBegin => {
                    instr.clear();
                    in_instr = true;
                    result_start = None;
                }
                ObjKind::Hidden if in_instr && o.xml.contains("instrText") => {
                    if let Ok(el) = crate::xml::parse(&o.xml) {
                        instr.push_str(&el.run_text());
                    }
                }
                ObjKind::FieldSep => {
                    in_instr = false;
                    let first = instr.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                    if first == "PAGE" || first == "NUMPAGES" || first == "SECTIONPAGES" {
                        result_start = Some((ci + 1, first != "PAGE"));
                    }
                }
                ObjKind::FieldEnd => {
                    if let Some((s, total)) = result_start.take() {
                        if ci > s {
                            out.push((pi, s, ci, total));
                        }
                    }
                    in_instr = false;
                }
                _ => {}
            }
        }
    }
    out
}

/// Lays out a header/footer sub-document across the page width.
fn hf_layout(main: &Document, sub: &Document, fonts: &mut FontsView<'_>, zoom: f32, top: f32) -> Box<Layout> {
    let mut d = sub.clone();
    d.page = main.page.clone();
    d.page.height = 20000.0;
    d.page.margins = [top, main.page.margins[1], 0.0, main.page.margins[3]];
    d.header.clear();
    d.footer.clear();
    d.page_numbers = false;
    d.hf = Default::default();
    d.notes = Default::default();
    Box::new(layout(&d, fonts, zoom))
}

/// Height of the content of a header/footer layout below its top margin.
fn content_height(l: &Layout) -> f32 {
    let lines = l.lines.iter().map(|x| x.y + x.height).fold(0.0f32, f32::max);
    let cells = l.cells.iter().map(|c| c.y + c.h).fold(0.0f32, f32::max);
    (lines.max(cells) - l.margins[0]).max(0.0)
}

/// Format used for header and footer text.
pub fn hf_fmt() -> CharFmt {
    CharFmt { size: 10.0, color: [0x59, 0x59, 0x59, 255], ..CharFmt::body() }
}

/// Font used to draw a character with format `f` at `scale` points-per-pt.
pub fn font_for(f: &CharFmt, scale: f32) -> (FontId, fonts::Resolved) {
    let r = fonts::resolve(f.font, f.bold, f.italic);
    let mut size = f.size * scale;
    if f.script != 0 {
        size *= SCRIPT_SCALE;
    }
    (FontId::new(size.max(1.0), r.family.clone()), r)
}

/// Font for a character, taking small caps into account.
pub fn font_for_char(f: &CharFmt, c: char, scale: f32) -> FontId {
    let (mut id, _) = font_for(f, scale);
    if f.small_caps && !f.caps && c.is_lowercase() {
        id.size *= SMALL_CAPS_SCALE;
    }
    id
}

/// Character as displayed (caps / small caps).
pub fn display_char(f: &CharFmt, c: char) -> char {
    if (f.caps || f.small_caps) && c.is_lowercase() {
        c.to_uppercase().next().unwrap_or(c)
    } else {
        c
    }
}

/// Vertical baseline shift for super/subscript (negative is up).
pub fn script_shift(f: &CharFmt, scale: f32) -> f32 {
    match f.script {
        1 => -f.size * scale * 0.33,
        -1 => f.size * scale * 0.15,
        _ => 0.0,
    }
}

type MetricCache = HashMap<(String, u32), (f32, f32)>;

/// (ascent, row height) for a font.
fn metrics(fonts: &mut FontsView<'_>, cache: &mut MetricCache, id: &FontId) -> (f32, f32) {
    let key = (format!("{:?}", id.family), id.size.to_bits());
    if let Some(m) = cache.get(&key) {
        return *m;
    }
    let g = fonts.layout_no_wrap("Hg".into(), id.clone(), Color32::BLACK);
    let h = g.size().y;
    let asc = g
        .rows
        .first()
        .and_then(|r| r.row.glyphs.first().map(|gl| r.pos.y + gl.pos.y))
        .unwrap_or(h * 0.8);
    cache.insert(key, (asc, h));
    (asc, h)
}

/// Text shown for an opaque object, if it takes space.
pub fn obj_label(lay_text: Option<&String>, kind: &ObjKind) -> Option<String> {
    match kind {
        ObjKind::Label(s) => Some(s.clone()),
        ObjKind::NoteRef { .. } => lay_text.cloned().or(Some("*".into())),
        _ => None,
    }
}

struct Ctx<'a, 'b> {
    fonts: &'a mut FontsView<'b>,
    cache: MetricCache,
    sc: f32,
    default_tab: f32,
    obj_text: HashMap<(usize, usize), String>,
    footnotes: u32,
    endnotes: u32,
}

struct ParaLines {
    lines: Vec<Line>,
    marker: Option<(String, f32)>,
    left: f32,
    right: f32,
}

impl Ctx<'_, '_> {
    fn extent(&mut self, f: &CharFmt) -> (f32, f32) {
        let (id, _) = font_for(f, self.sc);
        let (a, h) = metrics(self.fonts, &mut self.cache, &id);
        let shift = script_shift(f, self.sc);
        ((a - shift).max(0.0), (h - a + shift).max(0.0))
    }

    fn text_width(&mut self, f: &CharFmt, s: &str) -> f32 {
        let mut w = 0.0;
        for c in s.chars() {
            let id = font_for_char(f, c, self.sc);
            w += self.fonts.glyph_width(&id, display_char(f, c)) + f.char_spacing * self.sc;
        }
        w
    }

    /// Lays out paragraph `pi` between `left_edge` and `right_edge` (page-relative);
    /// line y values are relative to the paragraph top.
    fn para(&mut self, doc: &Document, pi: usize, label: Option<&String>, left_edge: f32, right_edge: f32) -> ParaLines {
        let sc = self.sc;
        let para = &doc.paras[pi];
        let n = para.len();
        let left = left_edge + para.ind_left * sc;
        let right = (right_edge - para.ind_right * sc).max(left + 20.0);
        let first_x = left + para.ind_first * sc;

        // Per-character metrics.
        let mut widths = vec![0.0f32; n];
        let mut asc = vec![0.0f32; n];
        let mut desc = vec![0.0f32; n];
        let mut is_img = vec![false; n];
        let mut cur: Option<(CharFmt, f32, f32)> = None;
        for i in 0..n {
            let f = para.fmts[i];
            let c = para.chars[i];
            if f.hidden {
                continue;
            }
            if f.image != 0 {
                if let Some(img) = doc.images.get(&f.image) {
                    if img.anchor.is_some() {
                        continue;
                    }
                    widths[i] = img.w * sc;
                    asc[i] = img.h * sc;
                    is_img[i] = true;
                }
                continue;
            }
            if f.obj != 0 {
                if let Some(o) = pool::obj(f.obj) {
                    match &o.kind {
                        ObjKind::Box { anchor: Some(_), .. } => {}
                        ObjKind::Box { w, h, .. } => {
                            widths[i] = w * sc;
                            asc[i] = h * sc;
                            is_img[i] = true;
                        }
                        ObjKind::NoteRef { endnote, id } => {
                            let self_label = if id == "self" { NOTE_SELF.with(|s| s.borrow().clone()) } else { None };
                            let label = if let Some(l) = self_label {
                                l
                            } else if *endnote {
                                self.endnotes += 1;
                                roman(self.endnotes)
                            } else {
                                self.footnotes += 1;
                                self.footnotes.to_string()
                            };
                            let mut lf = f;
                            lf.script = 1;
                            widths[i] = self.text_width(&lf, &label);
                            let (a, d) = self.extent(&lf);
                            asc[i] = a;
                            desc[i] = d;
                            self.obj_text.insert((pi, i), label);
                        }
                        ObjKind::Deleted(s) if show_markup() => {
                            widths[i] = self.text_width(&f, s);
                            let (a, d) = self.extent(&f);
                            asc[i] = a;
                            desc[i] = d;
                        }
                        ObjKind::Label(s) => {
                            widths[i] = self.text_width(&f, s);
                            let (a, d) = self.extent(&f);
                            asc[i] = a;
                            desc[i] = d;
                        }
                        _ => {}
                    }
                }
                continue;
            }
            if cur.as_ref().map(|c| c.0) != Some(f) {
                let (a, d) = self.extent(&f);
                cur = Some((f, a, d));
            }
            let (_, a, d) = cur.unwrap();
            widths[i] = match c {
                '\t' | PAGE_BREAK | LINE_BREAK | COLUMN_BREAK | '\u{00AD}' => 0.0,
                _ => {
                    let id = font_for_char(&f, c, sc);
                    self.fonts.glyph_width(&id, display_char(&f, c)) + f.char_spacing * sc
                }
            };
            asc[i] = a;
            desc[i] = d;
        }
        let (end_a, end_d) = self.extent(&para.end_fmt);

        // List marker.
        let mut marker = None;
        let mut first_text_x = first_x;
        if let Some(l) = label {
            let mut mf = para.fmt_at(0);
            mf.script = 0;
            mf.underline = false;
            let mw = self.text_width(&mf, l);
            marker = Some((l.clone(), first_x));
            first_text_x = if para.ind_first < 0.0 && first_x + mw + 2.0 <= left {
                left
            } else {
                let stop = self.default_tab * sc;
                let after = first_x + mw + 0.25 * stop / 2.0;
                if para.ind_first < 0.0 { after.max(left) } else { after }
            };
        }

        let segment_width = |from: usize, decimal: bool| -> f32 {
            let mut w = 0.0;
            for k in from..n {
                let c = para.chars[k];
                if c == '\t' || c == PAGE_BREAK || c == LINE_BREAK || c == COLUMN_BREAK {
                    break;
                }
                if decimal && (c == '.' || c == ',') {
                    break;
                }
                w += widths[k];
            }
            w
        };

        let mut lines: Vec<Line> = Vec::new();
        let mut y = 0.0f32;
        let mut i = 0usize;
        loop {
            let line_start = i;
            let x0 = if lines.is_empty() { first_text_x } else { left };
            let avail = (right - x0).max(10.0);
            let mut x = 0.0f32;
            let mut last_break: Option<usize> = None;
            let mut ws: Vec<f32> = Vec::new();
            let mut leaders = Vec::new();
            let mut j = i;
            let mut forced: Option<char> = None;
            let end;
            loop {
                if j >= n {
                    end = n;
                    break;
                }
                let c = para.chars[j];
                if c == PAGE_BREAK || c == LINE_BREAK || c == COLUMN_BREAK {
                    ws.push(0.0);
                    end = j + 1;
                    forced = Some(c);
                    break;
                }
                let w = if c == '\t' && !para.fmts[j].hidden {
                    let abs = x0 + x - left_edge;
                    let mut stops: Vec<doc::TabStop> = para.tabs.clone();
                    if para.ind_first < 0.0 {
                        stops.push(doc::TabStop { pos: para.ind_left, kind: TabKind::Left, leader: Leader::None });
                        stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                    }
                    let found = stops.iter().find(|t| t.pos * sc > abs + 0.5 && t.kind != TabKind::Clear);
                    match found {
                        Some(t) => {
                            let stop = t.pos * sc;
                            if t.leader != Leader::None {
                                leaders.push((j, t.leader));
                            }
                            let w = match t.kind {
                                TabKind::Right => stop - abs - segment_width(j + 1, false),
                                TabKind::Center => stop - abs - segment_width(j + 1, false) / 2.0,
                                TabKind::Decimal => stop - abs - segment_width(j + 1, true),
                                _ => stop - abs,
                            };
                            w.max(0.0)
                        }
                        None => {
                            let stop = (self.default_tab * sc).max(4.0);
                            let mut next = ((abs / stop).floor() + 1.0) * stop;
                            if next - abs < 1.0 {
                                next += stop;
                            }
                            next - abs
                        }
                    }
                } else {
                    widths[j]
                };
                if x + w > avail && j > line_start && c != ' ' && !para.fmts[j].hidden {
                    match last_break {
                        Some(b) => {
                            end = b;
                            ws.truncate(b - line_start);
                            leaders.retain(|(k, _)| *k < b);
                        }
                        None => end = j,
                    }
                    break;
                }
                ws.push(w);
                x += w;
                j += 1;
                if c == ' ' || c == '-' || c == '\t' || c == '\u{00AD}' || c == '\u{200B}' {
                    last_break = Some(j);
                }
            }

            // Vertical metrics.
            let mut a = 0.0f32;
            let mut d = 0.0f32;
            let mut text_h = 0.0f32;
            let mut any = false;
            for k in line_start..end {
                if widths[k] == 0.0 && asc[k] == 0.0 {
                    continue;
                }
                if matches!(para.chars[k], PAGE_BREAK | LINE_BREAK | COLUMN_BREAK) && end - line_start > 1 {
                    continue;
                }
                any = true;
                a = a.max(asc[k]);
                d = d.max(desc[k]);
                if !is_img[k] {
                    text_h = text_h.max(asc[k] + desc[k]);
                }
            }
            if !any {
                a = end_a;
                d = end_d;
                text_h = a + d;
            }
            let (height, baseline) = match para.line_rule {
                LineRule::Auto => (a + d + text_h * (para.line_spacing - 1.0).max(0.0), a),
                LineRule::Exact => {
                    let h = (para.line_spacing * sc).max(1.0);
                    (h, (h - d).max(0.0))
                }
                LineRule::AtLeast => {
                    let h = (para.line_spacing * sc).max(a + d);
                    (h, h - d)
                }
            };

            // Horizontal alignment.
            let len = end - line_start;
            let mut trailing = 0;
            while trailing < len {
                let c = para.chars[end - 1 - trailing];
                if c == ' ' || c == PAGE_BREAK || c == LINE_BREAK || c == COLUMN_BREAK {
                    trailing += 1;
                } else {
                    break;
                }
            }
            let natural: f32 = ws[..len - trailing].iter().sum();
            let is_last = end >= n && forced.is_none();
            let slack = (avail - natural).max(0.0);
            let mut offset = 0.0;
            let mut per_space = 0.0;
            // Text after a tab is positioned by the tab; alignment applies to the whole line otherwise.
            let has_tab = para.chars[line_start..end].contains(&'\t');
            match para.align {
                Align::Left => {}
                Align::Center if !has_tab => offset = slack / 2.0,
                Align::Right if !has_tab => offset = slack,
                Align::Justify => {
                    let spaces = para.chars[line_start..end - trailing].iter().filter(|c| **c == ' ').count();
                    if !is_last && forced.is_none() && spaces > 0 && !has_tab {
                        per_space = slack / spaces as f32;
                    }
                }
                _ => {}
            }
            let mut xs = Vec::with_capacity(len + 1);
            let mut cx = x0 + offset;
            xs.push(cx);
            for k in 0..len {
                cx += ws[k];
                if per_space > 0.0 && k < len - trailing && para.chars[line_start + k] == ' ' {
                    cx += per_space;
                }
                xs.push(cx);
            }

            lines.push(Line {
                para: pi,
                start: line_start,
                end,
                page: 0,
                y,
                height,
                baseline,
                xs,
                last_in_para: false,
                forced: forced.is_some(),
                leaders,
            });
            y += height;
            i = end;
            if i >= n && forced.is_none() {
                break;
            }
        }
        if let Some(l) = lines.last_mut() {
            l.last_in_para = true;
        }
        ParaLines { lines, marker, left, right }
    }
}

fn roman(mut n: u32) -> String {
    let table = [(10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut s = String::new();
    while n >= 40 {
        s.push_str("xl");
        n -= 40;
    }
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// Whether a section break of this paragraph starts a new page.
/// Whether the section starting after break number `n` begins on a new page.
/// The break type lives in the *following* section's `w:sectPr`.
fn section_breaks_page(sects: &[String], n: usize) -> bool {
    !sects.get(n + 1).is_some_and(|s| s.contains("w:type w:val=\"continuous\""))
}

struct Pager {
    page: usize,
    y: f32,
    content_h: f32,
    mt: f32,
    force: bool,
    /// Height reserved for footnotes at the bottom of the current page.
    notes_h: f32,
    /// Number of text columns and the current one.
    cols: usize,
    col: usize,
    /// Where the current set of columns starts on this page.
    col_top: f32,
    /// Lowest point reached on the page (across columns).
    max_y: f32,
    /// Move to the next column (column break).
    force_col: bool,
    /// Vertical ranges (relative to the top margin) kept clear by floating objects on this page.
    bands: Vec<(f32, f32)>,
}

impl Pager {
    fn place(&mut self, h: f32) -> (usize, f32) {
        self.place_with(h, 0.0)
    }

    fn new_page(&mut self) {
        self.page += 1;
        self.y = 0.0;
        self.notes_h = 0.0;
        self.col = 0;
        self.col_top = 0.0;
        self.max_y = 0.0;
        self.force = false;
        self.force_col = false;
        self.bands.clear();
    }

    /// Places a block of height `h` that brings `notes` of footnote height with it.
    fn place_with(&mut self, h: f32, notes: f32) -> (usize, f32) {
        // Skip past floating objects that text must keep clear of.
        for _ in 0..8 {
            let hit = self.bands.iter().copied().find(|&(t, b)| self.y < b && self.y + h > t && !self.force);
            match hit {
                Some((_, b)) => self.y = b,
                None => break,
            }
        }
        let full = self.y + h + self.notes_h + notes > self.content_h && self.y > self.col_top;
        if self.force {
            self.new_page();
        } else if full || self.force_col {
            self.force_col = false;
            if self.col + 1 < self.cols {
                self.col += 1;
                self.y = self.col_top;
            } else {
                self.new_page();
            }
        }
        let at = (self.page, self.mt + self.y);
        self.y += h;
        self.max_y = self.max_y.max(self.y);
        self.notes_h += notes;
        at
    }

    /// Whether a block of height `h` fits below the current position.
    fn fits(&self, h: f32) -> bool {
        self.y + h + self.notes_h <= self.content_h
    }

    /// Moves to the next column or page (a pagination-rule break).
    fn advance(&mut self) {
        if self.y <= self.col_top {
            return;
        }
        if self.col + 1 < self.cols {
            self.col += 1;
            self.y = self.col_top;
        } else {
            self.force = true;
        }
    }

    /// Starts a new set of columns below everything on the current page.
    fn set_columns(&mut self, cols: usize) {
        if cols != self.cols || self.col > 0 {
            self.y = self.max_y.max(self.y);
            self.col_top = self.y;
            self.col = 0;
        }
        self.cols = cols.max(1);
    }
}

/// Rebalances the lines of a multi-column section on its last page so the
/// columns end at about the same height (Word does this before a continuous
/// section break). Returns the new bottom y.
#[allow(clippy::too_many_arguments)]
fn balance_columns(
    lines: &mut [Line],
    line_cols: &mut [usize],
    boxes: &mut [Option<ParaBox>],
    from: usize,
    ncols: usize,
    col_w: f32,
    gap: f32,
    page: usize,
) -> Option<f32> {
    let idx: Vec<usize> = (from..lines.len()).filter(|i| lines[*i].page == page).collect();
    if ncols < 2 || idx.len() < 2 {
        return None;
    }
    let top = idx.iter().map(|i| lines[*i].y).fold(f32::MAX, f32::min);
    // Vertical advance of each line within its original column (keeps paragraph spacing).
    let adv: Vec<f32> = idx
        .iter()
        .enumerate()
        .map(|(k, &i)| match idx.get(k + 1) {
            Some(&n) if line_cols[n] == line_cols[i] && lines[n].y > lines[i].y => lines[n].y - lines[i].y,
            _ => lines[i].height,
        })
        .collect();
    let total: f32 = adv.iter().sum();
    let target = total / ncols as f32;
    let old_cols: Vec<usize> = idx.iter().map(|i| line_cols[*i]).collect();
    let mut col = 0;
    let mut acc = 0.0;
    let mut bottom = top;
    for (k, &i) in idx.iter().enumerate() {
        if acc > 0.0 && acc + adv[k] / 2.0 > target && col + 1 < ncols {
            col += 1;
            acc = 0.0;
        }
        let dx = (col as f32 - old_cols[k] as f32) * (col_w + gap);
        for x in &mut lines[i].xs {
            *x += dx;
        }
        lines[i].y = top + acc;
        line_cols[i] = col;
        acc += adv[k];
        bottom = bottom.max(lines[i].y + lines[i].height);
    }
    // Paragraph boxes follow their first line.
    for b in boxes.iter_mut().flatten() {
        if let Some(&first) = idx.iter().find(|i| b.lines.start == **i) {
            let k = idx.iter().position(|i| *i == first).unwrap();
            let dx = (line_cols[first] as f32 - old_cols[k] as f32) * (col_w + gap);
            b.left += dx;
            b.right += dx;
            if let Some(m) = &mut b.marker {
                m.1 += dx;
            }
        }
    }
    Some(bottom)
}

/// (column count, gap in points) of a section's `w:cols`.
fn section_columns(sect: Option<&String>) -> (usize, f32) {
    let Some(el) = sect.and_then(|s| crate::xml::parse(s).ok()) else { return (1, 36.0) };
    let Some(cols) = el.child("cols") else { return (1, 36.0) };
    let explicit = cols.elems().filter(|c| c.name == "col").count();
    let num = cols.attr("num").and_then(|v| v.parse::<usize>().ok()).unwrap_or(explicit.max(1));
    let space = cols.attr("space").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 20.0).unwrap_or(36.0);
    (num.clamp(1, 12), space)
}

thread_local! {
    /// Number shown by a note's own `footnoteRef` while laying out its text.
    static NOTE_SELF: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// A floating picture or shape placed on a page.
#[derive(Clone)]
pub struct FloatBox {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Anchor character.
    pub para: usize,
    pub off: usize,
    pub behind: bool,
    /// Text keeps clear of the object.
    pub clear: bool,
    pub image: u32,
    pub label: String,
}

/// Reference areas for positioning floats.
struct Frame {
    pw: f32,
    ph: f32,
    margins: [f32; 4],
    col_left: f32,
    col_w: f32,
    sc: f32,
}

/// Anchored object of a character: (anchor, width pt, height pt, label).
fn anchored(doc: &Document, f: &CharFmt) -> Option<(doc::Anchor, f32, f32, String)> {
    if f.image != 0 {
        let img = doc.images.get(&f.image)?;
        return img.anchor.map(|a| (a, img.w, img.h, String::new()));
    }
    match pool::obj(f.obj)?.kind {
        ObjKind::Box { anchor: Some(a), w, h, label } => Some((a, w, h, label)),
        _ => None,
    }
}

/// Floats anchored on a placed line.
fn floats_on_line(doc: &Document, pi: usize, l: &Line, para_top: f32, fr: &Frame) -> Vec<FloatBox> {
    use doc::{HRel, PosSpec, VRel};
    let para = &doc.paras[pi];
    let sc = fr.sc;
    let [mt, mr, mb, ml] = fr.margins;
    let mut out = Vec::new();
    for k in l.start..l.end {
        let f = para.fmts[k];
        let Some((a, w, h, label)) = anchored(doc, &f) else { continue };
        let (w, h) = (w * sc, h * sc);
        let (fx, fw) = match a.h_rel {
            HRel::Page => (0.0, fr.pw),
            HRel::Margin => (ml, fr.pw - ml - mr),
            HRel::Column => (fr.col_left, fr.col_w),
            HRel::Character => (l.xs[k - l.start], 0.0),
        };
        let x = match a.h_pos {
            PosSpec::Offset(o) => fx + o * sc,
            PosSpec::Start => fx,
            PosSpec::Center => fx + (fw - w) / 2.0,
            PosSpec::End => fx + fw - w,
        };
        let (fy, fh) = match a.v_rel {
            VRel::Page => (0.0, fr.ph),
            VRel::Margin => (mt, fr.ph - mt - mb),
            VRel::Paragraph => (para_top, 0.0),
            VRel::Line => (l.y, l.height),
        };
        let y = match a.v_pos {
            PosSpec::Offset(o) => fy + o * sc,
            PosSpec::Start => fy,
            PosSpec::Center => fy + (fh - h) / 2.0,
            PosSpec::End => fy + fh - h,
        };
        out.push(FloatBox {
            page: l.page,
            x,
            y,
            w,
            h,
            para: pi,
            off: k,
            behind: a.behind,
            clear: a.wrap == doc::Wrap::Clear,
            image: f.image,
            label,
        });
    }
    out
}

/// A laid-out footnote or endnote.
#[derive(Clone)]
pub struct NoteBox {
    pub page: usize,
    pub y: f32,
    pub key: (bool, String),
    pub lay: Box<Layout>,
}

/// Lays out a note's text across the body width, numbered `label`.
fn note_layout(main: &Document, note: &Document, fonts: &mut FontsView<'_>, zoom: f32, label: &str) -> Box<Layout> {
    NOTE_SELF.with(|s| *s.borrow_mut() = Some(label.to_string()));
    let l = hf_layout(main, note, fonts, zoom, 0.0);
    NOTE_SELF.with(|s| *s.borrow_mut() = None);
    l
}

/// Footnote references (offset, label, id) on a line.
fn line_note_refs(doc: &Document, obj_text: &HashMap<(usize, usize), String>, pi: usize, l: &Line, endnote: bool) -> Vec<(String, String)> {
    let para = &doc.paras[pi];
    (l.start..l.end)
        .filter_map(|k| {
            let o = pool::obj(para.fmts[k].obj)?;
            match o.kind {
                ObjKind::NoteRef { endnote: e, id } if e == endnote && id != "self" => {
                    Some((obj_text.get(&(pi, k)).cloned().unwrap_or_default(), id))
                }
                _ => None,
            }
        })
        .collect()
}

pub fn layout(doc: &Document, fonts: &mut FontsView<'_>, zoom: f32) -> Layout {
    let sc = zoom * PT_TO_PX;
    let pw = doc.page.width * sc;
    let ph = doc.page.height * sc;
    let [mut mt, mr, mut mb, ml] = doc.page.margins.map(|m| m * sc);

    // Rich headers and footers; the body moves down/up if they don't fit in the margins.
    let any = |v: [usize; 2]| (0..doc.hf.len()).any(|s| v.iter().any(|x| hf_doc(doc, s, *x).is_some()));
    let rich_header = any([0, 1]);
    let rich_footer = any([2, 3]);
    let mut hf: Vec<HfBox> = Vec::new();
    for section in 0..doc.hf.len() {
        for variant in 0..4 {
            let is_header = variant < 2;
            if (is_header && !rich_header) || (!is_header && !rich_footer) {
                continue;
            }
            let Some(sub) = hf_doc(doc, section, variant) else { continue };
            if is_header {
                let l = hf_layout(doc, sub, fonts, zoom, doc.page.header_dist);
                let bottom = doc.page.header_dist * sc + content_height(&l);
                mt = mt.max(bottom + 4.0 * sc);
                hf.push(HfBox { section, variant, lay: l, dy: 0.0 });
            } else {
                let l = hf_layout(doc, sub, fonts, zoom, 0.0);
                let h = content_height(&l);
                mb = mb.max(doc.page.footer_dist * sc + h + 4.0 * sc);
                hf.push(HfBox { section, variant, lay: l, dy: ph - doc.page.footer_dist * sc - h });
            }
        }
    }

    let labels = doc.list_labels();
    let mut ctx = Ctx {
        fonts,
        cache: MetricCache::new(),
        sc,
        default_tab: doc.page.default_tab,
        obj_text: HashMap::new(),
        footnotes: 0,
        endnotes: 0,
    };
    let mut pager = Pager {
        page: 0,
        y: 0.0,
        content_h: (ph - mt - mb).max(20.0),
        mt,
        force: false,
        notes_h: 0.0,
        cols: 1,
        col: 0,
        col_top: 0.0,
        max_y: 0.0,
        force_col: false,
        bands: Vec::new(),
    };
    let mut floats: Vec<FloatBox> = Vec::new();
    let mut nested: Vec<(usize, usize, f32, f32, Box<Layout>)> = Vec::new();
    let page_margins = doc.page.margins.map(|m| m * sc);
    // Footnotes per page: (page, key, layout, height).
    let mut page_notes: Vec<(usize, (bool, String), Box<Layout>, f32)> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    let mut boxes: Vec<Option<ParaBox>> = (0..doc.paras.len()).map(|_| None).collect();
    let mut cells: Vec<CellBox> = Vec::new();
    let mut pending_after = 0.0f32;
    // Index of the first line of each section after the first.
    let mut section_starts: Vec<usize> = Vec::new();
    let sects = doc.section_props();
    // Column of each line, and the first line of the current section.
    let mut line_cols: Vec<usize> = Vec::new();
    let mut section_first_line = 0usize;
    let has_endnotes = doc.notes.keys().any(|(e, _)| *e);
    let mut prev_style: Option<(Option<String>, bool)> = None;

    let mut pi = 0;
    while pi < doc.paras.len() {
        let para = &doc.paras[pi];
        if let Some(c) = para.cell {
            // ---------------------------------------------------- table
            let range = {
                let mut e = pi;
                while e < doc.paras.len() && doc.paras[e].cell.is_some_and(|x| x.table == c.table) {
                    e += 1;
                }
                pi..e
            };
            let table = doc.tables.get(&c.table).cloned().unwrap_or_default();
            pager.y += pending_after;
            pending_after = 0.0;
            if para.page_break_before && pager.y > 0.0 {
                pager.force = true;
            }
            let avail = pw - ml - mr;
            let total: f32 = table.grid.iter().sum::<f32>() * sc;
            let mut widths: Vec<f32> = table.grid.iter().map(|g| g * sc).collect();
            let max_w = pw - ml - (mr * 0.25);
            if total > max_w && total > 0.0 {
                for w in &mut widths {
                    *w *= max_w / total;
                }
            }
            let tw: f32 = widths.iter().sum();
            let x0 = match table.align {
                Some(Align::Center) => ml + (avail - tw) / 2.0,
                Some(Align::Right) => pw - mr - tw,
                _ => ml + table.indent * sc - table.cell_margins[1] * sc,
            };
            let col_x = |col: u32| x0 + widths.iter().take(col as usize).sum::<f32>();
            let tedges = table.display_edges();
            let ncols = table.grid.len().max(1) as u32;
            let last_row = doc.paras[range.end - 1].cell.unwrap().row;
            let mut k = range.start;
            while k < range.end {
                let row = doc.paras[k].cell.unwrap().row;
                let row_props = table.rows.get(&row).cloned().unwrap_or_default();
                // Lay out each cell of the row.
                let mut row_cells: Vec<(doc::CellRef, Vec<(usize, ParaLines, f32)>, f32)> = Vec::new();
                let mut nested_here: HashMap<usize, Box<Layout>> = HashMap::new();
                while k < range.end && doc.paras[k].cell.unwrap().row == row {
                    let cref = doc.paras[k].cell.unwrap();
                    let cp = table.cell(cref.row, cref.col);
                    let m = cp.margins.unwrap_or(table.cell_margins);
                    let cx = col_x(cref.col);
                    let cw: f32 = widths.iter().skip(cref.col as usize).take(cp.span.max(1) as usize).sum();
                    let inner_l = cx + m[1] * sc;
                    let inner_r = (cx + cw - m[3] * sc).max(inner_l + 4.0);
                    let mut paras_in_cell = Vec::new();
                    let mut cy = m[0] * sc;
                    let mut prev_after = 0.0;
                    let mut first = true;
                    while k < range.end && doc.paras[k].cell == Some(cref) {
                        let p = &doc.paras[k];
                        if !first {
                            cy += prev_after + p.space_before * sc;
                        }
                        let pl = match &p.nested {
                            Some(sub) => {
                                // A nested table: lay it out at the cell's inner width.
                                let mut d = (**sub).clone();
                                d.page = doc.page.clone();
                                d.page.width = (inner_r - inner_l) / sc;
                                d.page.height = 20000.0;
                                d.page.margins = [0.0; 4];
                                let nl = Box::new(layout(&d, ctx.fonts, zoom));
                                let h = content_height(&nl).max(4.0 * sc);
                                let len = p.chars.len();
                                nested_here.insert(k, nl);
                                ParaLines {
                                    lines: vec![Line {
                                        para: k,
                                        start: 0,
                                        end: len,
                                        page: 0,
                                        y: 0.0,
                                        height: h,
                                        baseline: h,
                                        xs: vec![inner_l; len + 1],
                                        last_in_para: true,
                                        forced: false,
                                        leaders: Vec::new(),
                                    }],
                                    marker: None,
                                    left: inner_l,
                                    right: inner_r,
                                }
                            }
                            None => ctx.para(doc, k, labels[k].as_ref(), inner_l, inner_r),
                        };
                        let h: f32 = pl.lines.iter().map(|l| l.height).sum();
                        paras_in_cell.push((k, pl, cy));
                        cy += h;
                        prev_after = p.space_after * sc;
                        first = false;
                        k += 1;
                    }
                    cy += prev_after + m[2] * sc;
                    let content_h = if cp.vmerge == VMerge::Continue { 0.0 } else { cy };
                    row_cells.push((cref, paras_in_cell, content_h));
                }
                let mut row_h = row_cells.iter().map(|c| c.2).fold(0.0f32, f32::max).max(4.0 * sc);
                if row_props.height > 0.0 {
                    row_h = if row_props.exact { row_props.height * sc } else { row_h.max(row_props.height * sc) };
                }
                let (page, top) = pager.place(row_h);
                for (cref, paras_in_cell, content_h) in row_cells {
                    let cp = table.cell(cref.row, cref.col);
                    let span = cp.span.max(1);
                    let cx = col_x(cref.col);
                    let cw: f32 = widths.iter().skip(cref.col as usize).take(span as usize).sum();
                    // Imported cells carry resolved edges; generated tables use the table's.
                    let mut edges = if cp.edges.iter().any(|e| e.is_some()) {
                        cp.edges
                    } else {
                        [
                            if cref.row == 0 { tedges.top } else { tedges.inside_h },
                            if cref.col + span >= ncols { tedges.right } else { tedges.inside_v },
                            if cref.row == last_row { tedges.bottom } else { tedges.inside_h },
                            if cref.col == 0 { tedges.left } else { tedges.inside_v },
                        ]
                    };
                    if cp.vmerge == VMerge::Continue {
                        edges[0] = None;
                    }
                    if cp.vmerge != VMerge::None
                        && table.cells.get(&(cref.row + 1, cref.col)).is_some_and(|n| n.vmerge == VMerge::Continue)
                    {
                        edges[2] = None;
                    }
                    cells.push(CellBox {
                        page,
                        x: cx,
                        y: top,
                        w: cw,
                        h: row_h,
                        shade: cp.shade.or(cp.style_shade),
                        edges,
                    });
                    let dy = match cp.valign {
                        doc::VAlign::Top => 0.0,
                        doc::VAlign::Center => ((row_h - content_h) / 2.0).max(0.0),
                        doc::VAlign::Bottom => (row_h - content_h).max(0.0),
                    };
                    for (idx, pl, py) in paras_in_cell {
                        let first_line = lines.len();
                        let para_top = top + py + dy;
                        for mut l in pl.lines {
                            l.page = page;
                            l.y += top + py + dy;
                            let fr = Frame { pw, ph, margins: page_margins, col_left: pl.left, col_w: pl.right - pl.left, sc };
                            floats.extend(floats_on_line(doc, idx, &l, para_top, &fr));
                            if let Some(nl) = nested_here.remove(&idx) {
                                nested.push((idx, page, pl.left, l.y, nl));
                            }
                            lines.push(l);
                        }
                        boxes[idx] = Some(ParaBox { lines: first_line..lines.len(), left: pl.left, right: pl.right, marker: pl.marker });
                    }
                }
            }
            prev_style = None;
            pi = range.end;
            continue;
        }

        // ------------------------------------------------------ paragraph
        let same_style = prev_style.as_ref().is_some_and(|(s, _)| *s == para.style_id);
        let prev_contextual = prev_style.as_ref().is_some_and(|(_, c)| *c);
        if !(same_style && prev_contextual) {
            pager.y += pending_after;
        }
        if para.page_break_before && pager.y > 0.0 {
            pager.force = true;
        }
        if pager.y > 0.0 && !pager.force && !(same_style && para.contextual) {
            pager.y += para.space_before * sc;
        }
        let (ncols, gap) = section_columns(sects.get(section_starts.len()));
        if ncols != pager.cols {
            pager.set_columns(ncols);
        }
        let col_w = ((pw - ml - mr) - gap * sc * (ncols as f32 - 1.0)) / ncols as f32;
        let pl = ctx.para(doc, pi, labels[pi].as_ref(), ml, ml + col_w);
        // Pagination rules: keep lines together, keep with next, orphan control.
        let heights: Vec<f32> = pl.lines.iter().map(|l| l.height).collect();
        let n_lines = heights.len();
        let total_h: f32 = heights.iter().sum();
        if !pager.force && pager.y > pager.col_top {
            let mut need = if para.keep_lines || (para.widow && n_lines <= 3) {
                total_h
            } else if para.widow && n_lines >= 2 {
                heights[0] + heights[1]
            } else {
                heights.first().copied().unwrap_or(0.0)
            };
            if para.keep_next {
                // This paragraph plus the start of the next ones in the chain.
                need = total_h;
                let mut j = pi + 1;
                while j < doc.paras.len() && doc.paras[j].cell.is_none() && j < pi + 8 {
                    let next = &doc.paras[j];
                    let nl = ctx.para(doc, j, labels[j].as_ref(), ml, ml + col_w);
                    let nh: Vec<f32> = nl.lines.iter().map(|l| l.height).collect();
                    need += next.space_before * sc;
                    if next.keep_next {
                        need += nh.iter().sum::<f32>() + next.space_after * sc;
                        j += 1;
                        continue;
                    }
                    need += if next.widow && nh.len() >= 2 { nh[0] + nh[1] } else { nh.first().copied().unwrap_or(0.0) };
                    break;
                }
            }
            // Only break if the block would fit on an empty page.
            if !pager.fits(need) && need <= pager.content_h - pager.col_top {
                pager.advance();
            }
        }
        let first_line = lines.len();
        let mut first_shift = None;
        // Floats are positioned relative to where the paragraph starts, before its
        // own lines are placed, so the paragraph itself can flow around them.
        {
            let first_h = pl.lines.first().map(|l| l.height).unwrap_or(0.0);
            let next_page = pager.force || (pager.y + first_h + pager.notes_h > pager.content_h && pager.y > pager.col_top);
            let (est_page, est_top) = if next_page { (pager.page + 1, pager.mt) } else { (pager.page, pager.mt + pager.y) };
            let shift = pager.col as f32 * (col_w + gap * sc);
            let fr = Frame { pw, ph, margins: page_margins, col_left: ml + shift, col_w, sc };
            let mut y = est_top;
            for l in &pl.lines {
                let mut est = Line { page: est_page, y, xs: l.xs.iter().map(|x| x + shift).collect(), leaders: Vec::new(), ..*l };
                est.para = pi;
                for fb in floats_on_line(doc, pi, &est, est_top, &fr) {
                    if fb.clear && fb.page == pager.page {
                        pager.bands.push((fb.y - mt - 2.0 * sc, fb.y + fb.h - mt + 4.0 * sc));
                    }
                    floats.push(fb);
                }
                y += l.height;
            }
        }
        for (li_idx, mut l) in pl.lines.into_iter().enumerate() {
            // Widow control: don't leave the last line alone at the top of a page.
            if para.widow && n_lines >= 3 && li_idx == n_lines - 2 && !pager.force {
                if pager.fits(heights[li_idx]) && !pager.fits(heights[li_idx] + heights[li_idx + 1]) {
                    pager.advance();
                }
            }
            // Footnotes referenced on this line must fit on the same page.
            let mut new_notes = Vec::new();
            let mut notes_h = 0.0;
            if !doc.notes.is_empty() {
                for (label, id) in line_note_refs(doc, &ctx.obj_text, pi, &l, false) {
                    if let Some(nd) = doc.notes.get(&(false, id.clone())) {
                        let nl = note_layout(doc, nd, ctx.fonts, zoom, &label);
                        let h = content_height(&nl);
                        notes_h += h;
                        new_notes.push(((false, id), nl, h));
                    }
                }
            }
            let separator = if !new_notes.is_empty() { 14.0 * sc } else { 0.0 };
            let (page, y) = pager.place_with(l.height, notes_h + if pager.notes_h == 0.0 { separator } else { 0.0 });
            for (key, nl, h) in new_notes {
                page_notes.push((page, key, nl, h));
            }
            l.page = page;
            l.y = y;
            // Shift lines into their column.
            let shift = pager.col as f32 * (col_w + gap * sc);
            if shift != 0.0 {
                for x in &mut l.xs {
                    *x += shift;
                }
            }
            first_shift.get_or_insert(shift);
            line_cols.resize(lines.len(), 0);
            line_cols.push(pager.col);
            let last = doc.paras[pi].chars.get(l.end.wrapping_sub(1)).copied();
            let page_break = l.forced && last == Some(PAGE_BREAK);
            let column_break = l.forced && last == Some(COLUMN_BREAK);
            lines.push(l);
            if page_break {
                pager.force = true;
            }
            if column_break {
                pager.force_col = true;
            }
        }
        let shift = first_shift.unwrap_or(0.0);
        let marker = pl.marker.map(|(t, x)| (t, x + shift));
        boxes[pi] = Some(ParaBox { lines: first_line..lines.len(), left: pl.left + shift, right: pl.right + shift, marker });
        pending_after = para.space_after * sc;
        prev_style = Some((para.style_id.clone(), para.contextual));
        if para.sect_break && !section_breaks_page(&sects, section_starts.len()) && pager.cols > 1 {
            line_cols.resize(lines.len(), 0);
            if let Some(b) = balance_columns(&mut lines, &mut line_cols, &mut boxes, section_first_line, pager.cols, col_w, gap * sc, pager.page) {
                pager.y = b - pager.mt;
                pager.max_y = pager.y;
                pager.col = 0;
                pager.col_top = pager.y;
            }
        }
        if para.sect_break && section_breaks_page(&sects, section_starts.len()) {
            pager.force = true;
            pending_after = 0.0;
        }
        if para.sect_break {
            section_starts.push(lines.len());
            section_first_line = lines.len();
        }
        pi += 1;
    }

    // Endnotes follow the last paragraph (below all columns), after a separator.
    if pager.cols > 1 && has_endnotes {
        line_cols.resize(lines.len(), 0);
        let (ncols, gap) = section_columns(sects.get(section_starts.len()));
        let col_w = ((pw - ml - mr) - gap * sc * (ncols as f32 - 1.0)) / ncols as f32;
        if let Some(b) = balance_columns(&mut lines, &mut line_cols, &mut boxes, section_first_line, ncols, col_w, gap * sc, pager.page) {
            pager.y = b - pager.mt;
            pager.max_y = pager.y;
        }
    }
    pager.set_columns(1);
    let mut notes: Vec<NoteBox> = Vec::new();
    let mut separators: Vec<(usize, f32)> = Vec::new();
    if doc.notes.keys().any(|(e, _)| *e) {
        let mut first = true;
        for li in 0..lines.len() {
            let pi = lines[li].para;
            let refs = line_note_refs(doc, &ctx.obj_text, pi, &lines[li], true);
            for (label, id) in refs {
                let Some(nd) = doc.notes.get(&(true, id.clone())) else { continue };
                if first {
                    pager.y += 12.0 * sc;
                    let (page, y) = pager.place(6.0 * sc);
                    separators.push((page, y));
                    first = false;
                }
                let nl = note_layout(doc, nd, ctx.fonts, zoom, &label);
                let h = content_height(&nl);
                let (page, y) = pager.place(h);
                notes.push(NoteBox { page, y, key: (true, id), lay: nl });
            }
        }
    }
    // Footnotes stack up from the bottom margin of their page.
    let body_bottom = ph - mb;
    let mut by_page: BTreeMapPages = BTreeMapPages::new();
    for (page, key, nl, h) in page_notes {
        by_page.entry(page).or_default().push((key, nl, h));
    }
    for (page, list) in by_page {
        let total: f32 = list.iter().map(|x| x.2).sum();
        let mut y = body_bottom - total;
        separators.push((page, y - 7.0 * sc));
        for (key, nl, h) in list {
            notes.push(NoteBox { page, y, key, lay: nl });
            y += h;
        }
    }

    // Lines with tracked changes get a change bar.
    let ins_ranges = find_revisions(doc);
    let mut rev_lines = std::collections::HashSet::new();
    for (li, l) in lines.iter().enumerate() {
        let p = &doc.paras[l.para];
        let ins = ins_ranges.iter().any(|(pp, s, e)| *pp == l.para && *s < l.end.max(l.start + 1) && *e > l.start);
        let del = (l.start..l.end).any(|k| pool::obj(p.fmts[k].obj).is_some_and(|o| matches!(o.kind, ObjKind::Deleted(_))));
        if ins || del {
            rev_lines.insert(li);
        }
    }
    let pages = pager.page + 1;
    let section_of_line = |li: usize| section_starts.iter().filter(|s| **s <= li).count();
    let mut page_section = vec![0usize; pages];
    let mut cur = 0;
    for (pg, slot) in page_section.iter_mut().enumerate() {
        if let Some(li) = lines.iter().position(|l| l.page == pg) {
            cur = section_of_line(li);
        }
        *slot = cur;
    }
    let mut section_first_page = vec![0usize];
    for s in &section_starts {
        let pg = lines.get(*s).map(|l| l.page).unwrap_or(pages - 1);
        section_first_page.push(pg);
    }
    // Lines inside tables are emitted cell by cell; keep the page index ordered.
    let mut page_lines = vec![0..0; pages];
    for pg in 0..pages {
        let s = lines.iter().position(|l| l.page == pg).unwrap_or(lines.len());
        let e = lines.iter().rposition(|l| l.page == pg).map(|i| i + 1).unwrap_or(s);
        page_lines[pg] = s..e.max(s);
    }
    let paras = boxes
        .into_iter()
        .map(|b| b.unwrap_or(ParaBox { lines: 0..0, left: ml, right: pw - mr, marker: None }))
        .collect();
    let hff = hf_fmt();
    let header = (doc.header.clone(), ctx.text_width(&hff, &doc.header));
    let footers = (0..pages)
        .map(|pg| {
            let mut s = doc.footer.clone();
            if doc.page_numbers {
                if !s.is_empty() {
                    s.push_str("    ");
                }
                s.push_str(&format!("Page {} of {}", pg + 1, pages));
            }
            let w = ctx.text_width(&hff, &s);
            (s, w)
        })
        .collect();

    Layout {
        lines,
        paras,
        page_lines,
        cells,
        pages,
        scale: sc,
        page_w: pw,
        page_h: ph,
        margins: [mt, mr, mb, ml],
        obj_text: ctx.obj_text,
        header,
        footers,
        hf,
        page_section,
        section_first_page,
        rich_header,
        rich_footer,
        page_fields: find_page_fields(doc),
        notes,
        note_separators: separators,
        floats,
        nested,
        rev_lines,
        ins_ranges,
        comment_ranges: if doc.comments.is_empty() { Vec::new() } else { find_comments(doc) },
    }
}

/// A drawable piece of a line: consecutive characters with one format.
/// Whitespace, objects and small-caps case changes get their own segments.
pub struct Seg {
    pub start: usize,
    pub end: usize,
    pub fmt: CharFmt,
    pub blank: bool,
    /// Lowercase letters drawn as reduced capitals.
    pub small: bool,
}

pub fn segments(para: &doc::Para, line: &Line) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    for k in line.start..line.end {
        let c = para.chars[k];
        let f = para.fmts[k];
        if f.hidden {
            continue;
        }
        let blank = c.is_whitespace() || c == PAGE_BREAK || c == LINE_BREAK || c == COLUMN_BREAK || c == '\u{00AD}';
        let small = f.small_caps && !f.caps && c.is_lowercase();
        if let Some(last) = out.last_mut() {
            if last.fmt == f && last.blank == blank && !blank && !f.is_object() && last.end == k && last.small == small {
                last.end = k + 1;
                continue;
            }
        }
        out.push(Seg { start: k, end: k + 1, fmt: f, blank, small });
    }
    out
}

/// Text of a segment as displayed.
pub fn seg_text(para: &doc::Para, s: &Seg) -> String {
    para.chars[s.start..s.end].iter().map(|c| display_char(&s.fmt, *c)).collect()
}

impl Layout {
    /// x range of chars `[s, e)` on a line.
    pub fn x_range(line: &Line, s: usize, e: usize) -> (f32, f32) {
        (line.xs[s - line.start], line.xs[e - line.start])
    }

    /// Index of the line that displays the caret for `pos`.
    pub fn line_of(&self, pos: Pos) -> usize {
        let pb = &self.paras[pos.p.min(self.paras.len() - 1)];
        for li in pb.lines.clone() {
            let l = &self.lines[li];
            if pos.o < l.end || l.last_in_para {
                return li;
            }
        }
        pb.lines.end.saturating_sub(1).min(self.lines.len().saturating_sub(1))
    }

    /// (page, x, top y, height) of the caret.
    pub fn caret(&self, pos: Pos) -> (usize, f32, f32, f32) {
        if self.lines.is_empty() {
            return (0, 0.0, 0.0, 10.0);
        }
        let l = &self.lines[self.line_of(pos)];
        let k = pos.o.saturating_sub(l.start).min(l.xs.len() - 1);
        (l.page, l.xs[k], l.y, l.height)
    }

    /// Closest document position on line `li` to page-relative `x`.
    pub fn pos_in_line(&self, li: usize, x: f32) -> Pos {
        let l = &self.lines[li];
        let len = l.end - l.start;
        let mut max_k = len;
        if len > 0 && (l.forced || !l.last_in_para) {
            max_k = len - 1;
        }
        let mut best = 0;
        let mut best_d = f32::MAX;
        for k in 0..=max_k {
            let d = (l.xs[k] - x).abs();
            if d < best_d {
                best_d = d;
                best = k;
            }
        }
        Pos::new(l.para, l.start + best)
    }

    fn line_box(&self, li: usize) -> (f32, f32, f32, f32) {
        let l = &self.lines[li];
        let pb = &self.paras[l.para];
        let left = pb.left.min(l.xs[0]) - 2.0;
        let right = pb.right.max(*l.xs.last().unwrap()) + 2.0;
        (left, right, l.y, l.y + l.height)
    }

    /// Hit-test a page-relative point.
    pub fn hit(&self, page: usize, x: f32, y: f32) -> Pos {
        let page = page.min(self.pages - 1);
        let range = self.page_lines[page].clone();
        let mut best: Option<(f32, usize)> = None;
        for li in range.clone() {
            if self.lines[li].page != page {
                continue;
            }
            let (l, r, t, b) = self.line_box(li);
            let dy = if y < t { t - y } else if y > b { y - b } else { 0.0 };
            let dx = if x < l { l - x } else if x > r { x - r } else { 0.0 };
            let d = dy * 4.0 + dx;
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, li));
            }
        }
        match best {
            Some((_, li)) => self.pos_in_line(li, x),
            None => {
                let li = self.lines.iter().rposition(|l| l.page <= page).unwrap_or(0);
                self.pos_in_line(li, x)
            }
        }
    }

    /// The line visually above (dir < 0) or below (dir > 0) line `li` at `x`.
    pub fn vertical(&self, li: usize, x: f32, dir: i32) -> Option<usize> {
        let cur = &self.lines[li];
        let key = |l: &Line| (l.page as f32) * 1.0e6 + l.y;
        let cur_key = key(cur);
        // Nearest line in that direction that overlaps x; otherwise the nearest at all.
        let mut overlap: Option<(f32, usize)> = None;
        let mut any: Option<(f32, f32, usize)> = None;
        for (i, l) in self.lines.iter().enumerate() {
            if i == li {
                continue;
            }
            let k = key(l);
            let ahead = if dir > 0 { k > cur_key + 0.5 } else { k < cur_key - 0.5 };
            if !ahead {
                continue;
            }
            let (left, right, _, _) = self.line_box(i);
            let dx = if x < left { left - x } else if x > right { x - right } else { 0.0 };
            let dist = (k - cur_key).abs();
            if dx < 1.0 && overlap.is_none_or(|(d, _)| dist < d) {
                overlap = Some((dist, i));
            }
            if any.is_none_or(|(d, ddx, _)| dist < d - 0.5 || ((dist - d).abs() <= 0.5 && dx < ddx)) {
                any = Some((dist, dx, i));
            }
        }
        overlap.map(|o| o.1).or(any.map(|a| a.2))
    }
}
