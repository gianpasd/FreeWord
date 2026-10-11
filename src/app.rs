//! Application state and editing commands.

use crate::doc::{
    Align, CharFmt, Document, ImageData, LINE_BREAK, ListKind, OBJ, PAGE_BREAK, Para, Pos, Style,
};
use crate::layout::{self, Layout};
use crate::{docx, io, pdf};
use eframe::egui::{self, TextureHandle};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Insert,
    Layout,
    Review,
    View,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Delete,
    Other,
}

#[derive(Clone)]
struct Snap {
    doc: Document,
    cursor: Pos,
    anchor: Pos,
}

pub struct Clip {
    pub text: String,
    pub paras: Vec<Para>,
    pub images: BTreeMap<u32, ImageData>,
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub replace: bool,
    pub query: String,
    pub replacement: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub matches: Vec<(Pos, Pos)>,
    pub current: Option<usize>,
    pub focus_query: bool,
    pub searched_rev: u64,
    pub searched_query: String,
}

#[derive(Default)]
pub struct Dialogs {
    pub word_count: bool,
    pub symbols: bool,
    pub header_footer: bool,
    pub page_setup: bool,
    pub paragraph: bool,
    pub about: bool,
    pub shortcuts: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum After {
    New,
    Open,
    Exit,
}

#[derive(Default)]
pub struct Drag {
    pub selecting: bool,
    pub word_mode: Option<(Pos, Pos)>,
    /// (image id, start width pt, pointer start)
    pub resizing: Option<(u32, f32, egui::Pos2)>,
}

pub struct App {
    pub ctx: egui::Context,
    pub doc: Document,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub cursor: Pos,
    pub anchor: Pos,
    pub goal_x: Option<f32>,
    pub pending_fmt: Option<CharFmt>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last_kind: EditKind,
    last_edit: Instant,
    pub rev: u64,
    pub layout: Option<Layout>,
    layout_key: (u64, u32),
    pub zoom: f32,
    pub show_marks: bool,
    pub show_ruler: bool,
    pub dark: bool,
    pub tab: Tab,
    pub find: Find,
    pub dlg: Dialogs,
    pub clip: Option<Clip>,
    pub format_painter: Option<(CharFmt, Para)>,
    pub textures: HashMap<u32, TextureHandle>,
    pub scroll_to_caret: bool,
    pub blink: Instant,
    pub text_input_focused: bool,
    pub drag: Drag,
    pub status: Option<(String, Instant)>,
    pub confirm: Option<After>,
    pub allow_close: bool,
    pub last_title: String,
    pub last_click: (f64, egui::Pos2, u32),
    pub fit_request: Option<bool>,
    pub last_color: crate::doc::Rgba,
    pub last_highlight: crate::doc::Rgba,
    /// Set when a dialog text field had keyboard focus during this frame.
    pub text_focus_frame: bool,
    pub viewport_h: f32,
}

/// Loads a document by extension.
pub fn load_any(path: &Path) -> Result<Document, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "docx" => docx::load_docx(path),
        "fwd" => io::load_native(path),
        _ => io::load_text(path),
    }
}

