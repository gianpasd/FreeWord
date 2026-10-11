//! Editing headers, footers and notes in place ("stories" other than the body).
//!
//! While one is open, its sub-document is swapped into `App::doc` so every
//! editing command works on it unchanged, and the body is kept here. The body
//! is laid out with the edited story synced into it, and the editor works on a
//! layout of just the story placed where it is shown.

use crate::app::{App, Snap};
use crate::doc::{CharFmt, Document, OBJ, Pos};
use crate::layout::{self, Layout};
use crate::pool::{self, ObjKind, RawObj};
use std::sync::Arc;

/// What is being edited.
#[derive(Clone, PartialEq, Debug)]
pub enum Target {
    /// Variants: 0 header, 1 first-page header, 2 footer, 3 first-page footer.
    Hf { section: usize, variant: usize },
    /// Footnote (false) or endnote (true) by id.
    Note((bool, String)),
    /// Nested table held by a body paragraph.
    Nested(usize),
}

/// Area of a page under the pointer.
#[derive(Clone, PartialEq, Debug)]
pub enum Zone {
    Header,
    Footer,
    Note((bool, String)),
    /// Nested table held by a body paragraph.
    Nested(usize),
    Body,
}

pub struct Story {
    pub target: Target,
    /// Page the story is edited on.
    pub page: usize,
    pub body: Document,
    pub body_layout: Option<Layout>,
    /// Body state before the story was opened (the undo point for all its edits).
    before: Snap,
    body_undo: Vec<Snap>,
    body_redo: Vec<Snap>,
    entry_rev: u64,
    created: bool,
}

impl Story {
    /// The page zone this story occupies.
    pub fn zone(&self) -> Zone {
        match &self.target {
            Target::Hf { variant, .. } if *variant < 2 => Zone::Header,
            Target::Hf { .. } => Zone::Footer,
            Target::Note(k) => Zone::Note(k.clone()),
            Target::Nested(p) => Zone::Nested(*p),
        }
    }

    pub fn is_hf(&self) -> bool {
        matches!(self.target, Target::Hf { .. })
    }

    pub fn name(&self) -> String {
        match &self.target {
            Target::Hf { section, variant } => {
                let base = match variant {
                    0 => "Header",
                    1 => "First Page Header",
                    2 => "Footer",
                    _ => "First Page Footer",
                };
                if self.body.hf.len() > 1 { format!("{base} -Section {}-", section + 1) } else { base.to_string() }
            }
            Target::Note((endnote, _)) => (if *endnote { "Endnote" } else { "Footnote" }).to_string(),
            Target::Nested(_) => "Nested Table".to_string(),
        }
    }
}

/// A request from the canvas, applied once the frame's layout is back in place.
pub enum StoryReq {
    /// Open the header (or footer) of `page`, with the caret near page-relative `at`.
    Open { page: usize, header: bool, at: Option<(f32, f32)> },
    /// Open a note, with the caret near `at` = (page, x, y).
    OpenNote { key: (bool, String), at: Option<(usize, f32, f32)> },
    /// Open the nested table of body paragraph `para`, with the caret near `at` = (page, x, y).
    OpenNested { para: usize, at: Option<(usize, f32, f32)> },
    /// Return to the body, with the caret near `at` if given.
    Close { at: Option<(usize, f32, f32)> },
}

impl App {
    /// The body document (also while a story is being edited).
    pub fn main_doc(&self) -> &Document {
        self.story.as_deref().map(|s| &s.body).unwrap_or(&self.doc)
    }

    /// The body layout (also while a story is being edited).
    pub fn main_layout(&self) -> Option<&Layout> {
        match self.story.as_deref() {
            Some(s) => s.body_layout.as_ref(),
            None => self.layout.as_ref(),
        }
    }

    /// Page showing the caret.
    pub fn caret_page(&mut self) -> usize {
        if let Some(st) = self.story.as_deref() {
            return st.page;
        }
        self.ensure_layout();
        self.layout.as_ref().map(|l| l.caret(self.cursor).0).unwrap_or(0)
    }

