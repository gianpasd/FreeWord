//! Microsoft Word (.docx) import and export.
//!
//! Import resolves styles, theme fonts, numbering and tables into FreeWord's
//! model. Everything FreeWord does not model is kept: unknown paragraph and
//! run properties as raw XML, unknown inline content (fields, bookmarks,
//! comments, revisions, equations, shapes...) as opaque objects, and every
//! other part of the package (styles, theme, settings, footnotes, headers...)
//! byte for byte. Export writes formatting as differences from the styles.

use crate::doc::{
    Align, CellProps, CellRef, CharFmt, COLUMN_BREAK, Document, ImageData, LINE_BREAK, Leader, LineRule,
    ListKind, OBJ, PAGE_BREAK, Package, PageSetup, Para, RowProps, Style, TabKind, TabStop, Table, VMerge, Edges,
    VAlign,
};
use crate::fonts;
use crate::io::{hex, image_from_bytes, image_kind};
use crate::numbering::Numbering;
use crate::pool::{self, ObjKind, RawObj};
use crate::styles::{self, HIGHLIGHTS, PProps, StyleSheet};
use crate::xml::{self, El, esc, esc_text};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::Arc;
use zip::write::SimpleFileOptions;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_T: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NBH: char = '\u{2011}';
const SHY: char = '\u{00AD}';

/// Schema order of `w:rPr` children.
const RPR_ORDER: &[&str] = &[
    "rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow",
    "emboss", "imprint", "noProof", "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern",
    "position", "sz", "szCs", "highlight", "u", "effect", "bdr", "shd", "fitText", "vertAlign", "rtl", "cs",
    "em", "lang", "eastAsianLayout", "specVanish", "oMath", "ins", "del", "moveFrom", "moveTo", "rPrChange",
];

/// Schema order of `w:pPr` children.
const PPR_ORDER: &[&str] = &[
    "pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl", "numPr",
    "suppressLineNumbers", "pBdr", "shd", "tabs", "suppressAutoHyphens", "kinsoku", "wordWrap",
    "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN", "bidi", "adjustRightInd", "snapToGrid",
    "spacing", "ind", "contextualSpacing", "mirrorIndents", "suppressOverlap", "jc", "textDirection",
    "textAlignment", "textboxTightWrap", "outlineLvl", "divId", "cnfStyle", "rPr", "sectPr", "pPrChange",
];

/// Schema order of `w:tcPr` children.
const TCPR_ORDER: &[&str] = &[
    "cnfStyle", "tcW", "gridSpan", "hMerge", "vMerge", "tcBorders", "shd", "noWrap", "tcMar", "textDirection",
    "tcFitText", "vAlign", "hideMark", "headers", "cellIns", "cellDel", "cellMerge", "tcPrChange",
];

const RPR_MODELED: &[&str] = &[
    "rFonts", "b", "bCs", "i", "iCs", "u", "strike", "dstrike", "caps", "smallCaps", "vanish", "color", "sz",
    "szCs", "spacing", "highlight", "shd", "vertAlign",
];
const PPR_MODELED: &[&str] = &[
    "pStyle", "numPr", "spacing", "ind", "jc", "contextualSpacing", "pageBreakBefore", "shd", "tabs", "rPr",
];

fn order_of(order: &[&str], name: &str) -> usize {
    order.iter().position(|n| *n == name).unwrap_or(order.len())
}

/// Sorts (local name, xml) pairs into schema order and concatenates them.
fn ordered(order: &[&str], mut items: Vec<(String, String)>) -> String {
    items.sort_by_key(|(n, _)| order_of(order, n));
    items.into_iter().map(|(_, x)| x).collect()
}

fn twips_attr(e: &El, key: &str) -> Option<f32> {
    e.attr(key).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 20.0)
}

fn tw(v: f32) -> i32 {
    (v * 20.0).round() as i32
}

// ================================================================ import

#[derive(Clone)]
struct Rel {
    kind: String,
    target: String,
    external: bool,
}

/// Reads relationships of `part` (e.g. `word/document.xml`).
fn read_rels(parts: &BTreeMap<String, Vec<u8>>, part: &str) -> HashMap<String, Rel> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_name = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
    let mut out = HashMap::new();
    let Some(bytes) = parts.get(&rels_name) else { return out };
    let Ok(root) = xml::parse(&String::from_utf8_lossy(bytes)) else { return out };
    for r in root.elems() {
        let (Some(id), Some(t)) = (r.attr("Id"), r.attr("Target")) else { continue };
        let external = r.attr("TargetMode") == Some("External");
        let target = if external {
            t.to_string()
        } else if let Some(abs) = t.strip_prefix('/') {
            abs.to_string()
        } else {
            normalize_path(&if dir.is_empty() { t.to_string() } else { format!("{dir}/{t}") })
        };
        let kind = r.attr("Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
        out.insert(id.to_string(), Rel { kind, target, external });
    }
    out
}

fn normalize_path(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            ".." => {
                out.pop();
            }
            "." | "" => {}
            s => out.push(s),
        }
    }
    out.join("/")
}

fn part_xml(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> Option<El> {
    let bytes = parts.get(name)?;
    let s = String::from_utf8_lossy(bytes);
    xml::parse(s.trim_start_matches('\u{feff}')).ok()
}

/// Table-style paragraph and run properties that apply inside one cell.
struct CellFmt {
    ppr: Vec<El>,
    rpr: Vec<El>,
}

struct Reader<'a> {
    parts: &'a BTreeMap<String, Vec<u8>>,
    rels: HashMap<String, Rel>,
    sheet: &'a StyleSheet,
    numbering: &'a Numbering,
    pkg: u64,
    images: BTreeMap<u32, ImageData>,
    tables: BTreeMap<u32, Table>,
    /// Inline objects found between paragraphs, attached to the next one.
    pending: Vec<(u32, CharFmt)>,
    /// Image ids are allocated above this value (keeps header images distinct).
    image_base: u32,
}

fn push(p: &mut Para, c: char, f: CharFmt) {
    p.chars.push(c);
    p.fmts.push(f);
}

