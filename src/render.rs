//! Page painting shared by the screen view and PDF export.
//!
//! Coordinates are page-relative layout units (screen points on screen,
//! PDF points when laid out with `pdf::PDF_ZOOM`).

use crate::doc::{CharFmt, Document, Leader, Rgba, Style};
use crate::layout::{self, Layout, Line};
use crate::pool::{self, ObjKind};

pub trait Canvas {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba);
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba);
    /// Draws `text` with its left end on the baseline at (x, y); `size` is the font size in layout units.
    fn text(&mut self, x: f32, y: f32, text: &str, f: &CharFmt, size: f32);
    fn image(&mut self, id: u32, x: f32, y: f32, w: f32, h: f32);
}

fn dashed_rect<C: Canvas>(c: &mut C, x: f32, y: f32, w: f32, h: f32, col: Rgba) {
    let dash = 4.0;
    let mut t = 0.0;
    while t < w {
        let e = (t + dash).min(w);
        c.line(x + t, y, x + e, y, 1.0, col);
        c.line(x + t, y + h, x + e, y + h, 1.0, col);
        t += dash * 2.0;
    }
    let mut t = 0.0;
    while t < h {
        let e = (t + dash).min(h);
        c.line(x, y + t, x, y + e, 1.0, col);
        c.line(x + w, y + t, x + w, y + e, 1.0, col);
        t += dash * 2.0;
    }
}

fn leader<C: Canvas>(c: &mut C, x0: f32, x1: f32, base: f32, kind: Leader, f: &CharFmt, sc: f32) {
    let size = f.size * sc;
    match kind {
        Leader::None => {}
        Leader::Underscore => c.line(x0, base + 1.0, x1, base + 1.0, (size * 0.05).max(0.6), f.color),
        Leader::Hyphen => {
            let step = size * 0.45;
            let mut x = x0 + step * 0.3;
            while x + step * 0.6 < x1 {
                c.line(x, base - size * 0.25, x + step * 0.5, base - size * 0.25, (size * 0.05).max(0.6), f.color);
                x += step;
            }
        }
        Leader::Dot => {
            let step = size * 0.3;
            let r = (size * 0.045).max(0.5);
            let mut x = x0 + step * 0.5;
            while x + step * 0.5 < x1 {
                c.rect(x - r, base - r * 2.0, r * 2.0, r * 2.0, f.color);
                x += step;
            }
        }
    }
}

/// Shifts drawing vertically (used to place footers).
struct Offset<'a, C: Canvas> {
    inner: &'a mut C,
    dy: f32,
}

impl<C: Canvas> Canvas for Offset<'_, C> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        self.inner.rect(x, y + self.dy, w, h, c);
    }
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba) {
        self.inner.line(x0, y0 + self.dy, x1, y1 + self.dy, width, c);
    }
    fn text(&mut self, x: f32, y: f32, text: &str, f: &CharFmt, size: f32) {
        self.inner.text(x, y + self.dy, text, f, size);
    }
    fn image(&mut self, id: u32, x: f32, y: f32, w: f32, h: f32) {
        self.inner.image(id, x, y + self.dy, w, h);
    }
}

/// Paints everything on `page` except overlays. `under` is called for each
/// line after backgrounds and before text (selection, search highlights).
pub fn draw_page<C: Canvas>(c: &mut C, doc: &Document, lay: &Layout, page: usize, under: impl FnMut(&mut C, usize)) {
    let pages = lay.pages;
    draw_content(c, doc, lay, page, (page + 1, pages), under);
    let sc = lay.scale;
    let subs = [&doc.hf.header, &doc.hf.header_first, &doc.hf.footer, &doc.hf.footer_first];
    for header in [true, false] {
        let rich = if header { lay.rich_header } else { lay.rich_footer };
        if rich {
            if let Some(i) = layout::hf_variant(doc, header, page) {
                if let (Some(l), Some(d)) = (&lay.hf[i], subs[i]) {
                    let mut oc = Offset { inner: &mut *c, dy: lay.hf_dy[i] };
                    for p in 0..l.pages {
                        draw_content(&mut oc, d, l, p, (page + 1, pages), |_, _| {});
                    }
                }
            }
            continue;
        }
        // Plain-text header/footer.
        let hf = layout::hf_fmt();
        let size = hf.size * sc;
        let [_, mr, _, ml] = lay.margins;
        let cx = ml + (lay.page_w - ml - mr) / 2.0;
        if header {
            if !lay.header.0.is_empty() {
                let y = doc.page.header_dist * sc + size * 0.8;
                c.text(cx - lay.header.1 / 2.0, y, &lay.header.0, &hf, size);
            }
        } else if let Some((text, w)) = lay.footers.get(page) {
            if !text.is_empty() {
                let y = lay.page_h - doc.page.footer_dist * sc - size * 0.25;
                c.text(cx - w / 2.0, y, text, &hf, size);
            }
        }
    }
}