    pub fn apply_story_req(&mut self, req: StoryReq) {
        match req {
            StoryReq::Open { page, header, at } => self.open_hf(page, header, at),
            StoryReq::OpenNote { key, at } => self.open_note(key, at),
            StoryReq::OpenNested { para, at } => self.open_nested(para, at),
            StoryReq::Close { at } => {
                self.close_story();
                if let Some((page, x, y)) = at {
                    self.ensure_layout();
                    let pos = self.layout.as_ref().map(|l| l.hit(page, x, y)).unwrap_or_default();
                    self.set_cursor(pos, false);
                }
            }
        }
    }

    /// Starts editing the header (or footer) shown on `page`, creating it if needed.
    pub fn open_hf(&mut self, page: usize, header: bool, at: Option<(f32, f32)>) {
        // Same header on another page: just move there.
        if let Some(st) = self.story.as_deref() {
            let slot = st.body_layout.as_ref().map(|l| l.hf_slot(&st.body, header, page));
            if slot.is_some_and(|(section, variant)| st.target == Target::Hf { section, variant }) {
                self.story.as_deref_mut().unwrap().page = page;
                self.layout = None;
                self.place_caret(page, at);
                return;
            }
            self.close_story();
        }
        self.ensure_layout();
        let lay = self.layout.as_ref().unwrap();
        let page = page.min(lay.pages - 1);
        let (section, variant) = lay.hf_slot(&self.doc, header, page);
        let before = self.snap();
        let existing = layout::hf_doc(&self.doc, section, variant).cloned();
        let created = existing.is_none();
        let sub = match existing {
            Some(d) => d,
            None => {
                // New headers become the default header of the last section (see docx::docx_bytes).
                let sections = self.doc.section_props().len().max(1);
                if section + 1 != sections || variant % 2 == 1 {
                    self.set_status("Adding a header or footer to this section is not supported yet");
                    return;
                }
                let hf = Arc::make_mut(&mut self.doc.hf);
                if hf.len() < sections {
                    hf.resize_with(sections, Default::default);
                }
                let sub = crate::docx::hf_from_plain(&self.doc, header);
                if header {
                    self.doc.header.clear();
                } else {
                    self.doc.footer.clear();
                    self.doc.page_numbers = false;
                }
                self.doc.put_hf(section, variant, sub.clone());
                sub
            }
        };
        self.begin_story(Target::Hf { section, variant }, page, sub, before, created);
        self.place_caret(page, at);
    }

    /// Starts editing a footnote or endnote, with the caret near `at` = (page, x, y).
    pub fn open_note(&mut self, key: (bool, String), at: Option<(usize, f32, f32)>) {
        if self.story.as_deref().is_none_or(|s| s.target != Target::Note(key.clone())) {
            self.close_story();
            let Some(sub) = self.doc.notes.get(&key).cloned() else { return };
            self.ensure_layout();
            let page = self.layout.as_ref().and_then(|l| l.notes.iter().find(|n| n.key == key)).map(|n| n.page).unwrap_or(0);
            let before = self.snap();
            self.begin_story(Target::Note(key), page, sub, before, false);
        }
        match at {
            Some((page, x, y)) => self.place_caret(page, Some((x, y))),
            None => {
                let end = self.doc.end();
                self.set_cursor(end, false);
            }
        }
    }

    /// Starts editing the nested table held by body paragraph `para`, with the caret near `at` = (page, x, y).
    pub fn open_nested(&mut self, para: usize, at: Option<(usize, f32, f32)>) {
        if self.story.as_deref().is_none_or(|s| s.target != Target::Nested(para)) {
            self.close_story();
            let Some(sub) = self.doc.paras.get(para).and_then(|p| p.nested.as_deref()).cloned() else { return };
            self.ensure_layout();
            let page = self.layout.as_ref().and_then(|l| l.nested.iter().find(|n| n.0 == para)).map(|n| n.1).unwrap_or(0);
            let before = self.snap();
            self.begin_story(Target::Nested(para), page, sub, before, false);
        }
        match at {
            Some((page, x, y)) => self.place_caret(page, Some((x, y))),
            None => self.set_cursor(Pos::default(), false),
        }
    }

