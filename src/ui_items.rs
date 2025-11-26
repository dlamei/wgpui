use glam::Vec2;

use crate::{
    core::RGBA, ctext, gpu, mouse::{CursorIcon, MouseBtn}, rect::Rect, ui::{self, CornerRadii, Id, ItemFlags, Signal, TabBar, TextInputFlags, TextInputState, TextureId}
};

macro_rules! ui_text {
    ($ui:ident: $($tt:tt)*) => {
        $ui.text(&format!($($tt)*));
    }
}
pub(crate) use ui_text;

impl ui::Context {

    pub fn image(&mut self, size: Vec2, uv_min: Vec2, uv_max: Vec2, tex: &gpu::Texture) {
        let tex_id = self.register_texture(tex);
        self.image_id(size, uv_min, uv_max, tex_id);
    }

    pub fn image_id(&mut self, size: Vec2, uv_min: Vec2, uv_max: Vec2, tex_id: TextureId) {
        // let id = self.gen_id(tex_id);
        let id = Id::NULL;
        let rect = self.place_item(size);
        self.reg_item_(id, rect);
        self.draw(rect.draw_rect().uv(uv_min, uv_max).texture(tex_id));
        // self.draw(|list| {
        //     list.rect(rect.min, rect.max)
        //         .texture_uv(uv_min, uv_max, tex_id)
        //         .add()
        // })
    }

    pub fn button(&mut self, label: &str) -> bool {
        let id = self.gen_id(label);
        let active = self.style.btn_press();
        let hover = self.style.btn_hover();
        let default = self.style.btn_default();

        let total_h = self.style.line_height();
        let text_shape = self.layout_text(label, self.style.text_size());
        let text_dim = text_shape.size();

        let vert_pad = ((total_h - text_dim.y) / 2.0).max(0.0);
        let horiz_pad = vert_pad;
        let size = Vec2::new(text_dim.x + horiz_pad * 2.0, total_h);

        let rect = self.place_item(size);
        let sig = self.reg_item_active_on_press(id, rect);

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

        self.draw(
            rect.draw_rect()
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(btn_col),
        )
        .draw(text_shape.draw_rects(text_pos, text_col));
        // self.draw(|list| {
        //     list.rect(rect.min, rect.max)
        //         .corners(CornerRadii::all(self.style.btn_corner_radius()))
        //         .fill(btn_col)
        //         .add();
        //     list.add_text(text_pos, &text_shape, text_col);
        // });

        sig.released() && !start_drag_outside
    }

    pub fn switch(&mut self, label: &str, b: &mut bool) -> bool {
        let height = self.style.line_height();
        let width = height * 1.8;
        let size = Vec2::new(width, self.style.line_height());
        let text_shape = self.layout_text(label, self.style.text_size());
        let text_dim = text_shape.size();

        let id = self.gen_id(label);
        let rect = self.place_item(size);
        let sig = self.reg_item_active_on_press(id, rect);

        if sig.released() {
            *b = !*b;
        }

        let mut bg_col = if sig.hovering() {
            self.style.btn_hover()
        } else {
            self.style.btn_default()
        };
        let mut handle_col = self.style.btn_press();

        if *b {
            std::mem::swap(&mut bg_col, &mut handle_col);
        }

        // self.draw(|list|
        {
            let rail_min = rect.min;
            let rail_max = rail_min + Vec2::new(width, height);
            self.draw(
                rect.draw_rect()
                    // .corners(CornerRadii::all(height * 0.5))
                    // .corners(CornerRadii::all(height * 0.3))
                    .corners(CornerRadii::all(self.style.btn_corner_radius()))
                    .fill(bg_col),
            );

            let handle_r = height * 0.8 * 0.5;
            let handle_x = if *b {
                rail_max.x - height * 0.5
            } else {
                rail_min.x + height * 0.5
            };
            let handle_center = Vec2::new(handle_x, rail_min.y + height * 0.5);

            self.draw(
                Rect::from_center_size(handle_center, Vec2::splat(handle_r) * 2.0)
                    .draw_rect()
                    // .circle(handle_center, handle_r)
                    // .corners(CornerRadii::all(height * 0.8 * 0.3))
                    .corners(CornerRadii::all(self.style.btn_corner_radius()))
                    .fill(handle_col),
            );
            // .add();
        }
        // );

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
        let text_shape = self.layout_text(label, self.style.text_size());

        let rect = self.place_item(Vec2::splat(box_size));
        let sig = self.reg_item_active_on_press(id, rect);

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
        // self.draw(|list| {
        let inset = box_size * 0.15;
        let inner_min = rect.min + Vec2::splat(inset);
        let inner_max = rect.max - Vec2::splat(inset);

        self.draw(rect.draw_rect().fill(col).corners(radii));
        if *b {
            self.draw(
                Rect::from_min_max(inner_min, inner_max)
                    .draw_rect()
                    .corners(radii)
                    .fill(active),
            );
        }
        // });

        self.same_line();
        self.text(label);

        *b
    }

