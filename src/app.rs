//! Open Studio Sheets — the spreadsheet UI (egui / eframe).

use crate::engine::{col_name, Pos, Sheet, Value};
use eframe::egui::{self, pos2, Align2, FontId, Key, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::HashMap;

const COLS: usize = 26;
const ROWS: usize = 500;
const CELL_W: f32 = 96.0;
const CELL_H: f32 = 24.0;
const HDR_W: f32 = 48.0;
const HDR_H: f32 = 24.0;
const STORAGE_KEY: &str = "sheet_csv";

pub struct SheetApp {
    sheet: Sheet,
    values: HashMap<Pos, Value>,
    dirty: bool,
    sel: Pos,
    editing: Option<String>,
    focus_requested: bool,
    skip_keys: bool,
    scroll_to_sel: bool,
    show_csv: bool,
    csv_text: String,
}

fn sample() -> Sheet {
    let rows = [
        ["Item", "Qty", "Price", "Total"],
        ["Notebook", "3", "4.5", "=B2*C2"],
        ["Pen", "12", "1.25", "=B3*C3"],
        ["Bag", "1", "29.99", "=B4*C4"],
        ["", "", "Grand total", "=SUM(D2:D4)"],
    ];
    let mut sheet = Sheet::default();
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            sheet.set((c, r), text);
        }
    }
    sheet
}

fn cell_rect(origin: Pos2, c: usize, r: usize) -> Rect {
    Rect::from_min_size(
        pos2(
            origin.x + HDR_W + c as f32 * CELL_W,
            origin.y + HDR_H + r as f32 * CELL_H,
        ),
        Vec2::new(CELL_W, CELL_H),
    )
}

