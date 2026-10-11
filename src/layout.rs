//! Line breaking and pagination.
//!
//! All coordinates produced here are in screen points relative to the top-left
//! corner of the page the line lives on.

use crate::doc::{
    self, Align, CharFmt, Document, LINE_BREAK, ListKind, PAGE_BREAK, Pos, Style,
};
use crate::fonts;
use eframe::egui::{Color32, FontId, epaint::text::FontsView};
use std::collections::HashMap;
use std::ops::Range;

pub const PT_TO_PX: f32 = 96.0 / 72.0;
const TAB_STOP_PT: f32 = 36.0;
const LIST_HANG_PT: f32 = 18.0;
const LEVEL_INDENT_PT: f32 = 36.0;
const SCRIPT_SCALE: f32 = 0.65;

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
}

pub struct ParaBox {
    pub lines: Range<usize>,
    pub left: f32,
    pub right: f32,
    pub marker: Option<(String, f32)>,
}

pub struct Layout {
    pub lines: Vec<Line>,
    pub paras: Vec<ParaBox>,
    pub page_lines: Vec<Range<usize>>,
    pub pages: usize,
    pub scale: f32,
    pub page_w: f32,
    pub page_h: f32,
    /// top, right, bottom, left in screen points.
    pub margins: [f32; 4],
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

/// Ascent/descent for a text character, including script shift.
fn char_extent(fonts: &mut FontsView<'_>, cache: &mut MetricCache, f: &CharFmt, sc: f32) -> (f32, f32, FontId) {
    let (id, _) = font_for(f, sc);
    let (a, h) = metrics(fonts, cache, &id);
    let shift = script_shift(f, sc);
    ((a - shift).max(0.0), (h - a + shift).max(0.0), id)
}

pub fn layout(doc: &Document, fonts: &mut FontsView<'_>, zoom: f32) -> Layout {
    let sc = zoom * PT_TO_PX;
    let pw = doc.page.width * sc;
    let ph = doc.page.height * sc;
    let [mt, mr, mb, ml] = doc.page.margins.map(|m| m * sc);
    let content_h = (ph - mt - mb).max(20.0);
    let labels = doc::list_labels(&doc.paras);
    let mut cache = MetricCache::new();

    let mut lines: Vec<Line> = Vec::new();
    let mut paras = Vec::with_capacity(doc.paras.len());
    let mut page = 0usize;
    let mut y = 0.0f32;
    let mut force_new_page = false;

    for (pi, para) in doc.paras.iter().enumerate() {
        let quote = if para.style == Style::Quote { 36.0 } else { 0.0 };
        let list_hang = if para.list != ListKind::None { LIST_HANG_PT } else { 0.0 };
        let level_x = para.level as f32 * LEVEL_INDENT_PT + quote;
        let left = ml + (level_x + list_hang) * sc;
        let right = pw - mr - quote * sc;
        let avail = (right - left).max(20.0);
        let n = para.len();

        let mut widths = vec![0.0f32; n];
        let mut asc = vec![0.0f32; n];
        let mut desc = vec![0.0f32; n];
        let mut is_img = vec![false; n];
        let mut cur: Option<(CharFmt, f32, f32, FontId)> = None;
        for i in 0..n {
            let f = para.fmts[i];
            let c = para.chars[i];
            if f.image != 0 {
                if let Some(img) = doc.images.get(&f.image) {
                    widths[i] = img.w * sc;
                    asc[i] = img.h * sc;
                    is_img[i] = true;
                    continue;
                }
            }
            if cur.as_ref().map(|c| c.0) != Some(f) {
                let (a, d, id) = char_extent(fonts, &mut cache, &f, sc);
                cur = Some((f, a, d, id));
            }
            let (_, a, d, id) = cur.as_ref().unwrap();
            widths[i] = match c {
                '\t' | PAGE_BREAK | LINE_BREAK => 0.0,
                _ => fonts.glyph_width(id, c),
            };
            asc[i] = *a;
            desc[i] = *d;
        }
        let (end_a, end_d, _) = char_extent(fonts, &mut cache, &para.end_fmt, sc);

        if y > 0.0 && !force_new_page {
            y += para.space_before * sc;
        }
        let first_line = lines.len();
        let mut i = 0usize;
        loop {
            let line_start = i;
            let mut x = 0.0f32;
            let mut last_break: Option<usize> = None;
            let mut ws: Vec<f32> = Vec::new();
            let mut j = i;
            let mut forced: Option<char> = None;
            let end;
            loop {
                if j >= n {
                    end = n;
                    break;
                }
                let c = para.chars[j];
                if c == PAGE_BREAK || c == LINE_BREAK {
                    ws.push(0.0);
                    end = j + 1;
                    forced = Some(c);
                    break;
                }
                let w = if c == '\t' {
                    let stop = TAB_STOP_PT * sc;
                    let abs = (left - ml) + x;
                    let mut next = ((abs / stop).floor() + 1.0) * stop;
                    if next - abs < 2.0 {
                        next += stop;
                    }
                    next - abs
                } else {
                    widths[j]
                };
                if x + w > avail && j > line_start && c != ' ' {
                    match last_break {
                        Some(b) => {
                            end = b;
                            ws.truncate(b - line_start);
                        }
                        None => end = j,
                    }
                    break;
                }
                ws.push(w);
                x += w;
                j += 1;
                if c == ' ' || c == '-' || c == '\t' {
                    last_break = Some(j);
                }
            }

            // Vertical metrics.
            let mut a = 0.0f32;
            let mut d = 0.0f32;
            let mut text_h = 0.0f32;
            let mut any = false;
            for k in line_start..end {
                if matches!(para.chars[k], PAGE_BREAK | LINE_BREAK) && end - line_start > 1 {
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
            let height = a + d + text_h * (para.line_spacing - 1.0).max(0.0);

            if force_new_page || (y + height > content_h && y > 0.0) {
                page += 1;
                y = 0.0;
                force_new_page = false;
            }

            // Horizontal alignment.
            let len = end - line_start;
            let mut trailing = 0;
            while trailing < len {
                let c = para.chars[end - 1 - trailing];
                if c == ' ' || c == PAGE_BREAK || c == LINE_BREAK {
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
            match para.align {
                Align::Left => {}
                Align::Center => offset = slack / 2.0,
                Align::Right => offset = slack,
                Align::Justify => {
                    let spaces = para.chars[line_start..end - trailing]
                        .iter()
                        .filter(|c| **c == ' ')
                        .count();
                    if !is_last && forced.is_none() && spaces > 0 {
                        per_space = slack / spaces as f32;
                    }
                }
            }
            let mut xs = Vec::with_capacity(len + 1);
            let mut cx = left + offset;
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
                page,
                y: mt + y,
                height,
                baseline: a,
                xs,
                last_in_para: false,
                forced: forced.is_some(),
            });
            y += height;
            if forced == Some(PAGE_BREAK) {
                force_new_page = true;
            }
            i = end;
            if i >= n && forced.is_none() {
                break;
            }
        }
        if let Some(l) = lines.last_mut() {
            l.last_in_para = true;
        }
        y += para.space_after * sc;
        paras.push(ParaBox {
            lines: first_line..lines.len(),
            left,
            right,
            marker: labels[pi].clone().map(|l| (l, ml + level_x * sc)),
        });
    }

    let pages = page + 1;
    let mut page_lines = vec![0..0; pages];
    let mut k = 0;
    for (pg, range) in page_lines.iter_mut().enumerate() {
        let s = k;
        while k < lines.len() && lines[k].page == pg {
            k += 1;
        }
        *range = s..k;
    }

    Layout {
        lines,
        paras,
        page_lines,
        pages,
        scale: sc,
        page_w: pw,
        page_h: ph,
        margins: [mt, mr, mb, ml],
    }
}

/// A drawable piece of a line: consecutive characters with one format.
/// Whitespace and pictures always get their own segments.
pub struct Seg {
    pub start: usize,
    pub end: usize,
    pub fmt: CharFmt,
    pub blank: bool,
}

pub fn segments(para: &doc::Para, line: &Line) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    for k in line.start..line.end {
        let c = para.chars[k];
        let f = para.fmts[k];
        let blank = c.is_whitespace() || c == PAGE_BREAK || c == LINE_BREAK;
        if let Some(last) = out.last_mut() {
            if last.fmt == f && last.blank == blank && !blank && f.image == 0 && last.end == k {
                last.end = k + 1;
                continue;
            }
        }
        out.push(Seg { start: k, end: k + 1, fmt: f, blank });
    }
    out
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
        pb.lines.end.saturating_sub(1)
    }

    /// (page, x, top y, height) of the caret.
    pub fn caret(&self, pos: Pos) -> (usize, f32, f32, f32) {
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

    /// Hit-test a page-relative point.
    pub fn hit(&self, page: usize, x: f32, y: f32) -> Pos {
        let page = page.min(self.pages - 1);
        let mut range = self.page_lines[page].clone();
        if range.is_empty() {
            // Find the nearest non-empty page.
            range = self
                .page_lines
                .iter()
                .rev()
                .find(|r| !r.is_empty())
                .cloned()
                .unwrap_or(0..1);
        }
        let mut li = range.start;
        for k in range.clone() {
            let l = &self.lines[k];
            li = k;
            if y < l.y + l.height {
                break;
            }
        }
        self.pos_in_line(li, x)
    }
}
