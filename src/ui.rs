use cosmic_text as ctext;
use glam::{Mat4, UVec2, Vec2};
use std::{
    cell::{Ref, RefCell},
    fmt, hash,
    rc::Rc,
};
use wgpu::util::DeviceExt;

use crate::{
    Vertex as VertexTyp,
    core::{
        ArrVec, Axis, DataMap, Dir, HashMap, HashSet, Instant, RGBA, id_type, stacked_fields_struct,
    },
    gpu::{self, RenderPassHandle, ShaderHandle, WGPU, WGPUHandle, Window, WindowId},
    mouse::{Clipboard, CursorIcon, MouseBtn, MouseState},
    rect::Rect,
};

// TODO[NOTE]: when docked there sometimes is a border a bit wider then it should be
// TODO[NOTE]: framepadding style?
// TODO[BUG]: stack overflow when resizing / maybe dragging scrollbar at the dock split? i think
// its happens when dragging the lower panel of a vertically splitted node. we try to dock an
// already docked node

fn load_window_icon() -> (u32, u32, Vec<u8>) {
    use image::imageops;
    let icon_bytes = include_bytes!("../res/icon3.png");
    let mut img = image::load_from_memory(icon_bytes).unwrap().into_rgba8();
    let img = imageops::resize(&img, 32, 32, imageops::FilterType::Lanczos3);
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    (width, height, rgba)
}

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
            SF::WindowTitlebarHeight => SV::WindowTitlebarHeight(40.0),
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
            SF::PanelDarkBg => SV::PanelDarkBg(RGBA::hex("#282c34")),
            SF::PanelCornerRadius => SV::PanelCornerRadius(7.0),
            SF::PanelOutline => SV::PanelOutline(Outline::center(dark, 2.0)),
            SF::PanelHoverOutline => SV::PanelHoverOutline(Outline::center(btn_hover, 2.0)),
            SF::ScrollbarWidth => SV::ScrollbarWidth(6.0),
            SF::ScrollbarPadding => SV::ScrollbarPadding(5.0),
            SF::PanelPadding => SV::PanelPadding(10.0),
            SF::SpacingV => SV::SpacingV(6.0),
            SF::SpacingH => SV::SpacingH(12.0),
            SF::Red => SV::Red(RGBA::hex("#e65858")),
        }
    })
}

pub struct Context {
    // pub panels: HashMap<Id, Panel>,
    pub panels: IdMap<Panel>,
    pub widget_data: DataMap<Id>,
    pub dock_tree: DockTree,
    // pub style: Style,
    pub style: StyleTable,

    pub current_panel_stack: Vec<Id>,
    pub current_panel_id: Id,
    pub draw_order: Vec<Id>,

    pub current_tabbar_id: Id,
    pub tabbars: IdMap<TabBar>,
    pub tabbar_count: u32,

    pub text_input_states: IdMap<TextInputState>,

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
    pub active_id_changed: bool,

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
    pub window_panel_id: Id,
    // pub window_panel_titlebar_height: f32,
    pub prev_hot_panel_id: Id,
    pub prev_active_panel_id: Id,

    // some items can only be interacted with while dragging, e.g. sliders
    // just holding down the mouse will not register as a drag, only a press
    // this flag signals that the current mouse press should be handled as a drag
    pub expect_drag: bool,

    pub clip_content: bool,
    pub draw_wireframe: bool,
    pub draw_clip_rect: bool,
    pub draw_content_outline: bool,
    pub draw_full_content_outline: bool,
    pub draw_item_outline: bool,
    pub draw_position_bounds: bool,

    pub circle_max_err: f32,

    pub frame_count: u64,
    pub prev_frame_time: Instant,

    pub mouse: MouseState,
    pub modifiers: winit::keyboard::ModifiersState,
    pub cursor_icon: CursorIcon,
    pub cursor_icon_changed: bool,
    pub resize_threshold: f32,
    pub undock_threshold: f32,
    pub scroll_speed: f32,
    pub n_draw_calls: usize,

    pub draw: MergedDrawLists,
    pub glyph_cache: RefCell<GlyphCache>,
    pub text_item_cache: RefCell<TextItemCache>,
    pub font_table: FontTable,
    pub icon_uv: Rect,

    pub close_pressed: bool,
    pub window: Window,
    pub requested_windows: Vec<(Vec2, Vec2)>,
    pub ext_window: Option<Window>,
    pub clipboard: Clipboard,
}

impl Context {
    pub fn new(wgpu: WGPUHandle, window: Window) -> Self {
        let mut font_table = FontTable::new();
        font_table.load_font(
            "Inter",
            include_bytes!("../res/Inter-VariableFont_opsz,wght.ttf").to_vec(),
        );
        font_table.load_font("Phosphor", include_bytes!("../res/Phosphor.ttf").to_vec());

        let mut glyph_cache = GlyphCache::new(&wgpu, font_table.clone());
        let icon_uv = {
            let (w, h, data) = load_window_icon();
            glyph_cache.alloc_data(w, h, &data, &wgpu).unwrap()
        };

        Self {
            panels: IdMap::new(),
            widget_data: DataMap::new(),
            dock_tree: DockTree::new(),
            // style: Style::dark(),
            style: dark_theme(),
            draw: MergedDrawLists::new(glyph_cache.texture.clone(), wgpu),
            current_panel_stack: vec![],

            current_tabbar_id: Id::NULL,
            tabbars: IdMap::new(),
            tabbar_count: 0,
            text_input_states: IdMap::new(),

            current_panel_id: Id::NULL,
            prev_item_data: PrevItemData::new(),

            hot_id: Id::NULL,
            hot_panel_id: Id::NULL,
            active_id: Id::NULL,
            active_id_changed: false,
            active_panel_id: Id::NULL,
            window_panel_id: Id::NULL,
            // window_panel_titlebar_height: 0.0,
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
            clip_content: true,
            draw_clip_rect: false,
            draw_content_outline: false,
            draw_full_content_outline: false,
            draw_item_outline: false,
            draw_position_bounds: false,
            circle_max_err: 0.3,

            frame_count: 0,
            prev_frame_time: Instant::now(),
            mouse: MouseState::new(),
            modifiers: winit::keyboard::ModifiersState::empty(),
            cursor_icon: CursorIcon::Default,
            cursor_icon_changed: false,
            resize_threshold: 5.0,
            undock_threshold: 50.0,
            scroll_speed: 1.0,
            n_draw_calls: 0,

            glyph_cache: RefCell::new(glyph_cache),
            text_item_cache: RefCell::new(TextItemCache::new()),
            font_table,
            icon_uv,

            close_pressed: false,
            window,
            requested_windows: Vec::new(),
            ext_window: None,
            clipboard: Clipboard::new(),
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
            self.window.set_cursor_icon(self.cursor_icon);
            self.cursor_icon_changed = false;
        }
    }