impl Reader<'_> {
    fn obj(&self, xml: String, kind: ObjKind) -> u32 {
        pool::add_obj(RawObj { xml, kind, pkg: self.pkg })
    }

    fn push_obj(&self, p: &mut Para, f: &CharFmt, xml: String, kind: ObjKind) {
        let id = self.obj(xml, kind);
        let mut of = *f;
        of.obj = id;
        of.image = 0;
        push(p, OBJ, of);
    }

    /// Splits rPr children into modeled properties (applied) and raw extras.
    fn run_props(&self, rpr: &El, f: &mut CharFmt) {
        self.sheet.apply_rpr(rpr, f);
        let mut extra = String::new();
        for e in rpr.elems() {
            let fancy_u = e.name == "u" && !matches!(e.attr("val"), None | Some("single" | "none" | "0" | "false"));
            let fancy_color = e.name == "color" && e.attr("themeColor").is_some();
            if !RPR_MODELED.contains(&e.name.as_str()) || fancy_u || fancy_color {
                extra.push_str(&e.to_xml());
            }
        }
        f.rpr_extra = pool::intern(&extra);
    }

    fn paragraph(&mut self, el: &El, cell: Option<CellRef>, tbl_style: Option<&CellFmt>) -> Para {
        let ppr = el.child("pPr");
        let style_id = ppr
            .and_then(|p| p.child_val("pStyle"))
            .map(|s| s.to_string())
            .or_else(|| self.sheet.default_para.clone());
        let is_default_style = style_id == self.sheet.default_para;
        let mut pp = self.sheet.para_props(style_id.as_deref(), self.numbering);
        let mut base = self.sheet.char_fmt(style_id.as_deref(), None);
        // Table style formatting applies to paragraphs using the default style.
        if let (Some(cf), true) = (tbl_style, is_default_style) {
            for tp in &cf.ppr {
                self.sheet.apply_ppr(tp, &mut pp, self.numbering);
            }
            for tr in &cf.rpr {
                self.sheet.apply_rpr(tr, &mut base);
            }
        }
        if let Some(ppr) = ppr {
            self.sheet.apply_ppr(ppr, &mut pp, self.numbering);
        }
        let style = style_id
            .as_deref()
            .map(|id| Style::from_docx(id, self.sheet.style_name(id)))
            .unwrap_or(Style::Normal);
        let mut para = Para::new(style);
        para.style_id = style_id.clone();
        set_para_props(&mut para, &pp, self.numbering);
        para.cell = cell;
        if let Some(ppr) = ppr {
            let mut extra = String::new();
            for e in ppr.elems() {
                if !PPR_MODELED.contains(&e.name.as_str()) {
                    extra.push_str(&e.to_xml());
                }
                if e.name == "sectPr" {
                    para.sect_break = true;
                }
            }
            para.ppr_extra = extra;
            let mut end = base;
            if let Some(rpr) = ppr.child("rPr") {
                self.run_props(rpr, &mut end);
            }
            para.end_fmt = end;
        } else {
            para.end_fmt = base;
        }
        for (id, f) in std::mem::take(&mut self.pending) {
            let mut of = f;
            of.obj = id;
            push(&mut para, OBJ, of);
        }
        self.inline(el, &mut para, &base, 0);
        para
    }

    fn inline(&mut self, el: &El, p: &mut Para, base: &CharFmt, link: u32) {
        for c in el.elems() {
            match c.name.as_str() {
                "pPr" | "proofErr" | "lastRenderedPageBreak" => {}
                "r" => self.run(c, p, base, link),
                "hyperlink" => {
                    let url = c
                        .attr("id")
                        .and_then(|id| self.rels.get(id))
                        .filter(|r| r.external)
                        .map(|r| r.target.clone())
                        .unwrap_or_default();
                    let anchor = c.attr("anchor").unwrap_or("");
                    let id = pool::link_id(&url, anchor);
                    self.inline(c, p, base, if id != 0 { id } else { link });
                }
                "ins" | "moveTo" => {
                    // Keep tracked insertions as balanced start/end markers.
                    let attrs: String = c.attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", esc(v))).collect();
                    let mut f = *base;
                    f.link = link;
                    self.push_obj(p, &f, format!("<{}{attrs}>", c.qname), ObjKind::Wrap { open: true });
                    self.inline(c, p, base, link);
                    self.push_obj(p, &f, format!("</{}>", c.qname), ObjKind::Wrap { open: false });
                }
                "smartTag" | "customXml" | "dir" | "bdo" => self.inline(c, p, base, link),
                "sdt" => {
                    if let Some(content) = c.child("sdtContent") {
                        self.inline(content, p, base, link);
                    }
                }
                "fldSimple" => {
                    let instr = c.attr("instr").unwrap_or("").to_string();
                    let mut f = *base;
                    f.link = link;
                    self.push_obj(p, &f, "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>".into(), ObjKind::FieldBegin);
                    self.push_obj(
                        p,
                        &f,
                        format!("<w:r><w:instrText xml:space=\"preserve\">{}</w:instrText></w:r>", esc_text(&instr)),
                        ObjKind::Hidden,
                    );
                    self.push_obj(p, &f, "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>".into(), ObjKind::FieldSep);
                    self.inline(c, p, base, link);
                    self.push_obj(p, &f, "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>".into(), ObjKind::FieldEnd);
                }
                "oMath" | "oMathPara" => {
                    let mut ts = Vec::new();
                    c.find_all("t", &mut ts);
                    let text: String = ts.iter().map(|t| t.text()).collect();
                    let mut f = *base;
                    f.italic = true;
                    f.font = fonts::font_id(if fonts::is_available("Cambria") { "Cambria" } else { fonts::default_font() });
                    self.push_obj(p, &f, c.to_xml(), ObjKind::Label(text));
                }
                "del" | "moveFrom" => {
                    let mut f = *base;
                    f.link = link;
                    self.push_obj(p, &f, c.to_xml(), ObjKind::Deleted(c.run_text()));
                }
                _ => {
                    let mut f = *base;
                    f.link = link;
                    self.push_obj(p, &f, c.to_xml(), ObjKind::Hidden);
                }
            }
        }
    }

    fn run(&mut self, r: &El, p: &mut Para, base: &CharFmt, link: u32) {
        let rpr = r.child("rPr");
        let rstyle = rpr.and_then(|x| x.child_val("rStyle"));
        let mut f = if rstyle.is_some() { self.sheet.char_fmt(p.style_id.as_deref(), rstyle) } else { *base };
        if let Some(rpr) = rpr {
            self.run_props(rpr, &mut f);
        }
        f.link = link;
        let rpr_raw = rpr.map(|x| x.to_xml()).unwrap_or_default();
        let raw_run = |c: &El| format!("<w:r>{rpr_raw}{}</w:r>", c.to_xml());
        for c in r.elems() {
            match c.name.as_str() {
                "rPr" | "lastRenderedPageBreak" => {}
                "t" | "delText" => {
                    for ch in c.text().chars() {
                        let ch = if ch == '\n' || ch == '\r' { ' ' } else { ch };
                        push(p, ch, f);
                    }
                }
                "tab" | "ptab" => push(p, '\t', f),
                "br" => push(
                    p,
                    match c.attr("type") {
                        Some("page") => PAGE_BREAK,
                        Some("column") => COLUMN_BREAK,
                        _ => LINE_BREAK,
                    },
                    f,
                ),
                "cr" => push(p, LINE_BREAK, f),
                "noBreakHyphen" => push(p, NBH, f),
                "softHyphen" => push(p, SHY, f),
                "sym" => {
                    let code = c.attr("char").and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or(0x2022);
                    let glyph = sym_glyph(code, c.attr("font"));
                    self.push_obj(p, &f, raw_run(c), ObjKind::Label(glyph));
                }
                "drawing" | "pict" | "object" | "AlternateContent" => self.drawing(c, raw_run(c), &f, p),
                "fldChar" => {
                    let kind = match c.attr("fldCharType") {
                        Some("begin") => ObjKind::FieldBegin,
                        Some("separate") => ObjKind::FieldSep,
                        _ => ObjKind::FieldEnd,
                    };
                    self.push_obj(p, &f, raw_run(c), kind);
                }
                "footnoteReference" | "endnoteReference" => {
                    let kind = ObjKind::NoteRef {
                        endnote: c.name == "endnoteReference",
                        id: c.attr("id").unwrap_or("").to_string(),
                    };
                    self.push_obj(p, &f, raw_run(c), kind);
                }
                // The note's own number at the start of its text.
                "footnoteRef" | "endnoteRef" => {
                    let kind = ObjKind::NoteRef { endnote: c.name == "endnoteRef", id: "self".into() };
                    self.push_obj(p, &f, raw_run(c), kind);
                }
                _ => self.push_obj(p, &f, raw_run(c), ObjKind::Hidden),
            }
        }
    }

    fn drawing(&mut self, c: &El, raw: String, f: &CharFmt, p: &mut Para) {
        let mut blips = Vec::new();
        c.find_all("blip", &mut blips);
        let mut embeds: Vec<&str> = blips.iter().filter_map(|b| b.attr("embed")).collect();
        let mut vml = Vec::new();
        c.find_all("imagedata", &mut vml);
        embeds.extend(vml.iter().filter_map(|v| v.attr("id")));
        embeds.sort();
        embeds.dedup();
        let other_refs = raw.contains(" r:id=\"") && vml.is_empty() || raw.contains(" r:link=") || raw.contains("c:chart") || raw.contains("dgm:");
        // Size from wp:extent or VML style.
        let mut size = c.find("extent").and_then(|e| {
            let cx = e.attr("cx")?.parse::<f32>().ok()?;
            let cy = e.attr("cy")?.parse::<f32>().ok()?;
            Some((cx / 12700.0, cy / 12700.0))
        });
        if size.is_none() {
            if let Some(shape) = c.find("shape").or_else(|| c.find("rect")) {
                size = vml_size(shape.attr("style").unwrap_or(""));
            }
        }
        if embeds.len() == 1 && !other_refs && !raw.contains("txbx") {
            if let Some(bytes) = self.rels.get(embeds[0]).and_then(|r| self.parts.get(&r.target)) {
                if let Ok(mut img) = image_from_bytes(bytes.clone(), f32::MAX) {
                    if let Some((w, h)) = size {
                        if w > 0.5 && h > 0.5 {
                            img.w = w;
                            img.h = h;
                        }
                    }
                    img.anchor = parse_anchor(c);
                    img.raw = Some((raw, self.pkg));
                    let id = self.images.keys().next_back().copied().unwrap_or(self.image_base).max(self.image_base) + 1;
                    self.images.insert(id, img);
                    let mut imf = *f;
                    imf.image = id;
                    imf.obj = 0;
                    push(p, OBJ, imf);
                    return;
                }
            }
        }
        let label = if raw.contains("c:chart") || raw.contains("/chart\"") {
            "Chart".to_string()
        } else if raw.contains("txbx") {
            let mut paras = Vec::new();
            c.find_all("txbxContent", &mut paras);
            let t = paras.first().map(|t| t.run_text()).unwrap_or_default();
            let t: String = t.chars().take(80).collect();
            if t.trim().is_empty() { "Text box".into() } else { t }
        } else if raw.contains("dgm:") {
            "SmartArt".into()
        } else if c.name == "object" || raw.contains("OLEObject") {
            "Embedded object".into()
        } else {
            "Shape".into()
        };
        let (w, h) = size.unwrap_or((72.0, 36.0));
        let anchor = parse_anchor(c);
        // Zero-size objects take no space.
        let kind = if w < 1.0 || h < 1.0 {
            ObjKind::Hidden
        } else if anchor.is_some() {
            ObjKind::Box { label, w, h, anchor }
        } else {
            ObjKind::Box { label, w: w.min(468.0), h: h.min(600.0), anchor: None }
        };
        self.push_obj(p, f, raw, kind);
    }

    fn table(&mut self, el: &El, out: &mut Vec<Para>, outer: Option<CellRef>) {
        if let Some(cell) = outer {
            // Nested tables are kept verbatim inside the outer cell.
            let mut p = Para::new(Style::Normal);
            p.cell = Some(cell);
            p.raw_block = Some(el.to_xml());
            p.nested = self.nested_doc(el).map(Arc::new);
            let label = format!("[Nested table: {}]", el.run_text().chars().take(60).collect::<String>());
            let mut f = self.sheet.char_fmt(None, None);
            f.color = [0x80, 0x80, 0x80, 255];
            f.italic = true;
            for c in label.chars() {
                push(&mut p, c, f);
            }
            p.end_fmt = f;
            out.push(p);
            return;
        }
        let id = self.tables.keys().next_back().copied().unwrap_or(0) + 1;
        let tblpr = el.child("tblPr");
        let tbl_style = tblpr.and_then(|t| t.child_val("tblStyle")).map(|s| s.to_string());
        let ts = self.sheet.table_style(tbl_style.as_deref());
        let mut t = Table { tblpr_raw: tblpr.map(|x| x.to_xml()), ..Default::default() };
        // Borders: style, then direct w:tblBorders.
        let mut edges = ts.edges;
        if let Some(b) = tblpr.and_then(|x| x.child("tblBorders")) {
            edges.merge(&styles::parse_edges(b));
        }
        t.borders = edges.any_visible();
        t.edges = Some(edges);
        let mut margins = ts.cell_margins.unwrap_or([0.0, 5.4, 0.0, 5.4]);
        if let Some(m) = tblpr.and_then(|x| x.child("tblCellMar")) {
            styles::parse_margins(m, &mut margins);
        }
        t.cell_margins = margins;
        if let Some(ind) = tblpr.and_then(|x| x.child("tblInd")) {
            if ind.attr("type").is_none_or(|ty| ty == "dxa") {
                t.indent = twips_attr(ind, "w").unwrap_or(0.0);
            }
        }
        t.align = match tblpr.and_then(|x| x.child_val("jc")) {
            Some("center") => Some(Align::Center),
            Some("right" | "end") => Some(Align::Right),
            _ => None,
        };
        if let Some(grid) = el.child("tblGrid") {
            t.grid = grid.elems().filter(|g| g.name == "gridCol").map(|g| twips_attr(g, "w").unwrap_or(72.0)).collect();
        }
        // Which conditional formats are enabled (w:tblLook).
        let look = tblpr.and_then(|x| x.child("tblLook"));
        let flag = |name: &str, bit: u32, default: bool| -> bool {
            let Some(l) = look else { return default };
            if let Some(v) = l.attr(name) {
                return v == "1" || v == "true";
            }
            match l.attr("val").and_then(|v| u32::from_str_radix(v, 16).ok()) {
                Some(bits) => bits & bit != 0,
                None => default,
            }
        };
        let first_row = flag("firstRow", 0x0020, true);
        let last_row = flag("lastRow", 0x0040, false);
        let first_col = flag("firstColumn", 0x0080, true);
        let last_col = flag("lastColumn", 0x0100, false);
        let h_band = !flag("noHBand", 0x0200, false);
        let v_band = !flag("noVBand", 0x0400, true);

        let mut rows: Vec<&El> = Vec::new();
        collect_named(el, "tr", &["sdt", "sdtContent", "customXml"], &mut rows);
        let nrows = rows.len() as u32;
        let ncols = t.grid.len().max(1) as u32;
        let mut max_col = 0u32;
        for (ri, tr) in rows.iter().enumerate() {
            let ri = ri as u32;
            let trpr = tr.child("trPr");
            let mut rp = RowProps { trpr_raw: trpr.map(|x| x.to_xml()), ..Default::default() };
            let mut col = 0u32;
            if let Some(trpr) = trpr {
                if let Some(h) = trpr.child("trHeight") {
                    rp.height = twips_attr(h, "val").unwrap_or(0.0);
                    rp.exact = h.attr("hRule") == Some("exact");
                }
                rp.header = trpr.child("tblHeader").is_some_and(styles::on);
                col = trpr.child_val("gridBefore").and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            t.rows.insert(ri, rp);
            let mut cells: Vec<&El> = Vec::new();
            collect_named(tr, "tc", &["sdt", "sdtContent", "customXml"], &mut cells);
            for tc in cells {
                let tcpr = tc.child("tcPr");
                let span: u32 = tcpr.and_then(|x| x.child_val("gridSpan")).and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
                let vmerge = match tcpr.and_then(|x| x.child("vMerge")) {
                    Some(v) if v.attr("val") == Some("restart") => VMerge::Restart,
                    Some(_) => VMerge::Continue,
                    None => VMerge::None,
                };
                // Conditional regions this cell belongs to, lowest priority first.
                let body_row = ri.saturating_sub(first_row as u32);
                let body_col = col.saturating_sub(first_col as u32);
                let in_first_row = first_row && ri == 0;
                let in_last_row = last_row && ri + 1 == nrows;
                let in_first_col = first_col && col == 0;
                let in_last_col = last_col && col + span >= ncols;
                let mut regions: Vec<&str> = vec!["wholeTable"];
                if v_band && !in_first_col && !in_last_col {
                    regions.push(if (body_col / ts.col_band) % 2 == 0 { "band1Vert" } else { "band2Vert" });
                }
                if h_band && !in_first_row && !in_last_row {
                    regions.push(if (body_row / ts.row_band) % 2 == 0 { "band1Horz" } else { "band2Horz" });
                }
                for (on, name) in [
                    (in_first_col, "firstCol"),
                    (in_last_col, "lastCol"),
                    (in_first_row, "firstRow"),
                    (in_last_row, "lastRow"),
                    (in_first_row && in_first_col, "nwCell"),
                    (in_first_row && in_last_col, "neCell"),
                    (in_last_row && in_first_col, "swCell"),
                    (in_last_row && in_last_col, "seCell"),
                ] {
                    if on {
                        regions.push(name);
                    }
                }
                // Cell borders: table edges by position, then style regions, then w:tcBorders.
                let mut ce = Edges {
                    top: if ri == 0 { edges.top } else { edges.inside_h },
                    bottom: if ri + 1 == nrows { edges.bottom } else { edges.inside_h },
                    left: if col == 0 { edges.left } else { edges.inside_v },
                    right: if col + span >= ncols { edges.right } else { edges.inside_v },
                    ..Default::default()
                };
                let mut style_shade = ts.shade;
                let mut cell_fmt = CellFmt { ppr: ts.ppr.clone(), rpr: ts.rpr.clone() };
                for region in &regions {
                    for (_, cond) in ts.cond.iter().filter(|(ty, _)| ty == region) {
                        if let Some(tcp) = cond.child("tcPr") {
                            if let Some(b) = tcp.child("tcBorders") {
                                let be = styles::parse_edges(b);
                                ce.merge(&Edges { inside_h: None, inside_v: None, ..be });
                            }
                            if let Some(s) = tcp.child("shd") {
                                style_shade = styles::shd_fill(s);
                            }
                        }
                        if let Some(p) = cond.child("pPr") {
                            cell_fmt.ppr.push(p.clone());
                        }
                        if let Some(rr) = cond.child("rPr") {
                            cell_fmt.rpr.push(rr.clone());
                        }
                    }
                }
                if let Some(b) = tcpr.and_then(|x| x.child("tcBorders")) {
                    ce.merge(&styles::parse_edges(b));
                }
                let mut cell_margins = None;
                if let Some(m) = tcpr.and_then(|x| x.child("tcMar")) {
                    let mut mm = margins;
                    styles::parse_margins(m, &mut mm);
                    cell_margins = Some(mm);
                }
                let valign = match tcpr.and_then(|x| x.child_val("vAlign")) {
                    Some("center") => VAlign::Center,
                    Some("bottom") => VAlign::Bottom,
                    _ => VAlign::Top,
                };
                let shade = tcpr.and_then(|x| x.child("shd")).and_then(styles::shd_fill);
                t.cells.insert(
                    (ri, col),
                    CellProps {
                        span,
                        vmerge,
                        shade,
                        tcpr_raw: tcpr.map(|x| x.to_xml()),
                        edges: [ce.top, ce.right, ce.bottom, ce.left],
                        style_shade,
                        margins: cell_margins,
                        valign,
                    },
                );
                let cref = CellRef { table: id, row: ri, col };
                let before = out.len();
                self.block_children(tc, out, Some(cref), Some(&cell_fmt));
                if out.len() == before {
                    let mut p = Para::new(Style::Normal);
                    p.cell = Some(cref);
                    out.push(p);
                }
                col += span;
            }
            max_col = max_col.max(col);
        }
        // Make sure the grid covers every column.
        while (t.grid.len() as u32) < max_col {
            t.grid.push(72.0);
        }
        if t.grid.is_empty() {
            t.grid.push(72.0);
        }
        self.tables.insert(id, t);
    }

    /// A nested table as its own document (rendered inside the outer cell).
    fn nested_doc(&self, el: &El) -> Option<Document> {
        let base = 200_000_000 + self.images.len() as u32 * 1000 + (el.to_xml().len() as u32 % 997) * 10_000;
        let mut r = Reader {
            parts: self.parts,
            rels: self.rels.clone(),
            sheet: self.sheet,
            numbering: self.numbering,
            pkg: self.pkg,
            images: BTreeMap::new(),
            tables: BTreeMap::new(),
            pending: Vec::new(),
            image_base: base,
        };
        let mut paras = Vec::new();
        r.table(el, &mut paras, None);
        if paras.is_empty() {
            return None;
        }
        let mut d = Document::new();
        d.paras = paras;
        d.images = std::mem::take(&mut r.images);
        d.tables = std::mem::take(&mut r.tables);
        d.numbering = self.numbering.clone();
        d.styles = Arc::new(self.sheet.clone());
        d.ensure_trailing_para();
        // No trailing empty paragraph inside a cell.
        if d.paras.last().is_some_and(|p| p.cell.is_none() && p.chars.is_empty()) && d.paras.len() > 1 {
            d.paras.pop();
        }
        Some(d)
    }

    fn block_children(&mut self, el: &El, out: &mut Vec<Para>, cell: Option<CellRef>, tbl_style: Option<&CellFmt>) {
        for c in el.elems() {
            match c.name.as_str() {
                "p" => {
                    let p = self.paragraph(c, cell, tbl_style);
                    out.push(p);
                }
                "tbl" => self.table(c, out, cell),
                "sdt" => {
                    if let Some(content) = c.child("sdtContent") {
                        self.block_children(content, out, cell, tbl_style);
                    }
                }
                "customXml" => self.block_children(c, out, cell, tbl_style),
                "tcPr" | "sectPr" => {}
                "bookmarkStart" | "bookmarkEnd" | "commentRangeStart" | "commentRangeEnd" | "permStart"
                | "permEnd" | "moveFromRangeStart" | "moveFromRangeEnd" | "moveToRangeStart" | "moveToRangeEnd"
                | "proofErr" => {
                    let id = self.obj(c.to_xml(), ObjKind::Hidden);
                    let f = self.sheet.char_fmt(None, None);
                    self.pending.push((id, f));
                }
                _ => {
                    let mut p = Para::new(Style::Normal);
                    p.cell = cell;
                    p.raw_block = Some(c.to_xml());
                    let mut f = self.sheet.char_fmt(None, None);
                    f.color = [0x80, 0x80, 0x80, 255];
                    for ch in format!("[{}]", c.name).chars() {
                        push(&mut p, ch, f);
                    }
                    out.push(p);
                }
            }
        }
    }
}

