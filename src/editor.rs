//! The page canvas: keyboard and mouse handling, and painting.

use crate::app::{App, After, Tab};
use crate::doc::{Align, LINE_BREAK, ListKind, OBJ, PAGE_BREAK, Pos, Rgba, Style};
use crate::layout::{self, Layout};
use eframe::egui::{
    self, Color32, CursorIcon, Event, EventFilter, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextFormat, Ui, Vec2, pos2, text::LayoutJob, vec2,
};

pub const GAP: f32 = 24.0;

pub fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

impl App {
    // ================================================================ keyboard

    pub fn handle_events(&mut self, ctx: &egui::Context) {
        if self.text_input_focused {
            return;
        }
        let (events, mods) = ctx.input(|i| (i.events.clone(), i.modifiers));
        for ev in events {
            match ev {
                // Ctrl+key chords are shortcuts, not text (AltGr = Ctrl+Alt still types).
                Event::Text(t) if !(mods.ctrl && !mods.alt) => self.insert_typed(&t),
                Event::Ime(egui::ImeEvent::Commit(t)) => self.insert_typed(&t),
                Event::Copy => self.copy(),
                Event::Cut => self.cut(),
                Event::Paste(t) => self.paste_text(&t),
                Event::Key { key, pressed: true, modifiers, .. } => self.key(key, modifiers),
                _ => {}
            }
        }
    }

    fn key(&mut self, key: Key, m: Modifiers) {
        let ctrl = m.command;
        let shift = m.shift;
        let alt = m.alt;
        let horizontal = |s: &mut App, pos: Pos| {
            s.goal_x = None;
            s.set_cursor(pos, shift);
        };
        match key {
            Key::ArrowLeft => {
                if !shift && self.has_sel() {
                    let a = self.sel().0;
                    horizontal(self, a);
                } else {
                    let p = if ctrl { self.doc.word_left(self.cursor) } else { self.doc.prev(self.cursor) };
                    horizontal(self, p);
                }
            }
            Key::ArrowRight => {
                if !shift && self.has_sel() {
                    let b = self.sel().1;
                    horizontal(self, b);
                } else {
                    let p = if ctrl { self.doc.word_right(self.cursor) } else { self.doc.next(self.cursor) };
                    horizontal(self, p);
                }
            }
            Key::ArrowUp if ctrl => {
                let p = if self.cursor.o == 0 && self.cursor.p > 0 { self.cursor.p - 1 } else { self.cursor.p };
                horizontal(self, Pos::new(p, 0));
            }
            Key::ArrowDown if ctrl => {
                let p = (self.cursor.p + 1).min(self.doc.paras.len() - 1);
                let pos = if p == self.cursor.p { Pos::new(p, self.doc.paras[p].len()) } else { Pos::new(p, 0) };
                horizontal(self, pos);
            }
            Key::ArrowUp => self.move_vertical(-1, shift, false),
            Key::ArrowDown => self.move_vertical(1, shift, false),
            Key::PageUp => self.move_vertical(-1, shift, true),
            Key::PageDown => self.move_vertical(1, shift, true),
            Key::Home => {
                if ctrl {
                    horizontal(self, Pos::default());
                } else {
                    self.ensure_layout();
                    let lay = self.layout.as_ref().unwrap();
                    let l = &lay.lines[lay.line_of(self.cursor)];
                    let p = Pos::new(l.para, l.start);
                    horizontal(self, p);
                }
            }
            Key::End => {
                if ctrl {
                    let e = self.doc.end();
                    horizontal(self, e);
                } else {
                    self.ensure_layout();
                    let lay = self.layout.as_ref().unwrap();
                    let l = &lay.lines[lay.line_of(self.cursor)];
                    let o = if l.last_in_para && !l.forced { l.end } else { l.end.saturating_sub(1).max(l.start) };
                    let p = Pos::new(l.para, o);
                    horizontal(self, p);
                }
            }
            Key::Backspace => self.backspace(ctrl),
            Key::Delete => self.delete_forward(ctrl),
            Key::Enter => {
                if ctrl {
                    self.insert_page_break();
                } else if shift {
                    self.insert_line_break();
                } else {
                    self.enter();
                }
            }
            Key::Tab => {
                let multi = self.sel().0.p != self.sel().1.p;
                let p = self.current_para();
                if multi || (p.list != ListKind::None && self.cursor.o == 0) {
                    self.indent(if shift { -1 } else { 1 });
                } else if !shift {
                    self.insert_typed("\t");
                }
            }
            Key::Escape => {
                self.format_painter = None;
                self.find.open = false;
            }
            _ if ctrl => self.shortcut(key, shift, alt),
            _ => {}
        }
    }

