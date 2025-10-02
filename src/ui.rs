use std::{
    cell::{Ref, RefCell},
    fmt, hash,
    rc::Rc,
};
use cosmic_text as ctext;
use glam::{Mat4, UVec2, Vec2};
use wgpu::util::DeviceExt;

use crate::{
    core::{id_type, stacked_fields_struct, ArrVec, DataMap, Dir, HashMap, Instant, RGBA}, gpu::{self, RenderPassHandle, ShaderHandle, WGPUHandle, Window, WindowId, WGPU}, mouse::{CursorIcon, MouseBtn, MouseState}, rect::Rect, Vertex as VertexTyp
};

fn dark_theme() -> StyleTable {
    use StyleField as SF;
    use StyleVar as SV;
    StyleTable::init(|f| {
        let accent = RGBA::hex("#cbdfd4");
        let btn_default = RGBA::hex("#4f5559");
        let dark = RGBA::hex("#1d1d1d");
        let btn_hover = RGBA::hex("#576a76");

        match f {
            SF::TitlebarColor => SV::TitlebarColor(dark),
            SF::TitlebarHeight => SV::TitlebarHeight(30.0),
            SF::TextSize => SV::TextSize(18.0),
            SF::TextCol => SV::TextCol(RGBA::hex("#EEEBE1")),
            SF::LineHeight => SV::LineHeight(24.0),
            SF::BtnRoundness => SV::BtnRoundness(0.15),
            SF::BtnDefault => SV::BtnDefault(btn_default),
            SF::BtnHover => SV::BtnHover(btn_hover),
            SF::BtnPress => SV::BtnPress(accent),
            SF::BtnPressText => SV::BtnPressText(btn_default),
            SF::WindowBg => SV::WindowBg(RGBA::hex("#5c6b6f")),
            SF::PanelBg => SV::PanelBg(RGBA::hex("#343B40")),
            SF::PanelCornerRadius => SV::PanelCornerRadius(5.0),
            SF::PanelOutline => {
                SV::PanelOutline(Outline::new(dark, 1.5).with_place(OutlinePlacement::Outer))
            }
            SF::PanelHoverOutline => SV::PanelHoverOutline(
                Outline::new(btn_hover, 1.5).with_place(OutlinePlacement::Outer),
            ),
            SF::PanelPadding => SV::PanelPadding(10.0),
            SF::SpacingV => SV::SpacingV(12.0),
            SF::SpacingH => SV::SpacingH(12.0),
            SF::Red => SV::Red(RGBA::hex("#e65858")),
        }
    })
}

pub struct Context {
    // pub panels: HashMap<Id, Panel>,
    pub panels: PanelMap,
    pub widget_data: DataMap<Id>,
    // pub style: Style,
    pub style: StyleTable,

    pub current_panel_stack: Vec<Id>,
    pub current_panel_id: Id,
    pub draw_order: Vec<Id>,

    // TODO[CHECK]: still needed? how to use exactly
    pub prev_item_data: PrevItemData,
    pub panel_action: PanelAction,
    // pub resizing_window_dir: Option<Dir>,
    pub next: NextPanelData,

    // TODO[CHECK]: when do we set the panels and item ids?
    // TODO[BUG]: if cursor quickly exists window hot_id may not be set to NULL
    /// the id of the element that is currently hovered
    ///
    /// can either be an item or a panel
    pub hot_id: Id,

    /// the id of the element that is currently active
    ///
    /// Can either be an item or a panel.
    /// This allows e.g. dragging the panel by its titlebar (item) or the panel itself
    pub active_id: Id,

    pub prev_hot_id: Id,
    pub prev_active_id: Id,

    /// the id of the hot panel
    ///
    /// the hot_id can only point to elements of the currently hot panel
    pub hot_panel_id: Id,

    /// the id of the active panel
    ///
    /// the active_id can only point to elements of the currently active panel
    pub active_panel_id: Id,
    pub root_panel_id: Id,
    pub root_panel_titlebar_height: f32,

    pub prev_hot_panel_id: Id,
    pub prev_active_panel_id: Id,

    pub expect_drag: bool,

    pub draw_wireframe: bool,
    pub draw_clip_rect: bool,
    pub draw_item_outline: bool,
    pub circle_max_err: f32,

    pub frame_count: u64,
    pub prev_frame_time: Instant,

    pub mouse: MouseState,
    pub cursor_icon: CursorIcon,
    pub cursor_icon_changed: bool,
    pub resize_threshold: f32,
    pub n_draw_calls: usize,

    pub draw: MergedDrawLists,
    pub glyph_cache: GlyphCache,
    pub text_item_cache: TextItemCache,
    pub font_table: FontTable,

    pub close_pressed: bool,
    pub window: Window,
    pub requested_windows: Vec<(Vec2, Vec2)>,
    pub ext_window: Option<Window>,
}

impl Context {
    pub fn new(wgpu: WGPUHandle, window: Window) -> Self {
        let glyph_cache = GlyphCache::new(&wgpu);
        let mut font_table = FontTable::new();
        font_table.load_font(
            "Rubik",
            include_bytes!("../res/Inter-VariableFont_opsz,wght.ttf").to_vec(),
        );

        Self {
            panels: PanelMap::default(),
            widget_data: DataMap::new(),
            // style: Style::dark(),
            style: dark_theme(),
            draw: MergedDrawLists::new(glyph_cache.texture.clone(), wgpu),
            current_panel_stack: vec![],
            current_panel_id: Id::NULL,
            prev_item_data: PrevItemData::new(),

            hot_id: Id::NULL,
            hot_panel_id: Id::NULL,
            active_id: Id::NULL,
            active_panel_id: Id::NULL,
            root_panel_id: Id::NULL,
            root_panel_titlebar_height: 0.0,

            panel_action: PanelAction::None,
            prev_hot_panel_id: Id::NULL,
            prev_active_panel_id: Id::NULL,
            prev_hot_id: Id::NULL,
            prev_active_id: Id::NULL,
            expect_drag: false,
            // resizing_window_dir: None,
            next: NextPanelData::default(),

            draw_order: Vec::new(),
            draw_wireframe: false,
            draw_clip_rect: false,
            draw_item_outline: false,
            circle_max_err: 0.3,


            frame_count: 0,
            prev_frame_time: Instant::now(),
            mouse: MouseState::new(),
            cursor_icon: CursorIcon::Default,
            cursor_icon_changed: false,
            resize_threshold: 10.0,
            n_draw_calls: 0,

            glyph_cache,
            text_item_cache: TextItemCache::new(),
            font_table,

            close_pressed: false,
            window,
            requested_windows: Vec::new(),
            ext_window: None,
        }
    }

    pub fn get_mut_window(&mut self, id: WindowId) -> &mut Window {
        if id == self.window.id {
            &mut self.window
        } else {
            self.ext_window.as_mut().unwrap()
        }
    }

    pub fn get_window(&self, id: WindowId) -> &Window {
        if id == self.window.id {
            &self.window
        } else {
            self.ext_window.as_ref().unwrap()
        }
    }

    pub fn resize_window(&mut self, id: WindowId, x: u32, y: u32) {
        let wgpu = self.draw.wgpu.clone();
        self.get_mut_window(id).resize(x, y, &wgpu.device);
        // self.window.resize(x, y, &self.draw.wgpu.device)
    }

    /// apply changes to the cursor icon
    ///
    /// called only once every frame to prevent flickering
    pub fn update_cursor_icon(&mut self) {
        // this is needed because outside events can change the icon, so we only update the icon
        // when it was manually changed
        if self.cursor_icon_changed {
            self.window.set_cursor_icon(self.cursor_icon)
        }
    }

