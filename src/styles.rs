//! Word style sheet (`styles.xml` + theme fonts) and property resolution.
//!
//! Documents store fully resolved formatting per character/paragraph. When a
//! .docx is written, formatting is emitted as the *difference* from what the
//! paragraph/character style already provides, so styles keep working in Word.

use crate::doc::{Align, Border, BorderKind, CharFmt, Edges, Leader, LineRule, Rgba, TabKind, TabStop, BLACK, NO_COLOR};
use crate::fonts;
use crate::numbering::Numbering;
use crate::xml::{self, El};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct StyleDef {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub based_on: Option<String>,
    pub ppr: Option<El>,
    pub rpr: Option<El>,
    pub tblpr: Option<El>,
    pub tcpr: Option<El>,
    /// Conditional table formatting (`w:tblStylePr`) by type.
    pub cond: Vec<(String, El)>,
}

#[derive(Clone, Debug, Default)]
pub struct StyleSheet {
    pub styles: HashMap<String, StyleDef>,
    pub default_para: Option<String>,
    pub doc_rpr: Option<El>,
    pub doc_ppr: Option<El>,
    pub major_font: String,
    pub minor_font: String,
    /// styles.xml generated for documents that did not come from a .docx.
    pub generated_xml: Option<String>,
}

/// Resolved paragraph properties.
#[derive(Clone, Debug, PartialEq)]
pub struct PProps {
    pub align: Align,
    pub before: f32,
    pub after: f32,
    pub line: f32,
    pub line_rule: LineRule,
    pub contextual: bool,
    pub ind_left: f32,
    pub ind_right: f32,
    pub ind_first: f32,
    pub num: Option<(u32, u8)>,
    pub tabs: Vec<TabStop>,
    pub shading: Rgba,
    pub page_break_before: bool,
    pub borders: Edges,
    pub keep_next: bool,
    pub keep_lines: bool,
    /// Widow/orphan control (Word's default is on).
    pub widow: bool,
}

impl Default for PProps {
    fn default() -> Self {
        PProps {
            align: Align::Left,
            before: 0.0,
            after: 0.0,
            line: 1.0,
            line_rule: LineRule::Auto,
            contextual: false,
            ind_left: 0.0,
            ind_right: 0.0,
            ind_first: 0.0,
            num: None,
            tabs: Vec::new(),
            shading: NO_COLOR,
            page_break_before: false,
            borders: Edges::default(),
            keep_next: false,
            keep_lines: false,
            widow: true,
        }
    }
}

pub const HIGHLIGHTS: &[(&str, Rgba)] = &[
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

pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

pub fn on(e: &El) -> bool {
    !matches!(e.attr("val"), Some("0" | "false" | "off" | "none"))
}

fn twips(e: &El, key: &str) -> Option<f32> {
    e.attr(key).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 20.0)
}

/// Parses a border element (`w:top`, `w:insideH`, ...).
pub fn parse_border(e: &El) -> Border {
    let kind = match e.attr("val") {
        None | Some("nil" | "none") => BorderKind::None,
        Some("double" | "triple" | "thinThickSmallGap" | "thickThinSmallGap" | "thinThickMediumGap" | "thickThinMediumGap" | "thinThickLargeGap" | "thickThinLargeGap") => BorderKind::Double,
        Some("dotted" | "dotDash" | "dotDotDash") => BorderKind::Dotted,
        Some("dashed" | "dashSmallGap" | "dashDotStroked") => BorderKind::Dashed,
        _ => BorderKind::Single,
    };
    // w:sz is in eighths of a point.
    let width = e.attr("sz").and_then(|v| v.parse::<f32>().ok()).map(|v| (v / 8.0).max(0.25)).unwrap_or(0.5);
    let color = match e.attr("color") {
        Some("auto") | None => BLACK,
        Some(c) => parse_hex(c).unwrap_or(BLACK),
    };
    let space = e.attr("space").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    Border { kind, width, color, space }
}

/// Parses `w:tblBorders` / `w:tcBorders` / `w:pBdr`.
pub fn parse_edges(e: &El) -> Edges {
    let mut ed = Edges::default();
    for c in e.elems() {
        let b = Some(parse_border(c));
        match c.name.as_str() {
            "top" => ed.top = b,
            "left" | "start" => ed.left = b,
            "bottom" => ed.bottom = b,
            "right" | "end" => ed.right = b,
            "insideH" | "between" => ed.inside_h = b,
            "insideV" => ed.inside_v = b,
            _ => {}
        }
    }
    ed
}