    fn snap(&self) -> Snap {
        Snap { doc: self.doc.clone(), cursor: self.cursor, anchor: self.anchor }
    }

    /// Swaps `sub` in as the edited document.
    fn begin_story(&mut self, target: Target, page: usize, mut sub: Document, before: Snap, created: bool) {
        sub.page = self.doc.page.clone();
        // List definitions are shared with the body (only its numbering part is saved).
        sub.numbering = self.doc.numbering.clone();
        let body = std::mem::replace(&mut self.doc, sub);
        self.story = Some(Box::new(Story {
            target,
            page,
            body,
            body_layout: None,
            before,
            body_undo: std::mem::take(&mut self.undo),
            body_redo: std::mem::take(&mut self.redo),
            entry_rev: 0,
            created,
        }));
        self.rev += 1;
        self.story.as_deref_mut().unwrap().entry_rev = self.rev;
        self.layout = None;
        self.pending_fmt = None;
        self.goal_x = None;
    }

    fn place_caret(&mut self, page: usize, at: Option<(f32, f32)>) {
        self.ensure_layout();
        let pos = match (at, self.layout.as_ref()) {
            (Some((x, y)), Some(lay)) if !lay.lines.is_empty() => lay.hit(page, x, y),
            _ => Pos::default(),
        };
        self.set_cursor(pos, false);
    }

    /// Whether the open story was changed.
    fn story_changed(&self) -> bool {
        self.story.as_deref().is_some_and(|s| s.created || self.rev != s.entry_rev)
    }

    /// Puts the edited story into the body document (called before laying out the body).
    pub fn sync_story(&mut self) {
        let changed = self.story_changed();
        let Some(st) = self.story.as_deref_mut() else { return };
        let mut sub = self.doc.clone();
        sub.edited |= changed;
        st.body.numbering = sub.numbering.clone();
        match &st.target {
            Target::Hf { section, variant } => st.body.put_hf(*section, *variant, sub),
            Target::Note(key) => {
                Arc::make_mut(&mut st.body.notes).insert(key.clone(), sub);
            }
            Target::Nested(p) => {
                if let Some(para) = st.body.paras.get_mut(*p) {
                    para.nested = Some(Arc::new(sub));
                }
            }
        }
    }

    /// Lays out the body with the edited story and returns the story's own layout.
    pub fn story_layouts(&mut self) -> Option<Layout> {
        self.sync_story();
        let zoom = self.zoom;
        let st = self.story.as_deref_mut()?;
        let body = &st.body;
        let bl = self.ctx.fonts_mut(|f| layout::layout(body, f, zoom));
        st.page = st.page.min(bl.pages - 1);
        let l = match &st.target {
            Target::Hf { section, variant } => bl.story_layout(*section, *variant, st.page)?,
            Target::Note(key) => {
                let (l, page) = bl.note_story_layout(key)?;
                st.page = page;
                l
            }
            Target::Nested(p) => {
                let (l, page) = bl.nested_story_layout(*p)?;
                st.page = page;
                l
            }
        };
        st.body_layout = Some(bl);
        Some(l)
    }

    /// Stops editing the story and returns to the body.
    pub fn close_story(&mut self) {
        if self.story.is_none() {
            return;
        }
        let changed = self.story_changed();
        self.sync_story();
        let st = *self.story.take().unwrap();
        let sub = std::mem::replace(&mut self.doc, st.body);
        self.undo = st.body_undo;
        self.redo = st.body_redo;
        if changed {
            self.undo.push(st.before.clone());
            self.redo.clear();
            self.dirty = true;
        }
        drop(sub);
        self.cursor = self.doc.clamp(st.before.cursor);
        self.anchor = self.doc.clamp(st.before.anchor);
        self.pending_fmt = None;
        self.goal_x = None;
        self.rev += 1;
        self.layout = None;
        self.moved();
    }