    fn shortcut(&mut self, key: Key, shift: bool, alt: bool) {
        match key {
            Key::A => {
                let e = self.doc.end();
                self.select(Pos::default(), e);
            }
            Key::B => self.toggle_bold(),
            Key::I => self.toggle_italic(),
            Key::U => self.toggle_underline(),
            Key::Z => self.undo(),
            Key::Y => self.redo(),
            Key::S if shift => {
                self.save_as();
            }
            Key::S => {
                self.save();
            }
            Key::O => self.guarded(After::Open),
            Key::N if shift => self.set_style(Style::Normal),
            Key::N => self.guarded(After::New),
            Key::P => self.print(),
            Key::F => {
                self.find.open = true;
                self.find.replace = false;
                self.find.focus_query = true;
                self.prefill_find();
            }
            Key::H => {
                self.find.open = true;
                self.find.replace = true;
                self.find.focus_query = true;
                self.prefill_find();
            }
            Key::E => self.set_align(Align::Center),
            Key::L if shift => self.toggle_list(ListKind::Bullet),
            Key::L => self.set_align(Align::Left),
            Key::R => self.set_align(Align::Right),
            Key::J => self.set_align(Align::Justify),
            Key::M => self.indent(if shift { -1 } else { 1 }),
            Key::OpenBracket => self.grow_font(false),
            Key::CloseBracket => self.grow_font(true),
            Key::Comma if shift => self.grow_font(false),
            Key::Period if shift => self.grow_font(true),
            Key::Plus => self.toggle_script(1),
            Key::Equals if shift => self.toggle_script(1),
            Key::Equals => self.toggle_script(-1),
            Key::Space => self.clear_formatting(),
            Key::Num1 if alt => self.set_style(Style::Heading1),
            Key::Num2 if alt => self.set_style(Style::Heading2),
            Key::Num3 if alt => self.set_style(Style::Heading3),
            Key::Num1 => self.set_line_spacing(1.0),
            Key::Num2 => self.set_line_spacing(2.0),
            Key::Num5 => self.set_line_spacing(1.5),
            Key::D if shift => self.toggle_strike(),
            _ => {}
        }
    }

    fn prefill_find(&mut self) {
        if self.has_sel() {
            let (a, b) = self.sel();
            if a.p == b.p && b.o - a.o < 100 {
                self.find.query = self.doc.plain_text(a, b);
            }
        }
        self.invalidate_find();
    }

    fn move_vertical(&mut self, dir: i32, extend: bool, by_page: bool) {
        self.ensure_layout();
        let lay = self.layout.as_ref().unwrap();
        let li = lay.line_of(self.cursor);
        let (_, x, _, _) = lay.caret(self.cursor);
        let gx = self.goal_x.unwrap_or(x);
        let mut target = li as i64;
        if by_page {
            let mut acc = 0.0;
            let limit = self.viewport_h * 0.9;
            while acc < limit {
                let next = target + dir as i64;
                if next < 0 || next >= lay.lines.len() as i64 {
                    break;
                }
                target = next;
                acc += lay.lines[target as usize].height;
            }
        } else {
            target += dir as i64;
        }
        let pos = if target < 0 {
            Pos::default()
        } else if target >= lay.lines.len() as i64 {
            self.doc.end()
        } else {
            lay.pos_in_line(target as usize, gx)
        };
        self.set_cursor(pos, extend);
        self.goal_x = Some(gx);
    }