/// Cell margins from `w:tblCellMar` / `w:tcMar` applied over `m` (top, left, bottom, right).
pub fn parse_margins(e: &El, m: &mut [f32; 4]) {
    for c in e.elems() {
        let Some(v) = c.attr("w").and_then(|v| v.parse::<f32>().ok()) else { continue };
        if c.attr("type").is_some_and(|t| t != "dxa") {
            continue;
        }
        let v = v / 20.0;
        match c.name.as_str() {
            "top" => m[0] = v,
            "left" | "start" => m[1] = v,
            "bottom" => m[2] = v,
            "right" | "end" => m[3] = v,
            _ => {}
        }
    }
}

/// Table-level formatting resolved from a table style chain.
#[derive(Clone, Debug, Default)]
pub struct TableStyle {
    pub edges: Edges,
    pub cell_margins: Option<[f32; 4]>,
    /// Shading of the whole table (style `w:tcPr`).
    pub shade: Option<Rgba>,
    /// Conditional formats by type, root style first.
    pub cond: Vec<(String, El)>,
    pub row_band: u32,
    pub col_band: u32,
    /// Paragraph/run properties of the style itself.
    pub ppr: Vec<El>,
    pub rpr: Vec<El>,
}

/// Shading fill color, if any.
pub fn shd_fill(e: &El) -> Option<Rgba> {
    let fill = e.attr("fill")?;
    if fill == "auto" {
        return None;
    }
    parse_hex(fill)
}

impl StyleSheet {
    pub fn parse(styles: Option<&El>, theme: Option<&El>) -> StyleSheet {
        let mut sh = StyleSheet { major_font: "Calibri Light".into(), minor_font: "Calibri".into(), ..Default::default() };
        if let Some(theme) = theme {
            if let Some(fs) = theme.find("fontScheme") {
                for (tag, slot) in [("majorFont", &mut sh.major_font), ("minorFont", &mut sh.minor_font)] {
                    if let Some(face) = fs.child(tag).and_then(|f| f.child("latin")).and_then(|l| l.attr("typeface")) {
                        if !face.is_empty() {
                            *slot = face.to_string();
                        }
                    }
                }
            }
        }
        let Some(root) = styles else { return sh };
        if let Some(dd) = root.child("docDefaults") {
            sh.doc_rpr = dd.child("rPrDefault").and_then(|r| r.child("rPr")).cloned();
            sh.doc_ppr = dd.child("pPrDefault").and_then(|p| p.child("pPr")).cloned();
        }
        for s in root.elems().filter(|e| e.name == "style") {
            let Some(id) = s.attr("styleId") else { continue };
            let kind = s.attr("type").unwrap_or("paragraph").to_string();
            let def = StyleDef {
                id: id.to_string(),
                name: s.child_val("name").unwrap_or(id).to_string(),
                kind: kind.clone(),
                based_on: s.child_val("basedOn").map(|v| v.to_string()),
                ppr: s.child("pPr").cloned(),
                rpr: s.child("rPr").cloned(),
                tblpr: s.child("tblPr").cloned(),
                tcpr: s.child("tcPr").cloned(),
                cond: s
                    .elems()
                    .filter(|e| e.name == "tblStylePr")
                    .map(|e| (e.attr("type").unwrap_or("").to_string(), e.clone()))
                    .collect(),
            };
            if kind == "paragraph" && s.attr("default").is_some_and(|v| v == "1" || v == "true") {
                sh.default_para = Some(id.to_string());
            }
            sh.styles.insert(id.to_string(), def);
        }
        sh
    }

    /// Style sheet for new documents, built from FreeWord's own style definitions.
    pub fn generated() -> StyleSheet {
        let xml = crate::docx::default_styles_xml();
        let root = xml::parse(&xml).ok();
        let mut sh = StyleSheet::parse(root.as_ref(), None);
        sh.minor_font = fonts::default_font().to_string();
        sh.major_font = fonts::heading_font().to_string();
        sh.generated_xml = Some(xml);
        sh
    }

    /// Style chain from root to `id` (inclusive).
    fn chain(&self, id: Option<&str>) -> Vec<&StyleDef> {
        let mut out = Vec::new();
        let mut cur = id.map(|s| s.to_string());
        while let Some(c) = cur {
            let Some(def) = self.styles.get(&c) else { break };
            if out.iter().any(|d: &&StyleDef| d.id == def.id) || out.len() > 20 {
                break;
            }
            out.push(def);
            cur = def.based_on.clone();
        }
        out.reverse();
        out
    }

    pub fn para_style_or_default<'a>(&'a self, id: Option<&'a str>) -> Option<&'a str> {
        match id {
            Some(i) if self.styles.contains_key(i) => Some(i),
            _ => self.default_para.as_deref(),
        }
    }

    pub fn style_name(&self, id: &str) -> Option<&str> {
        self.styles.get(id).map(|s| s.name.as_str())
    }

    /// Finds a style id by its (case-insensitive) display name.
    pub fn id_for_name(&self, name: &str) -> Option<String> {
        self.styles
            .values()
            .find(|s| s.kind == "paragraph" && s.name.eq_ignore_ascii_case(name))
            .map(|s| s.id.clone())
    }