fn collect_named<'a>(el: &'a El, name: &str, transparent: &[&str], out: &mut Vec<&'a El>) {
    for c in el.elems() {
        if c.name == name {
            out.push(c);
        } else if transparent.contains(&c.name.as_str()) {
            collect_named(c, name, transparent, out);
        }
    }
}

/// Placement of a DrawingML `wp:anchor` (None for inline objects).
pub(crate) fn parse_anchor(c: &El) -> Option<crate::doc::Anchor> {
    use crate::doc::{Anchor, HRel, PosSpec, VRel, Wrap};
    let a = c.find("anchor")?;
    let emu = |e: &El| e.child("posOffset").and_then(|o| o.text().trim().parse::<f32>().ok()).map(|v| v / 12700.0);
    let align = |e: &El| -> Option<PosSpec> {
        Some(match e.child("align")?.text().trim() {
            "left" | "top" | "inside" => PosSpec::Start,
            "center" => PosSpec::Center,
            _ => PosSpec::End,
        })
    };
    let (mut h_rel, mut h_pos) = (HRel::Column, PosSpec::Offset(0.0));
    if let Some(h) = a.child("positionH") {
        h_rel = match h.attr("relativeFrom") {
            Some("page" | "leftMargin" | "outsideMargin") => HRel::Page,
            Some("margin" | "insideMargin" | "rightMargin") => HRel::Margin,
            Some("character") => HRel::Character,
            _ => HRel::Column,
        };
        h_pos = align(h).or(emu(h).map(PosSpec::Offset)).unwrap_or(PosSpec::Offset(0.0));
    }
    let (mut v_rel, mut v_pos) = (VRel::Paragraph, PosSpec::Offset(0.0));
    if let Some(v) = a.child("positionV") {
        v_rel = match v.attr("relativeFrom") {
            Some("page" | "topMargin") => VRel::Page,
            Some("margin" | "bottomMargin" | "insideMargin" | "outsideMargin") => VRel::Margin,
            Some("line") => VRel::Line,
            _ => VRel::Paragraph,
        };
        v_pos = align(v).or(emu(v).map(PosSpec::Offset)).unwrap_or(PosSpec::Offset(0.0));
    }
    let wrap = if a.child("wrapNone").is_some() { Wrap::None } else { Wrap::Clear };
    let behind = matches!(a.attr("behindDoc"), Some("1" | "true"));
    Some(Anchor { h_rel, h_pos, v_rel, v_pos, wrap: if behind { Wrap::None } else { wrap }, behind })
}

fn vml_size(style: &str) -> Option<(f32, f32)> {
    let mut w = None;
    let mut h = None;
    for part in style.split(';') {
        let (k, v) = part.split_once(':')?;
        let v = v.trim();
        let num = |s: &str| -> Option<f32> {
            if let Some(x) = s.strip_suffix("pt") {
                x.parse().ok()
            } else if let Some(x) = s.strip_suffix("in") {
                x.parse::<f32>().ok().map(|v| v * 72.0)
            } else if let Some(x) = s.strip_suffix("px") {
                x.parse::<f32>().ok().map(|v| v * 0.75)
            } else {
                s.parse().ok()
            }
        };
        match k.trim() {
            "width" => w = num(v),
            "height" => h = num(v),
            _ => {}
        }
    }
    Some((w?, h?))
}

fn sym_glyph(code: u32, font: Option<&str>) -> String {
    let low = if (0xF000..=0xF0FF).contains(&code) { code - 0xF000 } else { code };
    let f = font.unwrap_or("").to_ascii_lowercase();
    let c = if f.contains("wingdings") {
        match low {
            0xFC => '✓',
            0xFB => '✗',
            0xA7 => '▪',
            0xD8 => '➢',
            0x6C => '●',
            0x6E => '■',
            0x71 => '❑',
            0xA8 => '◆',
            0x4A => '☺',
            0xE0 => '→',
            _ => '•',
        }
    } else if f.contains("symbol") {
        match low {
            0xB7 => '•',
            0x61 => 'α',
            0x62 => 'β',
            0x70 => 'π',
            0xA5 => '∞',
            0xB1 => '±',
            0xB4 => '×',
            0xB8 => '÷',
            0xAE => '→',
            0xD3 | 0xE3 => '©',
            _ => char::from_u32(low).unwrap_or('•'),
        }
    } else {
        char::from_u32(code).unwrap_or('•')
    };
    c.to_string()
}

/// Copies resolved paragraph properties into a paragraph.
fn set_para_props(p: &mut Para, pp: &PProps, numbering: &Numbering) {
    p.align = pp.align;
    p.space_before = pp.before;
    p.space_after = pp.after;
    p.line_spacing = pp.line;
    p.line_rule = pp.line_rule;
    p.contextual = pp.contextual;
    p.ind_left = pp.ind_left;
    p.ind_right = pp.ind_right;
    p.ind_first = pp.ind_first;
    p.tabs = pp.tabs.clone();
    p.shading = pp.shading;
    p.page_break_before = pp.page_break_before;
    p.borders = pp.borders;
    p.keep_next = pp.keep_next;
    p.keep_lines = pp.keep_lines;
    p.widow = pp.widow;
    p.num_id = 0;
    p.list = ListKind::None;
    p.level = 0;
    if let Some((n, l)) = pp.num {
        if let Some(level) = numbering.level(n, l) {
            if level.fmt != crate::numbering::NumFmt::None || !level.text.is_empty() {
                p.num_id = n;
                p.level = l;
                p.list = if numbering.is_bullet(n, l) { ListKind::Bullet } else { ListKind::Number };
            }
        }
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
                "tab" => out.push(' '),
                "p" => {
                    if !out.trim().is_empty() {
                        out.push(' ');
                    }
                    walk(c, out, in_field, page);
                }
                "drawing" | "pict" | "AlternateContent" | "sdtPr" => {}
                _ => walk(c, out, in_field, page),
            }
        }
    }
    let mut s = String::new();
    let mut page = false;
    walk(el, &mut s, &mut false, &mut page);
    let mut t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if page {
        for pat in ["Page of", "Page"] {
            if let Some(stripped) = t.strip_suffix(pat) {
                t = stripped.trim().to_string();
                break;
            }
        }
    }
    (t, page)
}