    pub fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if self.cursor_icon != icon {
            self.cursor_icon = icon;
            self.cursor_icon_changed = true;
        }
    }

    pub fn on_key_event(&mut self, key: &winit::event::KeyEvent) {
        use ctext::{Action, Edit, Motion, Selection};
        use winit::{
            event::ElementState,
            keyboard::{KeyCode, PhysicalKey},
        };

        let Some(input) = self.text_input_states.get_mut(self.active_id) else {
            return;
        };

        if !matches!(key.state, ElementState::Pressed) {
            return;
        }

        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        // let sys = &mut self.font_table.borrow_mut().sys;

        match key.physical_key {
            PhysicalKey::Code(KeyCode::ArrowRight) => {
                input.move_cursor_right(&self.modifiers);
            }
            PhysicalKey::Code(KeyCode::ArrowLeft) => {
                input.move_cursor_left(&self.modifiers);
            }
            PhysicalKey::Code(KeyCode::ArrowDown) => {
                input.move_cursor_down(&self.modifiers);
            }
            PhysicalKey::Code(KeyCode::ArrowUp) => {
                input.move_cursor_up(&self.modifiers);
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                input.backspace(&self.modifiers);
            }
            PhysicalKey::Code(KeyCode::KeyV) if ctrl => {
                if let Some(text) = self.clipboard.get_text() {
                    input.paste(&text);
                }
            }
            PhysicalKey::Code(KeyCode::KeyC) if ctrl => {
                if let Some(text) = input.copy_selection() {
                    self.clipboard.set_text(&text);
                }
            }
            PhysicalKey::Code(KeyCode::KeyX) if ctrl => {
                if let Some(text) = input.copy_selection() {
                    self.clipboard.set_text(&text);
                    input.delete_selection();
                }
            }
            PhysicalKey::Code(KeyCode::KeyA) if ctrl => {
                input.select_all();
            }
            PhysicalKey::Code(KeyCode::Delete) => input.delete(),
            PhysicalKey::Code(KeyCode::Enter) => input.enter(),
            _ => {
                if let Some(text) = &key.text {
                    input.paste(&text);
                }
            }
        }
    }

    // TODO[BUG]: scrolling on mousepad with two fingers upwards and one finger leaves the mousepad results
    // in a scroll upwards
    // TODO[NOTE]: we need acceleration (or maybe smoothing) when scrolling. or momentum
    pub fn set_mouse_scroll(&mut self, delta: Vec2) {
        let delta = delta * self.scroll_speed;
        let mut target = if !self.hot_panel_id.is_null() {
            &mut self.panels[self.hot_panel_id]
            // self.panels[self.hot_panel_id].move_scroll(delta * self.scroll_speed);
        } else if !self.active_panel_id.is_null() {
            &mut self.panels[self.active_panel_id]
            // &mut self.panels[self.active_id]
            // self.panels[self.active_panel_id].move_scroll(delta * self.scroll_speed);
            // self.panels[self.active_panel_id].scroll += delta * self.scroll_speed;
        } else {
            return;
        };

        // println!("{}", target.scrolling_past_bounds(delta));

        let mut parent = target.parent_id;
        if target.scrolling_past_bounds(delta) && !parent.is_null() {
            target = &mut self.panels[parent];
            parent = target.parent_id;
        }

        target.set_scroll(delta);
    }

    pub fn set_mouse_press(&mut self, btn: MouseBtn, press: bool) {
        self.mouse.set_button_press(btn, press);

        let w_size = self.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);

        let mut resize_dir = None;
        if !self.window.is_maximized() {
            resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold * 1.5);
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

        let resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold * 1.5);

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

    pub fn current_drawlist(&self) -> &DrawList {
        &self.get_current_panel().drawlist
    }

    pub fn current_drawlist_over(&self) -> &DrawList {
        &self.get_current_panel().drawlist_over
    }

    pub fn push_merged_clip_rect(&self, rect: Rect) {
        let list = &self.get_current_panel().drawlist;
        list.push_merged_clip_rect(rect);
    }

    pub fn push_clip_rect(&self, rect: Rect) {
        let list = &self.get_current_panel().drawlist;
        list.push_clip_rect(rect);
    }

    pub fn pop_clip_rect(&self) {
        let list = &self.get_current_panel().drawlist;
        list.pop_clip_rect();
    }

    pub fn draw(&self, itm: impl DrawableRects) -> &Self {
        let list = &self.get_current_panel().drawlist;
        itm.add_to_drawlist(list);
        self
    }

    pub fn draw_over(&self, itm: impl DrawableRects) -> &Self {
        let list = &self.get_current_panel().drawlist_over;
        itm.add_to_drawlist(list);
        self
    }

    // pub fn draw_over(&self, f: impl FnOnce(&mut DrawList)) {
    //     let p = self.get_current_panel();
    //     let draw_list = &p.draw_list_over;
    //     f(draw_list)
    // }

    // pub fn draw(&self, f: impl FnOnce(&mut DrawList)) {
    //     let p = self.get_current_panel();
    //     let draw_list = &mut p.draw_list.borrow_mut();
    //     f(draw_list)
    // }

    pub fn gen_id(&self, label: &str) -> Id {
        if self.current_panel_id.is_null() {
            Id::from_str(label)
        } else {
            self.get_current_panel().gen_id(label)
        }
    }

    pub fn begin(&mut self, name: impl Into<String>) {
        self.begin_ex(name, PanelFlags::DRAW_V_SCROLLBAR);
    }

    pub fn begin_ex(&mut self, name: impl Into<String>, flags: PanelFlags) {
        fn next_window_pos(screen: Vec2, panel_size: Vec2) -> Vec2 {
            static mut PANEL_COUNT: u32 = 1;
            let offset = 60.0;
            let size = if panel_size.is_finite() {
                panel_size
            } else {
                Vec2::new(500.0, 300.0)
            };

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
        let id = self.gen_id(&name);

        if !self.panels.contains_id(id) {
            self.create_panel(name);
            self.panels[id].id = id;
            newly_created = true;
        }

        // clear panels children every frame
        self.panels[id].children.clear();

        // setup child / parent ids
        let (root_id, parent_id) = if flags.has(PanelFlags::IS_CHILD) {
            let parent_id = self.current_panel_id;
            let parent = &mut self.panels[parent_id];
            let root = parent.root;
            parent.children.push(id);

            (root, parent_id)
        } else {
            (id, Id::NULL)
        };

        if newly_created {
            if flags.has(PanelFlags::USE_PARENT_DRAWLIST) {
                let parent = &self.panels[parent_id];
                let draw_list = parent.drawlist.clone();
                let draw_list_over = parent.drawlist_over.clone();
                let p = &mut self.panels[id];
                p.drawlist = draw_list;
                p.drawlist_over = draw_list_over;
            }

            // only push to draw list once when created
            // persists across frames
            let p = &mut self.panels[id];
            p.draw_order = self.draw_order.len();
            self.draw_order.push(id);

            if self.next.pos.is_nan() {
                p.pos = next_window_pos(self.draw.screen_size, self.next.size);
            }
        }

        self.current_panel_stack.push(id);
        self.current_panel_id = id;

        let p = &mut self.panels[id];

        if self.next.pos.x.is_finite() {
            p.pos.x = self.next.pos.x;
        }
        if self.next.pos.y.is_finite() {
            p.pos.y = self.next.pos.y;
        }

        // reset temp data
        if !flags.has(PanelFlags::USE_PARENT_DRAWLIST) {
            if !p.drawlist.data.borrow().clip_stack.is_empty() {
                log::error!("clip rect stack not empty");
            }
            p.drawlist.clear();
            p.drawlist_over.clear();
        }

        p.root = root_id;
        p.parent_id = parent_id;

        let is_window = id == self.window_panel_id;

        assert!(p.id == id);
        // TODO[CHECK]:
        p.push_id(p.id);
        p.flags = flags;
        p.explicit_size = self.next.size;
        p.drawlist.data.borrow_mut().circle_max_err = self.circle_max_err;
        p.drawlist.draw_clip_rect = self.draw_clip_rect;
        p.titlebar_height = if flags.has(PanelFlags::NO_TITLEBAR) {
            0.0
        } else if is_window {
            self.style.window_titlebar_height()
        } else {
            self.style.titlebar_height()
        };

        p.padding = self.style.panel_padding();
        p.scrollbar_width = self.style.scrollbar_width();
        p.scrollbar_padding = self.style.scrollbar_padding();
        p.last_frame_used = self.frame_count;
        p.move_id = p.gen_id("##_MOVE");
        p.drawlist.data.borrow_mut().clip_content = self.clip_content;

        // p.scroll = p.next_scroll;
        p.scroll = p.next_scroll.min(p.scroll_max()).max(p.scroll_min());
        p.next_scroll = p.scroll;
        // if !self.panel_action.is_scroll() {
        //     let scroll_min = p.scroll_min();
        //     let scroll_max = p.scroll_max();
        //     p.scroll = p.scroll.min(scroll_max).max(scroll_min);
        // }

        p.min_size = self.next.min_size;
        p.max_size = self.next.max_size;

        if flags.has(PanelFlags::NO_MOVE) {
            p.move_id = Id::NULL;
        } else if flags.has(PanelFlags::NO_TITLEBAR) {
            // move the window by dragging it if no titlebar exists
            p.titlebar_height = 0.0;
        }

        self.next.reset();
        // if !p.flags.has(PanelFlags::ONLY_MOVE_FROM_TITLEBAR) {
        //     p.nav_root = p.move_id;
        // } else {
        //     p.nav_root = p.root;
        // }

        let (pos_bounds, clamp_pos_bounds) = if flags.has(PanelFlags::IS_CHILD) {
            let p = &self.panels[parent_id];
            (p.full_content_rect().translate(p.scroll), true)
        } else {
            let bounds = if is_window {
                // p.visible_content_rect()
                p.visible_content_rect().expand(p.padding)
            } else {
                let p = &self.panels[self.window_panel_id];
                p.visible_content_rect().expand(p.padding)
            };
            (bounds, false)
        };

        let p = &mut self.panels[id];
        p.position_bounds = pos_bounds;
        p.clamp_position_to_bounds = clamp_pos_bounds;

        if !is_window {
            let tb_height = p.titlebar_height;
            // p.pos.y = p.pos.y.max(height);
            let screen = self.draw.screen_size;

            let thr = self.resize_threshold;
            // p.pos.x = p.pos.x.max(pos_bounds.left() - p.size.x + thr).min(pos_bounds.top() - tb_height);
            // p.pos.x = p.pos.x.max(-p.size.x + tb_height).min(screen.x - tb_height);

            if p.dock_id.is_null() {
                p.pos.x = p
                    .pos
                    .x
                    .max(-p.size.x + pos_bounds.left() + tb_height)
                    .min(pos_bounds.right() - tb_height);
                p.pos.y = p
                    .pos
                    .y
                    // .max(self.style.window_titlebar_height())
                    .max(pos_bounds.top())
                    .min(pos_bounds.bottom() - tb_height);
            } else {
                let dock_root = self.dock_tree.get_root(p.dock_id);
                let dock_rect = self.dock_tree.nodes[dock_root].rect;

                let mut pos = dock_rect.min;
                let size = dock_rect.size();
                pos.x = pos
                    .x
                    .max(-size.x + pos_bounds.left() + tb_height)
                    .min(pos_bounds.right() - tb_height);
                pos.y = pos
                    .y
                    .max(pos_bounds.top())
                    .min(pos_bounds.bottom() - tb_height);

                if pos != dock_rect.min {
                    self.dock_tree
                        .recompute_rects(dock_root, Rect::from_min_size(pos, size));
                }
            }
        }

        let p = &mut self.panels[id];

        let prev_max_pos = p.cursor_max_pos();
        let prev_content_start = p.content_start_pos();

        p.init_content_cursor(p.visible_content_start_pos());

        // TODO[NOTE]: how do we design outline on hover? maybe just highlight border that can be
        // resized
        // let outline = if p.id == self.prev_hot_panel_id || p.id == self.active_panel_id {
        //     self.style.panel_hover_outline()
        // } else {
        //     self.style.panel_outline()
        // };
        let panel_outline = self.style.panel_outline();

        p.outline_offset = panel_outline.offset();

        let corner_radii = if p.dock_id.is_null() {
            CornerRadii::all(self.style.panel_corner_radius())
        } else {
            let [n_n, n_e, n_s, n_w] = self
                .dock_tree
                .get_neighbors(p.dock_id)
                .map(|n| !n.is_null());
            let [tl, tr, br, bl] = [
                (n_n, n_w), // tl
                (n_n, n_e), // tr
                (n_s, n_e), // br
                (n_s, n_w), // bl
            ]
            .map(|(n1, n2)| {
                if !(n1 || n2) {
                    self.style.panel_corner_radius()
                } else {
                    0.0
                }
            });

            CornerRadii::new(tl, tr, bl, br)
        };

        // preserve when?
        // p.full_content_size = prev_max_pos - prev_content_start;

        // p.full_size =
        //     prev_max_pos - p.pos + Vec2::splat(p.padding); // + Vec2::splat(outline.offset()) * 2.0;

        // // TODO[NOTE]: is it possible to get size from only 1 frame?
        // // or configurable
        // if self.frame_count - p.frame_created <= 2 {
        //     // p.size = p.full_size * 1.1;
        //     // TODO[NOTE]: account for scrollbar width?
        //     p.size = p.full_size + p.padding + self.style.scrollbar_padding();
        // }

        let panel_pos = p.pos;

        // bg
        let panel_size = if p.explicit_size.is_finite() {
            p.explicit_size
        } else {
            p.size
        };

        p.size = panel_size.min(p.panel_max_size()).max(p.panel_min_size());

        if !p.dock_id.is_null() {
            // override pos and size if docked
            let dock_rect = self.dock_tree.nodes[p.dock_id].rect;
            // p.pos = dock_rect.min;
            p.move_panel_to(dock_rect.min);
            p.size = dock_rect.size();
        }

        let outline_width = self.style.panel_outline().width;
        let full_rect = Rect::from_min_size(p.pos - outline_width, p.size + 2.0 * outline_width);
        let mut clip_rect = p.full_rect;

        if flags.has(PanelFlags::USE_PARENT_CLIP) {
            let clip = p.drawlist.current_clip_rect();
            clip_rect = p.full_rect.intersect(clip);
        }

        p.full_rect = full_rect;
        p.clip_rect = clip_rect;

        let p = &self.panels[id];
        // let panel_rect = p.panel_rect();

        if p.clip_rect.contains(self.mouse.pos)
            && (self.hot_panel_id.is_null()
                || self.panels[self.hot_panel_id].draw_order < p.draw_order)
            && self.panel_action.is_none()
            && !p.flags.has(PanelFlags::NO_FOCUS)
        {
            self.hot_panel_id = id;
            self.hot_id = id;
        }

        if let PanelAction::Move {
            id,
            dock_target,
            cancelled_docking,
            drag_by_titlebar,
            ..
        } = &mut self.panel_action
        {
            if p.clip_rect.contains(self.mouse.pos)
                // && self.panels[*id].titlebar_rect().contains(self.mouse.pos)
                && *drag_by_titlebar
                && !self.modifiers.shift_key()
            {
                let curr_draw_order = p.draw_order;
                let moving_draw_order = self.panels[*id].draw_order;
                let dock_target_draw_order = if !dock_target.is_null() {
                    self.panels[*dock_target].draw_order
                } else {
                    0
                };

                if !flags.has(PanelFlags::NO_DOCKING)
                    && curr_draw_order < moving_draw_order
                    && (curr_draw_order > dock_target_draw_order || dock_target.is_null())
                    && !*cancelled_docking
                {
                    // gets reset in update_panel_dock
                    *dock_target = p.id;
                }
            }
        }

        // let p = &self.panels[id];

        // TODO[NOTE]: include outline width in panel size?
        // draw panel

        if self.draw_position_bounds {
            let bounds = p.position_bounds;
            self.push_clip_rect(bounds);
            self.draw_over(bounds.draw_rect().outline(Outline::new(RGBA::GREEN, 2.0)));
            self.pop_clip_rect();
        }

        // draw background
        let bg_fill = if p.is_window_panel {
            self.style.window_bg()
        } else {
            self.style.panel_bg()
        };

        // self.draw(|list| {
        // panel clip rectangle
        // let rect = p.panel_rect();
        let mut clip = p.panel_rect_with_outline();
        clip.min = clip.min.floor();
        clip.max = clip.max.ceil();

        if flags.has(PanelFlags::USE_PARENT_CLIP) {
            self.push_merged_clip_rect(clip);
        } else {
            self.push_clip_rect(clip);
        }

        self.draw(
            p.panel_rect()
                .draw_rect()
                .fill(bg_fill)
                // .outline(panel_outline)
                .corners(corner_radii),
        );

        if self.draw_content_outline {
            self.draw_over(
                p.visible_content_rect()
                    .draw_rect()
                    .outline(Outline::new(RGBA::GREEN, 2.0)),
            );
        }

        if self.draw_full_content_outline {
            self.draw_over(
                p.full_content_rect()
                    .draw_rect()
                    .outline(Outline::new(RGBA::BLUE, 2.0)),
            );
        }
        // let p = &self.panels[id];
        if !p.flags.has(PanelFlags::NO_TITLEBAR) {
            let titlebar_height = p.titlebar_height;
            let p_pos = p.pos;
            let (tb, min, max, close, min_width) = if p.id == self.window_panel_id {
                self.draw_panel_decorations(false, true, true, true, CornerRadii::zero())
            } else {
                let draw_title_handle = !p.dock_id.is_null();
                self.draw_panel_decorations(draw_title_handle, false, false, true, corner_radii)
            };

            self.panels[id].title_handle_rect =
                Rect::from_min_size(p_pos, Vec2::new(min_width, titlebar_height));

            if id == self.window_panel_id {
                if min.released() {
                    self.window.minimize();
                }
                if max.released() || tb.double_clicked() {
                    self.window.toggle_maximize();
                }
                if close.released() {
                    self.close_pressed = true;
                }

                let pad = 5.0;
                self.draw(
                    Rect::from_min_max(Vec2::splat(pad), Vec2::splat(titlebar_height - pad))
                        .draw_rect()
                        .uv(self.icon_uv.min, self.icon_uv.max)
                        .texture(1),
                );
                // self.draw(|list| {
                //     list.rect(Vec2::splat(pad), Vec2::splat(titlebar_height - pad))
                //         .texture_uv(self.icon_uv.min, self.icon_uv.max, 1)
                //         .add()
                // });
            }

            // start drawing content
            self.set_cursor_pos(self.content_start_pos());
            self.prev_item_data.reset();
        }

        // draw panel outline last
        let p = &self.panels[id];
        self.draw(
            p.panel_rect()
                .draw_rect()
                .corners(corner_radii)
                .outline(panel_outline),
        );

        // draw scrollbar
        let (x_scroll, y_scroll) = p.needs_scrollbars();
        if y_scroll && flags.has(PanelFlags::DRAW_V_SCROLLBAR) {
            self.draw_scrollbar(1);
        }
        if x_scroll && flags.has(PanelFlags::DRAW_H_SCROLLBAR) {
            self.draw_scrollbar(0);
        }

        let p = &self.panels[id];

        if flags.has(PanelFlags::USE_PARENT_CLIP) {
            self.push_merged_clip_rect(p.visible_content_rect());
        } else {
            self.push_clip_rect(p.visible_content_rect());
        }
    }

    fn draw_scrollbar(&mut self, axis: usize) {
        let other_axis = 1 - axis;
        let p = &self.get_current_panel();
        let content = p.visible_content_rect();
        let full_content = p.full_content_rect();
        let scrollbar_width = p.scrollbar_width;

        let view_size = content.size()[axis];
        let full_size = full_content.size()[axis].max(1.0);

        // Only show if content is scrollable
        if full_size <= view_size {
            return;
        }

        let track_size = view_size;
        let handle_size = ((view_size / full_size) * track_size).max(scrollbar_width);
        let scrollable = full_size - view_size;
        let track_move = (track_size - handle_size).max(1.0);

        // Calculate thumb position (scroll is negative when scrolled)
        let offset = (-p.scroll[axis]).clamp(0.0, scrollable);
        let thumb_pos = if scrollable > 0.0 {
            content.min[axis] + (offset / scrollable) * track_move
        } else {
            content.min[axis]
        };

        let scroll_id = self.gen_id(&format!("##_SCROLLBAR_{}", axis));
        let scroll_pad = p.padding / 2.0 + p.scrollbar_padding / 2.0;

        let (min, max) = if axis == 1 {
            // Vertical scrollbar (Y axis)
            let min = Vec2::new(content.max.x + scroll_pad, thumb_pos);
            let max = min + Vec2::new(scrollbar_width, handle_size);
            (min, max)
        } else {
            // Horizontal scrollbar (X axis)
            let min = Vec2::new(thumb_pos, content.max.y + scroll_pad);
            let max = min + Vec2::new(handle_size, scrollbar_width);
            (min, max)
        };

        let scrollbar_rect = Rect::from_min_max(min, max);

        // handle panel action
        let sig = self.register_rect(scroll_id, scrollbar_rect);
        let p = &self.panels[self.current_panel_id];
        if (sig.pressed() || sig.dragging()) && self.panel_action.is_none() {
            if sig.pressed() && !sig.dragging() {
                self.expect_drag = true;
            }

            let offset = self.mouse.pos - min;
            let scroll_rect = Rect::from_min_max(p.scroll_min(), p.scroll_max());

            self.panel_action = PanelAction::Scroll {
                axis: axis,
                id: p.id,
                start_scroll: p.scroll,
                press_offset: offset,
                scroll_rect,
            };
        } else if self.panel_action.is_scroll() && !self.mouse.pressed(MouseBtn::Left) {
            self.panel_action = PanelAction::None;
        }

        let is_scrolling = if let PanelAction::Scroll {
            id,
            axis: curr_axis,
            ..
        } = self.panel_action
        {
            id == p.id && axis == curr_axis
        } else {
            false
        };

        // draw
        let handle_col = if sig.pressed() || is_scrolling {
            self.style.btn_press()
        } else if sig.hovering() {
            self.style.btn_hover()
        } else {
            self.style.panel_dark_bg()
        };

        self.draw(
            Rect::from_min_max(min, max)
                .draw_rect()
                .corners(scrollbar_width * 0.3)
                .fill(handle_col),
        );
    }

    pub fn draw_panel_decorations(
        &mut self,
        draw_title_handle: bool,
        minimize: bool,
        maximize: bool,
        close: bool,
        panel_corners: CornerRadii,
    ) -> (Signal, Signal, Signal, Signal, f32) {
        let p = self.get_current_panel();
        let titlebar_height = p.titlebar_height;
        let panel_pos = p.pos;
        let panel_size = p.size;
        let title = p.name.clone();
        let move_id = p.move_id;

        let title_text = self.layout_text(&title, self.style.text_size());
        let pad = (titlebar_height - title_text.height) / 2.0;
        // draw titlebar background
        let mut tb_corners = panel_corners;
        tb_corners.bl = 0.0;
        tb_corners.br = 0.0;

        let min_width = title_text.size().x + pad * 2.0;
        self.draw(
            Rect::from_min_size(panel_pos, Vec2::new(panel_size.x, titlebar_height))
                .draw_rect()
                .fill(self.style.titlebar_color())
                .corners(tb_corners),
        );

        if draw_title_handle {
            self.draw(
                Rect::from_min_size(panel_pos, title_text.size() + Vec2::splat(pad * 2.0))
                    .draw_rect()
                    .corners(CornerRadii::top(self.style.panel_corner_radius()))
                    .fill(self.style.panel_bg()),
            );
        }

        self.draw(title_text.draw_rects(panel_pos + pad, self.style.text_col()));

        let tb_sig = self.register_rect(
            move_id,
            Rect::from_min_size(panel_pos, Vec2::new(panel_size.x, titlebar_height)),
        );

        let btn_size = Vec2::new(25.0, 25.0);
        let btn_spacing = 10.0;
        let mut btn_x = panel_size.x - (btn_size.x + btn_spacing);
        let btn_y = (titlebar_height - btn_size.y) / 2.0;

        let mut min_sig = Signal::NONE;
        let mut max_sig = Signal::NONE;
        let mut close_sig = Signal::NONE;

        // draw close button
        if close {
            let close_id = self.gen_id("##_CLOSE_ICON");
            let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
            close_sig = self.register_rect(close_id, Rect::from_min_size(btn_pos, btn_size));

            let color = if close_sig.hovering() {
                self.style.red()
            } else {
                RGBA::WHITE
            };

            let x_icon = self.layout_icon(phosphor_font::X, self.style.text_size());
            let pad = btn_size - x_icon.size();
            let pos = btn_pos + pad / 2.0;
            self.draw(x_icon.draw_rects(pos, color));
            btn_x -= btn_size.x + btn_spacing;
        }

        // draw maximize button
        if maximize {
            let max_id = self.gen_id("##_MAX_ICON");
            let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
            max_sig = self.register_rect(max_id, Rect::from_min_size(btn_pos, btn_size));

            let color = if max_sig.hovering() {
                self.style.btn_hover()
            } else {
                self.style.text_col()
            };

            {
                let max_icon = if self.window.is_maximized() {
                    self.layout_icon(phosphor_font::MAXIMIZE_OFF, self.style.text_size())
                } else {
                    self.layout_icon(phosphor_font::MAXIMIZE, self.style.text_size())
                };
                let pad = btn_size - max_icon.size();
                let pos = btn_pos + pad / 2.0;
                self.draw(max_icon.draw_rects(pos, color));
                // list.add_text(pos, &max_icon, color);
            }

            btn_x -= btn_size.x + btn_spacing;
        }

        // draw minimize button
        if minimize {
            let min_id = self.gen_id("##_MIN_ICON");
            let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
            min_sig = self.register_rect(min_id, Rect::from_min_size(btn_pos, btn_size));

            let color = if min_sig.hovering() {
                self.style.btn_hover()
            } else {
                self.style.text_col()
            };

            let min_icon = self.layout_icon(phosphor_font::MINIMIZE, self.style.text_size());
            let pad = btn_size - min_icon.size();
            let pos = btn_pos + pad / 2.0;
            self.draw(min_icon.draw_rects(pos, color));
        }

        (tb_sig, min_sig, max_sig, close_sig, min_width)
    }

    pub fn update_panel_scroll(&mut self) {
        let PanelAction::Scroll {
            id,
            start_scroll,
            press_offset,
            scroll_rect,
            axis,
        } = self.panel_action
        else {
            return;
        };

        if !self.mouse.pressed(MouseBtn::Left) {
            self.panel_action = PanelAction::None;
            return;
        }

        let p = &mut self.panels[id];
        let content = p.visible_content_rect();
        let full_content = p.full_content_rect();

        let view_size = content.size()[axis];
        let full_size = full_content.size()[axis].max(1.0);
        let track_size = view_size;
        let handle_size = ((view_size / full_size) * track_size).max(p.scrollbar_width);
        let scrollable = (full_size - view_size).max(0.0);
        let track_move = (track_size - handle_size).max(1.0);

        // Compute thumb position from current mouse pos while respecting press_offset
        let thumb_pos_unclamped = self.mouse.pos[axis] - press_offset[axis];
        let thumb_pos = thumb_pos_unclamped
            .max(content.min[axis])
            .min(content.min[axis] + track_move);

        // Convert thumb position to content offset
        let new_scroll = if scrollable > 0.0 {
            let offset = ((thumb_pos - content.min[axis]) / track_move) * scrollable;
            -offset // scroll is negative when scrolled
        } else {
            0.0
        };

        // Set new scroll (keep other axis from start_scroll)
        let mut scroll = start_scroll;
        scroll[axis] = new_scroll.round();
        // p.set_scroll(scroll);
        p.next_scroll = scroll;

        // let scroll_min = p.scroll_min();
        // let scroll_max = p.scroll_max();

        // p.next_scroll = scroll.min(scroll_max).max(scroll_min);
    }

    pub fn update_panel_resize(&mut self) {
        // check if we should start resize action
        if let Some(p) = self.panels.get_mut(self.hot_panel_id) {
            let id = p.id;
            let rect = p.panel_rect();
            // let rect = if p.dock_id.is_null() {
            //     p.panel_rect()
            // } else {
            //     let dock_root = self.dock_tree.get_root(p.dock_id);
            //     self.dock_tree.nodes[dock_root].rect
            // };

            let dir = is_in_resize_region(rect, self.mouse.pos, self.resize_threshold);

            let (can_resize_in_dir, is_split, prev_rect) = if dir.is_none() {
                (false, false, rect)
            } else if !p.dock_id.is_null() {
                let [n_n, n_e, n_s, n_w] = self
                    .dock_tree
                    .get_neighbors(p.dock_id)
                    .map(|n| !n.is_null());

                let dock_root = self.dock_tree.get_root(p.dock_id);
                let dock_root_rect = self.dock_tree.nodes[dock_root].rect;

                match dir.unwrap() {
                    Dir::NW => (!(n_n || n_w), false, dock_root_rect),
                    Dir::NE => (!(n_n || n_e), false, dock_root_rect),
                    Dir::SW => (!(n_s || n_w), false, dock_root_rect),
                    Dir::SE => (!(n_s || n_e), false, dock_root_rect),
                    // _ => true,
                    Dir::N => (true, n_n, if !n_n { dock_root_rect } else { rect }),
                    Dir::E => (true, n_e, if !n_e { dock_root_rect } else { rect }),
                    Dir::S => (true, n_s, if !n_s { dock_root_rect } else { rect }),
                    Dir::W => (true, n_w, if !n_w { dock_root_rect } else { rect }),
                }
            } else {
                (true, false, rect)
            };

            if can_resize_in_dir && self.panel_action.is_none() && !p.is_window_panel
            // && !(p.flags.has(PanelFlags::NO_RESIZE) || p.is_window_panel)
            {
                let dir = dir.unwrap();
                let dock_id = p.dock_id;
                self.set_cursor_icon(dir.as_cursor());

                // we check that we are not dragging so that if one starts dragging the mouse and
                // then go over the panel border it should not trigger a resize action
                if self.mouse.pressed(MouseBtn::Left) && !self.mouse.dragging(MouseBtn::Left) {
                    self.expect_drag = true;
                    if is_split {
                        let split_dock_id = self.dock_tree.get_split_node(dock_id, dir);
                        assert!(!split_dock_id.is_null());
                        let DockNodeKind::Split { ratio, .. } =
                            self.dock_tree.nodes[split_dock_id].kind
                        else {
                            unreachable!()
                        };

                        self.panel_action = PanelAction::DragSplit {
                            dir,
                            dock_split_id: split_dock_id,
                            prev_ratio: ratio,
                        };
                    } else {
                        self.panel_action = PanelAction::Resize { dir, id, prev_rect };
                    }
                }
            }
        }

        if let PanelAction::DragSplit {
            dir,
            dock_split_id,
            prev_ratio,
        } = &self.panel_action
        {
            if !self.mouse.pressed(MouseBtn::Left) {
                self.set_cursor_icon(CursorIcon::Default);
                self.panel_action = PanelAction::None;
                return;
            }

            let axis = dir.axis().unwrap();

            let m_start = self
                .mouse
                .drag_start(MouseBtn::Left)
                .unwrap_or(self.mouse.pos);
            let m_delta = (self.mouse.pos - m_start)[axis as usize];

            let dock_split = &self.dock_tree.nodes[*dock_split_id];
            let split_rect = dock_split.rect;
            let split_size = split_rect.size()[axis as usize];
            let DockNodeKind::Split {
                children, ratio, ..
            } = dock_split.kind
            else {
                panic!()
            };

            let split_range = self.dock_tree.get_split_range(*dock_split_id);
            let tb_height = self.style.titlebar_height() + 5.0;

            let prev_ratio_px = prev_ratio * split_size;
            let new_ratio_px = prev_ratio_px + m_delta;
            let new_ratio = new_ratio_px.min(split_range - tb_height).max(tb_height) / split_size;


            for i in 0..=1 {
                let sibling = &mut self.dock_tree.nodes[children[i]];
                if let DockNodeKind::Split {
                    ratio: sibling_ratio,
                    axis: sibling_axis,
                    ..
                } = &mut sibling.kind
                {
                    if axis != *sibling_axis {
                        continue;
                    }

                    let i_f = i as f32;
                    let sibling_size = sibling.rect.size()[axis as usize];
                    let sibling_ratio_px = (i_f - *sibling_ratio) * sibling_size;

                    let new_sibling_size = (i_f - new_ratio) * split_size;

                    let new_sibling_ratio = (i_f - sibling_ratio_px / new_sibling_size);
                    *sibling_ratio = new_sibling_ratio;
                }
            }

            let DockNodeKind::Split {
                children, ratio, ..
            } = &mut self.dock_tree.nodes[*dock_split_id].kind
            else {
                panic!()
            };

            *ratio = new_ratio;
            self.dock_tree.recompute_rects(*dock_split_id, split_rect);
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

            // TODO[NOTE]: if docked should maybe compute min docked tree size?
            let min_size = p.panel_min_size();
            let max_size = p.panel_max_size();

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

            if p.dock_id.is_null() {
                p.move_panel_to(nr.min);
                p.size = nr.size();
            } else {
                let dock_root = self.dock_tree.get_root(p.dock_id);
                self.dock_tree.recompute_rects(dock_root, nr);
            }
        }
    }

    pub fn get_dock_target(mouse: Vec2, target_area: Rect) -> (Rect, Dir, f32) {
        let mut dock_target = target_area;
        let mut delta = (mouse - target_area.center()) / target_area.size() * 2.0;
        delta.x = delta.x.clamp(-1.0, 1.0);
        delta.y = delta.y.clamp(-1.0, 1.0);

        let use_horizontal = delta.x.abs() >= delta.y.abs();
        let min_px = 8.0_f32;

        let dir: Dir;
        let mut ratio: f32;

        if use_horizontal {
            dir = if delta.x >= 0.0 { Dir::E } else { Dir::W };
            let desired_width = (1.0 - delta.x.abs()) * target_area.width();
            let clamped_width = desired_width.clamp(min_px, target_area.width());
            ratio = clamped_width / target_area.width();
        } else {
            dir = if delta.y >= 0.0 { Dir::S } else { Dir::N };
            let desired_height = (1.0 - delta.y.abs()) * target_area.height();
            let clamped_height = desired_height.clamp(min_px, target_area.height());
            ratio = clamped_height / target_area.height();
        }

        if (ratio - 0.5).abs() < 0.06 {
            ratio = 0.5;
        }

        if ratio > 0.90 {
            ratio = 0.90;
        }

        if use_horizontal {
            let width = ratio * target_area.width();
            if dir == Dir::E {
                let right = target_area.right();
                let left = right - width;
                dock_target.set_left(left);
                dock_target.set_right(right);
            } else {
                let left = target_area.left();
                let right = left + width;
                dock_target.set_left(left);
                dock_target.set_right(right);
            }
        } else {
            let height = ratio * target_area.height();
            if dir == Dir::S {
                let bottom = target_area.bottom();
                let top = bottom - height;
                dock_target.set_top(top);
                dock_target.set_bottom(bottom);
            } else {
                let top = target_area.top();
                let bottom = top + height;
                dock_target.set_top(top);
                dock_target.set_bottom(bottom);
            }
        }

        (dock_target, dir, ratio)
    }

    // pub fn get_dock_target2(mouse: Vec2, target_area: Rect) -> (Rect, Dir, f32) {
    //     let mut dock_target = target_area;
    //     let mut delta = (mouse - dock_target.center()) / dock_target.size() * 2.0;
    //     delta.x = delta.x.clamp(-1.0, 1.0);
    //     delta.y = delta.y.clamp(-1.0, 1.0);

    //     // pick dominant axis
    //     let use_horizontal = delta.x.abs() >= delta.y.abs();

    //     // minimum preview size to avoid inversion
    //     let min_px = 8.0_f32;

    //     let dir: Dir;
    //     let mut ratio: f32;

    //     if use_horizontal {
    //         let right;
    //         let left;

    //         if delta.x >= 0.0 {
    //             right = dock_target.right();
    //             left = dock_target.right() - (1.0 - delta.x) * dock_target.width();
    //             dir = Dir::E;
    //         } else {
    //             right = dock_target.left() + (delta.x + 1.0) * dock_target.width();
    //             left = dock_target.left();
    //             dir = Dir::W;
    //         }

    //         let left = left.clamp(dock_target.left(), dock_target.right() - min_px);
    //         let right = right.clamp(dock_target.left() + min_px, dock_target.right());

    //         dock_target.set_left(left);
    //         dock_target.set_right(right);

    //         ratio = dock_target.width() / target_area.width();
    //     } else {
    //         let bottom;
    //         let top;

    //         if delta.y >= 0.0 {
    //             bottom = dock_target.bottom();
    //             top = dock_target.bottom() - (1.0 - delta.y) * dock_target.height();
    //             dir = Dir::S;
    //         } else {
    //             bottom = dock_target.top() + (delta.y + 1.0) * dock_target.height();
    //             top = dock_target.top();
    //             dir = Dir::N;
    //         }

    //         let top = top.clamp(dock_target.top(), dock_target.bottom() - min_px);
    //         let bottom = bottom.clamp(dock_target.top() + min_px, dock_target.bottom());

    //         dock_target.set_top(top);
    //         dock_target.set_bottom(bottom);

    //         ratio = dock_target.height() / target_area.height();
    //     }

    //     println!("{ratio}");
    //     if (ratio - 0.5).abs() < 0.1 {
    //         ratio = 0.5;
    //     }

    //     (dock_target, dir, ratio)
    // }

    pub fn update_panel_dock(&mut self) {
        // check if we should dock the panel and stop move action
        let PanelAction::Move {
            id,
            dock_target,
            drag_by_titlebar: drag_tb,
            drag_by_title_handle: drag_th,
            cancelled_docking,
            ..
        } = self.panel_action
        else {
            return;
        };

        let p_dock_id = self.panels[id].dock_id;
        let can_dock = !dock_target.is_null()
            && !cancelled_docking
            // if already docked the tree can be docked if we drag the titlebar not the titlehandle
            && (p_dock_id.is_null() && drag_tb || !p_dock_id.is_null() && (drag_tb && !drag_th));

        if !self.mouse.pressed(MouseBtn::Left) {
            if can_dock {
                // && !dock_target.is_null()
                // if !self.panels[id].dock_id.is_null() {
                //     log::warn!("docking with panel that is already docked");
                // }
                let curr_size = self.panels[id].size;
                self.panels[id].size_pre_dock = curr_size;
                let curr_dock_id = self.panels[id].dock_id;

                let target_panel = &mut self.panels[dock_target];

                if target_panel.dock_id.is_null() {
                    // init target panel as dock node
                    target_panel.size_pre_dock = target_panel.size;
                    let dock_id = self
                        .dock_tree
                        .add_root(target_panel.full_rect, target_panel.id);
                    target_panel.dock_id = dock_id;
                }

                let (preview, dir, ratio) =
                    Self::get_dock_target(self.mouse.pos, target_panel.panel_rect());

                if !curr_dock_id.is_null() {
                    let root_dock_id = self.dock_tree.get_root(curr_dock_id);
                    let id =
                        self.dock_tree
                            .merge_nodes(target_panel.dock_id, root_dock_id, ratio, dir);
                    target_panel.dock_id = id;
                } else {
                    let (l, r) = self.dock_tree.split_node2(target_panel.dock_id, ratio, dir);

                    target_panel.dock_id = l;
                    self.dock_tree.nodes[l].panel_id = target_panel.id;

                    self.panels[id].dock_id = r;
                    self.dock_tree.nodes[r].panel_id = id;
                }

                self.bring_panel_to_front(id);
            }

            self.panel_action = PanelAction::None;
        }

        // draw dock preview
        let PanelAction::Move {
            dock_target,
            cancelled_docking,
            ..
        } = &mut self.panel_action
        else {
            return;
        };

        if self.mouse.pressed(MouseBtn::Right) {
            *cancelled_docking = true;
        }

        if !dock_target.is_null()
            && (!self.panels[*dock_target].clip_rect.contains(self.mouse.pos)
                || self.modifiers.shift_key())
            || *cancelled_docking
        {
            *dock_target = Id::NULL
        }

        if !dock_target.is_null() && can_dock {
            let dock_target_panel = &mut self.panels[*dock_target];
            // let mut preview = dock_target_panel.visible_content_rect();
            let mut fill = self.style.btn_press();
            fill.a = 0.3;

            let (preview, dir, ratio) =
                Self::get_dock_target(self.mouse.pos, dock_target_panel.panel_rect());

            let prev_size = preview.size() - self.style.panel_outline().width * 2.0;
            let prev_center = preview.center();

            dock_target_panel.drawlist_over.draw(
                Rect::from_center_size(prev_center, prev_size)
                    // preview
                    .draw_rect()
                    // .corners(self.style.panel_corner_radius())
                    .fill(fill),
            );
        }
    }

    pub fn update_panel_move(&mut self) {
        // TODO[BUG]: after drag quickly drag over another panel make the wrong panel move
        // probably because of prev_active_panel_id and not current id

        // check if we should start move action
        if !self.active_panel_id.is_null() {
            let p = &mut self.panels[self.active_panel_id];
            if self.active_id == p.move_id
                && !p.move_id.is_null()
                && !p.flags.has(PanelFlags::NO_MOVE)
            // || self.active_id == p.id && p.nav_root == p.move_id
            {
                if self.mouse.dragging(MouseBtn::Left) && self.panel_action.is_none() {
                    let start_pos = if p.dock_id.is_null() {
                        p.pos
                    } else {
                        let dock_root = self.dock_tree.get_root(p.dock_id);
                        self.dock_tree.nodes[dock_root].rect.min
                    };

                    let mouse_pos = self.mouse.drag_start(MouseBtn::Left).unwrap();

                    let tb_rect = p.titlebar_rect();
                    let tb_handle_rect = p.title_handle_rect;

                    self.panel_action = PanelAction::Move {
                        id: p.root,
                        start_pos,
                        dock_target: Id::NULL,
                        cancelled_docking: false,
                        drag_by_titlebar: tb_rect.contains(mouse_pos),
                        drag_by_title_handle: tb_handle_rect.contains(mouse_pos),
                    }
                }
            }
        }

        // move the panel
        let PanelAction::Move {
            start_pos,
            id: drag_id,
            dock_target,
            cancelled_docking,
            drag_by_titlebar,
            drag_by_title_handle,
        } = &mut self.panel_action
        else {
            return;
        };

        let drag_id = *drag_id;

        if !self.mouse.pressed(MouseBtn::Left) && dock_target.is_null() {
            self.panel_action = PanelAction::None;
            return;
        }

        if self.mouse.dragging(MouseBtn::Left) {
            let drag_start = self.mouse.drag_start(MouseBtn::Left).unwrap();
            let p = &mut self.panels[drag_id];

            let mouse_delta = *start_pos - drag_start;
            let new_pos = self.mouse.pos + mouse_delta;

            if p.dock_id.is_null() {
                p.move_panel_to(new_pos);
            } else if *drag_by_title_handle {
                if (*start_pos - new_pos).length() > self.undock_threshold {
                    let dock_root = self.dock_tree.get_root(p.dock_id);
                    let dock_root_pos = self.dock_tree.nodes[dock_root].rect.min;
                    let dock_pos = self.dock_tree.nodes[p.dock_id].rect.min;
                    *start_pos += dock_pos - dock_root_pos;

                    self.dock_tree.undock_node(p.dock_id, &mut self.panels);
                    // dock root may be removed if e.g. we only had a single split
                    // if let Some(n) = self.dock_tree.nodes.get(dock_root) {
                    //     self.dock_tree.recompute_rects(n.id, n.rect);
                    // }
                    let p = &mut self.panels[drag_id];
                    self.bring_panel_to_front(drag_id);
                }
            } else {
                let dock_root = self.dock_tree.get_root(p.dock_id);
                let n = &mut self.dock_tree.nodes[dock_root];
                let size = n.rect.size();
                let rect = Rect::from_min_size(new_pos, size);
                self.dock_tree.recompute_rects(dock_root, rect);
                // n.rect = n.rect.translate(mouse_delta);
            }
        }
    }

    pub fn end_assert(&mut self, name: Option<&str>) {
        let p = self.get_current_panel();
        let id = p.id;
        if let Some(name) = name {
            assert!(name == &p.name);
        }

        self.end();
    }

    pub fn end(&mut self) {
        let p = self.get_current_panel();
        let id = p.id;

        let p = self.get_current_panel();
        let p_pad = p.padding;
        // p.id_stack.pop().unwrap();
        assert!(id == p.pop_id());
        if !p.id_stack_ref().is_empty() {
            log::warn!("non empty id stack at ");
        }
        // self.offset_cursor_pos(Vec2::splat(p_pad));

        //         {
        //             let mut c = p.cursor.borrow_mut();
        //             c.max_pos += Vec2::splat(p.padding);
        //         }

        let list = self.current_drawlist();
        list.pop_clip_rect_n(2);
        // self.draw(|list| {
        //     list.pop_clip_rect();
        //     list.pop_clip_rect();
        // });

        let p = &mut self.panels[id];

        let prev_max_pos = p.cursor_max_pos();
        let prev_content_start = p.content_start_pos();

        // p.init_content_cursor(p.visible_content_start_pos());

        // sizing
        p.full_content_size = prev_max_pos - prev_content_start;
        p.full_size = prev_max_pos - p.pos + Vec2::splat(p.padding); // + Vec2::splat(outline.offset()) * 2.0;

        // TODO[NOTE]: is it possible to get size from only 1 frame?
        // or configurable
        if self.frame_count - p.frame_created <= 2 {
            // p.size = p.full_size * 1.1;
            // TODO[NOTE]: account for scrollbar width?
            p.size = p.full_size + p.padding + self.style.scrollbar_padding();
        }

        assert!(id == self.current_panel_stack.pop().unwrap());
        self.current_panel_id = self.current_panel_stack.last().copied().unwrap_or(Id::NULL);
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
            if self.mouse.just_pressed(Btn::Left) {
                sig |= Signal::JUST_PRESSED_LEFT;
            }
            if self.mouse.just_pressed(Btn::Right) {
                sig |= Signal::JUST_PRESSED_RIGHT;
            }
            if self.mouse.just_pressed(Btn::Middle) {
                sig |= Signal::JUST_PRESSED_MIDDLE;
            }

            if self.mouse.pressed(Btn::Left) {
                sig |= Signal::PRESSED_LEFT;
            }
            if self.mouse.pressed(Btn::Right) {
                sig |= Signal::PRESSED_RIGHT;
            }
            if self.mouse.pressed(Btn::Middle) {
                sig |= Signal::PRESSED_MIDDLE;
            }

            if self.mouse.double_pressed(Btn::Left) {
                sig |= Signal::DOUBLE_PRESSED_LEFT;
            }
            if self.mouse.double_pressed(Btn::Right) {
                sig |= Signal::DOUBLE_PRESSED_RIGHT;
            }
            if self.mouse.double_pressed(Btn::Middle) {
                sig |= Signal::DOUBLE_PRESSED_MIDDLE;
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

            if self.mouse.triple_clicked(Btn::Left) {
                sig |= Signal::TRIPLE_CLICKED_LEFT;
            }
            if self.mouse.triple_clicked(Btn::Right) {
                sig |= Signal::TRIPLE_CLICKED_RIGHT;
            }
            if self.mouse.triple_clicked(Btn::Middle) {
                sig |= Signal::TRIPLE_CLICKED_MIDDLE;
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
        &self.panels[self.window_panel_id]
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

    pub fn glyph_cache(&mut self) -> &mut GlyphCache {
        self.glyph_cache.get_mut()
    }

    pub fn indent(&mut self, indent: f32) {
        let mut c = self.get_current_panel()._cursor.borrow_mut();
        c.pos.x += indent;
        c.max_pos = c.max_pos.max(c.pos);
        c.indent = indent;
    }

    pub fn unindent(&mut self, indent: f32) {
        let mut c = self.get_current_panel()._cursor.borrow_mut();
        c.pos.x -= indent;
        c.max_pos = c.max_pos.max(c.pos);
        c.indent -= indent;
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
        let mut c = self.get_current_panel()._cursor.borrow_mut();
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

    pub fn new_line(&mut self) {
        self.place_item(Id::NULL, Vec2::new(0.0, self.style.line_height()));
    }

    pub fn same_line(&self) {
        let p = self.get_current_panel();
        // TODO[CHECK]: scroll
        let mut c = p._cursor.borrow_mut();
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
        (p.visible_content_rect().max - p.cursor_pos()).max(Vec2::ZERO)
    }

    pub fn full_available_content(&self) -> Vec2 {
        let p = self.get_current_panel();
        (p.full_content_rect().max - p.cursor_pos()).max(Vec2::ZERO)
    }

    // based on: https://github.com/ocornut/imgui/blob/3dafd9e898290ca890c29a379188be9e53b88537/imgui.cpp#L11183
    // TODO[NOTE]: what do we do with layout? now that we have same_line
    pub fn place_item(&mut self, id: Id, size: Vec2) -> Rect {
        let p = self.get_current_panel();
        // let rect = Rect::from_min_size(p.cursor_pos().round() + p.scroll, size.round());
        let rect = Rect::from_min_size(p.cursor_pos().round(), size.round());
        let clip_rect = p.current_clip_rect();

        let mut c = p._cursor.borrow_mut();

        let line_y1 = if c.is_same_line {
            c.pos_prev_line.y
        } else {
            c.pos.y
        };
        let line_height = c.line_height.max(c.pos.y - line_y1 + size.y);

        c.pos_prev_line.x = c.pos.x + size.x;
        c.pos_prev_line.y = line_y1;

        c.pos.x = (p.pos.x + p.padding + c.indent).round();
        c.pos.y = line_y1 + line_height + self.style.spacing_v();

        c.max_pos.x = c.max_pos.x.max(c.pos_prev_line.x);
        c.max_pos.y = c.max_pos.y.max(c.pos.y - self.style.spacing_v());

        c.prev_line_height = line_height;
        c.line_height = 0.0;
        c.is_same_line = false;
        drop(c);

        if !id.is_null() {
            self.prev_item_data.reset();
            self.prev_item_data.id = id;
            self.prev_item_data.rect = rect;

            let Some(crect) = rect.clip(clip_rect) else {
                self.prev_item_data.is_hidden = true;
                return rect;
            };

            if self.draw_item_outline {
                // self.draw_over(|list| {
                self.draw_over(
                    rect.draw_rect()
                        .outline(Outline::outer(RGBA::PASTEL_YELLOW, 1.5)),
                );
                // list.add_rect_outline(
                //     rect.min,
                //     rect.max,
                //     Outline::outer(RGBA::PASTEL_YELLOW, 1.5),
                // );
                if let Some(crect) = rect.clip(clip_rect) {
                    self.draw_over(crect.draw_rect().outline(Outline::outer(RGBA::YELLOW, 1.5)));
                    // list.add_rect_outline(
                    //     crect.min,
                    //     crect.max,
                    //     Outline::outer(RGBA::YELLOW, 1.5),
                    // );
                }
                // });
            }

            self.prev_item_data.clipped_rect = crect;
            self.prev_item_data.is_clipped = !clip_rect.contains_rect(rect);
        }

        rect
    }

    pub fn update_hot_id(&mut self, id: Id, bb: Rect, flags: ItemFlags) {
        let is_topmost =
            self.prev_hot_panel_id == self.current_panel_id || self.prev_hot_panel_id.is_null();
        if bb.contains(self.mouse.pos)
            && !id.is_null()
            && self.panel_action.is_none()
            && is_topmost
            && !self.mouse.dragging(MouseBtn::Left)
            && !self.expect_drag
        {
            self.hot_id = id;

            // if self.mouse.pressed(MouseBtn::Left) && self.active_id != id
            if self.active_id != id {
                if flags.has(ItemFlags::ACTIVATE_ON_RELEASE) && self.mouse.released(MouseBtn::Left)
                    || !flags.has(ItemFlags::ACTIVATE_ON_RELEASE)
                        && self.mouse.pressed(MouseBtn::Left)
                {
                    self.active_id = id;
                    self.active_id_changed = true;
                }

                // self.active_panel_id = self.hot_panel_id;

                // if !self.active_panel_id.is_null() {
                //     self.bring_panel_to_front(self.active_panel_id);
                // }
            }
        }
    }

    pub fn register_rect(&mut self, id: Id, rect: Rect) -> Signal {
        let p = &self.panels[self.current_panel_id];
        let clip_rect = p.current_clip_rect();
        if let Some(clip) = clip_rect.clip(rect) {
            self.update_hot_id(id, clip, ItemFlags::NONE);
        }
        self.get_item_signal(id, rect)
    }

    pub fn register_item(&mut self, id: Id) -> Signal {
        self.register_item_ex(id, ItemFlags::NONE)
    }

    /// "registers" the item, i.e. potentially sets hot_id and returns the item signals
    ///
    /// assumes the item to be a rect at position of the cursor with given size
    pub fn register_item_ex(&mut self, id: Id, flags: ItemFlags) -> Signal {
        if id.is_null() {
            return Signal::NONE;
        }

        assert!(self.prev_item_data.id == id);
        // let p = self.get_current_panel();
        if self.prev_item_data.is_hidden && self.active_id != id {
            return Signal::NONE;
        }

        let clip_rect = self.prev_item_data.clipped_rect;
        self.update_hot_id(id, clip_rect, flags);

        // if clip_rect.contains(self.mouse.pos) {
        //     // let is_over = if let Some(hot) = self.get_hot_panel() {
        //     //     hot.draw_order > draw_order
        //     // } else {
        //     //     true
        //     // };
        //     // if is_over

        //     // TODO[CHECK]: is this correct?, maybe use draw order?
        //     // TODO[CHECK]: use prev_hot_panel_id because if we used hot_panel_id
        //     // we would potentially return multiple hovering signals per frame?
        //     // maybe instead use some prev_hot_id in get_item_signals?
        //     if self.prev_hot_panel_id == self.current_panel_id
        //         || self.prev_hot_panel_id.is_null()
        //         || self.panel_action.is_none()
        //     {
        //         self.hot_id = id;
        //     }
        // }

        self.get_item_signal(id, clip_rect)
    }

    pub fn create_panel(&mut self, name: impl Into<String>) -> Id {
        let name: String = name.into();
        let mut p = Panel::new(&name);
        let id = self.gen_id(&name);
        p.frame_created = self.frame_count;

        if self.next.initial_width.is_finite() {
            p.size.x = self.next.initial_width;
        }
        if self.next.initial_height.is_finite() {
            p.size.y = self.next.initial_height;
        }

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

    // pub fn get_panel_id_with_name(&self, name: &str) -> Id {
    //     let id = self.gen_id(name);
    //     // if self.panels.contains_key(&id) {
    //     if self.panels.contains_id(id) {
    //         id
    //     } else {
    //         Id::NULL
    //     }
    // }

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
        if p.explicit_size.is_finite() {
            log::warn!("set_current_panel_max_size with also explicit size");
        }
        p.max_size = f(p.size, p.full_size, p.full_content_size);
    }

    pub fn set_current_panel_min_size(&mut self, f: impl Fn(Vec2, Vec2, Vec2) -> Vec2) {
        let p = &mut self.panels[self.current_panel_id];
        if p.explicit_size.is_finite() {
            log::warn!("set_current_panel_min_size with also explicit size");
        }
        p.min_size = f(p.size, p.full_size, p.full_content_size);
    }

    // pub fn bring_panel_to_front(&mut self, panel_id: Id) {
    //     assert_eq!(self.panels.len(), self.draw_order.len());

    //     use std::collections::{HashMap, HashSet, VecDeque};

    //     // gather the panel and all of its descendants (children, grandchildren, ...)
    //     let mut stack = vec![panel_id];
    //     let mut group_set: HashSet<Id> = HashSet::new();
    //     while let Some(id) = stack.pop() {
    //         if !group_set.insert(id) {
    //             continue;
    //         }
    //         for &c in &self.panels[id].children {
    //             stack.push(c);
    //         }
    //     }

    //     if group_set.is_empty() {
    //         return;
    //     }

    //     // include panels that belong to the same dock tree
    //     let dock_id = self.panels[panel_id].dock_id;
    //     if !dock_id.is_null() {
    //         let dock_tree: HashSet<_> = self.dock_tree.get_tree(dock_id).into_iter().collect();
    //         for (_, p) in &self.panels {
    //             if !p.dock_id.is_null() && dock_tree.contains(&p.dock_id) {
    //                 group_set.insert(p.id);
    //             }
    //         }
    //     }

    //     // map original draw order index for stable tie-breaking
    //     let mut orig_index: HashMap<Id, usize> = HashMap::new();
    //     for (i, &id) in self.draw_order.iter().enumerate() {
    //         orig_index.insert(id, i);
    //     }

    //     // build adjacency and indegree for nodes inside group_set (parent -> child)
    //     let mut indegree: HashMap<Id, usize> = HashMap::new();
    //     let mut adj: HashMap<Id, Vec<Id>> = HashMap::new();
    //     for &id in &group_set {
    //         indegree.entry(id).or_insert(0);
    //         for &ch in &self.panels[id].children {
    //             if group_set.contains(&ch) {
    //                 adj.entry(id).or_default().push(ch);
    //                 *indegree.entry(ch).or_insert(0) += 1;
    //             }
    //         }
    //     }

    //     // Kahn's algorithm but stable by original draw order (parents before children)
    //     let mut zero: Vec<Id> = indegree
    //         .iter()
    //         .filter_map(|(&id, &deg)| if deg == 0 { Some(id) } else { None })
    //         .collect();
    //     zero.sort_by_key(|id| orig_index.get(id).cloned().unwrap_or(usize::MAX));
    //     let mut queue: VecDeque<Id> = zero.into_iter().collect();
    //     let mut group_order: Vec<Id> = Vec::with_capacity(group_set.len());

    //     while let Some(node) = queue.pop_front() {
    //         group_order.push(node);
    //         if let Some(neis) = adj.get(&node) {
    //             for &m in neis {
    //                 if let Some(d) = indegree.get_mut(&m) {
    //                     *d -= 1;
    //                     if *d == 0 {
    //                         // insert preserving original order
    //                         let pos = queue
    //                             .iter()
    //                             .position(|&q| {
    //                                 orig_index
    //                                     .get(&q)
    //                                     .cloned()
    //                                     .unwrap_or(usize::MAX)
    //                                     > orig_index.get(&m).cloned().unwrap_or(usize::MAX)
    //                             })
    //                         .unwrap_or(queue.len());
    //                         queue.insert(pos, m);
    //                     }
    //                 }
    //             }
    //         }
    //     }

    //     // if we didn't produce a full ordering (cycle or unexpected), fall back to original relative order
    //     if group_order.len() != group_set.len() {
    //         group_order = self
    //             .draw_order
    //             .iter()
    //             .cloned()
    //             .filter(|id| group_set.contains(id))
    //             .collect();
    //     }

    //     if group_order.is_empty() {
    //         return;
    //     }

    //     // if the group is already at the very top in the same order, nothing to do
    //     let group_len = group_order.len();
    //     if group_len > 0 {
    //         let tail_slice = &self.draw_order[self.draw_order.len() - group_len..];
    //         if tail_slice == group_order.as_slice() {
    //             return;
    //         }
    //     }

    //     // build new draw order: all panels except group (preserving their order), then append group in the computed order
    //     let mut new_draw_order: Vec<Id> = self
    //         .draw_order
    //         .iter()
    //         .cloned()
    //         .filter(|id| !group_set.contains(id))
    //         .collect();

    //     new_draw_order.extend(group_order.iter().cloned());

    //     // write back and update per-panel draw_order indices
    //     self.draw_order = new_draw_order;
    //     for (i, &id) in self.draw_order.iter().enumerate() {
    //         self.panels[id].draw_order = i;
    //         assert_eq!(self.panels[id].draw_order, i);
    //     }
    // }

    pub fn bring_panel_to_front(&mut self, panel_id: Id) {
        assert_eq!(self.panels.len(), self.draw_order.len());

        // gather the panel and all of its descendants (children, grandchildren, ...)
        let mut stack = vec![panel_id];
        let mut group_set = HashSet::new();
        while let Some(id) = stack.pop() {
            if !group_set.insert(id) {
                continue;
            }
            // push children for DFS
            for &c in &self.panels[id].children {
                stack.push(c);
            }
        }

        if group_set.is_empty() {
            return;
        }

        let dock_id = self.panels[panel_id].dock_id;
        if !dock_id.is_null() {
            let dock_tree: HashSet<_> = self.dock_tree.get_tree(dock_id).into_iter().collect();

            for (_, p) in &self.panels {
                if !p.dock_id.is_null() && dock_tree.contains(&p.dock_id) {
                    group_set.insert(p.id);
                    group_set.extend(&p.children);
                }
            }
        }

        // preserve relative ordering as they appear in draw_order
        let group_in_draw_order: Vec<Id> = self
            .draw_order
            .iter()
            .cloned()
            .filter(|id| group_set.contains(id))
            .collect();

        if group_in_draw_order.is_empty() {
            return;
        }

        // if the group is already at the very top in the same order, nothing to do
        let group_len = group_in_draw_order.len();
        if group_len > 0 {
            let tail_slice = &self.draw_order[self.draw_order.len() - group_len..];
            if tail_slice == group_in_draw_order.as_slice() {
                return;
            }
        }

        // build new draw order: all panels except group (preserving their order), then append group in their original relative order
        let mut new_draw_order: Vec<Id> = self
            .draw_order
            .iter()
            .cloned()
            .filter(|id| !group_set.contains(id))
            .collect();

        new_draw_order.extend(group_in_draw_order.iter().cloned());

        // write back and update per-panel draw_order indices
        self.draw_order = new_draw_order;
        for (i, &id) in self.draw_order.iter().enumerate() {
            self.panels[id].draw_order = i;
            assert_eq!(self.panels[id].draw_order, i);
        }
    }

    // pub fn bring_panel_to_front(&mut self, panel_id: Id) {
    //     assert_eq!(self.panels.len(), self.draw_order.len());

    //     let root_id = {
    //         let p = &self.panels[panel_id];
    //         p.root
    //     };

    //     let curr_order = self.panels[root_id].draw_order;
    //     assert!(self.draw_order[curr_order] == root_id);

    //     let new_order = self.draw_order.len() - 1;
    //     if self.draw_order[new_order] == root_id {
    //         return;
    //     }

    //     for i in curr_order..new_order {
    //         let moved = self.draw_order[i + 1];
    //         self.draw_order[i] = moved;
    //         self.panels[moved].draw_order = i;
    //         assert_eq!(self.panels[moved].draw_order, i);
    //     }

    //     self.draw_order[new_order] = root_id;
    //     self.panels[root_id].draw_order = new_order;
    // }

    // TODO[BUG]: panel with multiple children leads to crash
    pub fn begin_child(&mut self, name: &str) {
        let id = self.gen_id(name);
        let panel_flags = PanelFlags::NO_TITLEBAR
            | PanelFlags::NO_DOCKING
            | PanelFlags::USE_PARENT_DRAWLIST
            | PanelFlags::DRAW_V_SCROLLBAR
            | PanelFlags::USE_PARENT_CLIP
            | PanelFlags::IS_CHILD;

        let parent = &mut self.panels[self.current_panel_id];
        let root = parent.root;
        // let nav_root = parent.nav_root;
        parent.child_id = id;

        let outline_offset = self.style.panel_outline().width;
        self.next.pos = parent.cursor_pos() + Vec2::splat(outline_offset);
        self.begin_ex(name, panel_flags);

        let child_id = self.current_panel_id;
        assert!(id == child_id);

        let child = &mut self.panels[child_id];
        child.root = root;
        // child.nav_root = nav_root;
    }

    pub fn end_child(&mut self) {
        let child_id = self.current_panel_id;
        self.end();
        let size = self.panels[child_id].size;

        let parent = self.current_panel_id;
        assert!(self.panels[parent].child_id == child_id);
        self.place_item(Id::NULL, size);
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
        self.next.pos = Vec2::ZERO;
        let win_size = self.window.window_size();
        self.next.size = win_size;
        // TODO
        // self.window
        match self.cursor_icon {
            CursorIcon::MoveH | CursorIcon::MoveV | CursorIcon::Text => {
                self.set_cursor_icon(CursorIcon::Default)
            }
            _ => (),
        }

        // NO_MOVE because the window panel dragging is handled by the window,
        // not the panel
        let mut flags = PanelFlags::NO_FOCUS | PanelFlags::NO_MOVE | PanelFlags::NO_DOCKING;

        if self.window.is_decorated() {
            flags |= PanelFlags::NO_TITLEBAR;
        } else {
            // self.window_panel_titlebar_height = self.style.titlebar_height();
        }

        self.window_panel_id = self.gen_id("##_WINDOW_PANEL");
        self.begin_ex("##_WINDOW_PANEL", flags);
        assert!(self.window_panel_id == self.current_panel_id);

        // }

        // let p_info: Vec<_> = self.panels.iter().map(|(_, p)| (p.name.clone(), p.draw_order)).collect();
        // println!("{:#?}", p_info);
        let root_panel = &mut self.panels[self.window_panel_id];
        root_panel.is_window_panel = true;
        if root_panel.close_pressed {
            self.close_pressed = true;
        }

        // let win_panel = self.get_current_panel();
        // let win_tb_height = win_panel.titlebar_height;
        // let win_size = win_panel.size;
        // self.next.pos = Vec2::new(0.0, win_tb_height);
        // self.next.size = win_size - self.next.pos;
        // let dockspace_rect = Rect::from_min_size(self.next.pos, self.next.size);

        // self.push_style(StyleVar::PanelBg(RGBA::ZERO));
        // self.push_style(StyleVar::PanelOutline(Outline::none()));
        // self.push_style(StyleVar::PanelHoverOutline(Outline::none()));
        // self.begin_ex(
        //     "##_DOCK_SPACE",
        //     PanelFlags::NO_FOCUS
        //         | PanelFlags::NO_MOVE
        //         | PanelFlags::NO_RESIZE
        //         | PanelFlags::NO_TITLEBAR,
        // );
        // let dock_space_id = self.get_current_panel().dock_id;
        // if !dock_space_id.is_null() {
        //     let dock_root = self.dock_tree.get_root(dock_space_id);
        //     self.dock_tree.recompute_rects(dock_root, dockspace_rect);
        // }
        // self.pop_style_n(3);
        // self.end();

        // if self.prev_hot_panel_id.is_null() || self.panels[self.prev_hot_panel_id].dock_id.is_null()
        // {
        //     return;
        // }
    }

    pub fn push_id(&self, id: Id) {
        let p = &self.panels[self.current_panel_id];
        p.push_id(id)
    }

    pub fn pop_id(&self) -> Id {
        let p = &self.panels[self.current_panel_id];
        p.pop_id()
    }

    pub fn push_style(&mut self, var: StyleVar) {
        self.style.push_var(var);
    }

    pub fn set_style(&mut self, var: StyleVar) {
        self.style.set_var(var);
    }

    pub fn pop_style_n(&mut self, n: u32) {
        for _ in 0..n {
            self.style.pop_var();
        }
    }

    pub fn pop_style(&mut self) {
        self.style.pop_var();
    }

    pub fn panel_debug_info(&mut self, id: Id) {
        use crate::ui_items::ui_text;

        if id.is_null() {
            ui_text!(self: "NONE");
            return;
        }

        let p = &self.panels[id];
        let name = p.name.clone();

        let id = p.id;
        let dock_id = p.dock_id;
        let root = p.root;
        let children = p.children.clone();
        let draw_order = p.draw_order;

        ui_text!(self: "name: {}", name);
        ui_text!(self: "id: {}", id);
        ui_text!(self: "dock_id: {}", dock_id);
        ui_text!(self: "root_id: {}", root);
        ui_text!(self: "children: {:?}", children);
        ui_text!(self: "draw order: {}", draw_order);
    }

    pub fn debug_window(&mut self) {
        use crate::ui_items::ui_text;

        self.next.initial_width = 450.0;
        self.begin_ex(
            "Debug##_DEBUG_PANEL",
            PanelFlags::DRAW_H_SCROLLBAR | PanelFlags::DRAW_V_SCROLLBAR,
        );

        let hot_name = self
            .get_panel_name_with_id(self.prev_hot_panel_id)
            .unwrap_or_default();

        let active_name = self
            .get_panel_name_with_id(self.active_panel_id)
            .unwrap_or_default();

        ui_text!(self: "hot: {hot_name}");
        ui_text!(self: "active: {active_name}");

        ui_text!(self: "hot item: {}", self.prev_hot_id);
        ui_text!(self: "active item: {}", self.prev_active_id);

        let draw_order: Vec<_> = self
            .draw_order
            .iter()
            .map(|id| self.panels[*id].name.clone().replace("#", ""))
            .collect();
        ui_text!(self: "draw_order: {draw_order:?}");

        let now = Instant::now();
        let dt = (now - self.prev_frame_time).as_secs_f32();
        let fps = 1.0 / dt;
        self.prev_frame_time = now;
        ui_text!(self: "dt: {:0.1?}\t, fps: {fps:0.1?}", dt * 1000.0);

        // self.pop_style();

        ui_text!(self: "action: {}", self.panel_action);
        ui_text!(self: "n. of draw calls: {}", self.n_draw_calls);

        // self.separator_h(4.0, self.style.panel_dark_bg());

        self.begin_child("text");
        // println!("{:#?}", self.get_current_panel());
        let mut flags = TextInputFlags::NONE;
        if self.checkbox_intern("multiline input (buggy)") {
            flags |= TextInputFlags::MULTILINE
        }
        if self.checkbox_intern("select text on activation") {
            flags |= TextInputFlags::SELECT_ON_ACTIVE;
        }

        let avail = self.available_content();
        self.text_input_ex("this is a text input field", flags);
        self.end_child();

        self.move_down(10.0);
        self.begin_tabbar("tabbar");

        self.indent(10.0);
        self.move_down(10.0);

        if self.tabitem("Style Settings") {
            let mut v = self.style.titlebar_height();
            self.slider_f32("titlebar height", 0.0, 100.0, &mut v);
            self.style.set_var(StyleVar::TitlebarHeight(v));

            let mut v = self.style.window_titlebar_height();
            self.slider_f32("window titlebar height", 0.0, 100.0, &mut v);
            self.style.set_var(StyleVar::WindowTitlebarHeight(v));

            let mut v = self.style.spacing_h();
            self.slider_f32("spacing h", 0.0, 30.0, &mut v);
            self.style.set_var(StyleVar::SpacingH(v));

            let mut v = self.style.spacing_v();
            self.slider_f32("spacing v", 0.0, 30.0, &mut v);
            self.style.set_var(StyleVar::SpacingV(v));

            let mut v = self.style.line_height();
            self.slider_f32("line height", 0.0, 30.0, &mut v);
            self.style.set_var(StyleVar::LineHeight(v));

            let mut v = self.style.panel_padding();
            self.slider_f32("panel padding", 0.0, 30.0, &mut v);
            v = v.round();
            self.style.set_var(StyleVar::PanelPadding(v));

            let mut out1 = self.style.panel_outline();
            let mut out2 = self.style.panel_hover_outline();
            self.slider_f32("panel outline width", 0.0, 30.0, &mut out1.width);
            out2.width = out1.width;
            self.style.set_var(StyleVar::PanelOutline(out1));
            self.style.set_var(StyleVar::PanelHoverOutline(out2));

            let mut v = self.style.scrollbar_width();
            self.slider_f32("scrollbar width", 0.0, 30.0, &mut v);
            v = v.round();
            self.style.set_var(StyleVar::ScrollbarWidth(v));

            let mut v = self.style.scrollbar_padding();
            self.slider_f32("scrollbar padding", 0.0, 30.0, &mut v);
            v = v.round();
            self.style.set_var(StyleVar::ScrollbarPadding(v));

            // TODO[NOTE]: not enough space in the font atlas
            // let mut v = self.style.text_size();
            // self.slider_f32("text height", 0.0, 30.0, &mut v);
            // self.style.set_var(StyleVar::TextSize(v));

            let mut v = self.style.btn_roundness();
            self.slider_f32("button corners", 0.0, 0.5, &mut v);
            self.style.set_var(StyleVar::BtnRoundness(v));

            let mut v = self.style.panel_corner_radius();
            self.slider_f32("panel corners", 0.0, 100.0, &mut v);
            self.style.set_var(StyleVar::PanelCornerRadius(v));
        }

        if self.tabitem("Textures") {
            if self.collapsing_header_intern("Font Atlas") {
                let avail = self.available_content().min(Vec2::splat(800.0));
                let uv_min = self.glyph_cache.borrow().min_alloc_uv;
                let uv_max = self.glyph_cache.borrow().max_alloc_uv;
                let size = uv_max - uv_min;
                let scale = (avail.x / size.x).min(avail.y / size.y);
                let fitted_size = size * scale;
                self.image(fitted_size - Vec2::new(20.0, 0.0), uv_min, uv_max, 1);
            }
        }

        if self.tabitem("Debug") {
            if self.button("reset docktree") {
                for (_, p) in &mut self.panels {
                    p.dock_id = Id::NULL;
                }
                self.dock_tree = DockTree::new();
            }

            let mut tmp = self.draw_wireframe;
            self.checkbox("draw wireframe", &mut tmp);
            self.draw_wireframe = tmp;

            let mut tmp = self.clip_content;
            self.checkbox("clip content", &mut tmp);
            self.clip_content = tmp;

            let mut tmp = self.draw_clip_rect;
            self.checkbox("draw clip rect", &mut tmp);
            self.draw_clip_rect = tmp;

            let mut tmp = self.draw_position_bounds;
            self.checkbox("draw position bounds", &mut tmp);
            self.draw_position_bounds = tmp;

            let mut tmp = self.draw_content_outline;
            self.checkbox("draw content outline", &mut tmp);
            self.draw_content_outline = tmp;

            let mut tmp = self.draw_full_content_outline;
            self.checkbox("draw full content outline", &mut tmp);
            self.draw_full_content_outline = tmp;

            let mut tmp = self.draw_item_outline;
            self.checkbox("draw item outline", &mut tmp);
            self.draw_item_outline = tmp;
        }

        self.unindent(10.0);
        self.end_tabbar();

        self.end();
    }

    pub fn end_frame(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !self.style.var_stack.is_empty() {
            log::warn!("style stack is not empty");
        }
        // if self.mouse.pressed(MouseBtn::Left) {
        //     println!("{}, {}, {}: {}, {}", !self.mouse.dragging(MouseBtn::Left), !self.expect_drag, self.panel_action.is_none(), self.hot_panel_id, self.hot_id);
        // }

        // update active panel
        if self.mouse.pressed(MouseBtn::Left)
            && !self.mouse.dragging(MouseBtn::Left)
            && !self.expect_drag
            && self.panel_action.is_none()
        // && self.hot_id != self.active_id
        {
            // set active id to panel id if we pressed mouse and dont hover any items
            if self.hot_id.is_null() {
                self.active_id = self.hot_id;
            } else if self.panels.contains_id(self.hot_id) {
                let panel = &self.panels[self.hot_id];
                self.active_id = panel.root;

                if !panel.flags.has(PanelFlags::ONLY_MOVE_FROM_TITLEBAR) {
                    self.active_id = self.panels[self.active_id].move_id;
                }
            }

            // set panel id
            self.active_panel_id = self.hot_panel_id;
            if !self.hot_panel_id.is_null() {
                self.active_panel_id = self.panels[self.hot_panel_id].root;
            }

            // if prev != self.active_id {
            //     self.active_id_changed = true;
            // }

            if !self.active_panel_id.is_null() {
                self.bring_panel_to_front(self.active_panel_id);
            }
        }

        if self.active_id != self.prev_active_id {
            self.active_id_changed = false;
        }

        self.update_panel_scroll();
        self.update_panel_resize();
        self.update_panel_move();
        self.update_panel_dock();

        self.prev_hot_panel_id = self.hot_panel_id;
        self.prev_active_panel_id = self.active_panel_id;
        self.prev_hot_id = self.hot_id;
        self.prev_active_id = self.active_id;

        self.end_assert(Some("##_WINDOW_PANEL"));

        if !self.draw_wireframe {
            self.build_draw_data();
        } else {
            self.build_dbg_draw_data();
        }
        self.n_draw_calls = self.draw.call_list.len();

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

        self.prune_nodes();

        self.frame_count += 1;
        self.mouse.end_frame();
    }

    pub fn prune_nodes(&mut self) {
        self.panels.retain(|id, panel| {
            let unused = self.frame_count - panel.last_frame_used > 1;
            if unused {
                debug_assert_eq!(*id, panel.id);
                debug_assert_ne!(*id, self.hot_id);
                debug_assert_ne!(*id, self.active_id);
                debug_assert_ne!(*id, self.hot_panel_id);
                debug_assert_ne!(*id, self.active_panel_id);
            }
            !unused
        });
    }

    pub fn layout_text_with_font(
        &self,
        text: &str,
        font_size: f32,
        font: &'static str,
    ) -> ShapedText {
        let text = match text.find("##") {
            Some(idx) => text[..idx].to_string(),
            None => text.to_string(),
        };

        let itm = TextItem::new(text, font_size, 1.0, font);
        let mut text_cache = self.text_item_cache.borrow_mut();
        let mut glyph_cache = self.glyph_cache.borrow_mut();
        let mut font_table = self.font_table.clone();

        let shaped_text = if !text_cache.contains_key(&itm) {
            let shaped_text = itm.layout(&mut font_table, &mut glyph_cache, &self.draw.wgpu);
            text_cache.entry(itm).or_insert(shaped_text)
        } else {
            text_cache.get(&itm).unwrap()
        };
        shaped_text.clone()
    }

    pub fn layout_text(&self, text: &str, font_size: f32) -> ShapedText {
        self.layout_text_with_font(text, font_size, "Inter")
    }

    pub fn layout_icon(&self, text: &str, font_size: f32) -> ShapedText {
        self.layout_text_with_font(text, font_size, "Phosphor")
    }

    pub fn draw_text(&mut self, text: &str, pos: Vec2) {
        let shape = self.layout_text(text, 32.0);

        for g in shape.glyphs.iter() {
            let min = g.meta.pos + pos;
            let max = min + g.meta.size;
            let uv_min = g.meta.uv_min;
            let uv_max = g.meta.uv_max;

            self.draw(
                Rect::from_min_max(min, max)
                    .draw_rect()
                    .texture(1)
                    .uv(uv_min, uv_max),
            );
        }
    }

    pub fn upload_draw_data(&mut self) {
        let draw_buff = &mut self.draw.call_list;
        if draw_buff.vtx_alloc.len() * std::mem::size_of::<Vertex>()
            > self.draw.gpu_vertices.size() as usize
        {
            self.draw.gpu_vertices =
                self.draw
                    .wgpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("draw_list_vertex_buffer"),
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
                        contents: bytemuck::cast_slice(&draw_buff.vtx_alloc),
                    });
        } else {
            self.draw.wgpu.queue.write_buffer(
                &self.draw.gpu_vertices,
                0,
                bytemuck::cast_slice(&draw_buff.vtx_alloc),
            );
        }

        if self.draw.call_list.idx_alloc.len() * std::mem::size_of::<u32>()
            > self.draw.gpu_indices.size() as usize
        {
            self.draw.gpu_indices =
                self.draw
                    .wgpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("draw_list_index_buffer"),
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::INDEX,
                        contents: bytemuck::cast_slice(&self.draw.call_list.idx_alloc),
                    });
        } else {
            self.draw.wgpu.queue.write_buffer(
                &self.draw.gpu_indices,
                0,
                bytemuck::cast_slice(&self.draw.call_list.idx_alloc),
            );
        }
    }

    pub fn build_draw_list(draw_buff: &mut DrawCallList, draw_list: &DrawList, screen_size: Vec2) {
        // let draw_list = self.panels[id].draw_list.borrow();
        // println!("draw_list:\n{:#?}", draw_list);
        // for cmd in &draw_list.cmd_buffer

        // println!("{:#?}", draw_list);

        for cmd in draw_list.commands().iter() {
            let vtx = &draw_list.vtx_slice(cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count);
            let idx = &draw_list.idx_slice(cmd.idx_offset..cmd.idx_offset + cmd.idx_count);

            let mut curr_clip = draw_buff.current_clip_rect();
            curr_clip.min = curr_clip.min.max(Vec2::ZERO);
            curr_clip.max = curr_clip.max.min(screen_size);

            let mut clip = cmd.clip_rect;
            clip.min = clip.min.max(Vec2::ZERO);
            clip.max = clip.max.min(screen_size);

            // draw_buff.set_clip_rect(cmd.clip_rect);
            if cmd.clip_rect_used {
                draw_buff.set_clip_rect(cmd.clip_rect);
            } else if !draw_buff.current_clip_rect().contains_rect(clip) {
                draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, screen_size));
            }
            draw_buff.push(vtx, idx);
        }
    }

    pub fn build_debug_draw_list(
        draw_buff: &mut DrawCallList,
        draw_list: &DrawList,
        screen_size: Vec2,
    ) {
        // let draw_list = self.panels[id].draw_list.borrow();
        // println!("{} draw_list:\n{:#?}", self.panels[id].name, draw_list);
        for cmd in draw_list.commands().iter() {
            let vtx = draw_list.vtx_slice(cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count);
            let idx = draw_list.idx_slice(cmd.idx_offset..cmd.idx_offset + cmd.idx_count);

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

    pub fn build_draw_data(&mut self) {
        let panels = &self.panels;
        let draw_buff = &mut self.draw.call_list;
        draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));

        for &id in &self.draw_order {
            let p = &self.panels[id];

            if p.flags.has(PanelFlags::USE_PARENT_DRAWLIST) {
                continue;
            }

            Self::build_draw_list(draw_buff, &p.drawlist, self.draw.screen_size);
            Self::build_draw_list(draw_buff, &p.drawlist_over, self.draw.screen_size);
        }

        self.upload_draw_data();
    }

    pub fn build_dbg_draw_data(&mut self) {
        let panels = &self.panels;
        let draw_buff = &mut self.draw.call_list;
        draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));

        for &id in &self.draw_order {
            let p = &self.panels[id];

            if p.flags.has(PanelFlags::USE_PARENT_DRAWLIST) {
                continue;
            }

            let draw_list = &p.drawlist;
            Self::build_debug_draw_list(draw_buff, &draw_list, self.draw.screen_size);
        }
        self.upload_draw_data();
    }
}