/// Headless conversion: `freeword --convert input output`.
pub fn convert(input: &Path, output: &Path) -> Result<(), String> {
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    // One empty frame so the font system is initialised.
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    let doc = load_any(input)?;
    let ext = output.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "pdf" => {
            let lay = ctx.fonts_mut(|f| layout::layout(&doc, f, pdf::PDF_ZOOM));
            pdf::export(&doc, &lay, output)
        }
        "fwd" => io::save_native(&doc, output),
        "txt" => io::save_text(&doc, output),
        "html" | "htm" => io::save_html(&doc, output, "Document"),
        _ => docx::save_docx(&doc, output),
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut app = App {
            ctx: cc.egui_ctx.clone(),
            doc: Document::new(),
            path: None,
            dirty: false,
            cursor: Pos::default(),
            anchor: Pos::default(),
            goal_x: None,
            pending_fmt: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_kind: EditKind::Other,
            last_edit: Instant::now(),
            rev: 1,
            layout: None,
            layout_key: (0, 0),
            zoom: 1.0,
            show_marks: false,
            show_ruler: true,
            dark: false,
            tab: Tab::Home,
            find: Find::default(),
            dlg: Dialogs::default(),
            clip: None,
            format_painter: None,
            textures: HashMap::new(),
            scroll_to_caret: false,
            blink: Instant::now(),
            text_input_focused: false,
            drag: Drag::default(),
            status: None,
            confirm: None,
            allow_close: false,
            last_title: String::new(),
            last_click: (0.0, egui::Pos2::ZERO, 0),
            fit_request: None,
            last_color: [0xC0, 0, 0, 255],
            last_highlight: [255, 255, 0, 255],
            text_focus_frame: false,
            viewport_h: 600.0,
        };
        crate::ui::apply_theme(&cc.egui_ctx, false);
        if let Some(arg) = std::env::args().nth(1) {
            app.open_path(Path::new(&arg));
        }
        app
    }

    // ------------------------------------------------------------ basics

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Document1".into())
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), Instant::now()));
    }

    pub fn error(&mut self, s: impl Into<String>) {
        self.dlg.error = Some(s.into());
    }

    pub fn sel(&self) -> (Pos, Pos) {
        if self.anchor <= self.cursor { (self.anchor, self.cursor) } else { (self.cursor, self.anchor) }
    }

    pub fn has_sel(&self) -> bool {
        self.anchor != self.cursor
    }

    pub fn moved(&mut self) {
        self.blink = Instant::now();
        self.scroll_to_caret = true;
    }

    pub fn set_cursor(&mut self, pos: Pos, extend: bool) {
        self.cursor = self.doc.clamp(pos);
        if !extend {
            self.anchor = self.cursor;
        }
        self.pending_fmt = None;
        self.moved();
    }

    pub fn select(&mut self, a: Pos, b: Pos) {
        self.anchor = self.doc.clamp(a);
        self.cursor = self.doc.clamp(b);
        self.pending_fmt = None;
        self.moved();
    }

    /// Records an undo point before a mutation.
    pub fn checkpoint(&mut self, kind: EditKind) {
        let now = Instant::now();
        let coalesce = kind != EditKind::Other
            && kind == self.last_kind
            && now.duration_since(self.last_edit) < Duration::from_millis(1500);
        if !coalesce {
            self.undo.push(Snap { doc: self.doc.clone(), cursor: self.cursor, anchor: self.anchor });
            if self.undo.len() > 300 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_kind = kind;
        self.last_edit = now;
        self.dirty = true;
        self.rev += 1;
        self.moved();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let cur = Snap { doc: std::mem::replace(&mut self.doc, s.doc), cursor: self.cursor, anchor: self.anchor };
            self.redo.push(cur);
            self.cursor = self.doc.clamp(s.cursor);
            self.anchor = self.doc.clamp(s.anchor);
            self.after_history();
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let cur = Snap { doc: std::mem::replace(&mut self.doc, s.doc), cursor: self.cursor, anchor: self.anchor };
            self.undo.push(cur);
            self.cursor = self.doc.clamp(s.cursor);
            self.anchor = self.doc.clamp(s.anchor);
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.last_kind = EditKind::Other;
        self.dirty = true;
        self.rev += 1;
        self.pending_fmt = None;
        self.moved();
    }

    /// Returns an up-to-date layout, recomputing it if the document or zoom changed.
    pub fn ensure_layout(&mut self) {
        let key = (self.rev, self.zoom.to_bits());
        if self.layout.is_none() || self.layout_key != key {
            let doc = &self.doc;
            let zoom = self.zoom;
            let lay = self.ctx.fonts_mut(|f| layout::layout(doc, f, zoom));
            self.layout = Some(lay);
            self.layout_key = key;
        }
    }

    // ------------------------------------------------------------ formatting queries

    /// Format shown in the ribbon (and used for typing).
    pub fn current_fmt(&self) -> CharFmt {
        if let Some(f) = self.pending_fmt {
            return f;
        }
        if self.has_sel() {
            let (a, _) = self.sel();
            let p = &self.doc.paras[a.p];
            if a.o < p.len() {
                return p.fmts[a.o].without_object();
            }
            return p.end_fmt;
        }
        self.doc.paras[self.cursor.p].fmt_at(self.cursor.o)
    }

    pub fn current_para(&self) -> &Para {
        &self.doc.paras[self.cursor.p]
    }

    // ------------------------------------------------------------ text editing

    pub fn delete_selection(&mut self) -> bool {
        if !self.has_sel() {
            return false;
        }
        let (a, b) = self.sel();
        self.doc.delete(a, b);
        self.cursor = a;
        self.anchor = a;
        true
    }

    pub fn insert_typed(&mut self, text: &str) {
        if text.is_empty() || text.chars().all(|c| c.is_control() && c != '\t') {
            return;
        }
        let fmt = self.current_fmt();
        self.checkpoint(if text == " " { EditKind::Other } else { EditKind::Typing });
        self.delete_selection();
        self.cursor = self.doc.insert_text(self.cursor, text, fmt);
        self.anchor = self.cursor;
        self.pending_fmt = None;
        if text == " " {
            self.autoformat_list();
        }
    }

    /// Word-style AutoFormat: "* " or "- " starts a bullet list, "1. " a numbered list.
    fn autoformat_list(&mut self) {
        let p = &self.doc.paras[self.cursor.p];
        if p.list != ListKind::None || self.cursor.o != p.len() {
            return;
        }
        let t = p.text();
        let kind = match t.as_str() {
            "* " | "- " => ListKind::Bullet,
            "1. " | "1) " => ListKind::Number,
            _ => return,
        };
        let p = &mut self.doc.paras[self.cursor.p];
        p.end_fmt = p.fmt_at(0);
        p.chars.clear();
        p.fmts.clear();
        p.list = kind;
        self.cursor.o = 0;
        self.anchor = self.cursor;
        self.rev += 1;
    }

    pub fn insert_special(&mut self, c: char) {
        let fmt = self.current_fmt();
        self.checkpoint(EditKind::Other);
        self.delete_selection();
        self.cursor = self.doc.insert_text(self.cursor, &c.to_string(), fmt);
        self.anchor = self.cursor;
    }

    pub fn enter(&mut self) {
        self.checkpoint(EditKind::Other);
        self.delete_selection();
        let p = &self.doc.paras[self.cursor.p];
        // Enter on an empty list item ends the list.
        if p.list != ListKind::None && p.chars.is_empty() {
            let p = &mut self.doc.paras[self.cursor.p];
            if p.level > 0 {
                p.level -= 1;
            } else {
                p.list = ListKind::None;
            }
            return;
        }
        self.cursor = self.doc.split(self.cursor);
        self.anchor = self.cursor;
    }

    pub fn backspace(&mut self, word: bool) {
        if self.has_sel() {
            self.checkpoint(EditKind::Delete);
            self.delete_selection();
            return;
        }
        let c = self.cursor;
        if c.o == 0 {
            let p = &mut self.doc.paras[c.p];
            // First remove list formatting / indentation, like Word.
            if p.list != ListKind::None || p.level > 0 {
                self.checkpoint(EditKind::Other);
                let p = &mut self.doc.paras[c.p];
                if p.list != ListKind::None {
                    p.list = ListKind::None;
                } else {
                    p.level -= 1;
                }
                return;
            }
            if c.p == 0 {
                return;
            }
        }
        let start = if word { self.doc.word_left(c) } else { self.doc.prev(c) };
        self.checkpoint(EditKind::Delete);
        self.doc.delete(start, c);
        self.cursor = start;
        self.anchor = start;
        self.doc.gc_images();
    }

    pub fn delete_forward(&mut self, word: bool) {
        if self.has_sel() {
            self.checkpoint(EditKind::Delete);
            self.delete_selection();
            return;
        }
        let c = self.cursor;
        let end = if word { self.doc.word_right(c) } else { self.doc.next(c) };
        if end == c {
            return;
        }
        self.checkpoint(EditKind::Delete);
        self.doc.delete(c, end);
        self.doc.gc_images();
    }

    // ------------------------------------------------------------ clipboard

    pub fn copy(&mut self) {
        if !self.has_sel() {
            return;
        }
        let (a, b) = self.sel();
        let text = self.doc.plain_text(a, b);
        let paras = self.doc.fragment(a, b);
        let mut images = BTreeMap::new();
        for p in &paras {
            for f in &p.fmts {
                if let Some(img) = self.doc.images.get(&f.image) {
                    images.insert(f.image, img.clone());
                }
            }
        }
        self.ctx.copy_text(text.clone());
        self.clip = Some(Clip { text, paras, images });
    }

    pub fn cut(&mut self) {
        if !self.has_sel() {
            return;
        }
        self.copy();
        self.checkpoint(EditKind::Other);
        self.delete_selection();
        self.doc.gc_images();
    }

    /// Paste text received from the system clipboard; uses the rich internal
    /// clipboard when it still matches.
    pub fn paste_text(&mut self, text: &str) {
        let norm = |s: &str| s.replace("\r\n", "\n");
        if let Some(clip) = &self.clip {
            if norm(&clip.text) == norm(text) && !clip.paras.is_empty() {
                let mut frag = clip.paras.clone();
                // Bring pictures along, renumbering if the id is taken by another image.
                let mut remap = HashMap::new();
                for (id, img) in &clip.images {
                    let new_id = match self.doc.images.get(id) {
                        Some(existing) if std::sync::Arc::ptr_eq(&existing.bytes, &img.bytes) => *id,
                        None => *id,
                        Some(_) => self.doc.next_image_id().max(*id + 1),
                    };
                    self.doc.images.insert(new_id, img.clone());
                    remap.insert(*id, new_id);
                }
                for p in &mut frag {
                    for f in &mut p.fmts {
                        if let Some(n) = remap.get(&f.image) {
                            f.image = *n;
                        }
                    }
                }
                self.checkpoint(EditKind::Other);
                self.delete_selection();
                self.cursor = self.doc.insert_fragment(self.cursor, &frag);
                self.anchor = self.cursor;
                return;
            }
        }
        let fmt = self.current_fmt();
        self.checkpoint(EditKind::Other);
        self.delete_selection();
        self.cursor = self.doc.insert_text(self.cursor, text, fmt);
        self.anchor = self.cursor;
    }

    /// Paste from the OS clipboard directly (ribbon / context menu); supports images.
    pub fn paste_from_system(&mut self) {
        let Ok(mut cb) = arboard::Clipboard::new() else { return };
        if let Ok(t) = cb.get_text() {
            if !t.is_empty() {
                self.paste_text(&t);
                return;
            }
        }
        if let Ok(img) = cb.get_image() {
            let Some(rgba) = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()) else {
                return;
            };
            let mut png = Vec::new();
            if image::DynamicImage::ImageRgba8(rgba)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .is_ok()
            {
                self.insert_image_bytes(png);
            }
        }
    }

    // ------------------------------------------------------------ character formatting

    pub fn apply_fmt(&mut self, f: impl Fn(&mut CharFmt)) {
        if self.has_sel() {
            self.checkpoint(EditKind::Other);
            let (a, b) = self.sel();
            self.doc.apply_fmt(a, b, &|c: &mut CharFmt| {
                let img = c.image;
                f(c);
                c.image = img;
            });
        } else {
            let mut pf = self.current_fmt();
            f(&mut pf);
            let p = &mut self.doc.paras[self.cursor.p];
            if p.chars.is_empty() {
                p.end_fmt = pf;
                self.rev += 1;
            }
            self.pending_fmt = Some(pf);
        }
    }

    pub fn toggle_bold(&mut self) {
        let v = !self.current_fmt().bold;
        self.apply_fmt(|f| f.bold = v);
    }
    pub fn toggle_italic(&mut self) {
        let v = !self.current_fmt().italic;
        self.apply_fmt(|f| f.italic = v);
    }
    pub fn toggle_underline(&mut self) {
        let v = !self.current_fmt().underline;
        self.apply_fmt(|f| f.underline = v);
    }
    pub fn toggle_strike(&mut self) {
        let v = !self.current_fmt().strike;
        self.apply_fmt(|f| f.strike = v);
    }
    pub fn toggle_script(&mut self, s: i8) {
        let v = if self.current_fmt().script == s { 0 } else { s };
        self.apply_fmt(|f| f.script = v);
    }

    pub const SIZES: [f32; 16] = [8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0, 48.0, 72.0, 96.0];

    pub fn grow_font(&mut self, up: bool) {
        let cur = self.current_fmt().size;
        let next = if up {
            Self::SIZES.iter().copied().find(|s| *s > cur + 0.01).unwrap_or(cur + 10.0)
        } else {
            Self::SIZES.iter().rev().copied().find(|s| *s < cur - 0.01).unwrap_or((cur - 1.0).max(1.0))
        };
        self.apply_fmt(|f| f.size = next);
    }

    pub fn clear_formatting(&mut self) {
        let (a, b) = self.sel();
        if !self.has_sel() {
            let base = self.doc.paras[self.cursor.p].style.char_fmt();
            self.pending_fmt = Some(base);
            return;
        }
        self.checkpoint(EditKind::Other);
        for p in a.p..=b.p {
            let para = &mut self.doc.paras[p];
            let base = para.style.char_fmt();
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            for f in &mut para.fmts[s..e] {
                let img = f.image;
                *f = base;
                f.image = img;
            }
        }
    }

    pub fn change_case(&mut self, mode: u8) {
        if !self.has_sel() {
            let (a, b) = self.doc.word_at(self.cursor);
            self.select(a, b);
            if !self.has_sel() {
                return;
            }
        }
        self.checkpoint(EditKind::Other);
        let (a, b) = self.sel();
        for p in a.p..=b.p {
            let para = &mut self.doc.paras[p];
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { para.len() };
            let mut start_of_word = s == 0 || !para.chars[s - 1].is_alphanumeric();
            let mut start_of_sentence = true;
            for i in s..e {
                let c = para.chars[i];
                let up = |c: char| c.to_uppercase().next().unwrap_or(c);
                let low = |c: char| c.to_lowercase().next().unwrap_or(c);
                para.chars[i] = match mode {
                    0 => {
                        // Sentence case.
                        if c.is_alphabetic() {
                            let r = if start_of_sentence { up(c) } else { low(c) };
                            start_of_sentence = false;
                            r
                        } else {
                            if matches!(c, '.' | '!' | '?') {
                                start_of_sentence = true;
                            }
                            c
                        }
                    }
                    1 => low(c),
                    2 => up(c),
                    3 => {
                        let r = if start_of_word { up(c) } else { low(c) };
                        r
                    }
                    _ => {
                        if c.is_uppercase() { low(c) } else { up(c) }
                    }
                };
                start_of_word = !c.is_alphanumeric();
            }
        }
    }

    // ------------------------------------------------------------ paragraph formatting

    pub fn para_range(&self) -> std::ops::RangeInclusive<usize> {
        let (a, b) = self.sel();
        a.p..=b.p
    }

    pub fn apply_para(&mut self, f: impl Fn(&mut Para)) {
        self.checkpoint(EditKind::Other);
        for p in self.para_range() {
            f(&mut self.doc.paras[p]);
        }
    }

    pub fn set_align(&mut self, a: Align) {
        self.apply_para(|p| p.align = a);
    }

    pub fn toggle_list(&mut self, kind: ListKind) {
        let all = self.para_range().all(|p| self.doc.paras[p].list == kind);
        let new = if all { ListKind::None } else { kind };
        self.apply_para(|p| p.list = new);
    }

    pub fn indent(&mut self, delta: i32) {
        self.apply_para(|p| p.level = (p.level as i32 + delta).clamp(0, 8) as u8);
    }

    pub fn set_style(&mut self, s: Style) {
        self.apply_para(|p| p.set_style(s));
        self.pending_fmt = None;
    }

    pub fn set_line_spacing(&mut self, v: f32) {
        self.apply_para(|p| p.line_spacing = v);
    }

    // ------------------------------------------------------------ format painter

    pub fn start_format_painter(&mut self) {
        if self.format_painter.is_some() {
            self.format_painter = None;
            return;
        }
        let f = self.current_fmt();
        let p = self.current_para().empty_like();
        self.format_painter = Some((f, p));
        self.set_status("Format Painter: select text to apply the formatting");
    }

    pub fn apply_format_painter(&mut self) {
        let Some((f, para)) = self.format_painter.take() else { return };
        if !self.has_sel() {
            return;
        }
        self.checkpoint(EditKind::Other);
        let (a, b) = self.sel();
        self.doc.apply_fmt(a, b, &|c: &mut CharFmt| {
            let img = c.image;
            *c = f;
            c.image = img;
        });
        if a.p != b.p {
            for p in a.p..=b.p {
                self.doc.paras[p].copy_props_from(&para);
            }
        }
    }

    // ------------------------------------------------------------ insert

    pub fn insert_page_break(&mut self) {
        self.insert_special(PAGE_BREAK);
    }

    pub fn insert_line_break(&mut self) {
        self.insert_special(LINE_BREAK);
    }

    pub fn insert_date(&mut self, fmt: &str) {
        let s = chrono::Local::now().format(fmt).to_string();
        let f = self.current_fmt();
        self.checkpoint(EditKind::Other);
        self.delete_selection();
        self.cursor = self.doc.insert_text(self.cursor, &s, f);
        self.anchor = self.cursor;
    }

    pub fn content_width_pt(&self) -> f32 {
        let m = self.doc.page.margins;
        (self.doc.page.width - m[1] - m[3]).max(36.0)
    }

    pub fn insert_image_bytes(&mut self, bytes: Vec<u8>) {
        match io::image_from_bytes(bytes, self.content_width_pt()) {
            Ok(img) => {
                let id = self.doc.next_image_id();
                self.checkpoint(EditKind::Other);
                self.delete_selection();
                self.doc.images.insert(id, img);
                let mut f = self.current_fmt();
                f.image = id;
                self.cursor = self.doc.insert_text(self.cursor, &OBJ.to_string(), f);
                self.anchor = self.cursor;
            }
            Err(e) => self.error(format!("Could not insert the picture: {e}")),
        }
    }

    pub fn insert_picture(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "bmp"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(b) => self.insert_image_bytes(b),
            Err(e) => self.error(format!("Could not read {}: {e}", path.display())),
        }
    }

    /// Image id if the selection is exactly one picture.
    pub fn selected_image(&self) -> Option<u32> {
        let (a, b) = self.sel();
        if a.p == b.p && b.o == a.o + 1 {
            let f = self.doc.paras[a.p].fmts[a.o];
            if f.image != 0 {
                return Some(f.image);
            }
        }
        None
    }

    pub fn scale_image(&mut self, id: u32, pct: f32) {
        let maxw = self.content_width_pt();
        if let Some(img) = self.doc.images.get(&id).cloned() {
            self.checkpoint(EditKind::Other);
            let img_mut = self.doc.images.get_mut(&id).unwrap();
            let mut w = img.px_w as f32 * 0.75 * pct;
            let mut h = img.px_h as f32 * 0.75 * pct;
            if w > maxw {
                h *= maxw / w;
                w = maxw;
            }
            img_mut.w = w;
            img_mut.h = h;
        }
    }

    // ------------------------------------------------------------ find & replace

    pub fn refresh_find(&mut self) {
        if self.find.searched_rev == self.rev && self.find.searched_query == self.find.query {
            return;
        }
        self.find.matches = self.doc.find_all(&self.find.query, self.find.match_case, self.find.whole_word);
        self.find.searched_rev = self.rev;
        self.find.searched_query = self.find.query.clone();
        if self.find.current.is_some_and(|c| c >= self.find.matches.len()) {
            self.find.current = None;
        }
    }

    pub fn invalidate_find(&mut self) {
        self.find.searched_rev = 0;
    }

    pub fn find_next(&mut self, forward: bool) {
        self.refresh_find();
        let m = &self.find.matches;
        if m.is_empty() {
            self.set_status(format!("\"{}\" was not found", self.find.query));
            return;
        }
        let (a, b) = self.sel();
        let idx = if forward {
            m.iter().position(|(s, _)| *s >= b).unwrap_or(0)
        } else {
            m.iter().rposition(|(s, _)| *s < a).unwrap_or(m.len() - 1)
        };
        let (s, e) = m[idx];
        self.find.current = Some(idx);
        self.select(s, e);
    }

    pub fn replace_one(&mut self) {
        self.refresh_find();
        let (a, b) = self.sel();
        if self.find.matches.iter().any(|m| *m == (a, b)) {
            let f = self.doc.paras[a.p].fmt_at(a.o + 1);
            self.checkpoint(EditKind::Other);
            self.doc.delete(a, b);
            let end = self.doc.insert_text(a, &self.find.replacement.clone(), f);
            self.select(end, end);
        }
        self.find_next(true);
    }

    pub fn replace_all(&mut self) {
        self.refresh_find();
        let matches = self.find.matches.clone();
        if matches.is_empty() {
            self.set_status("No matches to replace");
            return;
        }
        self.checkpoint(EditKind::Other);
        let rep = self.find.replacement.clone();
        for (a, b) in matches.iter().rev() {
            let f = self.doc.paras[a.p].fmt_at(a.o + 1);
            self.doc.delete(*a, *b);
            self.doc.insert_text(*a, &rep, f);
        }
        let c = self.doc.clamp(self.cursor);
        self.select(c, c);
        self.set_status(format!("Replaced {} occurrence(s)", matches.len()));
    }

    // ------------------------------------------------------------ files

    fn reset_with(&mut self, doc: Document, path: Option<PathBuf>) {
        self.doc = doc;
        self.path = path;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.cursor = Pos::default();
        self.anchor = Pos::default();
        self.pending_fmt = None;
        self.textures.clear();
        self.rev += 1;
        self.invalidate_find();
        self.moved();
    }

    /// Runs `action`, asking to save first if there are unsaved changes.
    pub fn guarded(&mut self, action: After) {
        if self.dirty {
            self.confirm = Some(action);
        } else {
            self.run_after(action);
        }
    }

    pub fn run_after(&mut self, action: After) {
        match action {
            After::New => self.reset_with(Document::new(), None),
            After::Open => self.open_dialog(),
            After::Exit => {
                self.allow_close = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn open_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("All supported", &["docx", "fwd", "txt", "md"])
            .add_filter("Word Document", &["docx"])
            .add_filter("FreeWord Document", &["fwd"])
            .add_filter("Text", &["txt", "md"])
            .pick_file()
        else {
            return;
        };
        self.open_path(&path);
    }

    pub fn open_path(&mut self, path: &Path) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match load_any(path) {
            Ok(doc) => {
                // Only formats we can write back keep their path for Save.
                let keep = matches!(ext.as_str(), "docx" | "fwd" | "txt");
                self.reset_with(doc, keep.then(|| path.to_path_buf()));
                self.set_status(format!("Opened {}", path.display()));
            }
            Err(e) => self.error(format!("Could not open {}:\n{e}", path.display())),
        }
    }

    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) => self.save_to(&p),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) -> bool {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string());
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Word Document", &["docx"])
            .add_filter("FreeWord Document", &["fwd"])
            .add_filter("Plain Text", &["txt"])
            .set_file_name(format!("{}.docx", name.unwrap_or_else(|| "Document1".into())))
            .save_file()
        else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("docx");
        }
        if self.save_to(&path) {
            self.path = Some(path);
            true
        } else {
            false
        }
    }

    fn save_to(&mut self, path: &Path) -> bool {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let res = match ext.as_str() {
            "fwd" => io::save_native(&self.doc, path),
            "txt" => io::save_text(&self.doc, path),
            "pdf" => self.write_pdf(path),
            "html" | "htm" => io::save_html(&self.doc, path, &self.title()),
            _ => docx::save_docx(&self.doc, path),
        };
        match res {
            Ok(()) => {
                self.dirty = false;
                self.set_status(format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.error(format!("Could not save {}:\n{e}", path.display()));
                false
            }
        }
    }

    fn write_pdf(&self, path: &Path) -> Result<(), String> {
        let doc = &self.doc;
        let lay = self.ctx.fonts_mut(|f| layout::layout(doc, f, pdf::PDF_ZOOM));
        pdf::export(doc, &lay, path)
    }

    pub fn export_pdf(&mut self) {
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string());
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name(format!("{}.pdf", stem.unwrap_or_else(|| "Document1".into())))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("pdf");
        }
        match self.write_pdf(&path) {
            Ok(()) => self.set_status(format!("Exported PDF to {}", path.display())),
            Err(e) => self.error(format!("Could not export PDF:\n{e}")),
        }
    }

    pub fn export_html(&mut self) {
        let Some(mut path) = rfd::FileDialog::new().add_filter("Web Page", &["html", "htm"]).save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("html");
        }
        match io::save_html(&self.doc, &path, &self.title()) {
            Ok(()) => self.set_status(format!("Exported {}", path.display())),
            Err(e) => self.error(e),
        }
    }

    pub fn export_text(&mut self) {
        let Some(mut path) = rfd::FileDialog::new().add_filter("Plain Text", &["txt"]).save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("txt");
        }
        match io::save_text(&self.doc, &path) {
            Ok(()) => self.set_status(format!("Exported {}", path.display())),
            Err(e) => self.error(e),
        }
    }

    /// Renders to a temporary PDF and opens it in the system viewer for printing.
    pub fn print(&mut self) {
        let path = std::env::temp_dir().join(format!("FreeWord-print-{}.pdf", std::process::id()));
        match self.write_pdf(&path).and_then(|_| io::open_with_system(&path)) {
            Ok(()) => self.set_status("Opened print preview (PDF) — print from your PDF viewer"),
            Err(e) => self.error(format!("Could not print:\n{e}")),
        }
    }
}
