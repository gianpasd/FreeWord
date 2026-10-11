//! Editing headers and footers in place ("stories" other than the body).
//!
//! While a header or footer is open, its sub-document is swapped into
//! `App::doc` so every editing command works on it unchanged, and the body is
//! kept here. The body is laid out with the edited header synced into it, and
//! the editor works on a layout of just the header placed on its page.

use crate::app::{App, Snap};
use crate::doc::{CharFmt, Document, OBJ, Pos};
use crate::layout::{self, Layout};
use crate::pool::{self, ObjKind, RawObj};

pub struct Story {
    pub section: usize,
    /// 0 header, 1 first-page header, 2 footer, 3 first-page footer.
    pub variant: usize,
    /// Page the header is being edited on.
    pub page: usize,
    pub body: Document,
    pub body_layout: Option<Layout>,
    /// Body state before the header was opened (the undo point for all header edits).
    before: Snap,
    body_undo: Vec<Snap>,
    body_redo: Vec<Snap>,
    entry_rev: u64,
    created: bool,
}

impl Story {
    pub fn is_header(&self) -> bool {
        self.variant < 2
    }

    pub fn name(&self) -> String {
        let base = match self.variant {
            0 => "Header",
            1 => "First Page Header",
            2 => "Footer",
            _ => "First Page Footer",
        };
        if self.body.hf.len() > 1 { format!("{base} -Section {}-", self.section + 1) } else { base.to_string() }
    }
}

/// A request from the canvas, applied once the frame's layout is back in place.
pub enum StoryReq {
    /// Open the header (or footer) of `page`, with the caret near page-relative `at`.
    Open { page: usize, header: bool, at: Option<(f32, f32)> },
    Close,
}

impl App {
    /// The body document (also while a header or footer is being edited).
    pub fn main_doc(&self) -> &Document {
        self.story.as_deref().map(|s| &s.body).unwrap_or(&self.doc)
    }

    /// The body layout (also while a header or footer is being edited).
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
            StoryReq::Close => self.close_story(),
        }
    }

    /// Starts editing the header (or footer) shown on `page`, creating it if needed.
    pub fn open_hf(&mut self, page: usize, header: bool, at: Option<(f32, f32)>) {
        // Same header on another page: just move there.
        if let Some(st) = self.story.as_deref() {
            let slot = st.body_layout.as_ref().map(|l| l.hf_slot(&st.body, header, page));
            if slot == Some((st.section, st.variant)) {
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
        let before = Snap { doc: self.doc.clone(), cursor: self.cursor, anchor: self.anchor };
        let existing = layout::hf_doc(&self.doc, section, variant).cloned();
        let created = existing.is_none();
        let mut sub = match existing {
            Some(d) => d,
            None => {
                // New headers become the default header of the last section (see docx::docx_bytes).
                let sections = self.doc.section_props().len().max(1);
                if section + 1 != sections || variant % 2 == 1 {
                    self.set_status("Adding a header or footer to this section is not supported yet");
                    return;
                }
                let hf = std::sync::Arc::make_mut(&mut self.doc.hf);
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
        sub.page = self.doc.page.clone();
        // List definitions are shared with the body (only its numbering part is saved).
        sub.numbering = self.doc.numbering.clone();
        let body = std::mem::replace(&mut self.doc, sub);
        let story = Story {
            section,
            variant,
            page,
            body,
            body_layout: None,
            before,
            body_undo: std::mem::take(&mut self.undo),
            body_redo: std::mem::take(&mut self.redo),
            entry_rev: 0,
            created,
        };
        self.story = Some(Box::new(story));
        self.rev += 1;
        self.story.as_deref_mut().unwrap().entry_rev = self.rev;
        self.layout = None;
        self.pending_fmt = None;
        self.goal_x = None;
        self.place_caret(page, at);
    }

    fn place_caret(&mut self, page: usize, at: Option<(f32, f32)>) {
        self.ensure_layout();
        let pos = match (at, self.layout.as_ref()) {
            (Some((x, y)), Some(lay)) if !lay.lines.is_empty() => lay.hit(page, x, y),
            _ => Pos::default(),
        };
        self.set_cursor(pos, false);
    }

    /// Whether the open header or footer was changed.
    fn story_changed(&self) -> bool {
        self.story.as_deref().is_some_and(|s| s.created || self.rev != s.entry_rev)
    }

    /// Puts the edited header into the body document (called before laying out the body).
    pub fn sync_story(&mut self) {
        let changed = self.story_changed();
        let Some(st) = self.story.as_deref_mut() else { return };
        let mut sub = self.doc.clone();
        sub.edited |= changed;
        st.body.numbering = sub.numbering.clone();
        st.body.put_hf(st.section, st.variant, sub);
    }

    /// Lays out the body with the edited header and returns the header's own layout.
    pub fn story_layouts(&mut self) -> Option<Layout> {
        self.sync_story();
        let zoom = self.zoom;
        let st = self.story.as_deref_mut()?;
        let body = &st.body;
        let bl = self.ctx.fonts_mut(|f| layout::layout(body, f, zoom));
        st.page = st.page.min(bl.pages - 1);
        let l = bl.story_layout(st.section, st.variant, st.page)?;
        st.body_layout = Some(bl);
        Some(l)
    }

    /// Stops editing the header or footer and returns to the body.
    pub fn close_story(&mut self) {
        if self.story.is_none() {
            return;
        }
        let changed = self.story_changed();
        self.sync_story();
        let st = *self.story.take().unwrap();
        let header = std::mem::replace(&mut self.doc, st.body);
        self.undo = st.body_undo;
        self.redo = st.body_redo;
        if changed {
            self.undo.push(st.before.clone());
            self.redo.clear();
            self.dirty = true;
        }
        drop(header);
        self.cursor = self.doc.clamp(st.before.cursor);
        self.anchor = self.doc.clamp(st.before.anchor);
        self.pending_fmt = None;
        self.goal_x = None;
        self.rev += 1;
        self.layout = None;
        self.moved();
    }

    /// Inserts a PAGE (or NUMPAGES) field at the caret.
    pub fn insert_page_field(&mut self, total: bool) {
        self.checkpoint(crate::app::EditKind::Other);
        self.delete_selection();
        let fmt = self.pending_fmt.take().unwrap_or_else(|| self.current_fmt()).without_object();
        let pkg = self.main_doc().pkg_id();
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
}