    pub fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if self.cursor_icon != icon {
            self.cursor_icon = icon;
            self.cursor_icon_changed = true;
        }
    }

    pub fn set_mouse_press(&mut self, btn: MouseBtn, press: bool) {
        self.mouse.set_button_press(btn, press);

        let w_size = self.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);

        let mut resize_dir = None;
        if !self.window.is_maximized() {
            resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold);
        }

        let lft_btn = btn == MouseBtn::Left;

        if self.window.is_decorated() {
            return;
        }

        if press && lft_btn {
            let root_panel = self.get_root_panel();
            let titlebar_height = root_panel.titlebar_height;
            if let Some(dir) = resize_dir {
                self.window.start_drag_resize_window(dir)
            } else if self.mouse.pos.y <= titlebar_height {
                self.window.start_drag_window()
            }
        }
    }


    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {

        self.mouse.set_mouse_pos(x, y);

        let w_size = self.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);
        let resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold);

        if resize_dir.is_none() && self.cursor_icon.is_resize() {
            self.set_cursor_icon(CursorIcon::Default);
        }

        if self.window.is_maximized() || self.window.is_decorated() {
            return;
        }

        if let Some(dir) = resize_dir {
            self.set_cursor_icon(dir.as_cursor());
        }
    }

    pub fn draw(&self, f: impl FnOnce(&mut DrawList)) {
        let p = self.get_current_panel();
        let draw_list = &mut p.draw_list.borrow_mut();
        f(draw_list)
    }

    pub fn gen_id(&self, label: impl hash::Hash) -> Id {
        self.get_current_panel().gen_id(label)
    }

    pub fn begin(&mut self, name: impl Into<String>) {
        self.begin_ex(name, PanelFlags::NONE);
    }

    pub fn begin_ex(&mut self, name: impl Into<String>, flags: PanelFlags) {
        fn next_window_pos(screen: Vec2, panel_size: Option<Vec2>) -> Vec2 {
            static mut PANEL_COUNT: u32 = 1;
            let offset = 60.0;
            let size = panel_size.unwrap_or(Vec2::new(500.0, 300.0));

            let (x, y);
            unsafe {
                x = (offset * PANEL_COUNT as f32) % (screen.x - size.x).max(0.0);
                y = (offset * PANEL_COUNT as f32) % (screen.y - size.y).max(0.0);
                PANEL_COUNT += 1;
            }

            Vec2::new(x, y)
        }

        let mut newly_created = false;
        let name: String = name.into();
        let mut id = self.get_panel_id_with_name(&name);
        if id.is_null() {
            id = self.create_panel(name);
            newly_created = true;
        }

        self.current_panel_stack.push(id);
        self.current_panel_id = id;

        let p = &mut self.panels[id];
        if newly_created {
            p.draw_order = self.draw_order.len();
            self.draw_order.push(id);

            if self.next.pos.is_none() {
                p.pos = next_window_pos(self.draw.screen_size, self.next.size);
            }
        }
        if let Some(pos) = self.next.pos {
            p.pos = pos;
        }

        p.clear_temp_data();

        assert!(p.id == id);
        p.push_id(p.id);
        p.flags = flags;
        p.explicit_size = self.next.size;
        p.draw_list.borrow_mut().circle_max_err = self.circle_max_err;
        p.titlebar_height = self.style.titlebar_height();
        p.padding = self.style.panel_padding();
        p.layout = self.next.layout;
        p.last_frame_used = self.frame_count;
        p.move_id = p.gen_id("#MOVE");
        p.min_size = self.next.min_size;
        p.max_size = self.next.max_size;

        if p.id != self.root_panel_id {
            p.pos.y = p.pos.y.max(self.root_panel_titlebar_height);
            let pad = self.root_panel_titlebar_height;
            let screen = self.draw.screen_size;
            p.pos.x = p.pos.x.min(screen.x - pad);
            p.pos.x = p.pos.x.max(-p.size.x + pad);
            p.pos.y = p.pos.y.min(screen.y - pad);
        }

        if flags.has(PanelFlags::NO_MOVE) {
            p.move_id = Id::NULL;
        } else if flags.has(PanelFlags::NO_TITLEBAR) {
            // move the window by dragging it if no titlebar exists
            p.move_id = p.id;
            p.titlebar_height = 0.0;
        }

        self.next.reset();
        p.root = p.id;

        let p = &mut self.panels[id];

        let prev_max_pos = p.cursor_max_pos();

        let content_start = p.pos + Vec2::new(p.padding, p.padding + p.titlebar_height);
        p.init_content_cursor(content_start);

        let outline = if p.id == self.prev_hot_panel_id {
            self.style.panel_hover_outline()
        } else {
            self.style.panel_outline()
        };

        let outline_offset = match outline.place {
            OutlinePlacement::Outer => outline.width,
            OutlinePlacement::Center => outline.width / 2.0,
            OutlinePlacement::Inner => 0.0,
        };
        // preserve when?
        p.outline_offset = outline_offset;
        p.full_content_size = prev_max_pos - p.content_start_pos();
        p.full_size = prev_max_pos - p.pos;

        if self.frame_count - p.frame_created == 1 {
            p.size = p.full_size;
        }

        let panel_pos = p.pos;

        // bg
        let panel_size = if let Some(size) = p.explicit_size {
            size
        } else {
            p.size
        };

        p.size = panel_size.min(p.max_panel_size()).max(p.min_panel_size());

        let p = &self.panels[id];
        let panel_rect = p.panel_rect();

        if panel_rect.contains(self.mouse.pos) {
            if self.hot_panel_id.is_null()
                || self.panels[self.hot_panel_id].draw_order < p.draw_order
            {
                if !p.flags.has(PanelFlags::NO_FOCUS) {
                    self.hot_panel_id = id;
                    self.hot_id = id;
                }
            }
        }

        // TODO[NOTE]: include outline width in panel size
        // draw panel
        let is_window_panel = p.is_window_panel;

        // draw background
        let bg_fill = if p.is_window_panel {
            self.style.window_bg()
        } else {
            self.style.panel_bg()
        };

        p.with_draw_list(|list| {
            let clip = p.panel_rect();
            list.push_clip_rect(clip);
            if self.draw_clip_rect {
                list.add_rect_outline(clip.min, clip.max, Outline::new(RGBA::RED, 2.0));
            }

            list.rect(panel_pos, panel_pos + panel_size)
                .fill(bg_fill)
                .outline(outline)
                .corners(CornerRadii::all(self.style.panel_corner_radius()))
                .add();
        });

        // let win_rect = self.window.window_rect();
        // if !win_rect.contains_rect(panel_rect) {
        //     self.requested_windows.push((p.size, self.window.window_pos() + p.pos));
        // }

        // let p = &self.panels[id];
        if !p.flags.has(PanelFlags::NO_TITLEBAR) {
            // draw titlebar
            p.with_draw_list(|list| {
                list.rect(
                    panel_pos,
                    panel_pos + Vec2::new(panel_size.x, p.titlebar_height),
                )
                    .fill(self.style.titlebar_color())
                    .corners(CornerRadii::top(self.style.panel_corner_radius()))
                    .add()
            });
            // let tb_rect = Rect::from_min_size(p.pos, Vec2::new(panel_size.x, p.titlebar_height));
            // self.set_cursor_pos(panel_pos);

            self.register_rect(
                p.move_id,
                Rect::from_min_size(panel_pos, Vec2::new(panel_size.x, p.titlebar_height)),
            );

            let p = &self.panels[id];
            let btn_size = Vec2::new(25.0, 25.0);

            let mut btns_pos = p.pos
                + Vec2::new(
                    panel_size.x - (btn_size.x + 10.0),
                    (p.titlebar_height - btn_size.y) / 2.0,
                );

            if is_window_panel {
                let min_id = self.panels[id].gen_id("min");

                // let mut btns_pos = Vec2::new(panel_size.x - 15.0 - (btn_size.x + 10.0) * 2.0, (p.titlebar_height - btn_size.y) / 2.0);
                btns_pos.x -= (btn_size.x + 10.0) * 2.0;

                let sig = self.register_rect(min_id, Rect::from_min_size(btns_pos, btn_size));
                let mut color = self.style.text_col();
                if sig.hovering() {
                    color = self.style.btn_hover();
                }
                if sig.released() {
                    self.window.minimize();
                }

                let p = &mut self.panels[id];
                // draw minimize button
                p.with_draw_list(|list| {
                    list.rect(btns_pos, btns_pos + btn_size)
                        .fill(color)
                        .circle()
                        .add()
                });

                let max_id = self.panels[id].gen_id("max");
                btns_pos.x += 10.0 + btn_size.x;
                let sig = self.register_rect(max_id, Rect::from_min_size(btns_pos, btn_size));
                let mut color = self.style.text_col();
                if sig.hovering() {
                    color = self.style.btn_hover();
                }
                if sig.released() {
                    self.window.toggle_maximize();
                }

                let p = &mut self.panels[id];
                // draw maximize button
                p.with_draw_list(|list| {
                    list.rect(btns_pos, btns_pos + btn_size)
                        .fill(color)
                        .circle()
                        .add()
                });

                btns_pos.x += 10.0 + btn_size.x;
            }

            let p = &self.panels[id];

            self.move_cursor(Vec2::new(btn_size.x + 10.0, 0.0));
            let close_id = p.gen_id("X");
            let sig = self.register_rect(close_id, Rect::from_min_size(btns_pos, btn_size));

            let mut color = RGBA::WHITE;
            if sig.hovering() {
                color = self.style.red();
            }
            if sig.pressed() {
                self.panels[id].close_pressed = true;
            }

            let p = &self.panels[id];
            // draw close button
            p.with_draw_list(|list| {
                list.rect(btns_pos, btns_pos + btn_size)
                    .fill(color)
                    .circle()
                    .add()
            });

            // start drawing content
            self.set_cursor_pos(self.content_start_pos());
            self.prev_item_data.reset();
        }

        let p = &self.panels[id];
        p.with_draw_list(|list| {
            let clip = p.content_rect();
            list.push_clip_rect(clip);
            if self.draw_clip_rect {
                list.add_rect_outline(clip.min, clip.max, Outline::new(RGBA::RED, 2.0));
            }
        })
    }

    pub fn update_panel_resize(&mut self) {
        if let Some(p) = self.panels.get_mut(self.hot_panel_id) {
            let id = p.id;
            let rect = p.panel_rect();
            let dir = is_in_resize_region(rect, self.mouse.pos, self.resize_threshold);
            if dir.is_some()
                && self.panel_action.is_none()
                && !(p.flags.has(PanelFlags::NO_RESIZE) || p.is_window_panel)
            {
                let dir = dir.unwrap();
                self.set_cursor_icon(dir.as_cursor());

                if self.mouse.pressed(MouseBtn::Left) && !self.mouse.dragging(MouseBtn::Left) {
                    self.panel_action = PanelAction::Resize {
                        dir,
                        id,
                        prev_rect: rect,
                    };
                }
            }
        }

        if let PanelAction::Resize { dir, id, prev_rect } = &self.panel_action {
            if !self.mouse.pressed(MouseBtn::Left) {
                self.set_cursor_icon(CursorIcon::Default);
                self.panel_action = PanelAction::None;
                return;
            }
            let p = &mut self.panels[*id];
            let pr = *prev_rect;
            let mut nr = pr;

            let min_size = p.min_panel_size();
            let max_size = p.max_panel_size();

            let m_start = self
                .mouse
                .drag_start(MouseBtn::Left)
                .unwrap_or(self.mouse.pos);
            let m_delta = self.mouse.pos - m_start;

            if dir.has_n() {
                let min_y = pr.max.y - max_size.y;
                let max_y = pr.max.y - min_size.y;
                nr.min.y = (pr.min.y + m_delta.y).clamp(min_y, max_y);
            }
            if dir.has_s() {
                let min_y = pr.min.y + min_size.y;
                let max_y = pr.min.y + max_size.y;
                nr.max.y = (pr.max.y + m_delta.y).clamp(min_y, max_y);
            }
            if dir.has_w() {
                let min_x = pr.max.x - max_size.x;
                let max_x = pr.max.x - min_size.x;
                nr.min.x = (pr.min.x + m_delta.x).clamp(min_x, max_x);
            }
            if dir.has_e() {
                let min_x = pr.min.x + min_size.x;
                let max_x = pr.min.x + max_size.x;
                nr.max.x = (pr.max.x + m_delta.x).clamp(min_x, max_x);
            }

            p.move_panel_to(nr.min);
            p.size = nr.size();
        }
    }

    pub fn update_panel_move(&mut self) {
        if !self.active_panel_id.is_null() {
            let p = &mut self.panels[self.active_panel_id];
            if self.active_id == p.move_id && !p.move_id.is_null() {
                if self.mouse.dragging(MouseBtn::Left) && self.panel_action.is_none() {
                    self.panel_action = PanelAction::Move {
                        id: p.root,
                        start_pos: p.pos,
                    }
                }
                if !self.mouse.dragging(MouseBtn::Left)
                    && matches!(self.panel_action, PanelAction::Move { .. })
                {
                    self.panel_action = PanelAction::None;
                }
            }
        }

        if let &PanelAction::Move {
            start_pos,
            id: drag_id,
        } = &self.panel_action
        {
            if self.mouse.dragging(MouseBtn::Left) {
                if let Some(drag_start) = self.mouse.drag_start(MouseBtn::Left) {
                    let p = &mut self.panels[drag_id];
                    let mouse_delta = start_pos - drag_start;
                    // p.pos = self.mouse.pos + mouse_delta;
                    p.move_panel_to(self.mouse.pos + mouse_delta);
                }
            }
        }
    }

    pub fn end_assert(&mut self, name: Option<&str>) {
        let p = self.get_current_panel();
        if let Some(name) = name {
            assert!(name == &p.name);
        }

        let p = self.get_current_panel();
        let p_pad = p.padding;
        // p.id_stack.pop().unwrap();
        p.pop_id();
        if !p.id_stack_ref().is_empty() {
            log::warn!("non empty id stack at ");
        }
        // self.offset_cursor_pos(Vec2::splat(p_pad));

        {
            let mut c = p.cursor.borrow_mut();
            c.max_pos += Vec2::splat(p.padding);
        }

        p.with_draw_list(|list| {
            list.pop_clip_rect();
        });

        self.current_panel_stack.pop();
        self.current_panel_id = self.current_panel_stack.last().copied().unwrap_or(Id::NULL);
    }

    pub fn end(&mut self) {
        self.end_assert(None)
    }

    pub fn get_item_signal(&self, id: Id, bb: Rect) -> Signal {
        use MouseBtn as Btn;
        let mut sig = Signal::empty();

        if bb.contains(self.mouse.pos) {
            sig |= Signal::MOUSE_OVER;

            if self.hot_id == id {
                sig |= Signal::HOVERING;
            }
        }

        // if !sig.hovering() {
        //     return sig;
        // }

        if sig.hovering() && self.active_id == id {
            if self.mouse.pressed(Btn::Left) {
                sig |= Signal::PRESSED_LEFT;
            }
            if self.mouse.pressed(Btn::Right) {
                sig |= Signal::PRESSED_RIGHT;
            }
            if self.mouse.pressed(Btn::Middle) {
                sig |= Signal::PRESSED_MIDDLE;
            }

            if self.mouse.double_clicked(Btn::Left) {
                sig |= Signal::DOUBLE_CLICKED_LEFT;
            }
            if self.mouse.double_clicked(Btn::Right) {
                sig |= Signal::DOUBLE_CLICKED_RIGHT;
            }
            if self.mouse.double_clicked(Btn::Middle) {
                sig |= Signal::DOUBLE_CLICKED_MIDDLE;
            }

            if self.mouse.released(Btn::Left) {
                sig |= Signal::RELEASED_LEFT
            }
            if self.mouse.released(Btn::Right) {
                sig |= Signal::RELEASED_RIGHT
            }
            if self.mouse.released(Btn::Middle) {
                sig |= Signal::RELEASED_MIDDLE
            }
        }

        if self.active_id == id {
            if self.mouse.dragging(Btn::Left) {
                sig |= Signal::DRAGGING_LEFT;
            }
            if self.mouse.dragging(Btn::Right) {
                sig |= Signal::DRAGGING_RIGHT;
            }
            if self.mouse.dragging(Btn::Middle) {
                sig |= Signal::DRAGGING_MIDDLE;
            }
        }


        sig
    }

    pub fn get_root_panel(&self) -> &Panel {
        &self.panels[self.root_panel_id]
    }

    pub fn get_active_panel(&self) -> Option<&Panel> {
        if self.active_panel_id.is_null() {
            None
        } else {
            Some(&self.panels[self.active_panel_id])
        }
    }

    pub fn get_hot_panel(&self) -> Option<&Panel> {
        if self.hot_panel_id.is_null() {
            None
        } else {
            Some(&self.panels[self.hot_panel_id])
        }
    }

    pub fn get_current_panel(&self) -> &Panel {
        &self.panels[self.current_panel_id]
    }

    pub fn move_down(&self, offset: f32) {
        self.move_cursor(Vec2::new(0.0, offset))
    }

    pub fn move_up(&self, offset: f32) {
        self.move_cursor(Vec2::new(0.0, -offset))
    }

    pub fn move_right(&self, offset: f32) {
        self.move_cursor(Vec2::new(offset, 0.0))
    }

    pub fn move_left(&self, offset: f32) {
        self.move_cursor(Vec2::new(-offset, 0.0))
    }

    pub fn move_cursor(&self, offset: Vec2) {
        let mut c = self.get_current_panel().cursor.borrow_mut();
        c.pos += offset;
        c.max_pos = c.max_pos.max(c.pos);
    }

    pub fn cursor_pos(&self) -> Vec2 {
        self.get_current_panel().cursor_pos()
    }

    pub fn cursor_max_pos(&self) -> Vec2 {
        self.get_current_panel().cursor_max_pos()
    }

    pub fn content_start_pos(&self) -> Vec2 {
        self.get_current_panel().content_start_pos()
    }

    pub fn set_cursor_pos(&self, pos: Vec2) {
        self.get_current_panel().set_cursor_pos(pos)
    }

    pub fn same_line(&self) {
        let p = self.get_current_panel();
        let mut c = p.cursor.borrow_mut();
        c.is_same_line = true;
        c.line_height = c.prev_line_height;
        c.pos = c.pos_prev_line + Vec2::new(self.style.spacing_h(), 0.0);
    }

    pub fn available_content(&self) -> Vec2 {
            // ImGuiContext& g = *GImGui;
    // ImGuiWindow* window = g.CurrentWindow;
    // ImVec2 mx = (window->DC.CurrentColumns || g.CurrentTable) ? window->WorkRect.Max : window->ContentRegionRect.Max;
    // return mx - window->DC.CursorPos;
    // 
        let p = self.get_current_panel();
        (p.content_rect().max - p.cursor_pos()).max(Vec2::ZERO)

    }

    // based on: https://github.com/ocornut/imgui/blob/3dafd9e898290ca890c29a379188be9e53b88537/imgui.cpp#L11183
    // TODO[NOTE]: what do we do with layout? now that we have same_line
    pub fn place_item(&mut self, id: Id, size: Vec2) -> Rect {
        let p = self.get_current_panel();
        let rect = Rect::from_min_size(p.cursor_pos(), size);
        let clip_rect = p.current_clip_rect();

        let mut c = p.cursor.borrow_mut();

        let line_y1 = if c.is_same_line {
            c.pos_prev_line.y
        } else {
            c.pos.y
        };
        let line_height = c.line_height.max(c.pos.y - line_y1 + size.y);

        c.pos_prev_line.x = c.pos.x + size.x;
        c.pos_prev_line.y = line_y1;

        c.pos.x = (p.pos.x + p.padding).round();
        c.pos.y = line_y1 + line_height + self.style.spacing_v();

        c.max_pos.x = c.max_pos.x.max(c.pos_prev_line.x);
        c.max_pos.y = c.max_pos.y.max(c.pos.y - self.style.spacing_v());

        c.prev_line_height = line_height;
        c.line_height = 0.0;
        c.is_same_line = false;
        drop(c);

        self.prev_item_data.reset();
        self.prev_item_data.id = id;
        self.prev_item_data.rect = rect;


        let Some(crect) = rect.clip(clip_rect) else {
            self.prev_item_data.is_hidden = true;
            return rect;
        };

        self.prev_item_data.clip_rect = crect;
        self.prev_item_data.is_clipped = !clip_rect.contains_rect(rect);

        if self.draw_item_outline {
            self.draw(|list| {
                list.add_rect_outline(rect.min, rect.max, Outline::outer(RGBA::PASTEL_YELLOW, 1.5));
                if let Some(crect) = rect.clip(clip_rect) {
                    list.add_rect_outline(crect.min, crect.max, Outline::outer(RGBA::YELLOW, 1.5));
                }
            });
        }



        rect
    }

    pub fn register_rect(&mut self, id: Id, rect: Rect) -> Signal {
        let p = &self.panels[self.current_panel_id];
        let clip_rect = p.current_clip_rect();

        if rect.contains(self.mouse.pos) {
            if self.prev_hot_panel_id == p.id || self.prev_hot_panel_id.is_null() {
                self.hot_id = id;
            }
        }

        self.get_item_signal(id, rect)
    }

    /// "registers" the item, i.e. potentially sets hot_id and returns the item signals
    ///
    /// assumes the item to be a rect at position of the cursor with given size
    pub fn register_item(&mut self, id: Id) -> Signal {
        assert!(self.prev_item_data.id == id);
        // let p = self.get_current_panel();
        if self.prev_item_data.is_hidden && self.active_id != id {
            return Signal::NONE
        }

        let clip_rect = self.prev_item_data.clip_rect;
        if clip_rect.contains(self.mouse.pos) {
            // let is_over = if let Some(hot) = self.get_hot_panel() {
            //     hot.draw_order > draw_order
            // } else {
            //     true
            // };
            // if is_over

            // TODO[CHECK]: is this correct?, maybe use draw order?
            // TODO[CHECK]: use prev_hot_panel_id because if we used hot_panel_id
            // we would potentially return multiple hovering signals per frame?
            // maybe instead use some prev_hot_id in get_item_signals?
            if self.prev_hot_panel_id == self.current_panel_id || self.prev_hot_panel_id.is_null() {
                self.hot_id = id;
            }
        }

        self.get_item_signal(id, clip_rect)
    }

    pub fn create_panel(&mut self, name: impl Into<String>) -> Id {
        let mut p = Panel::new(name);
        let id = p.id;
        p.frame_created = self.frame_count;
        self.panels.insert(id, p);
        id
    }

    pub fn get_panel_with_name(&self, name: &str) -> Option<&Panel> {
        let id = Id::from_str(name);
        if self.panels.contains_id(id) {
            Some(&self.panels[id])
        } else {
            None
        }
    }

    pub fn get_panel_id_with_name(&self, name: &str) -> Id {
        let id = Id::from_str(name);
        // if self.panels.contains_key(&id) {
        if self.panels.contains_id(id) {
            id
        } else {
            Id::NULL
        }
    }

    pub fn get_panel_name_with_id(&self, id: Id) -> Option<String> {
        if !id.is_null() {
            Some(self.panels[id].name.clone())
        } else {
            None
        }
    }

    // f(prev_size, full_size, content_size)
    pub fn set_current_panel_max_size(&mut self, f: impl Fn(Vec2, Vec2, Vec2) -> Vec2) {
        let p = &mut self.panels[self.current_panel_id];
        if p.explicit_size.is_some() {
            log::warn!("set_current_panel_max_size with also explicit size");
        }
        p.max_size = f(p.size, p.full_size, p.full_content_size);
    }

    pub fn set_current_panel_min_size(&mut self, f: impl Fn(Vec2, Vec2, Vec2) -> Vec2) {
        let p = &mut self.panels[self.current_panel_id];
        if p.explicit_size.is_some() {
            log::warn!("set_current_panel_min_size with also explicit size");
        }
        p.min_size = f(p.size, p.full_size, p.full_content_size);
    }

    pub fn bring_panel_to_front(&mut self, panel_id: Id) {
        assert_eq!(self.panels.len(), self.draw_order.len());

        let root_id = {
            let p = &self.panels[panel_id];
            p.root
        };

        let curr_order = self.panels[root_id].draw_order;
        assert!(self.draw_order[curr_order] == root_id);

        let new_order = self.draw_order.len() - 1;
        if self.draw_order[new_order] == root_id {
            return;
        }

        for i in curr_order..new_order {
            let moved = self.draw_order[i + 1];
            self.draw_order[i] = moved;
            self.panels[moved].draw_order = i;
            assert_eq!(self.panels[moved].draw_order, i);
        }

        self.draw_order[new_order] = root_id;
        self.panels[root_id].draw_order = new_order;
    }

    pub fn begin_frame(&mut self) {
        self.draw.clear();
        self.draw.screen_size = self.window.window_size();
        self.hot_panel_id = Id::NULL;
        self.hot_id = Id::NULL;

        if !self.mouse.pressed(MouseBtn::Left) {
            self.expect_drag = false;
        }

        // if !self.window.is_decorated() {
        self.next.pos = Some(Vec2::ZERO);
        let win_size = self.window.window_size();
        self.next.size = Some(win_size);
        // TODO
        // self.window
        match self.cursor_icon {
            CursorIcon::MoveH | CursorIcon::MoveV => self.set_cursor_icon(CursorIcon::Default),
            _ => (),
        }

        // NO_MOVE because the window panel dragging is handled by the window,
        // not the panel
        let mut flags = PanelFlags::NO_FOCUS | PanelFlags::NO_MOVE;

        if self.window.is_decorated() {
            flags |= PanelFlags::NO_TITLEBAR;
        } else {
            self.root_panel_titlebar_height = self.style.titlebar_height();
        }

        self.begin_ex("#ROOT_PANEL", flags);
        self.root_panel_id = self.current_panel_id;
        // }

        // let p_info: Vec<_> = self.panels.iter().map(|(_, p)| (p.name.clone(), p.draw_order)).collect();
        // println!("{:#?}", p_info);
        let root_panel = &mut self.panels[self.root_panel_id];
        root_panel.is_window_panel = true;
        if root_panel.close_pressed {
            self.close_pressed = true;
        }
    }

    pub fn push_id(&self, id: Id) {
        let p = &self.panels[self.current_panel_id];
        p.push_id(id)
    }

    pub fn pop_id(&self) {
        let p = &self.panels[self.current_panel_id];
        p.pop_id();
    }

    pub fn push_style(&mut self, var: StyleVar) {
        self.style.push_var(var);
    }

    pub fn set_style(&mut self, var: StyleVar) {
        self.style.set_var(var);
    }

    pub fn pop_style(&mut self) {
        self.style.pop_var();
    }

    pub fn debug_window(&mut self) {
        use crate::ui_items::ui_text;
        self.begin("#DEBUG");

        let hot_name = self.get_panel_name_with_id(self.prev_hot_panel_id);
        let active_name = self.get_panel_name_with_id(self.prev_active_panel_id);
        // let tmp = self.style.text_size();

        // self.style.text_size = 50.0;
        // self.push_style(StyleVar::TextSize(30.0));
        ui_text!(self: "hot: {hot_name:?}");
        ui_text!(self: "active: {active_name:?}");
        ui_text!(self: "hot item: {}", self.prev_hot_id);
        ui_text!(self: "active item: {}", self.prev_active_id);

        let now = Instant::now();
        let dt = (now - self.prev_frame_time).as_secs_f32();
        let fps = 1.0 / dt;
        self.prev_frame_time = now;
        ui_text!(self: "dt: {:0.3?}, fps: {fps:0.1?}", dt * 1000.0);

        // self.pop_style();

        ui_text!(self: "action: {}", self.panel_action);
        ui_text!(self: "n. of draw calls: {}", self.n_draw_calls);

        let mut tmp = self.draw_wireframe;
        self.checkbox("draw wireframe", &mut tmp);
        self.draw_wireframe = tmp;

        let mut tmp = self.draw_clip_rect;
        self.checkbox("draw clip rect", &mut tmp);
        self.draw_clip_rect = tmp;

        let mut tmp = self.draw_item_outline;
        self.checkbox("draw item outline", &mut tmp);
        self.draw_item_outline = tmp;

        if self.checkbox_intern("show font atlas") {
            let avail = self.available_content().min(Vec2::splat(800.0));
            let uv_min = self.glyph_cache.min_alloc_uv;
            let uv_max = self.glyph_cache.max_alloc_uv;
            let size = uv_max - uv_min;
            let scale = (avail.x / size.x).min(avail.y / size.y);
            let fitted_size = size * scale;
            self.image(fitted_size, self.glyph_cache.min_alloc_uv, self.glyph_cache.max_alloc_uv, 1);

        } 

        let mut v = self.circle_max_err;
        self.slider_f32("resolution", 0.0, 5.0, &mut v);
        self.circle_max_err = v;

        let mut v = self.style.spacing_h();
        self.slider_f32("spacing h", 0.0, 30.0, &mut v);
        self.style.set_var(StyleVar::SpacingH(v));

        let mut v = self.style.spacing_v();
        self.slider_f32("spacing v", 0.0, 30.0, &mut v);
        self.style.set_var(StyleVar::SpacingV(v));
        
        let mut v = self.style.line_height();
        self.slider_f32("line height", 0.0, 30.0, &mut v);
        self.style.set_var(StyleVar::LineHeight(v));

        let mut v = self.style.btn_roundness();
        self.slider_f32("button corners", 0.0, 1.0, &mut v);
        self.style.set_var(StyleVar::BtnRoundness(v));

        let mut v = self.style.panel_corner_radius();
        self.slider_f32("panel corners", 0.0, 100.0, &mut v);
        self.style.set_var(StyleVar::PanelCornerRadius(v));


        self.end();
    }

    pub fn end_frame(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.mouse.pressed(MouseBtn::Left)
            && !self.mouse.dragging(MouseBtn::Left) && !self.expect_drag
            && self.panel_action.is_none() && self.hot_id != self.active_id
        {
            let prev = self.active_id;
            self.active_id = self.hot_id;
            self.active_panel_id = self.hot_panel_id;

            if !self.active_panel_id.is_null() {
                self.bring_panel_to_front(self.active_panel_id);
            }
        }

        self.update_panel_resize();
        self.update_panel_move();

        self.prev_hot_panel_id = self.hot_panel_id;
        self.prev_active_panel_id = self.active_panel_id;
        self.prev_hot_id = self.hot_id;
        self.prev_active_id = self.active_id;

        self.end_assert(Some("#ROOT_PANEL"));

        if !self.draw_wireframe {
            self.build_draw_data();
        } else {
            self.build_dbg_draw_data();
        }
        self.n_draw_calls = self.draw.call_list.len();

        self.frame_count += 1;
        self.mouse.end_frame();
        // self.prev_item_data.reset();

        if let PanelAction::Resize { dir, .. } = self.panel_action {
            self.set_cursor_icon(dir.as_cursor())
        }
        self.update_cursor_icon();

        if self.ext_window.is_none() && !self.requested_windows.is_empty() {
            let (size, pos) = self.requested_windows.last().unwrap();
            let winit_window = event_loop
                .create_window(winit::window::WindowAttributes::default())
                .unwrap();
            let mut window =
                Window::new(winit_window, size.x as u32, size.y as u32, &self.draw.wgpu);
            window.set_window_size(size.x as u32, size.y as u32);
            window.set_window_pos(*pos);
            self.ext_window = Some(window);
        }
    }

    pub fn shape_text(&mut self, text: &str, font_size: f32) -> ShapedText {
        let itm = TextItem::new(text.into(), font_size, 1.0, "Rubik");
        let shaped_text = if !self.text_item_cache.contains_key(&itm) {
            let shaped_text = shape_text_item(
                itm.clone(),
                &mut self.font_table,
                &mut self.glyph_cache,
                &self.draw.wgpu,
            );
            self.text_item_cache.entry(itm).or_insert(shaped_text)
        } else {
            self.text_item_cache.get(&itm).unwrap()
        };
        shaped_text.clone()
    }

    pub fn draw_text(&mut self, text: &str, pos: Vec2) {
        let shape = self.shape_text(text, 32.0);
        let p = self.get_current_panel();

        for g in shape.glyphs.iter() {
            let min = g.meta.pos + pos;
            let max = min + g.meta.size;
            let uv_min = g.meta.uv_min;
            let uv_max = g.meta.uv_max;

            p.with_draw_list(|list| list.rect(min, max).texture_uv(uv_min, uv_max, 1).add())
        }
    }

    pub fn upload_draw_data(&mut self) {
        let draw_buff = &mut self.draw.call_list;
        if draw_buff.vtx_alloc.len() * std::mem::size_of::<Vertex>() > self.draw.gpu_vertices.size() as usize {
            self.draw.gpu_vertices = self.draw.wgpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("draw_list_vertex_buffer"),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
                contents: bytemuck::cast_slice(&draw_buff.vtx_alloc),
            });
        } else {
            self.draw.wgpu.queue
                .write_buffer(&self.draw.gpu_vertices, 0, bytemuck::cast_slice(&draw_buff.vtx_alloc));
        }

        if self.draw.call_list.idx_alloc.len() * std::mem::size_of::<u32>() > self.draw.gpu_indices.size() as usize {
            self.draw.gpu_indices = self.draw.wgpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("draw_list_index_buffer"),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::INDEX,
                contents: bytemuck::cast_slice(&self.draw.call_list.idx_alloc),
            });
        } else {
            self.draw.wgpu.queue
                .write_buffer(&self.draw.gpu_indices, 0, bytemuck::cast_slice(&self.draw.call_list.idx_alloc));
        }
    }

    pub fn build_draw_data(&mut self) {
        let panels = &self.panels;
        let draw_buff = &mut self.draw.call_list;
        draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));

        for &id in &self.draw_order {
            let name = self.panels[id].name.clone();
            let draw_list = self.panels[id].draw_list_ref();
            // println!("{} draw_list:\n{:#?}", self.panels[id].name, draw_list);
            for cmd in &draw_list.cmd_buffer {
                let vtx = &draw_list.vtx_buffer[cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count];
                let idx = &draw_list.idx_buffer[cmd.idx_offset..cmd.idx_offset + cmd.idx_count];

                let mut curr_clip = draw_buff.current_clip_rect();
                curr_clip.min = curr_clip.min.max(Vec2::ZERO);
                curr_clip.max = curr_clip.max.min(self.draw.screen_size);

                let mut clip = cmd.clip_rect;
                clip.min = clip.min.max(Vec2::ZERO);
                clip.max = clip.max.min(self.draw.screen_size);

                if cmd.clip_rect_used {
                    draw_buff.set_clip_rect(cmd.clip_rect);
                } else if  !draw_buff.current_clip_rect().contains_rect(clip) {
                    draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));
                }

                draw_buff.push(vtx, idx);
            }
        }

        self.upload_draw_data();
        // println!("final:\n{:#?}", self.draw.call_list);
        // println!("------------------------------------------");
    }

    pub fn build_dbg_draw_data(&mut self) {
        let panels = &self.panels;
        let draw_buff = &mut self.draw.call_list;
        draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));

        for &id in &self.draw_order {
            let draw_list = self.panels[id].draw_list_ref();
            for cmd in &draw_list.cmd_buffer {
                let vtx = &draw_list.vtx_buffer[cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count];
                let idx = &draw_list.idx_buffer[cmd.idx_offset..cmd.idx_offset + cmd.idx_count];

                for i in idx.chunks_exact(3) {
                    let v0 = vtx[i[0] as usize];
                    let v1 = vtx[i[1] as usize];
                    let v2 = vtx[i[2] as usize];
                    let cols = [v0.col, v1.col, v2.col, v0.col];
                    let path = [v0.pos, v1.pos, v2.pos, v0.pos];

                    let (mut vtx, idx) = tessellate_line(&path, cols[0], 1.5, true);
                    vtx.iter_mut().enumerate().for_each(|(i, v)| {
                        v.col = cols[i % cols.len()];
                    });

                    // draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));
                    draw_buff.push(&vtx, &idx);
                }
            }
        }
        self.upload_draw_data();
    }


}




