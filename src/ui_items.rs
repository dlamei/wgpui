use glam::Vec2;

use crate::{
    core::RGBA,
    ctext,
    mouse::{CursorIcon, MouseBtn},
    rect::Rect,
    ui::{self, CornerRadii, Id, TextInputState},
};

macro_rules! ui_text {
    ($ui:ident: $($tt:tt)*) => {
        $ui.text(&format!($($tt)*));
    }
}
pub(crate) use ui_text;

impl ui::Context {
    pub fn image(&mut self, size: Vec2, uv_min: Vec2, uv_max: Vec2, tex_id: u32) {
        let id = self.gen_id(tex_id);
        let rect = self.place_item(id, size);
        self.register_item(id);
        self.draw(|list| {
            list.rect(rect.min, rect.max)
                .texture_uv(uv_min, uv_max, tex_id)
                .add()
        })
    }

    pub fn button(&mut self, label: &str) -> bool {
        let id = self.gen_id(label);
        let active = self.style.btn_press();
        let hover = self.style.btn_hover();
        let default = self.style.btn_default();

        let total_h = self.style.line_height();
        let text_shape = self.shape_text(label, self.style.text_size());
        let text_dim = text_shape.size();

        let vert_pad = ((total_h - text_dim.y) / 2.0).max(0.0);
        let horiz_pad = vert_pad;
        let size = Vec2::new(text_dim.x + horiz_pad * 2.0, total_h);

        let rect = self.place_item(id, size);
        let sig = self.register_item(id);

        let start_drag_outside = self
            .mouse
            .drag_start(MouseBtn::Left)
            .map_or(false, |pos| !rect.contains(pos));

        let (btn_col, text_col) = if sig.pressed() && !start_drag_outside {
            (active, self.style.btn_press_text())
        } else if sig.hovering() {
            (hover, self.style.text_col())
        } else {
            (default, self.style.text_col())
        };

        let text_pos =
            rect.min + Vec2::new((size.x - text_dim.x) * 0.5, (size.y - text_dim.y) * 0.5);

        self.draw(|list| {
            list.rect(rect.min, rect.max)
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(btn_col)
                .add();
            list.add_text(text_pos, &text_shape, text_col);
        });

        sig.released() && !start_drag_outside
    }

    pub fn switch(&mut self, label: &str, b: &mut bool) -> bool {
        let height = self.style.line_height();
        let width = height * 1.8;
        let size = Vec2::new(width, self.style.line_height());
        let text_shape = self.shape_text(label, self.style.text_size());
        let text_dim = text_shape.size();

        let id = self.gen_id(label);
        let rect = self.place_item(id, size);
        let sig = self.register_item(id);

        if sig.released() {
            *b = !*b;
        }

        let mut bg_col = if sig.hovering() {
            self.style.btn_hover()
        } else {
            self.style.btn_default()
        };
        let mut knob_col = self.style.btn_press();

        if *b {
            std::mem::swap(&mut bg_col, &mut knob_col);
        }

        self.draw(|list| {
            let rail_min = rect.min;
            let rail_max = rail_min + Vec2::new(width, height);
            list.rect(rect.min, rect.max)
                // .corners(CornerRadii::all(height * 0.5))
                // .corners(CornerRadii::all(height * 0.3))
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(bg_col)
                .add();

            let knob_r = height * 0.8 * 0.5;
            let knob_x = if *b {
                rail_max.x - height * 0.5
            } else {
                rail_min.x + height * 0.5
            };
            let knob_center = Vec2::new(knob_x, rail_min.y + height * 0.5);
            list.circle(knob_center, knob_r)
                // .corners(CornerRadii::all(height * 0.8 * 0.3))
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(knob_col)
                .add();
        });

        self.same_line();
        self.text(label);

        *b
    }