    /// Inserts a footnote (or endnote) reference at the caret and opens the new note.
    pub fn insert_note(&mut self, endnote: bool) {
        self.close_story();
        let before = self.snap();
        self.delete_selection();
        let ids = self.doc.notes.keys().filter(|(e, _)| *e == endnote).filter_map(|(_, id)| id.parse::<i64>().ok());
        let id = (ids.max().unwrap_or(0).max(0) + 1).to_string();
        let (tag, style) = if endnote { ("endnoteReference", "EndnoteReference") } else { ("footnoteReference", "FootnoteReference") };
        let rpr = if self.doc.styles.styles.contains_key(style) {
            format!("<w:rStyle w:val=\"{style}\"/>")
        } else {
            "<w:vertAlign w:val=\"superscript\"/>".to_string()
        };
        let xml = format!("<w:r><w:rPr>{rpr}</w:rPr><w:{tag} w:id=\"{id}\"/></w:r>");
        let kind = ObjKind::NoteRef { endnote, id: id.clone() };
        let fmt = self.pending_fmt.take().unwrap_or_else(|| self.current_fmt()).without_object();
        let f = CharFmt { obj: pool::add_obj(RawObj { xml, kind, pkg: self.doc.pkg_id() }), ..fmt };
        let c = self.cursor;
        let para = &mut self.doc.paras[c.p];
        para.chars.insert(c.o, OBJ);
        para.fmts.insert(c.o, f);
        self.cursor = Pos::new(c.p, c.o + 1);
        self.anchor = self.cursor;
        let key = (endnote, id);
        let sub = crate::docx::new_note(&self.doc, endnote, &key.1);
        Arc::make_mut(&mut self.doc.notes).insert(key.clone(), sub.clone());
        self.rev += 1;
        self.ensure_layout();
        let page = self.layout.as_ref().and_then(|l| l.notes.iter().find(|n| n.key == key)).map(|n| n.page).unwrap_or(0);
        self.begin_story(Target::Note(key), page, sub, before, true);
        let end = self.doc.end();
        self.set_cursor(end, false);
    }

