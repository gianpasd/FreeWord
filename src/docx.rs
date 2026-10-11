//! Microsoft Word (.docx) import and export.

use crate::doc::{
    Align, CharFmt, Document, LINE_BREAK, ListKind, OBJ, PAGE_BREAK, Para, Rgba, Style,
};
use crate::fonts;
use crate::io::{hex, image_from_bytes, image_kind, parse_hex, runs};
use crate::xml::{self, El, esc};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const HIGHLIGHTS: &[(&str, Rgba)] = &[
    ("yellow", [255, 255, 0, 255]),
    ("green", [0, 255, 0, 255]),
    ("cyan", [0, 255, 255, 255]),
    ("magenta", [255, 0, 255, 255]),
    ("blue", [0, 0, 255, 255]),
    ("red", [255, 0, 0, 255]),
    ("darkBlue", [0, 0, 128, 255]),
    ("darkCyan", [0, 128, 128, 255]),
    ("darkGreen", [0, 128, 0, 255]),
    ("darkMagenta", [128, 0, 128, 255]),
    ("darkRed", [128, 0, 0, 255]),
    ("darkYellow", [128, 128, 0, 255]),
    ("darkGray", [128, 128, 128, 255]),
    ("lightGray", [192, 192, 192, 255]),
    ("black", [0, 0, 0, 255]),
    ("white", [255, 255, 255, 255]),
];

// ================================================================ export

fn rpr_xml(f: &CharFmt) -> String {
    let font = esc(&fonts::font_name(f.font));
    let mut s = format!("<w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:cs=\"{font}\"/>");
    if f.bold {
        s.push_str("<w:b/>");
    }
    if f.italic {
        s.push_str("<w:i/>");
    }
    if f.strike {
        s.push_str("<w:strike/>");
    }
    s.push_str(&format!("<w:color w:val=\"{}\"/>", hex(f.color)));
    let hp = (f.size * 2.0).round() as i32;
    s.push_str(&format!("<w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/>"));
    let named = (f.highlight[3] > 0)
        .then(|| HIGHLIGHTS.iter().find(|(_, c)| c[..3] == f.highlight[..3]))
        .flatten();
    if let Some((name, _)) = named {
        s.push_str(&format!("<w:highlight w:val=\"{name}\"/>"));
    }
    if f.underline {
        s.push_str("<w:u w:val=\"single\"/>");
    }
    if f.highlight[3] > 0 && named.is_none() {
        s.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(f.highlight)));
    }
    match f.script {
        1 => s.push_str("<w:vertAlign w:val=\"superscript\"/>"),
        -1 => s.push_str("<w:vertAlign w:val=\"subscript\"/>"),
        _ => {}
    }
    s.push_str("</w:rPr>");
    s
}

fn text_run_xml(text: &str, f: &CharFmt) -> String {
    let mut s = String::from("<w:r>");
    s.push_str(&rpr_xml(f));
    let mut buf = String::new();
    let flush = |buf: &mut String, s: &mut String| {
        if !buf.is_empty() {
            s.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(buf)));
            buf.clear();
        }
    };
    for c in text.chars() {
        match c {
            '\t' => {
                flush(&mut buf, &mut s);
                s.push_str("<w:tab/>");
            }
            LINE_BREAK => {
                flush(&mut buf, &mut s);
                s.push_str("<w:br/>");
            }
            PAGE_BREAK => {
                flush(&mut buf, &mut s);
                s.push_str("<w:br w:type=\"page\"/>");
            }
            OBJ => {}
            c => buf.push(c),
        }
    }
    flush(&mut buf, &mut s);
    s.push_str("</w:r>");
    s
}