    /// Applies an `rPr` element to a format.
    pub fn apply_rpr(&self, rpr: &El, f: &mut CharFmt) {
        for e in rpr.elems() {
            match e.name.as_str() {
                "rFonts" => {
                    let theme = e.attr("asciiTheme").or(e.attr("hAnsiTheme"));
                    let name = match theme {
                        Some(t) if t.starts_with("major") => Some(self.major_font.clone()),
                        Some(t) if t.starts_with("minor") => Some(self.minor_font.clone()),
                        _ => e.attr("ascii").or(e.attr("hAnsi")).map(|s| s.to_string()),
                    };
                    if let Some(n) = name {
                        f.font = fonts::font_id(&n);
                    }
                }
                "b" => f.bold = on(e),
                "i" => f.italic = on(e),
                "u" => f.underline = on(e),
                "strike" | "dstrike" => f.strike = on(e),
                "caps" => f.caps = on(e),
                "smallCaps" => f.small_caps = on(e),
                "vanish" => f.hidden = on(e),
                "color" => match e.attr("val") {
                    Some("auto") => f.color = BLACK,
                    Some(v) => {
                        if let Some(c) = parse_hex(v) {
                            f.color = c;
                        }
                    }
                    None => {}
                },
                "sz" => {
                    if let Some(v) = e.attr("val").and_then(|v| v.parse::<f32>().ok()) {
                        f.size = (v / 2.0).max(1.0);
                    }
                }
                "spacing" => {
                    if let Some(v) = twips(e, "val") {
                        f.char_spacing = v;
                    }
                }
                "highlight" => {
                    if let Some(v) = e.attr("val") {
                        f.highlight = HIGHLIGHTS
                            .iter()
                            .find(|(n, _)| n.eq_ignore_ascii_case(v))
                            .map(|(_, c)| *c)
                            .unwrap_or(NO_COLOR);
                    }
                }
                "shd" => {
                    if let Some(c) = shd_fill(e) {
                        f.highlight = c;
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

    /// Base character format before any style: Word's built-in defaults.
    fn base_fmt(&self) -> CharFmt {
        let mut f = CharFmt::body();
        f.font = fonts::font_id("Times New Roman");
        f.size = 10.0;
        if let Some(r) = &self.doc_rpr {
            self.apply_rpr(r, &mut f);
        }
        f
    }

    /// Effective format from docDefaults + paragraph style + character style.
    pub fn char_fmt(&self, pstyle: Option<&str>, rstyle: Option<&str>) -> CharFmt {
        let mut f = self.base_fmt();
        for d in self.chain(self.para_style_or_default(pstyle)) {
            if let Some(r) = &d.rpr {
                self.apply_rpr(r, &mut f);
            }
        }
        for d in self.chain(rstyle) {
            if let Some(r) = &d.rpr {
                self.apply_rpr(r, &mut f);
            }
        }
        f
    }

    /// Applies a `pPr` element. `numbering` is used to pull in list indents.
    pub fn apply_ppr(&self, ppr: &El, p: &mut PProps, numbering: &Numbering) {
        // numPr first, so that a direct w:ind below overrides the list indent.
        if let Some(np) = ppr.child("numPr") {
            let id: u32 = np.child_val("numId").and_then(|v| v.parse().ok()).unwrap_or(u32::MAX);
            let il: u8 = np.child_val("ilvl").and_then(|v| v.parse().ok()).unwrap_or(p.num.map(|n| n.1).unwrap_or(0));
            if id == 0 {
                p.num = None;
            } else if id != u32::MAX {
                p.num = Some((id, il.min(8)));
            } else if let Some((n, _)) = p.num {
                p.num = Some((n, il.min(8)));
            }
            if let Some((n, l)) = p.num {
                if let Some(lv) = numbering.level(n, l) {
                    p.ind_left = lv.left;
                    p.ind_first = -lv.hanging;
                }
            }
        }
        for e in ppr.elems() {
            match e.name.as_str() {
                "jc" => {
                    p.align = match e.attr("val") {
                        Some("center") => Align::Center,
                        Some("right" | "end") => Align::Right,
                        Some("both" | "distribute" | "justify" | "lowKashida" | "mediumKashida" | "highKashida" | "thaiDistribute") => Align::Justify,
                        _ => Align::Left,
                    }
                }
                "spacing" => {
                    if let Some(v) = twips(e, "before") {
                        p.before = v;
                    }
                    if let Some(v) = twips(e, "after") {
                        p.after = v;
                    }
                    if e.attr("beforeAutospacing").is_some_and(|v| v == "1" || v == "true") {
                        p.before = 14.0;
                    }
                    if e.attr("afterAutospacing").is_some_and(|v| v == "1" || v == "true") {
                        p.after = 14.0;
                    }
                    if let Some(l) = e.attr("line").and_then(|v| v.parse::<f32>().ok()) {
                        match e.attr("lineRule") {
                            Some("exact") => {
                                p.line_rule = LineRule::Exact;
                                p.line = l / 20.0;
                            }
                            Some("atLeast") => {
                                p.line_rule = LineRule::AtLeast;
                                p.line = l / 20.0;
                            }
                            _ => {
                                p.line_rule = LineRule::Auto;
                                p.line = (l / 240.0).clamp(0.25, 10.0);
                            }
                        }
                    }
                }
                "ind" => {
                    if let Some(v) = twips(e, "left").or(twips(e, "start")) {
                        p.ind_left = v;
                    }
                    if let Some(v) = twips(e, "right").or(twips(e, "end")) {
                        p.ind_right = v;
                    }
                    if let Some(v) = twips(e, "firstLine") {
                        p.ind_first = v;
                    }
                    if let Some(v) = twips(e, "hanging") {
                        p.ind_first = -v;
                    }
                }
                "contextualSpacing" => p.contextual = on(e),
                "pBdr" => p.borders.merge(&parse_edges(e)),
                "pageBreakBefore" => p.page_break_before = on(e),
                "keepNext" => p.keep_next = on(e),
                "keepLines" => p.keep_lines = on(e),
                "widowControl" => p.widow = on(e),
                "shd" => p.shading = shd_fill(e).unwrap_or(NO_COLOR),
                "tabs" => {
                    for t in e.elems().filter(|t| t.name == "tab") {
                        let Some(pos) = twips(t, "pos") else { continue };
                        let kind = match t.attr("val") {
                            Some("center") => TabKind::Center,
                            Some("right" | "end") => TabKind::Right,
                            Some("decimal") => TabKind::Decimal,
                            Some("bar") => TabKind::Bar,
                            Some("clear") => TabKind::Clear,
                            _ => TabKind::Left,
                        };
                        let leader = match t.attr("leader") {
                            Some("dot") => Leader::Dot,
                            Some("hyphen") => Leader::Hyphen,
                            Some("underscore" | "heavy") => Leader::Underscore,
                            Some("middleDot") => Leader::Dot,
                            _ => Leader::None,
                        };
                        p.tabs.retain(|x| (x.pos - pos).abs() > 0.5);
                        if kind != TabKind::Clear {
                            p.tabs.push(TabStop { pos, kind, leader });
                        }
                    }
                    p.tabs.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                }
                _ => {}
            }
        }
    }

    /// Effective paragraph properties from docDefaults + paragraph style chain.
    pub fn para_props(&self, pstyle: Option<&str>, numbering: &Numbering) -> PProps {
        let mut p = PProps::default();
        if let Some(d) = &self.doc_ppr {
            self.apply_ppr(d, &mut p, numbering);
        }
        for d in self.chain(self.para_style_or_default(pstyle)) {
            if let Some(pp) = &d.ppr {
                self.apply_ppr(pp, &mut p, numbering);
            }
        }
        p
    }

    /// Resolves a table style chain (borders, margins, shading, conditional formats).
    pub fn table_style(&self, id: Option<&str>) -> TableStyle {
        let mut ts = TableStyle { row_band: 1, col_band: 1, ..Default::default() };
        let id = id.or_else(|| {
            self.styles.values().find(|s| s.kind == "table" && s.id == "TableNormal").map(|s| s.id.as_str())
        });
        for d in self.chain(id) {
            if let Some(t) = &d.tblpr {
                if let Some(b) = t.child("tblBorders") {
                    ts.edges.merge(&parse_edges(b));
                }
                if let Some(m) = t.child("tblCellMar") {
                    let mut mm = ts.cell_margins.unwrap_or([0.0, 5.4, 0.0, 5.4]);
                    parse_margins(m, &mut mm);
                    ts.cell_margins = Some(mm);
                }
                if let Some(v) = t.child_val("tblStyleRowBandSize").and_then(|v| v.parse().ok()) {
                    ts.row_band = v;
                }
                if let Some(v) = t.child_val("tblStyleColBandSize").and_then(|v| v.parse().ok()) {
                    ts.col_band = v;
                }
            }
            if let Some(s) = d.tcpr.as_ref().and_then(|t| t.child("shd")).and_then(shd_fill) {
                ts.shade = Some(s);
            }
            if let Some(p) = &d.ppr {
                ts.ppr.push(p.clone());
            }
            if let Some(r) = &d.rpr {
                ts.rpr.push(r.clone());
            }
            ts.cond.extend(d.cond.iter().cloned());
        }
        ts.row_band = ts.row_band.max(1);
        ts.col_band = ts.col_band.max(1);
        ts
    }

}
