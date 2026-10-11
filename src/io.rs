//! Native (.fwd JSON), plain text and HTML formats.

use crate::doc::{
    Align, CharFmt, Document, ImageData, LINE_BREAK, ListKind, OBJ, PAGE_BREAK, PageSetup, Para,
    Rgba, Style,
};
use crate::fonts;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub fn hex(c: Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

/// (file extension, mime type) sniffed from image bytes.
pub fn image_kind(b: &[u8]) -> (&'static str, &'static str) {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        ("png", "image/png")
    } else if b.starts_with(&[0xFF, 0xD8]) {
        ("jpeg", "image/jpeg")
    } else if b.starts_with(b"GIF8") {
        ("gif", "image/gif")
    } else if b.starts_with(b"BM") {
        ("bmp", "image/bmp")
    } else {
        ("png", "image/png")
    }
}

/// Builds image metadata from encoded bytes; display size defaults to 96 dpi, capped at `max_w` points.
pub fn image_from_bytes(bytes: Vec<u8>, max_w: f32) -> Result<ImageData, String> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_dimensions()
        .map_err(|e| e.to_string())?;
    let mut dw = w as f32 * 0.75;
    let mut dh = h as f32 * 0.75;
    if dw > max_w {
        dh *= max_w / dw;
        dw = max_w;
    }
    Ok(ImageData { bytes: Arc::new(bytes), px_w: w, px_h: h, w: dw, h: dh })
}

// ---------------------------------------------------------------- native

#[derive(Serialize, Deserialize)]
struct FileDoc {
    app: String,
    version: u32,
    page: PageSetup,
    #[serde(default)]
    header: String,
    #[serde(default)]
    footer: String,
    #[serde(default)]
    page_numbers: bool,
    #[serde(default)]
    images: Vec<FileImage>,
    paras: Vec<FilePara>,
}

#[derive(Serialize, Deserialize)]
struct FileImage {
    id: u32,
    w: f32,
    h: f32,
    data: String,
}

#[derive(Serialize, Deserialize)]
struct FilePara {
    style: Style,
    align: Align,
    list: ListKind,
    level: u8,
    line_spacing: f32,
    space_before: f32,
    space_after: f32,
    end: FileFmt,
    runs: Vec<FileRun>,
}

#[derive(Serialize, Deserialize)]
struct FileRun {
    text: String,
    fmt: FileFmt,
}