    // ================================================================ canvas

    fn page_origin(&self, rect: Rect, lay: &Layout, page: usize) -> Pos2 {
        let x = rect.left() + ((rect.width() - lay.page_w) / 2.0).max(GAP);
        pos2(x, rect.top() + GAP + page as f32 * (lay.page_h + GAP))
    }

    fn hit_point(&self, rect: Rect, lay: &Layout, p: Pos2) -> (usize, Pos) {
        let rel = p.y - rect.top() - GAP;
        let page = ((rel / (lay.page_h + GAP)).floor().max(0.0) as usize).min(lay.pages - 1);
        let o = self.page_origin(rect, lay, page);
        (page, lay.hit(page, p.x - o.x, p.y - o.y))
    }

    /// Screen rectangle of a picture character (page-relative).
    fn image_box(&self, lay: &Layout, pos: Pos) -> Option<(usize, Rect)> {
        let para = self.doc.paras.get(pos.p)?;
        let f = para.fmts.get(pos.o)?;
        let img = self.doc.images.get(&f.image)?;
        let l = &lay.lines[lay.line_of(pos)];
        if pos.o < l.start || pos.o >= l.end {
            return None;
        }
        let x = l.xs[pos.o - l.start];
        let base = l.y + l.baseline;
        let sc = lay.scale;
        Some((l.page, Rect::from_min_size(pos2(x, base - img.h * sc), vec2(img.w * sc, img.h * sc))))
    }

