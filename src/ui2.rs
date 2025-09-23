use std::{collections::HashMap, fmt, hash, rc::Rc};

use bitflags::bitflags as flags;
use glam::{Mat4, Vec2};
use rustc_hash::FxHashMap;
use cosmic_text as ctext;

use crate::{
    gpu::{self, RenderPassHandle, ShaderHandle, WGPUHandle, Window, WindowId, WGPU},
    mouse::{CursorIcon, MouseBtn, MouseState},
    rect::Rect,
    ui::{Dir, Layout, Placement, Signals},
    ui_draw::{self, Vertex},
    utils::RGBA, Vertex as VertexTyp,
};

pub struct Context {
    pub panels: FxHashMap<Id, Panel>,

    pub current_panel_stack: Vec<Id>,
    pub current_panel: Id,
    pub draw_order: Vec<Id>,
    pub last_item_data: Option<LastItemData>,
    pub panel_action: PanelAction,
    // pub resizing_window_dir: Option<Dir>,
    pub next_panel_data: NextPanelData,

    pub hot_id: Id,
    pub hot_panel_id: Id,
    pub active_id: Id,
    pub active_panel_id: Id,

    pub frame_count: u64,
    pub draw_debug: bool,
    pub mouse: MouseState,
    pub cursor_icon: CursorIcon,
    pub cursor_icon_changed: bool,
    pub resize_threshold: f32,

    pub draw: MergedDrawLists,
    pub glyph_cache: GlyphCache,
    pub text_item_cache: TextItemCache,
    pub font_table: FontTable,

    pub close_pressed: bool,
    pub window: Window,
    pub requested_windows: Vec<(Vec2, Vec2)>,
    pub ext_window: Option<Window>,
}

macro_rules! get_panel {
    ($state:ident: $id:expr) => {{
        let id = $id;
        $state.panels.get(&id).unwrap()
    }};
}
macro_rules! get_curr_panel {
    ($state:ident) => {
        $state.panels.get(&$state.current_panel).unwrap()
    };

    (mut $state:ident) => {
        $state.panels.get_mut(&$state.current_panel).unwrap()
    };
}