    pub fn checkbox(&mut self, label: &str, b: &mut bool) -> bool {
        let id = self.gen_id(label);
        let active = self.style.btn_press();
        let hover = self.style.btn_hover();
        let default = self.style.btn_default();

        let box_size = self.style.line_height();
        let text_shape = self.shape_text(label, self.style.text_size());

        let rect = self.place_item(id, Vec2::splat(box_size));
        let sig = self.register_item(id);

        if sig.released() {
            *b = !*b;
        }

        let col = if sig.pressed() {
            active
        } else if sig.hovering() {
            hover
        } else {
            default
        };

        let radii = CornerRadii::all(self.style.btn_corner_radius());
        self.draw(|list| {
            let inset = box_size * 0.15;
            let inner_min = rect.min + Vec2::splat(inset);
            let inner_max = rect.max - Vec2::splat(inset);

            list.rect(rect.min, rect.max).fill(col).corners(radii).add();
            if *b {
                list.rect(inner_min, inner_max)
                    .corners(radii)
                    .fill(active)
                    .add();
            }
        });

        self.same_line();
        self.text(label);

        *b
    }

    pub fn separator_h(&mut self, thickness: f32, fill: RGBA) {
        let width = self.available_content().x;
        let rect = self.place_item(Id::NULL, Vec2::new(width, thickness));
        let col = self.style.panel_dark_bg();

        self.draw(|list| list.rect(rect.min, rect.max).fill(fill).add());
    }

    pub fn slider_f32(&mut self, label: &str, min: f32, max: f32, val: &mut f32) {
        let height = self.style.line_height();
        let width = self.available_content().x / 2.5;
        let rect = self.place_item(self.gen_id(label), Vec2::new(width, height));
        let sig = self.register_item(self.gen_id(label));

        let knob_size = height * 0.8;
        let rail_pad = height - knob_size;
        let usable_width = (rect.width() - knob_size - rail_pad).max(0.0);

        if sig.pressed() || sig.dragging() {
            let denom = usable_width.max(1.0);
            let t = ((self.mouse.pos.x - (rect.min.x + knob_size)) / denom).clamp(0.0, 1.0);
            if (max - min).abs() > f32::EPSILON {
                *val = min + t * (max - min);
            }
        }

        let ratio = if (max - min).abs() < f32::EPSILON {
            0.0
        } else {
            ((*val - min) / (max - min)).clamp(0.0, 1.0)
        };

        let mut knob_min = rect.min + Vec2::splat(rail_pad / 2.0);
        knob_min.x += ratio * usable_width;
        let knob_max = knob_min + Vec2::splat(knob_size);

        if sig.hovering() {
            self.set_cursor_icon(CursorIcon::MoveH);
        }
        if sig.pressed() && !sig.dragging() {
            self.expect_drag = true;
        }

        let (mut rail_col, mut knob_col) = if sig.dragging() || sig.pressed() {
            (self.style.btn_press(), self.style.btn_hover())
        } else if sig.hovering() {
            (self.style.btn_hover(), self.style.btn_press())
        } else {
            (self.style.btn_default(), self.style.btn_press())
        };

        self.draw(|list| {
            list.rect(rect.min, rect.max)
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(rail_col)
                .add();

            list.rect(knob_min, knob_max)
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(knob_col)
                .add()
        });

        self.same_line();
        self.text(label);
    }