    pub fn separator_h(&mut self, thickness: f32, fill: RGBA) {
        let width = self.available_content().x;
        let rect = self.place_item(Vec2::new(width, thickness));
        let col = self.style.panel_dark_bg();

        // self.draw(|list| list.rect(rect.min, rect.max).fill(fill).add());
        self.draw(rect.draw_rect().fill(fill));
    }

    pub fn slider_f32(&mut self, label: &str, min: f32, max: f32, val: &mut f32) {
        let id = self.gen_id(label);
        let height = self.style.line_height();
        let width = self.available_content().x / 2.5;
        let rect = self.place_item(Vec2::new(width, height));
        let sig = self.reg_item_active_on_press(id, rect);

        let handle_size = height * 0.8;
        let rail_pad = height - handle_size;
        let usable_width = (rect.width() - handle_size - rail_pad).max(0.0);

        if sig.pressed() || sig.dragging() {
            // Map mouse.x to the handle CENTER (not the left edge).
            // leftmost: minimal handle_min.x
            let leftmost = rect.min.x + rail_pad * 0.5;
            let denom = usable_width.max(1.0);
            let t = ((self.mouse.pos.x - (leftmost + handle_size * 0.5)) / denom).clamp(0.0, 1.0);
            if (max - min).abs() > f32::EPSILON {
                *val = min + t * (max - min);
            }
        }

        let ratio = if (max - min).abs() < f32::EPSILON {
            0.0
        } else {
            ((*val - min) / (max - min)).clamp(0.0, 1.0)
        };

        let mut handle_min = rect.min + Vec2::splat(rail_pad / 2.0);
        handle_min.x += ratio * usable_width;
        let handle_max = handle_min + Vec2::splat(handle_size);

        if sig.hovering() || sig.dragging() {
            self.set_cursor_icon(CursorIcon::MoveH);
        }
        if sig.pressed() && !sig.dragging() {
            self.expect_drag = true;
        }

        let (mut rail_col, mut handle_col) = if sig.dragging() || sig.pressed() {
            (self.style.btn_press(), self.style.btn_hover())
        } else if sig.hovering() {
            (self.style.btn_hover(), self.style.btn_press())
        } else {
            (self.style.btn_default(), self.style.btn_press())
        };

        // self.draw(|list| {
        self.draw(
            rect.draw_rect()
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(rail_col),
        )
        .draw(
            Rect::from_min_max(handle_min, handle_max)
                .draw_rect()
                .corners(self.style.btn_corner_radius())
                .fill(handle_col),
        );

        // list.rect(handle_min, handle_max)
        //     .corners(CornerRadii::all(self.style.btn_corner_radius()))
        //     .fill(handle_col)
        //     .add()
        // });

        self.same_line();
        self.text(label);
    }