/// Paints the paragraphs and tables of one page of `lay`.
fn draw_content<C: Canvas>(
    c: &mut C,
    doc: &Document,
    lay: &Layout,
    page: usize,
    page_no: (usize, usize),
    mut under: impl FnMut(&mut C, usize),
) {
    let sc = lay.scale;
    // Table cell shading.
    for cell in lay.cells.iter().filter(|b| b.page == page) {
        if let Some(s) = cell.shade {
            c.rect(cell.x, cell.y, cell.w, cell.h, s);
        }
    }
    for li in lay.page_lines[page].clone() {
        let l = &lay.lines[li];
        if l.page != page {
            continue;
        }
        let para = &doc.paras[l.para];
        let pb = &lay.paras[l.para];
        // Paragraph shading.
        let shade = if para.shading[3] > 0 {
            Some(para.shading)
        } else if para.style == Style::Code && para.style_id.as_deref().is_none_or(|s| s == "Code") {
            Some([0xF3, 0xF3, 0xF3, 255])
        } else {
            None
        };
        if let Some(s) = shade {
            c.rect(pb.left - 4.0, l.y, pb.right - pb.left + 8.0, l.height, s);
        }
        // Character highlights.
        for s in layout::segments(para, l) {
            if s.fmt.highlight[3] > 0 && !s.fmt.is_object() {
                let (x0, x1) = Layout::x_range(l, s.start, s.end);
                c.rect(x0, l.y, x1 - x0, l.height, s.fmt.highlight);
            }
        }
        under(c, li);
        draw_line(c, doc, lay, l, page_no);
    }
    // Cell borders on top of everything else.
    for cell in lay.cells.iter().filter(|b| b.page == page && b.borders) {
        let k: Rgba = [0, 0, 0, 255];
        let w = (0.5 * sc).max(0.75);
        if !cell.merged_above {
            c.line(cell.x, cell.y, cell.x + cell.w, cell.y, w, k);
        }
        c.line(cell.x, cell.y + cell.h, cell.x + cell.w, cell.y + cell.h, w, k);
        c.line(cell.x, cell.y, cell.x, cell.y + cell.h, w, k);
        c.line(cell.x + cell.w, cell.y, cell.x + cell.w, cell.y + cell.h, w, k);
    }
}

fn draw_line<C: Canvas>(c: &mut C, doc: &Document, lay: &Layout, l: &Line, page_no: (usize, usize)) {
    let sc = lay.scale;
    let para = &doc.paras[l.para];
    let base = l.y + l.baseline;
    // List marker.
    if l.start == 0 {
        if let Some((label, mx)) = &lay.paras[l.para].marker {
            let mut f = para.fmt_at(0);
            f.script = 0;
            f.underline = false;
            f.strike = false;
            f.highlight = crate::doc::NO_COLOR;
            c.text(*mx, base, label, &f, f.size * sc);
        }
    }
    for (k, kind) in &l.leaders {
        let (x0, x1) = Layout::x_range(l, *k, *k + 1);
        leader(c, x0, x1, base, *kind, &para.fmts[*k], sc);
    }
    let fields: Vec<&(usize, usize, usize, bool)> =
        lay.page_fields.iter().filter(|(p, s, e, _)| *p == l.para && *s < l.end && *e > l.start).collect();
    for s in layout::segments(para, l) {
        let f = s.fmt;
        let (x0, x1) = Layout::x_range(l, s.start, s.end);
        // PAGE / NUMPAGES results show the live number instead of the stored text.
        if let Some((_, fs, _, total)) = fields.iter().find(|(_, fs, fe, _)| s.start >= *fs && s.start < *fe) {
            if s.start == *fs.max(&l.start) {
                let n = if *total { page_no.1 } else { page_no.0 };
                let (fid, _) = layout::font_for(&f, sc);
                c.text(x0, base + layout::script_shift(&f, sc), &n.to_string(), &f, fid.size);
            }
            continue;
        }
        if f.image != 0 {
            if let Some(img) = doc.images.get(&f.image) {
                c.image(f.image, x0, base - img.h * sc, img.w * sc, img.h * sc);
            }
            continue;
        }
        if f.obj != 0 {
            let Some(o) = pool::obj(f.obj) else { continue };
            match &o.kind {
                ObjKind::Box { label, w, h } => {
                    let (w, h) = (w * sc, h * sc);
                    let top = base - h;
                    c.rect(x0, top, w, h, [0xF4, 0xF4, 0xF4, 255]);
                    dashed_rect(c, x0, top, w, h, [0x99, 0x99, 0x99, 255]);
                    let lf = CharFmt { size: 9.0, color: [0x66, 0x66, 0x66, 255], ..f };
                    let text: String = label.chars().take(((w / (5.0 * sc)) as usize).max(3)).collect();
                    c.text(x0 + 4.0 * sc, top + 12.0 * sc, &text, &lf, 9.0 * sc);
                }
                kind => {
                    if let Some(text) = layout::obj_label(lay.obj_text.get(&(l.para, s.start)), kind) {
                        let mut lf = f;
                        if matches!(kind, ObjKind::NoteRef { .. }) {
                            lf.script = 1;
                        }
                        let (fid, _) = layout::font_for(&lf, sc);
                        c.text(x0, base + layout::script_shift(&lf, sc), &text, &lf, fid.size);
                    }
                }
            }
            continue;
        }
        let (fid, _) = layout::font_for(&f, sc);
        let size = if s.small { fid.size * 0.8 } else { fid.size };
        let y = base + layout::script_shift(&f, sc);
        if !s.blank {
            let text = layout::seg_text(para, &s);
            if f.char_spacing != 0.0 {
                // Letter-spaced text is drawn glyph by glyph at the laid-out positions.
                for (i, ch) in text.chars().enumerate() {
                    let x = l.xs[s.start + i - l.start];
                    c.text(x, y, &ch.to_string(), &f, size);
                }
            } else {
                c.text(x0, y, &text, &f, size);
            }
        }
        let th = (fid.size * 0.06).max(0.75);
        if f.underline {
            c.line(x0, y + fid.size * 0.12, x1, y + fid.size * 0.12, th, f.color);
        }
        if f.strike {
            c.line(x0, y - fid.size * 0.28, x1, y - fid.size * 0.28, th, f.color);
        }
    }
}