#[derive(Clone)]
pub struct Panel {
    pub name: String,
    pub id: Id,
    pub move_id: Id,
    // TODO[NOTE]: implement
    pub close_id: Id,
    pub flags: PanelFlags,

    pub root: Id,

    pub padding: f32,

    /// pos of the panel at draw time
    ///
    /// preserved over frames
    pub pos: Vec2,

    /// size of the panel at draw time
    ///
    /// preserved over frames
    pub size: Vec2,

    /// full size of the panel, i.e. from top left to bottom right corner, including the titlebar
    ///
    pub full_size: Vec2,

    /// size of the content of a panel
    ///
    /// computed based on cursor.content_start_pos and cursor.max_pos
    pub full_content_size: Vec2,

    pub explicit_size: Option<Vec2>,

    pub outline_offset: f32,
    
    pub min_size: Vec2,
    pub max_size: Vec2,

    pub titlebar_height: f32,
    pub layout: Layout,

    pub draw_order: usize,

    pub last_frame_used: u64,
    pub frame_created: u64,
    pub close_pressed: bool,
    pub is_window_panel: bool,

    // try to not borrow outside of impl Panel { ... }
    pub draw_list: RefCell<DrawList>,
    pub id_stack: RefCell<Vec<Id>>,
    pub cursor: RefCell<Cursor>,
}