#[derive(Serialize, Deserialize)]
struct FileFmt {
    font: String,
    size: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    strike: bool,
    #[serde(default, skip_serializing_if = "is_zero_i8")]
    script: i8,
    color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    highlight: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    image: u32,
}

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero_i8(v: &i8) -> bool {
    *v == 0
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

impl From<&CharFmt> for FileFmt {
    fn from(f: &CharFmt) -> Self {
        FileFmt {
            font: fonts::font_name(f.font),
            size: f.size,
            bold: f.bold,
            italic: f.italic,
            underline: f.underline,
            strike: f.strike,
            script: f.script,
            color: hex(f.color),
            highlight: (f.highlight[3] > 0).then(|| hex(f.highlight)),
            image: f.image,
        }
    }
}

impl From<&FileFmt> for CharFmt {
    fn from(f: &FileFmt) -> Self {
        CharFmt {
            font: fonts::font_id(&f.font),
            size: f.size.clamp(1.0, 1638.0),
            bold: f.bold,
            italic: f.italic,
            underline: f.underline,
            strike: f.strike,
            script: f.script.clamp(-1, 1),
            color: parse_hex(&f.color).unwrap_or(crate::doc::BLACK),
            highlight: f.highlight.as_deref().and_then(parse_hex).unwrap_or(crate::doc::NO_COLOR),
            image: f.image,
        }
    }
}

/// Splits a paragraph into runs of identical formatting.
pub fn runs(p: &Para) -> Vec<(String, CharFmt)> {
    let mut out: Vec<(String, CharFmt)> = Vec::new();
    for (c, f) in p.chars.iter().zip(&p.fmts) {
        match out.last_mut() {
            Some((s, lf)) if lf == f && f.image == 0 => s.push(*c),
            _ => out.push((c.to_string(), *f)),
        }
    }
    out
}

pub fn save_native(doc: &Document, path: &Path) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let fd = FileDoc {
        app: "FreeWord".into(),
        version: 1,
        page: doc.page.clone(),
        header: doc.header.clone(),
        footer: doc.footer.clone(),
        page_numbers: doc.page_numbers,
        images: doc
            .images
            .iter()
            .map(|(id, img)| FileImage { id: *id, w: img.w, h: img.h, data: b64.encode(img.bytes.as_slice()) })
            .collect(),
        paras: doc
            .paras
            .iter()
            .map(|p| FilePara {
                style: p.style,
                align: p.align,
                list: p.list,
                level: p.level,
                line_spacing: p.line_spacing,
                space_before: p.space_before,
                space_after: p.space_after,
                end: (&p.end_fmt).into(),
                runs: runs(p).iter().map(|(t, f)| FileRun { text: t.clone(), fmt: f.into() }).collect(),
            })
            .collect(),
    };
    let json = serde_json::to_string(&fd).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn load_native(path: &Path) -> Result<Document, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let fd: FileDoc = serde_json::from_str(&s).map_err(|e| format!("Not a FreeWord document: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut doc = Document::new();
    doc.page = fd.page;
    doc.header = fd.header;
    doc.footer = fd.footer;
    doc.page_numbers = fd.page_numbers;
    for im in fd.images {
        if let Ok(bytes) = b64.decode(im.data.as_bytes()) {
            if let Ok(mut data) = image_from_bytes(bytes, f32::MAX) {
                data.w = im.w;
                data.h = im.h;
                doc.images.insert(im.id, data);
            }
        }
    }
    doc.paras = fd
        .paras
        .iter()
        .map(|fp| {
            let mut p = Para::new(fp.style);
            p.align = fp.align;
            p.list = fp.list;
            p.level = fp.level.min(8);
            p.line_spacing = fp.line_spacing;
            p.space_before = fp.space_before;
            p.space_after = fp.space_after;
            p.end_fmt = (&fp.end).into();
            for r in &fp.runs {
                let f: CharFmt = (&r.fmt).into();
                for c in r.text.chars() {
                    p.chars.push(c);
                    p.fmts.push(f);
                }
            }
            p
        })
        .collect();
    // Drop pictures whose data could not be decoded.
    for p in &mut doc.paras {
        let mut i = 0;
        while i < p.chars.len() {
            if p.fmts[i].image != 0 && !doc.images.contains_key(&p.fmts[i].image) {
                p.chars.remove(i);
                p.fmts.remove(i);
            } else {
                i += 1;
            }
        }
    }
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(Style::Normal));
    }
    Ok(doc)
}

// ---------------------------------------------------------------- text

pub fn load_text(path: &Path) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let s = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
    let mut doc = Document::new();
    doc.paras = s
        .trim_start_matches('\u{feff}')
        .split('\n')
        .map(|l| {
            let l: String = l.chars().map(|c| if c == '\x0c' { PAGE_BREAK } else { c }).collect();
            Para::with_text(&l, Style::Normal)
        })
        .collect();
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(Style::Normal));
    }
    Ok(doc)
}