    pub fn collapsing_header(&mut self, label: &str, open: &mut bool) -> bool {
        let id = self.gen_id(label);
        let active = self.style.btn_press();
        let hover = self.style.btn_hover();
        let default = self.style.btn_default();

        let total_h = self.style.line_height();

        let text_shape = self.shape_text(label, self.style.text_size());
        let text_dim = text_shape.size();

        let icon = if *open {
            ui::PhosphorFont::CARET_DOWN
        } else {
            ui::PhosphorFont::CARET_RIGHT
        };
        let icon_shape = self.shape_icon(icon, self.style.text_size());
        let icon_dim = text_shape.size();

        let vert_pad = ((total_h - text_dim.y) / 2.0).max(0.0);
        let avail = self.available_content();
        let size = Vec2::new(avail.x, total_h);

        let rect = self.place_item(id, size);
        let sig = self.register_item(id);

        let start_drag_outside = self
            .mouse
            .drag_start(MouseBtn::Left)
            .map_or(false, |pos| !rect.contains(pos));

        if sig.released() {
            *open = !*open;
        }

        let (btn_col, text_col) = if *open || sig.pressed() && !start_drag_outside {
            (active, self.style.btn_press_text())
        } else if sig.hovering() {
            (hover, self.style.text_col())
        } else {
            (default, self.style.text_col())
        };

        let icon_pos = rect.min + Vec2::new(vert_pad, (size.y - icon_dim.y) * 0.5);

        let text_pos = icon_pos + Vec2::new(self.style.text_size() * 2.0, 0.0);

        self.draw(|list| {
            list.rect(rect.min, rect.max)
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(btn_col)
                .add();

            list.add_text(icon_pos, &icon_shape, text_col);
            list.add_text(text_pos, &text_shape, text_col);
        });

        *open
    }

    pub fn text(&mut self, text: &str) {
        let text_height = self.style.text_size();
        let line_height = self.style.line_height().max(text_height);

        let pad = (line_height - text_height) / 2.0;
        self.move_down(pad);
        let shape = self.shape_text(text, self.style.text_size());

        let p = self.get_current_panel();
        let id = p.gen_id(text);

        let size = Vec2::new(shape.width, shape.height);
        let rect = self.place_item(id, size);
        // self.register_item(id);
        self.move_down(pad);

        self.draw(|list| list.add_text(rect.min, &shape, self.style.text_col()));
    }

    pub fn text_input(&mut self, text: &str) {
        use ctext::{Action, Edit, Motion};

        let text_height = self.style.text_size();
        let line_height = self.style.line_height().max(text_height);
        let vertical_offset = (line_height - text_height) / 2.0;
        self.move_down(vertical_offset);

        let panel = self.get_current_panel();
        let id = panel.gen_id(text);

        if !self.text_input_states.contains_id(id) {
            let item = ui::TextItem::new(text.to_string(), self.style.text_size(), 1.0, "Inter");
            self.text_input_states.insert(
                id,
                TextInputState::new(&mut self.font_table.get_mut(), item),
            );
        }

        let input = &mut self.text_input_states[id];

        let cursor_pos = input.edit.cursor_position();

        input
            .edit
            .shape_as_needed(&mut self.font_table.get_mut().sys, true);

        let shape = input.shape(
            self.font_table.get_mut(),
            self.glyph_cache.get_mut(),
            &mut self.draw.wgpu,
        );
        let text_dim = shape.size();

        let total_h = line_height;
        let vert_pad = ((total_h - text_dim.y) / 2.0).max(0.0);
        let horiz_pad = vert_pad;
        let size = Vec2::new(text_dim.x + horiz_pad * 2.0, total_h);

        let rect = self.place_item(id, size);
        let sig = self.register_item(id);

        if sig.hovering() || sig.dragging() {
            self.set_cursor_icon(CursorIcon::Text);
        }

        if sig.pressed() {
            let x = (self.mouse.pos.x - rect.min.x) as i32;
            let y = (self.mouse.pos.y - rect.min.y) as i32;
            self.text_input_states[id].edit.action(
                &mut self.font_table.borrow_mut().sys,
                Action::Click { x, y },
            );
        } else if sig.dragging() {
            let x = (self.mouse.pos.x - rect.min.x) as i32;
            let y = (self.mouse.pos.y - rect.min.y) as i32;
            self.text_input_states[id]
                .edit
                .action(&mut self.font_table.borrow_mut().sys, Action::Drag { x, y });
        }

        let outline_col = if self.active_id == id {
            self.style.btn_hover()
        } else {
            self.style.panel_dark_bg()
        };

        let text_pos =
            rect.min + Vec2::new((size.x - text_dim.x) * 0.5, (size.y - text_dim.y) * 0.5);

        self.draw_text_input(id, rect.min);
        // self.draw(|list| {
        //     list.rect(rect.min, rect.max)
        //         .corners(CornerRadii::all(self.style.btn_corner_radius()))
        //         .outline(ui::Outline::new(outline_col, 3.0))
        //         .add();

        //     let input = &self.text_input_states[id];
        //     let selection = input.edit.selection_bounds();
        //     let cursor = input.edit.cursor();
        //     // list.add_text(text_pos, &shape, self.style.text_col());

        //     // if let Some((cursor_x, _cursor_y)) = input.edit.cursor_position() {
        //     //     let cursor_width = 2.0;
        //     //     let cursor_height = text_dim.y.max(1.0);
        //     //     let cursor_top = text_pos.y;
        //     //     let cursor_pos = Vec2::new(text_pos.x + cursor_x as f32, cursor_top);

        //     //     list.rect(
        //     //         cursor_pos,
        //     //         cursor_pos + Vec2::new(cursor_width, cursor_height),
        //     //     )
        //     //     .fill(self.style.btn_press())
        //     //     .add();
        //     // }
        // });
    }