// BEGIN TYPES
//---------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Panel {
    pub name: String,
    pub id: Id,
    /// set active_id to this id to start dragging the panel
    pub move_id: Id,
    pub flags: PanelFlags,

    pub root: Id,
    // pub nav_root: Id,
    pub parent_id: Id,
    pub child_id: Id,
    // TODO[NOTE]: not implemented yet
    pub children: Vec<Id>,

    pub dock_id: Id,

    pub padding: f32,
    pub titlebar_height: f32,
    pub title_handle_rect: Rect,

    pub scrollbar_width: f32,
    pub scrollbar_padding: f32,

    /// pos of the panel at draw time
    ///
    /// preserved over frames, does not include outline
    pub pos: Vec2,

    /// size of the panel at draw time
    ///
    /// preserved over frames
    pub size: Vec2,

    /// full size of the panel, i.e. from top left to bottom right corner, including the titlebar
    ///
    /// does not include outline
    pub full_size: Vec2,

    pub size_pre_dock: Vec2,

    pub full_rect: Rect,
    pub clip_rect: Rect,

    pub position_bounds: Rect,
    /// determines how the panels position is constraint
    ///
    /// true => panel cannot exit bounds \
    /// false => panel cannot fully exit bounds
    // TODO[CHECK]: not used
    pub clamp_position_to_bounds: bool,

    // TODO[CHECK]: currently only used when placing the items. i.e. cursor position is not offset
    // by scroll, scroll is only added when generating the item rectangle
    pub scroll: Vec2,

    // scroll will take on this value next frame
    // we do this because sizing is computed based on cursor positions and cursor positions
    // are affected by scrolling leading to a feedback loop
    // TODO[CHECK]: currently we only clamp the scroll at the next begin(), i.e. when applying to
    // scroll. otherwise panel does not scroll back automatically when resizing (why?)
    pub next_scroll: Vec2,
    pub indent: f32,

    /// size of the content of a panel
    ///
    /// computed based on cursor.content_start_pos and cursor.max_pos
    pub full_content_size: Vec2,
    pub explicit_size: Vec2,

    pub outline_offset: f32,

    pub min_size: Vec2,
    pub max_size: Vec2,

    pub draw_order: usize,

    pub last_frame_used: u64,
    pub frame_created: u64,
    pub close_pressed: bool,
    pub is_window_panel: bool,

    // try to not borrow outside of impl Panel { ... }
    pub drawlist: DrawList,
    pub drawlist_over: DrawList,
    pub id_stack: RefCell<Vec<Id>>,
    pub _cursor: RefCell<Cursor>,
    pub scroll_offset: f32,
}

