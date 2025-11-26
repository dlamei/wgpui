use cosmic_text as ctext;
use glam::{Mat4, UVec2, Vec2};
use std::{
    cell::{Ref, RefCell},
    fmt, hash,
    rc::Rc,
};
use wgpu::util::DeviceExt;

use crate::{
    core::{
        id_type, stacked_fields_struct, ArrVec, Axis, DataMap, Dir, HashMap, HashSet, Instant, RGBA
    }, gpu::{self, RenderPassHandle, ShaderHandle, WGPUHandle, Window, WindowId, WGPU}, mouse::{Clipboard, CursorIcon, MouseBtn, MouseState}, rect::Rect, ui::{
        self, CornerRadii, DockNodeFlag, DockNodeKind, DockTree, DrawCallList, DrawList,
        DrawableRects, FontTable, GlyphCache, Id, IdMap, ItemFlags, RenderData, NextPanelData,
        Outline, Panel, PanelAction, PanelFlag, PrevItemData, RootId, ShapedText, Signal,
        StyleTable, StyleVar, TabBar, TextInputFlags, TextInputState, TextItem, TextItemCache, TextureId,
    }, Vertex as VertexTyp
};

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
    use ui::StyleField as SF;
    use ui::StyleVar as SV;
    StyleTable::init(|f| {
        let accent = RGBA::hex("#cbdfd4");
        let btn_default = RGBA::hex("#4f5559");
        let dark = RGBA::hex("#1d1d1d");
        let btn_hover = RGBA::hex("#576a76");

        match f {
            SF::TitlebarColor => SV::TitlebarColor(dark),
            SF::TitlebarHeight => SV::TitlebarHeight(26.0),
            SF::WindowTitlebarHeight => SV::WindowTitlebarHeight(40.0),
            SF::TextSize => SV::TextSize(18.0),
            SF::TextCol => SV::TextCol(RGBA::hex("#EEEBE1")),
            SF::LineHeight => SV::LineHeight(24.0),
            SF::BtnRoundness => SV::BtnRoundness(0.15),
            SF::BtnDefault => SV::BtnDefault(btn_default),
            SF::BtnHover => SV::BtnHover(btn_hover),
            SF::BtnPress => SV::BtnPress(accent),
            SF::BtnPressText => SV::BtnPressText(btn_default),
            // SF::WindowBg => SV::WindowBg(RGBA::hex("#5c6b6f")),
            SF::WindowBg => SV::WindowBg(dark),
            SF::PanelBg => SV::PanelBg(RGBA::hex("#343B40")),
            SF::PanelDarkBg => SV::PanelDarkBg(RGBA::hex("#282c34")),
            SF::PanelCornerRadius => SV::PanelCornerRadius(7.0),
            SF::PanelOutline => SV::PanelOutline(Outline::center(dark, 2.0)),
            SF::PanelHoverOutline => SV::PanelHoverOutline(Outline::center(btn_hover, 2.0)),
            SF::ScrollbarWidth => SV::ScrollbarWidth(6.0),
            SF::ScrollbarPadding => SV::ScrollbarPadding(5.0),
            SF::PanelPadding => SV::PanelPadding(10.0),
            SF::SpacingV => SV::SpacingV(1.0),
            SF::SpacingH => SV::SpacingH(12.0),
            SF::Red => SV::Red(RGBA::hex("#e65858")),
        }
    })
}

pub struct Context {
    // pub panels: HashMap<Id, Panel>,
    pub panels: IdMap<Panel>,
    // TODO: cleanup?
    pub widget_data: DataMap<Id>,
    pub docktree: DockTree,
    // pub style: Style,
    pub style: StyleTable,

    pub current_panel_stack: Vec<Id>,
    pub current_panel_id: Id,
    pub draworder: Vec<RootId>,

    pub current_tabbar_id: Id,
    // pub tabbars: IdMap<TabBar>,
    pub tabbar_count: u32,

    pub tabbar_stack: Vec<Id>,


    pub text_input_states: IdMap<TextInputState>,

    // TODO[CHECK]: still needed? how to use exactly
    // pub prev_item_data: PrevItemData,
    pub panel_action: PanelAction,
    // pub resizing_window_dir: Option<Dir>,
    pub next: NextPanelData,

    pub prev_item_id: Id,
    pub kb_focus_next_item: bool,
    pub kb_focus_prev_item: bool,
    pub kb_focus_item_id: Id,

    // TODO[CHECK]: when do we set the panels and item ids?
    // TODO[BUG]: if cursor quickly exists window hot_id may not be set to NULL
    /// the id of the element that is currently hovered
    ///
    /// can either be an item or a panel
    pub hot_id: Id,
    /// the hot_id from the previous frame
    ///
    /// needed because hot_id is reset every frame
    pub prev_hot_id: Id,

    /// the id of the element that is currently active
    ///
    /// Can either be an item or a panel.
    /// This allows e.g. dragging the panel by its titlebar (item) or the panel itself
    pub active_id: Id,
    pub prev_active_id: Id,
    pub active_id_changed: bool,

    /// the id of the hot panel
    ///
    /// the hot_id can only point to elements of the currently hot panel
    pub hot_panel_id: Id,
    pub prev_hot_panel_id: Id,


    /// the id of the active panel
    ///
    /// the active_id can only point to elements of the currently active panel
    pub active_panel_id: Id,
    pub prev_active_panel_id: Id,

    pub hot_tabbar_id: Id,
    pub prev_hot_tabbar_id: Id,

    pub window_panel_id: Id,

    // /// registered textures
    // /// 
    // /// texture id is defined as the index + 1 in this array, 0 is reserved for white texture
    // pub texture_reg: Vec<gpu::Texture>,

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

    pub draw: RenderData,
    pub glyph_cache: RefCell<GlyphCache>,
    pub text_item_cache: RefCell<TextItemCache>,
    pub font_table: FontTable,
    pub icon_uv: Rect,

    pub close_pressed: bool,
    pub window: Window,
    pub requested_windows: Vec<(Vec2, Vec2)>,
    pub ext_window: Option<Window>,
    pub clipboard: Clipboard,

    pub wgpu: WGPUHandle,
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

        // let white_texture = gpu::Texture::create(&wgpu, 1, 1, &[255, 255, 255, 255]);

