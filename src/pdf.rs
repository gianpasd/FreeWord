//! PDF export.
//!
//! The document is laid out with the same engine used on screen (at a scale of
//! one unit per point), so page breaks and line wrapping in the PDF match what
//! the user sees. Fonts are embedded (subset) so text stays selectable.

use crate::doc::{CharFmt, Document, OBJ, Rgba, Style};
use crate::layout::{self, Layout};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};
use krilla::Data;
use std::collections::HashMap;
use std::path::Path;

/// Zoom that makes the layout engine produce coordinates in PDF points.
pub const PDF_ZOOM: f32 = 1.0 / layout::PT_TO_PX;

struct Fonts {
    cache: HashMap<String, Option<Font>>,
}

impl Fonts {
    fn get(&mut self, f: &CharFmt) -> Option<(Font, bool, bool)> {
        let r = crate::fonts::resolve(f.font, f.bold, f.italic);
        let key = format!("{:?}", r.family);
        let font = self
            .cache
            .entry(key)
            .or_insert_with(|| {
                let path = crate::fonts::font_file(&r.family)?;
                let bytes = std::fs::read(path).ok()?;
                Font::new(Data::from(bytes), 0)
            })
            .clone()?;
        Some((font, r.fake_bold, r.fake_italic))
    }
}

fn fill(s: &mut Surface, c: Rgba) {
    s.set_stroke(None);
    s.set_fill(Some(Fill { paint: rgb::Color::new(c[0], c[1], c[2]).into(), ..Default::default() }));
}

fn rect(s: &mut Surface, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
    let Some(r) = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) else { return };
    let mut pb = PathBuilder::new();
    pb.push_rect(r);
    if let Some(p) = pb.finish() {
        fill(s, c);
        s.draw_path(&p);
    }
}

fn text(s: &mut Surface, fonts: &mut Fonts, f: &CharFmt, x: f32, baseline: f32, size: f32, t: &str) {
    let Some((font, fake_bold, fake_italic)) = fonts.get(f) else { return };
    fill(s, f.color);
    let skew = if fake_italic { -0.2 } else { 0.0 };
    s.push_transform(&Transform::from_row(1.0, 0.0, skew, 1.0, x, baseline));
    s.draw_text(Point::from_xy(0.0, 0.0), font.clone(), size, t, false, TextDirection::Auto);
    if fake_bold {
        s.draw_text(Point::from_xy(size * 0.03, 0.0), font, size, t, false, TextDirection::Auto);
    }
    s.pop();
}

fn image(s: &mut Surface, bytes: &[u8], x: f32, y: f32, w: f32, h: f32) {
    let data = || Data::from(bytes.to_vec());
    let img = match crate::io::image_kind(bytes).0 {
        "png" => Image::from_png(data(), true).ok(),
        "jpeg" => Image::from_jpeg(data(), true).ok(),
        "gif" => Image::from_gif(data(), true).ok(),
        _ => None,
    }
    .or_else(|| {
        let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (iw, ih) = rgba.dimensions();
        Some(Image::from_rgba8(rgba.into_raw(), iw, ih))
    });
    let (Some(img), Some(size)) = (img, Size::from_wh(w, h)) else { return };
    s.push_transform(&Transform::from_translate(x, y));
    s.draw_image(img, size);
    s.pop();
}

fn draw_page(s: &mut Surface, doc: &Document, lay: &Layout, page: usize, fonts: &mut Fonts) {
    let sc = lay.scale;
    // Code paragraph shading.
    for li in lay.page_lines[page].clone() {
        let l = &lay.lines[li];
        if doc.paras[l.para].style == Style::Code {
            let pb = &lay.paras[l.para];
            rect(s, pb.left - 4.0, l.y, pb.right - pb.left + 8.0, l.height, [0xF3, 0xF3, 0xF3, 255]);
        }
    }
    for li in lay.page_lines[page].clone() {
        let l = &lay.lines[li];
        let para = &doc.paras[l.para];
        let base = l.y + l.baseline;

        // List marker on the first line of a list paragraph.
        if l.start == 0 {
            if let Some((label, mx)) = &lay.paras[l.para].marker {
                let mut f = para.fmt_at(0);
                f.underline = false;
                f.strike = false;
                f.script = 0;
                text(s, fonts, &f, *mx, base, f.size * sc, label);
            }
        }

        for seg in layout::segments(para, l) {
            let (x0, x1) = Layout::x_range(l, seg.start, seg.end);
            let f = &seg.fmt;
            if f.image != 0 {
                if let Some(img) = doc.images.get(&f.image) {
                    image(s, &img.bytes, x0, base - img.h * sc, img.w * sc, img.h * sc);
                }
                continue;
            }
            let size = f.size * sc * if f.script != 0 { 0.65 } else { 1.0 };
            let y = base + layout::script_shift(f, sc);
            if f.highlight[3] > 0 {
                rect(s, x0, l.y, x1 - x0, l.height, f.highlight);
            }
            if !seg.blank {
                let t: String = para.chars[seg.start..seg.end].iter().filter(|c| **c != OBJ).collect();
                text(s, fonts, f, x0, y, size, &t);
            }
            let th = (size * 0.06).max(0.5);
            if f.underline {
                rect(s, x0, y + size * 0.12, x1 - x0, th, f.color);
            }
            if f.strike {
                rect(s, x0, y - size * 0.28, x1 - x0, th, f.color);
            }
        }
    }

    // Header and footer.
    let [mt, mr, mb, ml] = lay.margins;
    let hf = CharFmt { size: 10.0, color: [0x59, 0x59, 0x59, 255], ..CharFmt::body() };
    let center = |t: &str| ml + (lay.page_w - ml - mr) / 2.0 - t.chars().count() as f32 * 2.4;
    if !doc.header.is_empty() {
        text(s, fonts, &hf, center(&doc.header), mt / 2.0, 10.0, &doc.header);
    }
    let mut footer = doc.footer.clone();
    if doc.page_numbers {
        if !footer.is_empty() {
            footer.push_str("    ");
        }
        footer.push_str(&format!("Page {} of {}", page + 1, lay.pages));
    }
    if !footer.is_empty() {
        text(s, fonts, &hf, center(&footer), lay.page_h - mb / 2.0, 10.0, &footer);
    }
}

/// Writes `doc` as a PDF. `lay` must have been produced with [`PDF_ZOOM`].
pub fn export(doc: &Document, lay: &Layout, path: &Path) -> Result<(), String> {
    let mut pdf = krilla::Document::new();
    let mut fonts = Fonts { cache: HashMap::new() };
    let size = Size::from_wh(lay.page_w, lay.page_h).ok_or("invalid page size")?;
    for page in 0..lay.pages {
        let mut p = pdf.start_page_with(PageSettings::new(size));
        let mut s = p.surface();
        draw_page(&mut s, doc, lay, page, &mut fonts);
        s.finish();
        p.finish();
    }
    let bytes = pdf.finish().map_err(|e| format!("PDF export failed: {e:?}"))?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}