// impl fmt::Debug for Panel {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         f.debug_struct("Panel")
//             .field("name", &self.name)
//             .field("id", &format!("{}", self.id))
//             .field("order", &self.draw_order)
//             .field("pos", &self.pos)
//             .field("size", &self.size)
//             .field("full_size", &self.size)
//             .field("content_size", &self.size)
//             .field("exeplicit_size", &self.explicit_size)
//             .field("min_size", &self.min_size)
//             .field("max_size", &self.max_size)
//             .finish_non_exhaustive()
//     }
// }

impl Panel {
    pub fn new(name: impl Into<String>) -> Self {
        let name: String = name.into();
        let id = Id::from_str(&name);
        Self {
            name,
            id,
            parent_id: Id::NULL,
            child_id: Id::NULL,
            children: vec![],
            dock_id: Id::NULL,
            root: Id::NULL,
            // nav_root: Id::NULL,
            flags: PanelFlags::NONE,
            padding: 0.0,
            scrollbar_width: 0.0,
            scrollbar_padding: 0.0,
            // spacing: 10.0,
            pos: Vec2::splat(30.0),
            scroll: Vec2::ZERO,
            next_scroll: Vec2::ZERO,
            indent: 0.0,

            full_content_size: Vec2::ZERO,
            full_size: Vec2::ZERO,
            full_rect: Rect::ZERO,
            clip_rect: Rect::ZERO,

            position_bounds: Rect::ZERO,
            clamp_position_to_bounds: false,

            explicit_size: Vec2::NAN,
            outline_offset: 0.0,
            draw_order: 0,
            // bg_color: RGBA::ZERO,
            titlebar_height: 0.0,
            title_handle_rect: Rect::ZERO,
            move_id: Id::NULL,
            size: Vec2::ZERO,
            size_pre_dock: Vec2::NAN,
            min_size: Vec2::ZERO,
            max_size: Vec2::ZERO,
            frame_created: 0,
            last_frame_used: 0,
            // draw_list: DrawList::new(),
            // id_stack: Vec::new(),
            close_pressed: false,
            is_window_panel: false,

            drawlist: DrawList::new(),
            drawlist_over: DrawList::new(),
            id_stack: RefCell::new(Vec::new()),
            _cursor: RefCell::new(Cursor::default()),
            scroll_offset: 0.0,
        }
    }