    pub fn canvas(&mut self, ui: &mut Ui) {
        self.ensure_layout();
        let lay = self.layout.take().unwrap();
        let avail = ui.available_size();
        let total_w = (lay.page_w + 2.0 * GAP).max(avail.x);
        let total_h = lay.pages as f32 * (lay.page_h + GAP) + GAP;
        let (rect, resp) = ui.allocate_exact_size(vec2(total_w, total_h), Sense::click_and_drag());
        let id = resp.id;

        if !self.text_input_focused {
            ui.memory_mut(|m| {
                m.request_focus(id);
                m.set_focus_lock_filter(
                    id,
                    EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
        }

        self.mouse(ui, &resp, rect, &lay);
        self.paint(ui, rect, &lay);

        if self.scroll_to_caret {
            self.scroll_to_caret = false;
            let (pg, x, y, h) = lay.caret(self.cursor);
            let o = self.page_origin(rect, &lay, pg);
            let r = Rect::from_min_size(o + vec2(x, y), vec2(2.0, h)).expand(30.0);
            ui.scroll_to_rect(r, None);
        }
        self.layout = Some(lay);
    }

    fn mouse(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect, lay: &Layout) {
        let (pointer, pressed, secondary, down, released, shift, time) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.secondary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.modifiers.shift,
                i.time,
            )
        });
        let sc = lay.scale;
        let handle_of = |s: &App, id: u32| -> Option<Rect> {
            let (a, _) = s.sel();
            let (pg, r) = s.image_box(lay, a)?;
            let _ = id;
            let o = s.page_origin(rect, lay, pg);
            Some(Rect::from_center_size(o + r.max.to_vec2(), vec2(10.0, 10.0)))
        };

        if resp.hovered() {
            if let Some(p) = pointer {
                let over_handle = self.selected_image().and_then(|id| handle_of(self, id)).is_some_and(|h| h.contains(p));
                let icon = if over_handle {
                    CursorIcon::ResizeNwSe
                } else if self.format_painter.is_some() {
                    CursorIcon::Crosshair
                } else {
                    CursorIcon::Text
                };
                ui.ctx().set_cursor_icon(icon);
            }
        }

        if pressed && resp.hovered() {
            self.text_input_focused = false;
            if let Some(p) = pointer {
                if let Some(id) = self.selected_image() {
                    if handle_of(self, id).is_some_and(|h| h.contains(p)) {
                        let w = self.doc.images[&id].w;
                        self.checkpoint(crate::app::EditKind::Other);
                        self.drag.resizing = Some((id, w, p));
                        return;
                    }
                }
                let same_spot = time - self.last_click.0 < 0.45 && self.last_click.1.distance(p) < 6.0;
                let count = if same_spot { self.last_click.2 + 1 } else { 1 };
                self.last_click = (time, p, count);
                let (page, pos) = self.hit_point(rect, lay, p);
                self.goal_x = None;
                match count {
                    1 => {
                        // Clicking a picture selects it.
                        let o = self.page_origin(rect, lay, page);
                        let local = p - o.to_vec2();
                        let img_hit = [pos, self.doc.prev(pos)].into_iter().find(|cand| {
                            self.image_box(lay, *cand).is_some_and(|(pg, r)| pg == page && r.contains(local))
                        });
                        if let (Some(ip), false) = (img_hit, shift) {
                            let next = self.doc.next(ip);
                            self.select(ip, next);
                        } else {
                            self.set_cursor(pos, shift);
                            self.drag.selecting = true;
                        }
                    }
                    2 => {
                        let (a, b) = self.doc.word_at(pos);
                        self.select(a, b);
                        self.drag.word_mode = Some((a, b));
                        self.drag.selecting = true;
                    }
                    _ => {
                        let len = self.doc.paras[pos.p].len();
                        self.select(Pos::new(pos.p, 0), Pos::new(pos.p, len));
                        self.drag.selecting = false;
                    }
                }
            }
        }

        if secondary && resp.hovered() {
            if let Some(p) = pointer {
                let (_, pos) = self.hit_point(rect, lay, p);
                let (a, b) = self.sel();
                if !(self.has_sel() && pos >= a && pos <= b) {
                    self.set_cursor(pos, false);
                }
            }
        }

        if down {
            if let (Some((id, w0, start)), Some(p)) = (self.drag.resizing, pointer) {
                if let Some(img) = self.doc.images.get(&id).cloned() {
                    let maxw = self.content_width_pt();
                    let w = (w0 + (p.x - start.x) / sc).clamp(8.0, maxw);
                    let im = self.doc.images.get_mut(&id).unwrap();
                    im.w = w;
                    im.h = w * img.px_h as f32 / img.px_w.max(1) as f32;
                    self.rev += 1;
                }
            } else if self.drag.selecting {
                if let Some(p) = pointer {
                    let (_, pos) = self.hit_point(rect, lay, p);
                    if let Some((a, b)) = self.drag.word_mode {
                        let (wa, wb) = self.doc.word_at(pos);
                        if pos < a {
                            self.anchor = b;
                            self.cursor = wa;
                        } else {
                            self.anchor = a;
                            self.cursor = wb.max(b);
                        }
                    } else {
                        self.cursor = pos;
                    }
                    self.blink = std::time::Instant::now();
                    if !ui.clip_rect().contains(p) {
                        self.scroll_to_caret = true;
                    }
                }
            }
        }

        if released {
            if self.drag.selecting && self.format_painter.is_some() {
                self.apply_format_painter();
            }
            self.drag = Default::default();
        }

        resp.context_menu(|ui| {
            ui.set_min_width(180.0);
            if ui.add_enabled(self.has_sel(), egui::Button::new("✂  Cut").shortcut_text("Ctrl+X")).clicked() {
                self.cut();
                ui.close();
            }
            if ui.add_enabled(self.has_sel(), egui::Button::new("🗐  Copy").shortcut_text("Ctrl+C")).clicked() {
                self.copy();
                ui.close();
            }
            if ui.add(egui::Button::new("📋  Paste").shortcut_text("Ctrl+V")).clicked() {
                self.paste_from_system();
                ui.close();
            }
            ui.separator();
            if ui.add(egui::Button::new("Select All").shortcut_text("Ctrl+A")).clicked() {
                let e = self.doc.end();
                self.select(Pos::default(), e);
                ui.close();
            }
            ui.separator();
            if ui.button("Paragraph…").clicked() {
                self.dlg.paragraph = true;
                ui.close();
            }
            ui.menu_button("Bullets / Numbering", |ui| {
                if ui.button("• Bullets").clicked() {
                    self.toggle_list(ListKind::Bullet);
                    ui.close();
                }
                if ui.button("1. Numbering").clicked() {
                    self.toggle_list(ListKind::Number);
                    ui.close();
                }
            });
            if ui.button("Insert Picture…").clicked() {
                ui.close();
                self.tab = Tab::Insert;
                self.insert_picture();
            }
        });
    }