    /// Inserts a PAGE (or NUMPAGES) field at the caret.
    pub fn insert_page_field(&mut self, total: bool) {
        self.checkpoint(crate::app::EditKind::Other);
        self.delete_selection();
        let fmt = self.pending_fmt.take().unwrap_or_else(|| self.current_fmt()).without_object();
        let pkg = self.doc.pkg_id();
        let obj = |xml: &str, kind: ObjKind| pool::add_obj(RawObj { xml: xml.to_string(), kind, pkg });
        let instr = if total { " NUMPAGES " } else { " PAGE " };
        let parts: Vec<(char, u32)> = vec![
            (OBJ, obj("<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>", ObjKind::FieldBegin)),
            (OBJ, obj(&format!("<w:r><w:instrText xml:space=\"preserve\">{instr}</w:instrText></w:r>"), ObjKind::Hidden)),
            (OBJ, obj("<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>", ObjKind::FieldSep)),
            ('1', 0),
            (OBJ, obj("<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>", ObjKind::FieldEnd)),
        ];
        let c = self.cursor;
        let para = &mut self.doc.paras[c.p];
        for (k, (ch, id)) in parts.iter().enumerate() {
            let f = CharFmt { obj: *id, ..fmt };
            para.chars.insert(c.o + k, *ch);
            para.fmts.insert(c.o + k, f);
        }
        let end = Pos::new(c.p, c.o + parts.len());
        self.set_cursor(end, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    fn app() -> App {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        App::with_ctx(ctx)
    }

    fn hf_texts(doc: &Document) -> String {
        doc.hf
            .iter()
            .flat_map(|h| [&h.header, &h.header_first, &h.footer, &h.footer_first])
            .flatten()
            .map(|d| d.paras.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Opens the header (or footer) of page 1, types, closes and round-trips through .docx.
    /// The saved file is kept in `tests/out/hf` for `tests/validate_word.ps1`.
    fn edit_and_reload(app: &mut App, header: bool, text: &str) -> Document {
        app.open_hf(0, header, None);
        assert!(app.story.is_some(), "story did not open");
        let end = app.doc.end();
        app.set_cursor(end, false);
        app.insert_typed(text);
        app.close_story();
        assert!(app.story.is_none());
        assert!(hf_texts(&app.doc).contains(text), "edit not stored in the body document");
        let bytes = crate::docx::docx_bytes(&app.doc).expect("save");
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/out/hf");
        let _ = std::fs::create_dir_all(&out);
        let stem = app.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("new".into());
        let _ = std::fs::write(out.join(format!("{stem}.{text}.docx")), &bytes);
        crate::docx::load_docx_bytes(&bytes).expect("reload")
    }

    #[test]
    fn new_document_header_and_footer() {
        let mut a = app();
        let back = edit_and_reload(&mut a, true, "NewHeaderText");
        assert!(hf_texts(&back).contains("NewHeaderText"));
        a.doc = back;
        a.rev += 1;
        let back = edit_and_reload(&mut a, false, "NewFooterText");
        let t = hf_texts(&back);
        assert!(t.contains("NewHeaderText") && t.contains("NewFooterText"), "{t}");
    }

    fn note_texts(doc: &Document) -> String {
        doc.notes.values().flat_map(|d| d.paras.iter().map(|p| p.text())).collect::<Vec<_>>().join("\n")
    }

    fn save_reload(app: &App, name: &str) -> Document {
        let bytes = crate::docx::docx_bytes(&app.doc).expect("save");
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/out/hf");
        let _ = std::fs::create_dir_all(&out);
        let _ = std::fs::write(out.join(format!("{name}.docx")), &bytes);
        crate::docx::load_docx_bytes(&bytes).expect("reload")
    }

    fn note_refs(doc: &Document) -> usize {
        doc.paras
            .iter()
            .flat_map(|p| p.fmts.iter())
            .filter(|f| pool::obj(f.obj).is_some_and(|o| matches!(o.kind, ObjKind::NoteRef { ref id, .. } if id != "self")))
            .count()
    }

    #[test]
    fn insert_notes_in_new_document() {
        let mut a = app();
        a.insert_typed("Body text");
        a.insert_note(false);
        assert!(a.story.is_some());
        a.insert_typed("FootOne");
        a.close_story();
        a.insert_typed(" more");
        a.insert_note(true);
        a.insert_typed("EndOne");
        a.close_story();
        let back = save_reload(&a, "new.notes");
        let t = note_texts(&back);
        assert!(t.contains("FootOne") && t.contains("EndOne"), "{t}");
        assert_eq!(note_refs(&back), 2);
        // One undo removes the endnote and its reference.
        a.undo();
        assert_eq!(note_refs(&a.doc), 1);
    }

    #[test]
    fn edit_and_add_notes_in_corpus() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/04_fields.docx");
        let mut a = app();
        a.open_path(&path);
        let key = a.doc.notes.keys().find(|k| !k.0).cloned().expect("corpus footnote");
        a.open_note(key.clone(), None);
        a.insert_typed(" EditedNote");
        a.close_story();
        let end = a.doc.end();
        a.set_cursor(end, false);
        a.insert_note(false);
        a.insert_typed("AddedNote");
        a.close_story();
        let back = save_reload(&a, "04_fields.notes");
        let t = note_texts(&back);
        assert!(t.contains("EditedNote") && t.contains("AddedNote"), "{t}");
        assert_eq!(note_refs(&back), note_refs(&crate::docx::load_docx(&path).unwrap()) + 1);
    }

    #[test]
    fn undo_restores_body_after_header_edit() {
        let mut a = app();
        a.insert_typed("body");
        a.open_hf(0, true, None);
        a.insert_typed("H1");
        a.close_story();
        assert!(hf_texts(&a.doc).contains("H1"));
        a.undo();
        assert!(!hf_texts(&a.doc).contains("H1"));
        assert_eq!(a.doc.paras[0].text(), "body");
    }

    #[test]
    fn corpus_header_footer_round_trip() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
        let mut edited = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "docx") {
                continue;
            }
            for header in [true, false] {
                let mut a = app();
                a.open_path(&path);
                assert!(a.dlg.error.is_none(), "{}", path.display());
                let tag = if header { "ZzHeadEdit" } else { "ZzFootEdit" };
                a.ensure_layout();
                let (section, variant) = a.layout.as_ref().unwrap().hf_slot(&a.doc, header, 0);
                let sections = a.doc.section_props().len().max(1);
                let exists = layout::hf_doc(&a.doc, section, variant).is_some();
                if !exists && (section + 1 != sections || variant % 2 == 1) {
                    continue; // creating headers there is not supported
                }
                let back = edit_and_reload(&mut a, header, tag);
                assert!(hf_texts(&back).contains(tag), "{}: {tag} lost after save", path.display());
                // Body text is untouched.
                let orig = crate::docx::load_docx(&path).unwrap();
                assert_eq!(orig.paras.len(), back.paras.len(), "{}", path.display());
                edited += 1;
            }
        }
        assert!(edited >= 16, "only {edited} headers/footers edited");
    }

