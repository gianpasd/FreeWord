//! Native (.fwd JSON), plain text and HTML formats.

use crate::doc::{
    Align, CellRef, CharFmt, Document, ImageData, LINE_BREAK, LineRule, ListKind, OBJ, PAGE_BREAK, Package,
    PageSetup, Para, Rgba, Style, TabStop, Table,
};
use crate::fonts;
use crate::numbering::Numbering;
use crate::pool::{self, ObjKind, RawObj};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub fn hex(c: Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

pub fn parse_hex(s: &str) -> Option<Rgba> {
    crate::styles::parse_hex(s)
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
    Ok(ImageData { bytes: Arc::new(bytes), px_w: w, px_h: h, w: dw, h: dh, raw: None })
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
    #[serde(default)]
    tables: BTreeMap<u32, Table>,
    #[serde(default)]
    numbering: Option<Numbering>,
    #[serde(default)]
    package: Option<FilePackage>,
    paras: Vec<FilePara>,
}

#[derive(Serialize, Deserialize)]
struct FilePackage {
    parts: BTreeMap<String, String>,
    root_attrs: Vec<(String, String)>,
    body_sect: Option<String>,
    header_text: String,
    footer_text: String,
    page_numbers: bool,
    background: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct FileImage {
    id: u32,
    w: f32,
    h: f32,
    data: String,
    #[serde(default)]
    raw: Option<String>,
}

fn d_auto() -> LineRule {
    LineRule::Auto
}

#[derive(Serialize, Deserialize)]
struct FilePara {
    style: Style,
    #[serde(default)]
    style_id: Option<String>,
    align: Align,
    list: ListKind,
    level: u8,
    #[serde(default)]
    num_id: u32,
    line_spacing: f32,
    #[serde(default = "d_auto")]
    line_rule: LineRule,
    space_before: f32,
    space_after: f32,
    #[serde(default)]
    contextual: bool,
    #[serde(default)]
    ind: Option<[f32; 3]>,
    #[serde(default)]
    tabs: Vec<TabStop>,
    #[serde(default)]
    shading: Option<String>,
    #[serde(default)]
    page_break_before: bool,
    #[serde(default)]
    sect_break: bool,
    #[serde(default)]
    ppr_extra: String,
    #[serde(default)]
    cell: Option<CellRef>,
    #[serde(default)]
    raw_block: Option<String>,
    end: FileFmt,
    runs: Vec<FileRun>,
}

#[derive(Serialize, Deserialize)]
struct FileRun {
    text: String,
    fmt: FileFmt,
}

#[derive(Serialize, Deserialize)]
struct FileObj {
    xml: String,
    kind: ObjKind,
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
    #[serde(default, skip_serializing_if = "is_false")]
    caps: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    small_caps: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    hidden: bool,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    char_spacing: f32,
    #[serde(default, skip_serializing_if = "is_zero_i8")]
    script: i8,
    color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    highlight: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    image: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    rpr_extra: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    obj: Option<FileObj>,
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
fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
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
            caps: f.caps,
            small_caps: f.small_caps,
            hidden: f.hidden,
            char_spacing: f.char_spacing,
            script: f.script,
            color: hex(f.color),
            highlight: (f.highlight[3] > 0).then(|| hex(f.highlight)),
            image: f.image,
            link: (f.link != 0).then(|| pool::link(f.link)),
            rpr_extra: pool::get(f.rpr_extra),
            obj: pool::obj(f.obj).map(|o| FileObj { xml: o.xml, kind: o.kind }),
        }
    }
}

fn fmt_from_file(f: &FileFmt, pkg: u64) -> CharFmt {
    CharFmt {
        font: fonts::font_id(&f.font),
        size: f.size.clamp(1.0, 1638.0),
        bold: f.bold,
        italic: f.italic,
        underline: f.underline,
        strike: f.strike,
        caps: f.caps,
        small_caps: f.small_caps,
        hidden: f.hidden,
        char_spacing: f.char_spacing,
        script: f.script.clamp(-1, 1),
        color: parse_hex(&f.color).unwrap_or(crate::doc::BLACK),
        highlight: f.highlight.as_deref().and_then(parse_hex).unwrap_or(crate::doc::NO_COLOR),
        image: f.image,
        link: f.link.as_ref().map(|(u, a)| pool::link_id(u, a)).unwrap_or(0),
        rpr_extra: pool::intern(&f.rpr_extra),
        obj: f
            .obj
            .as_ref()
            .map(|o| pool::add_obj(RawObj { xml: o.xml.clone(), kind: o.kind.clone(), pkg }))
            .unwrap_or(0),
    }
}