    // ================================================================ painting

    fn texture(&mut self, ctx: &egui::Context, id: u32) -> Option<egui::TextureId> {
        if !self.textures.contains_key(&id) {
            let img = self.doc.images.get(&id)?;
            let rgba = image::load_from_memory(&img.bytes).ok()?.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            let tex = ctx.load_texture(format!("img{id}"), ci, egui::TextureOptions::LINEAR);
            self.textures.insert(id, tex);
        }
        self.textures.get(&id).map(|t| t.id())
    }

    fn paint(&mut self, ui: &Ui, rect: Rect, lay: &Layout) {
        let painter = ui.painter().clone();
        let clip = ui.clip_rect();
        let sc = lay.scale;
        let (sa, sb) = self.sel();
        let has_sel = self.has_sel();
        let sel_color = Color32::from_rgba_unmultiplied(0x33, 0x99, 0xFF, 80);
        let mark_color = Color32::from_rgb(0x52, 0x8B, 0xD6);
        let caret_on = (self.blink.elapsed().as_secs_f32() % 1.06) < 0.53;
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(265));

        if self.find.open && !self.find.query.is_empty() {
            self.refresh_find();
        }
        let matches = if self.find.open { self.find.matches.clone() } else { Vec::new() };
        let ids: Vec<u32> = self.doc.images.keys().copied().collect();
        for id in ids {
            self.texture(ui.ctx(), id);
        }