/// Parses one header/footer part into a laid-out-able sub-document.
fn hf_document(
    parts: &BTreeMap<String, Vec<u8>>,
    target: &str,
    pkg_id: u64,
    sheet: &Arc<StyleSheet>,
    numbering: &Numbering,
    image_base: u32,
) -> Option<Document> {
    let root = part_xml(parts, target)?;
    let mut reader = Reader {
        parts,
        rels: read_rels(parts, target),
        sheet,
        numbering,
        pkg: pkg_id,
        images: BTreeMap::new(),
        tables: BTreeMap::new(),
        pending: Vec::new(),
        image_base,
    };
    let mut paras = Vec::new();
    reader.block_children(&root, &mut paras, None, None);
    // Header tables get ids far away from body tables.
    let tables: BTreeMap<u32, Table> = std::mem::take(&mut reader.tables).into_iter().map(|(k, v)| (k + image_base, v)).collect();
    for p in &mut paras {
        if let Some(c) = &mut p.cell {
            c.table += image_base;
        }
    }
    if paras.is_empty() {
        return None;
    }
    let mut d = Document::new();
    d.paras = paras;
    d.images = std::mem::take(&mut reader.images);
    d.tables = tables;
    d.numbering = numbering.clone();
    d.styles = sheet.clone();
    d.source = Some(target.to_string());
    Some(d)
}

/// A new rich header (or footer) for `doc` holding its plain-text header
/// (or footer and page numbers), or one empty paragraph.
pub fn hf_from_plain(doc: &Document, header: bool) -> Document {
    let (tag, xml) = if header {
        ("hdr", if doc.header.is_empty() { None } else { Some(hf_xml("hdr", &doc.header, false)) })
    } else {
        ("ftr", if doc.footer.is_empty() && !doc.page_numbers { None } else { Some(hf_xml("ftr", &doc.footer, doc.page_numbers)) })
    };
    let xml = xml.unwrap_or_else(|| {
        let style = if header { "Header" } else { "Footer" };
        let ppr = if doc.styles.styles.contains_key(style) { format!("<w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>") } else { String::new() };
        format!("<w:{tag} xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\"><w:p>{ppr}</w:p></w:{tag}>")
    });
    hf_from_xml(doc, header, &xml)
}

/// A header (or footer) created in FreeWord, read back from its part XML.
pub fn hf_from_xml(doc: &Document, header: bool, xml: &str) -> Document {
    let target = format!("word/{}_fw.xml", if header { "header" } else { "footer" });
    let parts = BTreeMap::from([(target.clone(), xml.as_bytes().to_vec())]);
    let mut d = hf_document(&parts, &target, doc.pkg_id(), &doc.styles, &doc.numbering, 990_000_000).unwrap_or_else(Document::new);
    d.source = None;
    d.edited = true;
    d
}

/// Part XML of the headers (true) / footers (false) created in FreeWord.
pub fn new_hf_xml(doc: &Document) -> Vec<(bool, String)> {
    let Some(h) = doc.hf.last() else { return Vec::new() };
    [(true, &h.header), (false, &h.footer)]
        .into_iter()
        .filter_map(|(header, d)| {
            let d = d.as_deref().filter(|d| d.source.is_none())?;
            Some((header, hf_part_xml(d, doc.pkg_id(), if header { "hdr" } else { "ftr" }, &[], HashSet::new()).0))
        })
        .collect()
}