impl fmt::Debug for Panel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Panel")
            .field("name", &self.name)
            .field("id", &format!("{}", self.id))
            .field("order", &self.draw_order)
            .field("pos", &self.pos)
            .field("size", &self.size)
            .field("full_size", &self.size)
            .field("content_size", &self.size)
            .field("exeplicit_size", &self.explicit_size)
            .field("min_size", &self.min_size)
            .field("max_size", &self.max_size)
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl Panel {
    pub fn new(name: impl Into<String>) -> Self {
        let name: String = name.into();
        let id = Id::from_str(&name);
        Self {
            name,
            id,
            root: Id::NULL,
            flags: PanelFlags::NONE,
            padding: 0.0,
            // spacing: 10.0,
            pos: Vec2::splat(30.0),

            full_content_size: Vec2::ZERO,
            full_size: Vec2::ZERO,
            explicit_size: None,
            outline_offset: 0.0,
            draw_order: 0,
            // bg_color: RGBA::ZERO,
            titlebar_height: 0.0,
            move_id: Id::NULL,
            close_id: Id::NULL,
            size: Vec2::ZERO,
            min_size: Vec2::ZERO,
            max_size: Vec2::ZERO,
            frame_created: 0,
            last_frame_used: 0,
            // draw_list: DrawList::new(),
            // id_stack: Vec::new(),
            close_pressed: false,
            layout: Layout::Vertical,
            is_window_panel: false,

            draw_list: RefCell::new(DrawList::new()),
            id_stack: RefCell::new(Vec::new()),
            cursor: RefCell::new(Cursor::default()),
        }
    }

    pub fn min_panel_size(&self) -> Vec2 {
        let pad = 2.0 * self.padding;
        Vec2::new(pad, self.titlebar_height + pad).max(self.min_size)
    }

    pub fn max_panel_size(&self) -> Vec2 {
        self.max_size
    }

    pub fn panel_rect(&self) -> Rect {
        let off = Vec2::splat(self.outline_offset).ceil();
        Rect::from_min_max(self.pos - off, self.pos + self.size + off)
    }

    pub fn content_rect(&self) -> Rect {
        let max = self.pos + self.size - Vec2::splat(self.padding);
        Rect::from_min_max(self.content_start_pos(), max)
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.draw_list.borrow().current_clip_rect()
    }

    pub fn push_id(&self, id: Id) {
        self.id_stack.borrow_mut().push(id);
    }

    pub fn pop_id(&self) -> Id {
        self.id_stack.borrow_mut().pop().unwrap()
    }

    pub fn gen_id(&self, label: impl hash::Hash) -> Id {
        use std::hash::{Hash, Hasher};
        let ids = &self.id_stack.borrow();
        let seed = ids.last().expect("at least self.id should be in the stack");
        let mut hasher = ahash::AHasher::default();
        seed.hash(&mut hasher);
        label.hash(&mut hasher);
        Id(hasher.finish().max(1))
    }

    pub fn clear_temp_data(&mut self) {
        self.draw_list.get_mut().clear();
        self.root = Id::NULL;
    }

    pub fn id_stack_ref(&self) -> Ref<'_, Vec<Id>> {
        self.id_stack.borrow()
    }
    // pub fn id_stack_len(&self) -> usize {
    //     self.mut_panel_data.id_stack.borrow().len()
    // }

    pub fn draw_list_ref(&self) -> std::cell::Ref<'_, DrawList> {
        self.draw_list.borrow()
    }

    pub fn set_cursor_pos(&self, pos: Vec2) {
        self.cursor.borrow_mut().pos = pos;
    }

    pub fn init_content_cursor(&self, pos: Vec2) {
        let mut c = self.cursor.borrow_mut();
        c.content_start_pos = pos;
        c.pos = pos;
        c.max_pos = pos;
    }

    pub fn set_cursor_max_pos(&self, pos: Vec2) {
        self.cursor.borrow_mut().max_pos = pos;
    }

    pub fn cursor_reserve_size(&self, size: Vec2) {
        let mut c = self.cursor.borrow_mut();
        c.max_pos = c.max_pos.max(c.pos + size);
    }

    pub fn cursor_pos(&self) -> Vec2 {
        self.cursor.borrow().pos
    }

    pub fn cursor_max_pos(&self) -> Vec2 {
        self.cursor.borrow().max_pos
    }

    pub fn content_start_pos(&self) -> Vec2 {
        self.cursor.borrow().content_start_pos
    }

    // TODO[CHECK]: when / how / what does this exactly do
    /// sets the new panel position
    ///
    /// will also update the cursor so we dont get items lagging behind
    pub fn move_panel_to(&mut self, pos: Vec2) {
        let mut c = self.cursor.get_mut();
        let prev_pos = self.pos;
        self.pos = pos;

        let pos_d = c.pos - prev_pos;
        let max_pos_d = c.max_pos - prev_pos;
        let content_start_pos_d = c.content_start_pos - prev_pos;

        c.pos = pos_d + pos;
        c.max_pos = max_pos_d + pos;
        c.content_start_pos = content_start_pos_d + pos;
    }

    pub fn with_draw_list(&self, draw_fn: impl FnOnce(&mut DrawList)) {
        let draw_list = &mut self.draw_list.borrow_mut();
        draw_fn(draw_list);
    }
}

// BEGIN TYPES
//---------------------------------------------------------------------------------------

id_type!(Id);
id_type!(TextureId);

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub pos: Vec2,
    pub max_pos: Vec2,
    pub content_start_pos: Vec2,
    pub pos_prev_line: Vec2,
    pub line_height: f32,
    pub prev_line_height: f32,
    pub is_same_line: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub width: f32,
    pub place: OutlinePlacement,
    pub col: RGBA,
}


#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum OutlinePlacement {
    Outer,
    #[default]
    Center,
    Inner,
}

impl Outline {
    pub fn new(col: RGBA, width: f32) -> Self {
        Self {
            width,
            col,
            place: OutlinePlacement::default(),
        }
    }

    pub fn offset(&self) -> f32 {
        match self.place {
            OutlinePlacement::Outer => self.width,
            OutlinePlacement::Center => self.width / 2.0,
            OutlinePlacement::Inner => 0.0,
        }
    }

    pub fn outer(col: RGBA, width: f32) -> Self {
        Self::new(col, width).with_place(OutlinePlacement::Outer)
    }

    pub fn inner(col: RGBA, width: f32) -> Self {
        Self::new(col, width).with_place(OutlinePlacement::Inner)
    }

    pub fn center(col: RGBA, width: f32) -> Self {
        Self::new(col, width).with_place(OutlinePlacement::Center)
    }

    pub fn none() -> Self {
        Self::new(RGBA::ZERO, 0.0)
    }