    fn nested_texts(doc: &Document) -> String {
        doc.paras.iter().filter_map(|p| p.nested.as_deref()).flat_map(|d| d.paras.iter().map(|p| p.text())).collect::<Vec<_>>().join("
")
    }

    #[test]
    fn edit_nested_tables_in_corpus() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
        let mut edited = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "docx") {
                continue;
            }
            let mut a = app();
            a.open_path(&path);
            let Some(para) = a.doc.paras.iter().position(|p| p.nested.is_some()) else { continue };
            // The nested table is found under the pointer.
            a.ensure_layout();
            let lay = a.layout.as_ref().unwrap();
            let &(_, page, x, y, _) = lay.nested.iter().find(|n| n.0 == para).expect("nested table laid out");
            assert_eq!(lay.nested_at(page, x + 2.0, y + 2.0), Some(para), "{}", path.display());
            a.open_nested(para, Some((page, x + 2.0, y + 2.0)));
            assert!(a.story.is_some(), "{}: story did not open", path.display());
            assert!(a.doc.paras[a.cursor.p].cell.is_some(), "caret not in a cell");
            a.insert_typed("ZzNestedEdit");
            a.close_story();
            assert!(nested_texts(&a.doc).contains("ZzNestedEdit"), "edit not stored in the body");
            let orig_paras = a.doc.paras.len();
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            let back = save_reload(&a, &format!("{stem}.nested"));
            assert!(nested_texts(&back).contains("ZzNestedEdit"), "{}: edit lost after save", path.display());
            assert_eq!(back.paras.len(), orig_paras, "{}", path.display());
            // .fwd files keep the edit too.
            let fwd = std::env::temp_dir().join(format!("fw_nested_{stem}.fwd"));
            crate::io::save_native(&a.doc, &fwd).expect("save fwd");
            let back = crate::io::load_native(&fwd).expect("load fwd");
            let _ = std::fs::remove_file(&fwd);
            assert!(nested_texts(&back).contains("ZzNestedEdit"), "{}: edit lost in .fwd", path.display());
            // One undo restores the nested table.
            a.undo();
            assert!(!nested_texts(&a.doc).contains("ZzNestedEdit"));
            edited += 1;
        }
        assert!(edited >= 1, "no corpus document has a nested table");
    }
}
