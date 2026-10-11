//! PDF export.
//!
//! The document is laid out with the same engine used on screen (at a scale of
//! one unit per point), so page breaks and line wrapping in the PDF match what
//! the user sees. Fonts are embedded (subset) so text stays selectable.

use crate::doc::{CharFmt, Document, Rgba};
use crate::layout::{self, Layout};
use crate::render::{self, Canvas};
use krilla::Data;
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};
use std::collections::HashMap;
use std::path::Path;

/// Zoom that makes the layout engine produce coordinates in PDF points.
pub const PDF_ZOOM: f32 = 1.0 / layout::PT_TO_PX;

#[derive(Default)]
struct Resources {
    fonts: HashMap<String, Option<Font>>,
    images: HashMap<u32, Option<Image>>,
}

impl Resources {
    fn font(&mut self, f: &CharFmt) -> Option<(Font, bool, bool)> {
        let r = crate::fonts::resolve(f.font, f.bold, f.italic);
        let key = format!("{:?}", r.family);
        let font = self
            .fonts
            .entry(key)
            .or_insert_with(|| {
                let path = crate::fonts::font_file(&r.family)?;
                let bytes = std::fs::read(path).ok()?;
                Font::new(Data::from(bytes), 0)
            })
            .clone()?;
        Some((font, r.fake_bold, r.fake_italic))
    }

    fn image(&mut self, doc: &Document, id: u32) -> Option<Image> {
        self.images
            .entry(id)
            .or_insert_with(|| {
                // Header/footer pictures live in their own documents.
                let bytes = doc.all_images().into_iter().find(|(k, _)| *k == id)?.1.bytes.clone();
                let data = || Data::from(bytes.to_vec());
                match crate::io::image_kind(&bytes).0 {
                    "png" => Image::from_png(data(), true).ok(),
                    "jpeg" => Image::from_jpeg(data(), true).ok(),
                    "gif" => Image::from_gif(data(), true).ok(),
                    _ => None,
                }
                .or_else(|| {
                    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
                    let (iw, ih) = rgba.dimensions();
                    Some(Image::from_rgba8(rgba.into_raw(), iw, ih))
                })
            })
            .clone()
    }
}

struct PdfCanvas<'a, 'b> {
    s: &'a mut Surface<'b>,
    doc: &'a Document,
    res: &'a mut Resources,
}

fn fill(s: &mut Surface, c: Rgba) {
    s.set_stroke(None);
    s.set_fill(Some(Fill { paint: rgb::Color::new(c[0], c[1], c[2]).into(), ..Default::default() }));
}

impl Canvas for PdfCanvas<'_, '_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        let Some(r) = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) else { return };
        let mut pb = PathBuilder::new();
        pb.push_rect(r);
        if let Some(p) = pb.finish() {
            fill(self.s, c);
            self.s.draw_path(&p);
        }
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba) {
        // Lines are drawn as thin filled rectangles (axis-aligned in practice).
        if (y0 - y1).abs() < 0.01 {
            self.rect(x0.min(x1), y0 - width / 2.0, (x1 - x0).abs(), width, c);
        } else {
            self.rect(x0 - width / 2.0, y0.min(y1), width, (y1 - y0).abs(), c);
        }
    }

    fn text(&mut self, x: f32, y: f32, text: &str, f: &CharFmt, size: f32) {
        let Some((font, fake_bold, fake_italic)) = self.res.font(f) else { return };
        fill(self.s, f.color);
        let skew = if fake_italic { -0.2 } else { 0.0 };
        self.s.push_transform(&Transform::from_row(1.0, 0.0, skew, 1.0, x, y));
        self.s.draw_text(Point::from_xy(0.0, 0.0), font.clone(), size, text, false, TextDirection::Auto);
        if fake_bold {
            self.s.draw_text(Point::from_xy(size * 0.03, 0.0), font, size, text, false, TextDirection::Auto);
        }
        self.s.pop();
    }

    fn image(&mut self, id: u32, x: f32, y: f32, w: f32, h: f32) {
        let (Some(img), Some(size)) = (self.res.image(self.doc, id), Size::from_wh(w, h)) else { return };
        self.s.push_transform(&Transform::from_translate(x, y));
        self.s.draw_image(img, size);
        self.s.pop();
    }
}

/// Writes `doc` as a PDF. `lay` must have been produced with [`PDF_ZOOM`].
pub fn export(doc: &Document, lay: &Layout, path: &Path) -> Result<(), String> {
    let mut pdf = krilla::Document::new();
    let mut res = Resources::default();
    let size = Size::from_wh(lay.page_w, lay.page_h).ok_or("invalid page size")?;
    for page in 0..lay.pages {
        let mut p = pdf.start_page_with(PageSettings::new(size));
        let mut s = p.surface();
        {
            let mut c = PdfCanvas { s: &mut s, doc, res: &mut res };
            render::draw_page(&mut c, doc, lay, page, |_, _| {});
        }
        s.finish();
        p.finish();
    }
    let bytes = pdf.finish().map_err(|e| format!("PDF export failed: {e:?}"))?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}