    /// Slider that shows the current value centered. Click to edit the value as text,
    /// drag to change it continuously.
    pub fn input_slider_f32(&mut self, label: &str, min: f32, max: f32, val: &mut f32) {
        // AI SLOP
        use ctext::Edit;

        let height = self.style.line_height();
        let width = self.available_content().x / 2.5;
        let id = self.gen_id(label);
        let rect = self.place_item(Vec2::new(width, height));

        // If there's an active text editor for this item we are in edit mode
        let mut is_editing = self.widget_data.contains_key::<TextInputState>(&id);

        let sig = self.reg_item_active_on_press(id, rect);

        if (sig.clicked() || sig.keyboard_focused()) && !is_editing {
            let s = format!("{}", *val);
            let item = ui::TextItem::new(s, self.style.text_size(), 1.0, "Inter");
            self.active_id = id;
            self.widget_data.insert(id, TextInputState::new(id, self.font_table.clone(), item, false));
            self.widget_data.get_mut::<TextInputState>(&id).unwrap().select_all();
            is_editing = true;
        }

        let handle_size = height * 0.8;
        let rail_pad = height - handle_size;
        let usable_width = (rect.width() - handle_size - rail_pad).max(1.0);


        // Cursor hints when hovering/dragging
        if !is_editing && (sig.hovering() || sig.dragging()) {
            self.set_cursor_icon(CursorIcon::MoveH);
        } else if is_editing && sig.hovering() {
            self.set_cursor_icon(CursorIcon::Text);
        }

        // Dragging adjusts the value when not editing
        if sig.dragging() && !is_editing {
            let leftmost_center = rect.min.x + rail_pad * 0.5 + handle_size * 0.5;
            let t = ((self.mouse.pos.x - leftmost_center) / usable_width).clamp(0.0, 1.0);
            if (max - min).abs() > f32::EPSILON {
                *val = min + t * (max - min);
            }
        }

        // Draw only the rail background here; the numeric/text editor is drawn below
        let rail_col = if sig.dragging() || sig.pressed() {
            self.style.panel_dark_bg()
        } else if sig.hovering() {
            self.style.btn_hover()
        } else {
            self.style.btn_default()
        };
        self.draw(
            rect.draw_rect()
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(rail_col),
        );

        self.current_drawlist().push_merged_clip_rect(rect);

        // Editing: show text editor centered in the rail
        if is_editing {
            // let sig2 = self.reg_item(id, rect);

            let input = &mut self.widget_data.get_mut::<TextInputState>(&id).unwrap();
            input.edit.shape_as_needed(&mut self.font_table.sys(), true);
            let layout = input.layout_text(self.glyph_cache.get_mut(), &mut self.wgpu);
            let dim = layout.size();
            // Left-align the editor inside the rail with a small left padding
            let left_padding = rail_pad * 0.5 + 4.0; // extra 4px for breathing room
            let edit_pos = rect.min + Vec2::new(left_padding, (rect.height() - dim.y) * 0.5);

            // Forward mouse events relative to the editor origin
            let rel = self.mouse.pos - edit_pos;
            if sig.double_pressed() {
                input.mouse_double_clicked(rel);
            } else if sig.dragging() {
                input.mouse_dragging(rel);
            } else if sig.pressed() {
                input.mouse_pressed(rel);
            }

            // Live-validate input text
            let cur_text = input.copy_all();
            if let Ok(v) = cur_text.trim().parse::<f32>() {
                *val = v.clamp(min, max);
            }

            // Draw editor background (was previously drawn inside draw_text_input)
            let bg = self.style.panel_dark_bg();
            self.draw(
                rect.draw_rect()
                    .fill(bg)
                    .corners(self.style.btn_corner_radius()),
            );
            self.draw_text_input(id, edit_pos, rect);

            // Commit on focus loss
            if self.active_id != id {
                let new_text = self.widget_data.get::<TextInputState>(&id).unwrap().copy_all();
                if let Ok(v) = new_text.trim().parse::<f32>() {
                    *val = v.clamp(min, max);
                }
                self.widget_data.remove::<TextInputState>(&id);
            }
        } else {
            // Display centered numeric value when not editing
            // Format with up to 3 decimal places, trimming unnecessary trailing zeros
            let val_txt = {
                let v = *val;
                if !v.is_finite() {
                    format!("{}", v)
                } else {
                    let formatted = format!("{:.3}", v);
                    if formatted.contains('.') {
                        formatted.trim_end_matches('0').trim_end_matches('.').to_string()
                    } else {
                        formatted
                    }
                }
            };
            let txt = self.layout_text(&val_txt, self.style.text_size());
            let txt_sz = txt.size();
            let txt_pos = rect.min + Vec2::new((rect.width() - txt_sz.x) * 0.5, (rect.height() - txt_sz.y) * 0.5);
            self.draw(txt.draw_rects(txt_pos, self.style.text_col()));

            // Click to open editor (ignore if drag started outside)
            // // let start_drag_outside = self.mouse.drag_start(MouseBtn::Left).map_or(false, |p| !rect.contains(p));
            // if sig.clicked() {
            //     self.active_id = id;
            //     self.active_id_changed = true;
            // }

        }

        self.current_drawlist().pop_clip_rect();

        self.same_line();
        self.text(label);
    }