/// Comments (`comments.xml`) by id.
pub fn comments_from_parts(parts: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, crate::doc::Comment> {
    let mut out = BTreeMap::new();
    let main = read_rels(parts, "")
        .values()
        .find(|r| r.kind == "officeDocument")
        .map(|r| r.target.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let rels = read_rels(parts, &main);
    let Some(target) = rels.values().find(|r| r.kind == "comments").map(|r| r.target.clone()) else { return out };
    let Some(root) = part_xml(parts, &target) else { return out };
    for c in root.elems().filter(|e| e.name == "comment") {
        let Some(id) = c.attr("id") else { continue };
        let text: Vec<String> = c.elems().filter(|e| e.name == "p").map(|p| p.run_text()).collect();
        out.insert(
            id.to_string(),
            crate::doc::Comment {
                author: c.attr("author").unwrap_or("").to_string(),
                initials: c.attr("initials").unwrap_or("").to_string(),
                date: c.attr("date").unwrap_or("").to_string(),
                text: text.join("\n").trim().to_string(),
            },
        );
    }
    out
}

/// Footnote and endnote texts (`footnotes.xml`, `endnotes.xml`) by (is endnote, id).
pub fn notes_from_parts(
    parts: &BTreeMap<String, Vec<u8>>,
    pkg_id: u64,
    sheet: &Arc<StyleSheet>,
    numbering: &Numbering,
) -> BTreeMap<(bool, String), Document> {
    let mut out = BTreeMap::new();
    let main = read_rels(parts, "")
        .values()
        .find(|r| r.kind == "officeDocument")
        .map(|r| r.target.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let rels = read_rels(parts, &main);
    let mut base = 50_000_000;
    for (kind, tag, endnote) in [("footnotes", "footnote", false), ("endnotes", "endnote", true)] {
        let Some(target) = rels.values().find(|r| r.kind == kind).map(|r| r.target.clone()) else { continue };
        let Some(root) = part_xml(parts, &target) else { continue };
        for note in root.elems().filter(|e| e.name == tag) {
            // Separators are not notes.
            if note.attr("type").is_some_and(|t| t != "normal") {
                continue;
            }
            let Some(id) = note.attr("id") else { continue };
            let mut reader = Reader {
                parts,
                rels: read_rels(parts, &target),
                sheet,
                numbering,
                pkg: pkg_id,
                images: BTreeMap::new(),
                tables: BTreeMap::new(),
                pending: Vec::new(),
                image_base: base,
            };
            let mut paras = Vec::new();
            reader.block_children(note, &mut paras, None, None);
            if paras.is_empty() {
                continue;
            }
            let tables: BTreeMap<u32, Table> = std::mem::take(&mut reader.tables).into_iter().map(|(k, v)| (k + base, v)).collect();
            for p in &mut paras {
                if let Some(c) = &mut p.cell {
                    c.table += base;
                }
            }
            let mut d = Document::new();
            d.paras = paras;
            d.images = std::mem::take(&mut reader.images);
            d.tables = tables;
            d.numbering = numbering.clone();
            d.styles = sheet.clone();
            out.insert((endnote, id.to_string()), d);
            base += 10_000;
        }
    }
    out
}

/// Rich headers and footers for each section. References a section does not
/// define are inherited from the previous section, as in Word.
pub fn hf_sections(
    parts: &BTreeMap<String, Vec<u8>>,
    sects: &[String],
    pkg_id: u64,
    sheet: &Arc<StyleSheet>,
    numbering: &Numbering,
) -> Vec<crate::doc::HeaderFooter> {
    let main = read_rels(parts, "")
        .values()
        .find(|r| r.kind == "officeDocument")
        .map(|r| r.target.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let rels = read_rels(parts, &main);
    let mut cache: HashMap<String, Option<Box<Document>>> = HashMap::new();
    let mut base = 1_000_000;
    // (header?, type) -> part target, carried forward across sections.
    let mut current: HashMap<(bool, String), String> = HashMap::new();
    let mut out = Vec::new();
    for sx in sects {
        let Ok(sect) = xml::parse(sx) else {
            out.push(crate::doc::HeaderFooter::default());
            continue;
        };
        for e in sect.elems() {
            let is_header = match e.name.as_str() {
                "headerReference" => true,
                "footerReference" => false,
                _ => continue,
            };
            if let Some(target) = e.attr("id").and_then(|id| rels.get(id)).map(|r| r.target.clone()) {
                current.insert((is_header, e.attr("type").unwrap_or("default").to_string()), target);
            }
        }
        let mut hf = crate::doc::HeaderFooter { title_page: sect.child("titlePg").is_some_and(styles::on), ..Default::default() };
        let mut get = |header: bool, ty: &str, cache: &mut HashMap<String, Option<Box<Document>>>| -> Option<Box<Document>> {
            let target = current.get(&(header, ty.to_string()))?.clone();
            cache
                .entry(target.clone())
                .or_insert_with(|| {
                    base += 1_000_000;
                    hf_document(parts, &target, pkg_id, sheet, numbering, base).map(Box::new)
                })
                .clone()
        };
        hf.header = get(true, "default", &mut cache);
        hf.header_first = get(true, "first", &mut cache);
        hf.footer = get(false, "default", &mut cache);
        hf.footer_first = get(false, "first", &mut cache);
        out.push(hf);
    }
    out
}

/// Style sheet of a stored .docx package.
pub fn sheet_from_parts(parts: &BTreeMap<String, Vec<u8>>) -> StyleSheet {
    let main = read_rels(parts, "")
        .values()
        .find(|r| r.kind == "officeDocument")
        .map(|r| r.target.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let rels = read_rels(parts, &main);
    let part_of = |kind: &str| rels.values().find(|r| r.kind == kind).map(|r| r.target.clone());
    let styles_el = part_of("styles").and_then(|t| part_xml(parts, &t));
    let theme_el = part_of("theme").and_then(|t| part_xml(parts, &t));
    StyleSheet::parse(styles_el.as_ref(), theme_el.as_ref())
}

pub fn load_docx(path: &Path) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    load_docx_bytes(&bytes)
}

pub fn load_docx_bytes(bytes: &[u8]) -> Result<Document, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Not a valid .docx file: {e}"))?;
    let mut parts = BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let mut v = Vec::new();
        f.read_to_end(&mut v).map_err(|e| e.to_string())?;
        let name = f.name().map_err(|e| e.to_string())?.to_string();
        parts.insert(name, v);
    }

    let pkg_rels = read_rels(&parts, "");
    let main = pkg_rels
        .values()
        .find(|r| r.kind == "officeDocument")
        .map(|r| r.target.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let root = part_xml(&parts, &main).ok_or("The main document part is missing or invalid")?;
    let rels = read_rels(&parts, &main);
    let part_of = |kind: &str| rels.values().find(|r| r.kind == kind).map(|r| r.target.clone());

    let styles_el = part_of("styles").and_then(|t| part_xml(&parts, &t));
    let theme_el = part_of("theme").and_then(|t| part_xml(&parts, &t));
    let sheet = StyleSheet::parse(styles_el.as_ref(), theme_el.as_ref());
    let mut numbering = part_of("numbering").and_then(|t| part_xml(&parts, &t)).map(|n| Numbering::parse(&n)).unwrap_or_default();
    // Numbering styles: abstractNum with numStyleLink points at a style whose pPr has the numId.
    let mut style_nums = HashMap::new();
    for (id, def) in &sheet.styles {
        if let Some(n) = def.ppr.as_ref().and_then(|p| p.child("numPr")).and_then(|n| n.child_val("numId")).and_then(|v| v.parse().ok()) {
            style_nums.insert(id.clone(), n);
        }
    }
    numbering.resolve_style_links(&style_nums);

    let mut doc = Document::new();
    let pkg_id = pool::new_pkg_id();
    if let Some(settings) = part_of("settings").and_then(|t| part_xml(&parts, &t)) {
        if let Some(v) = settings.child("defaultTabStop").and_then(|e| twips_attr(e, "val")) {
            if v > 1.0 {
                doc.page.default_tab = v;
            }
        }
    }

    let mut reader = Reader {
        parts: &parts,
        rels,
        sheet: &sheet,
        numbering: &numbering,
        pkg: pkg_id,
        images: BTreeMap::new(),
        tables: BTreeMap::new(),
        pending: Vec::new(),
        image_base: 0,
    };
    let mut paras = Vec::new();
    let body = root.child("body");
    if let Some(body) = body {
        reader.block_children(body, &mut paras, None, None);
    }
    // Inline objects after the last paragraph go into a final paragraph.
    if !reader.pending.is_empty() {
        let mut p = Para::new(Style::Normal);
        p.style_id = sheet.default_para.clone();
        for (id, f) in std::mem::take(&mut reader.pending) {
            let mut of = f;
            of.obj = id;
            push(&mut p, OBJ, of);
        }
        paras.push(p);
    }
    let images = std::mem::take(&mut reader.images);
    let tables = std::mem::take(&mut reader.tables);
    let rels = std::mem::take(&mut reader.rels);

    let mut pkg = Package {
        id: pkg_id,
        root_attrs: root.attrs.clone(),
        background: root.child("background").map(|b| b.to_xml()),
        ..Default::default()
    };
    if let Some(sect) = body.and_then(|b| b.child("sectPr")) {
        read_section(sect, &mut doc.page);
        pkg.body_sect = Some(sect.to_xml());
        for (tag, is_header) in [("headerReference", true), ("footerReference", false)] {
            let part = sect
                .elems()
                .filter(|e| e.name == tag)
                .find(|e| e.attr("type") == Some("default"))
                .and_then(|e| e.attr("id"))
                .and_then(|id| rels.get(id))
                .and_then(|r| part_xml(&parts, &r.target));
            if let Some(part) = part {
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
    pkg.header_text = doc.header.clone();
    pkg.footer_text = doc.footer.clone();
    pkg.page_numbers = doc.page_numbers;
    pkg.parts = parts;

    doc.images = images;
    doc.tables = tables;
    doc.numbering = numbering;
    doc.styles = Arc::new(sheet);
    let mut sects: Vec<String> = paras.iter().filter(|p| p.sect_break).filter_map(|p| crate::doc::extract_sect_pr(&p.ppr_extra)).collect();
    if let Some(b) = &pkg.body_sect {
        sects.push(b.clone());
    }
    doc.hf = Arc::new(hf_sections(&pkg.parts, &sects, pkg_id, &doc.styles, &doc.numbering));
    doc.notes = Arc::new(notes_from_parts(&pkg.parts, pkg_id, &doc.styles, &doc.numbering));
    doc.comments = Arc::new(comments_from_parts(&pkg.parts));
    doc.pkg = Some(Arc::new(pkg));
    if !paras.is_empty() {
        doc.paras = paras;
    } else {
        doc.paras[0].style_id = doc.styles.default_para.clone();
    }
    doc.ensure_trailing_para();
    Ok(doc)
}

fn read_section(e: &El, page: &mut PageSetup) {
    if let Some(sz) = e.child("pgSz") {
        if let (Some(w), Some(h)) = (twips_attr(sz, "w"), twips_attr(sz, "h")) {
            page.width = w.max(72.0);
            page.height = h.max(72.0);
        }
    }
    if let Some(m) = e.child("pgMar") {
        let g = |k| twips_attr(m, k).map(|v| v.abs());
        page.margins = [
            g("top").unwrap_or(72.0),
            g("right").unwrap_or(72.0),
            g("bottom").unwrap_or(72.0),
            g("left").unwrap_or(72.0),
        ];
        page.header_dist = g("header").unwrap_or(36.0);
        page.footer_dist = g("footer").unwrap_or(36.0);
    }
}

// ================================================================ export

/// styles.xml for documents created in FreeWord.
pub fn default_styles_xml() -> String {
    let font = esc(fonts::default_font());
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"{W_NS}\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:eastAsia=\"{font}\" w:cs=\"{font}\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"en-US\" w:eastAsia=\"en-US\" w:bidi=\"ar-SA\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>"
    );
    for st in Style::ALL {
        s.push_str(&builtin_style_xml(st));
    }
    s.push_str(
        "<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>\
<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/><w:uiPriority w:val=\"39\"/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tblBorders></w:tblPr></w:style>\
<w:style w:type=\"numbering\" w:default=\"1\" w:styleId=\"NoList\"><w:name w:val=\"No List\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>",
    );
    s.push_str("</w:styles>");
    s
}

fn builtin_style_xml(st: Style) -> String {
    let f = st.char_fmt();
    let base = Style::Normal.char_fmt();
    let (b, a) = st.spacing();
    let mut ppr = format!(
        "<w:pPr><w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{}\" w:lineRule=\"auto\"/>",
        tw(b),
        tw(a),
        (st.line_spacing() * 240.0).round() as i32
    );
    if st == Style::Quote {
        ppr.push_str("<w:ind w:left=\"720\" w:right=\"720\"/>");
    }
    if st == Style::Code {
        ppr.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F3F3F3\"/>");
    }
    let outline = match st {
        Style::Heading1 => Some(0),
        Style::Heading2 => Some(1),
        Style::Heading3 => Some(2),
        _ => None,
    };
    if st.next_is_normal() {
        ppr.insert_str("<w:pPr>".len(), "<w:keepNext/><w:keepLines/>");
    }
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
        "<w:qFormat/>".to_string()
    } else {
        "<w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>".to_string()
    };
    let rpr = rpr_diff(&f, &base, 0, &mut DiffCache::default());
    format!(
        "<w:style w:type=\"paragraph\"{default} w:styleId=\"{}\"><w:name w:val=\"{name}\"/>{based}{ppr}{rpr}</w:style>",
        st.docx_id()
    )
}

#[derive(Default)]
struct DiffCache {
    extras: HashMap<u32, Vec<El>>,
}

impl DiffCache {
    fn extras(&mut self, id: u32) -> &Vec<El> {
        self.extras.entry(id).or_insert_with(|| xml::parse_elems(&pool::get(id)))
    }
}

fn highlight_xml(c: crate::doc::Rgba) -> (String, String) {
    if c[3] == 0 {
        return ("highlight".into(), "<w:highlight w:val=\"none\"/>".into());
    }
    match HIGHLIGHTS.iter().find(|(_, h)| h[..3] == c[..3]) {
        Some((name, _)) => ("highlight".into(), format!("<w:highlight w:val=\"{name}\"/>")),
        None => ("shd".into(), format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(c))),
    }
}

/// `w:rPr` holding the differences between `f` and `base`, plus raw extras.
fn rpr_diff(f: &CharFmt, base: &CharFmt, extra: u32, cache: &mut DiffCache) -> String {
    let mut items: Vec<(String, String)> = Vec::new();
    let toggle = |items: &mut Vec<(String, String)>, name: &str, on: bool, base_on: bool| {
        if on != base_on {
            let v = if on { String::new() } else { " w:val=\"0\"".into() };
            items.push((name.into(), format!("<w:{name}{v}/>")));
        }
    };
    if f.font != base.font {
        let n = esc(&fonts::font_name(f.font));
        items.push(("rFonts".into(), format!("<w:rFonts w:ascii=\"{n}\" w:hAnsi=\"{n}\" w:eastAsia=\"{n}\" w:cs=\"{n}\"/>")));
    }
    toggle(&mut items, "b", f.bold, base.bold);
    toggle(&mut items, "bCs", f.bold, base.bold);
    toggle(&mut items, "i", f.italic, base.italic);
    toggle(&mut items, "iCs", f.italic, base.italic);
    toggle(&mut items, "caps", f.caps, base.caps);
    toggle(&mut items, "smallCaps", f.small_caps, base.small_caps);
    toggle(&mut items, "strike", f.strike, base.strike);
    toggle(&mut items, "vanish", f.hidden, base.hidden);
    if f.color != base.color {
        items.push(("color".into(), format!("<w:color w:val=\"{}\"/>", hex(f.color))));
    }
    if (f.char_spacing - base.char_spacing).abs() > 0.01 {
        items.push(("spacing".into(), format!("<w:spacing w:val=\"{}\"/>", tw(f.char_spacing))));
    }
    if (f.size - base.size).abs() > 0.01 {
        let hp = (f.size * 2.0).round() as i32;
        items.push(("sz".into(), format!("<w:sz w:val=\"{hp}\"/>")));
        items.push(("szCs".into(), format!("<w:szCs w:val=\"{hp}\"/>")));
    }
    if f.highlight != base.highlight {
        items.push(highlight_xml(f.highlight));
    }
    let extras = if extra != 0 { cache.extras(extra).clone() } else { Vec::new() };
    let fancy_u = extras.iter().any(|e| e.name == "u");
    if f.underline != base.underline || (fancy_u && !f.underline) {
        if !(fancy_u && f.underline) {
            let v = if f.underline { "single" } else { "none" };
            items.push(("u".into(), format!("<w:u w:val=\"{v}\"/>")));
        }
    }
    if f.script != base.script {
        let v = match f.script {
            1 => "superscript",
            -1 => "subscript",
            _ => "baseline",
        };
        items.push(("vertAlign".into(), format!("<w:vertAlign w:val=\"{v}\"/>")));
    }
    let written: HashSet<String> = items.iter().map(|(n, _)| n.clone()).collect();
    for e in extras {
        if e.name == "u" && !f.underline {
            continue;
        }
        if e.name == "color" && f.color != base.color {
            continue;
        }
        if !written.contains(&e.name) {
            items.push((e.name.clone(), e.to_xml()));
        }
    }
    if items.is_empty() {
        return String::new();
    }
    format!("<w:rPr>{}</w:rPr>", ordered(RPR_ORDER, items))
}

struct Writer<'a> {
    doc: &'a Document,
    pkg_id: u64,
    cache: DiffCache,
    /// New relationships: (id, type, target, external).
    rels: Vec<(String, String, String, bool)>,
    link_rels: HashMap<String, String>,
    image_rels: HashMap<u32, String>,
    media: Vec<(String, Arc<Vec<u8>>)>,
    doc_pr_id: u32,
    /// (paragraph, char) of field markers to drop because they are unbalanced.
    drop_markers: HashSet<(usize, usize)>,
    /// Existing rel ids, to avoid collisions.
    used_rel_ids: HashSet<String>,
}

impl<'a> Writer<'a> {
    /// `pkg_id` is the package the document's raw XML may refer to (the main document's for headers).
    fn new(doc: &'a Document, pkg_id: u64, used_rel_ids: HashSet<String>) -> Self {
        let mut w = Writer {
            doc,
            pkg_id,
            cache: DiffCache::default(),
            rels: Vec::new(),
            link_rels: HashMap::new(),
            image_rels: HashMap::new(),
            media: Vec::new(),
            doc_pr_id: 0,
            drop_markers: HashSet::new(),
            used_rel_ids,
        };
        w.balance_fields();
        w
    }

    /// Paragraphs and tables of the whole document.
    fn blocks_xml(&mut self) -> String {
        let doc = self.doc;
        let mut out = String::new();
        let mut i = 0;
        while i < doc.paras.len() {
            if let Some(c) = doc.paras[i].cell {
                let mut e = i;
                while e < doc.paras.len() && doc.paras[e].cell.is_some_and(|x| x.table == c.table) {
                    e += 1;
                }
                self.table_xml(c.table, i..e, &mut out);
                i = e;
            } else {
                self.para_xml(i, &mut out);
                i += 1;
            }
        }
        out
    }

    fn new_rel(&mut self, kind: &str, target: &str, external: bool) -> String {
        let mut n = self.rels.len() + 1;
        let mut id = format!("rIdFw{n}");
        while self.used_rel_ids.contains(&id) {
            n += 1;
            id = format!("rIdFw{n}");
        }
        self.used_rel_ids.insert(id.clone());
        self.rels.push((id.clone(), format!("{REL_T}/{kind}"), target.to_string(), external));
        id
    }

    fn link_rel(&mut self, url: &str) -> String {
        if let Some(id) = self.link_rels.get(url) {
            return id.clone();
        }
        let id = self.new_rel("hyperlink", url, true);
        self.link_rels.insert(url.to_string(), id.clone());
        id
    }

    fn image_rel(&mut self, image: u32) -> String {
        if let Some(id) = self.image_rels.get(&image) {
            return id.clone();
        }
        let img = &self.doc.images[&image];
        let (ext, _) = image_kind(&img.bytes);
        let name = format!("media/fw_image{image}.{ext}");
        self.media.push((format!("word/{name}"), img.bytes.clone()));
        let id = self.new_rel("image", &name, false);
        self.image_rels.insert(image, id.clone());
        id
    }

    /// Finds unbalanced field markers across the whole document.
    fn balance_fields(&mut self) {
        let mut stack: Vec<((usize, usize), bool)> = Vec::new(); // (pos, has separator)
        let mut seps: Vec<(usize, usize)> = Vec::new();
        let mut drop = HashSet::new();
        // Each footnote/endnote may be referenced once (copy & paste can duplicate references).
        let mut note_refs: HashSet<(bool, String)> = HashSet::new();
        for (pi, p) in self.doc.paras.iter().enumerate() {
            // Wrappers (tracked insertions) must open and close within a paragraph.
            let mut wraps: Vec<usize> = Vec::new();
            for (ci, f) in p.fmts.iter().enumerate() {
                let Some(o) = pool::obj(f.obj) else { continue };
                if !self.obj_allowed(&o) {
                    continue;
                }
                match o.kind {
                    ObjKind::Wrap { open: true } => wraps.push(ci),
                    ObjKind::Wrap { open: false } => {
                        if wraps.pop().is_none() {
                            drop.insert((pi, ci));
                        }
                    }
                    ObjKind::FieldBegin => stack.push(((pi, ci), false)),
                    ObjKind::FieldSep => {
                        if let Some(top) = stack.last_mut() {
                            if top.1 {
                                drop.insert((pi, ci));
                            } else {
                                top.1 = true;
                                seps.push((pi, ci));
                            }
                        } else {
                            drop.insert((pi, ci));
                        }
                    }
                    ObjKind::FieldEnd => {
                        if stack.pop().is_none() {
                            drop.insert((pi, ci));
                        }
                    }
                    ObjKind::NoteRef { endnote, ref id } if id != "self" => {
                        if !note_refs.insert((endnote, id.clone())) {
                            drop.insert((pi, ci));
                        }
                    }
                    _ => {}
                }
            }
            for ci in wraps {
                drop.insert((pi, ci));
            }
        }
        // An unterminated field drops its begin marker and its separator.
        for ((pi, ci), has_sep) in stack {
            drop.insert((pi, ci));
            if has_sep {
                if let Some(s) = seps.iter().find(|s| **s > (pi, ci)) {
                    drop.insert(*s);
                }
            }
        }
        self.drop_markers = drop;
    }

    fn obj_allowed(&self, o: &RawObj) -> bool {
        if o.pkg == self.pkg_id {
            return true;
        }
        // Content from another package: only self-contained markup survives.
        match &o.kind {
            ObjKind::FieldBegin | ObjKind::FieldSep | ObjKind::FieldEnd | ObjKind::Wrap { .. } => true,
            ObjKind::Label(_) => !pool::has_rel_refs(&o.xml),
            ObjKind::Hidden => o.xml.contains("instrText") && !pool::has_rel_refs(&o.xml),
            ObjKind::Deleted(_) => !pool::has_rel_refs(&o.xml),
            _ => false,
        }
    }

    fn base_for(&mut self, p: &Para, extra: u32) -> CharFmt {
        let rstyle = if extra != 0 {
            self.cache.extras(extra).iter().find(|e| e.name == "rStyle").and_then(|e| e.attr("val").map(|s| s.to_string()))
        } else {
            None
        };
        let pstyle = self.style_for_write(p);
        self.doc.styles.char_fmt(pstyle.as_deref(), rstyle.as_deref())
    }

    /// Style id written for a paragraph (only ids the style sheet knows).
    fn style_for_write(&self, p: &Para) -> Option<String> {
        let id = match &p.style_id {
            Some(id) => Some(id.clone()),
            None if p.style != Style::Normal => self
                .doc
                .styles
                .id_for_name(p.style.name())
                .or_else(|| Some(p.style.docx_id().to_string())),
            None => None,
        };
        id.filter(|id| self.doc.styles.styles.contains_key(id))
    }

    fn para_xml(&mut self, pi: usize, out: &mut String) {
        let p = &self.doc.paras[pi];
        if let Some(raw) = &p.raw_block {
            out.push_str(raw);
            return;
        }
        let doc = self.doc;
        let style = self.style_for_write(p);
        let base = doc.styles.para_props(style.as_deref(), &doc.numbering);
        let mut items: Vec<(String, String)> = Vec::new();
        if let Some(s) = &style {
            if Some(s) != doc.styles.default_para.as_ref() {
                items.push(("pStyle".into(), format!("<w:pStyle w:val=\"{}\"/>", esc(s))));
            }
        }
        // Numbering.
        let ours = (p.list != ListKind::None && p.num_id != 0 && doc.numbering.nums.contains_key(&p.num_id)).then_some((p.num_id, p.level));
        let mut ind_base = base.clone();
        if ours != base.num {
            match ours {
                Some((n, l)) => {
                    items.push(("numPr".into(), format!("<w:numPr><w:ilvl w:val=\"{l}\"/><w:numId w:val=\"{n}\"/></w:numPr>")));
                    if let Some(lv) = doc.numbering.level(n, l) {
                        ind_base.ind_left = lv.left;
                        ind_base.ind_first = -lv.hanging;
                    }
                }
                None => items.push(("numPr".into(), "<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"0\"/></w:numPr>".into())),
            }
        }
        if (p.space_before - base.before).abs() > 0.01
            || (p.space_after - base.after).abs() > 0.01
            || (p.line_spacing - base.line).abs() > 0.001
            || p.line_rule != base.line_rule
        {
            let (line, rule) = match p.line_rule {
                LineRule::Auto => ((p.line_spacing * 240.0).round() as i32, "auto"),
                LineRule::AtLeast => (tw(p.line_spacing), "atLeast"),
                LineRule::Exact => (tw(p.line_spacing), "exact"),
            };
            items.push((
                "spacing".into(),
                format!(
                    "<w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{line}\" w:lineRule=\"{rule}\"/>",
                    tw(p.space_before),
                    tw(p.space_after)
                ),
            ));
        }
        if (p.ind_left - ind_base.ind_left).abs() > 0.01
            || (p.ind_right - ind_base.ind_right).abs() > 0.01
            || (p.ind_first - ind_base.ind_first).abs() > 0.01
        {
            let first = if p.ind_first < 0.0 {
                format!(" w:hanging=\"{}\"", tw(-p.ind_first))
            } else {
                format!(" w:firstLine=\"{}\"", tw(p.ind_first))
            };
            items.push((
                "ind".into(),
                format!("<w:ind w:left=\"{}\" w:right=\"{}\"{first}/>", tw(p.ind_left), tw(p.ind_right)),
            ));
        }
        if p.align != base.align {
            let v = match p.align {
                Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
                Align::Justify => "both",
            };
            items.push(("jc".into(), format!("<w:jc w:val=\"{v}\"/>")));
        }
        if p.contextual != base.contextual {
            items.push(("contextualSpacing".into(), format!("<w:contextualSpacing w:val=\"{}\"/>", p.contextual as u8)));
        }
        if p.page_break_before != base.page_break_before {
            items.push(("pageBreakBefore".into(), format!("<w:pageBreakBefore w:val=\"{}\"/>", p.page_break_before as u8)));
        }
        if p.shading != base.shading {
            let fill = if p.shading[3] == 0 { "auto".to_string() } else { hex(p.shading) };
            items.push(("shd".into(), format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{fill}\"/>")));
        }
        if p.tabs != base.tabs {
            let mut t = String::from("<w:tabs>");
            for b in &base.tabs {
                if !p.tabs.iter().any(|x| (x.pos - b.pos).abs() < 0.5) {
                    t.push_str(&format!("<w:tab w:val=\"clear\" w:pos=\"{}\"/>", tw(b.pos)));
                }
            }
            for x in &p.tabs {
                t.push_str(&tab_xml(x));
            }
            t.push_str("</w:tabs>");
            if t != "<w:tabs></w:tabs>" {
                items.push(("tabs".into(), t));
            }
        }
        let mark_base = self.base_for(p, p.end_fmt.rpr_extra);
        let mark = rpr_diff(&p.end_fmt, &mark_base, p.end_fmt.rpr_extra, &mut self.cache);
        if !mark.is_empty() {
            items.push(("rPr".into(), mark));
        }
        let written: HashSet<String> = items.iter().map(|(n, _)| n.clone()).collect();
        for e in xml::parse_elems(&p.ppr_extra) {
            if !written.contains(&e.name) {
                if e.name == "sectPr" && !p.sect_break {
                    continue;
                }
                items.push((e.name.clone(), e.to_xml()));
            }
        }
        out.push_str("<w:p>");
        if !items.is_empty() {
            out.push_str("<w:pPr>");
            out.push_str(&ordered(PPR_ORDER, items));
            out.push_str("</w:pPr>");
        }
        self.runs_xml(pi, out);
        out.push_str("</w:p>");
    }

    fn runs_xml(&mut self, pi: usize, out: &mut String) {
        let doc = self.doc;
        let p = &doc.paras[pi];
        let mut i = 0;
        let mut open_link = 0u32;
        while i < p.chars.len() {
            let f = p.fmts[i];
            // Hyperlink wrapper.
            if f.link != open_link {
                if open_link != 0 {
                    out.push_str("</w:hyperlink>");
                }
                open_link = f.link;
                if open_link != 0 {
                    let (url, anchor) = pool::link(open_link);
                    let mut attrs = String::new();
                    if !url.is_empty() {
                        let id = self.link_rel(&url);
                        attrs.push_str(&format!(" r:id=\"{id}\""));
                    }
                    if !anchor.is_empty() {
                        attrs.push_str(&format!(" w:anchor=\"{}\"", esc(&anchor)));
                    }
                    out.push_str(&format!("<w:hyperlink{attrs} w:history=\"1\">"));
                }
            }
            if f.image != 0 {
                self.image_xml(f.image, &f, p, out);
                i += 1;
                continue;
            }
            if f.obj != 0 {
                if let Some(o) = pool::obj(f.obj) {
                    if self.obj_allowed(&o) && !self.drop_markers.contains(&(pi, i)) {
                        // Paragraph-level objects (bookmarks, ins/del...) must not sit inside a hyperlink.
                        let paragraph_level = !o.xml.starts_with("<w:r>") && !o.xml.starts_with("<w:r ");
                        if paragraph_level && open_link != 0 {
                            out.push_str("</w:hyperlink>");
                            out.push_str(&o.xml);
                            open_link = 0;
                        } else {
                            out.push_str(&o.xml);
                        }
                    }
                }
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < p.chars.len() && p.fmts[j] == f {
                j += 1;
            }
            let base = self.base_for(p, f.rpr_extra);
            let rpr = rpr_diff(&f, &base, f.rpr_extra, &mut self.cache);
            out.push_str("<w:r>");
            out.push_str(&rpr);
            let mut buf = String::new();
            let flush = |buf: &mut String, out: &mut String| {
                if !buf.is_empty() {
                    out.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", esc_text(buf)));
                    buf.clear();
                }
            };
            for &c in &p.chars[i..j] {
                let special = match c {
                    '\t' => Some("<w:tab/>"),
                    LINE_BREAK => Some("<w:br/>"),
                    PAGE_BREAK => Some("<w:br w:type=\"page\"/>"),
                    COLUMN_BREAK => Some("<w:br w:type=\"column\"/>"),
                    NBH => Some("<w:noBreakHyphen/>"),
                    SHY => Some("<w:softHyphen/>"),
                    OBJ => Some(""),
                    _ => None,
                };
                match special {
                    Some(x) => {
                        flush(&mut buf, out);
                        out.push_str(x);
                    }
                    None => buf.push(c),
                }
            }
            flush(&mut buf, out);
            out.push_str("</w:r>");
            i = j;
        }
        if open_link != 0 {
            out.push_str("</w:hyperlink>");
        }
    }

    fn image_xml(&mut self, image: u32, f: &CharFmt, p: &Para, out: &mut String) {
        let Some(img) = self.doc.images.get(&image) else { return };
        let (w, h) = (img.w, img.h);
        let raw = img.raw.clone();
        let rel = self.image_rel(image);
        let cx = (w * 12700.0) as i64;
        let cy = (h * 12700.0) as i64;
        if let Some((raw, pkg)) = raw {
            if pkg == self.pkg_id || !raw.contains(" r:id=") {
                if let Ok(mut el) = xml::parse(&raw) {
                    patch_drawing(&mut el, &rel, cx, cy);
                    out.push_str(&el.to_xml());
                    return;
                }
            }
        }
        self.doc_pr_id += 1;
        let n = self.doc_pr_id;
        let base = self.base_for(p, f.rpr_extra);
        let mut plain = *f;
        plain.image = 0;
        let rpr = rpr_diff(&plain, &base, f.rpr_extra, &mut self.cache);
        out.push_str(&format!(
            "<w:r>{rpr}<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"{id}\" name=\"Picture {n}\"/>\
<wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"image{n}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r>",
            id = 60000 + n
        ));
    }

    fn table_xml(&mut self, table: u32, range: std::ops::Range<usize>, out: &mut String) {
        let doc = self.doc;
        let Some(t) = doc.tables.get(&table) else {
            for pi in range {
                self.para_xml(pi, out);
            }
            return;
        };
        out.push_str("<w:tbl>");
        match &t.tblpr_raw {
            Some(raw) => out.push_str(raw),
            None => {
                let style = if doc.styles.styles.contains_key("TableGrid") && t.borders {
                    "<w:tblStyle w:val=\"TableGrid\"/>".to_string()
                } else {
                    String::new()
                };
                let borders = if t.borders && style.is_empty() {
                    "<w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tblBorders>"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "<w:tblPr>{style}<w:tblW w:w=\"0\" w:type=\"auto\"/>{borders}<w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"1\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/></w:tblPr>"
                ));
            }
        }
        out.push_str("<w:tblGrid>");
        for g in &t.grid {
            out.push_str(&format!("<w:gridCol w:w=\"{}\"/>", tw(*g)));
        }
        out.push_str("</w:tblGrid>");
        // Group paragraphs into rows and cells.
        let mut i = range.start;
        while i < range.end {
            let row = doc.paras[i].cell.unwrap().row;
            let rp = t.rows.get(&row).cloned().unwrap_or_default();
            out.push_str("<w:tr>");
            match &rp.trpr_raw {
                Some(raw) => out.push_str(raw),
                None if rp.height > 0.0 => out.push_str(&format!(
                    "<w:trPr><w:trHeight w:val=\"{}\"{}/></w:trPr>",
                    tw(rp.height),
                    if rp.exact { " w:hRule=\"exact\"" } else { "" }
                )),
                None => {}
            }
            while i < range.end && doc.paras[i].cell.unwrap().row == row {
                let cref = doc.paras[i].cell.unwrap();
                let cp = t.cell(cref.row, cref.col);
                let width: f32 = t.grid.iter().skip(cref.col as usize).take(cp.span.max(1) as usize).sum();
                out.push_str("<w:tc>");
                out.push_str(&tcpr_xml(&cp, width));
                let mut last_was_para = false;
                while i < range.end && doc.paras[i].cell == Some(cref) {
                    last_was_para = doc.paras[i].raw_block.is_none();
                    self.para_xml(i, out);
                    i += 1;
                }
                if !last_was_para {
                    out.push_str("<w:p/>");
                }
                out.push_str("</w:tc>");
            }
            out.push_str("</w:tr>");
        }
        out.push_str("</w:tbl>");
    }
}