fn image_run_xml(id: u32, w_pt: f32, h_pt: f32, f: &CharFmt) -> String {
    let cx = (w_pt * 12700.0) as i64;
    let cy = (h_pt * 12700.0) as i64;
    format!(
        "<w:r>{rpr}<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\"/>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"image{id}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImg{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r>",
        rpr = rpr_xml(f)
    )
}

fn para_xml(p: &Para, doc: &Document, num_id: Option<u32>) -> String {
    let mut s = String::from("<w:p><w:pPr>");
    if p.style != Style::Normal {
        s.push_str(&format!("<w:pStyle w:val=\"{}\"/>", p.style.docx_id()));
    }
    if let Some(n) = num_id {
        s.push_str(&format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{n}\"/></w:numPr>", p.level));
    }
    s.push_str(&format!(
        "<w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{}\" w:lineRule=\"auto\"/>",
        (p.space_before * 20.0).round() as i32,
        (p.space_after * 20.0).round() as i32,
        (p.line_spacing * 240.0).round() as i32
    ));
    if num_id.is_none() && p.level > 0 {
        let quote = if p.style == Style::Quote { 720 } else { 0 };
        s.push_str(&format!("<w:ind w:left=\"{}\"/>", p.level as i32 * 720 + quote));
    }
    let jc = match p.align {
        Align::Left => None,
        Align::Center => Some("center"),
        Align::Right => Some("right"),
        Align::Justify => Some("both"),
    };
    if let Some(j) = jc {
        s.push_str(&format!("<w:jc w:val=\"{j}\"/>"));
    }
    s.push_str(&rpr_xml(&p.end_fmt));
    s.push_str("</w:pPr>");
    for (text, f) in runs(p) {
        if f.image != 0 {
            if let Some(img) = doc.images.get(&f.image) {
                s.push_str(&image_run_xml(f.image, img.w, img.h, &f));
            }
        } else {
            s.push_str(&text_run_xml(&text, &f));
        }
    }
    s.push_str("</w:p>");
    s
}

fn style_xml(st: Style) -> String {
    let f = st.char_fmt();
    let (b, a) = st.spacing();
    let mut ppr = format!(
        "<w:pPr><w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{}\" w:lineRule=\"auto\"/>",
        (b * 20.0) as i32,
        (a * 20.0) as i32,
        (st.line_spacing() * 240.0) as i32
    );
    if st == Style::Quote {
        ppr.push_str("<w:ind w:left=\"720\" w:right=\"720\"/>");
    }
    let outline = match st {
        Style::Heading1 => Some(0),
        Style::Heading2 => Some(1),
        Style::Heading3 => Some(2),
        _ => None,
    };
    if let Some(o) = outline {
        ppr.push_str(&format!("<w:outlineLvl w:val=\"{o}\"/>"));
    }
    ppr.push_str("</w:pPr>");
    let name = match st {
        Style::Heading1 => "heading 1",
        Style::Heading2 => "heading 2",
        Style::Heading3 => "heading 3",
        s => s.name(),
    };
    let default = if st == Style::Normal { " w:default=\"1\"" } else { "" };
    let based = if st == Style::Normal {
        String::new()
    } else {
        "<w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>".to_string()
    };
    format!(
        "<w:style w:type=\"paragraph\"{default} w:styleId=\"{}\"><w:name w:val=\"{name}\"/>{based}<w:qFormat/>{ppr}{}</w:style>",
        st.docx_id(),
        rpr_xml(&f)
    )
}

