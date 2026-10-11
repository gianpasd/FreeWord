//! Document model and the primitive editing operations on it.

use crate::fonts;
use crate::numbering::Numbering;
use crate::styles::StyleSheet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

pub type Rgba = [u8; 4];

pub const BLACK: Rgba = [0, 0, 0, 255];
pub const NO_COLOR: Rgba = [0, 0, 0, 0];

/// Object replacement character used for inline pictures and opaque objects.
pub const OBJ: char = '\u{FFFC}';
/// Manual page break (Ctrl+Enter).
pub const PAGE_BREAK: char = '\u{000C}';
/// Manual line break (Shift+Enter).
pub const LINE_BREAK: char = '\u{000B}';
/// Column break (laid out as a line break).
pub const COLUMN_BREAK: char = '\u{000E}';

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CharFmt {
    pub font: u16,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub caps: bool,
    pub small_caps: bool,
    /// `w:vanish` hidden text.
    pub hidden: bool,
    /// Extra letter spacing in points.
    pub char_spacing: f32,
    /// 0 = normal, 1 = superscript, -1 = subscript
    pub script: i8,
    pub color: Rgba,
    /// Alpha 0 means no highlight.
    pub highlight: Rgba,
    /// Image id for `OBJ` characters, 0 otherwise.
    pub image: u32,
    /// Opaque object id (see `pool`) for `OBJ` characters, 0 otherwise.
    pub obj: u32,
    /// Hyperlink id (see `pool::link`), 0 if none.
    pub link: u32,
    /// Interned raw XML of `w:rPr` children FreeWord does not model.
    pub rpr_extra: u32,
}

impl CharFmt {
    pub fn body() -> Self {
        CharFmt {
            font: fonts::font_id(fonts::default_font()),
            size: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            caps: false,
            small_caps: false,
            hidden: false,
            char_spacing: 0.0,
            script: 0,
            color: BLACK,
            highlight: NO_COLOR,
            image: 0,
            obj: 0,
            link: 0,
            rpr_extra: 0,
        }
    }

    /// Format for newly typed text derived from a neighbouring character.
    pub fn without_object(mut self) -> Self {
        self.image = 0;
        self.obj = 0;
        if self.link != 0 {
            self.link = 0;
            self.rpr_extra = 0;
        }
        self
    }

    pub fn is_object(&self) -> bool {
        self.image != 0 || self.obj != 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Style {
    Normal,
    Title,
    Subtitle,
    Heading1,
    Heading2,
    Heading3,
    Quote,
    Code,
}

impl Style {
    pub const ALL: [Style; 8] = [
        Style::Normal,
        Style::Heading1,
        Style::Heading2,
        Style::Heading3,
        Style::Title,
        Style::Subtitle,
        Style::Quote,
        Style::Code,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Style::Normal => "Normal",
            Style::Title => "Title",
            Style::Subtitle => "Subtitle",
            Style::Heading1 => "Heading 1",
            Style::Heading2 => "Heading 2",
            Style::Heading3 => "Heading 3",
            Style::Quote => "Quote",
            Style::Code => "Code",
        }
    }

    /// Word style id used in .docx files.
    pub fn docx_id(self) -> &'static str {
        match self {
            Style::Normal => "Normal",
            Style::Title => "Title",
            Style::Subtitle => "Subtitle",
            Style::Heading1 => "Heading1",
            Style::Heading2 => "Heading2",
            Style::Heading3 => "Heading3",
            Style::Quote => "Quote",
            Style::Code => "Code",
        }
    }

    /// Classifies a Word style (by id and display name) into a FreeWord style.
    pub fn from_docx(id: &str, name: Option<&str>) -> Style {
        let key = name.unwrap_or(id).to_ascii_lowercase().replace(' ', "");
        match key.as_str() {
            "title" => Style::Title,
            "subtitle" => Style::Subtitle,
            "heading1" => Style::Heading1,
            "heading2" => Style::Heading2,
            "heading3" | "heading4" | "heading5" | "heading6" | "heading7" | "heading8" | "heading9" => Style::Heading3,
            "quote" | "intensequote" => Style::Quote,
            "code" | "htmlpreformatted" | "plaintext" => Style::Code,
            _ => Style::Normal,
        }
    }