    pub fn with_place(mut self, place: OutlinePlacement) -> Self {
        self.place = place;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRadii {
    tl: f32,
    tr: f32,
    bl: f32,
    br: f32,
}

impl CornerRadii {
    pub fn new(tl: f32, tr: f32, bl: f32, br: f32) -> Self {
        Self { tl, tr, bl, br }
    }

    pub fn all(r: f32) -> Self {
        Self::new(r, r, r, r)
    }

    pub fn zero() -> Self {
        Self::all(0.0)
    }

    pub fn top(r: f32) -> Self {
        Self::new(r, r, 0.0, 0.0)
    }

    pub fn bottom(r: f32) -> Self {
        Self::new(0.0, 0.0, r, r)
    }

    pub fn any_round_corners(&self) -> bool {
        !(self.tl == 0.0 && self.tr == 0.0 && self.bl == 0.0 && self.br == 0.0)
    }
}

stacked_fields_struct!(Style {
    titlebar_color: RGBA,
    titlebar_height: f32,

    line_height: f32,
    text_size: f32,
    text_col: RGBA,

    btn_roundness: f32,

    btn_default: RGBA,
    btn_hover: RGBA,
    btn_press: RGBA,
    btn_press_text: RGBA,

    window_bg: RGBA,

    panel_bg: RGBA,
    panel_corner_radius: f32,
    panel_outline: Outline,
    panel_hover_outline: Outline,
    panel_padding: f32,

    spacing_h: f32,
    spacing_v: f32,

    red: RGBA,
});

impl StyleTable {
    pub fn btn_corner_radius(&self) -> f32 {
        self.btn_roundness() * self.line_height()
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct NextPanelData {
    pub pos: Option<Vec2>,
    pub placement: PanelPlacement,
    pub layout: Layout,
    pub size: Option<Vec2>,
    pub min_size: Vec2,
    pub max_size: Vec2,
    pub content_size: Option<Vec2>,
}

impl Default for NextPanelData {
    fn default() -> Self {
        Self::new()
    }
}

impl NextPanelData {
    pub fn new() -> Self {
        Self {
            pos: None,
            placement: PanelPlacement::TopLeft,
            layout: Layout::Vertical,
            size: None,
            // set both to infinity as default
            min_size: Vec2::ZERO,
            max_size: Vec2::INFINITY,
            content_size: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrevItemData {
    pub id: Id,
    pub rect: Rect,
    pub clip_rect: Rect,
    pub is_clipped: bool,
    pub is_hidden: bool,
    pub is_active: bool,
}

impl PrevItemData {
    pub fn new() -> Self {
        Self {
            id: Id::NULL,
            rect: Rect::ZERO,
            clip_rect: Rect::ZERO,
            is_clipped: false,
            is_hidden: false,
            is_active: false,
        }
    }

    pub fn reset(&mut self) {
        *self = PrevItemData::new()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layout {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PanelPlacement {
    #[default]
    TopLeft,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelAction {
    Resize { dir: Dir, id: Id, prev_rect: Rect },
    Move { start_pos: Vec2, id: Id },
    None,
}

impl fmt::Display for PanelAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resize { dir, id, prev_rect } => {
                write!(f, "RESIZE[{dir:?}] {{ {id}, {prev_rect} }}")
            }
            Self::Move { start_pos, id } => write!(f, "MOVE {{ {id}, {start_pos} }}"),

            Self::None => write!(f, "NONE"),
        }
    }
}

impl PanelAction {
    pub fn is_none(&self) -> bool {
        match self {
            Self::None => true,
            _ => false,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct PanelMap {
    map: HashMap<Id, Panel>,
}

impl PanelMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn contains_id(&self, id: Id) -> bool {
        if id.is_null() {
            return false;
        }
        self.map.contains_key(&id)
    }

    pub fn get(&self, id: Id) -> Option<&Panel> {
        if id.is_null() {
            return None;
        }
        self.map.get(&id)
    }

    pub fn get_mut(&mut self, id: Id) -> Option<&mut Panel> {
        if id.is_null() {
            return None;
        }
        self.map.get_mut(&id)
    }

    // pub fn hot(&self) -> Option<&Panel> {
    //     self.get(self.hot_id)
    // }

    // pub fn active(&self) -> Option<&Panel> {
    //     self.get(self.active_id)
    // }

    // pub fn current(&self) -> &Panel {
    //     self.get(self.current_id).unwrap()
    // }

    pub fn insert(&mut self, id: Id, panel: Panel) {
        assert!(!id.is_null());
        self.map.insert(id, panel);
    }
}

impl<'a> IntoIterator for &'a PanelMap {
    type Item = (&'a Id, &'a Panel);
    type IntoIter = std::collections::hash_map::Iter<'a, Id, Panel>;
    fn into_iter(self) -> Self::IntoIter {
        (&self.map).iter()
    }
}

impl std::ops::Index<Id> for PanelMap {
    type Output = Panel;

    fn index(&self, id: Id) -> &Self::Output {
        self.get(id).unwrap()
    }
}

impl std::ops::IndexMut<Id> for PanelMap {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        self.get_mut(id).unwrap()
    }
}

//---------------------------------------------------------------------------------------
// END TYPES

// BEGIN FLAGS
//---------------------------------------------------------------------------------------

macros::flags!(ItemFlags: MOVE_CURSOR_NO);
macros::flags!(PanelFlags: NO_TITLEBAR, NO_FOCUS, NO_MOVE, NO_RESIZE);

macros::flags!(
    Signal:

    PRESSED_LEFT,
    PRESSED_MIDDLE,
    PRESSED_RIGHT,
    PRESSED_KEYBOARD,

    DRAGGING_LEFT,
    DRAGGING_MIDDLE,
    DRAGGING_RIGHT,

    DOUBLE_DRAGGING_LEFT,
    DOUBLE_DRAGGING_MIDDLE,
    DOUBLE_DRAGGING_RIGHT,

    RELEASED_LEFT,
    RELEASED_MIDDLE,
    RELEASED_RIGHT,

    CLICKED_LEFT,
    CLICKED_MIDDLE,
    CLICKED_RIGHT,

    DOUBLE_CLICKED_LEFT,
    DOUBLE_CLICKED_MIDDLE,
    DOUBLE_CLICKED_RIGHT,

    MOUSE_OVER,
    HOVERING |= MOUSE_OVER,
);

macro_rules! sig_fn {
    ($fn_name:ident => $($x:ident),*) => {
        impl Signal {
            pub const fn $fn_name(&self) -> bool {
                // let flag = Signals::from_bits($x).unwrap();
                $(self.contains(Signal::$x) || )* false
            }
        }
    }
}

sig_fn!(hovering => HOVERING);
sig_fn!(mouse_over => MOUSE_OVER);
sig_fn!(pressed => PRESSED_LEFT, PRESSED_KEYBOARD);
sig_fn!(clicked => CLICKED_LEFT, PRESSED_KEYBOARD);
sig_fn!(double_clicked => DOUBLE_CLICKED_LEFT);
sig_fn!(dragging => DRAGGING_LEFT);
sig_fn!(released => RELEASED_LEFT);

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::NONE {
            return write!(f, "NONE");
        }

        let names = self
            .iter_names()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>();
        write!(f, "{}", names.join("|"))
    }
}

//---------------------------------------------------------------------------------------
// END FLAGS

// BEGIN DRAW LIST
//---------------------------------------------------------------------------------------

/// A single draw command
#[derive(Debug, Clone, Copy)]
pub struct DrawCmd {
    pub texture_id: u32,
    pub vtx_offset: usize,
    pub vtx_count: usize,
    pub idx_offset: usize,
    pub idx_count: usize,

    pub clip_rect: Rect,
    pub clip_rect_used: bool,
}

impl Default for DrawCmd {
    fn default() -> Self {
        Self {
            texture_id: 0,
            vtx_offset: 0,
            vtx_count: 0,
            idx_offset: 0,
            idx_count: 0,
            clip_rect: Rect::ZERO,
            clip_rect_used: false,
        }
    }
}

/// The draw list itself: holds geometry and draw commands
#[derive(Clone)]
pub struct DrawList {
    pub vtx_buffer: Vec<Vertex>,
    pub idx_buffer: Vec<u32>,
    pub cmd_buffer: Vec<DrawCmd>,

    pub resolution: f32,
    pub path: Vec<Vec2>,
    pub clip_stack: Vec<Rect>,

    pub circle_max_err: f32,
}

impl fmt::Debug for DrawList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrawList")
            .field("vtx_buffer_size", &self.vtx_buffer.len())
            .field("idx_buffer_size", &self.idx_buffer.len())
            .field("cmd_buffer", &self.cmd_buffer)
            .field("resolution", &self.resolution)
            .field("path", &self.path)
            .finish()
    }
}

impl Default for DrawList {
    fn default() -> Self {

        Self {
            vtx_buffer: vec![],
            idx_buffer: vec![],
            cmd_buffer: vec![],
            resolution: 20.0,
            path: vec![],
            clip_stack: vec![],

            circle_max_err: 0.3,
        }
    }
}


fn calc_circle_segment_count(rad: f32, max_err: f32) -> u8 {
    use std::f32::consts::PI;
    let tmp = (PI / (1.0 - rad.min(max_err) / rad).cos()).ceil() as u32;
    tmp.clamp(4, 512) as u8
}


impl DrawList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.vtx_buffer.clear();
        self.idx_buffer.clear();
        self.cmd_buffer.clear();
        self.path.clear();
        self.clip_stack.clear();
    }

    fn calc_circle_segment_count(&self, radius: f32) -> u8 {
        calc_circle_segment_count(radius, self.circle_max_err)
    }

    pub fn set_clip_rect(&mut self, rect: Rect) {
        if rect == Rect::ZERO {
            log::warn!("zero clip rect set");
        }
        let cmd = self.current_draw_cmd();
        if cmd.clip_rect == Rect::ZERO {
            cmd.clip_rect = rect;
        } else if cmd.clip_rect != rect {
            let cmd = self.push_draw_cmd();
            cmd.clip_rect = rect;
        }
    }

    // TODO[NOTE]: during drawing try to clip on cpu side. if all was clipped manually we dont need
    // to add another render pass
    pub fn push_clip_rect(&mut self, rect: Rect) {
        self.clip_stack.push(rect);
        self.set_clip_rect(rect);
    }

    pub fn pop_clip_rect(&mut self) -> Rect {
        let rect = self.clip_stack.pop().unwrap();
        self.set_clip_rect(rect);
        rect
    }

    pub fn current_draw_cmd(&mut self) -> &mut DrawCmd {
        if self.cmd_buffer.is_empty() {
            self.cmd_buffer.push(DrawCmd::default())
        }
        self.cmd_buffer.last_mut().unwrap()
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.clip_stack.last().copied().unwrap_or(Rect::INFINITY)
    }

    pub fn push_draw_cmd(&mut self) -> &mut DrawCmd {
        self.cmd_buffer.push(DrawCmd::default());
        let cmd = self.cmd_buffer.last_mut().unwrap();
        cmd.vtx_offset = self.vtx_buffer.len();
        cmd.idx_offset = self.idx_buffer.len();
        cmd
    }

    pub fn push_texture(&mut self, tex_id: u32) {
        if tex_id == 0 {
            return;
        }
        let cmd = self.current_draw_cmd();
        let prev_clip = cmd.clip_rect;
        // TODO[CHECK]: is this valid?
        // if cmd.texture_id == 0 {
        //     cmd.texture_id = tex_id;
        // }

        if cmd.texture_id != tex_id {
            let cmd = self.push_draw_cmd();
            cmd.texture_id = tex_id;
            cmd.clip_rect = prev_clip;
        }
    }

    #[inline]
    pub fn push_vtx_idx(&mut self, vtx: &[Vertex], idx: &[u32]) {
        let cmd = self.current_draw_cmd();
        let base = cmd.vtx_count as u32;

        self.vtx_buffer.extend_from_slice(vtx);
        self.idx_buffer.extend(idx.into_iter().map(|i| base + i));

        let cmd = self.current_draw_cmd();
        cmd.vtx_count += vtx.len();
        cmd.idx_count += idx.len();
    }

    pub fn circle(&mut self, center: Vec2, radius: f32) -> DrawRect<'_> {
        let r = Vec2::splat(radius);
        let min = center - r;
        let max = center + r;

        DrawRect {
            draw_list: self,
            min,
            max,
            uv_min: Vec2::ZERO,
            uv_max: Vec2::ONE,
            texture_id: 0,
            fill: RGBA::ZERO,
            outline: Outline::none(),
            corners: CornerRadii::all(radius),
        }
    }

    pub fn rect(&mut self, min: Vec2, max: Vec2) -> DrawRect<'_> {
        DrawRect {
            draw_list: self,
            min,
            max,
            uv_min: Vec2::ZERO,
            uv_max: Vec2::ONE,
            texture_id: 0,
            fill: RGBA::ZERO,
            outline: Outline::none(),
            corners: CornerRadii::zero(),
        }
    }

    pub fn add_text(&mut self, pos: Vec2, text: &ShapedText, col: RGBA) {
        for g in text.glyphs.iter() {
            let min = g.meta.pos + pos;
            let max = min + g.meta.size;
            let uv_min = g.meta.uv_min;
            let uv_max = g.meta.uv_max;

            self.rect(min, max)
                .texture_uv(uv_min, uv_max, 1)
                .fill(col)
                .add()
        }
    }


    #[inline]
    pub fn push_clipped_vtx_idx(&mut self, vtx: &[Vertex], idx: &[u32]) {
        let cmd = self.current_draw_cmd();
        let base = cmd.vtx_count as u32;
        let clip = self.current_clip_rect();

        fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

        fn interp_vertex(a: &Vertex, b: &Vertex, t: f32) -> Vertex {
            let mut out = a.clone();
            out.pos.x = lerp(a.pos.x, b.pos.x, t);
            out.pos.y = lerp(a.pos.y, b.pos.y, t);
            out.uv.x  = lerp(a.uv.x,  b.uv.x,  t);
            out.uv.y  = lerp(a.uv.y,  b.uv.y,  t);
            out.col   = a.col.lerp(b.col, t);
            out
        }

        // Pre-allocate and reuse temporary buffers to avoid per-triangle allocations
        let tri_count = idx.len() / 3;
        let mut out_vtxs: Vec<Vertex> = Vec::with_capacity(tri_count * 6); // triangle clipped -> <= ~6 verts typically
        let mut out_idx: Vec<u32> = Vec::with_capacity(tri_count * 6);
        let mut poly: Vec<Vertex> = Vec::with_capacity(8);
        let mut tmp: Vec<Vertex> = Vec::with_capacity(8);

        for tri in idx.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            let v0 = vtx[i0].clone();
            let v1 = vtx[i1].clone();
            let v2 = vtx[i2].clone();

            // trivial reject
            if (v0.pos.x < clip.min.x && v1.pos.x < clip.min.x && v2.pos.x < clip.min.x)
                || (v0.pos.x > clip.max.x && v1.pos.x > clip.max.x && v2.pos.x > clip.max.x)
                    || (v0.pos.y < clip.min.y && v1.pos.y < clip.min.y && v2.pos.y < clip.min.y)
                    || (v0.pos.y > clip.max.y && v1.pos.y > clip.max.y && v2.pos.y > clip.max.y)
            {
                continue;
            }

            poly.clear();
            poly.push(v0);
            poly.push(v1);
            poly.push(v2);

            // Helper macro-like inline to clip one edge into tmp, then swap poly/tmp
            macro_rules! clip_edge {
                ($inside:expr, $intersect_t:expr) => {
                    tmp.clear();
                    if !poly.is_empty() {
                        for i in 0..poly.len() {
                            let a = &poly[i];
                            let b = &poly[(i + 1) % poly.len()];
                            let ina = $inside(a);
                            let inb = $inside(b);
                            if ina && inb {
                                tmp.push(b.clone());
                            } else if ina && !inb {
                                let t = $intersect_t(a, b);
                                tmp.push(interp_vertex(a, b, t));
                            } else if !ina && inb {
                                let t = $intersect_t(a, b);
                                tmp.push(interp_vertex(a, b, t));
                                tmp.push(b.clone());
                            }
                        }
                    }
                    std::mem::swap(&mut poly, &mut tmp);
                };
            }

            // left  : x >= clip.min.x
            clip_edge!(
                |p: &Vertex| p.pos.x >= clip.min.x,
                |a: &Vertex, b: &Vertex| {
                    let dx = b.pos.x - a.pos.x;
                    if dx.abs() < 1e-6 { 0.0 } else { (clip.min.x - a.pos.x) / dx }.clamp(0.0, 1.0)
                }
            );
            if poly.len() < 3 { continue; }

            // right : x <= clip.max.x
            clip_edge!(
                |p: &Vertex| p.pos.x <= clip.max.x,
                |a: &Vertex, b: &Vertex| {
                    let dx = b.pos.x - a.pos.x;
                    if dx.abs() < 1e-6 { 0.0 } else { (clip.max.x - a.pos.x) / dx }.clamp(0.0, 1.0)
                }
            );
            if poly.len() < 3 { continue; }

            // top   : y >= clip.min.y
            clip_edge!(
                |p: &Vertex| p.pos.y >= clip.min.y,
                |a: &Vertex, b: &Vertex| {
                    let dy = b.pos.y - a.pos.y;
                    if dy.abs() < 1e-6 { 0.0 } else { (clip.min.y - a.pos.y) / dy }.clamp(0.0, 1.0)
                }
            );
            if poly.len() < 3 { continue; }

            // bottom: y <= clip.max.y
            clip_edge!(
                |p: &Vertex| p.pos.y <= clip.max.y,
                |a: &Vertex, b: &Vertex| {
                    let dy = b.pos.y - a.pos.y;
                    if dy.abs() < 1e-6 { 0.0 } else { (clip.max.y - a.pos.y) / dy }.clamp(0.0, 1.0)
                }
            );
            if poly.len() < 3 { continue; }

            let start = out_vtxs.len() as u32;
            out_vtxs.extend_from_slice(&poly);

            let vcount = poly.len() as u32;
            // fan-triangulate the clipped polygon
            for i in 1..(vcount - 1) {
                out_idx.push(base + start + 0);
                out_idx.push(base + start + i);
                out_idx.push(base + start + (i + 1));
            }
        }

        if !out_vtxs.is_empty() {
            self.vtx_buffer.extend_from_slice(&out_vtxs);
        }
        if !out_idx.is_empty() {
            self.idx_buffer.extend_from_slice(&out_idx);
        }

        let cmd = self.current_draw_cmd();
        cmd.vtx_count += out_vtxs.len();
        cmd.idx_count += out_idx.len();
    }


    #[inline]
    pub fn push_clipped_vtx_idx2(&mut self, vtx: &[Vertex], idx: &[u32]) {
        let cmd = self.current_draw_cmd();
        let base = cmd.vtx_count as u32;
        let clip = self.current_clip_rect();

        let mut kept: Vec<u32> = Vec::with_capacity(idx.len());
        for tri in idx.chunks_exact(3) {
            let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let (v0, v1, v2) = (vtx[i0], vtx[i1], vtx[i2]);

            if (v0.pos.x < clip.min.x && v1.pos.x < clip.min.x && v2.pos.x < clip.min.x)
                || (v0.pos.x > clip.max.x && v1.pos.x > clip.max.x && v2.pos.x > clip.max.x)
                || (v0.pos.y < clip.min.y && v1.pos.y < clip.min.y && v2.pos.y < clip.min.y)
                || (v0.pos.y > clip.max.y && v1.pos.y > clip.max.y && v2.pos.y > clip.max.y)
            {
                continue;
            }

            kept.push(base + tri[0]);
            kept.push(base + tri[1]);
            kept.push(base + tri[2]);
        }

        self.vtx_buffer.extend_from_slice(vtx);
        if !kept.is_empty() {
            self.idx_buffer.extend_from_slice(&kept);
        }

        let cmd = self.current_draw_cmd();
        cmd.vtx_count += vtx.len();
        cmd.idx_count += kept.len();
    }

    pub fn add_rect_rounded(
        &mut self,
        mut min: Vec2,
        mut max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: u32,
        tint: RGBA,
        outline: Outline,
        corners: CornerRadii,
    ) {
        if !corners.any_round_corners() {
            return self.add_rect(min, max, uv_min, uv_max, tex_id, tint, outline);
        }

        // rounded case (unchanged)
        let offset = Vec2::splat(outline.offset());

        let clip = self.current_clip_rect();
        if !(clip.contains(min - offset) || clip.contains(max + offset)) {
            return;
        } else if !clip.contains(min - offset) || !clip.contains(max + offset) {
            self.current_draw_cmd().clip_rect_used = true;
        }

        self.push_texture(tex_id);

        // account for outline placement as original did
        if outline.width != 0.0 {
            let offset = match outline.place {
                OutlinePlacement::Center => 0.0,
                OutlinePlacement::Inner => -outline.width * 0.5,
                OutlinePlacement::Outer => outline.width * 0.5,
            };
            min -= Vec2::splat(offset);
            max += Vec2::splat(offset);
        }

        self.path_clear();
        self.path_rect(min, max, corners);

        let start = self.vtx_buffer.len();
        let (vtx, idx) = tessellate_convex_fill(&self.path, tint, true);
        self.push_vtx_idx(&vtx, &idx);
        let end = start + vtx.len();
        if tex_id != 0 {
            self.distribute_uvs(start, end, min, max, uv_min, uv_max, true, tex_id);
        }

        if outline.width != 0.0 {
            let (vtx_o, idx_o) = tessellate_line(&self.path, outline.col, outline.width, true);
            self.push_vtx_idx(&vtx_o, &idx_o);
        }

        self.path_clear();
    }