pub fn save_text(doc: &Document, path: &Path) -> Result<(), String> {
    let labels = crate::doc::list_labels(&doc.paras);
    let mut s = String::new();
    for (p, label) in doc.paras.iter().zip(labels) {
        if let Some(l) = label {
            s.push_str(&"    ".repeat(p.level as usize));
            s.push_str(&l);
            s.push(' ');
        }
        for &c in &p.chars {
            match c {
                OBJ => {}
                LINE_BREAK => s.push_str("\r\n"),
                PAGE_BREAK => s.push('\x0c'),
                c => s.push(c),
            }
        }
        s.push_str("\r\n");
    }
    std::fs::write(path, s).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- html

fn html_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn css_fmt(f: &CharFmt) -> String {
    let mut s = format!(
        "font-family:'{}';font-size:{}pt;color:#{};",
        fonts::font_name(f.font),
        f.size,
        hex(f.color)
    );
    if f.bold {
        s.push_str("font-weight:bold;");
    }
    if f.italic {
        s.push_str("font-style:italic;");
    }
    match (f.underline, f.strike) {
        (true, true) => s.push_str("text-decoration:underline line-through;"),
        (true, false) => s.push_str("text-decoration:underline;"),
        (false, true) => s.push_str("text-decoration:line-through;"),
        _ => {}
    }
    if f.highlight[3] > 0 {
        s.push_str(&format!("background:#{};", hex(f.highlight)));
    }
    match f.script {
        1 => s.push_str("vertical-align:super;font-size:smaller;"),
        -1 => s.push_str("vertical-align:sub;font-size:smaller;"),
        _ => {}
    }
    s
}

pub fn to_html(doc: &Document, title: &str, print: bool) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let [mt, mr, mb, ml] = doc.page.margins;
    let mut h = String::new();
    h.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    h.push_str(&format!("<title>{}</title>\n<style>\n", html_esc(title)));
    h.push_str(&format!(
        "@page {{ size: {}pt {}pt; margin: {}pt {}pt {}pt {}pt; }}\n",
        doc.page.width, doc.page.height, mt, mr, mb, ml
    ));
    h.push_str("body { margin: 0 auto; max-width: ");
    h.push_str(&format!("{}pt; }}\n", doc.page.width - ml - mr));
    h.push_str("p { margin: 0; white-space: pre-wrap; }\nul, ol { margin: 0; }\n");
    h.push_str(".pb { page-break-after: always; break-after: page; }\n");
    h.push_str("</style>\n");
    if print {
        h.push_str("<script>window.addEventListener('load', () => setTimeout(() => window.print(), 300));</script>\n");
    }
    h.push_str("</head><body>\n");
    if !doc.header.is_empty() {
        h.push_str(&format!("<div style=\"text-align:center;color:#666\">{}</div>\n", html_esc(&doc.header)));
    }
    let labels = crate::doc::list_labels(&doc.paras);
    for (p, label) in doc.paras.iter().zip(labels) {
        let align = match p.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        };
        let mut indent = p.level as f32 * 36.0;
        if p.style == Style::Quote {
            indent += 36.0;
        }
        let tag = match p.style {
            Style::Title | Style::Heading1 => "h1",
            Style::Heading2 => "h2",
            Style::Heading3 => "h3",
            _ => "p",
        };
        let mut style = format!(
            "margin:{}pt 0 {}pt {}pt;text-align:{};line-height:{};font-weight:normal;",
            p.space_before, p.space_after, indent, align, p.line_spacing
        );
        if p.style == Style::Code {
            style.push_str("background:#f3f3f3;");
        }
        if label.is_some() {
            style.push_str(&format!("padding-left:18pt;text-indent:-18pt;margin-left:{}pt;", indent));
        }
        h.push_str(&format!("<{tag} style=\"{style}\">"));
        if let Some(l) = &label {
            h.push_str(&format!(
                "<span style=\"{}display:inline-block;width:18pt;text-indent:0\">{}</span>",
                css_fmt(&p.fmt_at(0)),
                html_esc(l)
            ));
        }
        if p.chars.is_empty() {
            h.push_str(&format!("<span style=\"{}\">&#8203;</span>", css_fmt(&p.end_fmt)));
        }
        let mut page_break = false;
        for (text, f) in runs(p) {
            if f.image != 0 {
                if let Some(img) = doc.images.get(&f.image) {
                    let (_, mime) = image_kind(&img.bytes);
                    h.push_str(&format!(
                        "<img src=\"data:{};base64,{}\" style=\"width:{}pt;height:{}pt;vertical-align:bottom\">",
                        mime,
                        b64.encode(img.bytes.as_slice()),
                        img.w,
                        img.h
                    ));
                }
                continue;
            }
            let mut t = String::new();
            for c in text.chars() {
                match c {
                    LINE_BREAK => t.push_str("<br>"),
                    PAGE_BREAK => page_break = true,
                    '\t' => t.push_str("&emsp;&emsp;"),
                    c => t.push_str(&html_esc(&c.to_string())),
                }
            }
            h.push_str(&format!("<span style=\"{}\">{}</span>", css_fmt(&f), t));
        }
        h.push_str(&format!("</{tag}>\n"));
        if page_break {
            h.push_str("<div class=\"pb\"></div>\n");
        }
    }
    if !doc.footer.is_empty() {
        h.push_str(&format!("<div style=\"text-align:center;color:#666\">{}</div>\n", html_esc(&doc.footer)));
    }
    h.push_str("</body></html>\n");
    h
}

pub fn save_html(doc: &Document, path: &Path, title: &str) -> Result<(), String> {
    std::fs::write(path, to_html(doc, title, false)).map_err(|e| e.to_string())
}

/// Opens a file with the operating system's default handler.
pub fn open_with_system(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").arg("/C").arg("start").arg("").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}