    pub fn draw_text_input(&mut self, id: Id, pos: Vec2) {
        use ctext::Edit;
        use std::cmp;
        use unicode_segmentation::UnicodeSegmentation;

        let text_color = self.style.text_col();
        let cursor_color = self.style.btn_press();
        let selection_color = self.style.btn_press();
        let selected_text_color = self.style.btn_press_text();

        let input = &mut self.text_input_states[id];

        let mut textured_glyphs: Vec<(Vec2, Vec2, Vec2, Vec2, RGBA)> = Vec::new();
        let mut selection_rects: Vec<(i32, i32, u32, u32)> = Vec::new();
        let mut cursor_rects: Vec<(i32, i32, u32, u32)> = Vec::new();

        let sel_bounds = input.edit.selection_bounds();
        let cursor = input.edit.cursor();
        input.edit.with_buffer_mut(|buffer| {
            for run in buffer.layout_runs() {
                let line_i = run.line_i;
                let line_y = run.line_y;
                let line_top = run.line_top;
                let line_height = run.line_height;

                // Selection highlighting (collect rects)
                if let Some((start, end)) = sel_bounds {
                    if line_i >= start.line && line_i <= end.line {
                        let mut range_opt: Option<(i32, i32)> = None;

                        for glyph in run.glyphs.iter() {
                            let cluster = &run.text[glyph.start..glyph.end];
                            let total = cluster.grapheme_indices(true).count();
                            let mut c_x = glyph.x;
                            let c_w = glyph.w / total as f32;

                            for (i, _g) in cluster.grapheme_indices(true) {
                                let c_start = glyph.start + i;
                                let c_end = glyph.start + i + _g.len();
                                if (start.line != line_i || c_end > start.index)
                                    && (end.line != line_i || c_start < end.index)
                                {
                                    range_opt = match range_opt.take() {
                                        Some((min, max)) => Some((
                                            cmp::min(min, c_x as i32),
                                            cmp::max(max, (c_x + c_w) as i32),
                                        )),
                                        None => Some((c_x as i32, (c_x + c_w) as i32)),
                                    };
                                } else if let Some((min, max)) = range_opt.take() {
                                    selection_rects.push((
                                        min,
                                        line_top as i32,
                                        cmp::max(0, max - min) as u32,
                                        line_height as u32,
                                    ));
                                }
                                c_x += c_w;
                            }
                        }

                        if run.glyphs.is_empty() && end.line > line_i {
                            range_opt = Some((0, buffer.size().0.unwrap_or(0.0) as i32));
                        }

                        if let Some((mut min, mut max)) = range_opt.take() {
                            if end.line > line_i {
                                if run.rtl {
                                    min = 0;
                                } else {
                                    max = buffer.size().0.unwrap_or(0.0) as i32;
                                }
                            }
                            selection_rects.push((
                                min,
                                line_top as i32,
                                cmp::max(0, max - min) as u32,
                                line_height as u32,
                            ));
                        }
                    }
                }

                // Cursor
                if let Some((x, y)) = cursor_position(&cursor, &run) {
                    cursor_rects.push((x, y, 1, line_height as u32));
                }

                // Glyphs (collect textured quads + color)
                for glyph in run.glyphs.iter() {
                    let physical_glyph = glyph.physical((0., 0.), 1.0);
                    // let mut glyph_color = match glyph.color_opt {
                    //     Some(c) => c,
                    //     None => text_color,
                    // };
                    let mut glyph_color = text_color;

                    if text_color != selected_text_color {
                        if let Some((start, end)) = sel_bounds {
                            if line_i >= start.line
                                && line_i <= end.line
                                && (start.line != line_i || glyph.end > start.index)
                                && (end.line != line_i || glyph.start < end.index)
                            {
                                glyph_color = selected_text_color;
                            }
                        }
                    }

                    let mut key = physical_glyph.cache_key;
                    key.x_bin = ctext::SubpixelBin::Three;
                    key.y_bin = ctext::SubpixelBin::Three;

                    let mut cache = self.glyph_cache.borrow_mut();
                    let mut fonts = self.font_table.borrow_mut();
                    let wgpu = &self.draw.wgpu;
                    if let Some(mut cached) = cache.get_glyph(key, &mut fonts, wgpu) {
                        let min = cached.meta.pos
                            + Vec2::new(
                                physical_glyph.x as f32,
                                physical_glyph.y as f32 + run.line_y,
                            );
                        let max = min + cached.meta.size;
                        let uv_min = cached.meta.uv_min;
                        let uv_max = cached.meta.uv_max;

                        textured_glyphs.push((min, max, uv_min, uv_max, glyph_color));
                    }
                }
            }
        });

        // Draw: selection -> cursor -> glyphs (matches reference ordering)
        self.draw(|list| {
            for (x, y, w, h) in selection_rects {
                list.rect(
                    Vec2::new(x as f32, y as f32) + pos,
                    Vec2::new((x + w as i32) as f32, (y + h as i32) as f32) + pos,
                )
                .fill(selection_color)
                .add();
            }

            for (x, y, w, h) in cursor_rects {
                list.rect(
                    Vec2::new(x as f32, y as f32) + pos,
                    Vec2::new((x + w as i32) as f32, (y + h as i32) as f32) + pos,
                )
                .fill(cursor_color)
                .add();
            }

            for (min, max, uv_min, uv_max, color) in textured_glyphs {
                list.rect(min + pos, max + pos)
                    .texture_uv(uv_min, uv_max, 1)
                    .fill(color)
                    .add();
            }
        });
    }