fn numbering_xml(numbered_lists: u32) -> String {
    let mut s = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"{W_NS}\">");
    // abstract 0: bullets
    s.push_str("<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
    for l in 0..9 {
        let sym = crate::doc::bullet_char(l as u8);
        s.push_str(&format!(
            "<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{sym}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            720 * l + 360
        ));
    }
    s.push_str("</w:abstractNum>");
    // abstract 1: numbers
    s.push_str("<w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
    for l in 0..9 {
        let fmt = ["decimal", "lowerLetter", "lowerRoman"][l % 3];
        s.push_str(&format!(
            "<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"%{}.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            l + 1,
            720 * l + 360
        ));
    }
    s.push_str("</w:abstractNum>");
    s.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
    for i in 0..numbered_lists {
        s.push_str(&format!(
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"1\"/></w:lvlOverride></w:num>",
            i + 2
        ));
    }
    s.push_str("</w:numbering>");
    s
}

fn hf_xml(tag: &str, text: &str, page_numbers: bool) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:{tag} xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\"><w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>"
    );
    let gray = "<w:rPr><w:color w:val=\"595959\"/><w:sz w:val=\"20\"/></w:rPr>";
    if !text.is_empty() {
        s.push_str(&format!("<w:r>{gray}<w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(text)));
    }
    if page_numbers {
        let sep = if text.is_empty() { "" } else { "    " };
        s.push_str(&format!("<w:r>{gray}<w:t xml:space=\"preserve\">{sep}Page </w:t></w:r>"));
        s.push_str(&format!("<w:fldSimple w:instr=\" PAGE \"><w:r>{gray}<w:t>1</w:t></w:r></w:fldSimple>"));
        s.push_str(&format!("<w:r>{gray}<w:t xml:space=\"preserve\"> of </w:t></w:r>"));
        s.push_str(&format!("<w:fldSimple w:instr=\" NUMPAGES \"><w:r>{gray}<w:t>1</w:t></w:r></w:fldSimple>"));
    }
    s.push_str(&format!("</w:p></w:{tag}>"));
    s
}

pub fn save_docx(doc: &Document, path: &Path) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();

    // Body; numbered lists get their own numbering instance so they restart at 1.
    let mut body = String::new();
    let mut num_lists = 0u32;
    let mut in_numbered = false;
    for p in &doc.paras {
        let num_id = match p.list {
            ListKind::None => {
                in_numbered = false;
                None
            }
            ListKind::Bullet => Some(1),
            ListKind::Number => {
                if !in_numbered {
                    num_lists += 1;
                    in_numbered = true;
                }
                Some(num_lists + 1)
            }
        };
        body.push_str(&para_xml(p, doc, num_id));
    }
    let has_header = !doc.header.is_empty();
    let has_footer = !doc.footer.is_empty() || doc.page_numbers;
    let mut sect = String::from("<w:sectPr>");
    if has_header {
        sect.push_str("<w:headerReference w:type=\"default\" r:id=\"rIdHeader1\"/>");
    }
    if has_footer {
        sect.push_str("<w:footerReference w:type=\"default\" r:id=\"rIdFooter1\"/>");
    }
    let pg = &doc.page;
    let tw = |v: f32| (v * 20.0).round() as i32;
    let orient = if pg.width > pg.height { " w:orient=\"landscape\"" } else { "" };
    sect.push_str(&format!("<w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/>", tw(pg.width), tw(pg.height)));
    sect.push_str(&format!(
        "<w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/>",
        tw(pg.margins[0]),
        tw(pg.margins[1]),
        tw(pg.margins[2]),
        tw(pg.margins[3])
    ));
    sect.push_str("</w:sectPr>");

    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:document xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<w:body>{body}{sect}</w:body></w:document>"
    );

    let mut styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"{W_NS}\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
        f = esc(fonts::default_font())
    );
    for st in Style::ALL {
        styles.push_str(&style_xml(st));
    }
    styles.push_str("</w:styles>");

    let mut ct = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Default Extension=\"gif\" ContentType=\"image/gif\"/>\
<Default Extension=\"bmp\" ContentType=\"image/bmp\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>",
    );
    if has_header {
        ct.push_str("<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>");
    }
    if has_footer {
        ct.push_str("<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>");
    }
    ct.push_str("</Types>");

    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

    let rt = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut doc_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdStyles\" Type=\"{rt}/styles\" Target=\"styles.xml\"/>\
<Relationship Id=\"rIdNumbering\" Type=\"{rt}/numbering\" Target=\"numbering.xml\"/>"
    );
    if has_header {
        doc_rels.push_str(&format!("<Relationship Id=\"rIdHeader1\" Type=\"{rt}/header\" Target=\"header1.xml\"/>"));
    }
    if has_footer {
        doc_rels.push_str(&format!("<Relationship Id=\"rIdFooter1\" Type=\"{rt}/footer\" Target=\"footer1.xml\"/>"));
    }
    let mut media = Vec::new();
    for (id, img) in &doc.images {
        let (ext, _) = image_kind(&img.bytes);
        let name = format!("media/image{id}.{ext}");
        doc_rels.push_str(&format!("<Relationship Id=\"rIdImg{id}\" Type=\"{rt}/image\" Target=\"{name}\"/>"));
        media.push((format!("word/{name}"), img.bytes.clone()));
    }
    doc_rels.push_str("</Relationships>");

    let file = File::create(path).map_err(|e| err(&e))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut put = |name: &str, data: &[u8]| -> Result<(), String> {
        zip.start_file(name, opts).map_err(|e| err(&e))?;
        zip.write_all(data).map_err(|e| err(&e))
    };
    put("[Content_Types].xml", ct.as_bytes())?;
    put("_rels/.rels", rels.as_bytes())?;
    put("word/document.xml", document.as_bytes())?;
    put("word/_rels/document.xml.rels", doc_rels.as_bytes())?;
    put("word/styles.xml", styles.as_bytes())?;
    put("word/numbering.xml", numbering_xml(num_lists).as_bytes())?;
    if has_header {
        put("word/header1.xml", hf_xml("hdr", &doc.header, false).as_bytes())?;
    }
    if has_footer {
        put("word/footer1.xml", hf_xml("ftr", &doc.footer, doc.page_numbers).as_bytes())?;
    }
    for (name, bytes) in &media {
        put(name, bytes)?;
    }
    zip.finish().map_err(|e| err(&e))?;
    Ok(())
}