    fn push_rect_vertices(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        color: RGBA,
        tex_id: u32,
    ) {
        const QUAD_IDX: [u32; 6] = [0, 1, 2, 0, 2, 3];

        let vertices = [
            Vertex::new(
                Vec2::new(min.x, max.y),
                color,
                Vec2::new(uv_min.x, uv_max.y),
                tex_id,
            ),
            Vertex::new(max, color, uv_max, tex_id),
            Vertex::new(
                Vec2::new(max.x, min.y),
                color,
                Vec2::new(uv_max.x, uv_min.y),
                tex_id,
            ),
            Vertex::new(min, color, uv_min, tex_id),
        ];

        self.push_vtx_idx(&vertices, &QUAD_IDX);
    }

    pub fn path_clear(&mut self) {
        self.path.clear();
    }

    pub fn path_to(&mut self, p: Vec2) {
        self.path.push(p);
    }

    pub fn path_rect(&mut self, min: Vec2, max: Vec2, corners: CornerRadii) {
        const PI: f32 = std::f32::consts::PI;

        let r0 = corners.tl;
        let r1 = corners.tr;
        let r2 = corners.br;
        let r3 = corners.bl;
        self.path_to(Vec2::new(min.x + r0, min.y));

        self.path_to(Vec2::new(max.x - r1, min.y));
        if r1 > 0.0 {
            self.path_arc(Vec2::new(max.x - r1, min.y + r1), r1, PI / 2.0, -PI / 2.0);
        }

        self.path_to(Vec2::new(max.x, min.y + r1));
        self.path_to(Vec2::new(max.x, max.y - r2));
        if r2 > 0.0 {
            self.path_arc(Vec2::new(max.x - r2, max.y - r2), r2, 0.0, -PI / 2.0);
        }

        self.path_to(Vec2::new(max.x - r2, max.y));
        self.path_to(Vec2::new(min.x + r3, max.y));
        if r3 > 0.0 {
            self.path_arc(Vec2::new(min.x + r3, max.y - r3), r3, -PI / 2.0, -PI / 2.0);
        }

        self.path_to(Vec2::new(min.x, max.y - r3));
        self.path_to(Vec2::new(min.x, min.y + r0));
        if r0 > 0.0 {
            self.path_arc(Vec2::new(min.x + r0, min.y + r0), r0, PI, -PI / 2.0);
        }
    }

    pub fn path_arc(&mut self, center: Vec2, radius: f32, start_angle: f32, sweep_angle: f32) {
        if radius == 0.0 || sweep_angle == 0.0 {
            return;
        }

        // maximum angular step so chord length ≤ resolution
        let segments = self.calc_circle_segment_count(radius);

        let step = sweep_angle / segments as f32;

        for i in 0..=segments {
            let theta = start_angle + step * (i as f32);
            let p = Vec2::new(
                center.x + theta.cos() * radius,
                center.y - theta.sin() * radius,
            );
            self.path.push(p);
        }
    }

    pub fn distribute_uvs(
        &mut self,
        vert_start: usize,
        vert_end: usize,
        a: Vec2,
        b: Vec2,
        uv_a: Vec2,
        uv_b: Vec2,
        clamp: bool,
        tex_id: u32,
    ) {
        if vert_end <= vert_start || vert_end > self.vtx_buffer.len() {
            return;
        }

        let size = b - a;
        let uv_size = uv_b - uv_a;
        let scale = Vec2::new(
            if size.x != 0.0 {
                uv_size.x / size.x
            } else {
                0.0
            },
            if size.y != 0.0 {
                uv_size.y / size.y
            } else {
                0.0
            },
        );

        let (min_uv, max_uv) = if clamp {
            (uv_a.min(uv_b), uv_a.max(uv_b))
        } else {
            (uv_a, uv_b)
        };

        for vert in &mut self.vtx_buffer[vert_start..vert_end] {
            let mut uv = uv_a + (vert.pos - a) * scale;
            if clamp {
                uv.x = uv.x.clamp(min_uv.x, max_uv.x);
                uv.y = uv.y.clamp(min_uv.y, max_uv.y);
            }
            vert.uv = uv;
            vert.tex = tex_id;
        }
    }

    pub fn add_rect(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: u32,
        tint: RGBA,
        outline: Outline,
    ) {
        // Fast path: opaque solid fill with outline (no texture)
        if tex_id == 0 && tint.a == 1.0 && outline.width > 0.0 {
            self.add_solid_rect_with_outline(min, max, uv_min, uv_max, tint, outline);
            return;
        }

        self.add_simple_rect(min, max, uv_min, uv_max, tex_id, tint);

        if outline.width > 0.0 {
            let clip = self.current_clip_rect();
            if let Some(crect) = Rect::from_min_max(min, max).clip(clip) {
                self.add_rect_outline(crect.min, crect.max, outline);
            }
        }
    }

    fn add_solid_rect_with_outline(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tint: RGBA,
        outline: Outline,
    ) {
        let clip = self.current_clip_rect();

        // Draw outline background first
        let outset = outline.width * 0.5;
        let outline_min = min - Vec2::splat(outset);
        let outline_max = max + Vec2::splat(outset);

        if let Some(outline_clip) =
            Rect::from_min_max(outline_min, outline_max).clip(clip)
        {
            let outline_uvs = compute_proportional_uvs(
                outline_min,
                outline_max,
                outline_clip.min,
                outline_clip.max,
                uv_min,
                uv_max,
            );
            self.push_rect_vertices(
                outline_clip.min,
                outline_clip.max,
                outline_uvs.0,
                outline_uvs.1,
                outline.col,
                0,
            );
        }

        // Draw fill on top
        if let Some(fill_clip) = Rect::from_min_max(min, max).clip(clip) {
            let fill_uvs =
                compute_clipped_uvs(min, max, fill_clip.min, fill_clip.max, uv_min, uv_max);
            self.push_rect_vertices(
                fill_clip.min,
                fill_clip.max,
                fill_uvs.0,
                fill_uvs.1,
                tint,
                0,
            );
        }
    }

    fn add_simple_rect(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: u32,
        tint: RGBA,
    ) {
        let clip = self.current_clip_rect();
        let Some(crect) = Rect::from_min_max(min, max).clip(clip) else {
            return;
        };

        self.push_texture(tex_id);
        let clipped_uvs = compute_clipped_uvs(min, max, crect.min, crect.max, uv_min, uv_max);

        let start = self.vtx_buffer.len();
        self.push_rect_vertices(crect.min, crect.max, clipped_uvs.0, clipped_uvs.1, tint, tex_id);

        if tex_id != 0 {
            let end = start + 4;
            self.distribute_uvs(
                start,
                end,
                crect.min,
                crect.max,
                clipped_uvs.0,
                clipped_uvs.1,
                true,
                tex_id,
            );
        }
    }

    // TODO[NOTE]: add clip?
    // TODO[NOTE]: consider outline placement for clipping
    fn add_rect_outline(&mut self, min: Vec2, max: Vec2, outline: Outline) {
        let pts = [
            Vec2::new(min.x, max.y), // bottom-left
            max,                     // top-right
            Vec2::new(max.x, min.y), // top-left
            min,                     // bottom-right
        ];
        let (vtx, idx) = tessellate_line(&pts, outline.col, outline.width, true);
        self.push_vtx_idx(&vtx, &idx);
    }

}


fn compute_clipped_uvs(
    omin: Vec2,
    omax: Vec2,
    cmin: Vec2,
    cmax: Vec2,
    uv_min: Vec2,
    uv_max: Vec2,
) -> (Vec2, Vec2) {
    let orig_size = omax - omin;
    let clipped_offset = cmin - omin;
    let clipped_size = cmax - cmin;

    let mut result_uv_min = uv_min;
    let mut result_uv_max = uv_max;

    if orig_size.x != 0.0 {
        let start_ratio = clipped_offset.x / orig_size.x;
        let end_ratio = (clipped_offset.x + clipped_size.x) / orig_size.x;
        let uv_range = uv_max.x - uv_min.x;
        result_uv_min.x = uv_min.x + start_ratio * uv_range;
        result_uv_max.x = uv_min.x + end_ratio * uv_range;
    }

    if orig_size.y != 0.0 {
        let start_ratio = clipped_offset.y / orig_size.y;
        let end_ratio = (clipped_offset.y + clipped_size.y) / orig_size.y;
        let uv_range = uv_max.y - uv_min.y;
        result_uv_min.y = uv_min.y + start_ratio * uv_range;
        result_uv_max.y = uv_min.y + end_ratio * uv_range;
    }

    (result_uv_min, result_uv_max)
}

fn compute_proportional_uvs(
    orig_min: Vec2,
    orig_max: Vec2,
    target_min: Vec2,
    target_max: Vec2,
    uv_min: Vec2,
    uv_max: Vec2,
) -> (Vec2, Vec2) {
    let orig_size = orig_max - orig_min;
    let uv_size = uv_max - uv_min;

    if orig_size.x == 0.0 || orig_size.y == 0.0 {
        return (uv_min, uv_max);
    }

    let start_offset = target_min - orig_min;
    let end_offset = target_max - orig_min;

    let uv_start = Vec2::new(
        uv_min.x + (start_offset.x / orig_size.x) * uv_size.x,
        uv_min.y + (start_offset.y / orig_size.y) * uv_size.y,
    );

    let uv_end = Vec2::new(
        uv_min.x + (end_offset.x / orig_size.x) * uv_size.x,
        uv_min.y + (end_offset.y / orig_size.y) * uv_size.y,
    );

    (uv_start, uv_end)
}

pub fn tessellate_line(
    points: &[Vec2],
    col: RGBA,
    thickness: f32,
    closed: bool,
) -> (Vec<Vertex>, Vec<u32>) {
    if points.len() < 2 {
        return (Vec::new(), Vec::new());
    }

    let count = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let half = thickness * 0.5;

    let mut verts: Vec<Vertex> = Vec::with_capacity(count * 4);
    let mut idxs: Vec<u32> = Vec::with_capacity(count * 12);

    // First pass through just adds verts
    for i in 0..count {
        let i_next = if (i + 1) == points.len() { 0 } else { i + 1 };

        let p_curr = points[i];
        let p_next = points[i_next];

        let mut dx_next = p_next.x - p_curr.x;
        let mut dy_next = p_next.y - p_curr.y;
        let len_next = dx_next * dx_next + dy_next * dy_next;
        if len_next <= std::f32::EPSILON {
            // degenerate segment -> make a vertical fallback
            dx_next = 0.0;
            dy_next = 1.0;
        } else {
            let inv_len = 1.0 / len_next.sqrt();
            dx_next *= inv_len;
            dy_next *= inv_len;
        }

        // perpendicular (normalized) scaled by half thickness
        let px = dy_next * half;
        let py = -dx_next * half;

        // 4 verts for the rect, vert 0 and 1 are "above" and "below" the first point and vert 2 and 3 are "above" and "below" the second point
        verts.push(Vertex::color(Vec2::new(p_curr.x + px, p_curr.y + py), col));
        verts.push(Vertex::color(Vec2::new(p_curr.x - px, p_curr.y - py), col));
        verts.push(Vertex::color(Vec2::new(p_next.x + px, p_next.y + py), col));
        verts.push(Vertex::color(Vec2::new(p_next.x - px, p_next.y - py), col));
    }

    let mut base_idx_prev: u32 = 0;
    let mut base_idx_curr: u32 = 0;
    // Second passthrough draws triangles
    for i in 0..count {
        base_idx_prev = if i == 0 {
            ((points.len() - 1) * 4).try_into().unwrap()
        } else {
            ((i - 1) * 4).try_into().unwrap()
        };
        base_idx_curr = (i * 4).try_into().unwrap();

        // Connection triangles to previous one. For first only do it if closed is true
        if (i > 0) || closed {
            idxs.push(base_idx_prev + 2);
            idxs.push(base_idx_curr + 0);
            idxs.push(base_idx_prev + 3);
            idxs.push(base_idx_prev + 2);
            idxs.push(base_idx_curr + 1);
            idxs.push(base_idx_prev + 3);
        }
        // two triangles (0,2,3) and (0,3,1) relative to base_idx
        idxs.push(base_idx_curr + 0);
        idxs.push(base_idx_curr + 2);
        idxs.push(base_idx_curr + 3);
        idxs.push(base_idx_curr + 0);
        idxs.push(base_idx_curr + 3);
        idxs.push(base_idx_curr + 1);
    }

    (verts, idxs)
}

pub fn tessellate_convex_fill(
    points: &[Vec2],
    col: RGBA,
    antialias: bool,
) -> (Vec<Vertex>, Vec<u32>) {
    let n = points.len();
    if n < 3 {
        return (Vec::new(), Vec::new());
    }

    if !antialias {
        let mut verts = Vec::new();
        let mut idxs = Vec::new();
        // no-AA: just triangulate polygon fan
        for p in points {
            verts.push(Vertex::color(*p, col));
        }

        for i in 2..n {
            idxs.extend_from_slice(&[0, (i - 1) as u32, i as u32]);
        }
        return (verts, idxs);
    }

    const AA_SIZE: f32 = 1.0;
    const EPS: f32 = 1e-12;
    let col_trans = RGBA::rgba_f(col.r, col.g, col.b, 0.0);
    let mut verts = Vec::with_capacity(n * 2);
    let mut idxs = Vec::with_capacity((n - 2) * 3 + n * 6);

    // compute edge normals
    let mut temp_normals = vec![Vec2 { x: 0.0, y: 0.0 }; n];
    for i1 in 0..n {
        let i0 = (i1 + n - 1) % n;
        let p0 = &points[i0];
        let p1 = &points[i1];
        let mut dx = p1.x - p0.x;
        let mut dy = p1.y - p0.y;
        let d2 = dx * dx + dy * dy;
        if d2 > EPS {
            let inv_len = 1.0_f32 / d2.sqrt();
            dx *= inv_len;
            dy *= inv_len;
        } else {
            dx = 0.0;
            dy = 0.0;
        }
        temp_normals[i0] = Vec2 { x: dy, y: -dx };
    }

    for i1 in 0..n {
        let i0 = (i1 + n - 1) % n;
        let n0 = &temp_normals[i0];
        let n1 = &temp_normals[i1];

        let mut dm_x = (n0.x + n1.x) * 0.5;
        let mut dm_y = (n0.y + n1.y) * 0.5;
        let d2 = dm_x * dm_x + dm_y * dm_y;
        if d2 <= EPS {
            dm_x = 1.0;
            dm_y = 0.0;
        } else {
            let inv_len = 1.0_f32 / d2.sqrt();
            dm_x *= inv_len;
            dm_y *= inv_len;
        }
        dm_x *= AA_SIZE * 0.5;
        dm_y *= AA_SIZE * 0.5;

        let p = &points[i1];
        let inner = Vec2 {
            x: p.x - dm_x,
            y: p.y - dm_y,
        };
        let outer = Vec2 {
            x: p.x + dm_x,
            y: p.y + dm_y,
        };

        verts.push(Vertex::color(inner, col));
        verts.push(Vertex::color(outer, col_trans));
    }

    let base: u32 = 0;

    for i in 2..n {
        let a = base;
        let b = base + ((i - 1) as u32) * 2;
        let c = base + (i as u32) * 2;
        idxs.push(a);
        idxs.push(b);
        idxs.push(c);
    }

    for i1 in 0..n {
        let i0 = (i1 + n - 1) % n;
        let inner_i1 = base + (i1 as u32) * 2;
        let inner_i0 = base + (i0 as u32) * 2;
        let outer_i0 = inner_i0 + 1;
        let outer_i1 = inner_i1 + 1;

        idxs.push(inner_i1);
        idxs.push(inner_i0);
        idxs.push(outer_i0);

        idxs.push(outer_i0);
        idxs.push(outer_i1);
        idxs.push(inner_i1);
    }

    (verts, idxs)
}

