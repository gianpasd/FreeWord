//! Ribbon, status bar, dialogs and the eframe entry point.

use crate::app::{After, App, Tab};
use crate::doc::{Align, ListKind, NO_COLOR, Rgba, Style};
use crate::editor::c32;
use crate::fonts;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, Rect, Response, RichText,
    Sense, Stroke, Ui, pos2, vec2,
};

const ACCENT: Color32 = Color32::from_rgb(0x2B, 0x57, 0x9A);
const GROUP_H: f32 = 66.0;

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.selection.bg_fill = if dark { Color32::from_rgb(0x3B, 0x6E, 0xC0) } else { Color32::from_rgb(0xC5, 0xD7, 0xF0) };
    v.selection.stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { ACCENT });
    if !dark {
        v.panel_fill = Color32::from_rgb(0xF3, 0xF3, 0xF3);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xF3, 0xF3, 0xF3);
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.button_padding = vec2(6.0, 3.0);
        s.spacing.item_spacing = vec2(4.0, 4.0);
    });
}

// ================================================================ widgets

#[derive(Clone, Copy)]
enum Icon {
    AlignLeft,
    AlignCenter,
    AlignRight,
    Justify,
    Bullets,
    Numbers,
    IndentMore,
    IndentLess,
    LineSpacing,
}

fn paint_icon(p: &egui::Painter, r: Rect, icon: Icon, col: Color32) {
    let s = Stroke::new(1.3, col);
    let c = r.center();
    let w = 14.0;
    let left = c.x - w / 2.0;
    let rows = [-4.5f32, -1.5, 1.5, 4.5];
    let line = |x0: f32, x1: f32, y: f32| p.line_segment([pos2(x0, c.y + y), pos2(x1, c.y + y)], s);
    match icon {
        Icon::AlignLeft => {
            for (i, y) in rows.iter().enumerate() {
                line(left, left + if i % 2 == 0 { w } else { w * 0.6 }, *y);
            }
        }
        Icon::AlignCenter => {
            for (i, y) in rows.iter().enumerate() {
                let ww = if i % 2 == 0 { w } else { w * 0.6 };
                line(c.x - ww / 2.0, c.x + ww / 2.0, *y);
            }
        }
        Icon::AlignRight => {
            for (i, y) in rows.iter().enumerate() {
                line(left + w - if i % 2 == 0 { w } else { w * 0.6 }, left + w, *y);
            }
        }
        Icon::Justify => {
            for y in rows {
                line(left, left + w, y);
            }
        }
        Icon::Bullets | Icon::Numbers => {
            for (i, y) in [-4.0f32, 0.0, 4.0].iter().enumerate() {
                if matches!(icon, Icon::Bullets) {
                    p.circle_filled(pos2(left + 1.5, c.y + y), 1.4, col);
                } else {
                    p.text(pos2(left + 1.5, c.y + y), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(6.5), col);
                }
                line(left + 5.0, left + w, *y);
            }
        }
        Icon::IndentMore | Icon::IndentLess => {
            for y in rows {
                line(left + 6.0, left + w, y);
            }
            let (a, b) = if matches!(icon, Icon::IndentMore) { (left, left + 4.0) } else { (left + 4.0, left) };
            p.add(egui::Shape::convex_polygon(
                vec![pos2(a, c.y - 3.0), pos2(a, c.y + 3.0), pos2(b, c.y)],
                col,
                Stroke::NONE,
            ));
        }
        Icon::LineSpacing => {
            for y in rows {
                line(left + 6.0, left + w, y);
            }
            p.line_segment([pos2(left + 2.0, c.y - 5.0), pos2(left + 2.0, c.y + 5.0)], s);
            p.line_segment([pos2(left, c.y - 3.0), pos2(left + 2.0, c.y - 5.5)], s);
            p.line_segment([pos2(left + 4.0, c.y - 3.0), pos2(left + 2.0, c.y - 5.5)], s);
            p.line_segment([pos2(left, c.y + 3.0), pos2(left + 2.0, c.y + 5.5)], s);
            p.line_segment([pos2(left + 4.0, c.y + 3.0), pos2(left + 2.0, c.y + 5.5)], s);
        }
    }
}

fn button_bg(ui: &Ui, r: Rect, resp: &Response, selected: bool) {
    let v = ui.visuals();
    let fill = if resp.is_pointer_button_down_on() {
        Some(v.selection.bg_fill.gamma_multiply(1.2))
    } else if selected {
        Some(v.selection.bg_fill)
    } else if resp.hovered() {
        Some(if v.dark_mode { Color32::from_gray(70) } else { Color32::from_rgb(0xDD, 0xE6, 0xF3) })
    } else {
        None
    };
    if let Some(f) = fill {
        ui.painter().rect_filled(r, CornerRadius::same(3), f);
        if selected {
            ui.painter().rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, ACCENT.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        }
    }
}

fn icon_btn(ui: &mut Ui, icon: Icon, selected: bool, tip: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    button_bg(ui, r, &resp, selected);
    paint_icon(ui.painter(), r, icon, ui.visuals().text_color());
    resp.on_hover_text(tip)
}

/// Small text-glyph button (B, I, U ...).
fn tool(ui: &mut Ui, text: RichText, selected: bool, tip: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    button_bg(ui, r, &resp, selected);
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let pos = r.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ui.visuals().text_color());
    resp.on_hover_text(tip)
}

fn big_btn(ui: &mut Ui, glyph: &str, label: &str, tip: &str) -> Response {
    big_btn_sel(ui, glyph, label, false, tip)
}

fn big_btn_sel(ui: &mut Ui, glyph: &str, label: &str, selected: bool, tip: &str) -> Response {
    let font = FontId::proportional(12.0);
    let lw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2((lw + 12.0).max(46.0), GROUP_H - 4.0), Sense::click());
    button_bg(ui, r, &resp, selected);
    let col = ui.visuals().text_color();
    ui.painter().text(pos2(r.center().x, r.top() + 20.0), Align2::CENTER_CENTER, glyph, FontId::proportional(24.0), ACCENT_FOR(ui));
    ui.painter().text(pos2(r.center().x, r.bottom() - 13.0), Align2::CENTER_CENTER, label, font, col);
    resp.on_hover_text(tip)
}

#[allow(non_snake_case)]
fn ACCENT_FOR(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_rgb(0x8F, 0xB4, 0xEA) } else { ACCENT }
}