fn tab_xml(t: &TabStop) -> String {
    let val = match t.kind {
        TabKind::Left => "left",
        TabKind::Center => "center",
        TabKind::Right => "right",
        TabKind::Decimal => "decimal",
        TabKind::Bar => "bar",
        TabKind::Clear => "clear",
    };
    let leader = match t.leader {
        Leader::None => "",
        Leader::Dot => " w:leader=\"dot\"",
        Leader::Hyphen => " w:leader=\"hyphen\"",
        Leader::Underscore => " w:leader=\"underscore\"",
    };
    format!("<w:tab w:val=\"{val}\"{leader} w:pos=\"{}\"/>", tw(t.pos))
}

fn tcpr_xml(cp: &CellProps, width: f32) -> String {
    let mut items: Vec<(String, String)> = Vec::new();
    items.push(("tcW".into(), format!("<w:tcW w:w=\"{}\" w:type=\"dxa\"/>", tw(width))));
    if cp.span > 1 {
        items.push(("gridSpan".into(), format!("<w:gridSpan w:val=\"{}\"/>", cp.span)));
    }
    match cp.vmerge {
        VMerge::Restart => items.push(("vMerge".into(), "<w:vMerge w:val=\"restart\"/>".into())),
        VMerge::Continue => items.push(("vMerge".into(), "<w:vMerge/>".into())),
        VMerge::None => {}
    }
    if let Some(c) = cp.shade {
        items.push(("shd".into(), format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(c))));
    }
    if let Some(raw) = &cp.tcpr_raw {
        if let Ok(el) = xml::parse(raw) {
            for e in el.elems() {
                if !matches!(e.name.as_str(), "tcW" | "gridSpan" | "vMerge" | "shd" | "hMerge") {
                    items.push((e.name.clone(), e.to_xml()));
                }
            }
        }
    }
    format!("<w:tcPr>{}</w:tcPr>", ordered(TCPR_ORDER, items))
}