pub fn is_in_resize_region(r: Rect, pnt: Vec2, thr: f32) -> Option<Dir> {
    let in_corner_region = |corner: Vec2| -> bool { corner.distance_squared(pnt) <= thr.powi(2) };

    if in_corner_region(r.right_top()) {
        Some(Dir::NE)
    } else if in_corner_region(r.right_bottom()) {
        Some(Dir::SE)
    } else if in_corner_region(r.left_bottom()) {
        Some(Dir::SW)
    } else if in_corner_region(r.left_top()) {
        Some(Dir::NW)
    } else {
        let top_y = r.left_top().y;
        let bottom_y = r.left_bottom().y;
        let left_x = r.left_top().x;
        let right_x = r.right_top().x;

        if (pnt.y - top_y).abs() <= thr && pnt.x >= left_x + thr && pnt.x <= right_x - thr {
            Some(Dir::N)
        } else if (pnt.y - bottom_y).abs() <= thr && pnt.x >= left_x + thr && pnt.x <= right_x - thr
        {
            Some(Dir::S)
        } else if (pnt.x - right_x).abs() <= thr && pnt.y >= top_y + thr && pnt.y <= bottom_y - thr
        {
            Some(Dir::E)
        } else if (pnt.x - left_x).abs() <= thr && pnt.y >= top_y + thr && pnt.y <= bottom_y - thr {
            Some(Dir::W)
        } else {
            None
        }
    }
}

pub struct DrawRect<'a> {
    pub draw_list: &'a mut DrawList,
    pub min: Vec2,
    pub max: Vec2,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub texture_id: u32,
    pub fill: RGBA,
    pub outline: Outline,
    pub corners: CornerRadii,
}

impl DrawRect<'_> {
    pub fn fill(mut self, fill: RGBA) -> Self {
        self.fill = fill;
        self
    }

    pub fn outline(mut self, outline: Outline) -> Self {
        self.outline = outline;
        self
    }

    pub fn texture_uv(mut self, uv_min: Vec2, uv_max: Vec2, id: u32) -> Self {
        self.uv_min = uv_min;
        self.uv_max = uv_max;
        self.texture_id = id;
        if self.fill.a == 0.0 {
            self.fill = RGBA::WHITE
        }
        self
    }

    pub fn texture(mut self, id: u32) -> Self {
        self.texture_id = id;
        if self.fill.a == 0.0 {
            self.fill = RGBA::WHITE
        }
        self
    }

    pub fn circle(mut self) -> Self {
        let width = self.max.x - self.min.x;
        let height = self.max.y - self.min.y;
        let rad = width.min(height) / 2.0;
        self.corners(CornerRadii::all(rad))
    }

    pub fn corners(mut self, corners: CornerRadii) -> Self {
        self.corners = corners;
        self
    }

    pub fn add(self) {
        self.draw_list.add_rect_rounded(
            self.min,
            self.max,
            self.uv_min,
            self.uv_max,
            self.texture_id,
            self.fill,
            self.outline,
            self.corners,
        )
    }
}

//---------------------------------------------------------------------------------------
// END DRAW LIST

//---------------------------------------------------------------------------------------
// BEGIN TEXT

pub type TextItemCache = HashMap<TextItem, ShapedText>;
pub type FontId = u64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMeta {
    pub pos: Vec2,
    pub size: Vec2,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub texture: gpu::Texture,
    pub meta: GlyphMeta,
}

#[derive(Debug, Clone)]
pub struct ShapedText {
    pub glyphs: Vec<Glyph>,
    pub width: f32,
    pub height: f32,
}

impl ShapedText {
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextItem {
    // pub font: FontId,
    pub font: &'static str,
    pub string: String,
    pub font_size_i: u64,
    pub line_height_i: u64,
    pub width_i: Option<u64>,
    pub height_i: Option<u64>,
}

pub struct FontTable {
    pub id_to_name: Vec<(FontId, String)>,
    pub sys: ctext::FontSystem,
}

pub struct GlyphCache {
    pub texture: gpu::Texture,
    pub alloc: etagere::AtlasAllocator,
    pub min_alloc_uv: Vec2,
    pub max_alloc_uv: Vec2,
    pub size: u32,
    pub cached_glyphs: HashMap<ctext::CacheKey, GlyphMeta>,
    pub swash_cache: ctext::SwashCache,
}

impl FontTable {
    pub fn new() -> Self {
        Self {
            id_to_name: Default::default(),
            sys: ctext::FontSystem::new(),
        }
    }
    // TODO[NOTE] remove font id?
    pub fn load_font(&mut self, name: &str, bytes: Vec<u8>) -> FontId {
        use hash::{Hash, Hasher};
        let db = self.sys.db_mut();
        let ids = db.load_font_source(ctext::fontdb::Source::Binary(std::sync::Arc::new(bytes)));
        let mut hasher = ahash::AHasher::default();
        ids.hash(&mut hasher);
        name.hash(&mut hasher);
        let id = hasher.finish();
        self.id_to_name.push((id, name.to_string()));
        id
    }

    pub fn get_font_attrib<'a>(&self, name: &'a str) -> ctext::Attrs<'a> {
        // let name = self.id_to_name.get(&id).unwrap();
        let attribs = ctext::Attrs::new().family(ctext::Family::Name(name));
        attribs
    }
}

fn shape_text_item(
    itm: TextItem,
    fonts: &mut FontTable,
    cache: &mut GlyphCache,
    wgpu: &WGPU,
) -> ShapedText {
    let mut buffer = ctext::Buffer::new(
        &mut fonts.sys,
        ctext::Metrics {
            font_size: itm.font_size(),
            line_height: itm.scaled_line_height(),
        },
    );

    let font_attrib = fonts.get_font_attrib(itm.font);
    buffer.set_size(&mut fonts.sys, itm.width(), itm.height());
    buffer.set_text(
        &mut fonts.sys,
        &itm.string,
        &font_attrib,
        ctext::Shaping::Advanced,
    );
    buffer.shape_until_scroll(&mut fonts.sys, false);

    let mut glyphs = Vec::new();
    let mut width = 0.0;
    let mut height = 0.0;

    for run in buffer.layout_runs() {
        width = run.line_w.max(width);
        // TODO[CHECK]: is it the sum?
        // height = run.line_height.max(height);
        height += run.line_height;

        for g in run.glyphs {
            let g_phys = g.physical((0.0, 0.0), 1.0);
            let mut key = g_phys.cache_key;
            // TODO[CHECK]: what does this do
            key.x_bin = ctext::SubpixelBin::Three;
            key.y_bin = ctext::SubpixelBin::Three;

            if let Some(mut glyph) = cache.get_glyph(key, fonts, wgpu) {
                glyph.meta.pos += Vec2::new(g_phys.x as f32, g_phys.y as f32 + run.line_y);
                glyphs.push(glyph);
            }
        }
    }

    let text = ShapedText {
        glyphs,
        width,
        height,
    };
    text
}

impl TextItem {
    pub const RESOLUTION: f32 = 1024.0;

    pub fn new(text: String, font_size: f32, line_height: f32, font: &'static str) -> Self {
        Self {
            font,
            string: text,
            font_size_i: (font_size * Self::RESOLUTION) as u64,
            line_height_i: (line_height * Self::RESOLUTION) as u64,
            width_i: None,
            height_i: None,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width_i = Some((width * Self::RESOLUTION) as u64);
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height_i = Some((height * Self::RESOLUTION) as u64);
        self
    }

    pub fn width(&self) -> Option<f32> {
        self.width_i.map(|w| w as f32 / Self::RESOLUTION)
    }

    pub fn height(&self) -> Option<f32> {
        self.height_i.map(|h| h as f32 / Self::RESOLUTION)
    }

    pub fn line_height(&self) -> f32 {
        self.line_height_i as f32 / Self::RESOLUTION
    }

    pub fn font_size(&self) -> f32 {
        self.font_size_i as f32 / Self::RESOLUTION
    }

    pub fn scaled_line_height(&self) -> f32 {
        self.line_height() * self.font_size()
    }
}

impl GlyphCache {
    pub fn new(wgpu: &WGPU) -> Self {
        const SIZE: u32 = 1024;
        let size = SIZE.min(wgpu.device.limits().max_texture_dimension_2d);

        let texture = wgpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_cache_texture"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let alloc = etagere::AtlasAllocator::new(etagere::Size::new(size as i32 + 3, size as i32 + 3));
        let texture = gpu::Texture::new(texture, texture_view);

        Self {
            texture,
            min_alloc_uv: Vec2::INFINITY,
            max_alloc_uv: Vec2::ZERO,
            alloc,
            size,
            cached_glyphs: Default::default(),
            swash_cache: ctext::SwashCache::new(),
        }
    }

    pub fn get_glyph(
        &mut self,
        glyph_key: ctext::CacheKey,
        fonts: &mut FontTable,
        wgpu: &WGPU,
    ) -> Option<Glyph> {
        if let Some(&meta) = self.cached_glyphs.get(&glyph_key) {
            return Some(Glyph {
                texture: self.texture.clone(),
                meta,
            });
        }

        self.alloc_new_glyph(glyph_key, fonts, wgpu)
    }

    pub fn alloc_rect(&mut self, w: u32, h: u32) -> Rect {
        let r = self
            .alloc
            .allocate(etagere::Size::new(w as i32, h as i32))
            .unwrap()
            .rectangle;

        let min = Vec2::new(r.min.x as f32, r.min.y as f32);
        let max = Vec2::new(r.max.x as f32, r.max.y as f32);

        self.min_alloc_uv = self.min_alloc_uv.min(min / self.texture.size());
        self.max_alloc_uv = self.max_alloc_uv.max(max / self.texture.size());

        Rect::from_min_max(min, max)
    }

    pub fn alloc_new_glyph(
        &mut self,
        glyph_key: ctext::CacheKey,
        fonts: &mut FontTable,
        wgpu: &WGPU,
    ) -> Option<Glyph> {
        let img = self
            .swash_cache
            .get_image_uncached(&mut fonts.sys, glyph_key)?;
        let x = img.placement.left;
        let y = img.placement.top;
        let w = img.placement.width;
        let h = img.placement.height;

        let (has_color, data) = match img.content {
            ctext::SwashContent::Mask => {
                let mut data = Vec::new();
                data.reserve_exact((w * h * 4) as usize);
                for val in img.data {
                    data.push(255);
                    data.push(255);
                    data.push(255);
                    data.push(val);
                }
                (false, data)
            }
            ctext::SwashContent::Color => (true, img.data),
            ctext::SwashContent::SubpixelMask => {
                unimplemented!()
            }
        };

        // let rect = self
        //     .alloc
        //     .allocate(etagere::Size::new(w as i32, h as i32))?
        //     .rectangle;
        let rect = self.alloc_rect(w, h);

        wgpu.queue.write_texture(
            wgpu::TexelCopyTextureInfoBase {
                texture: &self.texture.raw(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.min.x as u32,
                    y: rect.min.y as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        let tex_size = self.texture.width();
        assert!(self.texture.height() == tex_size);
        let pos = Vec2::new(x as f32, -y as f32);
        let size = Vec2::new(w as f32, h as f32);
        let uv_min = Vec2::new(rect.min.x as f32, rect.min.y as f32) / tex_size as f32;
        let uv_max = uv_min + size / tex_size as f32;

        // self.min_alloc_uv = self.min_alloc_uv.min(uv_min);
        // self.max_alloc_uv = self.max_alloc_uv.max(uv_max);

        let meta = GlyphMeta {
            pos,
            size,
            uv_min,
            uv_max,
        };
        self.cached_glyphs.insert(glyph_key, meta);

        Some(Glyph {
            texture: self.texture.clone(),
            meta,
        })
    }
}

//---------------------------------------------------------------------------------------
// END TEXT

// BEGIN RENDER
//---------------------------------------------------------------------------------------

pub const MAX_N_TEXTURES_PER_DRAW_CALL: usize = 8;

pub struct MergedDrawLists {
    pub gpu_vertices: wgpu::Buffer,
    pub gpu_indices: wgpu::Buffer,

    pub call_list: DrawCallList,
    pub screen_size: Vec2,

    pub antialias: bool,

    pub glyph_texture: gpu::Texture,

    pub wgpu: WGPUHandle,
}

impl MergedDrawLists {
    /// 2^16
    pub const MAX_VERTEX_COUNT: u64 = 65_536;
    // 2^17
    pub const MAX_INDEX_COUNT: u64 = 131_072;

    pub fn new(glyph_texture: gpu::Texture, wgpu: WGPUHandle) -> Self {
        // let mut font_db = ctext::fontdb::Database::new();
        // font_db.load_font_data(include_bytes!("../res/Roboto.ttf").to_vec());
        // // font_db.load_font_data(include_bytes!("CommitMono-400-Regular.otf").to_vec());
        // // font_db.load_font_data(include_bytes!("CommitMono-500-Regular.otf").to_vec());
        // let mut icon_font_db = ctext::fontdb::Database::new();
        // icon_font_db.load_font_data(include_bytes!("../res/Phosphor.ttf").to_vec());

        let gpu_vertices = wgpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draw_list_vertex_buffer"),
            size: std::mem::size_of::<Vertex>() as u64 * Self::MAX_VERTEX_COUNT,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        let gpu_indices = wgpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draw_list_vertex_buffer"),
            size: std::mem::size_of::<u32>() as u64 * Self::MAX_INDEX_COUNT,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::INDEX,
            mapped_at_creation: false,
        });

        Self {
            gpu_vertices,
            gpu_indices,
            screen_size: Vec2::ONE,
            antialias: true,
            call_list: DrawCallList::new(
                Self::MAX_VERTEX_COUNT as usize,
                Self::MAX_INDEX_COUNT as usize,
            ),
            glyph_texture,
            wgpu,
        }
    }

    pub fn clear(&mut self) {
        self.call_list.clear();
    }
}

impl RenderPassHandle for MergedDrawLists {
    const LABEL: &'static str = "draw_list_render_pass";

