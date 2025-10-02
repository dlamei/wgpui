use glam::Vec2;

use crate::{
    core::RGBA, mouse::{CursorIcon, MouseBtn}, ui::{self, CornerRadii}
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
        let text_dim = self.shape_text(label, self.style.text_size()).size();

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
                .corners(CornerRadii::all(height * 0.3))
                .fill(bg_col)
                .add();

            let knob_r = height * 0.8 * 0.5;
            let knob_x = if *b {
                rail_max.x - height * 0.5
            } else {
                rail_min.x + height * 0.5
            };
            let knob_center = Vec2::new(knob_x, rail_min.y + height * 0.5);
            list.circle(knob_center, knob_r).corners(CornerRadii::all(height * 0.8 * 0.3)).fill(knob_col).add();
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

    pub fn slider_f32(&mut self, label: &str, min: f32, max: f32, val: &mut f32) {
        let height = self.style.line_height();
        let width = self.available_content().x / 3.0;
        let size = Vec2::new(width, height);

        let id = self.gen_id(label);
        let rect = self.place_item(id, size);
        let sig = self.register_item(id);

        let knob_diam = height * 0.8;
        let knob_r = knob_diam * 0.5;
        let usable_width = (rect.width() - knob_diam).max(0.0);

        if sig.pressed() || sig.dragging() {
            let denom = if usable_width <= 0.0 { 1.0 } else { usable_width };
            let t = ((self.mouse.pos.x - (rect.min.x + knob_r)) / denom).clamp(0.0, 1.0);
            if (max - min).abs() > std::f32::EPSILON {
                *val = min + t * (max - min);
            }
        }

        let ratio = if (max - min).abs() < std::f32::EPSILON {
            0.0
        } else {
            ((*val - min) / (max - min)).clamp(0.0, 1.0)
        };

        let knob_x = rect.min.x + knob_r + ratio * usable_width;
        let knob_center = Vec2::new(knob_x, rect.min.y + height * 0.5);

        let mouse = self.mouse.pos;
        let dx = mouse.x - knob_center.x;
        let dy = mouse.y - knob_center.y;
        let over_knob = (dx * dx + dy * dy) <= (knob_r * knob_r);

        if sig.hovering() {
            self.set_cursor_icon(CursorIcon::MoveH);
        }

        if sig.pressed() && !sig.dragging() {
            self.expect_drag = true;
        }

        let mut rail_col = if sig.hovering() { self.style.btn_hover() } else { self.style.btn_default() };
        let mut knob_col = if sig.dragging() {
            self.style.btn_press()
        } else {
            self.style.btn_press()
        };

        if sig.dragging() {
            std::mem::swap(&mut rail_col, &mut knob_col);
        }

        self.draw(|list| {
            // rail (no left->right fill; knob indicates value)
            list.rect(rect.min, rect.max)
                .corners(CornerRadii::all(height * 0.3))
                .fill(rail_col)
                .add();

            // knob
            list.circle(knob_center, knob_r)
                .corners(CornerRadii::all(height * 0.8 * 0.3))
                .fill(knob_col)
                .add();
            });

        self.same_line();
        self.text(label);
    }


    pub fn slider_f322(&mut self, label: &str, min: f32, max: f32, val: &mut f32) {
        let height = self.style.line_height();
        let width = height * 6.0;
        let size = Vec2::new(width, height);

        let id = self.gen_id(label);
        let rect = self.place_item(id, size);
        let sig = self.register_item(id);

        let ratio = ((*val - min) / (max - min)).clamp(0.0, 1.0);
        if sig.dragging() {
            let t = ((self.mouse.pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
            *val = min + t * (max - min);
        }

        if sig.dragging() || sig.hovering() {
            self.set_cursor_icon(CursorIcon::MoveH)
        }

        let bg_col = self.style.btn_default();
        let fill_col = self.style.btn_press();

        self.draw(|list| {
            list.rect(rect.min, rect.max)
                .corners(CornerRadii::all(height * 0.3))
                .fill(bg_col)
                .add();
            let filled = Vec2::new(rect.min.x + rect.width() * ratio, rect.max.y);
            list.rect(rect.min, filled)
                .corners(CornerRadii::all(height * 0.3))
                .fill(fill_col)
                .add();
            });

        self.same_line();
        self.text(label);
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
        self.register_item(id);
        self.move_down(pad);

        self.draw(|list| list.add_text(rect.min, &shape, self.style.text_col()));
    }


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
}