impl Context {
    pub fn new(wgpu: WGPUHandle, window: Window) -> Self {

        let glyph_cache = GlyphCache::new(&wgpu);
        let mut font_table = FontTable::new();
        font_table.load_font("Roboto", include_bytes!("../res/Roboto.ttf").to_vec());
        Self {
            panels: Default::default(),
            draw: MergedDrawLists::new(glyph_cache.texture.clone(), wgpu),
            current_panel_stack: vec![],
            current_panel: Id::NULL,
            last_item_data: None,

            hot_id: Id::NULL,
            hot_panel_id: Id::NULL,
            active_id: Id::NULL,
            active_panel_id: Id::NULL,
            panel_action: PanelAction::None,
            // resizing_window_dir: None,
            next_panel_data: NextPanelData::default(),

            draw_order: Vec::new(),
            draw_debug: false,
            frame_count: 0,
            mouse: MouseState::new(),
            cursor_icon: CursorIcon::Default,
            cursor_icon_changed: false,
            resize_threshold: 10.0,

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
            let id = self.find_panel_by_name("#ROOT_PANEL");
            let root_panel = self.panels.get_mut(&id).unwrap();
            let titlebar_height = root_panel.titlebar_height;
            if let Some(dir) = resize_dir {
                // self.curr_widget_action = WidgetAction::ResizeWindow { dir };
                self.window.start_drag_resize_window(dir)
            } else if self.mouse.pos.y <= titlebar_height {
                // self.curr_widget_action = WidgetAction::DragWindow;
                self.window.start_drag_window()
            }
        }

        // if !press && lft_btn && self.curr_widget_action.is_window_action() {
        //     self.curr_widget_action = WidgetAction::None;
        // }
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.mouse.set_mouse_pos(x, y);

        if self.window.is_maximized() || self.window.is_decorated() {
            return;
        }

        let w_size = self.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);
        let resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold);

        if let Some(dir) = resize_dir {
            self.set_cursor_icon(dir.as_cursor());
        } else if self.cursor_icon.is_resize() {
            self.set_cursor_icon(CursorIcon::Default);
        }
    }

    pub fn begin(&mut self, name: &str) {
        self.begin_ex(name, PanelFlags::NONE);
    }

    pub fn begin_ex(&mut self, name: &str, flags: PanelFlags) {
        let mut newly_created = false;
        let mut id = self.find_panel_by_name(name);
        if id.is_null() {
            id = self.create_panel(name);
            newly_created = true;
        }

        self.current_panel_stack.push(id);
        self.current_panel = id;

        let p = self.panels.get_mut(&id).unwrap();
        if newly_created {
            p.draw_order = self.draw_order.len();
            self.draw_order.push(id);
        }
        if let Some(pos) = self.next_panel_data.pos {
            p.pos = pos;
        }
        p.explicit_size = self.next_panel_data.size;
        p.bg_color = self.next_panel_data.bg_color;
        p.outline = self.next_panel_data.outline;
        p.titlebar_height = self.next_panel_data.titlebar_height;
        p.layout = self.next_panel_data.layout;
        self.next_panel_data.reset();
        // let first_begin_this_frame = p.last_frame_used == self.frame_count;
        p.last_frame_used = self.frame_count;
        p.draw_list.clear();
        p.id_stack.push(p.id);
        p.tmp.root = p.id;

        let prev_max_pos = p.tmp.cursor_max_pos;

        p.tmp.cursor_content_start_pos =
            p.pos + Vec2::new(p.padding, p.padding + p.titlebar_height);
        p.tmp.cursor_pos = p.tmp.cursor_content_start_pos;
        p.tmp.cursor_max_pos = p.tmp.cursor_pos;

        // preserve when?
        p.content_size = prev_max_pos - p.tmp.cursor_content_start_pos;
        p.full_size = prev_max_pos - p.pos;

        let p = self.panels.get_mut(&id).unwrap();
        let panel_pos = p.pos;

        // bg
        let panel_size = if let Some(size) = p.explicit_size {
            size
        } else {
            p.full_size
        };
        p.size = panel_size;
        p.draw_list.add_rect(
            panel_pos,
            panel_pos + panel_size,
            Some(p.bg_color),
            p.outline.map(|(col, w)| (col, w, OutlinePlacement::Inner)),
            &[10.0],
        );

        let panel_rect = Rect::from_min_size(p.pos, p.full_size);
        let p_titlebar_height = p.titlebar_height;

        let curr_draw_order = p.draw_order;
        if panel_rect.contains(self.mouse.pos) {
            if self.hot_panel_id.is_null()
                || self.panels.get(&self.hot_panel_id).unwrap().draw_order < curr_draw_order
            {
                self.hot_panel_id = id;
            }
        }

        let p = self.panels.get_mut(&id).unwrap();
        let is_window_panel = p.is_window_panel;
        if !flags.contains(PanelFlags::NO_TITLEBAR) {
            // titlebar
            let tb_id = p.gen_id("#MOVE");
            p.move_id = tb_id;
            p.draw_list.add_rect(
                panel_pos,
                panel_pos + Vec2::new(panel_size.x, p.titlebar_height),
                Some(RGBA::hex("#202020")),
                None,
                &[10.0, 10.0, 0.0, 0.0],
            );
            // let tb_rect = Rect::from_min_size(p.pos, Vec2::new(panel_size.x, p.titlebar_height));
            let prev_pos = self.cursor_pos();
            self.set_cursor_pos(panel_pos);

            self.add_item(
                tb_id,
                Vec2::new(panel_size.x, p_titlebar_height),
                ItemFlags::RAW,
            );

            // let p = self.panels.get_mut(&id).unwrap();

            let button_size = Vec2::new(25.0, 25.0);
            self.set_cursor_pos(panel_pos);
            self.move_cursor(Vec2::new(0.0, (p_titlebar_height - button_size.y) / 2.0));
            self.move_cursor(Vec2::new(panel_size.x - 15.0 - button_size.x, 0.0));

            if is_window_panel {
                self.move_cursor(Vec2::new(-10.0 - button_size.x, 0.0));
                let p = self.panels.get_mut(&id).unwrap();
                let max_id = p.gen_id("max");
                self.add_item(max_id, button_size, ItemFlags::RAW);
                let sig = self.get_last_item_signals();
                let mut color = RGBA::WHITE;
                if sig.hovering() {
                    color = RGBA::BLUE;
                }
                if sig.released() {
                    self.window.toggle_maximize();
                }

                let p = self.panels.get_mut(&id).unwrap();
                p.draw_list.add_rect(
                    p.tmp.cursor_pos,
                    p.tmp.cursor_pos + button_size,
                    Some(color),
                    None,
                    &[],
                );

                self.move_cursor(Vec2::new(button_size.x + 10.0, 0.0));
            }

            let p = self.panels.get_mut(&id).unwrap();
            let close_id = p.gen_id("X");
            self.add_item(close_id, button_size, ItemFlags::RAW);

            // self.button("X", RGBA::WHITE);
            let sig = self.get_last_item_signals();
            let mut color = RGBA::WHITE;
            if sig.hovering() {
                color = RGBA::RED;
            }
            if sig.pressed() {
                let p = self.panels.get_mut(&id).unwrap();
                p.close_pressed = true;
            }

            let p = self.panels.get_mut(&id).unwrap();
            p.draw_list.add_rect(
                p.tmp.cursor_pos,
                p.tmp.cursor_pos + button_size,
                Some(color),
                None,
                &[],
            );
            self.set_cursor_pos(prev_pos);
            self.last_item_data = None;
        }
        // p.tmp.cursor_pos = panel_pos + Vec2::splat(10.0);
        // self.set_cursor_pos(panel_pos);
        // self.offset_cursor_pos(Vec2::new(panel_size.x - 40.0, 0.0));
    }

    pub fn get_last_item_signals(&self) -> Signals {
        let data = self.last_item_data.expect("no last item");
        let id = data.id;
        let rect = data.rect;
        self.get_item_signals(id, rect)
    }

    pub fn get_item_signals(&self, id: Id, bb: Rect) -> Signals {
        let mut sig = Signals::empty();

        if bb.contains(self.mouse.pos) {
            sig |= Signals::MOUSE_OVER;

            if self.hot_id == id {
                sig |= Signals::HOVERING;
            }
        }

        if !sig.hovering() {
            return sig;
        }

        if self.mouse.pressed(MouseBtn::Left) {
            sig |= Signals::PRESSED_LEFT;
        }
        if self.mouse.pressed(MouseBtn::Right) {
            sig |= Signals::PRESSED_RIGHT;
        }
        if self.mouse.pressed(MouseBtn::Middle) {
            sig |= Signals::PRESSED_MIDDLE;
        }

        if self.mouse.double_clicked(MouseBtn::Left) {
            sig |= Signals::DOUBLE_CLICKED_LEFT;
        }
        if self.mouse.double_clicked(MouseBtn::Right) {
            sig |= Signals::DOUBLE_CLICKED_RIGHT;
        }
        if self.mouse.double_clicked(MouseBtn::Middle) {
            sig |= Signals::DOUBLE_CLICKED_MIDDLE;
        }

        if self.mouse.dragging(MouseBtn::Left) {
            sig |= Signals::DRAGGING_LEFT;
        }
        if self.mouse.dragging(MouseBtn::Right) {
            sig |= Signals::DRAGGING_RIGHT;
        }
        if self.mouse.dragging(MouseBtn::Middle) {
            sig |= Signals::DRAGGING_MIDDLE;
        }

        if self.mouse.released(MouseBtn::Left) {
            sig |= Signals::RELEASED_LEFT
        }
        if self.mouse.released(MouseBtn::Right) {
            sig |= Signals::RELEASED_RIGHT
        }
        if self.mouse.released(MouseBtn::Middle) {
            sig |= Signals::RELEASED_MIDDLE
        }

        sig
    }

    pub fn end(&mut self) {
        let p = get_curr_panel!(mut self);
        let p_pad = p.padding;
        p.id_stack.pop().unwrap();
        if !p.id_stack.is_empty() {
            log::warn!("non empty id stack at ");
        }
        // self.offset_cursor_pos(Vec2::splat(p_pad));
        p.tmp.cursor_max_pos += Vec2::splat(p.padding);

        self.current_panel_stack.pop();
        self.current_panel = self.current_panel_stack.last().copied().unwrap_or(Id::NULL);
    }

    pub fn button(&mut self, label: &str, col: RGBA) {
        let p = get_curr_panel!(mut self);
        let id = p.gen_id(label);
        let size = Vec2::new(label.len() as f32 * 12.0, 12.0);

        // let bb = Rect::from_min_size(p.tmp.cursor_pos, size);
        self.add_item(id, size, ItemFlags::NONE);
        self.add_item_size(size);

        let p = get_curr_panel!(mut self);
        let item_rect = self.last_item_data.unwrap().rect;
        p.draw_list
            .add_rect(item_rect.min, item_rect.max, Some(col), None, &[]);
    }

    pub fn move_cursor(&mut self, off: Vec2) {
        let p = get_curr_panel!(mut self);
        p.tmp.cursor_pos += off;
        p.tmp.cursor_max_pos = p.tmp.cursor_max_pos.max(p.tmp.cursor_pos);
    }

    pub fn cursor_pos(&self) -> Vec2 {
        get_curr_panel!(self).tmp.cursor_pos
    }

    pub fn set_cursor_pos(&mut self, pos: Vec2) {
        let p = get_curr_panel!(mut self);
        p.tmp.cursor_pos = pos;
        p.tmp.cursor_max_pos = p.tmp.cursor_max_pos.max(p.tmp.cursor_pos);
    }

    pub fn add_item_size(&mut self, size: Vec2) {
        let p = get_curr_panel!(mut self);
        let rect = Rect::from_min_size(p.tmp.cursor_pos, size);

        match p.layout {
            Layout::Vertical => {
                p.tmp.cursor_max_pos = p.tmp.cursor_max_pos.max(rect.max);
                p.tmp.cursor_pos.y += size.y;
            }
            Layout::Horizontal => {
                p.tmp.cursor_max_pos = p.tmp.cursor_max_pos.max(rect.max);
                p.tmp.cursor_pos.x += size.x;
            }
        }
    }

    // pub fn add_item(&mut self, id: Id, bb: Rect)
    pub fn add_item(&mut self, id: Id, size: Vec2, flags: ItemFlags) {
        let p = get_curr_panel!(mut self);
        let p_draw_order = p.draw_order;

        if self.last_item_data.is_some() && !flags.contains(ItemFlags::RAW) {
            match p.layout {
                Layout::Vertical => {
                    p.tmp.cursor_pos.y += p.spacing;
                }
                Layout::Horizontal => {
                    p.tmp.cursor_pos.x += p.spacing;
                }
            }
        }

        let bb = Rect::from_min_size(p.tmp.cursor_pos, size);
        if bb.contains(self.mouse.pos) {
            let is_over = if self.hot_panel_id.is_null() {
                true
            } else {
                let hot_p = get_panel!(self: self.hot_panel_id);
                hot_p.draw_order >= p_draw_order
            };
            // if self.hot_panel_id.is_null() || self.hot_panel_id == p.id
            if is_over {
                self.hot_id = id;
            }
        }

        let p = get_curr_panel!(mut self);

        if self.mouse.pressed(MouseBtn::Left)
            && !self.mouse.dragging(MouseBtn::Left)
            && self.panel_action.is_none()
        {
            self.active_id = self.hot_id;
            self.active_panel_id = self.hot_panel_id;
        }

        if self.active_id == p.move_id {
            if self.mouse.dragging(MouseBtn::Left) && self.panel_action.is_none() {
                self.panel_action = PanelAction::Drag {
                    id: p.tmp.root,
                    start_pos: p.pos,
                }
            }
            if !self.mouse.dragging(MouseBtn::Left)
                && matches!(self.panel_action, PanelAction::Drag { .. })
            {
                self.panel_action = PanelAction::None;
            }

            match self.panel_action {
                PanelAction::Resize { dir, id, prev_rect } => {}
                PanelAction::Drag { start_pos, id } => {
                    let drag_start = self.mouse.drag_start(MouseBtn::Left).unwrap();
                    let delta = start_pos - drag_start;
                    p.pos = self.mouse.pos + delta;

                    let p_rect = Rect::from_min_size(p.pos, p.size);
                    let w_size = self.window.window_size();
                    let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);
                    if !w_rect.contains_rect(p_rect) && self.ext_window.is_none() {
                        let p_size = p_rect.size();
                        // self.requested_windows.push((Vec2::new(p_size.x, p_size.y - p.titlebar_height), self.window.window_pos() + p_rect.min));
                        self.requested_windows
                            .push((p_size, self.window.window_pos() + p_rect.min));
                    }
                }
                PanelAction::None => (),
            }
            // if let Some(MovingPanelData { panel_id, pre_pos }) = self.moving_panel_data {
            //     let drag_start = self.mouse.drag_start(MouseBtn::Left).unwrap();
            //     let delta = pre_pos - drag_start;
            //     p.pos = self.mouse.pos + delta;

            //     let p_rect = Rect::from_min_size(p.pos, p.size);
            //     let w_size = self.window.window_size();
            //     let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);
            //     if !w_rect.contains_rect(p_rect) && self.ext_window.is_none() {

            //         let p_size = p_rect.size();
            //         // self.requested_windows.push((Vec2::new(p_size.x, p_size.y - p.titlebar_height), self.window.window_pos() + p_rect.min));
            //         self.requested_windows.push((p_size, self.window.window_pos() + p_rect.min));
            //     }
            // }
        }

        // // debug
        // {
        //     p.draw_list.add_rect(rect.min, rect.max, Some(RGBA::CARMINE), None, 0.0);
        // }

        self.last_item_data = Some(LastItemData { id, rect: bb });
    }

    pub fn create_panel(&mut self, name: &str) -> Id {
        let mut p = Panel::new(name);
        let id = p.id;
        p.frame_created = self.frame_count;
        self.panels.insert(id, p);
        id
    }

    pub fn find_panel_by_name(&self, name: &str) -> Id {
        let id = Id::from_str(name);
        if self.panels.contains_key(&id) {
            id
        } else {
            Id::NULL
        }
    }

    pub fn get_panel_mut(&mut self, id: Id) -> &mut Panel {
        self.panels.get_mut(&id).unwrap()
    }

    pub fn bring_panel_to_front(&mut self, panel_id: Id) {
        let p = &self.panels[&panel_id];
        assert!(p.tmp.root == panel_id);
        let curr_order = p.draw_order;
        assert!(self.draw_order[curr_order] == panel_id);
        if *self.draw_order.last().unwrap() == panel_id {
            return;
        }

        let new_order = self.draw_order.len() - 1;
        for i in curr_order..new_order {
            self.draw_order[i] = self.draw_order[i + 1];
            self.get_panel_mut(self.draw_order[i]).draw_order -= 1;
            assert!(self.panels[&self.draw_order[i]].draw_order == i);
        }

        self.draw_order[new_order] = panel_id;
        self.get_panel_mut(panel_id).draw_order = new_order;
    }

    pub fn begin_frame(&mut self) {
        self.draw.clear();
        self.draw.screen_size = self.window.window_size();
        self.hot_panel_id = Id::NULL;
        self.hot_id = Id::NULL;

        // if !self.window.is_decorated() {
        self.next_panel_data.pos = Some(Vec2::ZERO);
        let win_size = self.window.window_size();
        self.next_panel_data.size = Some(win_size);
        // TODO
        // self.window
        let flags = if self.window.is_decorated() {
            PanelFlags::NO_TITLEBAR
        } else {
            PanelFlags::NONE
        };
        self.begin_ex("#ROOT_PANEL", flags);
        // }

        // let p_info: Vec<_> = self.panels.iter().map(|(_, p)| (p.name.clone(), p.draw_order)).collect();
        // println!("{:#?}", p_info);
        let id = self.find_panel_by_name("#ROOT_PANEL");
        let root_panel = self.panels.get_mut(&id).unwrap();
        root_panel.is_window_panel = true;
        if root_panel.close_pressed {
            self.close_pressed = true;
        }

        self.draw_text("Hello", Vec2::new(400.0, 300.0));
        self.draw_text("World", Vec2::new(500.0, 400.0));
    }

    pub fn end_frame(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.end();
        assert!(!self.panels.contains_key(&Id::NULL));
        self.build_draw_data();
        self.frame_count += 1;
        self.mouse.end_frame();
        self.last_item_data = None;
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
        } else if !self.requested_windows.is_empty() {
            // let (size, pos) = self.requested_windows.last().unwrap();
            // self.ext_window.as_mut().unwrap().resize(size.x as u32, size.y as u32, &self.draw.wgpu.device);
        }
    }

    pub fn shape_text(&mut self, text: &str, font_size: f32) -> &ShapedText {
        let itm = TextItem::new(text.into(), font_size, 1.0, "Roboto");
        let shaped_text = if !self.text_item_cache.contains_key(&itm) {
            let shaped_text = shape_text_item(itm.clone(), &mut self.font_table, &mut self.glyph_cache, &self.draw.wgpu);
            self.text_item_cache.entry(itm).or_insert(shaped_text)
        } else {
            self.text_item_cache.get(&itm).unwrap()
        };
        shaped_text
    }

    pub fn draw_text(&mut self, text: &str, pos: Vec2) {
        // TODO[NOTE]: remove clone
        let shape = self.shape_text(text, 32.0).clone();
        let p = get_curr_panel!(mut self);
        // let itm = TextItem::new(text.into(), 32.0, 1.0, "Roboto");
        for g in shape.glyphs.iter() {
            p.draw_list.add_rect_uv(g.meta.pos + pos, g.meta.pos + g.meta.size + pos, g.meta.uv_min, g.meta.uv_max, 1);
        }
    }

    pub fn build_draw_data(&mut self) {
        let panels = &mut self.panels;
        let draw_buff = &mut self.draw.draw_buffer;

        for (_, f) in panels {
            for cmd in &f.draw_list.cmd_buffer {
                let vtx = &f.draw_list.vtx_buffer[cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count];
                let idx = &f.draw_list.idx_buffer[cmd.idx_offset..cmd.idx_offset + cmd.idx_count];

                draw_buff.push(vtx, idx);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Panel {
    pub name: String,
    pub id: Id,
    pub move_id: Id,

    pub bg_color: RGBA,
    pub outline: Option<(RGBA, f32)>,
    pub padding: f32,
    pub spacing: f32,
    pub size: Vec2,
    pub pos: Vec2,
    pub full_size: Vec2,
    pub content_size: Vec2,
    pub explicit_size: Option<Vec2>,
    pub titlebar_height: f32,
    pub layout: Layout,

    pub draw_list: DrawList,
    pub id_stack: Vec<Id>,
    pub draw_order: usize,

    pub last_frame_used: u64,
    pub frame_created: u64,
    pub close_pressed: bool,
    pub is_window_panel: bool,

    pub tmp: TempPanelData,
}

impl Panel {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            id: Id::from_str(name),
            padding: 20.0,
            spacing: 10.0,
            pos: Vec2::splat(30.0),

            content_size: Vec2::ZERO,
            full_size: Vec2::ZERO,
            explicit_size: None,
            draw_order: 0,
            bg_color: RGBA::ZERO,
            outline: None,
            titlebar_height: 0.0,
            move_id: Id::NULL,
            size: Vec2::ZERO,
            frame_created: 0,
            last_frame_used: 0,
            draw_list: DrawList::new(),
            id_stack: Vec::new(),
            close_pressed: false,
            layout: Layout::Vertical,
            is_window_panel: false,
            tmp: TempPanelData::default(),
        }
    }

    pub fn push_id(&mut self, id: Id) {
        self.id_stack.push(id);
    }

    pub fn pop_id(&mut self) -> Id {
        self.id_stack.pop().unwrap()
    }

    pub fn gen_id(&self, label: &str) -> Id {
        use std::hash::{Hash, Hasher};
        let seed = self.id_stack.last().unwrap_or(&self.id);
        let mut hasher = rustc_hash::FxHasher::default();
        seed.hash(&mut hasher);
        label.hash(&mut hasher);
        Id(hasher.finish().max(1))
    }

    pub fn clear_temp_data(&mut self) {
        self.tmp = TempPanelData::default();
    }

    pub fn draw_titlebar() {}
}

flags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct PanelFlags: u32 {
        const NONE = 0;
        const NO_TITLEBAR = 1 << 0;
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TempPanelData {
    pub root: Id,
    pub parent: Id,

    pub cursor_content_start_pos: Vec2,
    pub cursor_pos: Vec2,
    pub cursor_max_pos: Vec2,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct NextPanelData {
    pub pos: Option<Vec2>,
    pub placement: Placement,
    pub layout: Layout,
    pub titlebar_height: f32,
    pub size: Option<Vec2>,
    pub min_size: Vec2,
    pub max_size: Vec2,
    pub content_size: Option<Vec2>,
    pub bg_color: RGBA,
    pub outline: Option<(RGBA, f32)>,
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
            placement: Placement::TopLeft,
            layout: Layout::Vertical,
            size: None,
            titlebar_height: 50.0,
            min_size: Vec2::INFINITY,
            max_size: Vec2::ZERO,
            content_size: None,
            bg_color: RGBA::INDIGO,
            outline: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new()
    }
}

// pub enum CondFlag {
//     Once,
//     Always,
// }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelAction {
    Resize { dir: Dir, id: Id, prev_rect: Rect },
    Drag { start_pos: Vec2, id: Id },
    None,
}

impl PanelAction {
    pub fn is_none(&self) -> bool {
        match self {
            Self::None => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LastItemData {
    pub id: Id,
    pub rect: Rect,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(u64);

impl Id {
    pub const NULL: Id = Id(0);

    pub fn from_str(s: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        s.hash(&mut hasher);
        Self(hasher.finish().max(1))
    }

    pub fn is_null(&self) -> bool {
        *self == Self::NULL
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut n = self.0;
        if n == 0 {
            return write!(f, "0");
        }
        let mut buf = Vec::new();
        while n > 0 {
            let rem = (n % 36) as u8;
            let ch = if rem < 10 {
                b'0' + rem
            } else {
                b'A' + (rem - 10)
            };
            buf.push(ch);
            n /= 36;
        }
        buf.reverse();
        let s = std::str::from_utf8(&buf).unwrap();
        write!(f, "{}", s)
    }
}

/// A single draw command
#[derive(Debug, Clone, Copy, Default)]
pub struct DrawCmd {
    pub texture_id: u32,
    pub vtx_offset: usize,
    pub vtx_count: usize,
    pub idx_offset: usize,
    pub idx_count: usize,
}

/// The draw list itself: holds geometry and draw commands
#[derive(Debug, Clone)]
pub struct DrawList {
    pub vtx_buffer: Vec<Vertex>,
    pub idx_buffer: Vec<u32>,
    pub cmd_buffer: Vec<DrawCmd>,

    pub resolution: f32,
    pub path: Vec<Vec2>,
}

impl Default for DrawList {
    fn default() -> Self {
        Self {
            vtx_buffer: vec![],
            idx_buffer: vec![],
            cmd_buffer: vec![],
            resolution: 20.0,
            path: vec![],
        }
    }
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
    }

    pub fn curr_draw_cmd(&mut self) -> &mut DrawCmd {
        if self.cmd_buffer.is_empty() {
            self.cmd_buffer.push(DrawCmd::default())
        }
        self.cmd_buffer.last_mut().unwrap()
    }

    pub fn push_draw_cmd(&mut self) -> &mut DrawCmd {
        self.cmd_buffer.push(DrawCmd::default());
        let cmd = self.cmd_buffer.last_mut().unwrap();
        cmd.vtx_offset = self.vtx_buffer.len();
        cmd.idx_offset = self.idx_buffer.len();
        cmd
    }

    pub fn push_texture(&mut self, tex_id: u32) {
        let cmd = self.curr_draw_cmd();
        if cmd.texture_id != tex_id && tex_id != 0 {
            let cmd = self.push_draw_cmd();
            cmd.texture_id = tex_id;
        }
    }

    #[inline]
    pub fn push_vtx_idx(&mut self, vtx: &[Vertex], idx: &[u32]) {
        let cmd = self.curr_draw_cmd();
        let base = cmd.vtx_count as u32;

        self.vtx_buffer.extend_from_slice(vtx);
        self.idx_buffer.extend(idx.into_iter().map(|i| base + i));

        let cmd = self.curr_draw_cmd();
        cmd.vtx_count += vtx.len();
        cmd.idx_count += idx.len();
    }

    pub fn add_rect_impl(
        &mut self,
        min: Vec2,
        max: Vec2,
        color: RGBA,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: u32,
    ) {
        self.push_texture(tex_id);

        let vtx = [
            Vertex::new(min.with_y(max.y), color, uv_min.with_y(uv_max.y), tex_id),
            Vertex::new(max, color, uv_max, tex_id),
            Vertex::new(min.with_x(max.x), color, uv_min.with_x(uv_max.x), tex_id),
            Vertex::new(min, color, uv_min, tex_id),
        ];

        let idx = [0, 1, 2, 0, 2, 3];

        self.push_vtx_idx(&vtx, &idx);
    }

    pub fn add_rect_uv(&mut self, min: Vec2, max: Vec2, uv_min: Vec2, uv_max: Vec2, tex_id: u32) {
        self.add_rect_impl(min, max, RGBA::WHITE, uv_min, uv_max, tex_id);
    }

    pub fn add_rect(
        &mut self,
        mut min: Vec2,
        mut max: Vec2,
        fill: Option<RGBA>,
        outline: Option<(RGBA, f32, OutlinePlacement)>,
        round: &[f32],
    ) {
        if round.is_empty() {
            if let Some(fill) = fill {
                self.add_rect_impl(min, max, fill, Vec2::ZERO, Vec2::ZERO, 0);
            }
            if let Some((col, width, placement)) = outline {
                let offset = match placement {
                    OutlinePlacement::Center => 0.0,
                    OutlinePlacement::Inner => -width * 0.5,
                    OutlinePlacement::Outer => width * 0.5,
                };

                min -= Vec2::splat(offset);
                max += Vec2::splat(offset);

                let pts = [min.with_y(max.y), max, max.with_x(min.x), min];
                let (vtx, idx) = tessellate_line(&pts, col, width, true);
                self.push_vtx_idx(&vtx, &idx);
            }
            return;
        }

        self.path_clear();
        if let Some((_, width, placement)) = outline {
            let offset = match placement {
                OutlinePlacement::Center => 0.0,
                OutlinePlacement::Inner => -width * 0.5,
                OutlinePlacement::Outer => width * 0.5,
            };
            min -= Vec2::splat(offset);
            max += Vec2::splat(offset);
        }
        self.path_rect(min, max, round);

        if let Some(fill) = fill {
            let (vtx, idx) = tessellate_convex_fill(&self.path, fill, true);
            self.push_vtx_idx(&vtx, &idx);
        }

        if let Some((col, width, _)) = outline {
            let (vtx, idx) = tessellate_line(&self.path, col, width, true);
            self.push_vtx_idx(&vtx, &idx);
        }
        self.path_clear();
    }

    // pub fn add_rect(
    //     &mut self,
    //     min: Vec2,
    //     max: Vec2,
    //     fill: Option<RGBA>,
    //     outline: Option<(RGBA, f32)>,
    //     round: &[f32],
    // ) {
    //     if round.is_empty() {
    //         if let Some(fill) = fill {
    //             self.add_rect_impl(min, max, fill, Vec2::ZERO, Vec2::ZERO, 0);
    //         }
    //         if let Some((col, width)) = outline {
    //             let pts = [min.with_y(max.x), max, max.with_x(max.y), min];
    //             let (vtx, idx) = tessellate_line(&pts, col, width, true);
    //             self.push_vtx_idx(&vtx, &idx);
    //         }
    //         return;
    //     }

    //     self.path_clear();
    //     self.path_rect(min, max, round);

    //     if let Some(fill) = fill {
    //         let (vtx, idx) = tessellate_convex_fill(&self.path, fill, true);
    //         self.push_vtx_idx(&vtx, &idx);
    //     }

    //     if let Some((col, width)) = outline {
    //         let (vtx, idx) = tessellate_line(&self.path, col, width, true);
    //         self.push_vtx_idx(&vtx, &idx);
    //     }
    //     self.path_clear();
    // }

    pub fn path_clear(&mut self) {
        self.path.clear();
    }

    pub fn path_to(&mut self, p: Vec2) {
        self.path.push(p);
    }

    pub fn path_rect(&mut self, min: Vec2, max: Vec2, corner_radii: &[f32]) {
        const PI: f32 = std::f32::consts::PI;
        let r = corner_radii;

        let r0;
        let r1;
        let r2;
        let r3;

        if r.is_empty() {
            r0 = 0.0;
            r1 = 0.0;
            r2 = 0.0;
            r3 = 0.0;
        } else if r.len() == 1 {
            r0 = r[0];
            r1 = r[0];
            r2 = r[0];
            r3 = r[0];
        } else {
            assert!(r.len() == 4);
            r0 = if r[0] >= 0.5 { r[0] } else { 0.0 }; // top-left
            r1 = if r[1] >= 0.5 { r[1] } else { 0.0 }; // top-right
            r2 = if r[2] >= 0.5 { r[2] } else { 0.0 }; // bottom-right
            r3 = if r[3] >= 0.5 { r[3] } else { 0.0 }; // bottom-left
        }

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
        let chord_step = 2.0
            * (self.resolution.max(0.1) / (2.0 * radius))
                .clamp(-1.0, 1.0)
                .asin();

        // also cap angular step to avoid low-segment arcs at small radius
        let max_angle_step = 0.25; // ≈ 14° in radians
        let step_angle = chord_step.min(max_angle_step);

        // segment count from sweep / step, with a minimum
        let mut segments = (sweep_angle.abs() / step_angle).ceil() as usize;
        if segments < 4 {
            segments = 4;
        }

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

    pub fn dist_lin_uv(
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

flags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ItemFlags: u32 {
        const NONE = 0;
        const RAW = 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutlinePlacement {
    Outer,
    Center,
    Inner,
}

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

pub type TextItemCache = HashMap<TextItem, ShapedText>;

pub type FontId = u64;

pub struct FontTable {
    pub id_to_name: Vec<(FontId, String)>,
    pub sys: ctext::FontSystem,
}

impl FontTable {
    pub fn new() -> Self {
        Self {
            id_to_name: Default::default(),
            sys: ctext::FontSystem::new(),
        }
    }
    pub fn load_font(&mut self, name: &str, bytes: Vec<u8>) -> FontId {
        use hash::{Hash, Hasher};
        let db = self.sys.db_mut();
        let ids = db.load_font_source(ctext::fontdb::Source::Binary(std::sync::Arc::new(bytes)));
        let mut hasher = rustc_hash::FxHasher::default();
        ids.hash(&mut hasher);
        name.hash(&mut hasher);
        let id = hasher.finish();
        self.id_to_name.push((id, name.to_string()));
        id
    }

    pub fn get_font_attrib<'a>(&self, name: &'a str) -> ctext::Attrs<'a> {
        // let name = self.id_to_name.get(&id).unwrap();
        let attribs = ctext::Attrs::new()
            .family(ctext::Family::Name(name));
        attribs
    }
}

fn load_roboto_font(db: &mut ctext::fontdb::Database) -> ctext::fontdb::ID {
    let data = include_bytes!("../res/Roboto.ttf").to_vec(); 
    let ids = db.load_font_source(ctext::fontdb::Source::Binary(std::sync::Arc::new(data)));
    ids[0]
}

fn load_phosphor_font(db: &mut ctext::fontdb::Database) -> ctext::fontdb::ID {
    let data = include_bytes!("../res/Phosphor.ttf").to_vec(); 
    let ids = db.load_font_source(ctext::fontdb::Source::Binary(std::sync::Arc::new(data)));
    ids[0]
}

fn shape_text_item(itm: TextItem, fonts: &mut FontTable, cache: &mut GlyphCache, wgpu: &WGPU) -> ShapedText {
    let mut buffer = ctext::Buffer::new(
        &mut fonts.sys,
        ctext::Metrics {
            font_size: itm.font_size(),
            line_height: itm.line_height(),
        },
    );

    let font_attrib = fonts.get_font_attrib(itm.font);
    buffer.set_size(&mut fonts.sys, itm.width(), itm.height());
    buffer.set_text(&mut fonts.sys, &itm.string, &font_attrib, ctext::Shaping::Advanced);
    buffer.shape_until_scroll(&mut fonts.sys, false);

    let mut glyphs = Vec::new();
    let mut width = 0.0;
    let mut height = 0.0;

    for run in buffer.layout_runs() {
        width = run.line_w.max(width);
        // TODO[CHECK]: is it the sum?
        height += run.line_height;

        for g in run.glyphs {
            let g_phys = g.physical((0.0, 0.0), 1.0);
            let mut key = g_phys.cache_key;
            // TODO[CHECK]: what does this do
            key.x_bin = ctext::SubpixelBin::Three;
            key.y_bin = ctext::SubpixelBin::Three;

            if let Some(mut glyph) = cache.get_glyph(key, fonts, wgpu) {
                // let pos = Vec2::new(phys.x as f32, phys.y as f32 + run.line_y) + glyph.meta.pos;
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

pub struct GlyphCache {
    pub texture: gpu::Texture,
    pub alloc: etagere::BucketedAtlasAllocator,
    pub size: u32,
    pub cached_glyphs: FxHashMap<ctext::CacheKey, GlyphMeta>,
    pub swash_cache: ctext::SwashCache,
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
        let alloc =
            etagere::BucketedAtlasAllocator::new(etagere::Size::new(size as i32, size as i32));
        let texture = gpu::Texture::new(texture, texture_view);

        Self {
            texture,
            alloc,
            size,
            cached_glyphs: Default::default(),
            swash_cache: ctext::SwashCache::new(),
        }
    }

    pub fn get_glyph(&mut self, glyph_key: ctext::CacheKey, fonts: &mut FontTable, wgpu: &WGPU) -> Option<Glyph> {
        if let Some(&meta) = self.cached_glyphs.get(&glyph_key) {
            return Some(Glyph {
                texture: self.texture.clone(),
                meta,
            });
        }

        self.alloc_new_glyph(glyph_key, fonts, wgpu)
    }

    pub fn alloc_new_glyph(&mut self, glyph_key: ctext::CacheKey, fonts: &mut FontTable, wgpu: &WGPU) -> Option<Glyph> {
        let img = self.swash_cache.get_image_uncached(&mut fonts.sys, glyph_key)?;
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

        let rect = self.alloc.allocate(etagere::Size::new(w as i32, h as i32))?.rectangle;

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
        let uv_min =
            Vec2::new(rect.min.x as f32, rect.min.y as f32) / tex_size as f32;
        let uv_max = uv_min + size / tex_size as f32;

        let meta = GlyphMeta {
            pos,
            size,
            uv_min,
            uv_max,
        };
        self.cached_glyphs.insert(
            glyph_key,
            meta, 
        );

        Some(Glyph {
            texture: self.texture.clone(),
            meta,
        })
    }
}


pub struct MergedDrawLists {
    pub gpu_vertices: wgpu::Buffer,
    pub gpu_indices: wgpu::Buffer,

    pub draw_buffer: DrawBuffer,
    pub screen_size: Vec2,

    pub resolution: f32,
    pub antialias: bool,

    pub glyph_texture: gpu::Texture,

    pub wgpu: WGPUHandle,
}

// fn vtx(pos: impl Into<Vec2>, col: impl Into<RGBA>) -> Vertex {
//     Vertex {
//         pos: pos.into(),
//         col: col.into(),
//     }
// }

impl MergedDrawLists {
    /// 2^16
    pub const MAX_VERTEX_COUNT: u64 = 65_536;
    // 2^17
    pub const MAX_INDEX_COUNT: u64 = 131_072;

    pub fn new(glyph_texture: gpu::Texture, wgpu: WGPUHandle) -> Self {
        let mut font_db = ctext::fontdb::Database::new();
        font_db.load_font_data(include_bytes!("../res/Roboto.ttf").to_vec());
        // font_db.load_font_data(include_bytes!("CommitMono-400-Regular.otf").to_vec());
        // font_db.load_font_data(include_bytes!("CommitMono-500-Regular.otf").to_vec());
        let mut icon_font_db = ctext::fontdb::Database::new();
        icon_font_db.load_font_data(include_bytes!("../res/Phosphor.ttf").to_vec());

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
            resolution: 20.0,
            antialias: true,
            draw_buffer: DrawBuffer::new(
                Self::MAX_VERTEX_COUNT as usize,
                Self::MAX_INDEX_COUNT as usize,
            ),
            glyph_texture,
            wgpu,
        }
    }

    pub fn clear(&mut self) {
        self.draw_buffer.clear();
    }
}

impl RenderPassHandle for MergedDrawLists {
    const LABEL: &'static str = "draw_list_render_pass";

    fn n_render_passes(&self) -> u32 {
        self.draw_buffer.chunks.len() as u32
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        self.draw_multiple(rpass, wgpu, 0);
    }

    fn draw_multiple<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU, i: u32) {
        let proj =
            Mat4::orthographic_lh(0.0, self.screen_size.x, self.screen_size.y, 0.0, -1.0, 1.0);

        let global_uniform = ui_draw::GlobalUniform::new(self.screen_size, proj);

        let bind_group = ui_draw::build_bind_group(
            global_uniform,
            self.glyph_texture.view(),
            wgpu,
        );

        let (verts, indxs) = self.draw_buffer.get_chunk_data(i as usize).unwrap();

        wgpu.queue
            .write_buffer(&self.gpu_vertices, 0, bytemuck::cast_slice(verts));
        wgpu.queue
            .write_buffer(&self.gpu_indices, 0, bytemuck::cast_slice(indxs));

        rpass.set_bind_group(0, &bind_group, &[]);
        rpass.set_vertex_buffer(0, self.gpu_vertices.slice(..));
        rpass.set_index_buffer(self.gpu_indices.slice(..), wgpu::IndexFormat::Uint32);
        rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

        rpass.draw_indexed(0..indxs.len() as u32, 0, 0..1);
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

/// Represents a contiguous segment of vertex and index data
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawChunk {
    pub vtx_ptr: usize,
    pub idx_ptr: usize,
    pub n_vtx: usize,
    pub n_idx: usize,
}

/// A chunked buffer storing vertices and indices,
///
/// Allowing multiple render passes
/// when a single draw exceeds GPU limits or predefined chunk sizes.
#[derive(Debug, Clone)]
pub struct DrawBuffer {
    pub max_vtx_per_chunk: usize,
    pub max_idx_per_chunk: usize,
    pub vtx_alloc: Vec<Vertex>,
    pub idx_alloc: Vec<u32>,
    /// Current write offset in `vtx_alloc`.
    pub vtx_ptr: usize,
    /// Current write offset in `idx_alloc`.
    pub idx_ptr: usize,
    pub chunks: Vec<DrawChunk>,
}

impl Default for DrawBuffer {
    fn default() -> Self {
        // 2^16
        const MAX_VERTEX_COUNT: usize = 65_536;
        // 2^17
        const MAX_INDEX_COUNT: usize = 131_072;
        Self::new(MAX_VERTEX_COUNT, MAX_INDEX_COUNT)
    }
}

impl DrawBuffer {
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.vtx_ptr = 0;
        self.idx_ptr = 0;
    }

    pub fn new(max_vtx_per_chunk: usize, max_idx_per_chunk: usize) -> Self {
        Self {
            max_vtx_per_chunk,
            max_idx_per_chunk,
            vtx_alloc: vec![],
            idx_alloc: vec![],
            vtx_ptr: 0,
            idx_ptr: 0,
            chunks: vec![],
        }
    }

    pub fn get_chunk_data(&self, chunk_idx: usize) -> Option<(&[Vertex], &[u32])> {
        self.chunks.get(chunk_idx).map(|chunk| {
            let vtx_slice = &self.vtx_alloc[chunk.vtx_ptr..chunk.vtx_ptr + chunk.n_vtx];
            let idx_slice = &self.idx_alloc[chunk.idx_ptr..chunk.idx_ptr + chunk.n_idx];
            (vtx_slice, idx_slice)
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

        if self.chunks.is_empty() {
            self.chunks.push(DrawChunk {
                vtx_ptr: 0,
                idx_ptr: 0,
                n_vtx: 0,
                n_idx: 0,
            });
        }

        let c = *self.chunks.last().unwrap();

        if c.n_vtx + vtx.len() > self.max_vtx_per_chunk
            || c.n_idx + idx.len() > self.max_idx_per_chunk
        {
            self.chunks.push(DrawChunk {
                vtx_ptr: self.vtx_ptr,
                idx_ptr: self.idx_ptr,
                n_vtx: 0,
                n_idx: 0,
            });
        }

        let c = self.chunks.last_mut().unwrap();

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
}