/// Small button with a glyph and label side by side.
fn small_btn(ui: &mut Ui, glyph: &str, label: &str, tip: &str) -> Response {
    let font = FontId::proportional(12.5);
    let lw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(lw + 30.0, 20.0), Sense::click());
    button_bg(ui, r, &resp, false);
    ui.painter().text(pos2(r.left() + 11.0, r.center().y), Align2::CENTER_CENTER, glyph, FontId::proportional(14.0), ACCENT_FOR(ui));
    ui.painter().text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, label, font, ui.visuals().text_color());
    resp.on_hover_text(tip)
}

/// A ribbon group: content plus a caption, followed by a separator.
fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let r = ui.vertical(|ui| {
        let inner = ui.horizontal(|ui| {
            ui.set_min_height(GROUP_H);
            add(ui);
        });
        let w = inner.response.rect.width();
        ui.allocate_ui_with_layout(vec2(w, 14.0), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.set_min_width(w);
            ui.label(RichText::new(title).size(11.0).color(ui.visuals().weak_text_color()));
        });
    });
    let x = r.response.rect.right() + 4.0;
    let y0 = r.response.rect.top() + 4.0;
    let y1 = r.response.rect.bottom() - 4.0;
    ui.add_space(8.0);
    ui.painter().line_segment([pos2(x, y0), pos2(x, y1)], Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    ui.add_space(4.0);
}

const THEME_COLORS: [[u8; 3]; 10] = [
    [0xFF, 0xFF, 0xFF], [0x00, 0x00, 0x00], [0xE7, 0xE6, 0xE6], [0x44, 0x54, 0x6A], [0x44, 0x72, 0xC4],
    [0xED, 0x7D, 0x31], [0xA5, 0xA5, 0xA5], [0xFF, 0xC0, 0x00], [0x5B, 0x9B, 0xD5], [0x70, 0xAD, 0x47],
];
const STANDARD_COLORS: [[u8; 3]; 10] = [
    [0xC0, 0x00, 0x00], [0xFF, 0x00, 0x00], [0xFF, 0xC0, 0x00], [0xFF, 0xFF, 0x00], [0x92, 0xD0, 0x50],
    [0x00, 0xB0, 0x50], [0x00, 0xB0, 0xF0], [0x00, 0x70, 0xC0], [0x00, 0x20, 0x60], [0x70, 0x30, 0xA0],
];
const HIGHLIGHT_COLORS: [[u8; 3]; 15] = [
    [255, 255, 0], [0, 255, 0], [0, 255, 255], [255, 0, 255], [0, 0, 255], [255, 0, 0], [0, 0, 128], [0, 128, 128],
    [0, 128, 0], [128, 0, 128], [128, 0, 0], [128, 128, 0], [128, 128, 128], [192, 192, 192], [0, 0, 0],
];

fn rgba(c: [u8; 3]) -> Rgba {
    [c[0], c[1], c[2], 255]
}

fn swatch(ui: &mut Ui, c: Rgba, tip: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    ui.painter().rect_filled(r, 0.0, c32(c));
    let stroke = if resp.hovered() { Stroke::new(2.0, Color32::from_rgb(0xF2, 0x94, 0x36)) } else { Stroke::new(1.0, Color32::from_gray(160)) };
    ui.painter().rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    resp.on_hover_text(tip).clicked()
}

/// Shading variants of a theme color, like Word's palette columns.
fn tint(c: [u8; 3], t: f32) -> Rgba {
    let f = |v: u8| -> u8 {
        if t >= 0.0 {
            (v as f32 + (255.0 - v as f32) * t) as u8
        } else {
            (v as f32 * (1.0 + t)) as u8
        }
    };
    [f(c[0]), f(c[1]), f(c[2]), 255]
}

/// Font color palette; returns the picked color.
fn color_palette(ui: &mut Ui, custom: &mut Color32) -> Option<Rgba> {
    let mut picked = None;
    ui.set_min_width(200.0);
    if ui.button("Automatic").clicked() {
        picked = Some(crate::doc::BLACK);
    }
    ui.label(RichText::new("Theme Colors").strong());
    ui.horizontal(|ui| {
        for c in THEME_COLORS {
            if swatch(ui, rgba(c), "") {
                picked = Some(rgba(c));
            }
        }
    });
    for t in [0.8, 0.6, 0.4, -0.25, -0.5] {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for c in THEME_COLORS {
                let col = tint(c, t);
                if swatch(ui, col, "") {
                    picked = Some(col);
                }
            }
        });
    }
    ui.label(RichText::new("Standard Colors").strong());
    ui.horizontal(|ui| {
        for c in STANDARD_COLORS {
            if swatch(ui, rgba(c), "") {
                picked = Some(rgba(c));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("More Colors:");
        egui::color_picker::color_edit_button_srgba(ui, custom, egui::color_picker::Alpha::Opaque);
        if ui.button("Apply").clicked() {
            picked = Some([custom.r(), custom.g(), custom.b(), 255]);
        }
    });
    if picked.is_some() {
        ui.close();
    }
    picked
}

// ================================================================ eframe

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Ask before closing with unsaved changes.
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(After::Exit);
        }

        let title = format!("{}{} - FreeWord", self.title(), if self.dirty { " •" } else { "" });
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        // Ctrl+wheel zoom.
        let zd = ctx.input(|i| i.zoom_delta());
        if zd != 1.0 {
            self.zoom = (self.zoom * zd).clamp(0.1, 5.0);
        }

        // Open dropped files.
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()).filter(|p| p.is_file())) {
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp") {
                if let Ok(b) = std::fs::read(&path) {
                    self.insert_image_bytes(b);
                }
            } else if !self.dirty {
                self.open_path(&path);
            } else {
                self.error("Save or discard your changes before opening another document.");
            }
        }

        self.text_focus_frame = false;
        self.handle_events(&ctx);

        egui::Panel::top("ribbon").frame(Frame::NONE.fill(ui.visuals().panel_fill)).show(ui, |ui| self.ribbon(ui));
        egui::Panel::bottom("status").frame(Frame::NONE.fill(ACCENT).inner_margin(Margin::symmetric(8, 3))).show(ui, |ui| self.status_bar(ui));

        let bg = if self.dark { Color32::from_gray(30) } else { Color32::from_rgb(0xE6, 0xE6, 0xE6) };
        egui::CentralPanel::default().frame(Frame::NONE.fill(bg)).show(ui, |ui| {
            if self.show_ruler {
                self.ensure_layout();
                self.ruler(ui);
            }
            let avail = ui.available_size();
            self.viewport_h = avail.y;
            self.apply_fit(avail);
            egui::ScrollArea::both().id_salt("doc_scroll").auto_shrink([false, false]).show(ui, |ui| {
                self.canvas(ui);
            });
        });

        self.dialogs(&ctx);
        self.text_input_focused = self.text_focus_frame;
    }
}