    pub fn panel_min_size(&self) -> Vec2 {
        let pad = 2.0 * self.padding;
        (self.title_handle_rect.size() + pad).max(self.min_size)
        // Vec2::new(pad, self.titlebar_height + pad).max(self.min_size)
    }

    pub fn panel_max_size(&self) -> Vec2 {
        self.max_size
    }

    pub fn panel_rect_with_outline(&self) -> Rect {
        let off = Vec2::splat(self.outline_offset);
        Rect::from_min_max(self.pos - off, self.pos + self.size + off)
    }

    pub fn panel_rect(&self) -> Rect {
        Rect::from_min_max(self.pos, self.pos + self.size)
    }

    pub fn needs_scrollbars(&self) -> (bool, bool) {
        let base = self.visible_content_start_pos();
        let full = Rect::from_min_max(base, base + self.full_content_size);

        let max_view_full = self.pos + self.size - Vec2::splat(self.padding);
        let min_view = base;
        let scrollbar_space = self.scrollbar_width + self.scrollbar_padding;

        let full_width = full.width();
        let full_height = full.height();
        let view_width = max_view_full.x - min_view.x;
        let view_height = max_view_full.y - min_view.y;

        // Check all 4 possible states explicitly

        // State 1: Neither scrollbar
        let w_none = view_width;
        let h_none = view_height;
        let valid_none = full_width <= w_none && full_height <= h_none;

        // State 2: Only horizontal scrollbar
        let w_h = view_width;
        let h_h = view_height - scrollbar_space;
        let valid_h = full_width <= w_h && full_height > h_h && full_height <= view_height;

        // State 3: Only vertical scrollbar
        let w_v = view_width - scrollbar_space;
        let h_v = view_height;
        let valid_v = full_width > w_v && full_width <= view_width && full_height <= h_v;

        // State 4: Both scrollbars
        let w_both = view_width - scrollbar_space;
        let h_both = view_height - scrollbar_space;
        let valid_both = full_width > w_both || full_height > h_both;

        // Return the first valid state in priority order
        let (x, y) = if valid_none {
            (false, false)
        } else if valid_h {
            (true, false)
        } else if valid_v {
            (false, true)
        } else {
            (true, true)
        };

        (
            x || !self.flags.has(PanelFlags::DONT_KEEP_SCROLLBAR_PAD)
                && self.flags.has(PanelFlags::DRAW_H_SCROLLBAR),
            y || !self.flags.has(PanelFlags::DONT_KEEP_SCROLLBAR_PAD)
                && self.flags.has(PanelFlags::DRAW_V_SCROLLBAR),
        )
    }

    fn scroll_min(&self) -> Vec2 {
        // use the unscrolled content origin (cursor.content_start_pos) so bounds don't depend on self.scroll
        let origin = self._cursor.borrow().content_start_pos;
        let full_end = origin + self.full_content_size;
        let visible_end = self.visible_content_end_pos();

        let x = (visible_end.x - full_end.x).min(0.0);
        let y = (visible_end.y - full_end.y).min(0.0);

        Vec2::new(x, y)
    }

    fn scrolling_past_bounds(&self, delta: Vec2) -> bool {
        let scroll = (self.scroll + delta)
            .min(self.scroll_max())
            .max(self.scroll_min());

        scroll == self.scroll
    }

    // Replace scroll_max() with:
    pub fn scroll_max(&self) -> Vec2 {
        Vec2::ZERO
        // let origin = self._cursor.borrow().content_start_pos;
        // let visible_start = self.visible_content_start_pos(); // this is the visible content origin

        // let x = (visible_start.x - origin.x).max(0.0);
        // let y = (visible_start.y - origin.y).max(0.0);

        // Vec2::new(x, y)
    }

    // fn scroll_min(&self) -> Vec2 {
    //     let full = self.content_start_pos();
    //     let visible = self.visible_content_start_pos();

    //     let x = if visible.x > full.x {
    //         full.x - visible.x
    //     } else {
    //         0.0
    //     };

    //     let y = if visible.y > full.y {
    //         full.y - visible.y
    //     } else {
    //         0.0
    //     };

    //     Vec2::new(x, y)
    // }

    // pub fn scroll_max(&self) -> Vec2 {
    //     let full = self.content_end_pos();
    //     let visible = self.visible_content_end_pos();

    //     let x = if visible.x < full.x {
    //         visible.x - full.x
    //     } else {
    //         0.0
    //     };

    //     let y = if visible.y < full.y {
    //         visible.y - full.y
    //     } else {
    //         0.0
    //     };

    //     Vec2::new(x, y)
    // }

    // pub fn set_scroll(&mut self, scroll: Vec2) {
    //     let min = self.scroll_min();
    //     let max = self.scroll_max();
    //     self.next_scroll = scroll.min(max).max(min);
    // }

    pub fn set_scroll(&mut self, delta: Vec2) {
        // self.next_scroll = self.scroll + delta;
        self.next_scroll = self.scroll + delta;
        // self.set_scroll(self.scroll + delta);
    }

    pub fn visible_content_rect(&self) -> Rect {
        Rect::from_min_max(
            self.visible_content_start_pos(),
            self.visible_content_end_pos(),
        )
    }

    pub fn full_content_rect(&self) -> Rect {
        Rect::from_min_max(self.content_start_pos(), self.content_end_pos())
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.drawlist.current_clip_rect()
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
        self.drawlist.clear();
        self.drawlist_over.clear();
        self.root = Id::NULL;
    }