/// Points a drawing's picture at a new relationship and updates its size.
fn patch_drawing(el: &mut El, rel: &str, cx: i64, cy: i64) {
    for c in el.elems_mut() {
        match c.name.as_str() {
            "blip" => {
                if c.attrs.iter().any(|(k, _)| k.ends_with(":embed")) {
                    let key = c.attrs.iter().find(|(k, _)| k.ends_with(":embed")).unwrap().0.clone();
                    c.set_attr(&key, rel);
                }
            }
            "imagedata" => {
                if let Some(key) = c.attrs.iter().find(|(k, _)| k.ends_with(":id")).map(|(k, _)| k.clone()) {
                    c.set_attr(&key, rel);
                }
            }
            "extent" | "ext" => {
                if c.attr("cx").is_some() && c.attr("cy").is_some() {
                    c.set_attr("cx", &cx.to_string());
                    c.set_attr("cy", &cy.to_string());
                }
            }
            _ => {}
        }
        patch_drawing(c, rel, cx, cy);
    }
}

fn hf_xml(tag: &str, text: &str, page_numbers: bool) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:{tag} xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\"><w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>"
    );
    let gray = "<w:rPr><w:color w:val=\"595959\"/><w:sz w:val=\"20\"/></w:rPr>";
    if !text.is_empty() {
        s.push_str(&format!("<w:r>{gray}<w:t xml:space=\"preserve\">{}</w:t></w:r>", esc_text(text)));
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

/// Body `w:sectPr` with the page setup (and possibly new header/footer refs) applied.
fn section_xml(doc: &Document, original: Option<&str>, header: Option<&str>, footer: Option<&str>) -> String {
    let pg = &doc.page;
    let mut el = original.and_then(|o| xml::parse(o).ok()).unwrap_or_else(|| El::new("w:sectPr"));
    let mut children: Vec<El> = el.elems().cloned().collect();
    let mut set = |name: &str, build: &dyn Fn(Option<&El>) -> El| {
        let pos = children.iter().position(|c| c.name == name);
        let new = build(pos.map(|i| &children[i]));
        match pos {
            Some(i) => children[i] = new,
            None => children.push(new),
        }
    };
    set("pgSz", &|old| {
        let mut e = old.cloned().unwrap_or_else(|| El::new("w:pgSz"));
        e.set_attr("w:w", &tw(pg.width).to_string());
        e.set_attr("w:h", &tw(pg.height).to_string());
        e.attrs.retain(|(k, _)| k != "w:orient");
        if pg.width > pg.height {
            e.set_attr("w:orient", "landscape");
        }
        e
    });
    set("pgMar", &|old| {
        let mut e = old.cloned().unwrap_or_else(|| El::new("w:pgMar"));
        for (k, v) in [
            ("w:top", pg.margins[0]),
            ("w:right", pg.margins[1]),
            ("w:bottom", pg.margins[2]),
            ("w:left", pg.margins[3]),
            ("w:header", pg.header_dist),
            ("w:footer", pg.footer_dist),
        ] {
            e.set_attr(k, &tw(v).to_string());
        }
        if e.qattr("w:gutter").is_none() {
            e.set_attr("w:gutter", "0");
        }
        e
    });
    for (tag, rid) in [("headerReference", header), ("footerReference", footer)] {
        if let Some(rid) = rid {
            children.retain(|c| !(c.name == tag && c.attr("type").is_none_or(|t| t == "default" || t == "first" || t == "even")));
            let mut r = El::new(&format!("w:{tag}"));
            r.set_attr("w:type", "default");
            r.set_attr("r:id", rid);
            children.push(r);
            // A title page setting would hide our new default header on page 1.
            children.retain(|c| c.name != "titlePg");
        }
    }
    const SECT_ORDER: &[&str] = &[
        "headerReference", "footerReference", "footnotePr", "endnotePr", "type", "pgSz", "pgMar", "paperSrc",
        "pgBorders", "lnNumType", "pgNumType", "cols", "formProt", "vAlign", "noEndnote", "titlePg",
        "textDirection", "bidi", "rtlGutter", "docGrid", "printerSettings", "sectPrChange",
    ];
    children.sort_by_key(|c| order_of(SECT_ORDER, &c.name));
    el.children = children.into_iter().map(xml::Node::El).collect();
    if el.qname.is_empty() || el.qname == "#root" {
        el.qname = "w:sectPr".into();
    }
    el.to_xml()
}

fn content_types_new(has_header: bool, has_footer: bool) -> String {
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
<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>\
<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>",
    );
    if has_header {
        ct.push_str("<Override PartName=\"/word/header_fw.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>");
    }
    if has_footer {
        ct.push_str("<Override PartName=\"/word/footer_fw.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>");
    }
    ct.push_str("</Types>");
    ct
}