impl SheetApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let saved = cc
            .storage
            .and_then(|s| eframe::get_value::<String>(s, STORAGE_KEY));
        let sheet = match saved {
            Some(csv) => Sheet::from_csv(&csv, COLS, ROWS),
            None => sample(),
        };
        SheetApp {
            sheet,
            values: HashMap::new(),
            dirty: true,
            sel: (0, 0),
            editing: None,
            focus_requested: false,
            skip_keys: false,
            scroll_to_sel: false,
            show_csv: false,
            csv_text: String::new(),
        }
    }

    fn refresh(&mut self) {
        if self.dirty {
            self.values = self.sheet.evaluate_all();
            self.dirty = false;
        }
    }

    fn sel_name(&self) -> String {
        format!("{}{}", col_name(self.sel.0), self.sel.1 + 1)
    }

    fn move_sel(&mut self, dx: i32, dy: i32) {
        let c = (self.sel.0 as i32 + dx).clamp(0, COLS as i32 - 1) as usize;
        let r = (self.sel.1 as i32 + dy).clamp(0, ROWS as i32 - 1) as usize;
        self.sel = (c, r);
        self.scroll_to_sel = true;
    }

    fn start_edit(&mut self, initial: Option<String>) {
        let text = match initial {
            Some(t) => t,
            None => self.sheet.get(self.sel).unwrap_or("").to_string(),
        };
        self.editing = Some(text);
        self.focus_requested = false;
    }

    fn commit_edit(&mut self) {
        if let Some(buf) = self.editing.take() {
            self.sheet.set(self.sel, &buf);
            self.dirty = true;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Open Studio Office");
            ui.label("· Sheets");
            ui.separator();
            if ui.button("New").clicked() {
                self.sheet = Sheet::default();
                self.editing = None;
                self.sel = (0, 0);
                self.dirty = true;
            }
            if ui.button("Sample").clicked() {
                self.sheet = sample();
                self.editing = None;
                self.sel = (0, 0);
                self.dirty = true;
            }
            if ui.button("CSV import / export").clicked() {
                self.csv_text = self.sheet.to_csv();
                self.show_csv = true;
            }
        });
    }

    fn formula_bar(&mut self, ui: &mut egui::Ui) {
        let name = self.sel_name();
        ui.horizontal(|ui| {
            ui.add_sized([56.0, 22.0], egui::Label::new(egui::RichText::new(name).strong()));
            ui.label("fx");
            if let Some(buf) = &self.editing {
                let mut shown = buf.clone();
                ui.add_enabled(
                    false,
                    egui::TextEdit::singleline(&mut shown).desired_width(f32::INFINITY),
                );
            } else {
                let mut text = self.sheet.get(self.sel).unwrap_or("").to_string();
                let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY));
                if resp.changed() {
                    self.sheet.set(self.sel, &text);
                    self.dirty = true;
                }
            }
        });
    }

    fn grid(&mut self, ui: &mut egui::Ui) {
        self.refresh();

        let visuals = ui.visuals().clone();
        let grid_col = visuals.widgets.noninteractive.bg_stroke.color;
        let text_col = visuals.text_color();
        let hdr_fill = visuals.widgets.inactive.bg_fill;
        let sel_fill = visuals.selection.bg_fill;
        let sel_col = visuals.selection.stroke.color;
        let cell_fill = visuals.extreme_bg_color;
        let err_col = visuals.error_fg_color;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let total = Vec2::new(
                    HDR_W + COLS as f32 * CELL_W,
                    HDR_H + ROWS as f32 * CELL_H,
                );
                let (rect, resp) = ui.allocate_exact_size(total, Sense::click());
                let origin = rect.min;
                let painter = ui.painter().clone();
                let clip = painter.clip_rect();

                // ── Handle clicks ──
                if resp.clicked() || resp.double_clicked() {
                    if let Some(pp) = resp.interact_pointer_pos() {
                        let rel = pp - origin;
                        if rel.x > viewport.min.x + HDR_W && rel.y > viewport.min.y + HDR_H {
                            let c = ((rel.x - HDR_W) / CELL_W).floor() as isize;
                            let r = ((rel.y - HDR_H) / CELL_H).floor() as isize;
                            if c >= 0 && r >= 0 && (c as usize) < COLS && (r as usize) < ROWS {
                                self.commit_edit();
                                self.sel = (c as usize, r as usize);
                                if resp.double_clicked() {
                                    self.start_edit(None);
                                }
                            }
                        }
                    }
                }
                let sel = self.sel;

                // ── Visible range ──
                let c0 = ((viewport.min.x - HDR_W) / CELL_W).floor().max(0.0) as usize;
                let c1 = ((((viewport.max.x - HDR_W) / CELL_W).ceil().max(0.0)) as usize + 1).min(COLS);
                let r0 = ((viewport.min.y - HDR_H) / CELL_H).floor().max(0.0) as usize;
                let r1 = ((((viewport.max.y - HDR_H) / CELL_H).ceil().max(0.0)) as usize + 1).min(ROWS);

                // ── Cells ──
                painter.rect_filled(
                    Rect::from_min_size(origin + viewport.min.to_vec2(), viewport.size()),
                    0.0,
                    cell_fill,
                );
                for r in r0..r1 {
                    for c in c0..c1 {
                        let cr = cell_rect(origin, c, r);
                        painter.rect_stroke(cr, 0.0, Stroke::new(1.0, grid_col));
                        if let Some(v) = self.values.get(&(c, r)) {
                            if matches!(v, Value::Empty) {
                                continue;
                            }
                            let (anchor, pos, color) = match v {
                                Value::Num(_) => (
                                    Align2::RIGHT_CENTER,
                                    pos2(cr.right() - 5.0, cr.center().y),
                                    text_col,
                                ),
                                Value::Bool(_) => (Align2::CENTER_CENTER, cr.center(), text_col),
                                Value::Err(_) => (Align2::CENTER_CENTER, cr.center(), err_col),
                                _ => (
                                    Align2::LEFT_CENTER,
                                    pos2(cr.left() + 5.0, cr.center().y),
                                    text_col,
                                ),
                            };
                            let p = painter.with_clip_rect(cr.shrink(1.0).intersect(clip));
                            p.text(pos, anchor, v.to_string(), FontId::proportional(14.0), color);
                        }
                    }
                }

                // ── Selection outline ──
                let sel_rect = cell_rect(origin, sel.0, sel.1);
                painter.rect_stroke(sel_rect, 0.0, Stroke::new(2.0, sel_col));
                if self.scroll_to_sel {
                    ui.scroll_to_rect(sel_rect, None);
                    self.scroll_to_sel = false;
                }

                // ── Frozen headers (drawn over the cells) ──
                let hx = origin.x + viewport.min.x;
                let hy = origin.y + viewport.min.y;
                for c in c0..c1 {
                    let x = origin.x + HDR_W + c as f32 * CELL_W;
                    let hr = Rect::from_min_size(pos2(x, hy), Vec2::new(CELL_W, HDR_H));
                    painter.rect_filled(hr, 0.0, if c == sel.0 { sel_fill } else { hdr_fill });
                    painter.rect_stroke(hr, 0.0, Stroke::new(1.0, grid_col));
                    painter.text(
                        hr.center(),
                        Align2::CENTER_CENTER,
                        col_name(c),
                        FontId::proportional(13.0),
                        text_col,
                    );
                }
                for r in r0..r1 {
                    let y = origin.y + HDR_H + r as f32 * CELL_H;
                    let hr = Rect::from_min_size(pos2(hx, y), Vec2::new(HDR_W, CELL_H));
                    painter.rect_filled(hr, 0.0, if r == sel.1 { sel_fill } else { hdr_fill });
                    painter.rect_stroke(hr, 0.0, Stroke::new(1.0, grid_col));
                    painter.text(
                        hr.center(),
                        Align2::CENTER_CENTER,
                        (r + 1).to_string(),
                        FontId::proportional(13.0),
                        text_col,
                    );
                }
                let corner = Rect::from_min_size(pos2(hx, hy), Vec2::new(HDR_W, HDR_H));
                painter.rect_filled(corner, 0.0, hdr_fill);
                painter.rect_stroke(corner, 0.0, Stroke::new(1.0, grid_col));

                // ── In-cell editor ──
                if let Some(mut buf) = self.editing.take() {
                    let er = Rect::from_min_size(sel_rect.min, Vec2::new(CELL_W * 2.5, CELL_H));
                    let te = egui::TextEdit::singleline(&mut buf);
                    let eresp = ui.put(er, te);
                    if !self.focus_requested {
                        eresp.request_focus();
                        self.focus_requested = true;
                    }
                    let (esc, enter, tab) = ui.input(|i| {
                        (
                            i.key_pressed(Key::Escape),
                            i.key_pressed(Key::Enter),
                            i.key_pressed(Key::Tab),
                        )
                    });
                    if esc {
                        self.skip_keys = true; // cancel: drop the buffer
                    } else if enter || tab || eresp.lost_focus() {
                        self.sheet.set(self.sel, &buf);
                        self.dirty = true;
                        self.skip_keys = true;
                        if enter {
                            self.move_sel(0, 1);
                        }
                        if tab {
                            self.move_sel(1, 0);
                            ui.memory_mut(|m| {
                                if let Some(id) = m.focused() {
                                    m.surrender_focus(id);
                                }
                            });
                        }
                    } else {
                        self.editing = Some(buf);
                    }
                }
            });
    }

    fn csv_window(&mut self, ctx: &egui::Context) {
        if !self.show_csv {
            return;
        }
        let mut open = true;
        let mut text = std::mem::take(&mut self.csv_text);
        let mut import = false;
        let mut refresh = false;
        egui::Window::new("CSV import / export")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("Copy this to export, or paste CSV here and press Import. Formulas are kept.");
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(12),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("Import (replace sheet)").clicked() {
                        import = true;
                    }
                    if ui.button("Refresh from sheet").clicked() {
                        refresh = true;
                    }
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                });
            });
        if import {
            self.sheet = Sheet::from_csv(&text, COLS, ROWS);
            self.sel = (0, 0);
            self.dirty = true;
        }
        if refresh {
            text = self.sheet.to_csv();
        }
        self.csv_text = text;
        self.show_csv = open;
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.skip_keys) {
            return;
        }
        if self.editing.is_some() || ctx.wants_keyboard_input() {
            return;
        }
        let (mut dx, mut dy) = (0i32, 0i32);
        let mut clear = false;
        let mut edit = false;
        let mut tab = false;
        let mut typed = String::new();
        ctx.input(|i| {
            let shift = i.modifiers.shift;
            if i.key_pressed(Key::ArrowDown) {
                dy = 1;
            }
            if i.key_pressed(Key::ArrowUp) {
                dy = -1;
            }
            if i.key_pressed(Key::ArrowLeft) {
                dx = -1;
            }
            if i.key_pressed(Key::ArrowRight) {
                dx = 1;
            }
            if i.key_pressed(Key::Enter) {
                dy = if shift { -1 } else { 1 };
            }
            if i.key_pressed(Key::Tab) {
                dx = if shift { -1 } else { 1 };
                tab = true;
            }
            if i.key_pressed(Key::F2) {
                edit = true;
            }
            if i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace) {
                clear = true;
            }
            if !i.modifiers.command {
                for ev in &i.events {
                    if let egui::Event::Text(t) = ev {
                        typed.push_str(t);
                    }
                }
            }
        });
        typed.retain(|c| !c.is_control());

        if dx != 0 || dy != 0 {
            self.move_sel(dx, dy);
        }
        if tab {
            // Tab also moves egui's widget focus; drop it so Enter can't hit a button.
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        if clear {
            self.sheet.set(self.sel, "");
            self.dirty = true;
        }
        if edit {
            self.start_edit(None);
        } else if !typed.is_empty() {
            self.start_edit(Some(typed));
        }
    }
}

impl eframe::App for SheetApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh();

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.toolbar(ui);
        });
        egui::TopBottomPanel::top("formula_bar").show(ctx, |ui| {
            self.formula_bar(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.grid(ui);
        });

        self.csv_window(ctx);
        self.keyboard(ctx);

        if self.dirty {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.sheet.to_csv());
    }
}
