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
const SCRIPT_SCALE: f32 = 0.65;
const SMALL_CAPS_SCALE: f32 = 0.8;

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

pub struct ParaBox {
    pub lines: Range<usize>,
    /// Horizontal extent of the paragraph's text area.
    pub left: f32,
    pub right: f32,
    pub marker: Option<(String, f32)>,
}

/// A table cell's rectangle on a page.
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
    /// Rich header/footer layouts: header, first-page header, footer, first-page footer.
    pub hf: [Option<Box<Layout>>; 4],
    /// Vertical offset at which each rich header/footer layout is drawn.
    pub hf_dy: [f32; 4],
    /// Whether rich (laid out) headers / footers are used instead of plain text.
    pub rich_header: bool,
    pub rich_footer: bool,
    /// PAGE / NUMPAGES field results: (paragraph, start, end, is NUMPAGES).
    pub page_fields: Vec<(usize, usize, usize, bool)>,
}

/// Which rich header/footer variant (index into `Layout::hf`) applies to a page.
pub fn hf_variant(doc: &Document, header: bool, page: usize) -> Option<usize> {
    let (default, first, base) = if header {
        (&doc.hf.header, &doc.hf.header_first, 0)
    } else {
        (&doc.hf.footer, &doc.hf.footer_first, 2)
    };
    if page == 0 && doc.hf.title_page {
        return first.is_some().then_some(base + 1);
    }
    default.is_some().then_some(base)
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
                    widths[i] = img.w * sc;
                    asc[i] = img.h * sc;
                    is_img[i] = true;
                }
                continue;
            }
            if f.obj != 0 {
                if let Some(o) = pool::obj(f.obj) {
                    match &o.kind {
                        ObjKind::Box { w, h, .. } => {
                            widths[i] = w * sc;
                            asc[i] = h * sc;
                            is_img[i] = true;
                        }
                        ObjKind::NoteRef { endnote, .. } => {
                            let label = if *endnote {
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
fn section_breaks_page(p: &doc::Para) -> bool {
    p.sect_break && !p.ppr_extra.contains("w:val=\"continuous\"")
}

struct Pager {
    page: usize,
    y: f32,
    content_h: f32,
    mt: f32,
    force: bool,
}

impl Pager {
    fn place(&mut self, h: f32) -> (usize, f32) {
        if self.force || (self.y + h > self.content_h && self.y > 0.0) {
            self.page += 1;
            self.y = 0.0;
            self.force = false;
        }
        let at = (self.page, self.mt + self.y);
        self.y += h;
        at
    }
}

pub fn layout(doc: &Document, fonts: &mut FontsView<'_>, zoom: f32) -> Layout {
    let sc = zoom * PT_TO_PX;
    let pw = doc.page.width * sc;
    let ph = doc.page.height * sc;
    let [mut mt, mr, mut mb, ml] = doc.page.margins.map(|m| m * sc);

    // Rich headers and footers; the body moves down/up if they don't fit in the margins.
    let rich_header = doc.pkg.as_ref().is_some_and(|p| p.header_text == doc.header)
        && (doc.hf.header.is_some() || doc.hf.header_first.is_some());
    let rich_footer = doc.pkg.as_ref().is_some_and(|p| p.footer_text == doc.footer && p.page_numbers == doc.page_numbers)
        && (doc.hf.footer.is_some() || doc.hf.footer_first.is_some());
    let mut hf: [Option<Box<Layout>>; 4] = [None, None, None, None];
    let mut hf_dy = [0.0f32; 4];
    let subs = [&doc.hf.header, &doc.hf.header_first, &doc.hf.footer, &doc.hf.footer_first];
    for (i, sub) in subs.into_iter().enumerate() {
        let is_header = i < 2;
        if (is_header && !rich_header) || (!is_header && !rich_footer) {
            continue;
        }
        let Some(sub) = sub else { continue };
        if is_header {
            let l = hf_layout(doc, sub, fonts, zoom, doc.page.header_dist);
            let bottom = doc.page.header_dist * sc + content_height(&l);
            mt = mt.max(bottom + 4.0 * sc);
            hf[i] = Some(l);
        } else {
            let l = hf_layout(doc, sub, fonts, zoom, 0.0);
            let h = content_height(&l);
            hf_dy[i] = ph - doc.page.footer_dist * sc - h;
            mb = mb.max(doc.page.footer_dist * sc + h + 4.0 * sc);
            hf[i] = Some(l);
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
    let mut pager = Pager { page: 0, y: 0.0, content_h: (ph - mt - mb).max(20.0), mt, force: false };
    let mut lines: Vec<Line> = Vec::new();
    let mut boxes: Vec<Option<ParaBox>> = (0..doc.paras.len()).map(|_| None).collect();
    let mut cells: Vec<CellBox> = Vec::new();
    let mut pending_after = 0.0f32;
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
                        let pl = ctx.para(doc, k, labels[k].as_ref(), inner_l, inner_r);
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
                        for mut l in pl.lines {
                            l.page = page;
                            l.y += top + py + dy;
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
        let pl = ctx.para(doc, pi, labels[pi].as_ref(), ml, pw - mr);
        let first_line = lines.len();
        for mut l in pl.lines {
            let (page, y) = pager.place(l.height);
            l.page = page;
            l.y = y;
            let page_break = l.forced && doc.paras[pi].chars.get(l.end.wrapping_sub(1)) == Some(&PAGE_BREAK);
            lines.push(l);
            if page_break {
                pager.force = true;
            }
        }
        boxes[pi] = Some(ParaBox { lines: first_line..lines.len(), left: pl.left, right: pl.right, marker: pl.marker });
        pending_after = para.space_after * sc;
        prev_style = Some((para.style_id.clone(), para.contextual));
        if section_breaks_page(para) {
            pager.force = true;
            pending_after = 0.0;
        }
        pi += 1;
    }

    let pages = pager.page + 1;
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
        hf_dy,
        rich_header,
        rich_footer,
        page_fields: find_page_fields(doc),
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