/// Adds Default/Override entries to an existing [Content_Types].xml.
fn patch_content_types(original: &str, defaults: &[(&str, &str)], overrides: &[(&str, &str)]) -> String {
    let mut s = original.to_string();
    let mut add = String::new();
    for (ext, ct) in defaults {
        let needle = format!("Extension=\"{ext}\"");
        let needle_upper = format!("Extension=\"{}\"", ext.to_uppercase());
        if !s.contains(&needle) && !s.contains(&needle_upper) {
            add.push_str(&format!("<Default Extension=\"{ext}\" ContentType=\"{ct}\"/>"));
        }
    }
    for (part, ct) in overrides {
        if !s.contains(&format!("PartName=\"{part}\"")) {
            add.push_str(&format!("<Override PartName=\"{part}\" ContentType=\"{ct}\"/>"));
        }
    }
    if let Some(i) = s.rfind("</Types>") {
        s.insert_str(i, &add);
    }
    s
}

fn patch_rels(original: &str, rels: &[(String, String, String, bool)]) -> String {
    let mut s = original.to_string();
    let add: String = rels
        .iter()
        .map(|(id, ty, target, ext)| {
            format!(
                "<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{}\"{}/>",
                esc(target),
                if *ext { " TargetMode=\"External\"" } else { "" }
            )
        })
        .collect();
    match s.rfind("</Relationships>") {
        Some(i) => s.insert_str(i, &add),
        None => {
            s = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{add}</Relationships>"
            )
        }
    }
    s
}

/// Header/footer part XML for a sub-document, with the relationships and media it adds.
/// `root_attrs` are the original root element's attributes (namespace declarations).
fn hf_part_xml(
    sub: &Document,
    pkg_id: u64,
    tag: &str,
    root_attrs: &[(String, String)],
    used: HashSet<String>,
) -> (String, Vec<(String, String, String, bool)>, Vec<(String, Arc<Vec<u8>>)>) {
    let mut sub = sub.clone();
    sub.ensure_trailing_para();
    let mut w = Writer::new(&sub, pkg_id, used);
    let body = w.blocks_xml();
    let mut attrs: Vec<(String, String)> = root_attrs.to_vec();
    for (k, v) in [
        ("xmlns:w", W_NS),
        ("xmlns:r", R_NS),
        ("xmlns:wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"),
        ("xmlns:a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
        ("xmlns:pic", "http://schemas.openxmlformats.org/drawingml/2006/picture"),
    ] {
        if !attrs.iter().any(|(a, _)| a == k) {
            attrs.push((k.into(), v.into()));
        }
    }
    let attr_s: String = attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", esc(v))).collect();
    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:{tag}{attr_s}>{body}</w:{tag}>");
    (xml, w.rels, w.media)
}

/// Rewritten parts (and their relationships and media) for every edited
/// header/footer that was read from the package.
pub fn edited_hf_parts(doc: &Document) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let Some(pkg) = doc.pkg.as_deref() else { return out };
    let mut done = HashSet::new();
    for sub in doc.hf.iter().flat_map(|h| [&h.header, &h.header_first, &h.footer, &h.footer_first]).flatten() {
        let Some(target) = sub.source.as_ref().filter(|_| sub.edited) else { continue };
        if !done.insert(target.clone()) {
            continue;
        }
        let Some(orig) = part_xml(&pkg.parts, target) else { continue };
        let tag = if orig.name == "ftr" { "ftr" } else { "hdr" };
        let (dir, file) = target.rsplit_once('/').unwrap_or(("", target));
        let rels_name = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
        let orig_rels = pkg.parts.get(&rels_name).map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
        let used: HashSet<String> = xml::parse(&orig_rels)
            .map(|r| r.elems().filter_map(|e| e.attr("Id").map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let (xml, rels, media) = hf_part_xml(sub, pkg.id, tag, &orig.attrs, used);
        out.push((target.clone(), xml.into_bytes()));
        if !rels.is_empty() {
            out.push((rels_name, patch_rels(&orig_rels, &rels).into_bytes()));
        }
        // Media paths are relative to the part's folder.
        out.extend(media.into_iter().map(|(n, b)| {
            let n = n.strip_prefix("word/").unwrap_or(&n);
            (if dir.is_empty() { n.to_string() } else { format!("{dir}/{n}") }, b.to_vec())
        }));
    }
    out
}

pub fn save_docx(doc: &Document, path: &Path) -> Result<(), String> {
    let bytes = docx_bytes(doc)?;
    // Write to a temporary file first so a failure never destroys the original.
    let tmp = path.with_extension("docx.fwtmp");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub fn docx_bytes(doc: &Document) -> Result<Vec<u8>, String> {
    let pkg = doc.pkg.as_deref();
    let main_part = "word/document.xml";
    let orig_rels_name = "word/_rels/document.xml.rels";
    let mut used = HashSet::new();
    if let Some(p) = pkg {
        if let Some(r) = p.parts.get(orig_rels_name) {
            if let Ok(root) = xml::parse(&String::from_utf8_lossy(r)) {
                for e in root.elems() {
                    if let Some(id) = e.attr("Id") {
                        used.insert(id.to_string());
                    }
                }
            }
        }
    }
    let mut w = Writer::new(doc, doc.pkg_id(), used);
    let body = w.blocks_xml();

    // Header/footer: keep the originals unless the user edited them. Headers
    // created in FreeWord (rich, or plain text) become new default parts of the last section.
    let last_hf = doc.hf.last();
    fn rich_new(d: &Option<Box<Document>>) -> Option<&Document> {
        d.as_deref().filter(|d| d.source.is_none())
    }
    let rich_header = last_hf.and_then(|h| rich_new(&h.header));
    let rich_footer = last_hf.and_then(|h| rich_new(&h.footer));
    let hf_changed = |pkg: &Package| {
        (doc.header != pkg.header_text, doc.footer != pkg.footer_text || doc.page_numbers != pkg.page_numbers)
    };
    let (plain_header, plain_footer) = match pkg {
        Some(p) => hf_changed(p),
        None => (!doc.header.is_empty(), !doc.footer.is_empty() || doc.page_numbers),
    };
    let mut hf_out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut new_part = |rich: Option<&Document>, plain: bool, header: bool| -> bool {
        let (tag, name) = if header { ("hdr", "header_fw.xml") } else { ("ftr", "footer_fw.xml") };
        let xml = match rich {
            Some(d) => {
                let (xml, rels, media) = hf_part_xml(d, doc.pkg_id(), tag, &[], HashSet::new());
                if !rels.is_empty() {
                    hf_out.push((format!("word/_rels/{name}.rels"), patch_rels("", &rels).into_bytes()));
                }
                hf_out.extend(media.into_iter().map(|(n, b)| (n, b.to_vec())));
                xml
            }
            None if plain => {
                if header { hf_xml("hdr", &doc.header, false) } else { hf_xml("ftr", &doc.footer, doc.page_numbers) }
            }
            None => return false,
        };
        hf_out.push((format!("word/{name}"), xml.into_bytes()));
        true
    };
    let new_header = new_part(rich_header, plain_header, true);
    let new_footer = new_part(rich_footer, plain_footer, false);
    hf_out.extend(edited_hf_parts(doc));
    let header_rel = new_header.then(|| w.new_rel("header", "header_fw.xml", false));
    let footer_rel = new_footer.then(|| w.new_rel("footer", "footer_fw.xml", false));
    let sect = section_xml(doc, pkg.and_then(|p| p.body_sect.as_deref()), header_rel.as_deref(), footer_rel.as_deref());

    // Root element with the original namespace declarations.
    let mut attrs: Vec<(String, String)> = pkg.map(|p| p.root_attrs.clone()).unwrap_or_default();
    for (k, v) in [
        ("xmlns:w", W_NS),
        ("xmlns:r", R_NS),
        ("xmlns:wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"),
        ("xmlns:a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
        ("xmlns:pic", "http://schemas.openxmlformats.org/drawingml/2006/picture"),
    ] {
        if !attrs.iter().any(|(a, _)| a == k) {
            attrs.push((k.into(), v.into()));
        }
    }
    let attr_s: String = attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", esc(v))).collect();
    let background = pkg.and_then(|p| p.background.clone()).unwrap_or_default();
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document{attr_s}>{background}<w:body>{body}{sect}</w:body></w:document>"
    );

    // Assemble the package.
    let mut parts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let numbering_used = doc.paras.iter().chain(doc.hf_docs().flat_map(|d| d.paras.iter())).any(|p| p.list != ListKind::None);
    match pkg {
        Some(p) => {
            for (k, v) in &p.parts {
                parts.insert(k.clone(), v.clone());
            }
            let rels = read_rels(&p.parts, main_part);
            let numbering_target = rels.values().find(|r| r.kind == "numbering").map(|r| r.target.clone());
            let mut overrides: Vec<(&str, &str)> = Vec::new();
            match numbering_target {
                Some(t) => {
                    let orig = String::from_utf8_lossy(&p.parts[&t]).to_string();
                    parts.insert(t, doc.numbering.patch_xml(&orig).into_bytes());
                }
                None if numbering_used => {
                    w.new_rel("numbering", "numbering_fw.xml", false);
                    parts.insert("word/numbering_fw.xml".into(), doc.numbering.to_xml().into_bytes());
                    overrides.push(("/word/numbering_fw.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"));
                }
                None => {}
            }
            if new_header {
                overrides.push(("/word/header_fw.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"));
            }
            if new_footer {
                overrides.push(("/word/footer_fw.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"));
            }
            let orig_rels = p.parts.get(orig_rels_name).map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
            parts.insert(orig_rels_name.into(), patch_rels(&orig_rels, &w.rels).into_bytes());
            let ct = String::from_utf8_lossy(p.parts.get("[Content_Types].xml").map(|v| v.as_slice()).unwrap_or(b"")).to_string();
            let defaults = [("png", "image/png"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("bmp", "image/bmp")];
            parts.insert("[Content_Types].xml".into(), patch_content_types(&ct, &defaults, &overrides).into_bytes());
        }
        None => {
            let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>\
</Relationships>";
            parts.insert("_rels/.rels".into(), rels.as_bytes().to_vec());
            let base_rels = vec![
                ("rIdStyles".to_string(), format!("{REL_T}/styles"), "styles.xml".to_string(), false),
                ("rIdSettings".to_string(), format!("{REL_T}/settings"), "settings.xml".to_string(), false),
                ("rIdNumbering".to_string(), format!("{REL_T}/numbering"), "numbering.xml".to_string(), false),
            ];
            let all: Vec<_> = base_rels.into_iter().chain(w.rels.iter().cloned()).collect();
            parts.insert(orig_rels_name.into(), patch_rels("", &all).into_bytes());
            let styles = doc.styles.generated_xml.clone().unwrap_or_else(default_styles_xml);
            parts.insert("word/styles.xml".into(), styles.into_bytes());
            parts.insert("word/numbering.xml".into(), doc.numbering.to_xml().into_bytes());
            parts.insert(
                "word/settings.xml".into(),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"{W_NS}\"><w:defaultTabStop w:val=\"{}\"/><w:characterSpacingControl w:val=\"doNotCompress\"/><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>",
                    tw(doc.page.default_tab)
                )
                .into_bytes(),
            );
            parts.insert("[Content_Types].xml".into(), content_types_new(new_header, new_footer).into_bytes());
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
            parts.insert(
                "docProps/core.xml".into(),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified></cp:coreProperties>"
                )
                .into_bytes(),
            );
            parts.insert(
                "docProps/app.xml".into(),
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>FreeWord</Application></Properties>"
                    .as_bytes()
                    .to_vec(),
            );
        }
    }
    parts.insert(main_part.into(), document.into_bytes());
    parts.extend(hf_out);
    for (name, bytes) in &w.media {
        parts.insert(name.clone(), bytes.to_vec());
    }

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // [Content_Types].xml first, as Office does.
        let mut names: Vec<&String> = parts.keys().collect();
        names.sort_by_key(|n| (n.as_str() != "[Content_Types].xml", n.as_str() != "_rels/.rels", (*n).clone()));
        for name in names {
            zip.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
            zip.write_all(&parts[name]).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}
