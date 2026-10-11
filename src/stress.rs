//! Randomized editing stress test: `freeword --stress input output seed steps`.
//!
//! Applies a deterministic sequence of real editor commands to a document,
//! checks model invariants after every step, saves the result and reloads it.
//! The saved file is then validated in Microsoft Word by `tests/stress.ps1`.

use crate::app::App;
use crate::doc::{Align, ListKind, Pos, Style};
use eframe::egui;
use std::path::Path;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }

    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
}

const WORDS: &[&str] = &["alpha", "Beta", "gamma ", " delta", "ε", "émoji 😀", "tab\there", "x", " ", "-", "Ünïcødé", "line"];

fn random_pos(app: &App, r: &mut Rng) -> Pos {
    let p = r.below(app.doc.paras.len());
    let o = r.below(app.doc.paras[p].len() + 1);
    Pos::new(p, o)
}

fn check(app: &App, step: usize, op: &str) {
    let d = &app.doc;
    assert!(!d.paras.is_empty(), "step {step} ({op}): document has no paragraphs");
    for (i, p) in d.paras.iter().enumerate() {
        assert_eq!(p.chars.len(), p.fmts.len(), "step {step} ({op}): paragraph {i} chars/fmts mismatch");
    }
    assert!(app.cursor.p < d.paras.len() && app.cursor.o <= d.paras[app.cursor.p].len(), "step {step} ({op}): cursor out of range");
    assert!(app.anchor.p < d.paras.len() && app.anchor.o <= d.paras[app.anchor.p].len(), "step {step} ({op}): anchor out of range");
    // Paragraphs of one table must be contiguous.
    let mut seen_tables = std::collections::HashSet::new();
    let mut prev: Option<u32> = None;
    for p in &d.paras {
        let t = p.cell.map(|c| c.table);
        if t != prev {
            if let Some(t) = t {
                assert!(seen_tables.insert(t), "step {step} ({op}): table {t} is split");
            }
        }
        prev = t;
    }
    assert!(d.paras.last().is_some_and(|p| p.cell.is_none()), "step {step} ({op}): document ends inside a table");
}

pub fn run(input: &Path, output: &Path, seed: u64, steps: usize) -> Result<(), String> {
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    let mut app = App::with_ctx(ctx);
    app.open_path(input);
    if app.dlg.error.is_some() {
        return Err(app.dlg.error.clone().unwrap_or_default());
    }
    let mut r = Rng(seed.max(1));
    for step in 0..steps {
        // Move the cursor or make a selection.
        let a = random_pos(&app, &mut r);
        if r.chance(30) {
            let b = random_pos(&app, &mut r);
            app.select(a, b);
        } else {
            app.set_cursor(a, false);
        }
        let op = r.below(47);
        let name = format!("op{op}");
        match op {
            0..=5 => app.insert_typed(WORDS[r.below(WORDS.len())]),
            6 => app.enter(),
            7 => app.backspace(r.chance(30)),
            8 => app.delete_forward(r.chance(30)),
            9 => app.toggle_bold(),
            10 => app.toggle_italic(),
            11 => app.toggle_underline(),
            12 => app.toggle_script(if r.chance(50) { 1 } else { -1 }),
            13 => app.grow_font(r.chance(50)),
            14 => app.apply_fmt(|f| f.color = [200, 30, 30, 255]),
            15 => {
                let s = Style::ALL[r.below(Style::ALL.len())];
                app.set_style(s);
            }
            16 => app.set_align([Align::Left, Align::Center, Align::Right, Align::Justify][r.below(4)]),
            17 => app.toggle_list(if r.chance(50) { ListKind::Bullet } else { ListKind::Number }),
            18 => app.indent(if r.chance(50) { 1 } else { -1 }),
            19 => app.set_line_spacing([1.0, 1.15, 1.5, 2.0][r.below(4)]),
            20 => {
                if app.in_table().is_none() {
                    app.insert_table(1 + r.below(3) as u32, 1 + r.below(3) as u32);
                }
            }
            21 => app.next_cell(r.chance(70)),
            22 => app.table_insert_row(r.chance(50)),
            23 => app.table_insert_col(r.chance(50)),
            24 => app.table_delete_row(),
            25 => app.table_delete_col(),
            26 => {
                if r.chance(20) {
                    app.delete_table();
                }
            }
            27 => app.shade_cell(Some([220, 230, 240, 255])),
            28 => app.insert_link("https://example.com/", "link"),
            29 => app.remove_link(),
            30 | 31 => {
                app.copy();
                let text = app.clip.as_ref().map(|c| c.text.clone()).unwrap_or_default();
                let to = random_pos(&app, &mut r);
                app.set_cursor(to, false);
                if !text.is_empty() {
                    app.paste_text(&text);
                }
            }
            32 => app.cut(),
            33 | 34 => app.undo(),
            35 => app.redo(),
            36 => app.change_case(r.below(5) as u8),
            37 => {
                if r.chance(15) {
                    app.resolve_all_changes(r.chance(50));
                } else {
                    app.insert_page_break();
                }
            }
            38 => app.insert_line_break(),
            39 => {
                app.find.query = "a".into();
                app.find.replacement = "A".into();
                app.invalidate_find();
                app.replace_all();
            }
            // Headers and footers: later operations edit them until closed.
            40 | 41 => {
                app.ensure_layout();
                let pages = app.main_layout().map(|l| l.pages).unwrap_or(1);
                app.open_hf(r.below(pages), op == 40, None);
            }
            42 => app.close_story(),
            44 => {
                if r.chance(30) {
                    app.insert_note(r.chance(30));
                }
            }
            45 => {
                let keys: Vec<(bool, String)> = app.main_doc().notes.keys().cloned().collect();
                if !keys.is_empty() {
                    app.open_note(keys[r.below(keys.len())].clone(), None);
                }
            }
            _ => {
                if app.story.is_some() {
                    app.insert_page_field(r.chance(50));
                }
            }
        }
        check(&app, step, &name);
    }
    app.close_story();
    check(&app, steps, "close");
    crate::docx::save_docx(&app.doc, output)?;
    // The saved file must load again.
    let back = crate::docx::load_docx(output)?;
    if back.paras.is_empty() {
        return Err("reloaded document is empty".into());
    }
    Ok(())
}