        for page in 0..lay.pages {
            let o = self.page_origin(rect, lay, page);
            let pr = Rect::from_min_size(o, vec2(lay.page_w, lay.page_h));
            if !pr.intersects(clip) {
                continue;
            }
            painter.rect_filled(pr.translate(vec2(2.0, 3.0)), 0.0, Color32::from_black_alpha(40));
            painter.rect_filled(pr, 0.0, Color32::WHITE);
            painter.rect_stroke(pr, 0.0, Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Outside);

            // Margin guides (corner marks, like Word).
            let [mt, mr, mb, ml] = lay.margins;
            let guide = Stroke::new(1.0, Color32::from_gray(190));
            for (cx, cy, dx, dy) in [
                (ml, mt, -1.0, -1.0),
                (lay.page_w - mr, mt, 1.0, -1.0),
                (ml, lay.page_h - mb, -1.0, 1.0),
                (lay.page_w - mr, lay.page_h - mb, 1.0, 1.0),
            ] {
                let c = o + vec2(cx, cy);
                painter.line_segment([c, c + vec2(dx * 12.0, 0.0)], guide);
                painter.line_segment([c, c + vec2(0.0, dy * 12.0)], guide);
            }

            self.paint_header_footer(&painter, lay, o, page);

            for li in lay.page_lines[page].clone() {
                let l = &lay.lines[li];
                let lr = Rect::from_min_size(o + vec2(0.0, l.y), vec2(lay.page_w, l.height));
                if !lr.intersects(clip) {
                    continue;
                }
                let para = &self.doc.paras[l.para];
                let pb = &lay.paras[l.para];
                let top = o.y + l.y;
                let base = top + l.baseline;
                let x_of = |k: usize| o.x + l.xs[k - l.start];

                if para.style == Style::Code {
                    painter.rect_filled(
                        Rect::from_min_max(pos2(o.x + pb.left - 4.0, top), pos2(o.x + pb.right + 4.0, top + l.height)),
                        0.0,
                        Color32::from_gray(243),
                    );
                }
                if para.style == Style::Quote && li == pb.lines.start {
                    let last = &lay.lines[pb.lines.end - 1];
                    if last.page == page {
                        painter.line_segment(
                            [pos2(o.x + pb.left - 10.0, top), pos2(o.x + pb.left - 10.0, o.y + last.y + last.height)],
                            Stroke::new(3.0, Color32::from_gray(200)),
                        );
                    }
                }

                let segs = layout::segments(para, l);
                // Highlights.
                for s in &segs {
                    if s.fmt.highlight[3] > 0 {
                        let r = Rect::from_min_max(pos2(x_of(s.start), top), pos2(x_of(s.end), top + l.height));
                        painter.rect_filled(r, 0.0, c32(s.fmt.highlight));
                    }
                }
                // Find matches.
                for (i, (ma, mb)) in matches.iter().enumerate() {
                    if ma.p != l.para || mb.o <= l.start || ma.o >= l.end {
                        continue;
                    }
                    let s = ma.o.max(l.start);
                    let e = mb.o.min(l.end);
                    let cur = self.find.current == Some(i);
                    let col = if cur { Color32::from_rgb(255, 170, 60) } else { Color32::from_rgb(255, 238, 150) };
                    painter.rect_filled(Rect::from_min_max(pos2(x_of(s), top), pos2(x_of(e), top + l.height)), 0.0, col);
                }
                // Selection.
                if has_sel && l.para >= sa.p && l.para <= sb.p {
                    let s = if l.para == sa.p { sa.o } else { 0 }.max(l.start);
                    let e = if l.para == sb.p { sb.o } else { usize::MAX }.min(l.end);
                    let extends = l.para < sb.p && l.last_in_para;
                    if s <= e {
                        let x0 = x_of(s);
                        let mut x1 = x_of(e);
                        if extends {
                            x1 += 6.0;
                        }
                        if x1 > x0 {
                            painter.rect_filled(Rect::from_min_max(pos2(x0, top), pos2(x1, top + l.height)), 0.0, sel_color);
                        }
                    }
                }

                // List marker.
                if l.start == 0 {
                    if let Some((label, mx)) = &pb.marker {
                        let mut f = para.fmt_at(0);
                        f.script = 0;
                        let (fid, res) = layout::font_for(&f, sc);
                        self.draw_text(&painter, label, fid, c32(f.color), res.fake_italic, false, pos2(o.x + mx, base));
                    }
                }

                // Text, pictures and decorations.
                for s in &segs {
                    let f = s.fmt;
                    let x0 = x_of(s.start);
                    let x1 = x_of(s.end);
                    if f.image != 0 {
                        if let Some(img) = self.doc.images.get(&f.image).cloned() {
                            let r = Rect::from_min_size(pos2(x0, base - img.h * sc), vec2(img.w * sc, img.h * sc));
                            match self.textures.get(&f.image).map(|t| t.id()) {
                                Some(t) => {
                                    painter.image(t, r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE)
                                }
                                None => painter.rect_filled(r, 0.0, Color32::from_gray(220)),
                            };
                            let selected = has_sel && sa == Pos::new(l.para, s.start) && sb == Pos::new(l.para, s.end);
                            if selected {
                                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(0x2B, 0x57, 0x9A)), StrokeKind::Outside);
                                painter.rect_filled(Rect::from_center_size(r.max, vec2(8.0, 8.0)), 0.0, Color32::WHITE);
                                painter.rect_stroke(
                                    Rect::from_center_size(r.max, vec2(8.0, 8.0)),
                                    0.0,
                                    Stroke::new(1.0, Color32::from_rgb(0x2B, 0x57, 0x9A)),
                                    StrokeKind::Inside,
                                );
                            }
                        }
                        continue;
                    }
                    let (fid, res) = layout::font_for(&f, sc);
                    let y = base + layout::script_shift(&f, sc);
                    if !s.blank {
                        let t: String = para.chars[s.start..s.end].iter().filter(|c| **c != OBJ).collect();
                        self.draw_text(&painter, &t, fid.clone(), c32(f.color), res.fake_italic, res.fake_bold, pos2(x0, y));
                    }
                    let th = (fid.size * 0.06).max(1.0);
                    if f.underline {
                        let uy = y + fid.size * 0.12;
                        painter.line_segment([pos2(x0, uy), pos2(x1, uy)], Stroke::new(th, c32(f.color)));
                    }
                    if f.strike {
                        let sy = y - fid.size * 0.28;
                        painter.line_segment([pos2(x0, sy), pos2(x1, sy)], Stroke::new(th, c32(f.color)));
                    }
                }