    pub fn collapsing_header(&mut self, label: &str, open: &mut bool) -> bool {
        let id = self.gen_id(label);
        let active = self.style.btn_press();
        let hover = self.style.btn_hover();
        let default = self.style.btn_default();

        let total_h = self.style.line_height();

        let text_shape = self.layout_text(label, self.style.text_size());
        let text_dim = text_shape.size();

        let icon = if *open {
            ui::phosphor_font::CARET_DOWN
        } else {
            ui::phosphor_font::CARET_RIGHT
        };
        let icon_shape = self.layout_icon(icon, self.style.text_size());
        let icon_dim = text_shape.size();

        let vert_pad = ((total_h - text_dim.y) / 2.0).max(0.0);
        let avail = self.available_content();
        let size = Vec2::new(avail.x, total_h);

        let rect = self.place_item(size);
        let sig = self.reg_item_active_on_press(id, rect);

        let start_drag_outside = self
            .mouse
            .drag_start(MouseBtn::Left)
            .map_or(false, |pos| !rect.contains(pos));

        if sig.just_pressed() {
            *open = !*open;
        }

        let (btn_col, text_col) = if sig.hovering() {
            (hover, self.style.text_col())
        } else {
            (default, self.style.text_col())
        };

        let icon_pos = rect.min + Vec2::new(vert_pad, (size.y - icon_dim.y) * 0.5);

        let text_pos = icon_pos + Vec2::new(self.style.text_size() * 2.0, 0.0);

        self.draw(
            rect.draw_rect()
                .corners(CornerRadii::all(self.style.btn_corner_radius()))
                .fill(btn_col),
        )
        .draw(icon_shape.draw_rects(icon_pos, text_col))
        .draw(text_shape.draw_rects(text_pos, text_col));

        *open
    }

    pub fn text(&mut self, text: &str) {
        let text_height = self.style.text_size();
        let line_height = self.style.line_height().max(text_height);

        let pad = (line_height - text_height) / 2.0;
        self.move_down(pad);
        let layout = self.layout_text(text, self.style.text_size());

        let id = self.gen_id(text);

        let size = Vec2::new(layout.width, layout.height.max(self.style.line_height()));
        let rect = self.place_item(size);
        // self.register_item(id);
        self.move_down(pad);

        self.draw(layout.draw_rects(rect.min, self.style.text_col()));
        // self.draw(|list| list.add_text(rect.min, &layout, self.style.text_col()));
    }

    pub fn input_text(&mut self, label: &str, default_text: &str) {
        self.input_text_ex(label, default_text, TextInputFlags::NONE);
    }