/// Splits a paragraph into runs of identical formatting.
pub fn runs(p: &Para) -> Vec<(String, CharFmt)> {
    let mut out: Vec<(String, CharFmt)> = Vec::new();
    for (c, f) in p.chars.iter().zip(&p.fmts) {
        match out.last_mut() {
            Some((s, lf)) if lf == f && !f.is_object() => s.push(*c),
            _ => out.push((c.to_string(), *f)),
        }
    }
    out
}

pub fn save_native(doc: &Document, path: &Path) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let package = doc.pkg.as_ref().map(|p| FilePackage {
        parts: p.parts.iter().map(|(k, v)| (k.clone(), b64.encode(v))).collect(),
        root_attrs: p.root_attrs.clone(),
        body_sect: p.body_sect.clone(),
        header_text: p.header_text.clone(),
        footer_text: p.footer_text.clone(),
        page_numbers: p.page_numbers,
        background: p.background.clone(),
    });
    let fd = FileDoc {
        app: "FreeWord".into(),
        version: 2,
        page: doc.page.clone(),
        header: doc.header.clone(),
        footer: doc.footer.clone(),
        page_numbers: doc.page_numbers,
        images: doc
            .images
            .iter()
            .map(|(id, img)| FileImage {
                id: *id,
                w: img.w,
                h: img.h,
                data: b64.encode(img.bytes.as_slice()),
                raw: img.raw.as_ref().map(|r| r.0.clone()),
            })
            .collect(),
        tables: doc.tables.clone(),
        numbering: Some(doc.numbering.clone()),
        package,
        paras: doc
            .paras
            .iter()
            .map(|p| FilePara {
                style: p.style,
                style_id: p.style_id.clone(),
                align: p.align,
                list: p.list,
                level: p.level,
                num_id: p.num_id,
                line_spacing: p.line_spacing,
                line_rule: p.line_rule,
                space_before: p.space_before,
                space_after: p.space_after,
                contextual: p.contextual,
                ind: Some([p.ind_left, p.ind_right, p.ind_first]),
                tabs: p.tabs.clone(),
                shading: (p.shading[3] > 0).then(|| hex(p.shading)),
                page_break_before: p.page_break_before,
                sect_break: p.sect_break,
                ppr_extra: p.ppr_extra.clone(),
                cell: p.cell,
                raw_block: p.raw_block.clone(),
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
    doc.tables = fd.tables;
    let mut pkg_id = 0;
    if let Some(fp) = fd.package {
        let parts: BTreeMap<String, Vec<u8>> =
            fp.parts.into_iter().filter_map(|(k, v)| b64.decode(v.as_bytes()).ok().map(|b| (k, b))).collect();
        pkg_id = pool::new_pkg_id();
        doc.styles = Arc::new(crate::docx::sheet_from_parts(&parts));
        doc.pkg = Some(Arc::new(Package {
            id: pkg_id,
            parts,
            root_attrs: fp.root_attrs,
            body_sect: fp.body_sect,
            header_text: fp.header_text,
            footer_text: fp.footer_text,
            page_numbers: fp.page_numbers,
            background: fp.background,
        }));
    }
    match fd.numbering {
        Some(n) => doc.numbering = n,
        None => doc.numbering = Numbering::new_default(),
    }
    for im in fd.images {
        if let Ok(bytes) = b64.decode(im.data.as_bytes()) {
            if let Ok(mut data) = image_from_bytes(bytes, f32::MAX) {
                data.w = im.w;
                data.h = im.h;
                data.raw = im.raw.map(|r| (r, pkg_id));
                doc.images.insert(im.id, data);
            }
        }
    }
    let version = fd.version;
    doc.paras = fd
        .paras
        .iter()
        .map(|fp| {
            let mut p = Para::new(fp.style);
            p.style_id = fp.style_id.clone();
            p.align = fp.align;
            p.list = fp.list;
            p.level = fp.level.min(8);
            p.num_id = fp.num_id;
            p.line_spacing = fp.line_spacing;
            p.line_rule = fp.line_rule;
            p.space_before = fp.space_before;
            p.space_after = fp.space_after;
            p.contextual = fp.contextual;
            if let Some([l, r, f]) = fp.ind {
                p.ind_left = l;
                p.ind_right = r;
                p.ind_first = f;
            }
            p.tabs = fp.tabs.clone();
            p.shading = fp.shading.as_deref().and_then(parse_hex).unwrap_or(crate::doc::NO_COLOR);
            p.page_break_before = fp.page_break_before;
            p.sect_break = fp.sect_break;
            p.ppr_extra = fp.ppr_extra.clone();
            p.cell = fp.cell;
            p.raw_block = fp.raw_block.clone();
            p.end_fmt = fmt_from_file(&fp.end, pkg_id);
            for r in &fp.runs {
                let f = fmt_from_file(&r.fmt, pkg_id);
                for c in r.text.chars() {
                    p.chars.push(c);
                    p.fmts.push(f);
                }
            }
            p
        })
        .collect();
    // Version 1 files: list numbering and indent levels were implicit.
    if version < 2 {
        upgrade_v1(&mut doc);
    }
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
    doc.ensure_trailing_para();
    Ok(doc)
}

fn upgrade_v1(doc: &mut Document) {
    let bullet = doc.numbering.bullet_num();
    let mut number = 0;
    let mut prev_numbered = false;
    for i in 0..doc.paras.len() {
        let kind = doc.paras[i].list;
        match kind {
            ListKind::None => {
                let lvl = doc.paras[i].level;
                let p = &mut doc.paras[i];
                p.ind_left += lvl as f32 * 36.0;
                p.level = 0;
                prev_numbered = false;
            }
            ListKind::Bullet => {
                doc.paras[i].num_id = bullet;
            }
            ListKind::Number => {
                if !prev_numbered {
                    number = doc.numbering.new_numbered();
                }
                doc.paras[i].num_id = number;
                prev_numbered = true;
            }
        }
        if kind != ListKind::None {
            let mut p = doc.paras[i].clone();
            doc.apply_list_indent(&mut p);
            doc.paras[i] = p;
        }
    }
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
    let labels = doc.list_labels();
    let mut s = String::new();
    let mut prev_cell = None;
    for (p, label) in doc.paras.iter().zip(labels) {
        if p.cell.is_some() && prev_cell.is_some_and(|c: crate::doc::CellRef| p.cell.unwrap().row == c.row && p.cell != Some(c)) {
            // Next cell in the same row: separate with a tab instead of a newline.
            if s.ends_with("\r\n") {
                s.truncate(s.len() - 2);
            }
            s.push('\t');
        }
        prev_cell = p.cell;
        if let Some(l) = label {
            s.push_str(&"    ".repeat(p.level as usize));
            s.push_str(&l);
            s.push(' ');
        }
        for (&c, f) in p.chars.iter().zip(&p.fmts) {
            if f.hidden {
                continue;
            }
            match c {
                OBJ => {
                    if let Some(o) = pool::obj(f.obj) {
                        if let ObjKind::Label(t) = o.kind {
                            s.push_str(&t);
                        }
                    }
                }
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
    if f.caps {
        s.push_str("text-transform:uppercase;");
    }
    if f.small_caps {
        s.push_str("font-variant:small-caps;");
    }
    if f.char_spacing != 0.0 {
        s.push_str(&format!("letter-spacing:{}pt;", f.char_spacing));
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

fn para_html(doc: &Document, p: &Para, label: Option<&String>, h: &mut String) {
    let b64 = base64::engine::general_purpose::STANDARD;
    let align = match p.align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
    };
    let tag = match p.style {
        crate::doc::Style::Title | crate::doc::Style::Heading1 => "h1",
        crate::doc::Style::Heading2 => "h2",
        crate::doc::Style::Heading3 => "h3",
        _ => "p",
    };
    let line = match p.line_rule {
        LineRule::Auto => format!("{}", p.line_spacing),
        _ => format!("{}pt", p.line_spacing),
    };
    let mut style = format!(
        "margin:{}pt {}pt {}pt {}pt;text-indent:{}pt;text-align:{};line-height:{};font-weight:normal;",
        p.space_before, p.ind_right, p.space_after, p.ind_left, p.ind_first, align, line
    );
    if p.shading[3] > 0 {
        style.push_str(&format!("background:#{};", hex(p.shading)));
    }
    if p.page_break_before {
        style.push_str("break-before:page;");
    }
    h.push_str(&format!("<{tag} style=\"{style}\">"));
    if let Some(l) = label {
        h.push_str(&format!(
            "<span style=\"{}display:inline-block;min-width:18pt;text-indent:0\">{}</span>",
            css_fmt(&p.fmt_at(0)),
            html_esc(l)
        ));
    }
    if p.chars.is_empty() {
        h.push_str(&format!("<span style=\"{}\">&#8203;</span>", css_fmt(&p.end_fmt)));
    }
    let mut page_break = false;
    let mut open_link = 0;
    for (text, f) in runs(p) {
        if f.link != open_link {
            if open_link != 0 {
                h.push_str("</a>");
            }
            open_link = f.link;
            if open_link != 0 {
                let (url, anchor) = pool::link(open_link);
                let href = if url.is_empty() { format!("#{anchor}") } else { url };
                h.push_str(&format!("<a href=\"{}\">", html_esc(&href)));
            }
        }
        if f.hidden {
            continue;
        }
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
        if f.obj != 0 {
            if let Some(o) = pool::obj(f.obj) {
                match o.kind {
                    ObjKind::Label(t) => h.push_str(&format!("<span style=\"{}\">{}</span>", css_fmt(&f), html_esc(&t))),
                    ObjKind::Box { label, w, h: bh } => h.push_str(&format!(
                        "<span style=\"display:inline-block;width:{w}pt;height:{bh}pt;border:1px dashed #999;color:#777;font-size:9pt;text-align:center\">{}</span>",
                        html_esc(&label)
                    )),
                    _ => {}
                }
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
    if open_link != 0 {
        h.push_str("</a>");
    }
    h.push_str(&format!("</{tag}>"));
    if page_break {
        h.push_str("<div class=\"pb\"></div>");
    }
}

pub fn to_html(doc: &Document, title: &str, print: bool) -> String {
    let [mt, mr, mb, ml] = doc.page.margins;
    let mut h = String::new();
    h.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    h.push_str(&format!("<title>{}</title>\n<style>\n", html_esc(title)));
    h.push_str(&format!(
        "@page {{ size: {}pt {}pt; margin: {}pt {}pt {}pt {}pt; }}\n",
        doc.page.width, doc.page.height, mt, mr, mb, ml
    ));
    h.push_str(&format!("body {{ margin: 0 auto; max-width: {}pt; }}\n", doc.page.width - ml - mr));
    h.push_str("p, h1, h2, h3 { margin: 0; white-space: pre-wrap; }\n");
    h.push_str("table { border-collapse: collapse; }\ntd { vertical-align: top; padding: 0 5.4pt; }\n");
    h.push_str(".pb { page-break-after: always; break-after: page; }\n");
    h.push_str("</style>\n");
    if print {
        h.push_str("<script>window.addEventListener('load', () => setTimeout(() => window.print(), 300));</script>\n");
    }
    h.push_str("</head><body>\n");
    if !doc.header.is_empty() {
        h.push_str(&format!("<div style=\"text-align:center;color:#666\">{}</div>\n", html_esc(&doc.header)));
    }
    let labels = doc.list_labels();
    let mut i = 0;
    while i < doc.paras.len() {
        let p = &doc.paras[i];
        if let Some(c) = p.cell {
            let t = doc.tables.get(&c.table).cloned().unwrap_or_default();
            let border = if t.borders { "border:1px solid #000;" } else { "" };
            h.push_str("<table>");
            let range = doc.table_range(c.table);
            let mut k = range.start;
            while k < range.end {
                let row = doc.paras[k].cell.unwrap().row;
                h.push_str("<tr>");
                while k < range.end && doc.paras[k].cell.unwrap().row == row {
                    let cref = doc.paras[k].cell.unwrap();
                    let cp = t.cell(cref.row, cref.col);
                    let w: f32 = t.grid.iter().skip(cref.col as usize).take(cp.span.max(1) as usize).sum();
                    let bg = cp.shade.map(|s| format!("background:#{};", hex(s))).unwrap_or_default();
                    h.push_str(&format!("<td colspan=\"{}\" style=\"{border}{bg}width:{w}pt\">", cp.span.max(1)));
                    while k < range.end && doc.paras[k].cell == Some(cref) {
                        para_html(doc, &doc.paras[k], labels[k].as_ref(), &mut h);
                        k += 1;
                    }
                    h.push_str("</td>");
                }
                h.push_str("</tr>");
            }
            h.push_str("</table>\n");
            i = range.end;
            continue;
        }
        para_html(doc, p, labels[i].as_ref(), &mut h);
        h.push('\n');
        i += 1;
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