    pub fn id_stack_ref(&self) -> Ref<'_, Vec<Id>> {
        self.id_stack.borrow()
    }

    pub fn set_cursor_pos(&self, pos: Vec2) {
        self._cursor.borrow_mut().pos = pos;
    }

    pub fn init_content_cursor(&self, pos: Vec2) {
        let mut c = self._cursor.borrow_mut();
        c.content_start_pos = pos;
        c.pos = pos;
        c.max_pos = pos;
    }

    pub fn cursor_pos(&self) -> Vec2 {
        // self._cursor.borrow().pos.round()
        self._cursor.borrow().pos + self.scroll
    }

    pub fn cursor_max_pos(&self) -> Vec2 {
        self._cursor.borrow().max_pos.round()
        // self._cursor.borrow().max_pos + self.scroll
    }

    pub fn visible_content_end_pos(&self) -> Vec2 {
        // self.visible_content_start_pos() + self.size()
        let mut max = self.pos + self.size - Vec2::splat(self.padding);
        let (x_scroll, y_scroll) = self.needs_scrollbars();

        let flags = self.flags;
        if x_scroll {
            max.x = max.x - self.scrollbar_width - self.scrollbar_padding;
        }

        if y_scroll {
            max.y = max.y - self.scrollbar_width - self.scrollbar_padding;
        }

        max.round()
    }

    pub fn titlebar_rect(&self) -> Rect {
        if self.flags.has(PanelFlags::NO_TITLEBAR) {
            return Rect::ZERO;
        } else {
            Rect::from_min_size(self.pos, Vec2::new(self.size.x, self.titlebar_height))
        }
    }

    pub fn content_end_pos(&self) -> Vec2 {
        let pos = self._cursor.borrow().content_start_pos + self.full_content_size; // + self.scroll; // + self.scroll + self.full_content_size
        pos.round()
    }

    pub fn content_start_pos(&self) -> Vec2 {
        let pos = self._cursor.borrow().content_start_pos; // + self.scroll;
        pos.round()
    }

    pub fn visible_content_start_pos(&self) -> Vec2 {
        let pos = self.pos + Vec2::new(0.0, self.titlebar_height) + Vec2::splat(self.padding);
        pos.round()
    }

    // TODO[CHECK]: when / how / what does this exactly do
    /// sets the new panel position
    ///
    /// will also update the cursor so we dont get items lagging behind
    pub fn move_panel_to(&mut self, pos: Vec2) {
        let pos = pos.round();
        let mut c = self._cursor.get_mut();
        let prev_pos = self.pos;
        self.pos = pos;

        // TODO[CHECK]: scroll?
        let pos_d = c.pos - prev_pos;
        let max_pos_d = c.max_pos - prev_pos;
        let content_start_pos_d = c.content_start_pos - prev_pos;

        c.pos = pos_d + pos;
        c.max_pos = max_pos_d + pos;
        c.content_start_pos = content_start_pos_d + pos;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockNodeKind {
    Split {
        // first id is top / left, second id is bottom / right depending on axis
        children: [Id; 2],
        axis: Axis,
        ratio: f32,
    },
    Leaf,
}

impl DockNodeKind {
    pub fn is_split(&self) -> bool {
        matches!(self, DockNodeKind::Split { .. })
    }

    pub fn is_leaf(&self) -> bool {
        matches!(self, DockNodeKind::Leaf)
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DockNode {
    pub id: Id,
    pub parent_id: Id,
    pub kind: DockNodeKind,
    pub rect: Rect,
    pub panel_id: Id,
}

#[derive(Debug, Clone)]
pub struct DockTree {
    pub nodes: IdMap<DockNode>,
}

impl DockTree {
    pub fn new() -> Self {
        Self {
            nodes: IdMap::new(),
            // roots: vec![],
        }
    }

    pub fn recompute_rects(&mut self, node_id: Id, root_rect: Rect) {
        let n = &mut self.nodes[node_id];
        n.rect = root_rect;

        match n.kind {
            DockNodeKind::Split {
                children: [n1, n2],
                axis,
                ratio,
            } => {
                let mut n1_size = root_rect.size();
                let mut n2_size = root_rect.size();
                n1_size[axis as usize] *= ratio;
                n2_size[axis as usize] *= 1.0 - ratio;

                let mut n1_rect = root_rect;
                n1_rect.set_size(n1_size);

                let mut n2_rect = root_rect;
                // shift min of n2 along the split axis by n1's size in that axis
                n2_rect.min[axis as usize] += n1_size[axis as usize];
                n2_rect.set_size(n2_size);

                self.recompute_rects(n1, n1_rect);
                self.recompute_rects(n2, n2_rect);
            }
            DockNodeKind::Leaf => {
                n.rect = root_rect;
            }
        }
    }

    pub fn add_root(&mut self, rect: Rect, panel_id: Id) -> Id {
        let id = Id::from_hash(&panel_id);
        let node = DockNode {
            id,
            kind: DockNodeKind::Leaf,
            rect,
            parent_id: Id::NULL,
            panel_id,
            // panel: panel_id,
        };

        self.nodes.insert(id, node);
        // self.roots.push(id);
        id
    }


    pub fn get_neighbor(&self, id: Id, dir: Dir) -> Id {
        fn descend_to_leaf(tree: &DockTree, mut node_id: Id, child_idx: usize) -> Id {
            loop {
                let node = &tree.nodes[node_id];
                match &node.kind {
                    DockNodeKind::Split { children, .. } => {
                        node_id = children[child_idx];
                        // node_id = if prefer_rightmost {
                        //     children[1]
                        // } else {
                        //     children[0]
                        // };
                    }
                    DockNodeKind::Leaf => return node_id,
                }
            }
        }

        let (target_axis, c_idx) = match dir {
            Dir::N => (Axis::Y, 1),
            Dir::E => (Axis::X, 0),
            Dir::S => (Axis::Y, 0),
            Dir::W => (Axis::X, 1),
            _ => panic!(),
        };

        let mut cur = id;
        while !cur.is_null() {
            let node = &self.nodes[cur];
            let parent = node.parent_id;
            if parent.is_null() {
                break;
            }
            let pnode = &self.nodes[parent];
            if let DockNodeKind::Split { children, axis, .. } = &pnode.kind {
                let child_index = if children[0] == cur {
                    0
                } else if children[1] == cur {
                    1
                } else {
                    cur = parent;
                    continue;
                };
                if *axis == target_axis && child_index == c_idx {
                    let sibling = children[1 - child_index];
                    return descend_to_leaf(self, sibling, c_idx);
                }
            }
            cur = parent;
        }
        Id::NULL
    }

    pub fn get_neighbors(&self, id: Id) -> [Id; 4] {
        [Dir::N, Dir::E, Dir::S, Dir::W].map(|dir| self.get_neighbor(id, dir))
    }

    pub fn get_split_range(&self, id: Id) -> f32 {
        fn descend_to_leaf(tree: &DockTree, mut node_id: Id, child_idx: usize) -> Id {
            loop {
                let node = &tree.nodes[node_id];
                match &node.kind {
                    DockNodeKind::Split { children, .. } => {
                        node_id = children[child_idx];
                    }
                    DockNodeKind::Leaf => return node_id,
                }
            }
        }

        let split_node = &self.nodes[id];
        let DockNodeKind::Split { children, axis, .. } = split_node.kind else {
            panic!()
        };

        // Find the rightmost leaf of left child and leftmost leaf of right child
        let c1 = descend_to_leaf(self, children[0], 1);
        let c2 = descend_to_leaf(self, children[1], 0);
        let r1 = self.nodes[c1].rect;
        let r2 = self.nodes[c2].rect;

        // Compute size along the split axis (perpendicular to the split line)
        match axis {
            Axis::X => r1.width() + r2.width(),
            Axis::Y => r1.height() + r2.height(),
        }
    }

    // pub fn get_split_range(&self, id: Id) -> f32 {
    //     fn descend_to_leaf(tree: &DockTree, mut node_id: Id, child_idx: usize) -> Id {
    //         loop {
    //             let node = &tree.nodes[node_id];
    //             match &node.kind {
    //                 DockNodeKind::Split { children, .. } => {
    //                     node_id = children[child_idx];
    //                     // node_id = if prefer_rightmost {
    //                     //     children[1]
    //                     // } else {
    //                     //     children[0]
    //                     // };
    //                 }
    //                 DockNodeKind::Leaf => return node_id,
    //             }
    //         }
    //     }

    //     let split_node = self.nodes[id];
    //     let DockNodeKind::Split { children, axis, .. } = split_node.kind else {
    //         panic!()
    //     };

    //     let c1 = descend_to_leaf(self, children[0], 1);
    //     let c2 = descend_to_leaf(self, children[1], 0);
    //     let r1 = self.nodes[c1].rect;
    //     let r2 = self.nodes[c2].rect;

    //     match axis {
    //         Axis::X => r2.right() - r1.left(),
    //         Axis::Y => r2.bottom() - r1.top(),
    //     }
    // }



    pub fn get_split_node(&self, id: Id, dir: Dir) -> Id {
        let (target_axis, c_idx) = match dir {
            Dir::N => (Axis::Y, 1),
            Dir::E => (Axis::X, 0),
            Dir::S => (Axis::Y, 0),
            Dir::W => (Axis::X, 1),
            _ => panic!(),
        };

        let mut cur = id;
        while !cur.is_null() {
            let node = &self.nodes[cur];
            let parent = node.parent_id;
            if parent.is_null() {
                break;
            }
            let pnode = &self.nodes[parent];
            if let DockNodeKind::Split { children, axis, .. } = &pnode.kind {
                let child_index = if children[0] == cur {
                    0
                } else if children[1] == cur {
                    1
                } else {
                    cur = parent;
                    continue;
                };
                if *axis == target_axis && child_index == c_idx {
                    return parent;
                }
            }
            cur = parent;
        }
        Id::NULL
    }

    pub fn get_tree(&mut self, mut node_id: Id) -> Vec<Id> {
        let root = self.get_root(node_id);
        let mut out = Vec::new();
        let mut stack = vec![root];

        while let Some(cur) = stack.pop() {
            out.push(cur);
            match &self.nodes[cur].kind {
                DockNodeKind::Split { children, .. } => {
                    stack.push(children[1]);
                    stack.push(children[0]);
                }
                DockNodeKind::Leaf => {}
            }
        }

        out
    }

    pub fn get_root(&mut self, mut node_id: Id) -> Id {
        let mut node = &self.nodes[node_id];
        while !node.parent_id.is_null() {
            node = &self.nodes[node.parent_id];
        }

        node.id
    }

    pub fn merge_nodes(&mut self, target_id: Id, docking_id: Id, mut ratio: f32, dir: Dir) -> Id {
        assert!(ratio < 1.0 && ratio > 0.0);
        assert!(self.nodes[target_id].kind.is_leaf());

        let original = self.nodes[target_id].clone();
        let parent_rect = original.rect;

        // create a new id for the existing content that used to live in `target_id`
        let new_old_id = Id::from_hash(&(original.id.0 + 1));

        let mut old_leaf = original;
        old_leaf.id = new_old_id;
        old_leaf.parent_id = target_id;
        old_leaf.kind = DockNodeKind::Leaf;
        old_leaf.rect = Rect::NAN;

        match dir {
            Dir::E | Dir::S => ratio = 1.0 - ratio,
            _ => (),
        }

        // set docking node's parent to the target (assumes docking node is a root or otherwise handled by caller)
        self.nodes[docking_id].parent_id = target_id;

        let axis = match dir {
            Dir::N | Dir::S => Axis::Y,
            Dir::E | Dir::W => Axis::X,
            _ => unreachable!(),
        };

        // children ordering: children[0] is top/left, children[1] is bottom/right
        let (c0, c1) = match dir {
            Dir::W | Dir::N => (docking_id, new_old_id), // docking goes to left/top
            _ => (new_old_id, docking_id),               // docking goes to right/bottom
        };

        // replace target node kind with a split and insert the preserved old leaf
        self.nodes[target_id].panel_id = Id::NULL;
        self.nodes[target_id].kind = DockNodeKind::Split {
            children: [c0, c1],
            axis,
            ratio,
        };

        self.nodes.insert(new_old_id, old_leaf);

        self.recompute_rects(target_id, parent_rect);
        new_old_id
    }

    pub fn undock_node(&mut self, node_id: Id, panels: &mut IdMap<Panel>) {
        use DockNodeKind as DNK;
        let n = self.nodes[node_id];
        assert!(panels[n.panel_id].dock_id == node_id);
        assert!(n.kind.is_leaf());

        // undock this panel
        // panels[n.panel_id].dock_id = Id::NULL;
        let p = &mut panels[n.panel_id];
        p.dock_id = Id::NULL;
        p.size = p.size_pre_dock;
        p.size_pre_dock = Vec2::NAN;

        // if it's a root leaf just remove it
        if n.parent_id.is_null() {
            self.nodes.remove(n.id);
            return;
        }

        let parent_id = n.parent_id;
        let parent = self.nodes[parent_id];
        match parent.kind {
            DNK::Split { children, .. } => {
                let rem_id = if children[0] == node_id {
                    children[1]
                } else {
                    assert!(children[1] == node_id);
                    children[0]
                };

                let grand_id = parent.parent_id;

                if grand_id.is_null() {
                    match self.nodes[rem_id].kind {
                        DNK::Leaf => {
                            // two-leaf root: remove both children and the parent, undock sibling too
                            let rem_panel_id = self.nodes[rem_id].panel_id;
                            panels[rem_panel_id].dock_id = Id::NULL;

                            // set sibling to size of the root
                            let parent_rect = parent.rect;
                            panels[rem_panel_id].size = parent_rect.size();
                            panels[rem_panel_id].pos = parent_rect.min;

                            self.nodes.remove(rem_id);
                            self.nodes.remove(n.id);
                            self.nodes.remove(parent_id);
                        }
                        DNK::Split { .. } => {
                            // promote rem_id to be the new root
                            self.nodes[rem_id].parent_id = Id::NULL;
                            self.nodes.remove(n.id);
                            self.nodes.remove(parent_id);

                            let parent_rect = parent.rect;
                            // self.nodes[rem_id].rect = parent_rect;
                            self.recompute_rects(rem_id, parent_rect);
                        }
                    }
                } else {
                    // replace parent with rem_id in grandparent
                    {
                        let gp = &mut self.nodes[grand_id];
                        match &mut gp.kind {
                            DNK::Split { children, .. } => {
                                if children[0] == parent_id {
                                    children[0] = rem_id;
                                } else {
                                    assert!(children[1] == parent_id);
                                    children[1] = rem_id;
                                }
                            }
                            DNK::Leaf => unreachable!(),
                        }
                    }

                    self.nodes[rem_id].parent_id = grand_id;

                    self.nodes.remove(n.id);
                    self.nodes.remove(parent_id);

                    let parent_rect = parent.rect;
                    self.recompute_rects(rem_id, parent_rect);
                }
            }
            DNK::Leaf => unreachable!(),
        }
    }

    // pub fn undock_node(&mut self, node_id: Id, panels: &mut IdMap<Panel>) {
    //     let n = self.nodes[node_id];
    //     assert!(panels[n.panel_id].dock_id == node_id);
    //     assert!(n.kind == DockNodeKind::Leaf);

    //     if n.parent_id.is_null() {
    //         log::warn!("undocking single leaf node");
    //         self.nodes.remove(n.id);
    //         return
    //     }

    //     let mut p_n = &mut self.nodes[n.parent_id];
    //     match p_n.kind {
    //         DockNodeKind::Split { children, axis, ratio } => {
    //             let rem_id = if children[0] == node_id {
    //                 children[1]
    //             } else {
    //                 assert!(children[1] == node_id);
    //                 children[0]
    //             };

    //             panels[n.panel_id].dock_id = Id::NULL;
    //             p_n.kind = DockNodeKind::Leaf;
    //             assert!(p_n.panel_id.is_null());

    //             let rem_panel_id = self.nodes[rem_id].panel_id;
    //             let rem_panel = &mut panels[rem_panel_id];
    //             assert!(rem_panel.dock_id == rem_id);
    //             rem_panel.dock_id = n.parent_id;

    //             self.nodes.remove(rem_id);
    //             self.nodes.remove(n.id);

    //             // if p_n.parent.is_null() {
    //             //     // remove reminding if it is root, no dangling node
    //             //     panels[p_n.panel_id].dock_id = Id::NULL;
    //             //     self.nodes.remove(rem_id);
    //             // } else {
    //             // // promote sibling to parent
    //             //     p_n.kind = DockNodeKind::Leaf;
    //             //     self.nodes.remove(rem_id);
    //             //     // self.nodes.insert(rem_id, p_n);
    //             //     // self.nodes.remove(node_id);
    //             //     panels[p_n.panel_id].dock_id = rem_id;

    //             //     // let gp_n = &mut self.nodes[p_n.parent];
    //             //     // match gp_n.kind {
    //             //     //     DockNodeKind::Split { children, axis, ratio } => {
    //             //     //     },
    //             //     //     DockNodeKind::Leaf => panic!(),
    //             //     // }
    //             // }
    //         },
    //         DockNodeKind::Leaf => panic!(),
    //     }
    // }

    // /// instead of split_node which creates two new nodes we dock one existing node into another
    // /// node
    // pub fn merge_nodes(&mut self, target_id: Id, docking_id: Id, mut ratio: f32, dir: Dir) -> Id {
    //     assert!(ratio < 1.0 && ratio > 0.0);
    //     let target = &self.nodes[target_id];
    //     assert!(target.kind == DockNodeKind::Leaf);
    //     let mut n1_id = Id::from_hash(&(target.id.0 + 0));
    //     let mut n2_id = target_id;

    //     match dir {
    //         Dir::E | Dir::S => ratio = 1.0 - ratio,
    //         _ => (),
    //     }

    //     let parent_rect = target.rect;

    //     let n1 = DockNode {
    //         id: n1_id,
    //         kind: DockNodeKind::Leaf,
    //         rect: Rect::NAN,
    //         parent: target_id,
    //     };

    //     let axis = match dir {
    //         Dir::N | Dir::S => Axis::Y,
    //         Dir::E | Dir::W => Axis::X,
    //         _ => unreachable!(),
    //     };

    //     self.nodes[target_id].kind = DockNodeKind::Split {
    //         children: [n1_id, n2_id],
    //         axis,
    //         ratio,
    //     };

    //     self.nodes.insert(n1_id, n1);
    //     self.recompute_rects(target_id, parent_rect);

    //     n1_id
    // }

    pub fn resize(&mut self, node_id: Id, dir: Dir, new_size: Rect) {}

    pub fn split_node2(&mut self, node_id: Id, mut ratio: f32, dir: Dir) -> (Id, Id) {
        assert!(ratio < 1.0 && ratio > 0.0);
        let node = &self.nodes[node_id];
        assert!(node.kind == DockNodeKind::Leaf);
        let mut n1_id = Id::from_hash(&(node.id.0 + 0));
        let mut n2_id = Id::from_hash(&(node.id.0 + 1));

        match dir {
            Dir::E | Dir::S => ratio = 1.0 - ratio,
            _ => (),
        }

        let parent_rect = node.rect;

        let n1 = DockNode {
            id: n1_id,
            kind: DockNodeKind::Leaf,
            rect: Rect::NAN,
            parent_id: node_id,
            panel_id: Id::NULL,
        };

        let n2 = DockNode {
            id: n2_id,
            kind: DockNodeKind::Leaf,
            rect: Rect::NAN,
            parent_id: node_id,
            panel_id: Id::NULL,
        };

        let axis = match dir {
            Dir::N | Dir::S => Axis::Y,
            Dir::E | Dir::W => Axis::X,
            _ => unreachable!(),
        };

        self.nodes[node_id].panel_id = Id::NULL;
        self.nodes[node_id].kind = DockNodeKind::Split {
            children: [n1_id, n2_id],
            axis,
            ratio,
        };

        self.nodes.insert(n1_id, n1);
        self.nodes.insert(n2_id, n2);

        self.recompute_rects(node_id, parent_rect);

        match dir {
            Dir::W | Dir::N => std::mem::swap(&mut n1_id, &mut n2_id),
            _ => (),
        }

        (n1_id, n2_id)
    }
}

id_type!(Id);
id_type!(TextureId);

impl Id {
    pub fn from_str(str: &str) -> Id {
        use hash::{Hash, Hasher};
        let str = match str.find("##") {
            Some(idx) => &str[idx..],
            None => &str,
        };

        Self::from_hash(&str)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub pos: Vec2,
    pub max_pos: Vec2,
    pub content_start_pos: Vec2,
    pub pos_prev_line: Vec2,
    pub line_height: f32,
    pub prev_line_height: f32,
    pub is_same_line: bool,

    pub indent: f32,
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

impl From<f32> for CornerRadii {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
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
    window_titlebar_height: f32,

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
    panel_dark_bg: RGBA,

    panel_corner_radius: f32,
    panel_outline: Outline,
    panel_hover_outline: Outline,
    panel_padding: f32,

    scrollbar_width: f32,
    scrollbar_padding: f32,

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
    pub initial_width: f32,
    pub initial_height: f32,
    pub initial_pos: Vec2,

    pub pos: Vec2,
    pub placement: PanelPlacement,
    pub size: Vec2,
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
            initial_width: f32::NAN,
            initial_height: f32::NAN,
            initial_pos: Vec2::NAN,

            pos: Vec2::NAN,
            placement: PanelPlacement::TopLeft,
            size: Vec2::NAN,
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
    pub clipped_rect: Rect,
    pub is_clipped: bool,
    pub is_hidden: bool,
    pub is_active: bool,
}

impl PrevItemData {
    pub fn new() -> Self {
        Self {
            id: Id::NULL,
            rect: Rect::ZERO,
            clipped_rect: Rect::ZERO,
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
    DragSplit {
        dir: Dir,
        dock_split_id: Id,
        prev_ratio: f32,
    },
    Resize {
        dir: Dir,
        id: Id,
        prev_rect: Rect,
    },
    Move {
        start_pos: Vec2,
        id: Id,
        dock_target: Id,
        drag_by_titlebar: bool,
        drag_by_title_handle: bool,
        // left-click cancels docking for the current panel being dragged
        cancelled_docking: bool,
    },
    Scroll {
        axis: usize,
        start_scroll: Vec2,
        press_offset: Vec2,
        scroll_rect: Rect,
        id: Id,
    },
    None,
}

impl fmt::Display for PanelAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DragSplit {
                dir,
                dock_split_id: split_dock_id,
                prev_ratio,
            } => {
                write!(f, "DRAG_SPLIT[{dir:?}] {{ {split_dock_id}, {prev_ratio} }}")
            }
            Self::Resize { dir, id, prev_rect } => {
                write!(f, "RESIZE[{dir:?}] {{ {id}, {prev_rect} }}")
            }
            Self::Move {
                start_pos,
                id,
                dock_target,
                cancelled_docking,
                drag_by_titlebar,
                drag_by_title_handle,
            } => write!(
                f,
                "MOVE {{ {id}, {start_pos}, dock_target: {dock_target}, cancel_dock: {cancelled_docking}, drag_tb: {drag_by_titlebar}, drag_title: {drag_by_title_handle} }}"
            ),
            Self::Scroll {
                start_scroll: start_offset,
                id,
                ..
            } => write!(f, "SCROLL {{ {id}, {start_offset} }}"),
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

    pub fn is_resize(&self) -> bool {
        matches!(self, Self::Resize { .. })
    }

    pub fn is_move(&self) -> bool {
        matches!(self, Self::Move { .. })
    }

    pub fn is_scroll(&self) -> bool {
        matches!(self, Self::Scroll { .. })
    }
}

#[derive(Debug, Default, Clone)]
pub struct IdMap<T> {
    pub map: HashMap<Id, T>,
}

impl<T> IdMap<T> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
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

    pub fn get(&self, id: Id) -> Option<&T> {
        if id.is_null() {
            return None;
        }
        self.map.get(&id)
    }

    pub fn get_mut(&mut self, id: Id) -> Option<&mut T> {
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

    pub fn insert(&mut self, id: Id, panel: T) {
        assert!(!id.is_null());
        self.map.insert(id, panel);
    }

    pub fn remove(&mut self, id: Id) {
        assert!(!id.is_null());
        self.map.remove(&id);
    }

    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, Id, T> {
        (&self.map).iter()
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&Id, &mut T) -> bool,
    {
        self.map.retain(f);
    }
}

impl<T> IntoIterator for IdMap<T> {
    type Item = (Id, T);
    type IntoIter = std::collections::hash_map::IntoIter<Id, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a IdMap<T> {
    type Item = (&'a Id, &'a T);
    type IntoIter = std::collections::hash_map::Iter<'a, Id, T>;
    fn into_iter(self) -> Self::IntoIter {
        (&self.map).iter()
    }
}

impl<'a, T> IntoIterator for &'a mut IdMap<T> {
    type Item = (&'a Id, &'a mut T);
    type IntoIter = std::collections::hash_map::IterMut<'a, Id, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter_mut()
    }
}

impl<T> FromIterator<(Id, T)> for IdMap<T> {
    fn from_iter<I: IntoIterator<Item = (Id, T)>>(iter: I) -> Self {
        IdMap {
            map: HashMap::from_iter(iter),
        }
    }
}

impl<T> Extend<(Id, T)> for IdMap<T> {
    fn extend<I: IntoIterator<Item = (Id, T)>>(&mut self, iter: I) {
        self.map.extend(iter);
    }
}

impl<T> std::ops::Index<Id> for IdMap<T> {
    type Output = T;

    fn index(&self, id: Id) -> &Self::Output {
        self.get(id).unwrap()
    }
}

impl<T> std::ops::IndexMut<Id> for IdMap<T> {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        self.get_mut(id).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabBar {
    pub panel_id: Id,
    pub id: Id,
    pub selected_tab_id: Id,
    // pub next_selected_tab_id: Id,
    pub bar_rect: Rect,
    pub cursor_backup: Cursor,
    pub tabs: Vec<TabItem>,

    pub is_dragging: bool,
    pub dragging_offset: f32,
}

impl TabBar {
    pub fn new() -> Self {
        Self {
            panel_id: Id::NULL,
            id: Id::NULL,
            cursor_backup: Cursor::default(),
            selected_tab_id: Id::NULL,
            // next_selected_tab_id: Id::NULL,
            bar_rect: Rect::ZERO,
            tabs: vec![],
            is_dragging: false,
            dragging_offset: f32::NAN,
        }
    }

    pub fn layout_tabs(&mut self) {
        let mut offset = 0.0;
        for tab in &mut self.tabs {
            tab.offset = offset;
            offset += tab.width;
            offset += 5.0;
        }
    }

    pub fn find_tab(&self, id: Id) -> Option<&TabItem> {
        assert!(!id.is_null());
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn find_mut_tab(&mut self, id: Id) -> Option<&mut TabItem> {
        assert!(!id.is_null());
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn get_insert_pos(&self, pos: f32, width: f32, current_idx: usize) -> usize {
        if self.tabs.is_empty() {
            return 0;
        }

        let drag_center = pos + width * 0.5;

        // Add deadzone: require crossing significantly past the midpoint to trigger a swap
        let deadzone = if current_idx < self.tabs.len() {
            self.tabs[current_idx].width * 0.25 // 25% of current tab width
        } else {
            20.0 // Default deadzone
        };

        // Find which tab position the drag center belongs to
        let mut insert_idx = 0;

        for (i, tab) in self.tabs.iter().enumerate() {
            let tab_start = self.bar_rect.min.x + tab.offset;
            let tab_end = tab_start + tab.width;
            let tab_center = tab_start + tab.width * 0.5;

            if i == current_idx {
                // Skip the current tab in calculations
                continue;
            }

            // Check if drag center is past this tab's adjusted center
            let threshold = if i < current_idx {
                // Moving left: need to cross center + deadzone
                tab_center + deadzone
            } else {
                // Moving right: need to cross center - deadzone
                tab_center - deadzone
            };

            if drag_center < threshold {
                insert_idx = i;
                break;
            }
            insert_idx = i + 1;
        }

        // Adjust for removal of current tab
        if insert_idx > current_idx {
            insert_idx -= 1;
        }

        insert_idx.min(self.tabs.len().saturating_sub(1))
    }

    pub fn move_tab(&mut self, orig: usize, new: usize) {
        if orig >= self.tabs.len() || new >= self.tabs.len() || orig == new {
            return;
        }

        let item = self.tabs.remove(orig);
        self.tabs.insert(new, item);

        self.layout_tabs();
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TabItem {
    pub id: Id,
    pub width: f32,
    pub offset: f32,
    pub close_pressed: bool,
}

#[derive(Debug, Clone)]
pub struct TextInputState {
    pub edit: ctext::Editor<'static>,
    pub fonts: FontTable,
    pub multiline: bool,
}

impl TextInputState {
    pub fn new(mut fonts: FontTable, text: TextItem, multiline: bool) -> Self {
        let mut buffer = ctext::Buffer::new(
            &mut fonts.sys(),
            ctext::Metrics {
                font_size: text.font_size(),
                line_height: text.scaled_line_height(),
            },
        );

        let font_attrib = fonts.get_font_attrib(text.font);
        buffer.set_text(
            &mut fonts.sys(),
            &text.string,
            &font_attrib,
            ctext::Shaping::Advanced,
        );

        let edit = ctext::Editor::new(buffer);

        Self {
            edit,
            fonts,
            multiline,
        }
    }

    pub fn layout_text(&self, cache: &mut GlyphCache, wgpu: &WGPU) -> ShapedText {
        use ctext::Edit;

        let buffer = match self.edit.buffer_ref() {
            ctext::BufferRef::Owned(b) => b,
            _ => panic!(),
        };

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

                if let Some(mut glyph) = cache.get_glyph(key, wgpu) {
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

    pub fn has_selection(&self) -> bool {
        use ctext::Edit;
        self.edit.selection_bounds().is_some()
    }

    pub fn copy_selection(&self) -> Option<String> {
        use ctext::Edit;
        self.edit.copy_selection()
    }

    pub fn copy_all(&self) -> String {
        use ctext::Edit;
        let mut text = String::new();

        self.edit.with_buffer(|buf| {
            let n_lines = buf.lines.len();
            for (i, line) in buf.lines.iter().enumerate() {
                text.push_str(line.text());
                if i != n_lines - 1 {
                    text.push('\n');
                }
            }
        });

        text
    }

    pub fn paste(&mut self, text: &str) {
        use ctext::Edit;
        self.edit.insert_string(text, None)
    }

    pub fn delete(&mut self) {
        use ctext::{Action, Edit};
        self.edit.action(&mut self.fonts.sys(), Action::Delete);
    }

    pub fn delete_selection(&mut self) {
        use ctext::Edit;
        self.edit.delete_selection();
    }

    pub fn enter(&mut self) {
        use ctext::{Action, Edit};
        if self.multiline {
            self.edit.action(&mut self.fonts.sys(), Action::Enter);
        }
    }

    pub fn escape(&mut self) {
        use ctext::{Action, Edit};
        self.edit.action(&mut self.fonts.sys(), Action::Escape);
    }

    pub fn backspace(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion};
        let ctrl = mods.control_key();

        let sys = &mut self.fonts.sys();

        if ctrl && self.edit.selection_bounds().is_none() {
            let end = self.edit.cursor();
            self.edit.action(sys, Action::Motion(Motion::LeftWord));
            let start = self.edit.cursor();
            self.edit.delete_range(start, end);
        } else {
            self.edit.action(sys, Action::Backspace)
        }
    }

    pub fn deselect_all(&mut self) {
        use ctext::{Edit, Selection};
        if self.has_selection() {
            self.escape()
        }
    }

    pub fn select_all(&mut self) {
        use ctext::{Edit, Selection};
        let mut line_start = 0;
        let mut indx_start = 0;
        let mut line_end = 0;
        let mut indx_end = 0;
        self.edit.with_buffer(|buff| {
            if !buff.lines.is_empty() {
                line_end = buff.lines.len() - 1;
                indx_end = buff.lines[line_end].text().len();
            }
        });
        let end = ctext::Cursor::new(line_end, indx_end);
        let start = ctext::Cursor::new(line_start, indx_start);
        self.edit.set_cursor(start);
        self.edit.set_selection(Selection::Normal(end));
    }

    pub fn move_cursor_up(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let start = edit.cursor();
            // if ctrl {
            //     edit.action(sys, Action::Motion(Motion::UpWord));
            // } else {
            edit.action(sys, Action::Motion(Motion::Up));
            // }
            edit.set_selection(Selection::Normal(start));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::Up));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Up));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(start);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Up))
            }
        }
    }

    pub fn move_cursor_down(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let start = edit.cursor();
            // if ctrl {
            //     edit.action(sys, Action::Motion(Motion::DownWord));
            // } else {
            edit.action(sys, Action::Motion(Motion::Down));
            // }
            edit.set_selection(Selection::Normal(start));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::Down));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Down));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(end);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Down))
            }
        }
    }

    pub fn move_cursor_right(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let start = edit.cursor();
            if ctrl {
                edit.action(sys, Action::Motion(Motion::RightWord));
            } else {
                edit.action(sys, Action::Motion(Motion::Right));
            }
            edit.set_selection(Selection::Normal(start));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::RightWord));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Right));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(end);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Right))
            }
        }
    }

    pub fn move_cursor_left(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let end = edit.cursor();
            if ctrl {
                edit.action(sys, Action::Motion(Motion::LeftWord));
            } else {
                edit.action(sys, Action::Motion(Motion::Left));
            }
            edit.set_selection(Selection::Normal(end));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::LeftWord));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Left));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(start);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Left))
            }
        }
    }

    pub fn mouse_pressed(&mut self, pos: Vec2) {
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit
            .action(&mut self.fonts.sys(), Action::Click { x: pos.x, y: pos.y })
    }

    pub fn mouse_double_clicked(&mut self, pos: Vec2) {
        // TODO[BUG]: if the cursor is between two words both words are selected
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit.action(
            &mut self.fonts.sys(),
            Action::DoubleClick { x: pos.x, y: pos.y },
        )
    }

    pub fn mouse_triple_clicked(&mut self, pos: Vec2) {
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit.action(
            &mut self.fonts.sys(),
            Action::TripleClick { x: pos.x, y: pos.y },
        )
    }

    // TODO[NOTE]: on first / last line we should not do wrapping selection
    pub fn mouse_dragging(&mut self, pos: Vec2) {
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit
            .action(&mut self.fonts.sys(), Action::Drag { x: pos.x, y: pos.y })
    }
}