// ================================================================ import

struct Ctx {
    styles: HashMap<String, Style>,
    /// (numId, ilvl) -> is bullet
    bullets: HashMap<(String, u8), bool>,
    rels: HashMap<String, String>,
    media: HashMap<String, Vec<u8>>,
    images: Vec<(u32, crate::doc::ImageData)>,
    next_image: u32,
}

fn read_entry(zip: &mut zip::ZipArchive<File>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut v = Vec::new();
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

fn read_xml(zip: &mut zip::ZipArchive<File>, name: &str) -> Option<El> {
    let bytes = read_entry(zip, name)?;
    xml::parse(&String::from_utf8_lossy(&bytes)).ok()
}

fn on(e: &El) -> bool {
    !matches!(e.attr("val"), Some("0" | "false" | "off" | "none"))
}

fn apply_rpr(rpr: &El, f: &mut CharFmt) {
    for e in rpr.elems() {
        match e.name.as_str() {
            "rFonts" => {
                if let Some(n) = e.attr("ascii").or(e.attr("hAnsi")) {
                    f.font = fonts::font_id(n);
                }
            }
            "b" => f.bold = on(e),
            "i" => f.italic = on(e),
            "u" => f.underline = on(e),
            "strike" | "dstrike" => f.strike = on(e),
            "color" => {
                if let Some(c) = e.attr("val").and_then(parse_hex) {
                    f.color = c;
                }
            }
            "sz" => {
                if let Some(v) = e.attr("val").and_then(|v| v.parse::<f32>().ok()) {
                    f.size = (v / 2.0).max(1.0);
                }
            }
            "highlight" => {
                if let Some(v) = e.attr("val") {
                    f.highlight = HIGHLIGHTS
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(v))
                        .map(|(_, c)| *c)
                        .unwrap_or(crate::doc::NO_COLOR);
                }
            }
            "shd" => {
                if let Some(c) = e.attr("fill").and_then(parse_hex) {
                    if c[..3] != [255, 255, 255] {
                        f.highlight = c;
                    }
                }
            }
            "vertAlign" => {
                f.script = match e.attr("val") {
                    Some("superscript") => 1,
                    Some("subscript") => -1,
                    _ => 0,
                }
            }
            _ => {}
        }
    }
}