    /// FreeWord's built-in look for the style (used for new documents).
    pub fn char_fmt(self) -> CharFmt {
        let mut f = CharFmt::body();
        let blue = [0x2F, 0x54, 0x96, 255];
        match self {
            Style::Normal => {}
            Style::Title => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 28.0;
            }
            Style::Subtitle => {
                f.size = 15.0;
                f.color = [0x59, 0x59, 0x59, 255];
            }
            Style::Heading1 => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 16.0;
                f.color = blue;
            }
            Style::Heading2 => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 13.0;
                f.color = blue;
            }
            Style::Heading3 => {
                f.font = fonts::font_id(fonts::heading_font());
                f.size = 12.0;
                f.color = [0x1F, 0x37, 0x63, 255];
            }
            Style::Quote => {
                f.italic = true;
                f.color = [0x40, 0x40, 0x40, 255];
            }
            Style::Code => {
                f.font = fonts::font_id(fonts::mono_font());
                f.size = 10.0;
            }
        }
        f
    }

    /// (space before, space after) in points.
    pub fn spacing(self) -> (f32, f32) {
        match self {
            Style::Normal => (0.0, 8.0),
            Style::Title => (0.0, 4.0),
            Style::Subtitle => (0.0, 8.0),
            Style::Heading1 => (12.0, 4.0),
            Style::Heading2 => (8.0, 4.0),
            Style::Heading3 => (6.0, 2.0),
            Style::Quote => (10.0, 8.0),
            Style::Code => (0.0, 0.0),
        }
    }

    pub fn line_spacing(self) -> f32 {
        match self {
            Style::Title | Style::Code => 1.0,
            _ => 1.15,
        }
    }

    pub fn indents(self) -> (f32, f32) {
        if self == Style::Quote { (36.0, 36.0) } else { (0.0, 0.0) }
    }

    /// Styles after which Enter starts a Normal paragraph.
    pub fn next_is_normal(self) -> bool {
        matches!(self, Style::Title | Style::Subtitle | Style::Heading1 | Style::Heading2 | Style::Heading3)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ListKind {
    None,
    Bullet,
    Number,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LineRule {
    /// `line_spacing` is a multiple of single spacing.
    Auto,
    /// `line_spacing` is a minimum height in points.
    AtLeast,
    /// `line_spacing` is an exact height in points.
    Exact,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TabKind {
    Left,
    Center,
    Right,
    Decimal,
    Bar,
    Clear,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Leader {
    None,
    Dot,
    Hyphen,
    Underscore,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct TabStop {
    /// Position in points from the left margin (or cell edge).
    pub pos: f32,
    pub kind: TabKind,
    pub leader: Leader,
}

/// Location of a paragraph inside a table.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash, PartialOrd, Ord)]
pub struct CellRef {
    pub table: u32,
    pub row: u32,
    /// Grid column where the cell starts.
    pub col: u32,
}

/// What a floating object's horizontal position is measured from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum HRel {
    Page,
    Margin,
    Column,
    Character,
}

/// What a floating object's vertical position is measured from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VRel {
    Page,
    Margin,
    Paragraph,
    Line,
}

/// Position along one axis: an offset in points or an alignment.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum PosSpec {
    Offset(f32),
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Wrap {
    /// In front of / behind text.
    None,
    /// Text keeps clear of the object (square, tight, through, top and bottom).
    Clear,
}

/// Placement of a floating (anchored) picture or shape.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Anchor {
    pub h_rel: HRel,
    pub h_pos: PosSpec,
    pub v_rel: VRel,
    pub v_pos: PosSpec,
    pub wrap: Wrap,
    pub behind: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BorderKind {
    None,
    Single,
    Double,
    Dotted,
    Dashed,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Border {
    pub kind: BorderKind,
    /// Line width in points.
    pub width: f32,
    pub color: Rgba,
    /// Distance from the text in points (paragraph borders).
    pub space: f32,
}

impl Border {
    pub fn single() -> Self {
        Border { kind: BorderKind::Single, width: 0.5, color: BLACK, space: 0.0 }
    }

    pub fn visible(b: &Option<Border>) -> bool {
        b.is_some_and(|b| b.kind != BorderKind::None && b.width > 0.0)
    }
}

/// Border set of a table, cell or paragraph. `None` = not specified;
/// `Some(kind: None)` = explicitly no border.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Edges {
    pub top: Option<Border>,
    pub left: Option<Border>,
    pub bottom: Option<Border>,
    pub right: Option<Border>,
    /// Between rows (tables) / between paragraphs (paragraph borders).
    pub inside_h: Option<Border>,
    pub inside_v: Option<Border>,
}

impl Edges {
    pub fn all(b: Border) -> Self {
        Edges { top: Some(b), left: Some(b), bottom: Some(b), right: Some(b), inside_h: Some(b), inside_v: Some(b) }
    }

    /// Overrides specified edges with those of `o`.
    pub fn merge(&mut self, o: &Edges) {
        for (dst, src) in [
            (&mut self.top, o.top),
            (&mut self.left, o.left),
            (&mut self.bottom, o.bottom),
            (&mut self.right, o.right),
            (&mut self.inside_h, o.inside_h),
            (&mut self.inside_v, o.inside_v),
        ] {
            if src.is_some() {
                *dst = src;
            }
        }
    }

    pub fn any_visible(&self) -> bool {
        [self.top, self.left, self.bottom, self.right, self.inside_h, self.inside_v].iter().any(Border::visible)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum VAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum VMerge {
    #[default]
    None,
    Restart,
    Continue,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CellProps {
    pub span: u32,
    pub vmerge: VMerge,
    pub shade: Option<Rgba>,
    /// Original `w:tcPr` XML, written back with span/merge/shading patched.
    #[serde(default)]
    pub tcpr_raw: Option<String>,
    /// Resolved display borders (table, style and cell borders combined): top, right, bottom, left.
    #[serde(default)]
    pub edges: [Option<Border>; 4],
    /// Shading coming from the table style (display only).
    #[serde(default)]
    pub style_shade: Option<Rgba>,
    /// Cell margins (top, left, bottom, right) in points, if set on the cell.
    #[serde(default)]
    pub margins: Option<[f32; 4]>,
    #[serde(default)]
    pub valign: VAlign,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RowProps {
    /// Minimum (or exact) row height in points; 0 = auto.
    pub height: f32,
    pub exact: bool,
    pub header: bool,
    #[serde(default)]
    pub trpr_raw: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    /// Column widths in points.
    pub grid: Vec<f32>,
    pub borders: bool,
    /// Left indent of the table in points.
    pub indent: f32,
    pub align: Option<Align>,
    #[serde(default)]
    pub cells: BTreeMap<(u32, u32), CellProps>,
    #[serde(default)]
    pub rows: BTreeMap<u32, RowProps>,
    /// Original `w:tblPr` XML.
    #[serde(default)]
    pub tblpr_raw: Option<String>,
    /// Paragraph that holds this table when it is nested in another table's cell.
    #[serde(default)]
    pub nested_raw: bool,
    /// Borders resolved from the table style and `w:tblBorders` (None = use `borders`).
    #[serde(default)]
    pub edges: Option<Edges>,
    /// Default cell margins (top, left, bottom, right) in points.
    #[serde(default = "default_cell_margins")]
    pub cell_margins: [f32; 4],
}

fn default_cell_margins() -> [f32; 4] {
    [0.0, 5.4, 0.0, 5.4]
}

impl Default for Table {
    fn default() -> Self {
        Table {
            grid: Vec::new(),
            borders: false,
            indent: 0.0,
            align: None,
            cells: BTreeMap::new(),
            rows: BTreeMap::new(),
            tblpr_raw: None,
            nested_raw: false,
            edges: None,
            cell_margins: default_cell_margins(),
        }
    }
}

impl Table {
    /// Display borders of the whole table.
    pub fn display_edges(&self) -> Edges {
        match (&self.edges, self.tblpr_raw.is_some()) {
            (Some(e), true) => *e,
            _ if self.borders => Edges::all(Border::single()),
            _ => Edges::default(),
        }
    }
}

impl Table {
    pub fn cell(&self, row: u32, col: u32) -> CellProps {
        self.cells.get(&(row, col)).cloned().unwrap_or(CellProps { span: 1, ..Default::default() })
    }
}

#[derive(Clone, Debug)]
pub struct Para {
    pub chars: Vec<char>,
    pub fmts: Vec<CharFmt>,
    /// FreeWord style category (drives the style gallery).
    pub style: Style,
    /// Word style id (kept for .docx round-trips).
    pub style_id: Option<String>,
    pub align: Align,
    pub list: ListKind,
    /// List level (0-8).
    pub level: u8,
    /// Word numbering instance (`w:numId`), 0 = none.
    pub num_id: u32,
    pub line_spacing: f32,
    pub line_rule: LineRule,
    pub space_before: f32,
    pub space_after: f32,
    /// Ignore spacing between paragraphs of the same style.
    pub contextual: bool,
    pub ind_left: f32,
    pub ind_right: f32,
    /// First-line indent; negative values are a hanging indent.
    pub ind_first: f32,
    pub tabs: Vec<TabStop>,
    pub shading: Rgba,
    /// Paragraph borders for display (the XML lives in the style or `ppr_extra`).
    pub borders: Edges,
    pub page_break_before: bool,
    /// A section ends with this paragraph (raw `w:sectPr` lives in `ppr_extra`).
    pub sect_break: bool,
    /// Raw XML of `w:pPr` children FreeWord does not model.
    pub ppr_extra: String,
    pub cell: Option<CellRef>,
    /// Opaque block-level XML this paragraph stands in for (e.g. nested tables).
    pub raw_block: Option<String>,
    /// Format of the paragraph mark (used when the paragraph is empty).
    pub end_fmt: CharFmt,
}

impl Para {
    pub fn new(style: Style) -> Self {
        let (b, a) = style.spacing();
        let (il, ir) = style.indents();
        Para {
            chars: Vec::new(),
            fmts: Vec::new(),
            style,
            style_id: None,
            align: Align::Left,
            list: ListKind::None,
            level: 0,
            num_id: 0,
            line_spacing: style.line_spacing(),
            line_rule: LineRule::Auto,
            space_before: b,
            space_after: a,
            contextual: false,
            ind_left: il,
            ind_right: ir,
            ind_first: 0.0,
            tabs: Vec::new(),
            shading: NO_COLOR,
            borders: Edges::default(),
            page_break_before: false,
            sect_break: false,
            ppr_extra: String::new(),
            cell: None,
            raw_block: None,
            end_fmt: style.char_fmt(),
        }
    }

    pub fn with_text(text: &str, style: Style) -> Self {
        let mut p = Para::new(style);
        let f = style.char_fmt();
        p.chars = text.chars().collect();
        p.fmts = vec![f; p.chars.len()];
        p
    }

    /// Copy of the paragraph properties without text.
    pub fn empty_like(&self) -> Self {
        let mut p = Para { chars: Vec::new(), fmts: Vec::new(), ..self.clone() };
        p.raw_block = None;
        p.page_break_before = false;
        if p.sect_break {
            // The section break stays with the original paragraph.
            p.sect_break = false;
            p.ppr_extra = strip_sect_pr(&p.ppr_extra);
        }
        p
    }

    pub fn copy_props_from(&mut self, o: &Para) {
        let (chars, fmts, end, cell) = (
            std::mem::take(&mut self.chars),
            std::mem::take(&mut self.fmts),
            self.end_fmt,
            self.cell,
        );
        *self = Para { chars, fmts, end_fmt: end, cell, raw_block: None, ..o.clone() };
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Format that newly typed text at offset `o` should get.
    pub fn fmt_at(&self, o: usize) -> CharFmt {
        let f = if o > 0 && o <= self.fmts.len() {
            self.fmts[o - 1]
        } else if !self.fmts.is_empty() {
            self.fmts[0]
        } else {
            self.end_fmt
        };
        f.without_object()
    }
}

/// The `w:sectPr` element inside raw pPr XML, if any.
pub fn extract_sect_pr(raw: &str) -> Option<String> {
    let s = raw.find("<w:sectPr")?;
    if let Some(e) = raw[s..].find("</w:sectPr>") {
        return Some(raw[s..s + e + "</w:sectPr>".len()].to_string());
    }
    // Self-closing form.
    let e = raw[s..].find("/>")?;
    Some(raw[s..s + e + 2].to_string())
}

/// Removes a `w:sectPr` element from raw pPr XML.
pub fn strip_sect_pr(raw: &str) -> String {
    if let (Some(s), Some(e)) = (raw.find("<w:sectPr"), raw.rfind("</w:sectPr>")) {
        let mut out = raw[..s].to_string();
        out.push_str(&raw[e + "</w:sectPr>".len()..]);
        return out;
    }
    raw.to_string()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub p: usize,
    pub o: usize,
}

impl Pos {
    pub fn new(p: usize, o: usize) -> Self {
        Pos { p, o }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageSetup {
    /// Points.
    pub width: f32,
    pub height: f32,
    /// top, right, bottom, left in points.
    pub margins: [f32; 4],
    /// Distance of header/footer text from the page edge.
    #[serde(default = "default_hf")]
    pub header_dist: f32,
    #[serde(default = "default_hf")]
    pub footer_dist: f32,
    /// Default tab stop interval.
    #[serde(default = "default_tab")]
    pub default_tab: f32,
}

fn default_hf() -> f32 {
    36.0
}
fn default_tab() -> f32 {
    36.0
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup { width: 612.0, height: 792.0, margins: [72.0; 4], header_dist: 36.0, footer_dist: 36.0, default_tab: 36.0 }
    }
}

#[derive(Clone, Debug)]
pub struct ImageData {
    pub bytes: Arc<Vec<u8>>,
    pub px_w: u32,
    pub px_h: u32,
    /// Display size in points.
    pub w: f32,
    pub h: f32,
    /// Original drawing XML (anchoring, effects...) from a .docx, and the
    /// package it came from.
    pub raw: Option<(String, u64)>,
    /// Floating placement; None for inline pictures.
    pub anchor: Option<Anchor>,
}

/// The original .docx package a document was opened from.
#[derive(Clone, Debug, Default)]
pub struct Package {
    pub id: u64,
    /// Every part of the original zip, by name.
    pub parts: BTreeMap<String, Vec<u8>>,
    /// Attributes of the original `w:document` element (namespace declarations).
    pub root_attrs: Vec<(String, String)>,
    /// Original body-level `w:sectPr`.
    pub body_sect: Option<String>,
    /// Header/footer text as displayed when the document was opened.
    pub header_text: String,
    pub footer_text: String,
    pub page_numbers: bool,
    /// Original body elements that precede the first paragraph we model (e.g.
    /// nothing) are not needed; background etc. are preserved here.
    pub background: Option<String>,
}

/// Rich header/footer content read from a .docx (laid out like the body).
#[derive(Clone, Debug, Default)]
pub struct HeaderFooter {
    pub header: Option<Box<Document>>,
    pub header_first: Option<Box<Document>>,
    pub footer: Option<Box<Document>>,
    pub footer_first: Option<Box<Document>>,
    /// Different first page (`w:titlePg`).
    pub title_page: bool,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub paras: Vec<Para>,
    /// Rich headers/footers for each section (in document order; the last is the body section).
    pub hf: Arc<Vec<HeaderFooter>>,
    /// Footnote (false) and endnote (true) texts by w:id.
    pub notes: Arc<BTreeMap<(bool, String), Document>>,
    pub page: PageSetup,
    pub header: String,
    pub footer: String,
    pub page_numbers: bool,
    pub images: BTreeMap<u32, ImageData>,
    pub tables: BTreeMap<u32, Table>,
    pub numbering: Numbering,
    pub styles: Arc<StyleSheet>,
    pub pkg: Option<Arc<Package>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> CharClass {
    if c.is_whitespace() || c == PAGE_BREAK || c == LINE_BREAK || c == COLUMN_BREAK {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' || c == '\'' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

impl Document {
    pub fn new() -> Self {
        Document {
            paras: vec![Para::new(Style::Normal)],
            hf: Arc::new(Vec::new()),
            notes: Arc::new(BTreeMap::new()),
            page: PageSetup::default(),
            header: String::new(),
            footer: String::new(),
            page_numbers: false,
            images: BTreeMap::new(),
            tables: BTreeMap::new(),
            numbering: Numbering::new_default(),
            styles: Arc::new(StyleSheet::generated()),
            pkg: None,
        }
    }

    /// Every header/footer and note document.
    pub fn hf_docs(&self) -> impl Iterator<Item = &Document> {
        self.hf
            .iter()
            .flat_map(|h| [&h.header, &h.header_first, &h.footer, &h.footer_first])
            .flatten()
            .map(|b| b.as_ref())
            .chain(self.notes.values())
    }

    /// Raw `w:sectPr` of every section in order (paragraph-level ones, then the body's).
    pub fn section_props(&self) -> Vec<String> {
        let mut v: Vec<String> = self.paras.iter().filter(|p| p.sect_break).filter_map(|p| extract_sect_pr(&p.ppr_extra)).collect();
        if let Some(b) = self.pkg.as_ref().and_then(|p| p.body_sect.clone()) {
            v.push(b);
        }
        v
    }

    /// All image stores (body plus header/footer documents).
    pub fn all_images(&self) -> Vec<(u32, &ImageData)> {
        let mut v: Vec<(u32, &ImageData)> = self.images.iter().map(|(k, v)| (*k, v)).collect();
        for d in self.hf_docs() {
            v.extend(d.images.iter().map(|(k, v)| (*k, v)));
        }
        v
    }

    pub fn pkg_id(&self) -> u64 {
        self.pkg.as_ref().map(|p| p.id).unwrap_or(0)
    }

    pub fn next_image_id(&self) -> u32 {
        self.images.keys().next_back().copied().unwrap_or(0) + 1
    }

    pub fn next_table_id(&self) -> u32 {
        self.tables.keys().next_back().copied().unwrap_or(0) + 1
    }

    pub fn end(&self) -> Pos {
        let p = self.paras.len() - 1;
        Pos::new(p, self.paras[p].len())
    }

    pub fn clamp(&self, pos: Pos) -> Pos {
        let p = pos.p.min(self.paras.len() - 1);
        Pos::new(p, pos.o.min(self.paras[p].len()))
    }

    pub fn prev(&self, pos: Pos) -> Pos {
        if pos.o > 0 {
            Pos::new(pos.p, pos.o - 1)
        } else if pos.p > 0 {
            Pos::new(pos.p - 1, self.paras[pos.p - 1].len())
        } else {
            pos
        }
    }

    pub fn next(&self, pos: Pos) -> Pos {
        if pos.o < self.paras[pos.p].len() {
            Pos::new(pos.p, pos.o + 1)
        } else if pos.p + 1 < self.paras.len() {
            Pos::new(pos.p + 1, 0)
        } else {
            pos
        }
    }

    pub fn word_left(&self, pos: Pos) -> Pos {
        if pos.o == 0 {
            return self.prev(pos);
        }
        let c = &self.paras[pos.p].chars;
        let mut o = pos.o;
        while o > 0 && class(c[o - 1]) == CharClass::Space {
            o -= 1;
        }
        if o > 0 {
            let k = class(c[o - 1]);
            while o > 0 && class(c[o - 1]) == k {
                o -= 1;
            }
        }
        Pos::new(pos.p, o)
    }

    pub fn word_right(&self, pos: Pos) -> Pos {
        let c = &self.paras[pos.p].chars;
        if pos.o >= c.len() {
            return self.next(pos);
        }
        let mut o = pos.o;
        let k = class(c[o]);
        if k != CharClass::Space {
            while o < c.len() && class(c[o]) == k {
                o += 1;
            }
        }
        while o < c.len() && class(c[o]) == CharClass::Space {
            o += 1;
        }
        Pos::new(pos.p, o)
    }

    /// Range of the word (or run of same-class characters) around `pos`.
    pub fn word_at(&self, pos: Pos) -> (Pos, Pos) {
        let c = &self.paras[pos.p].chars;
        if c.is_empty() {
            return (pos, pos);
        }
        let i = pos.o.min(c.len() - 1);
        let k = class(c[i]);
        let mut a = i;
        while a > 0 && class(c[a - 1]) == k {
            a -= 1;
        }
        let mut b = i;
        while b < c.len() && class(c[b]) == k {
            b += 1;
        }
        if k == CharClass::Word {
            while b < c.len() && c[b] == ' ' {
                b += 1;
            }
        }
        (Pos::new(pos.p, a), Pos::new(pos.p, b))
    }

    /// Paragraph properties for a new paragraph after `p` (Enter at its end).
    fn next_para_props(&self, p: &Para) -> Para {
        let mut np = p.empty_like();
        if p.style.next_is_normal() {
            let normal_id = self.styles.default_para.clone();
            self.apply_style_props(&mut np, normal_id.as_deref(), Style::Normal);
            np.end_fmt = self.style_char_fmt(np.style_id.as_deref(), Style::Normal);
        }
        np
    }

    /// Splits the paragraph at `pos`, returning the start of the new paragraph.
    pub fn split(&mut self, pos: Pos) -> Pos {
        let at_end = pos.o == self.paras[pos.p].len() && !self.paras[pos.p].chars.is_empty();
        let template = if at_end { Some(self.next_para_props(&self.paras[pos.p])) } else { None };
        let para = &mut self.paras[pos.p];
        let tail_chars = para.chars.split_off(pos.o);
        let tail_fmts = para.fmts.split_off(pos.o);
        let mut np = template.unwrap_or_else(|| para.empty_like());
        if !para.style.next_is_normal() || !at_end {
            np.end_fmt = para.fmt_at(para.len());
        }
        np.chars = tail_chars;
        np.fmts = tail_fmts;
        if para.chars.is_empty() && !np.chars.is_empty() {
            para.end_fmt = np.fmts[0].without_object();
        }
        // A section break belongs to the last paragraph of the section.
        if para.sect_break {
            let raw = para.ppr_extra.clone();
            para.sect_break = false;
            para.ppr_extra = strip_sect_pr(&raw);
            np.sect_break = true;
            np.ppr_extra = raw;
        }
        self.paras.insert(pos.p + 1, np);
        Pos::new(pos.p + 1, 0)
    }

    /// Inserts text (which may contain newlines) with a single format.
    pub fn insert_text(&mut self, pos: Pos, text: &str, fmt: CharFmt) -> Pos {
        let mut pos = pos;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                pos = self.split(pos);
            }
            let chars: Vec<char> = line
                .chars()
                .filter(|c| *c == '\t' || !c.is_control() || *c == PAGE_BREAK || *c == LINE_BREAK)
                .collect();
            let n = chars.len();
            let para = &mut self.paras[pos.p];
            para.chars.splice(pos.o..pos.o, chars);
            para.fmts.splice(pos.o..pos.o, std::iter::repeat_n(fmt, n));
            pos.o += n;
        }
        pos
    }

    /// Whether a range crosses table cell boundaries.
    fn crosses_cells(&self, a: Pos, b: Pos) -> bool {
        let c0 = self.paras[a.p].cell;
        (a.p..=b.p).any(|p| self.paras[p].cell != c0)
    }

    pub fn delete(&mut self, a: Pos, b: Pos) {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            return;
        }
        if a.p == b.p {
            let para = &mut self.paras[a.p];
            para.chars.drain(a.o..b.o);
            para.fmts.drain(a.o..b.o);
            return;
        }
        if self.crosses_cells(a, b) {
            // Never merge paragraphs across cells: clear text instead, and only
            // remove whole paragraphs that live outside tables.
            {
                let pa = &mut self.paras[a.p];
                pa.chars.truncate(a.o);
                pa.fmts.truncate(a.o);
            }
            {
                let pb = &mut self.paras[b.p];
                pb.chars.drain(..b.o);
                pb.fmts.drain(..b.o);
            }
            for p in a.p + 1..b.p {
                self.paras[p].chars.clear();
                self.paras[p].fmts.clear();
            }
            let mut p = b.p - 1;
            while p > a.p {
                if self.paras[p].cell.is_none() {
                    self.paras.remove(p);
                }
                p -= 1;
            }
            return;
        }
        let last = self.paras[b.p].clone();
        let first = &mut self.paras[a.p];
        first.chars.truncate(a.o);
        first.fmts.truncate(a.o);
        if first.chars.is_empty() && a.o == 0 && b.o == 0 {
            first.copy_props_from(&last);
            first.end_fmt = last.end_fmt;
        }
        first.chars.extend_from_slice(&last.chars[b.o..]);
        first.fmts.extend_from_slice(&last.fmts[b.o..]);
        if b.o < last.len() || first.chars.is_empty() {
            first.end_fmt = last.end_fmt;
        }
        if last.sect_break {
            first.sect_break = true;
            first.ppr_extra = last.ppr_extra.clone();
        }
        self.paras.drain(a.p + 1..=b.p);
    }

    pub fn at_cell_end(&self, pos: Pos) -> bool {
        pos.o == self.paras[pos.p].len()
            && pos.p + 1 < self.paras.len()
            && self.paras[pos.p].cell != self.paras[pos.p + 1].cell
    }

    /// Copies a range as a list of paragraphs (first and last may be partial).
    pub fn fragment(&self, a: Pos, b: Pos) -> Vec<Para> {
        let mut out = Vec::new();
        for p in a.p..=b.p {
            let para = &self.paras[p];
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            let mut np = para.empty_like();
            np.cell = None;
            np.chars = para.chars[s..e].to_vec();
            np.fmts = para.fmts[s..e].to_vec();
            out.push(np);
        }
        out
    }

    pub fn insert_fragment(&mut self, pos: Pos, frag: &[Para]) -> Pos {
        if frag.is_empty() {
            return pos;
        }
        if frag.len() == 1 {
            let para = &mut self.paras[pos.p];
            let n = frag[0].len();
            para.chars.splice(pos.o..pos.o, frag[0].chars.iter().copied());
            para.fmts.splice(pos.o..pos.o, frag[0].fmts.iter().copied());
            return Pos::new(pos.p, pos.o + n);
        }
        let cell = self.paras[pos.p].cell;
        let right = self.split(pos);
        {
            let left = &mut self.paras[pos.p];
            if left.chars.is_empty() {
                left.copy_props_from(&frag[0]);
                left.end_fmt = frag[0].end_fmt;
            }
            left.cell = cell;
            left.chars.extend_from_slice(&frag[0].chars);
            left.fmts.extend_from_slice(&frag[0].fmts);
        }
        let middle = &frag[1..frag.len() - 1];
        for (i, m) in middle.iter().enumerate() {
            let mut m = m.clone();
            m.cell = cell;
            self.paras.insert(right.p + i, m);
        }
        let r = right.p + middle.len();
        let last = frag.last().unwrap();
        let rp = &mut self.paras[r];
        if rp.chars.is_empty() {
            rp.copy_props_from(last);
            rp.end_fmt = last.end_fmt;
        }
        rp.cell = cell;
        rp.chars.splice(0..0, last.chars.iter().copied());
        rp.fmts.splice(0..0, last.fmts.iter().copied());
        Pos::new(r, last.len())
    }

    pub fn plain_text(&self, a: Pos, b: Pos) -> String {
        let mut s = String::new();
        for p in a.p..=b.p {
            let para = &self.paras[p];
            let st = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            for (k, &c) in para.chars[st..e].iter().enumerate() {
                if para.fmts[st + k].hidden {
                    continue;
                }
                match c {
                    OBJ => {}
                    LINE_BREAK | PAGE_BREAK | COLUMN_BREAK => s.push('\n'),
                    c => s.push(c),
                }
            }
            if p != b.p {
                let next_cell = self.paras[p + 1].cell;
                if para.cell.is_some() && next_cell.is_some() && para.cell != next_cell {
                    s.push(if next_cell.map(|c| c.row) == para.cell.map(|c| c.row) { '\t' } else { '\n' });
                } else {
                    s.push('\n');
                }
            }
        }
        s
    }

    /// Applies `f` to every character format in `[a, b)`.
    pub fn apply_fmt(&mut self, a: Pos, b: Pos, f: &dyn Fn(&mut CharFmt)) {
        for p in a.p..=b.p {
            let para = &mut self.paras[p];
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            for fm in &mut para.fmts[s..e] {
                f(fm);
            }
            if p < b.p || e == para.len() {
                f(&mut para.end_fmt);
                para.end_fmt.image = 0;
                para.end_fmt.obj = 0;
            }
        }
    }

    pub fn find_all(&self, query: &str, match_case: bool, whole_word: bool) -> Vec<(Pos, Pos)> {
        let fold = |c: char| if match_case { c } else { c.to_lowercase().next().unwrap_or(c) };
        let q: Vec<char> = query.chars().map(fold).collect();
        let mut out = Vec::new();
        if q.is_empty() {
            return out;
        }
        for (pi, para) in self.paras.iter().enumerate() {
            let c = &para.chars;
            if c.len() < q.len() {
                continue;
            }
            let mut i = 0;
            while i + q.len() <= c.len() {
                if (0..q.len()).all(|k| fold(c[i + k]) == q[k]) {
                    let ok = !whole_word
                        || ((i == 0 || class(c[i - 1]) != CharClass::Word)
                            && (i + q.len() == c.len() || class(c[i + q.len()]) != CharClass::Word));
                    if ok {
                        out.push((Pos::new(pi, i), Pos::new(pi, i + q.len())));
                        i += q.len();
                        continue;
                    }
                }
                i += 1;
            }
        }
        out
    }

    pub fn stats(&self) -> Stats {
        let mut st = Stats::default();
        for p in &self.paras {
            let t: String = p
                .chars
                .iter()
                .zip(&p.fmts)
                .filter(|(c, f)| **c != OBJ && !f.hidden)
                .map(|(c, _)| *c)
                .collect();
            if !t.trim().is_empty() {
                st.paragraphs += 1;
            }
            st.words += t.split_whitespace().count();
            st.chars += t.chars().filter(|c| !c.is_control()).count();
            st.chars_no_spaces += t.chars().filter(|c| !c.is_whitespace() && !c.is_control()).count();
        }
        st
    }

    /// Drops image data no longer referenced by any character.
    pub fn gc_images(&mut self) {
        let mut used = std::collections::BTreeSet::new();
        for p in &self.paras {
            for f in &p.fmts {
                if f.image != 0 {
                    used.insert(f.image);
                }
            }
        }
        self.images.retain(|k, _| used.contains(k));
    }

    /// List labels for every paragraph.
    pub fn list_labels(&self) -> Vec<Option<String>> {
        self.numbering.labels(self.paras.iter().map(|p| {
            if p.list != ListKind::None && p.num_id != 0 { Some((p.num_id, p.level)) } else { None }
        }))
    }

    /// Effective character format of a style in this document.
    pub fn style_char_fmt(&self, style_id: Option<&str>, fallback: Style) -> CharFmt {
        match style_id {
            Some(id) if self.styles.styles.contains_key(id) => self.styles.char_fmt(Some(id), None),
            _ => fallback.char_fmt(),
        }
    }

    /// Sets paragraph properties from a style (keeps text and table cell).
    pub fn apply_style_props(&self, p: &mut Para, style_id: Option<&str>, style: Style) {
        p.style = style;
        p.style_id = style_id.map(|s| s.to_string());
        let known = style_id.is_some_and(|id| self.styles.styles.contains_key(id));
        if known {
            let pp = self.styles.para_props(style_id, &self.numbering);
            p.align = pp.align;
            p.space_before = pp.before;
            p.space_after = pp.after;
            p.line_spacing = pp.line;
            p.line_rule = pp.line_rule;
            p.contextual = pp.contextual;
            p.ind_left = pp.ind_left;
            p.ind_right = pp.ind_right;
            p.ind_first = pp.ind_first;
            p.tabs = pp.tabs;
            p.shading = pp.shading;
            p.borders = pp.borders;
            match pp.num {
                Some((n, l)) => {
                    p.num_id = n;
                    p.level = l;
                    p.list = if self.numbering.is_bullet(n, l) { ListKind::Bullet } else { ListKind::Number };
                }
                None => {
                    p.num_id = 0;
                    p.list = ListKind::None;
                }
            }
        } else {
            let (b, a) = style.spacing();
            let (il, ir) = style.indents();
            p.space_before = b;
            p.space_after = a;
            p.line_spacing = style.line_spacing();
            p.line_rule = LineRule::Auto;
            p.ind_left = il;
            p.ind_right = ir;
            p.ind_first = 0.0;
        }
    }

    /// Applies a style to a paragraph, resetting its character formatting.
    pub fn set_para_style(&self, p: &mut Para, style: Style) {
        let id = self
            .styles
            .id_for_name(style.name())
            .or_else(|| self.styles.styles.contains_key(style.docx_id()).then(|| style.docx_id().to_string()))
            .unwrap_or_else(|| style.docx_id().to_string());
        self.apply_style_props(p, Some(&id), style);
        let base = self.style_char_fmt(Some(&id), style);
        for f in &mut p.fmts {
            let keep = *f;
            *f = base;
            f.image = keep.image;
            f.obj = keep.obj;
            f.link = keep.link;
            f.hidden = keep.hidden;
        }
        p.end_fmt = base;
    }

    /// Sets the list indentation of a paragraph from its numbering definition.
    pub fn apply_list_indent(&self, p: &mut Para) {
        if let Some(l) = self.numbering.level(p.num_id, p.level) {
            p.ind_left = l.left;
            p.ind_first = -l.hanging;
        }
    }

    /// Ensures the document does not end inside a table (Word requires a
    /// paragraph after the last table).
    pub fn ensure_trailing_para(&mut self) {
        if self.paras.last().is_some_and(|p| p.cell.is_some()) {
            let mut p = Para::new(Style::Normal);
            p.style_id = self.styles.default_para.clone();
            p.end_fmt = self.style_char_fmt(p.style_id.as_deref(), Style::Normal);
            self.paras.push(p);
        }
    }

    /// Paragraph index range of the cell containing paragraph `p`.
    pub fn cell_range(&self, p: usize) -> std::ops::Range<usize> {
        let c = self.paras[p].cell;
        let mut s = p;
        while s > 0 && self.paras[s - 1].cell == c {
            s -= 1;
        }
        let mut e = p + 1;
        while e < self.paras.len() && self.paras[e].cell == c {
            e += 1;
        }
        s..e
    }

    /// Paragraph index range of a whole table.
    pub fn table_range(&self, table: u32) -> std::ops::Range<usize> {
        let s = self.paras.iter().position(|p| p.cell.is_some_and(|c| c.table == table)).unwrap_or(0);
        let mut e = s;
        while e < self.paras.len() && self.paras[e].cell.is_some_and(|c| c.table == table) {
            e += 1;
        }
        s..e
    }
}

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub words: usize,
    pub chars: usize,
    pub chars_no_spaces: usize,
    pub paragraphs: usize,
}