//---------------------------------------------------------------------------------------
// END TYPES

// BEGIN FLAGS
//---------------------------------------------------------------------------------------

macros::flags!(ItemFlags: ACTIVATE_ON_RELEASE);
macros::flags!(PanelFlags:
    NO_TITLEBAR,
    NO_FOCUS,
    NO_MOVE,
    NO_RESIZE,
    // TODO[NOTE]: what / when / how to use
    NO_INPUT,
    ONLY_MOVE_FROM_TITLEBAR,
    DRAW_H_SCROLLBAR,
    DRAW_V_SCROLLBAR,
    NO_DOCKING,
    DONT_KEEP_SCROLLBAR_PAD,
    DONT_CLIP_CONTENT,

    USE_PARENT_DRAWLIST,
    USE_PARENT_CLIP,
    IS_CHILD,
);

macros::flags!(TextInputFlags:
    MULTILINE,
    SELECT_ON_ACTIVE,
);

macros::flags!(
    Signal:

    JUST_PRESSED_LEFT,
    JUST_PRESSED_MIDDLE,
    JUST_PRESSED_RIGHT,
    JUST_PRESSED_KEYBOARD,

    PRESSED_LEFT,
    PRESSED_MIDDLE,
    PRESSED_RIGHT,
    PRESSED_KEYBOARD,

    DRAGGING_LEFT,
    DRAGGING_MIDDLE,
    DRAGGING_RIGHT,

    RELEASED_LEFT,
    RELEASED_MIDDLE,
    RELEASED_RIGHT,

    CLICKED_LEFT,
    CLICKED_MIDDLE,
    CLICKED_RIGHT,

    DOUBLE_CLICKED_LEFT,
    DOUBLE_CLICKED_MIDDLE,
    DOUBLE_CLICKED_RIGHT,

    DOUBLE_PRESSED_LEFT,
    DOUBLE_PRESSED_MIDDLE,
    DOUBLE_PRESSED_RIGHT,

    TRIPLE_CLICKED_LEFT,
    TRIPLE_CLICKED_MIDDLE,
    TRIPLE_CLICKED_RIGHT,

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
sig_fn!(just_pressed => JUST_PRESSED_LEFT, JUST_PRESSED_KEYBOARD);
sig_fn!(pressed => PRESSED_LEFT, PRESSED_KEYBOARD);
sig_fn!(clicked => CLICKED_LEFT, PRESSED_KEYBOARD);
sig_fn!(double_clicked => DOUBLE_CLICKED_LEFT);
sig_fn!(double_pressed => DOUBLE_PRESSED_LEFT);
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
            clip_rect: Rect::NAN,
            clip_rect_used: false,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct DrawList {
    pub data: Rc<RefCell<DrawListData>>,
    pub draw_clip_rect: bool,
}

impl DrawList {
    pub fn new() -> Self {
        let data = Rc::new(RefCell::new(DrawListData::new()));
        Self {
            data,
            draw_clip_rect: false,
        }
    }

    pub fn commands(&self) -> Ref<'_, [DrawCmd]> {
        Ref::map(self.data.borrow(), |data| data.cmd_buffer.as_slice())
    }

    pub fn vtx_slice(&self, range: std::ops::Range<usize>) -> Ref<'_, [Vertex]> {
        Ref::map(self.data.borrow(), |data| &data.vtx_buffer[range])
    }

    pub fn idx_slice(&self, range: std::ops::Range<usize>) -> Ref<'_, [u32]> {
        Ref::map(self.data.borrow(), |data| &data.idx_buffer[range])
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.data.borrow().clip_rect
        // .clip_stack
        // .last()
        // .copied()
        // .unwrap_or(Rect::INFINITY)
    }

    pub fn add_draw_rect(&self, rect: DrawRect) {
        self.data.borrow_mut().add_rect_rounded(
            rect.min,
            rect.max,
            rect.uv_min,
            rect.uv_max,
            rect.texture_id,
            rect.fill,
            rect.outline,
            rect.corners,
        );
    }

    pub fn clear(&self) {
        let mut data = self.data.borrow_mut();
        data.clear();
    }

    pub fn draw(&self, itm: impl DrawableRects) {
        itm.add_to_drawlist(self);
    }

    pub fn pop_clip_rect_n(&self, n: u32) {
        let mut data = self.data.borrow_mut();
        for _ in 0..n {
            data.pop_clip_rect();
        }
    }

    pub fn pop_clip_rect(&self) -> Rect {
        self.data.borrow_mut().pop_clip_rect()
    }

    pub fn push_merged_clip_rect(&self, rect: Rect) {
        self.data.borrow_mut().push_merged_clip_rect(rect);
        if self.draw_clip_rect {
            self.add_draw_rect(rect.draw_rect().outline(Outline::inner(RGBA::RED, 2.0)));
        }
    }

    pub fn push_clip_rect(&self, rect: Rect) {
        self.data.borrow_mut().push_clip_rect(rect);
        if self.draw_clip_rect {
            self.add_draw_rect(rect.draw_rect().outline(Outline::inner(RGBA::RED, 2.0)));
        }
    }
    // pub fn vertices(&self) -> Ref<'_, [Vertex]> {
    //     Ref::map(self.data.borrow(), |data| &data.vtx_buffer)
    // }

    // pub fn indices(&self) -> Ref<'_, [u32]> {
    //     Ref::map(self.data.borrow(), |data| &data.idx_buffer)
    // }

    // /// Draw shaped text with optional selection and cursor.
    // /// - `selection_range`: Some((start_glyph_idx, end_glyph_idx)) where `end` is exclusive.
    // /// - `cursor_x`: x position in text-local coordinates (relative to `pos`) where the caret should be drawn.
    // /// - `selection_color` is used to draw the highlight rectangle(s).
    // /// - `selected_text_color` is used to color glyphs inside the selection.
    // pub fn add_editable_text(
    //     &mut self,
    //     pos: Vec2,
    //     text: &ShapedText,
    //     text_color: RGBA,
    //     selection_range: Option<(usize, usize)>,
    //     selection_color: RGBA,
    //     selected_text_color: RGBA,
    //     cursor_x: Option<f32>,
    //     cursor_color: RGBA,
    // ) {
    //     // Draw selection as merged rectangles across contiguous glyphs
    //     if let Some((sel_start, sel_end)) = selection_range {
    //         if sel_start < sel_end && !text.glyphs.is_empty() {
    //             let mut in_range = false;
    //             let mut range_min_x = 0.0f32;
    //             let mut range_max_x = 0.0f32;

    //             for (i, g) in text.glyphs.iter().enumerate() {
    //                 let g_min = g.meta.pos + pos;
    //                 let g_max = g_min + g.meta.size;

    //                 if i >= sel_start && i < sel_end {
    //                     if !in_range {
    //                         in_range = true;
    //                         range_min_x = g_min.x;
    //                         range_max_x = g_max.x;
    //                     } else {
    //                         range_max_x = range_max_x.max(g_max.x);
    //                     }
    //                 } else if in_range {
    //                     // self.rect(
    //                     //     Vec2::new(range_min_x, pos.y),
    //                     //     Vec2::new(range_max_x, pos.y + text.height),
    //                     // )
    //                     // .fill(selection_color)
    //                     // .add();
    //                     in_range = false;
    //                 }
    //             }

    //             if in_range {
    //                 self.add(
    //                     Rect::from_min_size(
    //                     Vec2::new(range_min_x, pos.y),
    //                     Vec2::new(range_max_x, pos.y + text.height))
    //                     .draw_rect().fill(selection_color),
    //                 );
    //                 // .fill(selection_color)
    //                 // .add();
    //             }

    //             // Special-case: empty text or selection that extends past last glyph -> highlight to end
    //             if text.glyphs.is_empty() {
    //                 // self.rect(
    //                 self.add(Rect::from_min_max(
    //                     Vec2::new(pos.x, pos.y),
    //                     Vec2::new(pos.x + text.width, pos.y + text.height)).draw_rect().fill(selection_color));
    //                 // )
    //                 // .fill(selection_color)
    //                 // .add();
    //             } else if sel_end > text.glyphs.len() {
    //                 // If selection extends beyond last glyph, ensure we cover to end of line
    //                 let last = &text.glyphs.last().unwrap();
    //                 let last_min = last.meta.pos + pos;
    //                 let end_x = pos.x + text.width.max(last_min.x + last.meta.size.x);
    //                 self.rect(
    //                     Rect::from_min_max(
    //                     Vec2::new(last_min.x + last.meta.size.x, pos.y),
    //                     Vec2::new(end_x, pos.y + text.height)),
    //                 )
    //                 .fill(selection_color)
    //                 .add();
    //             }
    //         }
    //     }

    //     // Draw cursor (thin vertical rectangle)
    //     if let Some(cx_rel) = cursor_x {
    //         let cx = pos.x + cx_rel;
    //         let caret_w = 1.0_f32;
    //         self.rect(
    //             Vec2::new(cx, pos.y),
    //             Vec2::new(cx + caret_w, pos.y + text.height),
    //         )
    //         .fill(cursor_color)
    //         .add();
    //     }

    //     // Draw glyphs (texture quads) with selected_text_color when inside selection
    //     for (i, g) in text.glyphs.iter().enumerate() {
    //         let min = g.meta.pos + pos;
    //         let max = min + g.meta.size;
    //         let uv_min = g.meta.uv_min;
    //         let uv_max = g.meta.uv_max;

    //         let glyph_color = match selection_range {
    //             Some((s, e)) if i >= s && i < e => selected_text_color,
    //             _ => text_color,
    //         };

    //         self.rect(min, max)
    //             .texture_uv(uv_min, uv_max, 1)
    //             .fill(glyph_color)
    //             .add()
    //     }
    // }
}

/// The draw list itself: holds geometry and draw commands
#[derive(Clone)]
pub struct DrawListData {
    pub vtx_buffer: Vec<Vertex>,
    pub idx_buffer: Vec<u32>,
    pub cmd_buffer: Vec<DrawCmd>,

    pub resolution: f32,
    pub path: Vec<Vec2>,
    pub clip_rect: Rect,
    pub clip_stack: Vec<Rect>,

    pub circle_max_err: f32,
    pub clip_content: bool,
}

impl fmt::Debug for DrawListData {
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

impl Default for DrawListData {
    fn default() -> Self {
        Self {
            vtx_buffer: vec![],
            idx_buffer: vec![],
            cmd_buffer: vec![],
            resolution: 20.0,
            path: vec![],
            clip_stack: vec![],
            clip_rect: Rect::INFINITY,

            circle_max_err: 0.3,
            clip_content: true,
        }
    }
}

fn calc_circle_segment_count(rad: f32, max_err: f32) -> u8 {
    use std::f32::consts::PI;
    let tmp = (PI / (1.0 - rad.min(max_err) / rad).cos()).ceil() as u32;
    tmp.clamp(4, 512) as u8
}

impl DrawListData {
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

        if cmd.clip_rect.is_nan() {
            cmd.clip_rect = rect;
        } else if cmd.clip_rect != rect {
            let cmd = self.begin_new_draw_cmd();
            cmd.clip_rect = rect;
            cmd.clip_rect_used = false;
        }
    }

    pub fn push_merged_clip_rect(&mut self, rect: Rect) {
        if !self.clip_content {
            return;
        }
        let curr_clip = self.clip_rect;
        let clip = rect.intersect(curr_clip);
        self.clip_stack.push(self.clip_rect);
        self.set_clip_rect(clip);
        self.clip_rect = clip;
    }

    pub fn push_clip_rect(&mut self, rect: Rect) {
        if !self.clip_content {
            return;
        }
        self.clip_stack.push(self.clip_rect);
        self.set_clip_rect(rect);
        self.clip_rect = rect;
    }

    pub fn pop_clip_rect(&mut self) -> Rect {
        if !self.clip_content {
            return Rect::INFINITY;
        }
        // let rect = self.clip_stack.pop().unwrap();
        self.clip_rect = self.clip_stack.pop().unwrap();
        self.set_clip_rect(self.clip_rect);
        self.clip_rect
    }

    pub fn current_draw_cmd(&mut self) -> &mut DrawCmd {
        if self.cmd_buffer.is_empty() {
            self.cmd_buffer.push(DrawCmd::default())
        }
        self.cmd_buffer.last_mut().unwrap()
    }

    // pub fn current_clip_rect(&self) -> Rect {
    //     // *self.clip_stack.last().unwrap()
    //     self.clip_stack.last().copied().unwrap_or(Rect::INFINITY)
    // }

    pub fn finish_draw_cmd(&mut self) {
        if self.cmd_buffer.is_empty() {
            log::warn!("finishing empty draw command");
        }
        // finish by starting a new command
        let _ = self.begin_new_draw_cmd();
    }

    pub fn begin_new_draw_cmd(&mut self) -> &mut DrawCmd {
        let last = self.cmd_buffer.last().copied();
        // if let Some(last) = last {
        //     if last.vtx_count == 0 {
        //         return self.cmd_buffer.last_mut().unwrap();
        //     }
        // }

        self.cmd_buffer.push(DrawCmd::default());
        let cmd = self.cmd_buffer.last_mut().unwrap();
        cmd.vtx_offset = self.vtx_buffer.len();
        cmd.idx_offset = self.idx_buffer.len();

        if let Some(last) = last {
            cmd.texture_id = last.texture_id;
            cmd.clip_rect = last.clip_rect;
            cmd.clip_rect_used = last.clip_rect_used;
        }
        cmd
    }