fn twips(e: &El, key: &str) -> Option<f32> {
    e.attr(key).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 20.0)
}

fn push(p: &mut Para, c: char, f: CharFmt) {
    p.chars.push(c);
    p.fmts.push(f);
}

fn run(r: &El, base: &CharFmt, p: &mut Para, cx: &mut Ctx) {
    let mut f = *base;
    if let Some(rpr) = r.child("rPr") {
        apply_rpr(rpr, &mut f);
    }
    for e in r.elems() {
        match e.name.as_str() {
            "t" => {
                for c in e.text().chars() {
                    push(p, c, f);
                }
            }
            "tab" => push(p, '\t', f),
            "br" => push(p, if e.attr("type") == Some("page") { PAGE_BREAK } else { LINE_BREAK }, f),
            "cr" => push(p, LINE_BREAK, f),
            "noBreakHyphen" => push(p, '-', f),
            "drawing" | "pict" | "object" => {
                let Some(rid) = e.find("blip").and_then(|b| b.attr("embed")).or_else(|| e.find("imagedata").and_then(|b| b.attr("id"))) else {
                    continue;
                };
                let Some(target) = cx.rels.get(rid) else { continue };
                let Some(bytes) = cx.media.get(target).cloned() else { continue };
                let Ok(mut img) = image_from_bytes(bytes, 6.5 * 72.0) else { continue };
                if let Some(ext) = e.find("extent") {
                    let cxv = ext.attr("cx").and_then(|v| v.parse::<f32>().ok());
                    let cyv = ext.attr("cy").and_then(|v| v.parse::<f32>().ok());
                    if let (Some(w), Some(h)) = (cxv, cyv) {
                        img.w = w / 12700.0;
                        img.h = h / 12700.0;
                    }
                }
                let id = cx.next_image;
                cx.next_image += 1;
                cx.images.push((id, img));
                let mut imf = f;
                imf.image = id;
                push(p, OBJ, imf);
            }
            _ => {}
        }
    }
}

fn runs_in(el: &El, base: &CharFmt, p: &mut Para, cx: &mut Ctx) {
    for e in el.elems() {
        match e.name.as_str() {
            "r" => run(e, base, p, cx),
            "hyperlink" | "ins" | "smartTag" | "customXml" | "fldSimple" | "sdt" | "sdtContent" | "bdo" | "dir" => {
                runs_in(e, base, p, cx)
            }
            _ => {}
        }
    }
}