    pub fn input_text_ex(&mut self, label: &str, default_text: &str, flags: TextInputFlags) {
        use ctext::Edit;

        let text_height = self.style.text_size();
        let line_height = self.style.line_height().max(text_height);
        let vertical_offset = (line_height - text_height) / 2.0;
        self.move_down(vertical_offset);

        let id = self.gen_id(label);

        if !self.widget_data.contains_key::<TextInputState>(&id) {
            let item = ui::TextItem::new(default_text.to_string(), self.style.text_size(), 1.0, "Inter");
            self.widget_data.insert(
                id,
                TextInputState::new(id, self.font_table.clone(), item, false),
            );
        }

        let input = &mut self.widget_data.get_mut::<TextInputState>(&id).unwrap();
        input.multiline = flags.has(TextInputFlags::MULTILINE);

        input.edit.shape_as_needed(&mut self.font_table.sys(), true);

        let layout = input.layout_text(self.glyph_cache.get_mut(), &mut self.wgpu);
        let text_dim = layout.size();

        let total_h = (text_dim.y).max(self.style.line_height());
        let vert_pad = ((total_h - text_dim.y) / 2.0).max(0.0);
        let horiz_pad = vert_pad;
        let size = Vec2::new(text_dim.x + horiz_pad * 2.0, total_h);

        let rect = self.place_item(size);
        // let sig = self.register_item_ex(id, ui::ItemFlags::ACTIVATE_ON_RELEASE);

        let itm_flag = if flags.has(TextInputFlags::SELECT_ON_ACTIVE) {
            // TODO[NOTE]: we need this because without it SELECT_ON_ACTIVE does not work
            // on press the text is selected but most certainly the mouse is still pressed on the next
            // frame immediately deselecting it. currently the item needs to be selected when the mouse
            // is no longer pressed
            ItemFlags::SET_ACTIVE_ON_RELEASE
        } else {
            ItemFlags::SET_ACTIVE_ON_PRESS
        };

        let sig = self.reg_item_ex(id, rect, itm_flag);

        if sig.hovering() || sig.dragging() {
            self.set_cursor_icon(CursorIcon::Text);
        }

        let relative_pos = self.mouse.pos - rect.min;

        let input = &mut self.widget_data.get_mut::<TextInputState>(&id).unwrap();
        if sig.double_pressed() {
            // TODO[BUG]: only works when double pressing and dragging afterwards, if holding the
            // double press we loose word selection mode
            input.mouse_double_clicked(relative_pos);
        } else if sig.dragging() {
            input.mouse_dragging(relative_pos);
        } else if sig.pressed() {
            input.mouse_pressed(relative_pos);
        }

        if self.active_id != id {
            input.deselect_all();
        }

        if self.active_id_changed
            && self.active_id == id
            && flags.has(TextInputFlags::SELECT_ON_ACTIVE)
        {
            input.select_all();
        }

        let text_pos =
            rect.min + Vec2::new((size.x - text_dim.x) * 0.5, (size.y - text_dim.y) * 0.5);
        // Draw input background (caller is responsible now)
        let bg = self.style.panel_dark_bg();
        self.draw(
            rect.draw_rect()
                .fill(bg)
                .corners(self.style.btn_corner_radius()),
        );
        self.draw_text_input(id, text_pos, rect);
    }

    pub fn draw_text_input(&mut self, id: Id, pos: Vec2, rect: Rect) {
        use ctext::Edit;
        use unicode_segmentation::UnicodeSegmentation;

        let bg = self.style.panel_dark_bg();
        let text_color = self.style.text_col();
        let cursor_color = self.style.btn_press();
        let selection_color = self.style.btn_hover();
        let selected_text_color = self.style.text_col();

        let input = &mut self.widget_data.get_mut::<TextInputState>(&id).unwrap();

        let mut glyphs = Vec::new();
        let mut selection_rects = Vec::new();
        let mut cursor_rects = Vec::new();
        // let mut cursor_rects: Vec<(i32, i32, u32, u32)> = Vec::new();

        let sel_bounds = input.edit.selection_bounds();
        let cursor = input.edit.cursor();
        input.edit.with_buffer_mut(|buffer| {
            for run in buffer.layout_runs() {
                let line_i = run.line_i;
                let line_y = run.line_y;
                let line_top = run.line_top;
                let line_height = run.line_height;

                // Selection highlighting (collect rects)
                // Selection highlighting (collect rects)
                if let Some((start, end)) = sel_bounds {
                    if line_i >= start.line && line_i <= end.line {
                        // use floats for accurate accumulation to avoid zero-width from truncation
                        let mut range_opt: Option<(f32, f32)> = None;

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
                                        Some((min_f, max_f)) => {
                                            Some((min_f.min(c_x), max_f.max(c_x + c_w)))
                                        }
                                        None => Some((c_x, c_x + c_w)),
                                    };
                                } else if let Some((min_f, max_f)) = range_opt.take() {
                                    let min = min_f.floor();
                                    let pos = Vec2::new(min, line_top);
                                    let max = max_f.ceil();
                                    let size = Vec2::new((max - min).max(0.0), line_height);
                                    selection_rects.push(Rect::from_min_size(pos, size));
                                }
                                c_x += c_w;
                            }
                        }

                        // IMPORTANT: Push any remaining accumulated range after processing all glyphs
                        // This handles the case where the selection continues to the end of the line
                        if let Some((min_f, max_f)) = range_opt.take() {
                            let min = min_f.floor();
                            let pos = Vec2::new(min, line_top);
                            let max = max_f.ceil();
                            let size = Vec2::new((max - min).max(0.0), line_height);
                            selection_rects.push(Rect::from_min_size(pos, size));
                        }

                        if run.glyphs.is_empty() && end.line > line_i {
                            range_opt = Some((0.0, buffer.size().0.unwrap_or(0.0)));
                        }

                        if let Some((mut min_f, mut max_f)) = range_opt.take() {
                            if end.line > line_i {
                                if run.rtl {
                                    min_f = 0.0;
                                } else {
                                    max_f = buffer.size().0.unwrap_or(0.0);
                                }
                            }
                            let min = min_f.floor();
                            let pos = Vec2::new(min, line_top);
                            let max = max_f.ceil();
                            let size = Vec2::new((max - min).max(0.0), line_height);
                            selection_rects.push(Rect::from_min_size(pos, size));
                        }
                    }
                }