        Self {
            panels: IdMap::new(),
            widget_data: DataMap::new(),
            docktree: DockTree::new(),
            // style: Style::dark(),
            style: dark_theme(),
            draw: RenderData::new(glyph_cache.texture.clone(), wgpu.clone()),
            current_panel_stack: vec![],

            current_tabbar_id: Id::NULL,
            // tabbars: IdMap::new(),
            tabbar_count: 0,
            tabbar_stack: Vec::new(),
            text_input_states: IdMap::new(),

            current_panel_id: Id::NULL,
            // prev_item_data: PrevItemData::new(),

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

            hot_tabbar_id: Id::NULL,
            prev_hot_tabbar_id: Id::NULL,
            prev_active_id: Id::NULL,

            expect_drag: false,
            // resizing_window_dir: None,
            next: NextPanelData::default(),
            kb_focus_next_item: false,
            kb_focus_prev_item: false,
            kb_focus_item_id: Id::NULL,
            prev_item_id: Id::NULL,

            draworder: Vec::new(),
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

            wgpu,
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
        let wgpu = self.wgpu.clone();
        self.get_mut_window(id).resize(x, y, &wgpu.device);
        // self.window.resize(x, y, &self.wgpu.device)
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
            PhysicalKey::Code(KeyCode::Tab) if !self.active_id.is_null() => {
                if shift {
                    self.kb_focus_prev_item = true;
                } else {
                    self.kb_focus_next_item = true;
                }
            }
            PhysicalKey::Code(KeyCode::Delete) => input.delete(),
            PhysicalKey::Code(KeyCode::Enter) => {
                if input.multiline {
                    input.enter()
                } else {
                    self.active_id = Id::NULL;
                }
            }
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
        // If we recently hovered over a tabbar, attempt to scroll its tabs horizontally.
        // Only consume the wheel event if the tabbar can actually move; otherwise fall through
        // so parent panels can handle scrolling.
        if !self.prev_hot_tabbar_id.is_null() {
            if let Some(tb) = self.widget_data.get_mut::<ui::TabBar>(&self.prev_hot_tabbar_id) {
                let scroll_amount = delta.y;
                let max_scroll = (tb.total_width - tb.bar_rect.width()).max(0.0);
                if max_scroll > 0.0 {
                    let new_offset = (tb.scroll_offset - scroll_amount).clamp(0.0, max_scroll);
                    if (new_offset - tb.scroll_offset).abs() > f32::EPSILON {
                        tb.scroll_offset = new_offset;
                        return;
                    }
                }
            }
        }
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

    pub fn gen_glob_id(&self, label: &str) -> Id {
        Id::from_str(label)
    }

    // TODO: id handling, creating a panel inside another panel that is not a child?
    // maybe gen_panel_id, and another for items
    pub fn gen_id(&self, label: &str) -> Id {
        if self.current_panel_id.is_null() {
            Id::from_str(label)
        } else {
            self.get_current_panel().gen_local_id(label)
        }
    }

    pub fn register_texture(&mut self, tex: &gpu::Texture) -> TextureId {
        if let Some(idx) = self.draw.texture_reg.iter().position(|t| t == tex) {
            return TextureId(idx as u64 + 1);
        }

        let id = self.draw.texture_reg.len();
        self.draw.texture_reg.push(tex.clone());
        TextureId(id as u64 + 1)
    }

    pub fn texture_id(&self, tex: &gpu::Texture) -> TextureId {
        if let Some(idx) = self.draw.texture_reg.iter().position(|t| t == tex) {
            return TextureId(idx as u64);
        }

        panic!("texture not registered");
    }

    pub fn is_in_draw_order(&self, id: RootId) -> bool {
        self.draworder.iter().find(|i| **i == id).is_some()
    }

    pub fn insert_in_draworder(&mut self, id: RootId) {
        debug_assert!(!self.is_in_draw_order(id));
        self.draworder.push(id);
        self.update_draworder();
    }

    pub fn insert_after_in_draworder(&mut self, id1: RootId, id2: RootId) {
        let idx = self.draworder.iter().position(|&i| i == id1).unwrap();
        self.draworder.insert(idx + 1, id2);
        self.update_draworder();
    }

    pub fn replace_in_draworder(&mut self, id1: RootId, id2: RootId) {
        let idx = self.draworder.iter().position(|&i| i == id1).unwrap();
        self.draworder[idx] = id2;
        self.update_draworder();
    }

    pub fn remove_from_draworder(&mut self, id: RootId) {
        let idx = self.draworder.iter().position(|&i| i == id).unwrap();
        self.draworder.remove(idx);
        self.update_draworder();
    }

    pub fn update_draworder(&mut self) {
        let mut order = 1;

        fn update_panel_order(ctx: &mut Context, id: Id, order: &mut usize) {
            let p = &mut ctx.panels[id];
            p.draw_order = *order;
            *order += 1;

            for c in p.children.clone() {
                update_panel_order(ctx, c, order);
            }
        }

        fn update_dock_order(ctx: &mut Context, id: Id, order: &mut usize) {
            let docked = ctx.docktree.get_leafs(id);

            for leaf in docked {
                let leaf = ctx.docktree.nodes[leaf];
                assert!(leaf.kind.is_leaf());
                update_panel_order(ctx, leaf.panel_id, order);
            }
        }

        for r in self.draworder.clone() {
            match r {
                RootId::Panel(id) => update_panel_order(self, id, &mut order),
                RootId::Dock(id) => update_dock_order(self, id, &mut order),
            }
        }
    }

    pub fn reset_docktree(&mut self) {
        self.draworder = self
            .draworder
            .clone()
            .into_iter()
            .map(|r| match r {
                RootId::Panel(_) => vec![r],
                RootId::Dock(id) => self
                    .docktree
                    .get_leafs(id)
                    .into_iter()
                    .map(|dock_id| RootId::Panel(self.docktree.nodes[dock_id].panel_id))
                    .collect(),
            })
            .flatten()
            .collect();

        self.docktree = DockTree::new();

        for (_, p) in &mut self.panels {
            if !p.dock_id.is_null() && !p.size_pre_dock.is_nan() {
                p.size = p.size_pre_dock;
            }

            p.dock_id = Id::NULL;
        }
    }

    pub fn bring_to_front(&mut self, id: RootId) {
        let idx = self.draworder.iter().position(|&i| i == id).unwrap();
        self.draworder.remove(idx);
        self.draworder.push(id);
        self.update_draworder();
    }

    pub fn bring_panel_to_front(&mut self, id: Id) {
        let p = &mut self.panels[id];
        if !p.parent_id.is_null() {
            let idx = p.children.iter().position(|&i| i == id).unwrap();
            p.children.remove(idx);
            p.children.push(id);
            let paren_id = p.parent_id;
            self.bring_panel_to_front(paren_id);
        } else if !p.dock_id.is_null() {
            let dock_root = self.docktree.get_root(p.dock_id);
            if !self.docktree.nodes[dock_root]
                .flags
                .has(DockNodeFlag::NO_BRING_TO_FRONT)
            {
                self.bring_to_front(RootId::Dock(dock_root));
            }
        } else {
            self.bring_to_front(RootId::Panel(id));
        }
    }

    pub fn get_panels_in_order(&self) -> Vec<Id> {
        let mut panels = vec![];

        fn push_panel_panels(ctx: &Context, id: Id, panels: &mut Vec<Id>) {
            panels.push(id);
            let p = &ctx.panels[id];
            for &c in &p.children {
                push_panel_panels(ctx, c, panels);
            }
        }

        fn push_dock_panels(ctx: &Context, id: Id, panels: &mut Vec<Id>) {
            let docked = ctx.docktree.get_leafs(id);

            for leaf in docked {
                let leaf = ctx.docktree.nodes[leaf];
                assert!(leaf.kind.is_leaf());
                push_panel_panels(ctx, leaf.panel_id, panels);
            }
        }

        for &r in &self.draworder {
            match r {
                RootId::Panel(id) => push_panel_panels(self, id, &mut panels),
                RootId::Dock(id) => push_dock_panels(self, id, &mut panels),
            }
        }

        panels
    }

    pub fn begin(&mut self, name: impl Into<String>) {
        self.begin_ex(name, PanelFlag::DRAW_V_SCROLLBAR);
    }

    pub fn begin_dockspace(&mut self) {
        // TODO[CHECK]: hacky
        let win_panel = &self.panels[self.window_panel_id];
        let win_tb_height = win_panel.titlebar_height;
        let win_size = win_panel.size;
        self.next.pos = Vec2::new(0.0, win_tb_height);
        self.next.size = win_size - self.next.pos;
        let dockspace_rect = Rect::from_min_size(self.next.pos, self.next.size);

        self.push_style(StyleVar::PanelBg(RGBA::ZERO));
        self.push_style(StyleVar::PanelOutline(Outline::none()));
        self.push_style(StyleVar::PanelHoverOutline(Outline::none()));

        self.begin_ex(
            "##_DOCK_SPACE",
            PanelFlag::NO_FOCUS
                | PanelFlag::NO_MOVE
                | PanelFlag::NO_RESIZE
                | PanelFlag::ONLY_DOCK_OVER
                | PanelFlag::NO_TITLEBAR,
        );

        self.pop_style_n(3);

        // let dock_space_p_id = self.get_current_panel().id;
        let mut dock_space_id = self.get_current_panel().dock_id;
        if dock_space_id.is_null() {
            dock_space_id = self.docktree.add_root_ex(
                dockspace_rect,
                self.current_panel_id,
                DockNodeFlag::NO_BRING_TO_FRONT | DockNodeFlag::ALLOW_SINGLE_LEAF,
            );
            self.docktree.nodes[dock_space_id].label = Some("DockSpace");
            self.replace_in_draworder(
                RootId::Panel(self.current_panel_id),
                RootId::Dock(dock_space_id),
            );
            self.panels[self.current_panel_id].dock_id = dock_space_id;
        } else {
            let dock_root = self.docktree.get_root(dock_space_id);
            self.docktree.recompute_rects(dock_root, dockspace_rect);
        }
    }

    pub fn panel_id(&mut self, name: impl Into<String>) -> Id {
        self.begin(name);
        let id = self.current_panel_id;
        self.end();
        id
    }

    pub fn begin_ex(&mut self, name: impl Into<String>, flags: PanelFlag) {
        fn next_window_pos(screen: Vec2, panel_size: Vec2) -> Vec2 {
            use std::sync::atomic::{AtomicU32, Ordering};
            static PANEL_COUNT: AtomicU32 = AtomicU32::new(0);

            const OFFSET: f32 = 60.0;
            const DEFAULT_SIZE: Vec2 = Vec2::new(500.0, 300.0);

            let size = if panel_size.is_finite() {
                panel_size
            } else {
                DEFAULT_SIZE
            };

            let count = PANEL_COUNT.fetch_add(1, Ordering::Relaxed);
            let cascade_offset = OFFSET * count as f32;

            let available_width = (screen.x - size.x).max(0.0);
            let available_height = (screen.y - size.y).max(0.0);

            let x = cascade_offset % available_width.max(1.0);
            let y = cascade_offset % available_height.max(1.0);

            Vec2::new(x, y)
        }

        let mut newly_created = false;
        let name: String = name.into();

        let id = if flags.has(PanelFlag::IS_CHILD) {
            self.gen_id(&name)
        } else {
            self.gen_glob_id(&name)
        };

        if !self.panels.contains_id(id) {
            self.create_panel(&name, id);
            self.panels[id].id = id;
            newly_created = true;
        }

        self.panels[id].name = name;

        // clear panels children every frame
        self.panels[id].children.clear();

        // setup child / parent ids
        let (root_id, parent_id) = if flags.has(PanelFlag::IS_CHILD) {
            let parent_id = self.current_panel_id;
            let parent = &mut self.panels[parent_id];
            let root = parent.root;
            parent.children.push(id);

            (root, parent_id)
        } else {
            (id, Id::NULL)
        };

        if newly_created {
            if flags.has(PanelFlag::USE_PARENT_DRAWLIST) {
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
            if !p.flags.has(PanelFlag::IS_CHILD) && p.dock_id.is_null() {
                self.insert_in_draworder(RootId::Panel(id));
            }

            // p.draw_order = self.draw_order.len();
            // self.draw_order.push(id);

            if self.next.pos.is_nan() {
                let p = &mut self.panels[id];
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
        if !flags.has(PanelFlag::USE_PARENT_DRAWLIST) {
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
        p.titlebar_height = if flags.has(PanelFlag::NO_TITLEBAR) {
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
        // p.move_id = p.gen_id("##_MOVE");
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

        if flags.has(PanelFlag::NO_MOVE) {
            // p.move_id = Id::NULL;
        } else if flags.has(PanelFlag::NO_TITLEBAR) {
            // move the window by dragging it if no titlebar exists
            p.titlebar_height = 0.0;
        }

        self.next.reset();
        // if !p.flags.has(PanelFlags::ONLY_MOVE_FROM_TITLEBAR) {
        //     p.nav_root = p.move_id;
        // } else {
        //     p.nav_root = p.root;
        // }

        let (pos_bounds, clamp_pos_bounds) = if flags.has(PanelFlag::IS_CHILD) {
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
                let dock_root = self.docktree.get_root(p.dock_id);
                let dock_rect = self.docktree.nodes[dock_root].rect;

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
                    self.docktree
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
            let [n_n, n_e, n_s, n_w] = self.docktree.get_neighbors(p.dock_id).map(|n| !n.is_null());
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
            let dock_rect = self.docktree.nodes[p.dock_id].rect;
            // p.pos = dock_rect.min;
            p.move_panel_to(dock_rect.min);
            p.size = dock_rect.size();
        }

        let outline_width = self.style.panel_outline().width;
        let full_rect = Rect::from_min_size(p.pos - outline_width, p.size + 2.0 * outline_width);
        let mut clip_rect = p.full_rect;

        if flags.has(PanelFlag::USE_PARENT_CLIP) {
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
            && !p.flags.has(PanelFlag::NO_FOCUS)
        {
            self.hot_panel_id = id;
            self.hot_id = id;
        }

        if let PanelAction::Move {
            id,
            dock_target,
            cancelled_docking,
            drag_by_titlebar,
            drag_by_title_handle,
            ..
        } = &mut self.panel_action
        {
            if p.clip_rect.contains(self.mouse.pos)
                // && self.panels[*id].titlebar_rect().contains(self.mouse.pos)
                // Only allow setting a dock target when dragging by titlebar if the moving panel
                // is not already docked, or when the drag originates from the title handle.
                && *drag_by_titlebar
                && (self.panels[*id].dock_id.is_null() || *drag_by_title_handle)
                && !self.modifiers.shift_key()
            {
                let curr_draw_order = p.draw_order;
                let moving_draw_order = self.panels[*id].draw_order;
                let dock_target_draw_order = if !dock_target.is_null() {
                    self.panels[*dock_target].draw_order
                } else {
                    0
                };

                if !flags.has(PanelFlag::NO_DOCKING)
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

        if flags.has(PanelFlag::USE_PARENT_CLIP) {
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
        if !p.flags.has(PanelFlag::NO_TITLEBAR) {
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

            if close.released() {
                self.panels[id].close_pressed = true;
            }

            if id == self.window_panel_id {
                if min.released() {
                    self.window.minimize();
                }
                if max.released() || tb.double_clicked() {
                    self.window.toggle_maximize();
                }

                let pad = 5.0;
                self.draw(
                    Rect::from_min_max(Vec2::splat(pad), Vec2::splat(titlebar_height - pad))
                        .draw_rect()
                        .uv(self.icon_uv.min, self.icon_uv.max)
                        .texture(TextureId::GLYPH),
                );
                // self.draw(|list| {
                //     list.rect(Vec2::splat(pad), Vec2::splat(titlebar_height - pad))
                //         .texture_uv(self.icon_uv.min, self.icon_uv.max, 1)
                //         .add()
                // });
            }

            // start drawing content
            self.set_cursor_pos(self.content_start_pos());
            // self.prev_item_data.reset();
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
        if y_scroll && flags.has(PanelFlag::DRAW_V_SCROLLBAR) {
            self.draw_scrollbar(1);
        }
        if x_scroll && flags.has(PanelFlag::DRAW_H_SCROLLBAR) {
            self.draw_scrollbar(0);
        }

        let p = &self.panels[id];

        if flags.has(PanelFlag::USE_PARENT_CLIP) {
            self.push_merged_clip_rect(p.visible_content_rect());
        } else {
            self.push_clip_rect(p.visible_content_rect());
        }
    }

    pub(crate) fn draw_scrollbar(&mut self, axis: usize) {
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
        let sig = self.reg_item_active_on_press(scroll_id, scrollbar_rect);
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
        // let move_id = p.move_id;
        let p_id = p.id;

        let title_text = self.layout_text(&title, self.style.text_size());
        let pad = (titlebar_height - title_text.height) / 2.0;

        // Draw titlebar background
        let mut tb_corners = panel_corners;
        tb_corners.bl = 0.0;
        tb_corners.br = 0.0;

        self.draw(
            Rect::from_min_size(panel_pos, Vec2::new(panel_size.x, titlebar_height))
                .draw_rect()
                .fill(self.style.titlebar_color())
                .corners(tb_corners),
        );

        // Calculate button dimensions
        let btn_size = Vec2::new(25.0, 25.0);
        let btn_spacing = 5.0;
        let num_buttons = [close, maximize, minimize].iter().filter(|&&b| b).count() as f32;
        let buttons_width = if num_buttons > 0.0 {
            num_buttons * btn_size.x + (num_buttons - 1.0) * btn_spacing
        } else {
            0.0
        };

        let handle_width = title_text.size().x
            + pad * 2.0
            + buttons_width
            + if buttons_width > 0.0 { pad } else { 0.0 };

        // Draw title handle
        if draw_title_handle {
            self.draw(
                Rect::from_min_size(panel_pos, Vec2::new(handle_width, titlebar_height))
                    .draw_rect()
                    .corners(CornerRadii::top(self.style.panel_corner_radius()))
                    .fill(self.style.panel_bg()),
            );
        }

        // Draw title text
        self.draw(title_text.draw_rects(panel_pos + pad, self.style.text_col()));

        // Register titlebar interaction area
        let tb_sig = self.reg_item_active_on_press(
            p_id,
            Rect::from_min_size(panel_pos, Vec2::new(panel_size.x, titlebar_height)),
        );

        // Calculate button starting position
        let btn_y = (titlebar_height - btn_size.y) / 2.0;
        let mut btn_x = if draw_title_handle {
            title_text.size().x + pad * 2.0
        } else {
            panel_size.x - buttons_width - btn_spacing
        };

        let mut min_sig = Signal::NONE;
        let mut max_sig = Signal::NONE;
        let mut close_sig = Signal::NONE;

        // Draw minimize button
        if minimize {
            let min_id = self.gen_id("##_MIN_ICON");
            let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
            min_sig = self.reg_item_active_on_release(min_id, Rect::from_min_size(btn_pos, btn_size));

            let color = if min_sig.hovering() {
                self.style.btn_hover()
            } else {
                self.style.text_col()
            };

            let min_icon = self.layout_icon(ui::phosphor_font::MINIMIZE, self.style.text_size());
            let icon_pad = (btn_size - min_icon.size()) / 2.0;
            self.draw(min_icon.draw_rects(btn_pos + icon_pad, color));

            btn_x += btn_size.x + btn_spacing;
        }

        // Draw maximize button
        if maximize {
            let max_id = self.gen_id("##_MAX_ICON");
            let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
            max_sig = self.reg_item_active_on_release(max_id, Rect::from_min_size(btn_pos, btn_size));

            let color = if max_sig.hovering() {
                self.style.btn_hover()
            } else {
                self.style.text_col()
            };

            let max_icon = if self.window.is_maximized() {
                self.layout_icon(ui::phosphor_font::MAXIMIZE_OFF, self.style.text_size())
            } else {
                self.layout_icon(ui::phosphor_font::MAXIMIZE, self.style.text_size())
            };
            let icon_pad = (btn_size - max_icon.size()) / 2.0;
            self.draw(max_icon.draw_rects(btn_pos + icon_pad, color));

            btn_x += btn_size.x + btn_spacing;
        }

        // Draw close button
        if close {
            let close_id = self.gen_id("##_CLOSE_ICON");
            let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
            close_sig = self.reg_item_active_on_release(close_id, Rect::from_min_size(btn_pos, btn_size));

            let color = if close_sig.hovering() {
                self.style.red()
            } else {
                RGBA::WHITE
            };

            let x_icon = self.layout_icon(ui::phosphor_font::X, self.style.text_size());
            let icon_pad = (btn_size - x_icon.size()) / 2.0;
            self.draw(x_icon.draw_rects(btn_pos + icon_pad, color));
        }

        (tb_sig, min_sig, max_sig, close_sig, handle_width)
    }

    // pub fn draw_panel_decorations(
    //     &mut self,
    //     draw_title_handle: bool,
    //     minimize: bool,
    //     maximize: bool,
    //     close: bool,
    //     panel_corners: CornerRadii,
    // ) -> (Signal, Signal, Signal, Signal, f32) {
    //     let p = self.get_current_panel();
    //     let titlebar_height = p.titlebar_height;
    //     let panel_pos = p.pos;
    //     let panel_size = p.size;
    //     let title = p.name.clone();
    //     let move_id = p.move_id;

    //     let title_text = self.layout_text(&title, self.style.text_size());
    //     let pad = (titlebar_height - title_text.height) / 2.0;
    //     // draw titlebar background
    //     let mut tb_corners = panel_corners;
    //     tb_corners.bl = 0.0;
    //     tb_corners.br = 0.0;

    //     let min_width = title_text.size().x + pad * 2.0;
    //     self.draw(
    //         Rect::from_min_size(panel_pos, Vec2::new(panel_size.x, titlebar_height))
    //             .draw_rect()
    //             .fill(self.style.titlebar_color())
    //             .corners(tb_corners),
    //     );

    //     if draw_title_handle {
    //         self.draw(
    //             Rect::from_min_size(panel_pos, title_text.size() + Vec2::splat(pad * 2.0))
    //                 .draw_rect()
    //                 .corners(CornerRadii::top(self.style.panel_corner_radius()))
    //                 .fill(self.style.panel_bg()),
    //         );
    //     }

    //     self.draw(title_text.draw_rects(panel_pos + pad, self.style.text_col()));

    //     let tb_sig = self.register_rect(
    //         move_id,
    //         Rect::from_min_size(panel_pos, Vec2::new(panel_size.x, titlebar_height)),
    //     );

    //     let btn_size = Vec2::new(25.0, 25.0);
    //     let btn_spacing = 10.0;
    //     let mut btn_x = panel_size.x - (btn_size.x + btn_spacing);
    //     let btn_y = (titlebar_height - btn_size.y) / 2.0;

    //     let mut min_sig = Signal::NONE;
    //     let mut max_sig = Signal::NONE;
    //     let mut close_sig = Signal::NONE;

    //     // draw close button
    //     if close {
    //         let close_id = self.gen_id("##_CLOSE_ICON");
    //         let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
    //         close_sig = self.register_rect(close_id, Rect::from_min_size(btn_pos, btn_size));

    //         let color = if close_sig.hovering() {
    //             self.style.red()
    //         } else {
    //             RGBA::WHITE
    //         };

    //         let x_icon = self.layout_icon(ui::phosphor_font::X, self.style.text_size());
    //         let pad = btn_size - x_icon.size();
    //         let pos = btn_pos + pad / 2.0;
    //         self.draw(x_icon.draw_rects(pos, color));
    //         btn_x -= btn_size.x + btn_spacing;
    //     }

    //     // draw maximize button
    //     if maximize {
    //         let max_id = self.gen_id("##_MAX_ICON");
    //         let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
    //         max_sig = self.register_rect(max_id, Rect::from_min_size(btn_pos, btn_size));

    //         let color = if max_sig.hovering() {
    //             self.style.btn_hover()
    //         } else {
    //             self.style.text_col()
    //         };

    //         {
    //             let max_icon = if self.window.is_maximized() {
    //                 self.layout_icon(ui::phosphor_font::MAXIMIZE_OFF, self.style.text_size())
    //             } else {
    //                 self.layout_icon(ui::phosphor_font::MAXIMIZE, self.style.text_size())
    //             };
    //             let pad = btn_size - max_icon.size();
    //             let pos = btn_pos + pad / 2.0;
    //             self.draw(max_icon.draw_rects(pos, color));
    //             // list.add_text(pos, &max_icon, color);
    //         }

    //         btn_x -= btn_size.x + btn_spacing;
    //     }

    //     // draw minimize button
    //     if minimize {
    //         let min_id = self.gen_id("##_MIN_ICON");
    //         let btn_pos = panel_pos + Vec2::new(btn_x, btn_y);
    //         min_sig = self.register_rect(min_id, Rect::from_min_size(btn_pos, btn_size));

    //         let color = if min_sig.hovering() {
    //             self.style.btn_hover()
    //         } else {
    //             self.style.text_col()
    //         };

    //         let min_icon = self.layout_icon(ui::phosphor_font::MINIMIZE, self.style.text_size());
    //         let pad = btn_size - min_icon.size();
    //         let pos = btn_pos + pad / 2.0;
    //         self.draw(min_icon.draw_rects(pos, color));
    //     }

    //     (tb_sig, min_sig, max_sig, close_sig, min_width)
    // }

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

    // TODO[bug] handle panel max size when docked
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
                let [n_n, n_e, n_s, n_w] =
                    self.docktree.get_neighbors(p.dock_id).map(|n| !n.is_null());

                let dock_root = self.docktree.get_root(p.dock_id);
                let dock_root_rect = self.docktree.nodes[dock_root].rect;

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
                        let split_dock_id = self.docktree.get_split_node(dock_id, dir);
                        assert!(!split_dock_id.is_null());
                        let DockNodeKind::Split { ratio, .. } =
                            self.docktree.nodes[split_dock_id].kind
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

            let dock_split = &self.docktree.nodes[*dock_split_id];
            let split_rect = dock_split.rect;
            let split_size = split_rect.size()[axis as usize];
            let DockNodeKind::Split {
                children, ratio, ..
            } = dock_split.kind
            else {
                panic!()
            };

            let (split_start, split_end) = self.docktree.get_split_range(*dock_split_id);
            let pad = self.style.titlebar_height() + 5.0;

            let prev_ratio_px = prev_ratio * split_size;
            // let new_ratio_px = prev_ratio_px + m_delta;

            let split_pos = prev_ratio_px + m_delta + split_rect.min[axis as usize];
            // println!("{split_pos}: {split_start}, {split_end}");

            let new_ratio = (split_pos.min(split_end - pad).max(split_start + pad)
                - split_rect.min[axis as usize])
                / split_size;
            self.docktree.set_split_ratio(*dock_split_id, new_ratio);
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
                let dock_root = self.docktree.get_root(p.dock_id);
                self.docktree.recompute_rects(dock_root, nr);
            }
        }
    }

    pub fn get_dock_target(mouse: Vec2, target_area: Rect, flags: PanelFlag) -> (Rect, Dir, f32) {
        if flags.has(PanelFlag::ONLY_DOCK_OVER) {
            return (target_area, Dir::N, 1.0);
        }

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

        const SNAP_THRESHOLD: f32 = 0.06;

        if flags.has(PanelFlag::DOCK_OVER) && ratio > 1.0 - SNAP_THRESHOLD {
            ratio = 1.0;
            return (target_area, Dir::N, ratio);
        }

        if (ratio - 0.5).abs() < SNAP_THRESHOLD {
            ratio = 0.5;
        }

        if ratio > 1.0 - SNAP_THRESHOLD {
            ratio = 1.0 - SNAP_THRESHOLD;
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

    pub fn dock_to_dockspace(&mut self, about_to_dock: Id, ratio: f32, dir: Dir) {
        let dockspace_id = self.gen_glob_id("##_DOCK_SPACE");
        self.dock_to_panel(about_to_dock, dockspace_id, ratio, dir);
    }

    pub fn dock_to_panel(&mut self, about_to_dock: Id, target_panel_id: Id, ratio: f32, dir: Dir) {
        let curr_size = self.panels[about_to_dock].size;
        self.panels[about_to_dock].size_pre_dock = curr_size;
        let curr_dock_id = self.panels[about_to_dock].dock_id;

        let target_panel = &mut self.panels[target_panel_id];

        if target_panel.dock_id.is_null() {
            assert!(
                target_panel.parent_id.is_null(),
                "currently children panels should not be dockable"
            );
            // init target panel as dock node
            target_panel.size_pre_dock = target_panel.size;
            let dock_id = self
                .docktree
                .add_root(target_panel.panel_rect(), target_panel.id);
            target_panel.dock_id = dock_id;

            let target_panel_id = target_panel.id;
            self.replace_in_draworder(RootId::Panel(target_panel_id), RootId::Dock(dock_id));
        }

        let target_panel = &mut self.panels[target_panel_id];

        if !curr_dock_id.is_null() {
            let root_dock_id = self.docktree.get_root(curr_dock_id);
            let id = self
                .docktree
                .merge_nodes(target_panel.dock_id, root_dock_id, ratio, dir);
            target_panel.dock_id = id;

            self.remove_from_draworder(RootId::Dock(root_dock_id));
        } else {
            let (l, r) = self.docktree.split_node2(target_panel.dock_id, ratio, dir);

            target_panel.dock_id = l;
            self.docktree.nodes[l].panel_id = target_panel.id;

            self.panels[about_to_dock].dock_id = r;
            self.docktree.nodes[r].panel_id = about_to_dock;

            self.remove_from_draworder(RootId::Panel(about_to_dock));
        }
    }

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
                let target_panel = &mut self.panels[dock_target];
                let (_, dir, ratio) = Self::get_dock_target(
                    self.mouse.pos,
                    target_panel.panel_rect(),
                    target_panel.flags,
                );

                self.dock_to_panel(id, dock_target, ratio, dir);

                //                 // && !dock_target.is_null()
                //                 // if !self.panels[id].dock_id.is_null() {
                //                 //     log::warn!("docking with panel that is already docked");
                //                 // }
                //                 let curr_size = self.panels[id].size;
                //                 self.panels[id].size_pre_dock = curr_size;
                //                 let curr_dock_id = self.panels[id].dock_id;

                //                 let target_panel = &mut self.panels[dock_target];

                //                 if target_panel.dock_id.is_null() {
                //                     assert!(
                //                         target_panel.parent_id.is_null(),
                //                         "currently children panels should not be dockable"
                //                     );
                //                     // init target panel as dock node
                //                     target_panel.size_pre_dock = target_panel.size;
                //                     let dock_id = self
                //                         .dock_tree
                //                         .add_root(target_panel.full_rect, target_panel.id);
                //                     target_panel.dock_id = dock_id;

                //                     let target_panel_id = target_panel.id;
                //                     self.replace_in_draworder(
                //                         RootId::Panel(target_panel_id),
                //                         RootId::Dock(dock_id),
                //                     );
                //                 }

                //                 let target_panel = &mut self.panels[dock_target];

                //                 if !curr_dock_id.is_null() {
                //                     let root_dock_id = self.dock_tree.get_root(curr_dock_id);
                //                     let id =
                //                         self.dock_tree
                //                             .merge_nodes(target_panel.dock_id, root_dock_id, ratio, dir);
                //                     target_panel.dock_id = id;

                //                     self.remove_from_draworder(RootId::Dock(root_dock_id));
                //                 } else {
                //                     let (l, r) = self.dock_tree.split_node2(target_panel.dock_id, ratio, dir);

                //                     target_panel.dock_id = l;
                //                     self.dock_tree.nodes[l].panel_id = target_panel.id;

                //                     self.panels[id].dock_id = r;
                //                     self.dock_tree.nodes[r].panel_id = id;

                //                     self.remove_from_draworder(RootId::Panel(id));
                //                 }

                if !self.panels[id].flags.has(PanelFlag::NO_FOCUS) {
                    self.bring_panel_to_front(id);
                }
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

            let (preview, dir, ratio) = Self::get_dock_target(
                self.mouse.pos,
                dock_target_panel.panel_rect(),
                dock_target_panel.flags,
            );

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
            if self.active_id == p.id
                // && !p.move_id.is_null()
                && !p.flags.has(PanelFlag::NO_MOVE)
            // || self.active_id == p.id && p.nav_root == p.move_id
            {
                if self.mouse.dragging(MouseBtn::Left) && self.panel_action.is_none() {
                    let start_pos = if p.dock_id.is_null() {
                        p.pos
                    } else {
                        let dock_root = self.docktree.get_root(p.dock_id);
                        self.docktree.nodes[dock_root].rect.min
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
                    let dock_root = self.docktree.get_root(p.dock_id);
                    let dock_root_pos = self.docktree.nodes[dock_root].rect.min;
                    let dock_pos = self.docktree.nodes[p.dock_id].rect.min;
                    *start_pos += dock_pos - dock_root_pos;

                    let dock_root_id = self.docktree.get_root(p.dock_id);
                    self.docktree
                        .undock_node(p.dock_id, &mut self.panels, &mut self.draworder);

                    // dock root may be removed if e.g. we only had a single split
                    // if let Some(n) = self.dock_tree.nodes.get(dock_root) {
                    //     self.dock_tree.recompute_rects(n.id, n.rect);
                    // }

                    let p = &mut self.panels[drag_id];
                    self.bring_panel_to_front(drag_id);
                }
            } else {
                let dock_root = self.docktree.get_root(p.dock_id);
                let n = &mut self.docktree.nodes[dock_root];
                let size = n.rect.size();
                let rect = Rect::from_min_size(new_pos, size);
                self.docktree.recompute_rects(dock_root, rect);
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
        if self.frame_count - p.frame_created <= 1 {
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

        // if sig.hovering() && self.active_id == id {
        if sig.hovering() {
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


            if self.mouse.clicked(Btn::Left) {
                sig |= Signal::CLICKED_LEFT;
            }
            if self.mouse.clicked(Btn::Right) {
                sig |= Signal::CLICKED_RIGHT;
            }
            if self.mouse.clicked(Btn::Middle) {
                sig |= Signal::CLICKED_MIDDLE;
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
        self.place_item(Vec2::new(0.0, self.style.line_height()));
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
    pub fn place_item(&mut self, size: Vec2) -> Rect {
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
        // drop(c);

        // if !id.is_null() {
        //     self.prev_item_data.reset();
        //     self.prev_item_data.id = id;
        //     self.prev_item_data.rect = rect;

        //     let Some(crect) = rect.clip(clip_rect) else {
        //         self.prev_item_data.is_hidden = true;
        //         return rect;
        //     };

        //     if self.draw_item_outline {
        //         // self.draw_over(|list| {
        //         self.draw_over(
        //             rect.draw_rect()
        //                 .outline(Outline::outer(RGBA::PASTEL_YELLOW, 1.5)),
        //         );
        //         // list.add_rect_outline(
        //         //     rect.min,
        //         //     rect.max,
        //         //     Outline::outer(RGBA::PASTEL_YELLOW, 1.5),
        //         // );
        //         if let Some(crect) = rect.clip(clip_rect) {
        //             self.draw_over(crect.draw_rect().outline(Outline::outer(RGBA::YELLOW, 1.5)));
        //             // list.add_rect_outline(
        //             //     crect.min,
        //             //     crect.max,
        //             //     Outline::outer(RGBA::YELLOW, 1.5),
        //             // );
        //         }
        //         // });
        //     }

        //     self.prev_item_data.clipped_rect = crect;
        //     self.prev_item_data.is_clipped = !clip_rect.contains_rect(rect);
        // }

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
                if flags.has(ItemFlags::SET_ACTIVE_ON_RELEASE) && self.mouse.released(MouseBtn::Left)
                    || flags.has(ItemFlags::SET_ACTIVE_ON_PRESS) && self.mouse.pressed(MouseBtn::Left)
                    || flags.has(ItemFlags::SET_ACTIVE_ON_CLICK) && self.mouse.clicked(MouseBtn::Left)
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

    // pub fn register_rect(&mut self, id: Id, rect: Rect) -> Signal {
    //     let p = &self.panels[self.current_panel_id];
    //     let clip_rect = p.current_clip_rect();
    //     if let Some(clip) = clip_rect.clip(rect) {
    //         self.update_hot_id(id, clip, ItemFlags::NONE);
    //     }
    //     self.get_item_signal(id, rect)
    // }

    pub fn reg_item_active_on_press(&mut self, id: Id, bb: Rect) -> Signal {
        self.reg_item_ex(id, bb, ItemFlags::SET_ACTIVE_ON_PRESS)
    }

    pub fn reg_item_active_on_release(&mut self, id: Id, bb: Rect) -> Signal {
        self.reg_item_ex(id, bb, ItemFlags::SET_ACTIVE_ON_RELEASE)
    }

    pub fn reg_item_active_on_click(&mut self, id: Id, bb: Rect) -> Signal {
        self.reg_item_ex(id, bb, ItemFlags::SET_ACTIVE_ON_CLICK)
    }

    pub fn reg_item_(&mut self, id: Id, bb: Rect) -> Signal {
        self.reg_item_ex(id, bb, ItemFlags::NONE)
    }

    /// "registers" the item, i.e. potentially sets hot_id and returns the item signals
    ///
    pub fn reg_item_ex(&mut self, id: Id, bb: Rect, flags: ItemFlags) -> Signal {
        let p = self.get_current_panel();
        let clip_rect = p.current_clip_rect();

        let is_hidden = bb.clip(clip_rect).is_none();

        if self.draw_item_outline {
            // self.draw_over(|list| {
            self.draw_over(
                bb.draw_rect()
                .outline(Outline::outer(RGBA::PASTEL_YELLOW, 1.5)),
            );

            if let Some(c_bb) = bb.clip(clip_rect) {
                self.draw_over(c_bb.draw_rect().outline(Outline::outer(RGBA::YELLOW, 1.5)));
            }
        }

        if id.is_null() {
            return Signal::NONE;
        }


        if self.kb_focus_next_item && self.prev_item_id == self.active_id {
            self.kb_focus_item_id = id;
            self.kb_focus_next_item = false;
            self.active_id_changed = true;
        }

        if self.kb_focus_prev_item && self.active_id == id {
            // self.active_id = self.prev_item_id;
            self.kb_focus_item_id = self.prev_item_id;
            self.kb_focus_prev_item = false;
            self.active_id_changed = true;
        }

        let mut signal = Signal::NONE;
        if self.kb_focus_item_id == id && self.active_id != id {
            signal |= Signal::GAINED_KEYBOARD_FOCUS;
            self.kb_focus_item_id = Id::NULL;
        }

        // assert!(self.prev_item_data.id == id);
        // let p = self.get_current_panel();
        if is_hidden && self.active_id != id {
            return Signal::NONE;
        }

        let c_bb = bb.clip(clip_rect).unwrap();
        self.update_hot_id(id, c_bb, flags);


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

        signal |= self.get_item_signal(id, c_bb);
        self.prev_item_id = id;

        signal
    }

    pub fn create_panel(&mut self, name: impl Into<String>, id: Id) {
        let name: String = name.into();
        let mut p = Panel::new(&name);
        p.frame_created = self.frame_count;

        if self.next.initial_width.is_finite() {
            p.size.x = self.next.initial_width;
        }
        if self.next.initial_height.is_finite() {
            p.size.y = self.next.initial_height;
        }

        self.panels.insert(id, p);
    }

    pub fn get_panel_with_name(&self, name: &str) -> Option<&Panel> {
        let id = self.gen_glob_id(name);
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

    // pub fn bring_panel_to_front(&mut self, panel_id: Id) {
    //     assert_eq!(self.panels.len(), self.draw_order.len());

    //     // gather the panel and all of its descendants (children, grandchildren, ...)
    //     let mut stack = vec![panel_id];
    //     let mut group_set = HashSet::new();
    //     while let Some(id) = stack.pop() {
    //         if !group_set.insert(id) {
    //             continue;
    //         }
    //         // push children for DFS
    //         for &c in &self.panels[id].children {
    //             stack.push(c);
    //         }
    //     }

    //     if group_set.is_empty() {
    //         return;
    //     }

    //     let dock_id = self.panels[panel_id].dock_id;
    //     if !dock_id.is_null() {
    //         let dock_tree: HashSet<_> = self.dock_tree.get_tree(dock_id).into_iter().collect();

    //         for (_, p) in &self.panels {
    //             if !p.dock_id.is_null() && dock_tree.contains(&p.dock_id) {
    //                 group_set.insert(p.id);
    //                 group_set.extend(&p.children);
    //             }
    //         }
    //     }

    //     // preserve relative ordering as they appear in draw_order
    //     let group_in_draw_order: Vec<Id> = self
    //         .draw_order
    //         .iter()
    //         .cloned()
    //         .filter(|id| group_set.contains(id))
    //         .collect();

    //     if group_in_draw_order.is_empty() {
    //         return;
    //     }

    //     // if the group is already at the very top in the same order, nothing to do
    //     let group_len = group_in_draw_order.len();
    //     if group_len > 0 {
    //         let tail_slice = &self.draw_order[self.draw_order.len() - group_len..];
    //         if tail_slice == group_in_draw_order.as_slice() {
    //             return;
    //         }
    //     }

    //     // build new draw order: all panels except group (preserving their order), then append group in their original relative order
    //     let mut new_draw_order: Vec<Id> = self
    //         .draw_order
    //         .iter()
    //         .cloned()
    //         .filter(|id| !group_set.contains(id))
    //         .collect();

    //     new_draw_order.extend(group_in_draw_order.iter().cloned());

    //     // write back and update per-panel draw_order indices
    //     self.draw_order = new_draw_order;
    //     for (i, &id) in self.draw_order.iter().enumerate() {
    //         self.panels[id].draw_order = i;
    //         assert_eq!(self.panels[id].draw_order, i);
    //     }
    // }

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
        let panel_flags = PanelFlag::NO_TITLEBAR
            | PanelFlag::NO_DOCKING
            | PanelFlag::USE_PARENT_DRAWLIST
            | PanelFlag::DRAW_V_SCROLLBAR
            | PanelFlag::USE_PARENT_CLIP
            | PanelFlag::IS_CHILD;

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
        self.place_item(size);
    }

    pub fn init(&mut self) {
        self.begin_frame();
        self.end_frame();
    }

    pub fn begin_frame(&mut self) {
        self.draw.clear();
        self.draw.screen_size = self.window.window_size();
        self.hot_panel_id = Id::NULL;
        self.hot_id = Id::NULL;

        if !self.mouse.pressed(MouseBtn::Left) {
            self.expect_drag = false;
        }
        // reset hovered tabbar each frame
        self.hot_tabbar_id = Id::NULL;

        if self.active_id == Id::NULL {
            self.kb_focus_next_item = false;
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
        let mut flags = PanelFlag::NO_FOCUS | PanelFlag::NO_MOVE | PanelFlag::NO_DOCKING;

        if self.window.is_decorated() {
            flags |= PanelFlag::NO_TITLEBAR;
        } else {
            // self.window_panel_titlebar_height = self.style.titlebar_height();
        }

        self.window_panel_id = self.gen_glob_id("##_WINDOW_PANEL");
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

        self.begin_dockspace();
        self.end();

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

    pub fn debug_panel(&mut self) {
        use crate::ui_items::ui_text;

        self.next.initial_width = 450.0;
        self.begin_ex(
            "Debug##_DEBUG_PANEL",
            PanelFlag::DRAW_H_SCROLLBAR | PanelFlag::DRAW_V_SCROLLBAR,
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

        if self.button("print dock tree") {
            println!("{}", self.docktree);
        }

        // let draw_order: Vec<_> = self
        //     .draw_order
        //     .iter()
        //     .map(|id| self.panels[*id].name.clone().replace("#", ""))
        //     .collect();
        // ui_text!(self: "draw_order: {draw_order:?}");

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
            self.input_slider_f32("titlebar height", 0.0, 100.0, &mut v);
            self.style.set_var(StyleVar::TitlebarHeight(v));

            let mut v = self.style.window_titlebar_height();
            self.input_slider_f32("window titlebar height", 0.0, 100.0, &mut v);
            self.style.set_var(StyleVar::WindowTitlebarHeight(v));

            let mut v = self.style.spacing_h();
            self.input_slider_f32("spacing h", 0.0, 30.0, &mut v);
            self.style.set_var(StyleVar::SpacingH(v));

            let mut v = self.style.spacing_v();
            self.input_slider_f32("spacing v", 0.0, 30.0, &mut v);
            self.style.set_var(StyleVar::SpacingV(v));

            let mut v = self.style.line_height();
            self.input_slider_f32("line height", 0.0, 30.0, &mut v);
            self.style.set_var(StyleVar::LineHeight(v));

            let mut v = self.style.panel_padding();
            self.input_slider_f32("panel padding", 0.0, 30.0, &mut v);
            v = v.round();
            self.style.set_var(StyleVar::PanelPadding(v));

            let mut out1 = self.style.panel_outline();
            let mut out2 = self.style.panel_hover_outline();
            self.input_slider_f32("panel outline width", 0.0, 30.0, &mut out1.width);
            out2.width = out1.width;
            self.style.set_var(StyleVar::PanelOutline(out1));
            self.style.set_var(StyleVar::PanelHoverOutline(out2));

            let mut v = self.style.scrollbar_width();
            self.input_slider_f32("scrollbar width", 0.0, 30.0, &mut v);
            v = v.round();
            self.style.set_var(StyleVar::ScrollbarWidth(v));

            let mut v = self.style.scrollbar_padding();
            self.input_slider_f32("scrollbar padding", 0.0, 30.0, &mut v);
            v = v.round();
            self.style.set_var(StyleVar::ScrollbarPadding(v));

            // TODO[NOTE]: not enough space in the font atlas
            // let mut v = self.style.text_size();
            // self.slider_f32("text height", 0.0, 30.0, &mut v);
            // self.style.set_var(StyleVar::TextSize(v));

            let mut v = self.style.btn_roundness();
            self.input_slider_f32("button corners", 0.0, 0.5, &mut v);
            self.style.set_var(StyleVar::BtnRoundness(v));

            let mut v = self.style.panel_corner_radius();
            self.input_slider_f32("panel corners", 0.0, 100.0, &mut v);
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
                self.image_id(fitted_size - Vec2::new(20.0, 0.0), uv_min, uv_max, TextureId::GLYPH);
            }
        }

        if self.tabitem("Debug") {
            if self.button("reset docktree") {
                for (_, p) in &mut self.panels {
                    p.dock_id = Id::NULL;
                }
                self.docktree = DockTree::new();
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

            self.begin_tabbar("tabbar 2");
            self.tabitem("tab1");
            self.tabitem("tab2");
            self.tabitem("tab3");
            self.end_tabbar();
        }


        self.unindent(10.0);
        self.end_tabbar();

        self.end();
    }

    pub fn end_frame(&mut self) {
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

                if !panel.flags.has(PanelFlag::ONLY_MOVE_FROM_TITLEBAR) {
                    self.active_id = self.panels[self.active_id].id;
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
        self.prev_hot_tabbar_id = self.hot_tabbar_id;

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

        // if self.ext_window.is_none() && !self.requested_windows.is_empty() {
        //     let (size, pos) = self.requested_windows.last().unwrap();
        //     let winit_window = event_loop
        //         .create_window(winit::window::WindowAttributes::default())
        //         .unwrap();
        //     let mut window =
        //         Window::new(winit_window, size.x as u32, size.y as u32, &self.wgpu);
        //     window.set_window_size(size.x as u32, size.y as u32);
        //     window.set_window_pos(*pos);
        //     self.ext_window = Some(window);
        // }

        self.prune_nodes();

        self.frame_count += 1;
        self.mouse.end_frame();
    }

    pub fn prune_nodes(&mut self) {
        // remove roots root ids in draworder. if panel is the root of a docktree also remove from
        // docktree.
        // todo!();

        let remove_panel = |p: &Panel| -> bool { self.frame_count - p.last_frame_used > 1 };

        let ids: Vec<_> = self.panels.iter().map(|(id, panel)| *id).collect();

        ids.into_iter().for_each(|i| {
            let reset = |id: &mut Id| {
                if *id == i {
                    *id = Id::NULL;
                }
            };

            if remove_panel(&self.panels[i]) {
                if !self.panels[i].dock_id.is_null() {
                    self.docktree.undock_node(
                        self.panels[i].dock_id,
                        &mut self.panels,
                        &mut self.draworder,
                    )
                }
                reset(&mut self.hot_id);
                reset(&mut self.active_id);
                reset(&mut self.hot_panel_id);
                reset(&mut self.active_panel_id);
            }
        });

        self.panels.retain(|id, panel| !remove_panel(panel));

        self.draworder.retain(|id| match *id {
            RootId::Panel(id) => self.panels.contains_id(id),
            RootId::Dock(id) => self.docktree.nodes.contains_id(id),
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
            let shaped_text = itm.layout(&mut font_table, &mut glyph_cache, &self.wgpu);
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
                    .texture(TextureId::GLYPH)
                    .uv(uv_min, uv_max),
            );
        }
    }

    pub fn upload_draw_data(&mut self) {
        let draw_buff = &mut self.draw.call_list;
        if draw_buff.vtx_alloc.len() * std::mem::size_of::<ui::Vertex>()
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
            self.wgpu.queue.write_buffer(
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
            self.wgpu.queue.write_buffer(
                &self.draw.gpu_indices,
                0,
                bytemuck::cast_slice(&self.draw.call_list.idx_alloc),
            );
        }
    }

    // pub fn build_draw_list(draw_buff: &mut DrawCallList, draw_list: &DrawList, screen_size: Vec2) {
    //     // let draw_list = self.panels[id].draw_list.borrow();
    //     // println!("draw_list:\n{:#?}", draw_list);

    //     // println!("{:#?}", draw_list);

    //     for cmd in draw_list.commands().iter() {
    //         let vtx = &draw_list.vtx_slice(cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count);
    //         let idx = &draw_list.idx_slice(cmd.idx_offset..cmd.idx_offset + cmd.idx_count);

    //         let mut curr_clip = draw_buff.current_clip_rect();
    //         curr_clip.min = curr_clip.min.max(Vec2::ZERO);
    //         curr_clip.max = curr_clip.max.min(screen_size);

    //         let mut clip = cmd.clip_rect;
    //         clip.min = clip.min.max(Vec2::ZERO);
    //         clip.max = clip.max.min(screen_size);

    //         // draw_buff.set_clip_rect(cmd.clip_rect);
    //         if cmd.clip_rect_used {
    //             draw_buff.set_clip_rect(cmd.clip_rect);
    //         } else if !draw_buff.current_clip_rect().contains_rect(clip) {
    //             draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, screen_size));
    //         }
    //         draw_buff.push(vtx, idx);
    //     }
    // }

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

                let (mut vtx, idx) = ui::tessellate_line(&path, cols[0], 1.5, true);
                vtx.iter_mut().enumerate().for_each(|(i, v)| {
                    v.col = cols[i % cols.len()];
                });

                // draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));
                draw_buff.push(&vtx, &idx);
            }
        }
    }

    pub fn build_draw_data(&mut self) {
        let order = self.get_panels_in_order();
        // let panels = &self.panels;
        // let draw_buff = &mut self.draw.call_list;
        self.draw.call_list.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));

        for id in order {
            let p = &self.panels[id];

            if p.flags.has(PanelFlag::USE_PARENT_DRAWLIST) {
                continue;
            }

            // Self::build_draw_list(draw_buff, &p.drawlist, self.draw.screen_size);

            self.draw.push_drawlist(&p.drawlist);
            self.draw.push_drawlist(&p.drawlist_over);
            // Self::build_draw_list(&mut self.draw.call_list, &p.drawlist_over, self.draw.screen_size);
        }
        self.upload_draw_data();

        // let panels = &self.panels;
        // let draw_buff = &mut self.draw.call_list;
        // draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));

        // for &id in &self.draw_order {
        //     let p = &self.panels[id];

        //     if p.flags.has(PanelFlags::USE_PARENT_DRAWLIST) {
        //         continue;
        //     }

        //     Self::build_draw_list(draw_buff, &p.drawlist, self.draw.screen_size);
        //     Self::build_draw_list(draw_buff, &p.drawlist_over, self.draw.screen_size);
        // }

        // self.upload_draw_data();
    }

    pub fn build_dbg_draw_data(&mut self) {
        let order = self.get_panels_in_order();

        let panels = &self.panels;
        let draw_buff = &mut self.draw.call_list;
        draw_buff.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.draw.screen_size));

        for id in order {
            let p = &self.panels[id];

            if p.flags.has(PanelFlag::USE_PARENT_DRAWLIST) {
                continue;
            }

            let draw_list = &p.drawlist;
            Self::build_debug_draw_list(draw_buff, &draw_list, self.draw.screen_size);
        }
        self.upload_draw_data();
    }
}