fn paragraph(el: &El, cx: &mut Ctx) -> Para {
    let ppr = el.child("pPr");
    let style = ppr
        .and_then(|p| p.child_val("pStyle"))
        .map(|id| cx.styles.get(id).copied().unwrap_or_else(|| Style::from_docx(id, None)))
        .unwrap_or(Style::Normal);
    let mut para = Para::new(style);
    let base = style.char_fmt();
    if let Some(ppr) = ppr {
        para.align = match ppr.child_val("jc") {
            Some("center") => Align::Center,
            Some("right" | "end") => Align::Right,
            Some("both" | "distribute" | "justify") => Align::Justify,
            _ => Align::Left,
        };
        if let Some(num) = ppr.child("numPr") {
            let id = num.child_val("numId").unwrap_or("0").to_string();
            let lvl: u8 = num.child_val("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
            if id != "0" {
                let bullet = cx.bullets.get(&(id, lvl)).copied().unwrap_or(true);
                para.list = if bullet { ListKind::Bullet } else { ListKind::Number };
                para.level = lvl.min(8);
            }
        }
        if let Some(sp) = ppr.child("spacing") {
            if let Some(v) = twips(sp, "before") {
                para.space_before = v;
            }
            if let Some(v) = twips(sp, "after") {
                para.space_after = v;
            }
            if let Some(l) = sp.attr("line").and_then(|v| v.parse::<f32>().ok()) {
                if matches!(sp.attr("lineRule"), None | Some("auto")) {
                    para.line_spacing = (l / 240.0).clamp(0.5, 5.0);
                }
            }
        }
        if para.list == ListKind::None {
            if let Some(ind) = ppr.child("ind") {
                if let Some(left) = twips(ind, "left").or(twips(ind, "start")) {
                    let quote = if style == Style::Quote { 36.0 } else { 0.0 };
                    para.level = ((left - quote) / 36.0).round().clamp(0.0, 8.0) as u8;
                }
            }
        }
        let mut end = base;
        if let Some(rpr) = ppr.child("rPr") {
            apply_rpr(rpr, &mut end);
        }
        para.end_fmt = end.without_object();
    }
    runs_in(el, &base, &mut para, cx);
    para
}

fn table(el: &El, cx: &mut Ctx, out: &mut Vec<Para>) {
    for tr in el.elems().filter(|e| e.name == "tr") {
        let mut row = Para::new(Style::Normal);
        let mut first_cell = true;
        for tc in tr.elems().filter(|e| e.name == "tc") {
            if !first_cell {
                let f = row.fmt_at(row.len());
                push(&mut row, '\t', f);
            }
            first_cell = false;
            let mut first_p = true;
            for p in tc.elems() {
                let cell_para = match p.name.as_str() {
                    "p" => paragraph(p, cx),
                    "tbl" => {
                        table(p, cx, out);
                        continue;
                    }
                    _ => continue,
                };
                if !first_p && !cell_para.chars.is_empty() {
                    let f = row.fmt_at(row.len());
                    push(&mut row, ' ', f);
                }
                first_p = false;
                row.chars.extend(cell_para.chars);
                row.fmts.extend(cell_para.fmts);
            }
        }
        out.push(row);
    }
}

fn block(el: &El, cx: &mut Ctx, out: &mut Vec<Para>, doc: &mut Document) {
    for e in el.elems() {
        match e.name.as_str() {
            "p" => out.push(paragraph(e, cx)),
            "tbl" => table(e, cx, out),
            "sdt" | "sdtContent" | "customXml" => block(e, cx, out, doc),
            "sectPr" => section(e, doc),
            _ => {}
        }
    }
}

fn section(e: &El, doc: &mut Document) {
    if let Some(sz) = e.child("pgSz") {
        if let (Some(w), Some(h)) = (twips(sz, "w"), twips(sz, "h")) {
            doc.page.width = w;
            doc.page.height = h;
        }
    }
    if let Some(m) = e.child("pgMar") {
        let g = |k| twips(m, k).map(|v| v.abs());
        doc.page.margins = [
            g("top").unwrap_or(72.0),
            g("right").unwrap_or(72.0),
            g("bottom").unwrap_or(72.0),
            g("left").unwrap_or(72.0),
        ];
    }
}

/// Text of a header/footer part, skipping field results. Returns (text, has PAGE field).
fn hf_text(el: &El) -> (String, bool) {
    fn walk(e: &El, out: &mut String, in_field: &mut bool, page: &mut bool) {
        for c in e.elems() {
            match c.name.as_str() {
                "fldSimple" => {
                    if c.attr("instr").unwrap_or("").contains("PAGE") {
                        *page = true;
                    }
                }
                "fldChar" => match c.attr("fldCharType") {
                    Some("begin") => *in_field = true,
                    Some("end") => *in_field = false,
                    _ => {}
                },
                "instrText" => {
                    if c.text().contains("PAGE") {
                        *page = true;
                    }
                }
                "t" if !*in_field => out.push_str(&c.text()),
                "p" => {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    walk(c, out, in_field, page);
                }
                _ => walk(c, out, in_field, page),
            }
        }
    }
    let mut s = String::new();
    let mut page = false;
    walk(el, &mut s, &mut false, &mut page);
    let mut t = s.trim().to_string();
    if page {
        for pat in ["Page  of", "Page of", "Page"] {
            if let Some(stripped) = t.strip_suffix(pat) {
                t = stripped.trim().to_string();
                break;
            }
        }
    }
    (t, page)
}

pub fn load_docx(path: &Path) -> Result<Document, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Not a valid .docx file: {e}"))?;
    let root = read_xml(&mut zip, "word/document.xml").ok_or("word/document.xml is missing or invalid")?;

    let mut cx = Ctx {
        styles: HashMap::new(),
        bullets: HashMap::new(),
        rels: HashMap::new(),
        media: HashMap::new(),
        images: Vec::new(),
        next_image: 1,
    };
    if let Some(styles) = read_xml(&mut zip, "word/styles.xml") {
        for s in styles.elems().filter(|e| e.name == "style") {
            if let Some(id) = s.attr("styleId") {
                let name = s.child_val("name");
                cx.styles.insert(id.to_string(), Style::from_docx(id, name));
            }
        }
    }
    if let Some(num) = read_xml(&mut zip, "word/numbering.xml") {
        let mut abstract_fmt: HashMap<String, HashMap<u8, bool>> = HashMap::new();
        for a in num.elems().filter(|e| e.name == "abstractNum") {
            let id = a.attr("abstractNumId").unwrap_or("").to_string();
            let mut lv = HashMap::new();
            for l in a.elems().filter(|e| e.name == "lvl") {
                let il: u8 = l.attr("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                lv.insert(il, l.child_val("numFmt") == Some("bullet"));
            }
            abstract_fmt.insert(id, lv);
        }
        for n in num.elems().filter(|e| e.name == "num") {
            let nid = n.attr("numId").unwrap_or("").to_string();
            let aid = n.child_val("abstractNumId").unwrap_or("").to_string();
            if let Some(lv) = abstract_fmt.get(&aid) {
                for (il, b) in lv {
                    cx.bullets.insert((nid.clone(), *il), *b);
                }
            }
        }
    }
    if let Some(rels) = read_xml(&mut zip, "word/_rels/document.xml.rels") {
        for r in rels.elems() {
            if let (Some(id), Some(t)) = (r.attr("Id"), r.attr("Target")) {
                let target = if let Some(abs) = t.strip_prefix('/') { abs.to_string() } else { format!("word/{t}") };
                cx.rels.insert(id.to_string(), target);
            }
        }
    }
    let names: Vec<String> = cx.rels.values().filter(|t| t.contains("media/")).cloned().collect();
    for n in names {
        if let Some(b) = read_entry(&mut zip, &n) {
            cx.media.insert(n, b);
        }
    }

    let mut doc = Document::new();
    let mut paras = Vec::new();
    if let Some(body) = root.child("body") {
        block(body, &mut cx, &mut paras, &mut doc);
        if let Some(sect) = body.child("sectPr") {
            for (tag, is_header) in [("headerReference", true), ("footerReference", false)] {
                let r = sect
                    .elems()
                    .filter(|e| e.name == tag)
                    .find(|e| e.attr("type") == Some("default"))
                    .and_then(|e| e.attr("id"))
                    .and_then(|id| cx.rels.get(id).cloned());
                if let Some(part) = r.and_then(|t| read_xml(&mut zip, &t)) {
                    let (text, page) = hf_text(&part);
                    if is_header {
                        doc.header = text;
                    } else {
                        doc.footer = text;
                    }
                    doc.page_numbers |= page;
                }
            }
        }
    }
    for (id, img) in cx.images {
        doc.images.insert(id, img);
    }
    if !paras.is_empty() {
        doc.paras = paras;
    }
    Ok(doc)
}