                // Cursor
                if let Some((x, y)) = cursor_position(&cursor, &run) {
                    let pos = Vec2::new(x as f32, y as f32);
                    let size = Vec2::new(2.0, line_height);
                    cursor_rects.push(Rect::from_min_size(pos, size))
                    // cursor_rects.push((x, y, 1, line_height as u32));
                }

                // Glyphs (collect textured quads + color)
                for glyph in run.glyphs.iter() {
                    let physical_glyph = glyph.physical((0., 0.), 1.0);
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
                    let wgpu = &self.wgpu;
                    if let Some(mut cached) = cache.get_glyph(key, wgpu) {
                        let pos = cached.meta.pos
                            + Vec2::new(
                                physical_glyph.x as f32,
                                physical_glyph.y as f32 + run.line_y,
                            );
                        let size = cached.meta.size;
                        let uv_min = cached.meta.uv_min;
                        let uv_max = cached.meta.uv_max;

                        glyphs.push((
                            ui::GlyphMeta {
                                pos,
                                size,
                                uv_min,
                                uv_max,
                            },
                            glyph_color,
                        ));
                    }
                }
            }
        });

        // Draw: selection -> cursor -> glyphs
        // self.draw(|list| {

        // list.rect(rect.min, rect.max)
        //     .corners(CornerRadii::all(self.style.btn_corner_radius()))
        //     .fill(bg)
        //     .add();
        // Background is drawn by the caller; draw selection highlights next
        self.draw(
            selection_rects
                .iter()
                .map(|r| r.draw_rect().offset(pos).fill(selection_color)),
        );

        // for r in &selection_rects {
        //     list.rect(r.min + pos, r.max + pos)
        //         .fill(selection_color)
        //         .add();
        // }

        if self.active_id == id && selection_rects.is_empty() {
            self.draw(
                cursor_rects
                    .iter()
                    .map(|r| r.draw_rect().offset(pos).fill(cursor_color)),
            );
            // for r in cursor_rects {
            //     list.rect(r.min + pos, r.max + pos).fill(cursor_color).add();
            // }
        }

        self.draw(glyphs.iter().map(|(g, color)| {
            let min = g.pos;
            let max = min + g.size;
            Rect::from_min_size(g.pos, g.size)
                .draw_rect()
                .offset(pos)
                .fill(*color)
                .texture(TextureId::GLYPH)
                .uv(g.uv_min, g.uv_max)
        }));

        // for (g, color) in glyphs {
        //     let min = g.pos;
        //     let max = min + g.size;
        //     list.rect(min + pos, max + pos)
        //         .texture_uv(g.uv_min, g.uv_max, 1)
        //         .fill(color)
        //         .add();
        // }
        // });
    }

    pub fn begin_tabbar(&mut self, label: &str) {
        // TODO[NOTE] tabbar stack
        let id = self.gen_id(label);
        // self.tabbars.map.entry(id).or_insert(ui::TabBar::new());
        let _ = self.widget_data.get_or_insert(id, ui::TabBar::new());

        self.tabbar_stack.push(id);
        self.current_tabbar_id = id;
        self.push_id(id);

        let avail = self.available_content();

        self.push_style(ui::StyleVar::SpacingV(0.0));
        let rect = self.place_item(Vec2::new(avail.x, self.style.line_height()));
        self.pop_style();
        self.separator_h(3.0, self.style.btn_hover());

        let cursor = self.get_current_panel()._cursor.clone().into_inner();

        let tb = self.widget_data.get_mut::<TabBar>(&id).unwrap();
        tb.id = id;
        tb.panel_id = self.current_panel_id;
        tb.cursor_backup = cursor;
        tb.bar_rect = rect;

        tb.layout_tabs();

        // clamp scroll offset to valid range after laying out tabs
        let max_scroll = (tb.total_width - rect.width()).max(0.0);
        tb.scroll_offset = tb.scroll_offset.clamp(0.0, max_scroll);

        if rect.contains(self.mouse.pos) {
            self.hot_tabbar_id = id;
        }
    }

    pub fn end_tabbar(&mut self) {
        let tb_id = self.tabbar_stack.pop().expect("end_tabbar without matching begin_tabbar");
        let tb = self.widget_data.get::<TabBar>(&tb_id).unwrap();
        let tb_id_confirm = tb.id;
        assert!(self.pop_id() == tb_id_confirm);

        self.current_tabbar_id = self.tabbar_stack.last().copied().unwrap_or(Id::NULL);
        // self.get_current_panel()._cursor.replace(cursor);
    }

    pub fn tabitem(&mut self, label: &str) -> bool {
        let tb_id = self.current_tabbar_id;
        // let tb_rect = self.tabbars[tb_id].bar_rect;
        let tb_rect = self.widget_data.get::<TabBar>(&tb_id).unwrap().bar_rect;
        assert!(!tb_id.is_null());

        let id = self.gen_id(label);
        // let tb = &mut self.tabbars[tb_id];
        let tb = self.widget_data.get_mut::<TabBar>(&tb_id).unwrap();
        if tb.tabs.is_empty() {
            tb.selected_tab_id = id;
        }

        let text_shape = self.layout_text(label, self.style.text_size());
        let text_dim = text_shape.size();
        let vert_pad = ((tb_rect.height() - text_dim.y) / 2.0).max(0.0);
        let item_width = vert_pad * 2.0 + text_dim.x;

        let tb = self.widget_data.get_mut::<TabBar>(&tb_id).unwrap();
        // let tb = &mut self.tabbars[tb_id];
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
        // account for horizontal scrolling when placing tabs
        let rect = Rect::from_min_size(tb_rect.min + Vec2::new(item.offset - tb.scroll_offset, 0.0), tab_size);
        let sig = self.reg_item_active_on_press(id, rect);

        let (btn_col, text_col) = if is_selected {
            (self.style.btn_hover(), self.style.text_col())
        } else if sig.hovering() {
            (self.style.btn_default(), self.style.text_col())
        } else {
            (self.style.panel_bg(), self.style.text_col())
        };

        // let tb = &mut self.tabbars[tb_id];
        let tb = self.widget_data.get_mut::<TabBar>(&tb_id).unwrap();

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

        if tb.is_dragging && tb.selected_tab_id == id {
            item_pos.x = item_pos
                .x
                .max(tb_rect.min.x)
                .min(tb_rect.max.x - rect.width());
        }

        let text_pos = item_pos
            + Vec2::new(
                (item.width - text_dim.x) * 0.5,
                (tb_rect.height() - text_dim.y) * 0.5,
            );

        if tb.is_dragging && tb.selected_tab_id == id {
            self.draw_over(
                Rect::from_min_size(item_pos, rect.size())
                    .draw_rect()
                    .fill(btn_col)
                    .corners(CornerRadii::top(self.style.btn_corner_radius())),
            )
            .draw_over(text_shape.draw_rects(text_pos, text_col));
        } else {
            self.draw(
                Rect::from_min_size(item_pos, rect.size())
                    .draw_rect()
                    .fill(btn_col)
                    .corners(CornerRadii::top(self.style.btn_corner_radius())),
            )
            .draw(text_shape.draw_rects(text_pos, text_col));
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