                // Formatting marks.
                if self.show_marks {
                    let mf = egui::FontId::proportional((11.0 * sc).max(6.0));
                    for k in l.start..l.end {
                        let c = para.chars[k];
                        let (x0, x1) = (x_of(k), x_of(k + 1));
                        match c {
                            ' ' => {
                                painter.circle_filled(pos2((x0 + x1) / 2.0, base - 3.0 * sc), 1.2 * sc.max(1.0), mark_color);
                            }
                            '\t' => {
                                painter.text(pos2(x0, base), egui::Align2::LEFT_BOTTOM, "→", mf.clone(), mark_color);
                            }
                            LINE_BREAK => {
                                painter.text(pos2(x0, base), egui::Align2::LEFT_BOTTOM, "↵", mf.clone(), mark_color);
                            }
                            PAGE_BREAK => {
                                let r = pos2(o.x + pb.right, base);
                                let left = pos2(x0 + 4.0, base - 4.0);
                                painter.line_segment([left, pos2(r.x, left.y)], Stroke::new(1.0, mark_color));
                                painter.text(
                                    pos2((left.x + r.x) / 2.0, left.y),
                                    egui::Align2::CENTER_CENTER,
                                    " Page Break ",
                                    mf.clone(),
                                    mark_color,
                                );
                            }
                            _ => {}
                        }
                    }
                    if l.last_in_para {
                        let x = x_of(l.end);
                        painter.text(pos2(x + 1.0, base), egui::Align2::LEFT_BOTTOM, "¶", mf.clone(), mark_color);
                    }
                }
            }
        }

        // Caret.
        if caret_on && !self.text_input_focused && (!has_sel || self.drag.selecting) && self.selected_image().is_none() {
            let (pg, x, y, h) = lay.caret(self.cursor);
            let ls = self.doc.paras[self.cursor.p].line_spacing.max(1.0);
            let o = self.page_origin(rect, lay, pg);
            let ch = h / ls;
            let top = o + vec2(x, y + (h - ch).min(0.0));
            painter.line_segment([top, top + vec2(0.0, ch)], Stroke::new(1.5, Color32::BLACK));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &self,
        painter: &egui::Painter,
        text: &str,
        font_id: egui::FontId,
        color: Color32,
        italics: bool,
        bold: bool,
        baseline: Pos2,
    ) {
        let size = font_id.size;
        let job = LayoutJob::single_section(
            text.to_string(),
            TextFormat { font_id, color, italics, ..Default::default() },
        );
        let galley = painter.layout_job(job);
        let asc = galley
            .rows
            .first()
            .and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y))
            .unwrap_or(size * 0.8);
        let pos = baseline - vec2(0.0, asc);
        if bold {
            painter.galley(pos + vec2((size * 0.04).max(0.6), 0.0), galley.clone(), color);
        }
        painter.galley(pos, galley, color);
    }

    fn paint_header_footer(&self, painter: &egui::Painter, lay: &Layout, o: Pos2, page: usize) {
        let [mt, mr, mb, ml] = lay.margins;
        let font = egui::FontId::new(
            10.0 * lay.scale,
            crate::fonts::resolve(crate::fonts::font_id(crate::fonts::default_font()), false, false).family,
        );
        let color = Color32::from_gray(0x59);
        let cx = o.x + ml + (lay.page_w - ml - mr) / 2.0;
        if !self.doc.header.is_empty() {
            let g = painter.layout_no_wrap(self.doc.header.clone(), font.clone(), color);
            self.draw_text(painter, &self.doc.header, font.clone(), color, false, false, pos2(cx - g.size().x / 2.0, o.y + mt / 2.0));
        }
        let mut footer = self.doc.footer.clone();
        if self.doc.page_numbers {
            if !footer.is_empty() {
                footer.push_str("    ");
            }
            footer.push_str(&format!("Page {} of {}", page + 1, lay.pages));
        }
        if !footer.is_empty() {
            let g = painter.layout_no_wrap(footer.clone(), font.clone(), color);
            self.draw_text(painter, &footer, font, color, false, false, pos2(cx - g.size().x / 2.0, o.y + lay.page_h - mb / 2.0));
        }
    }

    /// Horizontal ruler showing margins and the current paragraph indent.
    pub fn ruler(&self, ui: &mut Ui) {
        let Some(lay) = &self.layout else { return };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        let p = ui.painter_at(rect);
        let dark = ui.visuals().dark_mode;
        p.rect_filled(rect, 0.0, if dark { Color32::from_gray(45) } else { Color32::from_gray(235) });
        let total_w = (lay.page_w + 2.0 * GAP).max(rect.width());
        let x0 = rect.left() + ((total_w - lay.page_w) / 2.0).max(GAP);
        let [_, mr, _, ml] = lay.margins;
        let page = Rect::from_min_max(pos2(x0, rect.top() + 3.0), pos2(x0 + lay.page_w, rect.bottom() - 3.0));
        p.rect_filled(page, 0.0, Color32::from_gray(200));
        let content = Rect::from_min_max(pos2(x0 + ml, page.top()), pos2(x0 + lay.page_w - mr, page.bottom()));
        p.rect_filled(content, 0.0, Color32::WHITE);
        let inch = 72.0 * lay.scale;
        let tick = Color32::from_gray(110);
        let n = (lay.page_w / (inch / 8.0)) as i32;
        for i in -((ml / (inch / 8.0)) as i32)..=n {
            let x = content.left() + i as f32 * inch / 8.0;
            if x < page.left() || x > page.right() {
                continue;
            }
            if i % 8 == 0 {
                if i != 0 {
                    p.text(
                        pos2(x, page.center().y),
                        egui::Align2::CENTER_CENTER,
                        format!("{}", (i / 8).abs()),
                        egui::FontId::proportional(10.0),
                        tick,
                    );
                }
            } else {
                let h = if i % 4 == 0 { 5.0 } else if i % 2 == 0 { 3.0 } else { 0.0 };
                if h > 0.0 {
                    p.line_segment([pos2(x, page.center().y - h / 2.0), pos2(x, page.center().y + h / 2.0)], Stroke::new(1.0, tick));
                }
            }
        }
        // Indent markers for the current paragraph.
        if let Some(pb) = lay.paras.get(self.cursor.p) {
            let marker_x = pb.marker.as_ref().map(|m| m.1).unwrap_or(pb.left);
            let blue = Color32::from_rgb(0x55, 0x55, 0x55);
            let tri = |x: f32, down: bool| {
                let y = if down { page.top() } else { page.bottom() };
                let d = if down { 5.0 } else { -5.0 };
                vec![pos2(x0 + x - 4.0, y), pos2(x0 + x + 4.0, y), pos2(x0 + x, y + d)]
            };
            p.add(egui::Shape::convex_polygon(tri(marker_x, true), Color32::WHITE, Stroke::new(1.0, blue)));
            p.add(egui::Shape::convex_polygon(tri(pb.left, false), Color32::WHITE, Stroke::new(1.0, blue)));
            p.add(egui::Shape::convex_polygon(tri(pb.right, false), Color32::WHITE, Stroke::new(1.0, blue)));
        }
    }

    pub fn apply_fit(&mut self, avail: Vec2) {
        if let Some(page_width) = self.fit_request.take() {
            let pw = self.doc.page.width * layout::PT_TO_PX;
            let ph = self.doc.page.height * layout::PT_TO_PX;
            let z = if page_width {
                (avail.x - 2.0 * GAP - 20.0) / pw
            } else {
                ((avail.y - 2.0 * GAP) / ph).min((avail.x - 2.0 * GAP) / pw)
            };
            self.zoom = z.clamp(0.1, 5.0);
        }
    }
}