    // pub fn draw_text_input(&mut self, id: Id, pos: Vec2) {
    //     use ctext::Edit;

    //     let mut input = &mut self.text_input_states[id];

    //     let mut glyphs = Vec::new();
    //     input.edit.with_buffer_mut(|buf| {
    //         // TODO[CHECK]: when how to call shape_...

    //         for run in buf.layout_runs() {

    //             for g in run.glyphs {
    //                 let g_phys = g.physical((0.0, 0.0), 1.0);
    //                 let mut key = g_phys.cache_key;

    //                 key.x_bin = ctext::SubpixelBin::Three;
    //                 key.y_bin = ctext::SubpixelBin::Three;

    //                 let mut cache = self.glyph_cache.borrow_mut();
    //                 let mut fonts = self.font_table.borrow_mut();
    //                 let wgpu = &self.draw.wgpu;
    //                 if let Some(mut glyph) = cache.get_glyph(key, &mut fonts, wgpu) {
    //                     let min = glyph.meta.pos + Vec2::new(g_phys.x as f32, g_phys.y as f32 + run.line_y) + pos;
    //                     let max = min + glyph.meta.size;
    //                     let uv_min = glyph.meta.uv_min;
    //                     let uv_max = glyph.meta.uv_max;

    //                     glyphs.push((min, max, uv_min, uv_max));