    pub fn push_texture(&mut self, tex_id: u32) {
        if tex_id == 0 {
            return;
        }
        let cmd = self.current_draw_cmd();

        if cmd.texture_id == 0 {
            cmd.texture_id = tex_id;
            return;
        }
        // TODO[CHECK]: is this valid?
        // if cmd.texture_id == 0 {
        //     cmd.texture_id = tex_id;
        // }

        if cmd.texture_id != tex_id {
            let cmd = self.begin_new_draw_cmd();
            cmd.texture_id = tex_id;
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

    pub fn circle(&mut self, center: Vec2, radius: f32) -> DrawRect {
        let r = Vec2::splat(radius);
        let min = center - r;
        let max = center + r;

        DrawRect {
            // draw_list: self,
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

    // pub fn add_text(&mut self, pos: Vec2, text: &ShapedText, col: RGBA) {
    //     for g in text.glyphs.iter() {
    //         let min = g.meta.pos + pos;
    //         let max = min + g.meta.size;
    //         let uv_min = g.meta.uv_min;
    //         let uv_max = g.meta.uv_max;

    //         self.rect(min, max)
    //             .texture_uv(uv_min, uv_max, 1)
    //             .fill(col)
    //             .add()
    //     }
    // }

    //     #[inline]
    //     pub fn push_clipped_vtx_idx(&mut self, vtx: &[Vertex], idx: &[u32]) {
    //         let cmd = self.current_draw_cmd();
    //         let base = cmd.vtx_count as u32;
    //         let clip = self.current_clip_rect();

    //         fn lerp(a: f32, b: f32, t: f32) -> f32 {
    //             a + (b - a) * t
    //         }

    //         fn interp_vertex(a: &Vertex, b: &Vertex, t: f32) -> Vertex {
    //             let mut out = a.clone();
    //             out.pos.x = lerp(a.pos.x, b.pos.x, t);
    //             out.pos.y = lerp(a.pos.y, b.pos.y, t);
    //             out.uv.x = lerp(a.uv.x, b.uv.x, t);
    //             out.uv.y = lerp(a.uv.y, b.uv.y, t);
    //             out.col = a.col.lerp(b.col, t);
    //             out
    //         }

    //         // Pre-allocate and reuse temporary buffers to avoid per-triangle allocations
    //         let tri_count = idx.len() / 3;
    //         let mut out_vtxs: Vec<Vertex> = Vec::with_capacity(tri_count * 6); // triangle clipped -> <= ~6 verts typically
    //         let mut out_idx: Vec<u32> = Vec::with_capacity(tri_count * 6);
    //         let mut poly: Vec<Vertex> = Vec::with_capacity(8);
    //         let mut tmp: Vec<Vertex> = Vec::with_capacity(8);

    //         for tri in idx.chunks_exact(3) {
    //             let i0 = tri[0] as usize;
    //             let i1 = tri[1] as usize;
    //             let i2 = tri[2] as usize;
    //             let v0 = vtx[i0].clone();
    //             let v1 = vtx[i1].clone();
    //             let v2 = vtx[i2].clone();

    //             // trivial reject
    //             if (v0.pos.x < clip.min.x && v1.pos.x < clip.min.x && v2.pos.x < clip.min.x)
    //                 || (v0.pos.x > clip.max.x && v1.pos.x > clip.max.x && v2.pos.x > clip.max.x)
    //                 || (v0.pos.y < clip.min.y && v1.pos.y < clip.min.y && v2.pos.y < clip.min.y)
    //                 || (v0.pos.y > clip.max.y && v1.pos.y > clip.max.y && v2.pos.y > clip.max.y)
    //             {
    //                 continue;
    //             }

    //             poly.clear();
    //             poly.push(v0);
    //             poly.push(v1);
    //             poly.push(v2);

    //             // Helper macro-like inline to clip one edge into tmp, then swap poly/tmp
    //             macro_rules! clip_edge {
    //                 ($inside:expr, $intersect_t:expr) => {
    //                     tmp.clear();
    //                     if !poly.is_empty() {
    //                         for i in 0..poly.len() {
    //                             let a = &poly[i];
    //                             let b = &poly[(i + 1) % poly.len()];
    //                             let ina = $inside(a);
    //                             let inb = $inside(b);
    //                             if ina && inb {
    //                                 tmp.push(b.clone());
    //                             } else if ina && !inb {
    //                                 let t = $intersect_t(a, b);
    //                                 tmp.push(interp_vertex(a, b, t));
    //                             } else if !ina && inb {
    //                                 let t = $intersect_t(a, b);
    //                                 tmp.push(interp_vertex(a, b, t));
    //                                 tmp.push(b.clone());
    //                             }
    //                         }
    //                     }
    //                     std::mem::swap(&mut poly, &mut tmp);
    //                 };
    //             }

    //             // left  : x >= clip.min.x
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.x >= clip.min.x,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dx = b.pos.x - a.pos.x;
    //                     if dx.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.min.x - a.pos.x) / dx
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             // right : x <= clip.max.x
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.x <= clip.max.x,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dx = b.pos.x - a.pos.x;
    //                     if dx.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.max.x - a.pos.x) / dx
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             // top   : y >= clip.min.y
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.y >= clip.min.y,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dy = b.pos.y - a.pos.y;
    //                     if dy.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.min.y - a.pos.y) / dy
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             // bottom: y <= clip.max.y
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.y <= clip.max.y,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dy = b.pos.y - a.pos.y;
    //                     if dy.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.max.y - a.pos.y) / dy
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             let start = out_vtxs.len() as u32;
    //             out_vtxs.extend_from_slice(&poly);

    //             let vcount = poly.len() as u32;
    //             // fan-triangulate the clipped polygon
    //             for i in 1..(vcount - 1) {
    //                 out_idx.push(base + start + 0);
    //                 out_idx.push(base + start + i);
    //                 out_idx.push(base + start + (i + 1));
    //             }
    //         }

    //         if !out_vtxs.is_empty() {
    //             self.vtx_buffer.extend_from_slice(&out_vtxs);
    //         }
    //         if !out_idx.is_empty() {
    //             self.idx_buffer.extend_from_slice(&out_idx);
    //         }

    //         let cmd = self.current_draw_cmd();
    //         cmd.vtx_count += out_vtxs.len();
    //         cmd.idx_count += out_idx.len();
    //     }

    //     #[inline]
    //     pub fn push_clipped_vtx_idx2(&mut self, vtx: &[Vertex], idx: &[u32]) {
    //         let cmd = self.current_draw_cmd();
    //         let base = cmd.vtx_count as u32;
    //         let clip = self.current_clip_rect();

    //         let mut kept: Vec<u32> = Vec::with_capacity(idx.len());
    //         for tri in idx.chunks_exact(3) {
    //             let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
    //             let (v0, v1, v2) = (vtx[i0], vtx[i1], vtx[i2]);

    //             if (v0.pos.x < clip.min.x && v1.pos.x < clip.min.x && v2.pos.x < clip.min.x)
    //                 || (v0.pos.x > clip.max.x && v1.pos.x > clip.max.x && v2.pos.x > clip.max.x)
    //                 || (v0.pos.y < clip.min.y && v1.pos.y < clip.min.y && v2.pos.y < clip.min.y)
    //                 || (v0.pos.y > clip.max.y && v1.pos.y > clip.max.y && v2.pos.y > clip.max.y)
    //             {
    //                 continue;
    //             }

    //             kept.push(base + tri[0]);
    //             kept.push(base + tri[1]);
    //             kept.push(base + tri[2]);
    //         }

    //         self.vtx_buffer.extend_from_slice(vtx);
    //         if !kept.is_empty() {
    //             self.idx_buffer.extend_from_slice(&kept);
    //         }

    //         let cmd = self.current_draw_cmd();
    //         cmd.vtx_count += vtx.len();
    //         cmd.idx_count += kept.len();
    //     }

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

        let offset = Vec2::splat(outline.offset());

        let clip = self.clip_rect;
        let bb = Rect::from_min_max(min - offset, max + offset);
        // if !(clip.contains(min - offset) || clip.contains(max + offset)) {
        if !clip.overlaps(bb) {
            return;
        }

        // log::info!("clip?: clip: {clip}, bb: {bb}");
        if !clip.contains(bb.min) || !clip.contains(bb.max) {
            // log::info!("clipped: clip: {clip}, bb: {bb}");
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
            let clip = self.clip_rect;
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
        let clip = self.clip_rect;

        // Draw outline background first
        let outset = outline.width * 0.5;
        let outline_min = min - Vec2::splat(outset);
        let outline_max = max + Vec2::splat(outset);

        if let Some(outline_clip) = Rect::from_min_max(outline_min, outline_max).clip(clip) {
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
        let clip = self.clip_rect;
        let Some(crect) = Rect::from_min_max(min, max).clip(clip) else {
            return;
        };

        self.push_texture(tex_id);
        let clipped_uvs = compute_clipped_uvs(min, max, crect.min, crect.max, uv_min, uv_max);

        let start = self.vtx_buffer.len();
        self.push_rect_vertices(
            crect.min,
            crect.max,
            clipped_uvs.0,
            clipped_uvs.1,
            tint,
            tex_id,
        );

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRect {
    // pub draw_list: &'a mut DrawList,
    pub min: Vec2,
    pub max: Vec2,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub texture_id: u32,
    pub fill: RGBA,
    pub outline: Outline,
    pub corners: CornerRadii,
}

impl ShapedText {
    pub fn draw_rects(&self, pos: Vec2, col: RGBA) -> Vec<DrawRect> {
        let mut rects = Vec::new();
        for g in self.glyphs.iter() {
            let min = g.meta.pos + pos;
            let max = min + g.meta.size;
            let uv_min = g.meta.uv_min;
            let uv_max = g.meta.uv_max;

            rects.push(
                DrawRect::new(min, max)
                    .fill(col)
                    .texture(1)
                    .uv(uv_min, uv_max),
            );
            // DrawRect::new(min, max)
            //     .texture(1)
            //     .uv(uv_min, uv_max)
            // self.rect(min, max)
            //     .texture_uv(uv_min, uv_max, 1)
            //     .fill(col)
            //     .add()
        }
        rects
    }
}

pub trait DrawableRects {
    fn add_to_drawlist(self, drawlist: &DrawList);
}

impl<'a, I> DrawableRects for I
where
    I: IntoIterator,
    I::Item: DrawableRects,
{
    fn add_to_drawlist(self, drawlist: &DrawList) {
        for drawable in self.into_iter() {
            drawable.add_to_drawlist(drawlist);
        }
    }
}

impl DrawableRects for DrawRect {
    fn add_to_drawlist(self, drawlist: &DrawList) {
        drawlist.data.borrow_mut().add_rect_rounded(
            self.min,
            self.max,
            self.uv_min,
            self.uv_max,
            self.texture_id,
            self.fill,
            self.outline,
            self.corners,
        );
    }
}

impl Rect {
    pub fn draw_rect(self) -> DrawRect {
        DrawRect::new(self.min, self.max)
    }
}

impl DrawRect {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        DrawRect {
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

    pub fn offset(mut self, offset: Vec2) -> Self {
        self.min += offset;
        self.max += offset;
        self
    }

    pub fn fill(mut self, fill: RGBA) -> Self {
        self.fill = fill;
        self
    }

    pub fn outline(mut self, outline: Outline) -> Self {
        self.outline = outline;
        self
    }

    pub fn uv(mut self, uv_min: Vec2, uv_max: Vec2) -> Self {
        self.uv_min = uv_min;
        self.uv_max = uv_max;
        self
    }

    // pub fn texture_uv(mut self, uv_min: Vec2, uv_max: Vec2, id: u32) -> Self {
    //     self.uv_min = uv_min;
    //     self.uv_max = uv_max;
    //     self.texture_id = id;
    //     if self.fill.a == 0.0 {
    //         self.fill = RGBA::WHITE
    //     }
    //     self
    // }

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

    pub fn corners(mut self, corners: impl Into<CornerRadii>) -> Self {
        self.corners = corners.into();
        self
    }

    // pub fn add(self) {
    //     self.draw_list.add_rect_rounded(
    //         self.min,
    //         self.max,
    //         self.uv_min,
    //         self.uv_max,
    //         self.texture_id,
    //         self.fill,
    //         self.outline,
    //         self.corners,
    //     )
    // }
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

#[derive(Debug, Clone)]
pub struct FontTable {
    // pub id_to_name: Vec<(FontId, String)>,
    pub sys: Rc<RefCell<ctext::FontSystem>>,
}

impl FontTable {
    pub fn new() -> Self {
        Self {
            // id_to_name: Default::default(),
            sys: Rc::new(RefCell::new(ctext::FontSystem::new())),
        }
    }

    pub fn sys(&mut self) -> std::cell::RefMut<'_, ctext::FontSystem> {
        self.sys.borrow_mut()
    }
    // TODO[NOTE] remove font id?
    pub fn load_font(&mut self, name: &str, bytes: Vec<u8>) {
        let mut sys = self.sys();
        let db = sys.db_mut();
        let ids = db.load_font_source(ctext::fontdb::Source::Binary(std::sync::Arc::new(bytes)));
        // self.id_to_name.push((id, name.to_string()));
    }

    pub fn get_font_attrib<'a>(&self, name: &'a str) -> ctext::Attrs<'a> {
        // let name = self.id_to_name.get(&id).unwrap();
        let attribs = ctext::Attrs::new().family(ctext::Family::Name(name));
        attribs
    }
}

impl TextItem {
    pub fn layout(&self, fonts: &mut FontTable, cache: &mut GlyphCache, wgpu: &WGPU) -> ShapedText {
        let mut buffer = ctext::Buffer::new(
            &mut fonts.sys(),
            ctext::Metrics {
                font_size: self.font_size(),
                line_height: self.scaled_line_height(),
            },
        );

        let font_attrib = fonts.get_font_attrib(self.font);
        buffer.set_size(&mut fonts.sys(), self.width(), self.height());
        buffer.set_text(
            &mut fonts.sys(),
            &self.string,
            &font_attrib,
            ctext::Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut fonts.sys(), false);

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

                if let Some(mut glyph) = cache.get_glyph(key, wgpu) {
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
}

// fn shape_text_item(
//     itm: TextItem,
//     fonts: &mut FontTable,
//     cache: &mut GlyphCache,
//     wgpu: &WGPU,
// ) -> ShapedText {
//     let mut buffer = ctext::Buffer::new(
//         &mut fonts.sys,
//         ctext::Metrics {
//             font_size: itm.font_size(),
//             line_height: itm.scaled_line_height(),
//         },
//     );

//     let font_attrib = fonts.get_font_attrib(itm.font);
//     buffer.set_size(&mut fonts.sys, itm.width(), itm.height());
//     buffer.set_text(
//         &mut fonts.sys,
//         &itm.string,
//         &font_attrib,
//         ctext::Shaping::Advanced,
//     );
//     buffer.shape_until_scroll(&mut fonts.sys, false);

//     let mut glyphs = Vec::new();
//     let mut width = 0.0;
//     let mut height = 0.0;

//     for run in buffer.layout_runs() {
//         width = run.line_w.max(width);
//         // TODO[CHECK]: is it the sum?
//         // height = run.line_height.max(height);
//         height += run.line_height;

//         for g in run.glyphs {
//             let g_phys = g.physical((0.0, 0.0), 1.0);
//             let mut key = g_phys.cache_key;
//             // TODO[CHECK]: what does this do
//             key.x_bin = ctext::SubpixelBin::Three;
//             key.y_bin = ctext::SubpixelBin::Three;

//             if let Some(mut glyph) = cache.get_glyph(key, fonts, wgpu) {
//                 glyph.meta.pos += Vec2::new(g_phys.x as f32, g_phys.y as f32 + run.line_y);
//                 glyphs.push(glyph);
//             }
//         }
//     }

//     let text = ShapedText {
//         glyphs,
//         width,
//         height,
//     };
//     text
// }

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
    pub alloc: etagere::AtlasAllocator,
    pub min_alloc_uv: Vec2,
    pub max_alloc_uv: Vec2,
    pub size: u32,
    pub cached_glyphs: HashMap<ctext::CacheKey, GlyphMeta>,
    pub swash_cache: ctext::SwashCache,
    pub fonts: FontTable,
}

// TODO[NOTE]: dealloc with garbage collector

impl GlyphCache {
    pub fn new(wgpu: &WGPU, fonts: FontTable) -> Self {
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
            etagere::AtlasAllocator::new(etagere::Size::new(size as i32 + 3, size as i32 + 3));
        let texture = gpu::Texture::new(texture, texture_view);

        Self {
            texture,
            min_alloc_uv: Vec2::INFINITY,
            max_alloc_uv: Vec2::ZERO,
            alloc,
            size,
            cached_glyphs: Default::default(),
            swash_cache: ctext::SwashCache::new(),
            fonts,
        }
    }

    pub fn get_glyph(&mut self, glyph_key: ctext::CacheKey, wgpu: &WGPU) -> Option<Glyph> {
        if let Some(&meta) = self.cached_glyphs.get(&glyph_key) {
            return Some(Glyph {
                texture: self.texture.clone(),
                meta,
            });
        }

        self.alloc_new_glyph(glyph_key, wgpu)
    }

    pub fn alloc_rect(&mut self, mut w: u32, mut h: u32) -> Rect {
        // TODO[CHECK]: account for roundoff error?
        w += 1;
        h += 1;
        let alloc = self
            .alloc
            .allocate(etagere::Size::new(w as i32, h as i32))
            .unwrap();

        let r = alloc.rectangle;

        let min = Vec2::new(r.min.x as f32, r.min.y as f32);
        let max = Vec2::new(r.max.x as f32, r.max.y as f32);

        self.min_alloc_uv = self.min_alloc_uv.min(min / self.texture.size());
        self.max_alloc_uv = self.max_alloc_uv.max(max / self.texture.size());

        Rect::from_min_max(min, max)
    }

    pub fn alloc_data(&mut self, w: u32, h: u32, data: &[u8], wgpu: &WGPU) -> Option<Rect> {
        assert_eq!(w * h * 4, data.len() as u32);
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
        // let pos = Vec2::new(x as f32, -y as f32);
        let size = Vec2::new(w as f32, h as f32);
        let uv_min = Vec2::new(rect.min.x as f32, rect.min.y as f32) / tex_size as f32;
        let uv_max = uv_min + size / tex_size as f32;

        Some(Rect::from_min_max(uv_min, uv_max))
    }

    pub fn alloc_new_glyph(&mut self, glyph_key: ctext::CacheKey, wgpu: &WGPU) -> Option<Glyph> {
        let img = self
            .swash_cache
            .get_image_uncached(&mut self.fonts.sys(), glyph_key)?;
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

        let uv_rect = self.alloc_data(w, h, &data, wgpu)?;
        let pos = Vec2::new(x as f32, -y as f32);
        let size = Vec2::new(w as f32, h as f32);

        let meta = GlyphMeta {
            pos,
            size,
            uv_min: uv_rect.min,
            uv_max: uv_rect.max,
        };
        self.cached_glyphs.insert(glyph_key, meta);

        Some(Glyph {
            texture: self.texture.clone(),
            meta,
        })
    }
}

pub mod phosphor_font {
    // from https://phosphoricons.com/
    pub const X: &'static str = "\u{E4F6}";
    pub const MAXIMIZE_OFF: &'static str = "\u{E0F8}";
    pub const MAXIMIZE: &'static str = "\u{E3F0}";
    pub const MINIMIZE: &'static str = "\u{E32A}";
    pub const CARET_RIGHT: &'static str = "\u{E13A}";
    pub const CARET_DOWN: &'static str = "\u{E136}";
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
        let mut i = 0;
        // println!("n_calls: {}", self.call_list.calls.len());
        for call in &self.call_list.calls {
            // i += 1;

            // if i != 2 && self.call_list.calls.len() == 3 {
            //     continue
            // }
            let clip = call.clip_rect;
            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.set_vertex_buffer(0, self.gpu_vertices.slice(..));
            rpass.set_index_buffer(self.gpu_indices.slice(..), wgpu::IndexFormat::Uint32);
            rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

            let target_size = self.screen_size.floor().as_uvec2();
            let clip_min = clip.min.as_uvec2().max(UVec2::ZERO).min(target_size);
            let clip_max = clip.max.as_uvec2().max(clip_min).min(target_size);
            let clip_size = clip_max - clip_min;

            // let clip_min = clip.min.as_uvec2().clamp(Vec2::ZERO, target_size);
            // let clip_size = clip.size().as_uvec2().clamp(Vec2::ZERO, target_size);
            rpass.set_scissor_rect(clip_min.x, clip_min.y, clip_size.x, clip_size.y);

            let idx_offset = call.idx_ptr as u32;
            let vtx_offset = call.vtx_ptr as i32;
            let n_idx = call.n_idx as u32;
            rpass.draw_indexed(idx_offset..idx_offset + n_idx, vtx_offset, 0..1);
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