impl App {
    fn track(&mut self, r: &Response) {
        if r.has_focus() {
            self.text_focus_frame = true;
        }
    }

    // ================================================================ ribbon

    fn ribbon(&mut self, ui: &mut Ui) {
        // Tab strip.
        Frame::NONE.fill(ACCENT).inner_margin(Margin::symmetric(6, 0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_height(30.0);
                ui.visuals_mut().override_text_color = Some(Color32::WHITE);
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().widgets.hovered.weak_bg_fill = Color32::from_rgb(0x3E, 0x6D, 0xB5);
                ui.visuals_mut().widgets.open.weak_bg_fill = Color32::from_rgb(0x1E, 0x40, 0x75);
                ui.menu_button(RichText::new("  File  ").size(14.0), |ui| {
                    ui.visuals_mut().override_text_color = None;
                    self.file_menu(ui);
                });
                for (tab, name) in [
                    (Tab::Home, "Home"),
                    (Tab::Insert, "Insert"),
                    (Tab::Layout, "Layout"),
                    (Tab::Review, "Review"),
                    (Tab::View, "View"),
                ] {
                    let sel = self.tab == tab;
                    let font = FontId::proportional(14.0);
                    let w = ui.painter().layout_no_wrap(name.into(), font.clone(), Color32::WHITE).size().x + 22.0;
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
                    let p = ui.painter();
                    if sel {
                        let tab_rect = Rect::from_min_max(pos2(r.left(), r.top() + 4.0), r.max);
                        p.rect_filled(tab_rect, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, ui.style().visuals.panel_fill);
                    } else if resp.hovered() {
                        p.rect_filled(r, 0.0, Color32::from_rgb(0x3E, 0x6D, 0xB5));
                    }
                    let col = if sel { ACCENT_FOR(ui) } else { Color32::WHITE };
                    p.text(r.center() + vec2(0.0, 2.0), Align2::CENTER_CENTER, name, font, col);
                    if resp.clicked() {
                        self.tab = tab;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    if ui.add_enabled(self.can_redo(), egui::Button::new("↷").frame(false)).on_hover_text("Redo (Ctrl+Y)").clicked() {
                        self.redo();
                    }
                    if ui.add_enabled(self.can_undo(), egui::Button::new("↶").frame(false)).on_hover_text("Undo (Ctrl+Z)").clicked() {
                        self.undo();
                    }
                    if ui.add(egui::Button::new("💾").frame(false)).on_hover_text("Save (Ctrl+S)").clicked() {
                        self.save();
                    }
                });
            });
        });

        // Ribbon body.
        Frame::NONE.inner_margin(Margin { left: 8, right: 8, top: 4, bottom: 2 }).show(ui, |ui| {
            egui::ScrollArea::horizontal().id_salt("ribbon_scroll").show(ui, |ui| {
                ui.horizontal_top(|ui| match self.tab {
                    Tab::Home => self.home_tab(ui),
                    Tab::Insert => self.insert_tab(ui),
                    Tab::Layout => self.layout_tab(ui),
                    Tab::Review => self.review_tab(ui),
                    Tab::View => self.view_tab(ui),
                });
            });
        });
        let r = ui.max_rect();
        ui.painter().line_segment(
            [pos2(r.left(), ui.min_rect().bottom()), pos2(r.right(), ui.min_rect().bottom())],
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(230.0);
        let item = |ui: &mut Ui, label: &str, sc: &str| ui.add(egui::Button::new(label).shortcut_text(sc));
        if item(ui, "📄  New", "Ctrl+N").clicked() {
            ui.close();
            self.guarded(After::New);
        }
        if item(ui, "📂  Open…", "Ctrl+O").clicked() {
            ui.close();
            self.guarded(After::Open);
        }
        ui.separator();
        if item(ui, "💾  Save", "Ctrl+S").clicked() {
            ui.close();
            self.save();
        }
        if item(ui, "    Save As…", "Ctrl+Shift+S").clicked() {
            ui.close();
            self.save_as();
        }
        ui.separator();
        if item(ui, "📕  Export as PDF…", "").clicked() {
            ui.close();
            self.export_pdf();
        }
        if item(ui, "🌐  Export as Web Page…", "").clicked() {
            ui.close();
            self.export_html();
        }
        if item(ui, "🗒  Export as Plain Text…", "").clicked() {
            ui.close();
            self.export_text();
        }
        ui.separator();
        if item(ui, "🖶  Print…", "Ctrl+P").clicked() {
            ui.close();
            self.print();
        }
        if item(ui, "    Page Setup…", "").clicked() {
            ui.close();
            self.dlg.page_setup = true;
        }
        ui.separator();
        if item(ui, "    Keyboard Shortcuts", "").clicked() {
            ui.close();
            self.dlg.shortcuts = true;
        }
        if item(ui, "    About FreeWord", "").clicked() {
            ui.close();
            self.dlg.about = true;
        }
        ui.separator();
        if item(ui, "    Exit", "Alt+F4").clicked() {
            ui.close();
            self.guarded(After::Exit);
        }
    }

    fn home_tab(&mut self, ui: &mut Ui) {
        let f = self.current_fmt();
        let para = self.current_para().clone();

        group(ui, "Clipboard", |ui| {
            if big_btn(ui, "📋", "Paste", "Paste (Ctrl+V)").clicked() {
                self.paste_from_system();
            }
            ui.vertical(|ui| {
                if small_btn(ui, "✂", "Cut", "Cut (Ctrl+X)").clicked() {
                    self.cut();
                }
                if small_btn(ui, "🗐", "Copy", "Copy (Ctrl+C)").clicked() {
                    self.copy();
                }
                let fp = self.format_painter.is_some();
                let r = small_btn(ui, "🖌", if fp { "Painting…" } else { "Format Painter" }, "Copy formatting from one place and apply it to another");
                if r.clicked() {
                    self.start_format_painter();
                }
            });
        });

        group(ui, "Font", |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let name = fonts::font_name(f.font);
                    egui::ComboBox::from_id_salt("font_family").selected_text(&name).width(150.0).height(400.0).show_ui(ui, |ui| {
                        for n in fonts::available() {
                            let fam = fonts::resolve(fonts::font_id(&n), false, false).family;
                            let text = RichText::new(&n).font(FontId::new(15.0, fam));
                            if ui.selectable_label(n.eq_ignore_ascii_case(&name), text).clicked() {
                                let id = fonts::font_id(&n);
                                self.apply_fmt(|c| c.font = id);
                            }
                        }
                    });
                    let size_text = if f.size.fract() == 0.0 { format!("{}", f.size) } else { format!("{:.1}", f.size) };
                    egui::ComboBox::from_id_salt("font_size").selected_text(size_text).width(48.0).height(400.0).show_ui(ui, |ui| {
                        for s in App::SIZES {
                            if ui.selectable_label((f.size - s).abs() < 0.01, format!("{s}")).clicked() {
                                self.apply_fmt(|c| c.size = s);
                            }
                        }
                    });
                    if tool(ui, RichText::new("A▴").size(13.0), false, "Increase Font Size (Ctrl+])").clicked() {
                        self.grow_font(true);
                    }
                    if tool(ui, RichText::new("A▾").size(11.0), false, "Decrease Font Size (Ctrl+[)").clicked() {
                        self.grow_font(false);
                    }
                    ui.menu_button(RichText::new("Aa▾"), |ui| {
                        for (i, label) in ["Sentence case.", "lowercase", "UPPERCASE", "Capitalize Each Word", "tOGGLE cASE"].iter().enumerate() {
                            if ui.button(*label).clicked() {
                                self.change_case(i as u8);
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Change Case");
                    if tool(ui, RichText::new("A⌫").size(12.0), false, "Clear Formatting (Ctrl+Space)").clicked() {
                        self.clear_formatting();
                    }
                });
                ui.horizontal(|ui| {
                    if tool(ui, RichText::new("B").strong().size(15.0), f.bold, "Bold (Ctrl+B)").clicked() {
                        self.toggle_bold();
                    }
                    if tool(ui, RichText::new("I").italics().size(15.0), f.italic, "Italic (Ctrl+I)").clicked() {
                        self.toggle_italic();
                    }
                    if tool(ui, RichText::new("U").underline().size(15.0), f.underline, "Underline (Ctrl+U)").clicked() {
                        self.toggle_underline();
                    }
                    if tool(ui, RichText::new("ab").strikethrough().size(14.0), f.strike, "Strikethrough (Ctrl+Shift+D)").clicked() {
                        self.toggle_strike();
                    }
                    if tool(ui, RichText::new("x₂").size(14.0), f.script == -1, "Subscript (Ctrl+=)").clicked() {
                        self.toggle_script(-1);
                    }
                    if tool(ui, RichText::new("x²").size(14.0), f.script == 1, "Superscript (Ctrl+Shift+=)").clicked() {
                        self.toggle_script(1);
                    }
                    ui.add_space(4.0);
                    // Highlight color.
                    let hl = self.last_highlight;
                    let r = tool(ui, RichText::new("ab").size(14.0), false, "Text Highlight Color");
                    ui.painter().rect_filled(Rect::from_min_size(r.rect.left_bottom() + vec2(5.0, -5.0), vec2(16.0, 3.0)), 0.0, c32(hl));
                    if r.clicked() {
                        self.apply_fmt(|c| c.highlight = hl);
                    }
                    ui.menu_button("▾", |ui| {
                        ui.set_min_width(140.0);
                        egui::Grid::new("hl").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                            for (i, c) in HIGHLIGHT_COLORS.iter().enumerate() {
                                if swatch(ui, rgba(*c), "") {
                                    self.last_highlight = rgba(*c);
                                    self.apply_fmt(|f| f.highlight = rgba(*c));
                                    ui.close();
                                }
                                if i % 5 == 4 {
                                    ui.end_row();
                                }
                            }
                        });
                        if ui.button("No Color").clicked() {
                            self.apply_fmt(|f| f.highlight = NO_COLOR);
                            ui.close();
                        }
                    });
                    // Font color.
                    let fc = self.last_color;
                    let r = tool(ui, RichText::new("A").size(15.0), false, "Font Color");
                    ui.painter().rect_filled(Rect::from_min_size(r.rect.left_bottom() + vec2(5.0, -5.0), vec2(16.0, 3.0)), 0.0, c32(fc));
                    if r.clicked() {
                        self.apply_fmt(|c| c.color = fc);
                    }
                    ui.menu_button("▾", |ui| {
                        let mut custom = c32(self.last_color);
                        if let Some(c) = color_palette(ui, &mut custom) {
                            self.last_color = c;
                            self.apply_fmt(|f| f.color = c);
                        }
                    });
                });
            });
        });

        group(ui, "Paragraph", |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if icon_btn(ui, Icon::Bullets, para.list == ListKind::Bullet, "Bullets (Ctrl+Shift+L)").clicked() {
                        self.toggle_list(ListKind::Bullet);
                    }
                    if icon_btn(ui, Icon::Numbers, para.list == ListKind::Number, "Numbering").clicked() {
                        self.toggle_list(ListKind::Number);
                    }
                    if icon_btn(ui, Icon::IndentLess, false, "Decrease Indent (Ctrl+Shift+M)").clicked() {
                        self.indent(-1);
                    }
                    if icon_btn(ui, Icon::IndentMore, false, "Increase Indent (Ctrl+M)").clicked() {
                        self.indent(1);
                    }
                    if tool(ui, RichText::new("¶").size(15.0), self.show_marks, "Show/Hide formatting marks").clicked() {
                        self.show_marks = !self.show_marks;
                    }
                });
                ui.horizontal(|ui| {
                    for (a, icon, tip) in [
                        (Align::Left, Icon::AlignLeft, "Align Left (Ctrl+L)"),
                        (Align::Center, Icon::AlignCenter, "Center (Ctrl+E)"),
                        (Align::Right, Icon::AlignRight, "Align Right (Ctrl+R)"),
                        (Align::Justify, Icon::Justify, "Justify (Ctrl+J)"),
                    ] {
                        if icon_btn(ui, icon, para.align == a, tip).clicked() {
                            self.set_align(a);
                        }
                    }
                    let r = icon_btn(ui, Icon::LineSpacing, false, "Line and Paragraph Spacing");
                    egui::Popup::menu(&r).show(|ui| {
                        for v in [1.0, 1.15, 1.5, 2.0, 2.5, 3.0] {
                            let label = format!("{}{v}", if (para.line_spacing - v).abs() < 0.01 { "✔ " } else { "    " });
                            if ui.button(label).clicked() {
                                self.set_line_spacing(v);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Paragraph Options…").clicked() {
                            self.dlg.paragraph = true;
                            ui.close();
                        }
                    });
                });
            });
        });

        group(ui, "Styles", |ui| {
            for st in Style::ALL {
                let sel = para.style == st;
                let (r, resp) = ui.allocate_exact_size(vec2(74.0, GROUP_H - 4.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(r, CornerRadius::same(2), Color32::WHITE);
                let stroke = if sel {
                    Stroke::new(2.0, ACCENT)
                } else if resp.hovered() {
                    Stroke::new(1.0, Color32::from_rgb(0x8F, 0xB4, 0xEA))
                } else {
                    Stroke::new(1.0, Color32::from_gray(215))
                };
                p.rect_stroke(r, CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
                let sf = st.char_fmt();
                let res = fonts::resolve(sf.font, sf.bold, sf.italic);
                let size = (sf.size * 1.25).min(20.0);
                let job = egui::text::LayoutJob::single_section(
                    "AaBbCc".into(),
                    egui::TextFormat {
                        font_id: FontId::new(size, res.family),
                        color: c32(sf.color),
                        italics: sf.italic || res.fake_italic,
                        ..Default::default()
                    },
                );
                let g = ui.painter().layout_job(job);
                let clip = ui.painter().with_clip_rect(r.shrink(3.0));
                clip.galley(pos2(r.center().x - g.size().x / 2.0, r.top() + 20.0 - g.size().y / 2.0), g, Color32::BLACK);
                p.text(pos2(r.center().x, r.bottom() - 11.0), Align2::CENTER_CENTER, st.name(), FontId::proportional(11.0), Color32::from_gray(60));
                if resp.on_hover_text(format!("Apply the {} style", st.name())).clicked() {
                    self.set_style(st);
                }
            }
        });

        group(ui, "Editing", |ui| {
            ui.vertical(|ui| {
                if small_btn(ui, "🔍", "Find", "Find (Ctrl+F)").clicked() {
                    self.find.open = true;
                    self.find.replace = false;
                    self.find.focus_query = true;
                }
                if small_btn(ui, "⇄", "Replace", "Replace (Ctrl+H)").clicked() {
                    self.find.open = true;
                    self.find.replace = true;
                    self.find.focus_query = true;
                }
                if small_btn(ui, "⬚", "Select All", "Select All (Ctrl+A)").clicked() {
                    let e = self.doc.end();
                    self.select(crate::doc::Pos::default(), e);
                }
            });
        });
    }

    fn insert_tab(&mut self, ui: &mut Ui) {
        group(ui, "Pages", |ui| {
            if big_btn(ui, "⤓", "Page Break", "Start the next page at the cursor (Ctrl+Enter)").clicked() {
                self.insert_page_break();
            }
            if big_btn(ui, "↵", "Line Break", "Start a new line in the same paragraph (Shift+Enter)").clicked() {
                self.insert_line_break();
            }
        });
        group(ui, "Illustrations", |ui| {
            if big_btn(ui, "🖼", "Pictures", "Insert a picture from a file").clicked() {
                self.insert_picture();
            }
        });
        if let Some(id) = self.selected_image() {
            group(ui, "Picture Size", |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        for pct in [25.0, 50.0, 75.0, 100.0] {
                            if ui.button(format!("{pct}%")).clicked() {
                                self.scale_image(id, pct / 100.0);
                            }
                        }
                    });
                    if ui.button("Fit to page width").clicked() {
                        self.scale_image(id, 100.0);
                    }
                    ui.label(RichText::new("Tip: drag the corner handle to resize").size(11.0).weak());
                });
            });
        }
        group(ui, "Header & Footer", |ui| {
            if big_btn(ui, "▤", "Header & Footer", "Edit the header and footer").clicked() {
                self.dlg.header_footer = true;
            }
            if big_btn_sel(ui, "#", "Page Number", self.doc.page_numbers, "Show page numbers in the footer").clicked() {
                self.checkpoint(crate::app::EditKind::Other);
                self.doc.page_numbers = !self.doc.page_numbers;
            }
        });
        group(ui, "Text", |ui| {
            let r = big_btn(ui, "📅", "Date & Time", "Insert the current date or time");
            egui::Popup::menu(&r).show(|ui| {
                let now = chrono::Local::now();
                for f in ["%-m/%-d/%Y", "%A, %B %-d, %Y", "%B %-d, %Y", "%Y-%m-%d", "%d %B %Y", "%-I:%M %p", "%H:%M", "%-m/%-d/%Y %-I:%M %p"] {
                    if ui.button(now.format(f).to_string()).clicked() {
                        self.insert_date(f);
                        ui.close();
                    }
                }
            });
        });
        group(ui, "Symbols", |ui| {
            let r = big_btn(ui, "Ω", "Symbol", "Insert a symbol");
            egui::Popup::menu(&r).show(|ui| {
                egui::Grid::new("quick_symbols").show(ui, |ui| {
                    for (i, s) in ["©", "®", "™", "€", "£", "¥", "§", "°", "±", "≠", "≤", "≥", "÷", "×", "∞", "µ", "α", "β", "π", "Ω", "—", "–", "…", "•"].iter().enumerate() {
                        if ui.button(RichText::new(*s).size(16.0)).clicked() {
                            self.insert_typed(s);
                            ui.close();
                        }
                        if i % 8 == 7 {
                            ui.end_row();
                        }
                    }
                });
                if ui.button("More Symbols…").clicked() {
                    self.dlg.symbols = true;
                    ui.close();
                }
            });
        });
    }

    fn layout_tab(&mut self, ui: &mut Ui) {
        group(ui, "Page Setup", |ui| {
            let r = big_btn(ui, "▣", "Margins", "Select the margin sizes");
            egui::Popup::menu(&r).show(|ui| {
                for (name, m) in [
                    ("Normal  (1\" all sides)", [72.0, 72.0, 72.0, 72.0]),
                    ("Narrow  (0.5\" all sides)", [36.0; 4]),
                    ("Moderate  (1\" × 0.75\")", [72.0, 54.0, 72.0, 54.0]),
                    ("Wide  (1\" × 2\")", [72.0, 144.0, 72.0, 144.0]),
                ] {
                    if ui.button(name).clicked() {
                        self.checkpoint(crate::app::EditKind::Other);
                        self.doc.page.margins = m;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Custom Margins…").clicked() {
                    self.dlg.page_setup = true;
                    ui.close();
                }
            });
            let r = big_btn(ui, "⟲", "Orientation", "Portrait or landscape");
            egui::Popup::menu(&r).show(|ui| {
                let pg = self.doc.page.clone();
                let landscape = pg.width > pg.height;
                if ui.selectable_label(!landscape, "Portrait").clicked() && landscape {
                    self.checkpoint(crate::app::EditKind::Other);
                    self.doc.page.width = pg.height;
                    self.doc.page.height = pg.width;
                    ui.close();
                }
                if ui.selectable_label(landscape, "Landscape").clicked() && !landscape {
                    self.checkpoint(crate::app::EditKind::Other);
                    self.doc.page.width = pg.height;
                    self.doc.page.height = pg.width;
                    ui.close();
                }
            });
            let r = big_btn(ui, "▯", "Size", "Choose a paper size");
            egui::Popup::menu(&r).show(|ui| {
                for (name, w, h) in [
                    ("Letter  8.5\" × 11\"", 612.0, 792.0),
                    ("Legal  8.5\" × 14\"", 612.0, 1008.0),
                    ("Executive  7.25\" × 10.5\"", 522.0, 756.0),
                    ("A4  21 cm × 29.7 cm", 595.3, 841.9),
                    ("A5  14.8 cm × 21 cm", 419.5, 595.3),
                ] {
                    if ui.button(name).clicked() {
                        self.checkpoint(crate::app::EditKind::Other);
                        let landscape = self.doc.page.width > self.doc.page.height;
                        let (w, h) = if landscape { (h, w) } else { (w, h) };
                        self.doc.page.width = w;
                        self.doc.page.height = h;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Custom Size…").clicked() {
                    self.dlg.page_setup = true;
                    ui.close();
                }
            });
        });
        let para = self.current_para().clone();
        group(ui, "Paragraph", |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Indent level:");
                    if icon_btn(ui, Icon::IndentLess, false, "Decrease Indent").clicked() {
                        self.indent(-1);
                    }
                    ui.label(format!("{}", para.level));
                    if icon_btn(ui, Icon::IndentMore, false, "Increase Indent").clicked() {
                        self.indent(1);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Before:");
                    let mut b = para.space_before;
                    if ui.add(egui::DragValue::new(&mut b).range(0.0..=200.0).speed(1.0).suffix(" pt")).changed() {
                        self.apply_para(|p| p.space_before = b);
                    }
                    ui.label("After:");
                    let mut a = para.space_after;
                    if ui.add(egui::DragValue::new(&mut a).range(0.0..=200.0).speed(1.0).suffix(" pt")).changed() {
                        self.apply_para(|p| p.space_after = a);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Line spacing:");
                    let mut ls = para.line_spacing;
                    if ui.add(egui::DragValue::new(&mut ls).range(0.5..=5.0).speed(0.05).fixed_decimals(2)).changed() {
                        self.set_line_spacing(ls);
                    }
                });
            });
        });
    }

    fn review_tab(&mut self, ui: &mut Ui) {
        group(ui, "Proofing", |ui| {
            if big_btn(ui, "123", "Word Count", "Find the number of words, characters and paragraphs").clicked() {
                self.dlg.word_count = true;
            }
        });
        group(ui, "Find", |ui| {
            if big_btn(ui, "🔍", "Find", "Find text (Ctrl+F)").clicked() {
                self.find.open = true;
                self.find.replace = false;
                self.find.focus_query = true;
            }
            if big_btn(ui, "⇄", "Replace", "Replace text (Ctrl+H)").clicked() {
                self.find.open = true;
                self.find.replace = true;
                self.find.focus_query = true;
            }
        });
        group(ui, "Export", |ui| {
            if big_btn(ui, "📕", "PDF", "Export the document as PDF").clicked() {
                self.export_pdf();
            }
        });
    }

    fn view_tab(&mut self, ui: &mut Ui) {
        group(ui, "Show", |ui| {
            ui.vertical(|ui| {
                ui.checkbox(&mut self.show_ruler, "Ruler");
                ui.checkbox(&mut self.show_marks, "Formatting marks");
                let mut dark = self.dark;
                if ui.checkbox(&mut dark, "Dark mode").changed() {
                    self.dark = dark;
                    apply_theme(ui.ctx(), dark);
                }
            });
        });
        group(ui, "Zoom", |ui| {
            if big_btn(ui, "🔍", "Zoom", "Zoom in").clicked() {
                self.zoom = (self.zoom * 1.1).min(5.0);
            }
            if big_btn(ui, "1:1", "100%", "Zoom to 100%").clicked() {
                self.zoom = 1.0;
            }
            if big_btn(ui, "▯", "One Page", "Fit a whole page in the window").clicked() {
                self.fit_request = Some(false);
            }
            if big_btn(ui, "↔", "Page Width", "Fit the page width to the window").clicked() {
                self.fit_request = Some(true);
            }
        });
    }

    // ================================================================ status bar

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            let (pages, page) = match &self.layout {
                Some(l) => (l.pages, l.caret(self.doc.clamp(self.cursor)).0 + 1),
                None => (1, 1),
            };
            ui.label(format!("Page {page} of {pages}"));
            ui.add_space(12.0);
            let st = self.doc.stats();
            if self.has_sel() {
                let (a, b) = self.sel();
                let words = self.doc.plain_text(a, b).split_whitespace().count();
                ui.label(format!("{words} of {} words", st.words));
            } else {
                ui.label(format!("{} words", st.words));
            }
            if let Some((msg, t)) = &self.status {
                if t.elapsed().as_secs() < 6 {
                    ui.add_space(12.0);
                    ui.label(msg);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.add(egui::Button::new("+").frame(false)).clicked() {
                    self.zoom = ((self.zoom * 10.0).round() / 10.0 + 0.1).min(5.0);
                }
                let mut z = self.zoom;
                ui.spacing_mut().slider_width = 110.0;
                if ui.add(egui::Slider::new(&mut z, 0.1..=5.0).logarithmic(true).show_value(false)).changed() {
                    self.zoom = z;
                }
                if ui.add(egui::Button::new("−").frame(false)).clicked() {
                    self.zoom = ((self.zoom * 10.0).round() / 10.0 - 0.1).max(0.1);
                }
            });
        });
    }

    // ================================================================ dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        // Unsaved-changes prompt.
        if let Some(action) = self.confirm {
            let mut choice = None;
            egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.heading("FreeWord");
                ui.add_space(6.0);
                ui.label(format!("Do you want to save changes to {}?", self.title()));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("  Save  ").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Don't Save").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(2);
                    }
                });
            });
            match choice {
                Some(0) => {
                    self.confirm = None;
                    if self.save() {
                        self.run_after(action);
                    }
                }
                Some(1) => {
                    self.confirm = None;
                    self.dirty = false;
                    self.run_after(action);
                }
                Some(2) => self.confirm = None,
                _ => {}
            }
        }

        if let Some(msg) = self.dlg.error.clone() {
            let r = egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("FreeWord");
                ui.add_space(6.0);
                ui.label(&msg);
                ui.add_space(10.0);
                ui.button("OK").clicked()
            });
            if r.inner || r.should_close() {
                self.dlg.error = None;
            }
        }

        self.find_dialog(ctx);

        if self.dlg.word_count {
            let st = self.doc.stats();
            let pages = self.layout.as_ref().map(|l| l.pages).unwrap_or(1);
            let mut open = true;
            egui::Window::new("Word Count").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                egui::Grid::new("wc").num_columns(2).spacing(vec2(30.0, 6.0)).show(ui, |ui| {
                    for (k, v) in [
                        ("Pages", pages),
                        ("Words", st.words),
                        ("Characters (no spaces)", st.chars_no_spaces),
                        ("Characters (with spaces)", st.chars),
                        ("Paragraphs", st.paragraphs),
                    ] {
                        ui.label(k);
                        ui.label(RichText::new(v.to_string()).strong());
                        ui.end_row();
                    }
                });
            });
            self.dlg.word_count = open;
        }

        if self.dlg.symbols {
            let mut open = true;
            let mut insert = None;
            egui::Window::new("Symbol").open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
                let ranges: [(&str, std::ops::RangeInclusive<u32>); 6] = [
                    ("Latin-1", 0xA1..=0xFF),
                    ("Greek", 0x391..=0x3C9),
                    ("Punctuation", 0x2010..=0x2044),
                    ("Currency", 0x20A0..=0x20BF),
                    ("Arrows", 0x2190..=0x21FF),
                    ("Math", 0x2200..=0x22FF),
                ];
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (name, range) in ranges {
                        ui.label(RichText::new(name).strong());
                        ui.horizontal_wrapped(|ui| {
                            for cp in range {
                                if let Some(c) = char::from_u32(cp) {
                                    if c.is_control() || c.is_whitespace() {
                                        continue;
                                    }
                                    let b = egui::Button::new(RichText::new(c.to_string()).size(16.0)).min_size(vec2(28.0, 28.0));
                                    if ui.add(b).on_hover_text(format!("U+{cp:04X}")).clicked() {
                                        insert = Some(c);
                                    }
                                }
                            }
                        });
                    }
                });
            });
            if let Some(c) = insert {
                self.insert_typed(&c.to_string());
            }
            self.dlg.symbols = open;
        }

        if self.dlg.header_footer {
            let mut open = true;
            let mut header = self.doc.header.clone();
            let mut footer = self.doc.footer.clone();
            let mut nums = self.doc.page_numbers;
            egui::Window::new("Header & Footer").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("Header text:");
                let r = ui.add(egui::TextEdit::singleline(&mut header).desired_width(320.0));
                self.track(&r);
                ui.label("Footer text:");
                let r = ui.add(egui::TextEdit::singleline(&mut footer).desired_width(320.0));
                self.track(&r);
                ui.checkbox(&mut nums, "Show page numbers (Page X of Y)");
            });
            if header != self.doc.header || footer != self.doc.footer || nums != self.doc.page_numbers {
                self.checkpoint(crate::app::EditKind::Typing);
                self.doc.header = header;
                self.doc.footer = footer;
                self.doc.page_numbers = nums;
            }
            self.dlg.header_footer = open;
        }

        if self.dlg.page_setup {
            let mut open = true;
            let mut pg = self.doc.page.clone();
            egui::Window::new("Page Setup").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                let inch = |ui: &mut Ui, label: &str, v: &mut f32, max: f32| {
                    ui.label(label);
                    let mut i = *v / 72.0;
                    if ui.add(egui::DragValue::new(&mut i).range(0.0..=max).speed(0.01).fixed_decimals(2).suffix("\"")).changed() {
                        *v = i * 72.0;
                    }
                };
                ui.label(RichText::new("Margins").strong());
                egui::Grid::new("margins").num_columns(4).show(ui, |ui| {
                    inch(ui, "Top:", &mut pg.margins[0], 5.0);
                    inch(ui, "Bottom:", &mut pg.margins[2], 5.0);
                    ui.end_row();
                    inch(ui, "Left:", &mut pg.margins[3], 5.0);
                    inch(ui, "Right:", &mut pg.margins[1], 5.0);
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Paper size").strong());
                egui::Grid::new("paper").num_columns(4).show(ui, |ui| {
                    inch(ui, "Width:", &mut pg.width, 22.0);
                    inch(ui, "Height:", &mut pg.height, 22.0);
                    ui.end_row();
                });
            });
            pg.width = pg.width.max(144.0);
            pg.height = pg.height.max(144.0);
            pg.margins[1] = pg.margins[1].min(pg.width / 2.0 - 36.0).max(0.0);
            pg.margins[3] = pg.margins[3].min(pg.width / 2.0 - 36.0).max(0.0);
            pg.margins[0] = pg.margins[0].min(pg.height / 2.0 - 36.0).max(0.0);
            pg.margins[2] = pg.margins[2].min(pg.height / 2.0 - 36.0).max(0.0);
            if pg != self.doc.page {
                self.checkpoint(crate::app::EditKind::Typing);
                self.doc.page = pg;
            }
            self.dlg.page_setup = open;
        }

        if self.dlg.paragraph {
            let mut open = true;
            let mut p = self.current_para().clone();
            let before = p.clone();
            egui::Window::new("Paragraph").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                egui::Grid::new("para").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
                    ui.label("Alignment:");
                    egui::ComboBox::from_id_salt("pa").selected_text(format!("{:?}", p.align)).show_ui(ui, |ui| {
                        for a in [Align::Left, Align::Center, Align::Right, Align::Justify] {
                            ui.selectable_value(&mut p.align, a, format!("{a:?}"));
                        }
                    });
                    ui.end_row();
                    ui.label("Indent level:");
                    ui.add(egui::DragValue::new(&mut p.level).range(0..=8));
                    ui.end_row();
                    ui.label("Spacing before:");
                    ui.add(egui::DragValue::new(&mut p.space_before).range(0.0..=200.0).suffix(" pt"));
                    ui.end_row();
                    ui.label("Spacing after:");
                    ui.add(egui::DragValue::new(&mut p.space_after).range(0.0..=200.0).suffix(" pt"));
                    ui.end_row();
                    ui.label("Line spacing:");
                    ui.add(egui::DragValue::new(&mut p.line_spacing).range(0.5..=5.0).speed(0.05).fixed_decimals(2));
                    ui.end_row();
                });
            });
            if p.align != before.align
                || p.level != before.level
                || p.space_before != before.space_before
                || p.space_after != before.space_after
                || p.line_spacing != before.line_spacing
            {
                self.apply_para(|q| {
                    q.align = p.align;
                    q.level = p.level;
                    q.space_before = p.space_before;
                    q.space_after = p.space_after;
                    q.line_spacing = p.line_spacing;
                });
            }
            self.dlg.paragraph = open;
        }

        if self.dlg.about {
            let mut open = true;
            egui::Window::new("About FreeWord").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.heading("FreeWord");
                ui.label("A word processor written in Rust with egui.");
                ui.label("Reads and writes .docx, exports PDF, HTML and plain text.");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            });
            self.dlg.about = open;
        }

        if self.dlg.shortcuts {
            let mut open = true;
            egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).show(ctx, |ui| {
                egui::Grid::new("keys").striped(true).show(ui, |ui| {
                    for (k, v) in [
                        ("Ctrl+N / O / S", "New / Open / Save"),
                        ("Ctrl+Shift+S", "Save As"),
                        ("Ctrl+P", "Print (via PDF)"),
                        ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                        ("Ctrl+X / C / V", "Cut / Copy / Paste"),
                        ("Ctrl+A", "Select all"),
                        ("Ctrl+B / I / U", "Bold / Italic / Underline"),
                        ("Ctrl+Shift+D", "Strikethrough"),
                        ("Ctrl+= / Ctrl+Shift+=", "Subscript / Superscript"),
                        ("Ctrl+[ / Ctrl+]", "Shrink / Grow font"),
                        ("Ctrl+Space", "Clear character formatting"),
                        ("Ctrl+L / E / R / J", "Align left / center / right / justify"),
                        ("Ctrl+1 / 2 / 5", "Line spacing 1 / 2 / 1.5"),
                        ("Ctrl+Alt+1 / 2 / 3", "Heading 1 / 2 / 3"),
                        ("Ctrl+Shift+N", "Normal style"),
                        ("Ctrl+Shift+L", "Bulleted list"),
                        ("Ctrl+M / Ctrl+Shift+M", "Indent / Outdent"),
                        ("Tab / Shift+Tab", "Indent list item"),
                        ("Ctrl+Enter", "Page break"),
                        ("Shift+Enter", "Line break"),
                        ("Ctrl+F / Ctrl+H", "Find / Replace"),
                        ("Ctrl+Mouse wheel", "Zoom"),
                    ] {
                        ui.label(RichText::new(k).strong());
                        ui.label(v);
                        ui.end_row();
                    }
                });
            });
            self.dlg.shortcuts = open;
        }
    }

    fn find_dialog(&mut self, ctx: &egui::Context) {
        if !self.find.open {
            return;
        }
        let mut open = true;
        let title = if self.find.replace { "Find and Replace" } else { "Find" };
        let screen = ctx.content_rect();
        egui::Window::new(title)
            .id(egui::Id::new("find_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(Pos2::new(screen.right() - 380.0, 150.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.find.replace, false, "Find");
                    ui.selectable_value(&mut self.find.replace, true, "Replace");
                });
                ui.separator();
                let mut enter = false;
                egui::Grid::new("find_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Find what:");
                    let r = ui.add(egui::TextEdit::singleline(&mut self.find.query).desired_width(220.0));
                    if self.find.focus_query {
                        r.request_focus();
                        self.find.focus_query = false;
                        self.text_focus_frame = true;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        enter = true;
                    }
                    self.track(&r);
                    ui.end_row();
                    if self.find.replace {
                        ui.label("Replace with:");
                        let r = ui.add(egui::TextEdit::singleline(&mut self.find.replacement).desired_width(220.0));
                        self.track(&r);
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.find.match_case, "Match case").changed() {
                        self.invalidate_find();
                    }
                    if ui.checkbox(&mut self.find.whole_word, "Whole words").changed() {
                        self.invalidate_find();
                    }
                });
                self.refresh_find();
                let n = self.find.matches.len();
                ui.label(if self.find.query.is_empty() {
                    String::new()
                } else if let Some(c) = self.find.current {
                    format!("Result {} of {n}", c + 1)
                } else {
                    format!("{n} result(s)")
                });
                ui.horizontal(|ui| {
                    if ui.button("◀ Previous").clicked() {
                        self.find_next(false);
                    }
                    if ui.button("Next ▶").clicked() || enter {
                        self.find_next(true);
                        if enter {
                            self.find.focus_query = true;
                        }
                    }
                    if self.find.replace {
                        if ui.button("Replace").clicked() {
                            self.replace_one();
                        }
                        if ui.button("Replace All").clicked() {
                            self.replace_all();
                        }
                    }
                });
            });
        if !open {
            self.find.open = false;
            self.find.current = None;
        }
    }
}