    //                 }
    //             }
    //         }
    //     });

    //     self.draw(|list| {
    //         for (min, max, uv_min, uv_max) in glyphs {
    //             list.rect(min, max)
    //             .texture_uv(uv_min, uv_max, 1)
    //             .add()
    //         }
    //     });

    // }

    // pub fn text_input(&mut self, text: &str) {
    //     use ctext::Edit;
    //     let text_height = self.style.text_size();
    //     let line_height = self.style.line_height().max(text_height);

    //     let pad = (line_height - text_height) / 2.0;
    //     self.move_down(pad);

    //     let p = self.get_current_panel();
    //     let id = p.gen_id(text);

    //     if !self.text_input_states.contains_id(id) {
    //     let itm = ui::TextItem::new(text.to_string(), self.style.text_size(), 1.0, "Inter");
    //         self.text_input_states.insert(id, TextInputState::new(&mut self.font_table.borrow_mut(), itm))
    //     }

    //     let input = self.text_input_states.get_mut(id).unwrap();
    //     input.edit.shape_as_needed(&mut self.font_table.borrow_mut().sys, true);
    //     let cursor_pos = input.edit.cursor_position();
    //     let shape = input.shape(&mut self.font_table.borrow_mut(), &mut self.glyph_cache.borrow_mut(), &mut self.draw.wgpu);
    //     let text_dim = shape.size();
    //     let total_h = self.style.line_height();

    //     let vert_pad = ((total_h - text_dim.y) / 2.0).max(0.0);
    //     let horiz_pad = vert_pad;
    //     let size = Vec2::new(text_dim.x + horiz_pad * 2.0, total_h);
    //     let pos = input.edit.cursor_position();

    //     let rect = self.place_item(id, size);
    //     let sig = self.register_item(id);

    //     let start_drag_outside = self
    //         .mouse
    //         .drag_start(MouseBtn::Left)
    //         .map_or(false, |pos| !rect.contains(pos));

    //     let outline_col = if self.active_id == id {
    //         self.style.btn_hover()
    //     } else {
    //         self.style.panel_dark_bg()
    //     };
    //     // let (btn_col, text_col) = if sig.pressed() && !start_drag_outside {
    //     //     (active, self.style.btn_press_text())
    //     // } else if sig.hovering() {
    //     //     (hover, self.style.text_col())
    //     // } else {
    //     //     (default, self.style.text_col())
    //     // };

    //     let text_pos =
    //         rect.min + Vec2::new((size.x - text_dim.x) * 0.5, (size.y - text_dim.y) * 0.5);

    //     self.draw(|list| {
    //         list.rect(rect.min, rect.max)
    //             .corners(CornerRadii::all(self.style.btn_corner_radius()))
    //             .outline(ui::Outline::new(outline_col, 3.0))
    //             .add();

    //         list.add_text(text_pos, &shape, self.style.text_col());

    //         if let Some((x, y)) = cursor_pos {
    //             let pos = Vec2::new(x as f32, y as f32) + rect.min;
    //             list.rect(pos, pos + Vec2::new(3.0, self.style.line_height()))
    //                 .fill(self.style.btn_press())
    //                 .add();
    //         }

    //     });
    // }

