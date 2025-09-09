use glam::{Mat4, UVec2, UVec4, Vec2, Vec4};
use macros::vertex;
use rustc_hash::FxHashMap;
use wgpu::util::DeviceExt;
use winit::window::Window;

use std::{
    collections::VecDeque,
    fmt,
    hash::{Hash, Hasher},
    ops,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    RGBA, RenderPassHandle, ShaderGenerics, ShaderHandle, VertexPosCol,
    gpu::{self, Vertex as VertexTyp, VertexDesc, WGPU},
    mouse::{CursorIcon, MouseBtn, MouseRec},
    rect::Rect,
};

macro_rules! sig_bits {
    ($n:literal) => { 1 << $n };
    ($i:ident) => { Signals::$i.bits() };
    ($($x:tt)|+) => {
        $(sig_bits!($x) | )* 0
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Signals: u32 {
        const NONE = 0;

        const PRESSED_LEFT = 1 << 0;
        const PRESSED_MIDDLE = 1 << 1;
        const PRESSED_RIGHT = 1 << 2;

        const DRAGGING_LEFT = 1 << 3;
        const DRAGGING_MIDDLE = 1 << 4;
        const DRAGGING_RIGHT = 1 << 5;

        const DOUBLE_DRAGGING_LEFT = 1 << 6;
        const DOUBLE_DRAGGING_MIDDLE = 1 << 7;
        const DOUBLE_DRAGGING_RIGHT = 1 << 8;

        const RELEASED_LEFT     = 1 << 9;
        const RELEASED_MIDDLE   = 1 << 10;
        const RELEASED_RIGHT    = 1 << 11;

        const CLICKED_LEFT      = 1 << 12;
        const CLICKED_MIDDLE    = 1 << 13;
        const CLICKED_RIGHT     = 1 << 14;

        const DOUBLE_CLICKED_LEFT   = 1 << 15;
        const DOUBLE_CLICKED_MIDDLE = 1 << 16;
        const DOUBLE_CLICKED_RIGHT  = 1 << 17;

        const HOVERING = 1 << 18 | sig_bits!(MOUSE_OVER);
        const MOUSE_OVER = 1 << 19; // may be occluded

        const PRESSED_KEYBOARD = 1 << 20;
    }
}

macro_rules! sig_fn {
    ($fn_name:ident => $($x:ident),*) => {
        impl Signals {
            pub const fn $fn_name(&self) -> bool {
                // let flag = Signals::from_bits($x).unwrap();
                $(self.contains(Signals::$x) || )* false
            }
        }
    }
}

sig_fn!(hovering => HOVERING);
sig_fn!(mouse_over => MOUSE_OVER);
sig_fn!(pressed => PRESSED_LEFT , PRESSED_KEYBOARD);
sig_fn!(clicked => CLICKED_LEFT , PRESSED_KEYBOARD);
sig_fn!(double_clicked => DOUBLE_CLICKED_LEFT);
sig_fn!(dragging => DRAGGING_LEFT);
sig_fn!(released => RELEASED_LEFT);

impl fmt::Display for Signals {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    X = 0,
    Y = 1,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PerAxis<T>(pub [T; 2]);

impl From<PerAxis<f32>> for Vec2 {
    fn from(value: PerAxis<f32>) -> Self {
        Vec2::new(value.0[0], value.0[1])
    }
}

impl<T> ops::Index<Axis> for PerAxis<T> {
    type Output = T;

    fn index(&self, index: Axis) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl<T> ops::IndexMut<Axis> for PerAxis<T> {
    fn index_mut(&mut self, index: Axis) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetId(u64);

impl WidgetId {
    pub const NULL: WidgetId = WidgetId(0);

    pub fn from_str(s: &str) -> Self {
        let mut hasher = rustc_hash::FxHasher::default();
        s.hash(&mut hasher);
        Self(hasher.finish().max(1))
    }

    pub fn is_null(&self) -> bool {
        *self == Self::NULL
    }
}

impl fmt::Display for WidgetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut n = self.0;
        if n == 0 {
            return write!(f, "ID(0)");
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
        write!(f, "ID({})", s)
    }
}

impl fmt::Debug for WidgetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SizeTyp {
    Px(f32),
    Fit,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WidgetOpt {
    pub fill: RGBA,
    pub outline_col: RGBA,
    pub outline_width: f32,
    pub corner_radius: f32,
    pub size: PerAxis<SizeTyp>,
    pub min_size: Vec2,
    pub pos: Option<Vec2>,
    pub flags: WidgetFlags,
    pub layout: Layout,
    pub padding: Padding,
    pub margin: Margin,
    pub spacing: f32,
}

impl WidgetOpt {
    pub fn new() -> Self {
        Self {
            fill: RGBA::ZERO,
            outline_col: RGBA::ZERO,
            outline_width: 0.0,
            corner_radius: 0.0,
            size: PerAxis([SizeTyp::Fit; 2]),
            min_size: Vec2::ZERO,
            pos: None,
            flags: WidgetFlags::NONE,
            layout: Default::default(),
            padding: Padding::ZERO,
            margin: Margin::ZERO,
            spacing: 0.0,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn margin(mut self, m: f32) -> Self {
        self.margin = Margin::all(m);
        self
    }

    pub fn padding(mut self, m: f32) -> Self {
        self.padding = Padding::all(m);
        self
    }

    pub fn fill(mut self, fill: RGBA) -> Self {
        self.fill = fill;
        self.flags |= WidgetFlags::DRAW_FILL;
        self
    }

    pub fn outline(mut self, col: RGBA, width: f32) -> Self {
        self.outline_col = col;
        self.outline_width = width;
        self.flags |= WidgetFlags::DRAW_OUTLINE;
        self
    }

    pub fn size_fix(mut self, x: f32, y: f32) -> Self {
        self.size = PerAxis([SizeTyp::Px(x), SizeTyp::Px(y)]);
        self
    }

    pub fn size_fit_x(mut self) -> Self {
        self.size[Axis::X] = SizeTyp::Fit;
        self
    }

    pub fn size_fit_y(mut self) -> Self {
        self.size[Axis::Y] = SizeTyp::Fit;
        self
    }

    pub fn size_fit(self) -> Self {
        self.size_fit_x().size_fit_y()
    }

    pub fn min_size_x(mut self, min_x: f32) -> Self {
        self.min_size.x = min_x;
        self
    }

    pub fn min_size_y(mut self, min_y: f32) -> Self {
        self.min_size.y = min_y;
        self
    }

    pub fn min_size(self, x: f32, y: f32) -> Self {
        self.min_size_x(x).min_size_y(y)
    }

    pub fn pos_fix(mut self, x: f32, y: f32) -> Self {
        self.pos = Some(Vec2::new(x, y));
        self
    }

    pub fn hoverable(mut self) -> Self {
        self.flags |= WidgetFlags::HOVERABLE;
        self
    }

    pub fn clickable(mut self) -> Self {
        self.flags |= WidgetFlags::CLICKABLE;
        self
    }

    pub fn draggable(mut self) -> Self {
        self.flags |= WidgetFlags::DRAGGABLE;
        self
    }

    pub fn resizable(mut self) -> Self {
        self.flags |= WidgetFlags::RESIZABLE;
        self
    }

    pub fn corner_radius(mut self, rad: f32) -> Self {
        self.corner_radius = rad;
        self
    }
}

macro_rules! widget_bits {
    ($n:literal) => { 1 << $n };
    ($i:ident) => { WidgetFlags::$i.bits() };
    ($($x:tt)|+) => {
        $(widget_bits!($x) | )* 0
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct WidgetFlags: u32 {
        const NONE          = 0;

        const DRAW_OUTLINE  = 1 << 0;
        const DRAW_FILL     = 1 << 1;

        const HOVERABLE     = 1 << 2;
        const CLICKABLE     = 1 << 3 | widget_bits!(HOVERABLE);
        const DRAGGABLE     = 1 << 4 | widget_bits!(CLICKABLE);
        const RESIZABLE     = 1 << 5;
    }
}

macro_rules! widget_flags_fn {
    ($fn_name:ident => $($x:tt)*) => {
        impl WidgetFlags {
            pub const fn $fn_name(&self) -> bool {
                let flag = WidgetFlags::from_bits(widget_bits!($($x)*)).unwrap();
                self.contains(flag)
            }
        }
    }
}

widget_flags_fn!(hoverable => HOVERABLE);
widget_flags_fn!(clickable => CLICKABLE);
widget_flags_fn!(draggable => DRAGGABLE);
widget_flags_fn!(resizable => RESIZABLE);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layout {
    #[default]
    Vertical,
    Horizontal,
}

impl Layout {
    pub fn axis(self) -> Axis {
        match self {
            Layout::Vertical => Axis::Y,
            Layout::Horizontal => Axis::X,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}
pub type Margin = Padding;

impl Padding {
    const ZERO: Padding = Padding::new(0.0, 0.0, 0.0, 0.0);

    pub const fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    pub const fn all(v: f32) -> Self {
        Self::new(v, v, v, v)
    }

    pub fn axis_sum(&self) -> Vec2 {
        (self.left + self.right, self.top + self.bottom).into()
    }

    pub fn sum_along_axis(&self, a: Axis) -> f32 {
        match a {
            Axis::X => self.left + self.right,
            Axis::Y => self.top + self.bottom,
        }
    }

    pub fn axis_padding(&self, a: Axis) -> [f32; 2] {
        match a {
            Axis::X => [self.left, self.right],
            Axis::Y => [self.top, self.bottom],
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Widget {
    pub id: WidgetId,
    pub parent: WidgetId,
    pub next_sibling: WidgetId,
    pub prev_sibling: WidgetId,

    pub first_child: WidgetId,
    pub last_child: WidgetId,
    pub n_children: u64,

    pub opt: WidgetOpt,

    pub rect: Rect,
    pub rel_pos: Vec2,
    pub comp_size: Vec2,
    pub comp_min_size: Vec2,
    // pub frac_units: Vec2,

    pub last_frame_used: u64,
}

impl Widget {
    pub fn new(id: WidgetId, opt: WidgetOpt) -> Self {
        Self {
            id,
            parent: WidgetId::NULL,
            first_child: WidgetId::NULL,
            last_child: WidgetId::NULL,
            n_children: 0,
            next_sibling: WidgetId::NULL,
            prev_sibling: WidgetId::NULL,
            rect: Rect::ZERO,
            rel_pos: Vec2::ZERO,
            comp_size: Vec2::ZERO,
            comp_min_size: Vec2::ZERO,
            // frac_units: Vec2::ZERO,
            // pre_drag_rect: None,
            last_frame_used: 0,
            opt,
        }
    }

    pub fn point_over(&self, point: Vec2, threashold: f32) -> bool {
        let off = Vec2::splat(self.opt.outline_width) / 2.0 + Vec2::splat(threashold);
        let min = self.rect.min - off;
        let max = self.rect.max + off;
        Rect::from_min_max(min, max).contains(point)
    }

    pub fn min_size(&self) -> Vec2 {
        let min = self.opt.outline_width.max(self.opt.corner_radius * 2.0);
        let min_w = self.opt.min_size.x.max(min).max(self.comp_min_size.x);
        let min_h = self.opt.min_size.y.max(min).max(self.comp_min_size.y);
        // let min_h = self.opt.min_size[Axis::Y].unwrap_or(min);
        Vec2::new(min_w, min_h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateStyle {
    pub default: RGBA,
    pub active: RGBA,
    pub hovered: RGBA,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStyle {
    pub fill: StateStyle,
    pub outline: StateStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ResizeDir {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl ResizeDir {
    fn as_cursor(self) -> CursorIcon {
        match self {
            ResizeDir::N => CursorIcon::ResizeN,
            ResizeDir::NE => CursorIcon::ResizeNE,
            ResizeDir::E => CursorIcon::ResizeE,
            ResizeDir::SE => CursorIcon::ResizeSE,
            ResizeDir::S => CursorIcon::ResizeS,
            ResizeDir::SW => CursorIcon::ResizeSW,
            ResizeDir::W => CursorIcon::ResizeW,
            ResizeDir::NW => CursorIcon::ResizeNW,
        }
    }

    fn has_n(&self) -> bool {
        matches!(self, Self::N | Self::NE | Self::NW)
    }
    fn has_e(&self) -> bool {
        matches!(self, Self::E | Self::NE | Self::SE)
    }
    fn has_s(&self) -> bool {
        matches!(self, Self::S | Self::SE | Self::SW)
    }
    fn has_w(&self) -> bool {
        matches!(self, Self::W | Self::NW | Self::SW)
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct Style {
//     frame_fill: RGBA,
//     frame_fill_active: RGBA,
//     frame_fill_hovered: RGBA,

//     frame_outline: RGBA,
//     frame_outline_active: RGBA,
//     frame_outline_hovered: RGBA,
//     frame_outline_width: f32,
// }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WidgetAction {
    Resize(ResizeDir),
    Move,
}

pub struct State {
    pub mouse: MouseRec,
    pub frame_count: u64,

    pub widgets: rustc_hash::FxHashMap<WidgetId, Widget>,
    /// determine widget parents
    pub widget_stack: Vec<WidgetId>,
    /// include parent hash in child hash
    ///
    /// at any time new hashes can be inserted to ensure uniqueness
    pub id_stack: Vec<WidgetId>,
    pub draw_order: Vec<WidgetId>,
    /// roots of trees that are selected for drawing
    pub roots: Vec<WidgetId>,

    /// hovered element
    pub hot_id: WidgetId,
    /// focused element
    pub active_id: WidgetId,

    /// position where next widget is drawn
    pub cursor: Vec2,
    /// generate triangle vertex & index buffers
    pub draw: DrawList,

    pub resize_threshold: f32,
    /// widget state before action, while its still being modified
    pub pre_action_widget_rect: Option<Rect>,
    pub curr_widget_action: Option<WidgetAction>,

    pub cursor_icon: CursorIcon,

    // pub style: Style,
    pub draw_dbg_wireframe: bool,
    pub window: Arc<Window>,
}

impl RenderPassHandle for State {
    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        if !self.roots.is_empty() {
            self.draw.draw(rpass, wgpu);
        }
    }
}

impl State {
    pub fn new(window: impl Into<Arc<Window>>) -> Self {
        Self {
            draw: DrawList::new(),
            cursor_icon: CursorIcon::Default,
            cursor: Vec2::ZERO,
            roots: Vec::new(),
            pre_action_widget_rect: None,
            curr_widget_action: None,
            hot_id: WidgetId::NULL,
            active_id: WidgetId::NULL,
            mouse: MouseRec::new(),
            frame_count: 0,
            widgets: rustc_hash::FxHashMap::default(),
            id_stack: Vec::new(),
            widget_stack: Vec::new(),
            draw_order: Vec::new(),
            draw_dbg_wireframe: false,
            resize_threshold: 10.0,
            window: window.into(),
        }
    }

    pub fn set_mouse_press(&mut self, button: MouseBtn, press: bool) {
        self.mouse.set_button_press(button, press)
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.mouse.set_mouse_pos(x, y)
    }

    pub fn set_screen_size(&mut self, w: f32, h: f32) {
        self.draw.screen_size = (w, h).into();
    }

    pub fn begin_frame(&mut self) {
        self.draw.clear();
        self.id_stack.clear();
        self.roots.clear();
        self.widget_stack.clear();
        self.cursor = Vec2::ZERO;

        let size = self.window.inner_size();
        self.draw.screen_size = (size.width as f32, size.height as f32).into();

        let icon = match self.curr_widget_action {
            Some(WidgetAction::Resize(dir)) => dir.as_cursor(),
            _ => CursorIcon::Default,
        };

        self.set_cursor_icon(icon);
    }

    pub fn end_frame(&mut self) {
        if !self.id_stack.is_empty() {
            log::warn!("end_frame: id_stack is not empty at frame end");
        }

        if let Some(w) = self.widgets.get(&WidgetId::NULL) {
            log::warn!("widget should not have null as id:\n{:?}", w);
        }

        self.prune_unused_nodes();

        self.update_hot_widget();
        self.update_active_widget();

        self.mouse.clear_released();

        // let keep_widget = |w: &Widget| w.last_frame_used == self.frame_count;
        let active_root = self.get_root(self.active_id);

        self.draw_order.clear();


        for &r in &self.roots {
            if r != active_root {
                self.draw_order.extend(self.collect_descendants_ids(r));
            }
        }

        if !active_root.is_null() {
            self.draw_order.extend(self.collect_descendants_ids(active_root));
        }



        self.build_draw_data();
        self.frame_count += 1;
    }

    fn prune_unused_nodes(&mut self) {
        self.draw_order.retain(|id| {
            self.widgets
                .get(id)
                .map_or(false, |w| w.last_frame_used == self.frame_count)
        });

        self.widgets.retain(|_, w| w.last_frame_used == self.frame_count);
    }

    pub fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if self.cursor_icon != icon {
            self.cursor_icon = icon;
            self.window.set_cursor(icon)
        }
    }

    pub fn id_from_str(&self, str: &str) -> WidgetId {
        use std::hash::{Hash, Hasher};
        if let Some(p_id) = self.id_stack.last() {
            let mut hasher = rustc_hash::FxHasher::with_seed(p_id.0 as usize);
            str.hash(&mut hasher);
            WidgetId(hasher.finish())
        } else {
            WidgetId::from_str(str)
        }
    }

    pub fn get_root(&self, id: WidgetId) -> WidgetId {
        if id.is_null() {
            return id;
        }
        let mut w = self.widgets.get(&id).unwrap();
        let mut p_id = w.parent;

        while !p_id.is_null() {
            w = &self.widgets[&p_id];
            p_id = w.parent;
        }

        w.id
    }

    pub fn iter_children(&self, id: WidgetId) -> impl Iterator<Item = &Widget> {
        let mut c_id = self.widgets[&id].first_child;
        std::iter::from_fn(move || {
            if c_id.is_null() {
                None
            } else {
                let c = &self.widgets[&c_id];
                c_id = c.next_sibling;
                Some(c)
            }
        })
    }

    pub fn iter_children_ids(&self, id: WidgetId) -> impl Iterator<Item = WidgetId> {
        let mut c_id = self.widgets[&id].first_child;
        std::iter::from_fn(move || {
            if c_id.is_null() {
                None
            } else {
                let c = &self.widgets[&c_id];
                c_id = c.next_sibling;
                Some(c.id)
            }
        })
    }

    pub fn collect_children_ids(&self, id: WidgetId) -> Vec<WidgetId> {
        self.iter_children_ids(id).collect()
    }

    pub fn collect_descendants_ids(&self, id: WidgetId) -> Vec<WidgetId> {
        let mut collect = vec![];

        let mut ids = vec![id];
        /// BF traversal
        while let Some(id) = ids.pop() {
            collect.push(id);
            ids.extend(self.iter_children_ids(id));
        }

        collect
    }


    pub fn is_id_over(&self, id1: WidgetId, id2: WidgetId) -> bool {
        if id2.is_null() {
            return true;
        }
        for &id in self.draw_order.iter().rev() {
            if id == id1 {
                return true;
            }

            if id == id2 {
                return false;
            }
        }

        false
    }

    pub fn update_hot_widget(&mut self) {
        let id = self.hot_id;
        if id.is_null() {
            return;
        }

        let w = self.widgets.get(&id).unwrap();
        let w_rect = w.rect;

        if !w.point_over(self.mouse.pos, self.resize_threshold) {
            self.hot_id = WidgetId::NULL;
            return;
        }

        // if mouse is pressed hot turns active
        if w.opt.flags.clickable() && self.mouse.pressed(MouseBtn::Left) {
            self.active_id = id;
        }

        let mut can_resize = None;
        if w.opt.flags.resizable() && self.curr_widget_action.is_none() {
            let r = &w_rect;
            let m = self.mouse.pos;

            let thr = self.resize_threshold + w.opt.outline_width / 2.0;

            let in_corner_region =
                |corner: Vec2| -> bool { corner.distance_squared(m) <= thr.powi(2) };

            // let mut cursor_icon = CursorIcon::Default;

            if in_corner_region(r.right_top()) {
                // cursor_icon = CursorIcon::ResizeNE;
                can_resize = Some(ResizeDir::NE)
            } else if in_corner_region(r.right_bottom()) {
                can_resize = Some(ResizeDir::SE)
            } else if in_corner_region(r.left_bottom()) {
                can_resize = Some(ResizeDir::SW)
            } else if in_corner_region(r.left_top()) {
                can_resize = Some(ResizeDir::NW)
            } else {
                let top_y = r.left_top().y;
                let bottom_y = r.left_bottom().y;
                let left_x = r.left_top().x;
                let right_x = r.right_top().x;

                if (m.y - top_y).abs() <= thr && m.x >= left_x + thr && m.x <= right_x - thr {
                    can_resize = Some(ResizeDir::N)
                } else if (m.y - bottom_y).abs() <= thr
                    && m.x >= left_x + thr
                    && m.x <= right_x - thr
                {
                    can_resize = Some(ResizeDir::S)
                } else if (m.x - right_x).abs() <= thr
                    && m.y >= top_y + thr
                    && m.y <= bottom_y - thr
                {
                    can_resize = Some(ResizeDir::E)
                } else if (m.x - left_x).abs() <= thr && m.y >= top_y + thr && m.y <= bottom_y - thr
                {
                    can_resize = Some(ResizeDir::W)
                }
            }

            if let Some(dir) = can_resize {
                self.set_cursor_icon(dir.as_cursor());

                if self.mouse.pressed(MouseBtn::Left) {
                    self.curr_widget_action = Some(WidgetAction::Resize(dir));
                    self.pre_action_widget_rect = Some(w_rect);
                }
            }
        }
    }

    pub fn update_active_widget(&mut self) {
        let id = self.active_id;
        if id.is_null() {
            return;
        }

        let w = self.widgets.get(&id).unwrap();
        let w_rect = w.rect;

        let w = self.widgets.get(&id).unwrap();

        let press_outside =
            self.mouse.pressed(MouseBtn::Left) && !self.mouse.dragging(MouseBtn::Left);

        // if curr_widget_action is some we are acting on the active widget, so its ok if the
        // mouse is not over the widget
        if self.curr_widget_action.is_none()
            && press_outside
            && !w.point_over(self.mouse.pos, self.resize_threshold)
        {
            self.active_id = WidgetId::NULL;
            self.pre_action_widget_rect = None;
            self.curr_widget_action = None;
            return;
        }

        if self.mouse.dragging(MouseBtn::Left) && w.opt.flags.draggable() {
            let w = self.widgets.get_mut(&id).unwrap();
            let pre_action = *self.pre_action_widget_rect.get_or_insert(w.rect);

            let m_start = self.mouse.drag_start(MouseBtn::Left);
            let m_delta = self.mouse.pos - m_start;

            // if we are not already performing an action set action to resizing if we are inside
            // the resize region, else we just move the widget
            let action = *self.curr_widget_action.get_or_insert(WidgetAction::Move);

            match action {
                WidgetAction::Resize(dir) => {
                    let min_size = w.min_size();
                    let mut new = pre_action;

                    if dir.has_n() {
                        new.min.y += m_delta.y;
                        if new.height() < min_size.y {
                            new.min.y = new.max.y - min_size.y;
                        }
                    }
                    if dir.has_s() {
                        new.max.y += m_delta.y;
                        if new.height() < min_size.y {
                            new.max.y = new.min.y + min_size.y;
                        }
                    }
                    if dir.has_w() {
                        new.min.x += m_delta.x;
                        if new.width() < min_size.x {
                            new.min.x = new.max.x - min_size.x;
                        }
                    }
                    if dir.has_e() {
                        new.max.x += m_delta.x;
                        if new.width() < min_size.x {
                            new.max.x = new.min.x + min_size.x;
                        }
                    }

                    w.rect = new;
                }
                WidgetAction::Move => {
                    w.rect = pre_action.translate(m_delta);
                }
            }
        }

        if !self.mouse.pressed(MouseBtn::Left) {
            self.pre_action_widget_rect = None;
            self.curr_widget_action = None;
        }
    }

    pub fn handle_signal_of_id(&mut self, id: WidgetId) -> Signals {
        let Some(w) = self.widgets.get(&id) else {
            return Signals::NONE;
        };

        let mut signal = Signals::NONE;

        if w.opt.flags.hoverable() && w.point_over(self.mouse.pos, self.resize_threshold) {
            signal |= Signals::MOUSE_OVER;
        }

        if signal.mouse_over()
            && !self.mouse.dragging(MouseBtn::Left)
            && self.is_id_over(id, self.hot_id)
        {
            self.hot_id = id;
        }

        // if hot_id from previous frame is id set to hovering
        if signal.mouse_over() && self.hot_id == id {
            signal |= Signals::HOVERING;
        }

        if signal.hovering() {
            if self.mouse.pressed(MouseBtn::Left) {
                signal |= Signals::PRESSED_LEFT;
            }
            if self.mouse.pressed(MouseBtn::Right) {
                signal |= Signals::PRESSED_RIGHT;
            }
            if self.mouse.pressed(MouseBtn::Middle) {
                signal |= Signals::PRESSED_MIDDLE;
            }

            if self.mouse.double_clicked(MouseBtn::Left) {
                signal |= Signals::DOUBLE_CLICKED_LEFT;
            }
            if self.mouse.double_clicked(MouseBtn::Right) {
                signal |= Signals::DOUBLE_CLICKED_RIGHT;
            }
            if self.mouse.double_clicked(MouseBtn::Middle) {
                signal |= Signals::DOUBLE_CLICKED_MIDDLE;
            }

            if self.mouse.dragging(MouseBtn::Left) {
                signal |= Signals::DRAGGING_LEFT;
            }
            if self.mouse.dragging(MouseBtn::Right) {
                signal |= Signals::DRAGGING_RIGHT;
            }
            if self.mouse.dragging(MouseBtn::Middle) {
                signal |= Signals::DRAGGING_MIDDLE;
            }

            if self.mouse.poll_released(MouseBtn::Left) {
                signal |= Signals::RELEASED_LEFT
            }
            if self.mouse.poll_released(MouseBtn::Right) {
                signal |= Signals::RELEASED_RIGHT
            }
            if self.mouse.poll_released(MouseBtn::Middle) {
                signal |= Signals::RELEASED_MIDDLE
            }
        }

        signal
    }

    pub fn is_hovered(&mut self, id: WidgetId) -> bool {
        id == self.hot_id
    }

    pub fn is_selected(&mut self, id: WidgetId) -> bool {
        id == self.active_id
    }

    pub fn add_button(&mut self, label: &str) -> bool {
        let id = self.id_from_str(label);
        let size = Vec2::new(50.0 * label.len() as f32, 80.0);

        let style = FrameStyle {
            fill: StateStyle {
                default: RGBA {
                    r: 0.20,
                    g: 0.22,
                    b: 0.25,
                    a: 1.0,
                },
                hovered: RGBA {
                    r: 0.28,
                    g: 0.30,
                    b: 0.34,
                    a: 1.0,
                },
                active: RGBA {
                    r: 0.12,
                    g: 0.14,
                    b: 0.18,
                    a: 1.0,
                },
            },
            outline: StateStyle {
                default: RGBA {
                    r: 0.10,
                    g: 0.10,
                    b: 0.10,
                    a: 1.0,
                },
                hovered: RGBA {
                    r: 0.35,
                    g: 0.35,
                    b: 0.40,
                    a: 1.0,
                },
                active: RGBA {
                    r: 0.50,
                    g: 0.50,
                    b: 0.55,
                    a: 1.0,
                },
            },
        };

        let (fill, outline) = if self.is_selected(id) && self.mouse.pressed(MouseBtn::Left) {
            (style.fill.active, style.outline.active)
        } else if self.is_hovered(id) {
            (style.fill.hovered, style.outline.hovered)
        } else {
            (style.fill.default, style.outline.default)
        };
        let opt = WidgetOpt::new()
            .size_fix(size.x, size.y)
            .fill(fill)
            .clickable()
            .corner_radius(10.0)
            .outline(outline, 5.0);

        let signal = self.begin_widget(label, opt);

        self.end_widget();
        signal.released()
    }

    pub fn parent_id(&self) -> WidgetId {
        self.widget_stack.last().copied().unwrap_or(WidgetId::NULL)
        // self.widget_stack.last().copied()
    }

    pub fn parent_widget_mut(&mut self) -> Option<&mut Widget> {
        let pid = self.parent_id();
        self.widgets.get_mut(&pid)
    }

    pub fn parent_widget(&self) -> Option<&Widget> {
        let pid = self.parent_id();
        self.widgets.get(&pid)
    }

    pub fn begin_widget(&mut self, label: &str, opt: WidgetOpt) -> Signals {
        let id = self.add_widget(label, opt);
        self.handle_signal_of_id(id)
    }

    pub fn add_widget(&mut self, label: &str, opt: WidgetOpt) -> WidgetId {
        let id = self.id_from_str(label);
        let parent_id = self.parent_id();
        self.id_stack.push(id);

        if parent_id.is_null() {
            self.roots.push(id);
        }

        if let Some(pos) = opt.pos {
            self.cursor = pos;
        }

        self.cursor.x += opt.margin.left;
        self.cursor.y += opt.margin.top;

        // if widget is root we draw at same position as last frame
        if parent_id.is_null() {
            if let Some(w) = self.widgets.get(&id) {
                self.cursor = w.rect.left_top();
            }
        }

        let mut content_size: Vec2 = if let Some(w) = self.widgets.get(&id) {
            w.rect.size()
        } else {
            let mut s = Vec2::ZERO;

            if let SizeTyp::Px(x) = opt.size[Axis::X] {
                s.x = x;
            }
            if let SizeTyp::Px(y) = opt.size[Axis::Y] {
                s.y = y;
            }
            s
            // opt.size.into()
        };
        content_size.x = content_size.x.max(opt.min_size.x);
        content_size.y = content_size.y.max(opt.min_size.y);

        if let Some(w) = self.widgets.get_mut(&id) {
            w.rect = Rect::from_min_size(self.cursor, content_size);
            w.opt = opt;
            w.last_frame_used = self.frame_count;
        } else {
            let mut w = Widget::new(id, opt);
            w.rect = Rect::from_min_size(self.cursor, content_size);
            w.last_frame_used = self.frame_count;
            self.widgets.insert(id, w);
            self.draw_order.push(id);
        }

        // self.cursor = self.widgets.get(&id).unwrap().rect.left_top();
        // let w = self.widgets.get(&id).unwrap();
        self.cursor.x += opt.padding.left;
        self.cursor.y += opt.padding.top;

        let mut prev_sibling = WidgetId::NULL;
        if let Some(p) = self.parent_widget_mut() {
            let last = p.last_child;
            p.last_child = id;
            if p.n_children == 0 {
                p.first_child = id;
            }
            p.n_children += 1;
            prev_sibling = last;
        }

        let w = self.widgets.get_mut(&id).unwrap();
        w.n_children = 0;
        w.parent = parent_id;
        w.first_child = WidgetId::NULL;
        w.last_child = WidgetId::NULL;
        w.next_sibling = WidgetId::NULL;
        w.prev_sibling = prev_sibling;

        if !prev_sibling.is_null() {
            let sib = self.widgets.get_mut(&prev_sibling).unwrap();
            sib.next_sibling = id;
        }

        self.widget_stack.push(id);

        id
    }
    pub fn end_widget(&mut self) {
        self.id_stack.pop();
        let id = self.widget_stack.pop().unwrap();
        let w = self.widgets.get(&id).unwrap();

        // move cursor to outer rect bottom (left-bottom) then add margin.bottom and parent spacing
        self.cursor = w.rect.left_bottom();
        self.cursor.y += w.opt.margin.bottom;

        if let Some(w) = self.parent_widget() {
            self.cursor.y += w.opt.spacing;
        }

        let mut size = Vec2::ZERO;
        if let SizeTyp::Fit = w.opt.size[Axis::X] {
            size.x = self.measure_fit_size_along_axis(w, Axis::X);
        }
        if let SizeTyp::Fit = w.opt.size[Axis::Y] {
        size.y = self.measure_fit_size_along_axis(w, Axis::Y);
        }
        // match w.opt.size[Axis::X] {
        //     SizeTyp::Px(_) => (),
        //     SizeTyp::Fit => {
        //         let sizes = self.iter_children(id).map(|w| w.rect.size().x);

        //         size.x = if w.opt.layout.axis() == Axis::X {
        //             sizes.sum()
        //         } else {
        //             sizes.fold(0.0, f32::max)
        //         };
        //         size.x += w.opt.padding.sum_along_axis(Axis::X);
        //     },
        // }

        let w = self.widgets.get_mut(&id).unwrap();
        w.comp_min_size = size;
        if w.rect.width() < size.x {
            w.rect.set_width(size.x);
        }
        if w.rect.height() < size.y {
            w.rect.set_height(size.y);
        }
        // w.rect.set_width(size.x);
    }

    fn measure_fit_size_along_axis(&self, w: &Widget, axis: Axis) -> f32 {
        let children: Vec<_> = self.iter_children(w.id).collect();
        let sizes = children.iter().map(|w| w.rect.size()[axis as usize]);
        let margins: f32 = children.iter().map(|w| w.opt.margin.sum_along_axis(axis)).sum();

        let mut size = if w.opt.layout.axis() == axis {
            sizes.sum::<f32>() + (w.n_children.max(1) - 1) as f32 * w.opt.spacing
        } else {
            sizes.fold(0.0, f32::max)
        };
        size += w.opt.padding.sum_along_axis(axis);
        size += margins;
        size
    }

    fn build_draw_data(&mut self) {
        for id in &self.draw_order {
            let w = self.widgets.get(id).unwrap();
            self.draw.draw_widget(w.rect, w.opt);
        }

        if self.draw_dbg_wireframe {
            self.draw.debug_wireframe(2.0);
        }
    }
}

#[vertex]
pub struct Vertex {
    pub pos: Vec2,
    pub col: RGBA,
}

#[derive(Debug, Copy, Clone, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct GlobalUniform {
    pub proj: glam::Mat4,
}

impl GlobalUniform {
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

fn vec2_to_point(v: Vec2) -> lyon::geom::Point<f32> {
    lyon::geom::Point::new(v.x, v.y)
}

fn path_from_points(points: &[Vec2], closed: bool) -> lyon::path::Path {
    let mut builder = lyon::path::Path::builder();
    if points.is_empty() {
        return builder.build();
    }
    builder.begin(vec2_to_point(points[0]));
    for &p in &points[1..] {
        builder.line_to(vec2_to_point(p));
    }
    builder.end(closed);
    builder.build()
}

pub fn tessellate_line(
    points: &[Vec2],
    col: RGBA,
    thickness: f32,
    is_closed: bool,
) -> (Vec<Vertex>, Vec<u32>) {
    use lyon::tessellation::{
        BuffersBuilder, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
    };
    if points.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let path = path_from_points(points, is_closed);

    let mut buffers = VertexBuffers::<Vertex, u32>::new();
    let mut tess = StrokeTessellator::new();
    let options = StrokeOptions::default()
        .with_line_width(thickness)
        .with_line_join(lyon::path::LineJoin::Round);

    let mut builder = BuffersBuilder::new(&mut buffers, |v: StrokeVertex| Vertex {
        pos: Vec2::new(v.position().x, v.position().y),
        col,
    });

    if let Err(e) = tess.tessellate_path(path.as_slice(), &options, &mut builder) {
        log::error!("Stroke tessellation failed: {:?}", e);
        return (Vec::new(), Vec::new());
    }

    (buffers.vertices, buffers.indices)
}

pub fn tessellate_fill(points: &[Vec2], fill: RGBA) -> (Vec<Vertex>, Vec<u32>) {
    use lyon::tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers,
    };
    if points.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let path = path_from_points(points, true);

    let mut buffers = VertexBuffers::<Vertex, u32>::new();
    let mut tess = FillTessellator::new();
    let mut builder = BuffersBuilder::new(&mut buffers, |v: FillVertex| Vertex {
        pos: Vec2::new(v.position().x, v.position().y),
        col: fill,
    });

    if let Err(e) = tess.tessellate_path(&path, &FillOptions::default(), &mut builder) {
        log::error!("Fill tessellation failed: {:?}", e);
        return (Vec::new(), Vec::new());
    }

    (buffers.vertices, buffers.indices)
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawRect {
    pub rect: Rect,
    pub fill: Option<RGBA>,
    pub outline: Option<(RGBA, f32)>,
    pub corner_radius: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawOpt {
    pub fill: RGBA,
    pub border_col: RGBA,
    pub border_width: f32,
    pub corner_radius: f32,
}

impl DrawOpt {
    pub fn new() -> Self {
        Self {
            fill: RGBA::ZERO,
            border_col: RGBA::ZERO,
            border_width: 0.0,
            corner_radius: 0.0,
        }
    }

    pub fn fill(mut self, fill: RGBA) -> Self {
        self.fill = fill;
        return self;
    }

    pub fn border(mut self, col: RGBA, width: f32) -> Self {
        self.border_col = col;
        self.border_width = width;
        self
    }

    pub fn corner_radius(mut self, rad: f32) -> Self {
        self.corner_radius = rad;
        self
    }
}

impl Rect {
    pub fn draw(self) -> DrawRect {
        DrawRect::new(self)
    }
}

impl DrawRect {
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            fill: None,
            outline: None,
            corner_radius: 0.0,
        }
    }

    pub fn fill(mut self, fill: RGBA) -> Self {
        self.fill = Some(fill);
        self
    }

    pub fn outline(mut self, col: RGBA, width: f32) -> Self {
        self.outline = Some((col, width));
        self
    }

    pub fn corner_radius(mut self, rad: f32) -> Self {
        self.corner_radius = rad;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawList {
    pub vtx_buffer: Vec<Vertex>,
    pub idx_buffer: Vec<u32>,
    pub screen_size: Vec2,

    pub path: Vec<Vec2>,
    pub path_closed: bool,

    pub resolution: f32,
}

fn vtx(pos: impl Into<Vec2>, col: impl Into<RGBA>) -> Vertex {
    Vertex {
        pos: pos.into(),
        col: col.into(),
    }
}

impl DrawList {
    pub fn new() -> Self {
        Self {
            vtx_buffer: Vec::new(),
            idx_buffer: Vec::new(),
            screen_size: Vec2::ONE,
            path: Vec::new(),
            path_closed: false,
            resolution: 8.0,
        }
    }

    pub fn clear(&mut self) {
        self.vtx_buffer.clear();
        self.idx_buffer.clear();
        self.path_clear();
    }

    pub fn draw_widget(&mut self, rect: Rect, opt: WidgetOpt) {
        self.path_rect(rect.min, rect.max, opt.corner_radius);

        if opt.flags.contains(WidgetFlags::DRAW_FILL) {
            let (vtx, idx) = tessellate_fill(&self.path, opt.fill);
            let off = self.vtx_buffer.len() as u32;
            self.vtx_buffer.extend(vtx);
            self.idx_buffer.extend(idx.into_iter().map(|i| i + off));
        }

        if opt.flags.contains(WidgetFlags::DRAW_OUTLINE) {
            self.path_clear();
            self.path_rect(rect.min, rect.max, opt.corner_radius);
            let (vtx, idx) = tessellate_line(&self.path, opt.outline_col, opt.outline_width, true);
            let off = self.vtx_buffer.len() as u32;
            self.vtx_buffer.extend(vtx);
            self.idx_buffer.extend(idx.into_iter().map(|i| i + off));
        }

        self.path_clear();
    }

    pub fn add_rect(&mut self, dr: DrawRect) {
        self.path_rect(dr.rect.min, dr.rect.max, dr.corner_radius);

        if let Some(fill) = dr.fill {
            let (vtx, idx) = tessellate_fill(&self.path, fill);
            let off = self.vtx_buffer.len() as u32;
            self.vtx_buffer.extend(vtx);
            self.idx_buffer.extend(idx.into_iter().map(|i| i + off));
        }

        if let Some((col, width)) = dr.outline {
            self.path_clear();
            self.path_rect(dr.rect.min, dr.rect.max, dr.corner_radius);
            let (vtx, idx) = tessellate_line(&self.path, col, width, true);
            let off = self.vtx_buffer.len() as u32;
            self.vtx_buffer.extend(vtx);
            self.idx_buffer.extend(idx.into_iter().map(|i| i + off));
        }

        self.path_clear();
    }

    pub fn path_arc_around(
        &mut self,
        center: Vec2,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
    ) {
        if radius == 0.0 || sweep_angle == 0.0 {
            return;
        }

        // maximum angular step so chord length ≤ resolution
        let chord_step = 2.0 * (self.resolution / (2.0 * radius)).clamp(-1.0, 1.0).asin();

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

    pub fn path_rect_offset(&mut self, min: Vec2, max: Vec2, rad: f32, off: f32) {
        const PI: f32 = std::f32::consts::PI;
        let rounded = rad != 0.0;
        // let segs = 8;

        self.path_to(Vec2::new(min.x + rad + off, min.y + off));
        self.path_to(Vec2::new(max.x - rad - off, min.y + off));
        if rounded {
            self.path_arc_around(
                Vec2::new(max.x - rad - off, min.y + rad + off),
                rad,
                PI / 2.0,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_to(Vec2::new(max.x - off, min.y + rad + off));
        self.path_to(Vec2::new(max.x - off, max.y - rad - off));
        if rounded {
            self.path_arc_around(
                Vec2::new(max.x - rad - off, max.y - rad - off),
                rad,
                0.0,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_to(Vec2::new(max.x - rad - off, max.y - off));
        self.path_to(Vec2::new(min.x + rad + off, max.y - off));
        if rounded {
            self.path_arc_around(
                Vec2::new(min.x + rad + off, max.y - rad - off),
                rad,
                -PI / 2.0,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_to(Vec2::new(min.x + off, max.y - rad - off));
        self.path_to(Vec2::new(min.x + off, min.y + rad + off));
        if rounded {
            self.path_arc_around(
                Vec2::new(min.x + rad + off, min.y + rad + off),
                rad,
                PI,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_close();
    }

    pub fn path_rect(&mut self, min: Vec2, max: Vec2, rad: f32) {
        const PI: f32 = std::f32::consts::PI;
        let rounded = rad != 0.0;
        // let segs = 8;

        self.path_to(Vec2::new(min.x + rad, min.y));
        self.path_to(Vec2::new(max.x - rad, min.y));
        if rounded {
            self.path_arc_around(
                Vec2::new(max.x - rad, min.y + rad),
                rad,
                PI / 2.0,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_to(Vec2::new(max.x, min.y + rad));
        self.path_to(Vec2::new(max.x, max.y - rad));
        if rounded {
            self.path_arc_around(
                Vec2::new(max.x - rad, max.y - rad),
                rad,
                0.0,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_to(Vec2::new(max.x - rad, max.y));
        self.path_to(Vec2::new(min.x + rad, max.y));
        if rounded {
            self.path_arc_around(
                Vec2::new(min.x + rad, max.y - rad),
                rad,
                -PI / 2.0,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_to(Vec2::new(min.x, max.y - rad));
        self.path_to(Vec2::new(min.x, min.y + rad));
        if rounded {
            self.path_arc_around(
                Vec2::new(min.x + rad, min.y + rad),
                rad,
                PI,
                -PI / 2.0,
                // segs,
            );
        }

        self.path_close();
    }

    pub fn path_clear(&mut self) {
        self.path.clear();
        self.path_closed = false;
    }

    pub fn path_to(&mut self, p: Vec2) {
        self.path.push(p);
    }

    pub fn path_close(&mut self) {
        self.path_closed = true;
    }

    pub fn build_path_stroke_multi_color(&mut self, thickness: f32, cols: &[RGBA]) {
        if cols.is_empty() {
            return;
        }
        let (vtx, idx) = tessellate_line(&self.path, cols[0], thickness, self.path_closed);
        let offset = self.vtx_buffer.len() as u32;
        self.vtx_buffer
            .extend(vtx.into_iter().enumerate().map(|(i, mut v)| {
                v.col = cols[i % cols.len()];
                v
            }));
        self.idx_buffer.extend(idx.into_iter().map(|i| i + offset));
        self.path_clear();
    }

    pub fn build_path_stroke(&mut self, thickness: f32, col: RGBA) {
        let (vtx, idx) = tessellate_line(&self.path, col, thickness, self.path_closed);
        let offset = self.vtx_buffer.len() as u32;
        self.vtx_buffer.extend(vtx.into_iter().map(|mut v| {
            v.col = col;
            v
        }));
        self.idx_buffer.extend(idx.into_iter().map(|i| i + offset));
        self.path_clear();
    }

    pub fn debug_wireframe(&mut self, thickness: f32) {
        self.path_clear();

        let mut vtx_buffer = Vec::new();
        std::mem::swap(&mut vtx_buffer, &mut self.vtx_buffer);
        let mut idx_buffer = Vec::new();
        std::mem::swap(&mut idx_buffer, &mut self.idx_buffer);

        for idxs in idx_buffer.chunks_exact(3) {
            let v0 = vtx_buffer[idxs[0] as usize];
            let v1 = vtx_buffer[idxs[1] as usize];
            let v2 = vtx_buffer[idxs[2] as usize];
            let cols = [v0.col, v1.col, v2.col, v0.col];
            self.path
                .extend_from_slice(&[v0.pos, v1.pos, v2.pos, v0.pos]);
            self.build_path_stroke_multi_color(thickness, &cols);
        }
    }
}

impl RenderPassHandle for DrawList {
    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        let vtx = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ui_vtx_buffer"),
                contents: &bytemuck::cast_slice(&self.vtx_buffer),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let idx = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ui_idx_buffer"),
                contents: &bytemuck::cast_slice(&self.idx_buffer),
                usage: wgpu::BufferUsages::INDEX,
            });

        let uniform = GlobalUniform {
            proj: Mat4::orthographic_lh(
                0.0,
                self.screen_size.x,
                self.screen_size.y,
                0.0,
                -1.0,
                0.0,
            ),
        }
        .build_bind_group(wgpu);

        rpass.set_bind_group(0, &uniform, &[]);

        rpass.set_vertex_buffer(0, vtx.slice(..));
        rpass.set_index_buffer(idx.slice(..), wgpu::IndexFormat::Uint32);

        rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

        rpass.draw_indexed(0..self.idx_buffer.len() as u32, 0, 0..1);
    }
}

pub struct UiShader;

impl ShaderHandle for UiShader {
    const RENDER_PIPELINE_ID: crate::ShaderID = "ui_shader";

    fn build_pipeline(&self, desc: &ShaderGenerics<'_>, wgpu: &WGPU) -> wgpu::RenderPipeline {
        const SHADER_SRC: &str = r#"


            @rust struct Vertex {
                pos: vec2<f32>,
                col: vec4<f32>,
                ...
            }

            struct GlobalUniform {
                proj: mat4x4<f32>,
            }

            @group(0) @binding(0)
            var<uniform> global: GlobalUniform;

            struct VSOut {
                @builtin(position) pos: vec4<f32>,
                @location(0) color: vec4<f32>,
            };

            @vertex
                fn vs_main(
                    v: Vertex,
                ) -> VSOut {
                    var out: VSOut;
                    out.color = v.col;
                    out.pos = global.proj * vec4(v.pos, 0.0, 1.0);

                    return out;
                }


            @fragment
                fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
                    return in.color;
                }
            "#;

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

        let shader_src = gpu::process_shader_code(SHADER_SRC, &desc).unwrap();
        let vertices = desc.iter().map(|d| d.0).collect::<Vec<_>>();
        gpu::PipelineBuilder::new(&shader_src, wgpu.surface_format)
            .label("rect_pipeline")
            .vertex_buffers(&vertices)
            .bind_groups(&[&global_bind_group_layout])
            .sample_count(gpu::Renderer::multisample_count())
            .build(&wgpu.device)
    }
}