    fn n_render_passes(&self) -> u32 {
        // self.call_list.calls.len() as u32
        1
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        // self.draw_multiple(rpass, wgpu, 0);

        let proj =
            Mat4::orthographic_lh(0.0, self.screen_size.x, self.screen_size.y, 0.0, -1.0, 1.0);

        let global_uniform = GlobalUniform::new(self.screen_size, proj);

        let bind_group = build_bind_group(global_uniform, self.glyph_texture.view(), wgpu);


        // if self.call_list.vtx_alloc.len() * std::mem::size_of::<Vertex>() >= self.gpu_vertices.size() as usize {
        //     self.gpu_vertices = wgpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        //         label: Some("draw_list_vertex_buffer"),
        //         usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        //         contents: bytemuck::cast_slice(&self.call_list.vtx_alloc),
        //     });
        // } else {
        //     wgpu.queue
        //         .write_buffer(&self.gpu_vertices, 0, bytemuck::cast_slice(&self.call_list.vtx_alloc));
        // }

        // if self.call_list.idx_alloc.len() * std::mem::size_of::<Vertex>() >= self.gpu_indices.size() as usize {
        //     self.gpu_indices = wgpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        //         label: Some("draw_list_index_buffer"),
        //         usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::INDEX,
        //         contents: bytemuck::cast_slice(&self.call_list.idx_alloc),
        //     });
        // } else {
        //     wgpu.queue
        //         .write_buffer(&self.gpu_indices, 0, bytemuck::cast_slice(&self.call_list.idx_alloc));
        // }

        // let (verts, indxs, clip) = self.call_list.get_draw_call_data(i).unwrap();
        for call in &self.call_list.calls {

            let clip = call.clip_rect;

            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.set_vertex_buffer(0, self.gpu_vertices.slice(..));
            rpass.set_index_buffer(self.gpu_indices.slice(..), wgpu::IndexFormat::Uint32);
            rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

            let target_size = self.screen_size.as_uvec2();
            let clip_min = clip.min.as_uvec2().max(UVec2::ZERO).min(target_size);
            let clip_max = clip.max.as_uvec2().max(clip_min).min(target_size);
            let clip_size = clip_max - clip_min;

            // let clip_min = clip.min.as_uvec2().clamp(Vec2::ZERO, target_size);
            // let clip_size = clip.size().as_uvec2().clamp(Vec2::ZERO, target_size);
            rpass.set_scissor_rect(clip_min.x, clip_min.y, clip_size.x, clip_size.y);

            let idx_offset = call.idx_ptr as u32;
            let vtx_offset = call.vtx_ptr as i32;
            let n_idx = call.n_idx as u32;
            rpass.draw_indexed(idx_offset..idx_offset+n_idx, vtx_offset, 0..1);
        }
    }

    fn draw_multiple<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU, i: u32) {
        let proj =
            Mat4::orthographic_lh(0.0, self.screen_size.x, self.screen_size.y, 0.0, -1.0, 1.0);

        let global_uniform = GlobalUniform::new(self.screen_size, proj);

        let bind_group = build_bind_group(global_uniform, self.glyph_texture.view(), wgpu);

        let (verts, indxs, clip) = self.call_list.get_draw_call_data(i).unwrap();

        wgpu.queue
            .write_buffer(&self.gpu_vertices, 0, bytemuck::cast_slice(verts));
        wgpu.queue
            .write_buffer(&self.gpu_indices, 0, bytemuck::cast_slice(indxs));

        rpass.set_bind_group(0, &bind_group, &[]);
        rpass.set_vertex_buffer(0, self.gpu_vertices.slice(..));
        rpass.set_index_buffer(self.gpu_indices.slice(..), wgpu::IndexFormat::Uint32);
        rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

        let target_size = self.screen_size.as_uvec2();
        let clip_min = clip.min.as_uvec2().max(UVec2::ZERO).min(target_size);
        let clip_max = clip.max.as_uvec2().max(clip_min).min(target_size);
        let clip_size = clip_max - clip_min;

        // let clip_min = clip.min.as_uvec2().clamp(Vec2::ZERO, target_size);
        // let clip_size = clip.size().as_uvec2().clamp(Vec2::ZERO, target_size);
        rpass.set_scissor_rect(clip_min.x, clip_min.y, clip_size.x, clip_size.y);

        rpass.draw_indexed(0..indxs.len() as u32, 0, 0..1);
    }
}

/// Represents a contiguous segment of vertex and index data
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawCall {
    pub clip_rect: Rect,
    pub vtx_ptr: usize,
    pub idx_ptr: usize,
    pub n_vtx: usize,
    pub n_idx: usize,
    pub textures: ArrVec<TextureId, MAX_N_TEXTURES_PER_DRAW_CALL>,
}

impl DrawCall {
    pub fn new() -> Self {
        Self {
            clip_rect: Rect::ZERO,
            vtx_ptr: 0,
            idx_ptr: 0,
            n_vtx: 0,
            n_idx: 0,
            textures: ArrVec::new(),
        }
    }
}

/// A chunked buffer storing vertices and indices,
///
/// Allowing multiple render passes
/// when a single draw exceeds GPU limits or predefined chunk sizes.
#[derive(Clone)]
pub struct DrawCallList {
    pub max_vtx_per_chunk: usize,
    pub max_idx_per_chunk: usize,
    pub vtx_alloc: Vec<Vertex>,
    pub idx_alloc: Vec<u32>,
    /// Current write offset in `vtx_alloc`.
    pub vtx_ptr: usize,
    /// Current write offset in `idx_alloc`.
    pub idx_ptr: usize,
    pub calls: Vec<DrawCall>,
}

impl fmt::Debug for DrawCallList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrawCallList")
            .field("max_vtx_per_chunk", &self.max_vtx_per_chunk)
            .field("max_idx_per_chunk", &self.max_idx_per_chunk)
            .field("vtx_alloc", &self.vtx_alloc.len())
            .field("idx_alloc", &self.idx_alloc.len())
            .field("vtx_ptr", &self.vtx_ptr)
            .field("idx_ptr", &self.idx_ptr)
            .field("calls", &self.calls)
            .finish()
    }
}

impl DrawCallList {
    pub fn clear(&mut self) {
        self.calls.clear();
        self.vtx_ptr = 0;
        self.idx_ptr = 0;
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn new(max_vtx_per_chunk: usize, max_idx_per_chunk: usize) -> Self {
        let max_idx_per_chunk = usize::MAX;
        let max_vtx_per_chunk = usize::MAX;
        Self {
            max_vtx_per_chunk,
            max_idx_per_chunk,
            vtx_alloc: vec![],
            idx_alloc: vec![],
            vtx_ptr: 0,
            idx_ptr: 0,
            calls: vec![],
        }
    }

    pub fn get_draw_call_data(&self, chunk_idx: u32) -> Option<(&[Vertex], &[u32], Rect)> {
        self.calls.get(chunk_idx as usize).map(|chunk| {
            let vtx_slice = &self.vtx_alloc[chunk.vtx_ptr..chunk.vtx_ptr + chunk.n_vtx];
            let idx_slice = &self.idx_alloc[chunk.idx_ptr..chunk.idx_ptr + chunk.n_idx];
            (vtx_slice, idx_slice, chunk.clip_rect)
        })
    }

    pub fn push(&mut self, vtx: &[Vertex], idx: &[u32]) {
        if vtx.len() > self.max_vtx_per_chunk || idx.len() > self.max_idx_per_chunk {
            panic!(
                "Input data exceeds maximum chunk size: vtx={}, idx={}, max_vtx={}, max_idx={}",
                vtx.len(),
                idx.len(),
                self.max_vtx_per_chunk,
                self.max_idx_per_chunk
            );
        }

        if self.calls.is_empty() {
            self.calls.push(DrawCall::new());
        }

        let c = *self.calls.last().unwrap();

        if c.n_vtx + vtx.len() > self.max_vtx_per_chunk
            || c.n_idx + idx.len() > self.max_idx_per_chunk
        {
            let prev_clip = self.calls.last().unwrap().clip_rect;
            self.calls.push(DrawCall {
                clip_rect: prev_clip,
                vtx_ptr: self.vtx_ptr,
                idx_ptr: self.idx_ptr,
                n_vtx: 0,
                n_idx: 0,
                textures: ArrVec::new(),
            });
        }

        let c = self.calls.last_mut().unwrap();

        if self.vtx_alloc.len() < self.vtx_ptr + vtx.len() {
            self.vtx_alloc
                .resize(self.vtx_ptr + vtx.len(), Vertex::ZERO);
        }

        if self.idx_alloc.len() < self.idx_ptr + idx.len() {
            self.idx_alloc.resize(self.idx_ptr + idx.len(), 0);
        }

        self.vtx_alloc[self.vtx_ptr..self.vtx_ptr + vtx.len()].copy_from_slice(vtx);
        self.idx_alloc[self.idx_ptr..self.idx_ptr + idx.len()]
            .iter_mut()
            .zip(idx.iter())
            .for_each(|(dst, &src)| *dst = src + c.n_vtx as u32);
        // for (i, &index) in idx.iter().enumerate() {
        //     self.idx_alloc[self.idx_ptr + i] = index + c.n_vtx as u32;
        // }

        c.n_vtx += vtx.len();
        c.n_idx += idx.len();
        self.vtx_ptr += vtx.len();
        self.idx_ptr += idx.len();
    }

    pub fn set_clip_rect(&mut self, rect: Rect) {
        if rect == Rect::ZERO {
            log::warn!("zero clip rect set");
        }
        if self.calls.is_empty() {
            self.calls.push(DrawCall::new());
        }

        let c = self.calls.last_mut().unwrap();
        if c.clip_rect == Rect::ZERO {
            c.clip_rect = rect
        } else if c.clip_rect != rect {
            self.calls.push(DrawCall {
                clip_rect: rect,
                vtx_ptr: self.vtx_ptr,
                idx_ptr: self.idx_ptr,
                n_vtx: 0,
                n_idx: 0,
                textures: ArrVec::new(),
            });
            // let c = self.calls.last_mut().unwrap();
            // c.clip_rect = rect;
        }
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.calls.last().unwrap().clip_rect
    }
}

pub struct UiShader;

impl gpu::ShaderHandle for UiShader {
    const RENDER_PIPELINE_ID: gpu::ShaderID = "ui_shader";

    fn build_pipeline(&self, desc: &gpu::ShaderTemplates<'_>, wgpu: &WGPU) -> wgpu::RenderPipeline {
        const SHADER_SRC: &str = r#"


            @rust struct Vertex {
                pos: vec2<f32>,
                uv: vec2<f32>,
                col: vec4<f32>,
                tex: u32,
                ...
            }

            struct GlobalUniform {
                screen_size: vec2<f32>,
                _pad: vec2<f32>,
                proj: mat4x4<f32>,
            }

            @group(0) @binding(0)
            var<uniform> global: GlobalUniform;

            struct VSOut {
                @builtin(position) pos: vec4<f32>,
                @location(0) color: vec4<f32>,
                @location(1) uv: vec2<f32>,
                @location(2) @interpolate(flat) tex: u32,
            };

            @vertex
            fn vs_main(
                v: Vertex,
            ) -> VSOut {
                var out: VSOut;

                out.color = v.col;
                out.uv = v.uv;
                out.tex = v.tex;

                out.pos = global.proj * vec4(v.pos, 0.0, 1.0);
                return out;
            }


            @group(0) @binding(1)
            var samp: sampler;
            @group(0) @binding(2)
            var texture: texture_2d<f32>;


            @fragment
            fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
                let c0 = textureSample(texture, samp, in.uv) * in.color;
                let c1 = in.color;
                return select(c0, c1, in.tex != 1);
            }
            "#;

        let bind_group_entries = [
            // global uniform
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // sampler
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // texture
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ];

        let global_bind_group_layout =
            wgpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &bind_group_entries,
                    label: Some("global_bind_group_layout"),
                });

        let shader_src = gpu::pre_process_shader_code(SHADER_SRC, &desc).unwrap();
        let vertices = desc.iter().map(|d| d.0).collect::<Vec<_>>();
        gpu::PipelineBuilder::new(&shader_src, wgpu.surface_format)
            .label("rect_pipeline")
            .vertex_buffers(&vertices)
            .bind_groups(&[&global_bind_group_layout])
            .blend_state(Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }))
            .sample_count(1)
            .build(&wgpu.device)
    }
}

#[macros::vertex]
pub struct Vertex {
    pub pos: Vec2,
    pub uv: Vec2,
    pub tex: u32,
    pub _pad: u32,
    pub col: RGBA,
}

impl Vertex {
    pub const ZERO: Self = Self {
        pos: Vec2::ZERO,
        uv: Vec2::ZERO,
        tex: 0,
        _pad: 0,
        col: RGBA::ZERO,
    };

    pub fn new(pos: Vec2, col: RGBA, uv: Vec2, tex: u32) -> Self {
        Self {
            pos,
            uv,
            tex,
            _pad: 0,
            col,
        }
    }
    pub fn color(pos: Vec2, col: RGBA) -> Self {
        Self::new(pos, col, Vec2::ZERO, 0)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct GlobalUniform {
    pub screen_size: Vec2,
    pub _pad: Vec2,
    pub proj: Mat4,
}

impl GlobalUniform {
    pub fn new(screen_size: Vec2, proj: Mat4) -> Self {
        Self {
            screen_size,
            _pad: Vec2::ZERO,
            proj,
        }
    }

    pub fn build_bind_group(&self, wgpu: &WGPU) -> wgpu::BindGroup {
        let global_uniform = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rect_global_uniform_buffer"),
                contents: bytemuck::cast_slice(&[*self]),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let global_bind_group_layout =
            wgpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                    label: Some("global_bind_group_layout"),
                });

        wgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("global_bind_group"),
            layout: &global_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: global_uniform.as_entire_binding(),
            }],
        })
    }
}

pub fn build_bind_group(
    glob: GlobalUniform,
    tex_view: &wgpu::TextureView,
    wgpu: &WGPU,
) -> wgpu::BindGroup {
    let global_uniform = wgpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rect_global_uniform_buffer"),
            contents: bytemuck::cast_slice(&[glob]),
            usage: wgpu::BufferUsages::UNIFORM,
        });

    let layout_entries = [
        // global uniform
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        // sampler
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        // texture
        wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
    ];

    let global_bind_group_layout =
        wgpu.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &layout_entries,
                label: Some("global_bind_group_layout"),
            });

    let sampler = wgpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("ui_texture_sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let group_entries = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &global_uniform,
                offset: 0,
                size: None,
            }),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(tex_view),
        },
    ];

    wgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("global_bind_group"),
        layout: &global_bind_group_layout,
        entries: &group_entries,
    })
}

//---------------------------------------------------------------------------------------
// END RENDER