    pub fn begin_tabbar(&mut self, label: &str) {
        // TODO[NOTE] tabbar stack
        let id = self.gen_id(label);
        self.tabbars.map.entry(id).or_insert(ui::TabBar::new());
        self.current_tabbar_id = id;
        self.push_id(id);

        let avail = self.available_content();

        self.push_style(ui::StyleVar::SpacingV(0.0));
        let rect = self.place_item(id, Vec2::new(avail.x, self.style.line_height()));
        self.pop_style();
        self.separator_h(3.0, self.style.btn_hover());

        let cursor = self.get_current_panel()._cursor.clone().into_inner();

        let tb = &mut self.tabbars[id];
        tb.id = id;
        tb.panel_id = self.current_panel_id;
        tb.cursor_backup = cursor;
        tb.bar_rect = rect;

        tb.layout_tabs();
    }

    pub fn end_tabbar(&mut self) {
        let tb = &self.tabbars[self.current_tabbar_id];
        // let cursor = tb.cursor_backup;
        let tb_id = tb.id;
        assert!(self.pop_id() == tb_id);

        self.current_tabbar_id = Id::NULL;
        // self.get_current_panel()._cursor.replace(cursor);
    }

    pub fn tabitem(&mut self, label: &str) -> bool {
        let tb_id = self.current_tabbar_id;
        let tb_rect = self.tabbars[tb_id].bar_rect;
        assert!(!tb_id.is_null());

        let id = self.gen_id(label);
        let tb = &mut self.tabbars[tb_id];
        if tb.tabs.is_empty() {
            tb.selected_tab_id = id;
        }

        let text_shape = self.shape_text(label, self.style.text_size());
        let text_dim = text_shape.size();
        let vert_pad = ((tb_rect.height() - text_dim.y) / 2.0).max(0.0);
        let item_width = vert_pad * 2.0 + text_dim.x;

        let tb = &mut self.tabbars[tb_id];
        let is_selected = tb.selected_tab_id == id;

        let indx = tb.tabs.iter().position(|t| t.id == id);
        let Some(indx) = indx else {
            let mut item = ui::TabItem::default();
            item.id = id;
            item.width = item_width;
            tb.tabs.push(item);
            return is_selected;
        };

        tb.tabs[indx].width = item_width;
        let item = tb.tabs[indx];

        let tab_size = Vec2::new(item.width, tb_rect.height());
        let rect = Rect::from_min_size(tb_rect.min + Vec2::new(item.offset, 0.0), tab_size);
        let sig = self.register_rect(id, rect);

        let (btn_col, text_col) = if is_selected {
            (self.style.btn_hover(), self.style.text_col())
        } else if sig.hovering() {
            (self.style.btn_default(), self.style.text_col())
        } else {
            (self.style.panel_bg(), self.style.text_col())
        };

        let tb = &mut self.tabbars[tb_id];

        if sig.pressed() {
            tb.selected_tab_id = id;
        }
        if sig.dragging() && self.active_id == id && !tb.is_dragging {
            tb.is_dragging = true;
            tb.selected_tab_id = id;
            tb.dragging_offset = rect.min.x - self.mouse.pos.x;
        }

        if is_selected && !self.mouse.pressed(MouseBtn::Left) && tb.is_dragging {
            tb.is_dragging = false;
        }

        let mut item_pos = rect.min;

        if tb.is_dragging && tb.selected_tab_id == id {
            item_pos.x = tb.dragging_offset + self.mouse.pos.x;
        }

        if is_selected {
            let new_indx = tb.get_insert_pos(item_pos.x, rect.width(), indx);
            tb.move_tab(indx, new_indx);
        }

        item_pos.x = item_pos
            .x
            .max(tb_rect.min.x)
            .min(tb_rect.max.x - rect.width());

        let text_pos = item_pos
            + Vec2::new(
                (item.width - text_dim.x) * 0.5,
                (tb_rect.height() - text_dim.y) * 0.5,
            );

        if tb.is_dragging && tb.selected_tab_id == id {
            self.draw_over(|list| {
                list.rect(item_pos, item_pos + rect.size())
                    .fill(btn_col)
                    .corners(CornerRadii::top(self.style.btn_corner_radius()))
                    .add();

                list.add_text(text_pos, &text_shape, text_col);
            });
        } else {
            self.draw(|list| {
                list.rect(item_pos, item_pos + rect.size())
                    .fill(btn_col)
                    .corners(CornerRadii::top(self.style.btn_corner_radius() * 1.5))
                    .add();

                list.add_text(text_pos, &text_shape, text_col);
            });
        }

        is_selected
    }
}

// BEGIN INTERN
//---------------------------------------------------------------------------------------

impl ui::Context {
    pub fn checkbox_intern(&mut self, label: &str) -> bool {
        let id = self.gen_id(label);
        let mut toggle = *self.widget_data.get_or_insert(id, false);
        self.checkbox(label, &mut toggle);
        self.widget_data.insert(id, toggle);
        toggle
    }

    pub fn switch_intern(&mut self, label: &str) -> bool {
        let id = self.gen_id(label);
        let mut toggle = *self.widget_data.get_or_insert(id, false);
        self.switch(label, &mut toggle);
        self.widget_data.insert(id, toggle);
        toggle
    }

    pub fn slider_f32_intern(&mut self, label: &str, min: f32, max: f32) -> f32 {
        let id = self.gen_id(label);
        let mut val = *self.widget_data.get_or_insert(id, (min + max) / 2.0);
        self.slider_f32(label, min, max, &mut val);
        self.widget_data.insert(id, val);
        val
    }

    pub fn collapsing_header_intern(&mut self, label: &str) -> bool {
        let id = self.gen_id(label);
        let mut b = *self.widget_data.get_or_insert(id, false);
        self.collapsing_header(label, &mut b);
        self.widget_data.insert(id, b);
        b
    }
}

fn cursor_glyph_opt(cursor: &ctext::Cursor, run: &ctext::LayoutRun) -> Option<(usize, f32)> {
    use unicode_segmentation::UnicodeSegmentation;
    if cursor.line == run.line_i {
        for (glyph_i, glyph) in run.glyphs.iter().enumerate() {
            if cursor.index == glyph.start {
                return Some((glyph_i, 0.0));
            } else if cursor.index > glyph.start && cursor.index < glyph.end {
                // Guess x offset based on characters
                let mut before = 0;
                let mut total = 0;

                let cluster = &run.text[glyph.start..glyph.end];
                for (i, _) in cluster.grapheme_indices(true) {
                    if glyph.start + i < cursor.index {
                        before += 1;
                    }
                    total += 1;
                }

                let offset = glyph.w * (before as f32) / (total as f32);
                return Some((glyph_i, offset));
            }
        }
        match run.glyphs.last() {
            Some(glyph) => {
                if cursor.index == glyph.end {
                    return Some((run.glyphs.len(), 0.0));
                }
            }
            None => {
                return Some((0, 0.0));
            }
        }
    }
    None
}

fn cursor_position(cursor: &ctext::Cursor, run: &ctext::LayoutRun) -> Option<(i32, i32)> {
    let (cursor_glyph, cursor_glyph_offset) = cursor_glyph_opt(cursor, run)?;
    let x = match run.glyphs.get(cursor_glyph) {
        Some(glyph) => {
            // Start of detected glyph
            if glyph.level.is_rtl() {
                (glyph.x + glyph.w - cursor_glyph_offset) as i32
            } else {
                (glyph.x + cursor_glyph_offset) as i32
            }
        }
        None => match run.glyphs.last() {
            Some(glyph) => {
                // End of last glyph
                if glyph.level.is_rtl() {
                    glyph.x as i32
                } else {
                    (glyph.x + glyph.w) as i32
                }
            }
            None => {
                // Start of empty line
                0
            }
        },
    };

    Some((x, run.line_top as i32))
}
