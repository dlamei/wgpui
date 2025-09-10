use glam::{Mat4, UVec2, UVec4, Vec2, Vec4};
use macros::vertex;
use rustc_hash::FxHashMap;
use wgpu::util::DeviceExt;
use winit::window::Window;

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fmt,
    hash::{Hash, Hasher},
    ops,
    sync::Arc,
};

use crate::{
    RGBA, ctext,
    gpu::{self, ShaderHandle, Vertex as VertexTyp, VertexDesc, WGPU, WGPUHandle},
    mouse::{CursorIcon, MouseBtn, MouseRec},
    rect::Rect,
    utils::{Duration, Instant},
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

impl SizeTyp {
    pub fn is_fit(&self) -> bool {
        matches!(self, Self::Fit)
    }

    pub fn is_px(&self) -> bool {
        matches!(self, Self::Px(_))
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Size {
    pub min: PerAxis<SizeTyp>,
    pub max: PerAxis<SizeTyp>,
}

impl Size {
    pub const NONE: Self = Self {
        min: PerAxis([SizeTyp::Px(0.0); 2]),
        max: PerAxis([SizeTyp::Px(f32::INFINITY); 2]),
    };

    pub fn cnst(low: SizeTyp, high: SizeTyp) -> Self {
        Self {
            min: PerAxis([low, high]),
            max: PerAxis([low, high]),
        }
    }

    pub fn max(mut self, x: SizeTyp, y: SizeTyp) -> Self {
        self.max[Axis::X] = x;
        self.max[Axis::Y] = y;
        self
    }

    pub fn min(mut self, x: SizeTyp, y: SizeTyp) -> Self {
        self.min[Axis::X] = x;
        self.min[Axis::Y] = y;
        self
    }

    pub fn axis_range(&self, a: Axis) -> (SizeTyp, SizeTyp) {
        (self.min[a], self.max[a])
    }

    pub fn min_px_bound(&self) -> Vec2 {
        let min_x = match self.min[Axis::X] {
            SizeTyp::Px(x) => x,
            SizeTyp::Fit => 0.0,
        };
        let min_y = match self.min[Axis::Y] {
            SizeTyp::Px(y) => y,
            SizeTyp::Fit => 0.0,
        };

        Vec2::new(min_x, min_y)
    }

    pub fn max_px_bound(&self) -> Vec2 {
        let min_x = match self.max[Axis::X] {
            SizeTyp::Px(x) => x,
            SizeTyp::Fit => f32::INFINITY,
        };
        let min_y = match self.max[Axis::Y] {
            SizeTyp::Px(y) => y,
            SizeTyp::Fit => f32::INFINITY,
        };

        Vec2::new(min_x, min_y)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetOpt {
    pub fill: RGBA,
    pub outline_col: RGBA,
    pub outline_width: f32,
    pub corner_radius: f32,
    // pub size: PerAxis<SizeTyp>,
    // pub min_size: Vec2,
    pub size: Size,

    pub text: Option<String>,
    pub font_size: f32,
    pub line_height: f32,

    pub pos: Option<Vec2>,
    pub flags: WidgetFlags,
    pub layout: Layout,
    pub padding: Padding,
    pub margin: Margin,
    pub spacing: f32,
}

macro_rules! widget_opt_size_fn {
    ($kind:ident ($($x:ident: $ty:ty),*) $e:expr ) => {
        paste::paste! {

            // set min x value
            pub fn [<size_min_x_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.size.min[Axis::X] = $e;
                self
            }

            // set min y value
            pub fn [<size_min_y_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.size.min[Axis::Y] = $e;
                self
            }

            // set max x value
            pub fn [<size_max_x_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.size.max[Axis::X] = $e;
                self
            }

            // set max y value
            pub fn [<size_max_y_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.size.max[Axis::Y] = $e;
                self
            }


            // assign the same value for min and max for x
            pub fn [<size_x_ $kind>](self, $($x:$ty),*) -> Self {
                self
                    .[<size_min_x_ $kind>]($($x),*)
                    .[<size_max_x_ $kind>]($($x),*)
            }

            // assign the same value for min and max for y
            pub fn [<size_y_ $kind>](self, $($x:$ty),*) -> Self {
                self
                    .[<size_min_y_ $kind>]($($x),*)
                    .[<size_max_y_ $kind>]($($x),*)
            }

            // set min value for x and y
            pub fn [<size_min_ $kind>](self, $([< $x _min_x >]:$ty,)* $([< $x _min_y >]:$ty),*) -> Self {
                self
                    .[<size_min_x_ $kind>]($([< $x _min_x >]),*)
                    .[<size_min_y_ $kind>]($([< $x _min_y >]),*)
            }

            // set max value for x and y
            pub fn [<size_max_ $kind>](self, $([< $x _max_x >]:$ty,)* $([< $x _max_y >]:$ty),*) -> Self {
                self
                    .[<size_max_x_ $kind>]($([< $x _max_x >]),*)
                    .[<size_max_y_ $kind>]($([< $x _max_y >]),*)
            }

            // assign the same value for min and max for x and y
            pub fn [<size_ $kind>](self, $([< $x _x >]:$ty,)* $([< $x _y >]:$ty),*) -> Self {
                self
                    .[<size_x_ $kind>]($([< $x _x >]),*)
                    .[<size_y_ $kind>]($([< $x _y >]),*)
            }

        }
    }
}

impl WidgetOpt {
    pub fn new() -> Self {
        Self {
            fill: RGBA::ZERO,
            outline_col: RGBA::ZERO,
            outline_width: 0.0,
            corner_radius: 0.0,
            // size: PerAxis([SizeTyp::Fit; 2]),
            size: Size::NONE,
            // min_size: Vec2::ZERO,
            pos: None,
            text: None,
            font_size: 0.0,
            line_height: 0.0,
            flags: WidgetFlags::NONE,
            layout: Default::default(),
            padding: Padding::ZERO,
            margin: Margin::ZERO,
            spacing: 0.0,
        }
    }

    widget_opt_size_fn!(px(px: f32) SizeTyp::Px(px));
    widget_opt_size_fn!(fit() SizeTyp::Fit);

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

    pub fn text(mut self, s: impl Into<String>, font_size: f32, line_height: f32) -> Self {
        self.text = Some(s.into());
        self.font_size = font_size;
        self.line_height = line_height;
        self.flags |= WidgetFlags::DRAW_TEXT;
        self
    }

    pub fn outline(mut self, col: RGBA, width: f32) -> Self {
        self.outline_col = col;
        self.outline_width = width;
        self.flags |= WidgetFlags::DRAW_OUTLINE;
        self
    }

    pub fn layout_v(mut self) -> Self {
        self.layout = Layout::Vertical;
        self
    }

    pub fn layout_h(mut self) -> Self {
        self.layout = Layout::Horizontal;
        self
    }

    pub fn text_meta(&self) -> TextMeta {
        TextMeta::new(self.text.clone().unwrap_or("".into()), self.font_size, self.line_height)
    }

    // pub fn size_fix(mut self, x: f32, y: f32) -> Self {
    //     self.size = PerAxis([SizeTyp::Px(x), SizeTyp::Px(y)]);
    //     self
    // }

    // pub fn size_fit_x(mut self) -> Self {
    //     self.size[Axis::X] = SizeTyp::Fit;
    //     self
    // }

    // pub fn size_fit_y(mut self) -> Self {
    //     self.size[Axis::Y] = SizeTyp::Fit;
    //     self
    // }

    // pub fn size_fit(self) -> Self {
    //     self.size_fit_x().size_fit_y()
    // }

    // pub fn min_size_x(mut self, min_x: f32) -> Self {
    //     self.min_size.x = min_x;
    //     self
    // }

    // pub fn min_size_y(mut self, min_y: f32) -> Self {
    //     self.min_size.y = min_y;
    //     self
    // }

    // pub fn min_size(self, x: f32, y: f32) -> Self {
    //     self.min_size_x(x).min_size_y(y)
    // }

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

    pub fn resizable_dir(mut self, dirs: &[Dir]) -> Self {
        for dir in dirs {
            if dir.has_n() {
                self.flags |= WidgetFlags::RESIZABLE_N;
            }
            if dir.has_e() {
                self.flags |= WidgetFlags::RESIZABLE_E;
            }
            if dir.has_s() {
                self.flags |= WidgetFlags::RESIZABLE_S;
            }
            if dir.has_w() {
                self.flags |= WidgetFlags::RESIZABLE_W;
            }
        }
        self
    }

    pub fn resizable(self) -> Self {
        self.resizable_dir(&[Dir::N, Dir::E, Dir::S, Dir::W])
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
        const DRAW_TEXT     = 1 << 2;

        const HOVERABLE     = 1 << 3;
        const CLICKABLE     = 1 << 4 | widget_bits!(HOVERABLE);
        const DRAGGABLE     = 1 << 5 | widget_bits!(CLICKABLE);

        const RESIZABLE_N   = 1 << 6;
        const RESIZABLE_E   = 1 << 7;
        const RESIZABLE_S   = 1 << 8;
        const RESIZABLE_W   = 1 << 9;
        const RESIZABLE_NE   = widget_bits!(RESIZABLE_N | RESIZABLE_E);
        const RESIZABLE_SE   = widget_bits!(RESIZABLE_S | RESIZABLE_E);
        const RESIZABLE_NW   = widget_bits!(RESIZABLE_N | RESIZABLE_W);
        const RESIZABLE_SW   = widget_bits!(RESIZABLE_S | RESIZABLE_W);
        // const RESIZABLE_X   = widget_bits!(RESIZABLE_E | RESIZABLE_W);
        // const RESIZABLE_Y   = widget_bits!(RESIZABLE_N | RESIZABLE_S);
        const RESIZABLE_ALL     = widget_bits!(RESIZABLE_N | RESIZABLE_E | RESIZABLE_S | RESIZABLE_W);
    }
}

// macro_rules! widget_flags_fn {
//     ($fn_name:ident => $($x:tt)*) => {
//         impl WidgetFlags {
//             pub const fn $fn_name(&self) -> bool {
//                 let flag = WidgetFlags::from_bits(widget_bits!($($x)*)).unwrap();
//                 self.contains(flag)
//             }
//         }
//     }
// }

macro_rules! widget_flags_fn {
    ($fn_name:ident => $($x:ident),*) => {
        impl WidgetFlags {
            pub const fn $fn_name(&self) -> bool {
                // let flag = Signals::from_bits($x).unwrap();
                $(self.contains(WidgetFlags::$x) || )* false
            }
        }
    }
}

widget_flags_fn!(hoverable => HOVERABLE);
widget_flags_fn!(clickable => CLICKABLE);
widget_flags_fn!(draggable => DRAGGABLE);
widget_flags_fn!(resizable => RESIZABLE_N, RESIZABLE_S, RESIZABLE_E, RESIZABLE_W);
widget_flags_fn!(resizable_all => RESIZABLE_ALL);

widget_flags_fn!(resizable_n => RESIZABLE_N);
widget_flags_fn!(resizable_ne => RESIZABLE_NE);
widget_flags_fn!(resizable_e => RESIZABLE_E);
widget_flags_fn!(resizable_se => RESIZABLE_SE);
widget_flags_fn!(resizable_s => RESIZABLE_S);
widget_flags_fn!(resizable_sw => RESIZABLE_SW);
widget_flags_fn!(resizable_w => RESIZABLE_W);
widget_flags_fn!(resizable_nw => RESIZABLE_NW);

widget_flags_fn!(resizable_x => RESIZABLE_E, RESIZABLE_W);
widget_flags_fn!(resizable_y => RESIZABLE_N, RESIZABLE_S);

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

#[derive(Debug, Clone, PartialEq)]
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
    pub comp_max_size: Vec2,
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
            comp_max_size: Vec2::INFINITY,
            // frac_units: Vec2::ZERO,
            // pre_drag_rect: None,
            last_frame_used: 0,
            opt,
        }
    }

    pub fn rect_width_w_fit_size(&mut self, fit_size: f32) {
        let (lo, hi) = self.opt.size.axis_range(Axis::X);
        let width = self.rect.width();
        let lo = match lo {
            SizeTyp::Px(x) => x,
            SizeTyp::Fit => fit_size,
        }
        .max(self.corner_circle_size().x);
        let hi = match hi {
            SizeTyp::Px(x) => x,
            SizeTyp::Fit => fit_size,
        }
        .max(self.corner_circle_size().x);

        let lo = lo.min(hi);

        let new_width = width.clamp(lo, hi);
        self.rect.set_width(new_width)
    }

    pub fn rect_height_w_fit_size(&mut self, fit_size: f32) {
        let (lo, hi) = self.opt.size.axis_range(Axis::Y);
        let height = self.rect.height();
        let lo = match lo {
            SizeTyp::Px(y) => y,
            SizeTyp::Fit => fit_size,
        }
        .max(self.corner_circle_size().y);
        let hi = match hi {
            SizeTyp::Px(y) => y,
            SizeTyp::Fit => fit_size,
        }
        .max(self.corner_circle_size().y);

        let lo = lo.min(hi);

        let new_height = height.clamp(lo, hi);
        self.rect.set_height(height.clamp(lo, hi))
    }

    /// clear data that must be re-computed every frame
    pub fn reset_frame_data(&mut self) {
        self.n_children = 0;
        self.parent = WidgetId::NULL;
        self.next_sibling = WidgetId::NULL;
        self.prev_sibling = WidgetId::NULL;
        self.first_child = WidgetId::NULL;
        self.last_child = WidgetId::NULL;
        self.comp_min_size = Vec2::ZERO;
        self.comp_max_size = Vec2::INFINITY;
    }

    pub fn point_over(&self, point: Vec2, threashold: f32) -> bool {
        let off = Vec2::splat(self.opt.outline_width) / 2.0 + Vec2::splat(threashold);
        let min = self.rect.min - off;
        let max = self.rect.max + off;
        Rect::from_min_max(min, max).contains(point)
    }

    /// determine the minimum size of a widget
    ///
    /// either use corner radius, optional min_size or comp_min_size which is calculated every
    /// frame based on other properties like SizeTyp::Fit
    pub fn total_min_size(&self) -> Vec2 {
        let min = self.opt.outline_width.max(self.opt.corner_radius * 2.0);
        let min_w = self.comp_min_size.x.max(min);
        let min_h = self.comp_min_size.y.max(min);
        Vec2::new(min_w, min_h)
    }

    pub fn corner_circle_size(&self) -> Vec2 {
        let min = self.opt.outline_width.max(self.opt.corner_radius * 2.0);
        Vec2::new(min, min)
    }

    /// determine the maximum size of a widget
    ///
    /// either use corner radius, optional min_size or comp_min_size which is calculated every
    /// frame based on other properties like SizeTyp::Fit
    pub fn total_max_size(&self) -> Vec2 {
        // let min = self.opt.outline_width.max(self.opt.corner_radius * 2.0);
        // let max_w = self.comp_max_size.x.max(min);
        // let max_h = self.comp_max_size.y.max(min);
        // Vec2::new(max_w, max_h)
        // min
        self.comp_max_size
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
pub enum Dir {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Dir {
    pub fn as_cursor(self) -> CursorIcon {
        match self {
            Dir::N => CursorIcon::ResizeN,
            Dir::NE => CursorIcon::ResizeNE,
            Dir::E => CursorIcon::ResizeE,
            Dir::SE => CursorIcon::ResizeSE,
            Dir::S => CursorIcon::ResizeS,
            Dir::SW => CursorIcon::ResizeSW,
            Dir::W => CursorIcon::ResizeW,
            Dir::NW => CursorIcon::ResizeNW,
        }
    }

    pub fn has_n(&self) -> bool {
        matches!(self, Self::N | Self::NE | Self::NW)
    }
    pub fn has_e(&self) -> bool {
        matches!(self, Self::E | Self::NE | Self::SE)
    }
    pub fn has_s(&self) -> bool {
        matches!(self, Self::S | Self::SE | Self::SW)
    }
    pub fn has_w(&self) -> bool {
        matches!(self, Self::W | Self::NW | Self::SW)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WidgetAction {
    Resize {
        dir: Dir,
        id: WidgetId,
        prev_rect: Rect,
    },
    Move {
        prev_pos: Vec2,
        id: WidgetId,
    },
}

pub struct State {
    pub mouse: MouseRec,
    pub frame_count: u64,

    pub widgets: FxHashMap<WidgetId, Widget>,
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
    pub curr_widget_action: Option<WidgetAction>,

    pub cursor_icon: CursorIcon,
    pub cursor_icon_changed: bool,

    // pub style: Style,
    pub draw_dbg_wireframe: bool,
    pub window: Arc<Window>,
    // text
    // pub font: ctext::FontSystem,
    // pub text_swash_cache: ctext::SwashCache,
    // pub font_atlas: FontAtlas,
    // pub white_texture: gpu::Texture,
    // pub text_cache: TextCache,
    // pub next_text_cache: TextCache,
}

impl gpu::RenderPassHandle for State {
    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        if !self.roots.is_empty() {
            self.draw.draw(rpass, wgpu);
            // return;
        }

        // let vtx = wgpu
        //     .device
        //     .create_buffer_init(&wgpu::util::BufferInitDescriptor {
        //         label: Some("ui_vtx_buffer"),
        //         contents: &bytemuck::cast_slice(&self.draw.vtx_buffer),
        //         usage: wgpu::BufferUsages::VERTEX,
        //     });

        // let idx = wgpu
        //     .device
        //     .create_buffer_init(&wgpu::util::BufferInitDescriptor {
        //         label: Some("ui_idx_buffer"),
        //         contents: &bytemuck::cast_slice(&self.draw.idx_buffer),
        //         usage: wgpu::BufferUsages::INDEX,
        //     });

        // let global_uniform = GlobalUniform {
        //     proj: Mat4::orthographic_lh(
        //         0.0,
        //         self.draw.screen_size.x,
        //         self.draw.screen_size.y,
        //         0.0,
        //         -1.0,
        //         0.0,
        //     ),
        // };
        // // .build_bind_group(wgpu);

        // let bind_group = build_bind_group(global_uniform, self.white_texture.view(), wgpu);

        // rpass.set_bind_group(0, &bind_group, &[]);

        // rpass.set_vertex_buffer(0, vtx.slice(..));
        // rpass.set_index_buffer(idx.slice(..), wgpu::IndexFormat::Uint32);

        // rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

        // rpass.draw_indexed(0..self.draw.idx_buffer.len() as u32, 0, 0..1);
    }
}

impl State {
    pub fn new(wgpu: WGPUHandle, window: impl Into<Arc<Window>>) -> Self {
        Self {
            draw: DrawList::new(wgpu),
            cursor_icon: CursorIcon::Default,
            cursor_icon_changed: false,
            cursor: Vec2::ZERO,
            roots: Vec::new(),
            curr_widget_action: None,
            hot_id: WidgetId::NULL,
            active_id: WidgetId::NULL,
            mouse: MouseRec::new(),
            frame_count: 0,
            widgets: FxHashMap::default(),
            id_stack: Vec::new(),
            widget_stack: Vec::new(),
            draw_order: Vec::new(),
            draw_dbg_wireframe: false,
            resize_threshold: 10.0,
            window: window.into(),
            // font: ctext::FontSystem::new(),
            // text_swash_cache: ctext::SwashCache::new(),
            // font_atlas: FontAtlas::new(wgpu),
            // white_texture: gpu::Texture::create(wgpu, 1, 1, &RGBA::INDIGO.as_bytes()),
            // text_cache: TextCache::new(),
            // next_text_cache: TextCache::new(),
        }
    }

    pub fn set_mouse_press(&mut self, button: MouseBtn, press: bool) {
        self.mouse.set_button_press(button, press)
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.mouse.set_mouse_pos(x, y)
    }

    pub fn start_frame(&mut self) {
        self.draw.clear();
        self.id_stack.clear();
        self.roots.clear();
        self.widget_stack.clear();
        self.cursor = Vec2::ZERO;
        self.cursor_icon_changed = false;

        let size = self.window.inner_size();
        self.draw.screen_size = (size.width as f32, size.height as f32).into();

        if self.curr_widget_action.is_none() {
            self.set_cursor_icon(CursorIcon::Default)
        }

        self.draw.draw_uv_rect(Rect::from_min_max(Vec2::ZERO, Vec2::splat(800.0)), Vec2::ZERO, Vec2::splat(1.0));
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
        self.handle_widget_action();
        self.update_cursor_icon();

        self.mouse.clear_released();

        let active_root = self.get_root(self.active_id);

        self.draw_order.clear();

        for &r in &self.roots {
            if r != active_root {
                self.draw_order.extend(self.collect_descendants_ids(r));
            }
        }

        if !active_root.is_null() {
            self.draw_order
                .extend(self.collect_descendants_ids(active_root));
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

        self.widgets
            .retain(|_, w| w.last_frame_used == self.frame_count);
    }

    /// apply changes to the cursor icon
    ///
    /// called only once every frame to prevent flickering
    pub fn update_cursor_icon(&mut self) {
        // this is needed because outside events can change the icon, so we only update the icon
        // when it was manually changed
        if self.cursor_icon_changed {
            self.window.set_cursor(self.cursor_icon)
        }
    }

    pub fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if self.cursor_icon != icon {
            self.cursor_icon = icon;
            self.cursor_icon_changed = true;
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

        let mut ids = VecDeque::new();
        ids.push_back(id);
        // let mut ids = vec![id];
        // BF traversal
        while let Some(id) = ids.pop_front() {
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
        let flags = w.opt.flags;

        if !w.point_over(self.mouse.pos, self.resize_threshold) {
            self.hot_id = WidgetId::NULL;
            return;
        }

        // if mouse is pressed hot turns active
        if flags.clickable() && self.mouse.pressed(MouseBtn::Left) {
            self.active_id = id;
        }

        let mut can_resize = None;
        if flags.resizable() && self.curr_widget_action.is_none() {
            let r = &w_rect;
            let m = self.mouse.pos;

            let thr = self.resize_threshold + w.opt.outline_width / 2.0;

            let in_corner_region =
                |corner: Vec2| -> bool { corner.distance_squared(m) <= thr.powi(2) };

            if in_corner_region(r.right_top()) && flags.resizable_ne() {
                can_resize = Some(Dir::NE)
            } else if in_corner_region(r.right_bottom()) && flags.resizable_se() {
                can_resize = Some(Dir::SE)
            } else if in_corner_region(r.left_bottom()) && flags.resizable_sw() {
                can_resize = Some(Dir::SW)
            } else if in_corner_region(r.left_top()) && flags.resizable_nw() {
                can_resize = Some(Dir::NW)
            } else {
                let top_y = r.left_top().y;
                let bottom_y = r.left_bottom().y;
                let left_x = r.left_top().x;
                let right_x = r.right_top().x;

                if (m.y - top_y).abs() <= thr
                    && m.x >= left_x + thr
                    && m.x <= right_x - thr
                    && flags.resizable_n()
                {
                    can_resize = Some(Dir::N)
                } else if (m.y - bottom_y).abs() <= thr
                    && m.x >= left_x + thr
                    && m.x <= right_x - thr
                    && flags.resizable_s()
                {
                    can_resize = Some(Dir::S)
                } else if (m.x - right_x).abs() <= thr
                    && m.y >= top_y + thr
                    && m.y <= bottom_y - thr
                    && flags.resizable_e()
                {
                    can_resize = Some(Dir::E)
                } else if (m.x - left_x).abs() <= thr
                    && m.y >= top_y + thr
                    && m.y <= bottom_y - thr
                    && flags.resizable_w()
                {
                    can_resize = Some(Dir::W)
                }
            }

            if let Some(dir) = can_resize {
                self.set_cursor_icon(dir.as_cursor());

                if self.mouse.pressed(MouseBtn::Left) {
                    self.curr_widget_action = Some(WidgetAction::Resize {
                        dir,
                        id,
                        prev_rect: w_rect,
                    });
                    self.active_id = id;
                }
            }
        }
    }

    fn handle_widget_action(&mut self) {
        let m_start = self.mouse.drag_start(MouseBtn::Left);
        let m_delta = self.mouse.pos - m_start;

        match self.curr_widget_action {
            Some(WidgetAction::Resize { dir, id, prev_rect }) => {
                if !self.mouse.pressed(MouseBtn::Left) {
                    self.curr_widget_action = None;
                    return;
                }
                let w = self.widgets.get_mut(&id).unwrap();

                let min_size = w.total_min_size();
                let max_size = w.total_max_size();
                // TODO: clamp when resizing not working

                let mut r = prev_rect;

                if dir.has_n() {
                    r.min.y += m_delta.y;
                    if r.height() < min_size.y {
                        r.min.y = r.max.y - min_size.y;
                    }
                }
                if dir.has_s() {
                    r.max.y += m_delta.y;
                    if r.height() < min_size.y {
                        r.max.y = r.min.y + min_size.y;
                    }
                }
                if dir.has_w() {
                    r.min.x += m_delta.x;
                    if r.width() < min_size.x {
                        r.min.x = r.max.x - min_size.x;
                    }
                }
                if dir.has_e() {
                    r.max.x += m_delta.x;
                    if r.width() < min_size.x {
                        r.max.x = r.min.x + min_size.x;
                    }
                }

                w.rect = r;
            }
            Some(WidgetAction::Move { prev_pos, id }) => {
                if !self.mouse.pressed(MouseBtn::Left) {
                    self.curr_widget_action = None;
                    return;
                }
                let w = self.widgets.get_mut(&id).unwrap();
                let size = w.rect.size();

                // NOTE: cancel action
                // if self.mouse.pressed(MouseBtn::Right) {
                //     w.rect = Rect::from_min_size(prev_pos, size);
                //     self.curr_widget_action = None;
                //     self.active_id = WidgetId::NULL;
                //     return
                // }
                let pos = prev_pos + m_delta;
                w.rect = Rect::from_min_size(pos, size);
            }
            _ => (),
        }
    }

    pub fn update_active_widget(&mut self) {
        let id = self.active_id;
        if id.is_null() {
            return;
        }

        let w = self.widgets.get(&id).unwrap();

        // if mouse is dragged over active draggable widget and we are not performing any action,
        // start moving the widget
        if self.mouse.dragging(MouseBtn::Left)
            && w.opt.flags.draggable()
            && w.point_over(self.mouse.pos, 0.0)
            && self.curr_widget_action.is_none()
        {
            self.curr_widget_action = Some(WidgetAction::Move {
                prev_pos: w.rect.min,
                id: w.id,
            });
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

    pub fn is_hovered(&self, id: WidgetId) -> bool {
        id == self.hot_id
    }

    pub fn is_selected(&self, id: WidgetId) -> bool {
        id == self.active_id
    }

    pub fn add_button(&mut self, label: &str) -> bool {
        let id = self.id_from_str(label);
        // let size = Vec2::new(50.0 * label.len() as f32, 80.0);

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


        let mut opt = WidgetOpt::new()
            // .size_px(size.x, size.y)
            .text(label, 48.0, 1.0)
            .fill(fill)
            .clickable()
            .corner_radius(10.0)
            .outline(outline, 5.0);

        let size = self.draw.measure_text_size(opt.text_meta());
        opt = opt.size_px(size.x, size.y);


        let (_, signal) = self.begin_widget(label, opt);

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

    pub fn begin_widget(&mut self, label: &str, opt: WidgetOpt) -> (WidgetId, Signals) {
        let id = self.add_widget(label, opt);
        (id, self.handle_signal_of_id(id))
    }

    pub fn add_widget(&mut self, label: &str, opt: WidgetOpt) -> WidgetId {
        let id = self.id_from_str(label);
        let parent_id = self.parent_id();
        self.id_stack.push(id);

        let opt_size = opt.size;
        let opt_padding = opt.padding;

        if parent_id.is_null() {
            self.roots.push(id);
        }

        if let Some(pos) = opt.pos {
            self.cursor = pos;
        }

        self.cursor.x += opt.margin.left;
        self.cursor.y += opt.margin.top;

        // if widget is root we draw at same position as last frame
        let w = self.widgets.get(&id);
        if parent_id.is_null() {
            if let Some(w) = w {
                self.cursor = w.rect.min;
            }
        }


        let mut widget_size: Vec2 = if let Some(w) = w {
            w.rect.size()
        } else {
            opt.size.min_px_bound() + opt.padding.axis_sum()
        };

        if let Some(w) = self.widgets.get_mut(&id) {
            w.rect = Rect::from_min_size(self.cursor, widget_size);
            w.opt = opt;
            w.last_frame_used = self.frame_count;
        } else {
            let mut w = Widget::new(id, opt);
            w.rect = Rect::from_min_size(self.cursor, widget_size);
            w.last_frame_used = self.frame_count;
            self.widgets.insert(id, w);
            self.draw_order.push(id);
        }

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

        w.reset_frame_data();

        w.comp_min_size = opt_size.min_px_bound();
        w.comp_max_size = opt_size.max_px_bound();
        w.rect = Rect::from_min_size(
            self.cursor,
            w.rect
                .size()
                .min(w.total_max_size())
                .max(w.total_min_size()),
        );

        w.parent = parent_id;
        w.prev_sibling = prev_sibling;

        if !prev_sibling.is_null() {
            let sib = self.widgets.get_mut(&prev_sibling).unwrap();
            sib.next_sibling = id;
        }

        self.widget_stack.push(id);
        self.cursor.x += opt_padding.left;
        self.cursor.y += opt_padding.top;

        id
    }

    pub fn end_widget(&mut self) {
        self.id_stack.pop();
        let id = self.widget_stack.pop().unwrap();
        let w = self.widgets.get(&id).unwrap();

        // let w_opt = w.opt;
        let w_rect = w.rect;
        let opt_size = w.opt.size;
        let opt_margin = w.opt.margin;
        let p_id = w.parent;

        // let mut size = Vec2::ZERO;
        let mut fit_size = Vec2::ZERO;

        if w.opt.size.min[Axis::X].is_fit() || opt_size.max[Axis::X].is_fit() {
            fit_size.x = self.sum_children_sizes_along_axis(w, Axis::X);
        }
        if w.opt.size.min[Axis::Y].is_fit() || opt_size.max[Axis::Y].is_fit() {
            fit_size.y = self.sum_children_sizes_along_axis(w, Axis::Y);
        }

        let w = self.widgets.get_mut(&id).unwrap();

        w.rect_width_w_fit_size(fit_size.x);
        w.rect_height_w_fit_size(fit_size.y);

        // self.cursor = w.rect.min;
        if let Some(p) = self.widgets.get(&p_id) {
            match p.opt.layout {
                Layout::Vertical => {
                    self.cursor.y = w_rect.max.y + opt_margin.bottom + p.opt.spacing
                    // self.cursor.y += w_rect.height();
                    // self.cursor.y += w_opt.margin.bottom;
                    // self.cursor.y += p.opt.spacing;
                }
                Layout::Horizontal => {
                    self.cursor.x = w_rect.max.x + opt_margin.right + p.opt.spacing
                    // self.cursor.x += w_rect.width();
                    // self.cursor.x += w_opt.margin.right;
                    // self.cursor.x += p.opt.spacing;
                }
            }
        }
    }

    fn sum_children_sizes_along_axis(&self, w: &Widget, axis: Axis) -> f32 {
        let children: Vec<_> = self.iter_children(w.id).collect();
        let sizes = children.iter().map(|w| w.rect.size()[axis as usize]);
        let margins: f32 = children
            .iter()
            .map(|w| w.opt.margin.sum_along_axis(axis))
            .sum();

        let mut content_size = if w.opt.layout.axis() == axis {
            sizes.sum::<f32>() + (w.n_children.max(1) - 1) as f32 * w.opt.spacing
        } else {
            sizes.fold(0.0, f32::max)
        };
        let size = content_size + margins + w.opt.padding.sum_along_axis(axis) + w.opt.padding.sum_along_axis(axis);
        size
    }

    fn build_draw_data(&mut self) {
        for id in &self.draw_order {
            let w = self.widgets.get(id).unwrap();
            self.draw.draw_widget(w.rect, &w.opt);
        }

        if self.draw_dbg_wireframe {
            self.draw.debug_wireframe(2.0);
        }
    }
}

pub struct FontAtlasTexture {
    pub texture: gpu::Texture,
    pub alloc: etagere::BucketedAtlasAllocator,
    pub size: u32,
}

impl FontAtlasTexture {
    const SIZE: u32 = 1024;

    pub fn new(wgpu: &WGPU) -> Self {
        let size = Self::SIZE.min(wgpu.device.limits().max_texture_dimension_2d);

        let texture = wgpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font_atlas_texture"),
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
        }
    }

    pub fn allocate(&mut self, x: u32, y: u32) -> Option<etagere::Allocation> {
        self.alloc.allocate(etagere::Size::new(x as i32, y as i32))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMeta {
    pub pos: Vec2,
    pub size: Vec2,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub has_color: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphEntry {
    pub tex_indx: usize,
    pub meta: GlyphMeta,
}

#[derive(Clone, PartialEq)]
pub struct Glyph<'a> {
    pub texture: &'a gpu::Texture,
    pub meta: GlyphMeta,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    texture: gpu::Texture,
    pos: Vec2,
    size: Vec2,
    uv_min: Vec2,
    uv_max: Vec2,
    has_color: bool
}

impl fmt::Debug for Glyph<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Glyph")
            // .field("texture", &self.texture)
            .field("meta", &self.meta).finish()
    }
}



pub struct FontAtlas {
    pub textures: Vec<FontAtlasTexture>,
    pub glyph_cache: HashMap<ctext::CacheKey, GlyphEntry>,
}

impl FontAtlas {
    pub fn new(wgpu: &WGPU) -> Self {
        Self {
            textures: vec![FontAtlasTexture::new(wgpu)],
            glyph_cache: Default::default(),
        }
    }

    pub fn get_glyph(
        &mut self,
        glyph: ctext::CacheKey,
        font_system: &mut ctext::FontSystem,
        swash_cache: &mut ctext::SwashCache,
        wgpu: &WGPU,
    ) -> Option<Glyph<'_>> {
        if let Some(e) = self.glyph_cache.get(&glyph) {
            return Some(Glyph {
                texture: &self.textures[e.tex_indx].texture,
                meta: e.meta,
            });
        }

        log::trace!("load glyph");
        let image = swash_cache.get_image_uncached(font_system, glyph)?;
        let x = image.placement.left;
        let y = image.placement.top;
        let w = image.placement.width;
        let h = image.placement.height;

        let (has_color, data) = match image.content {
            ctext::SwashContent::Mask => {
                let mut data = Vec::new();
                data.reserve_exact((w * h * 4) as usize);
                for val in image.data {
                    data.push(255);
                    data.push(255);
                    data.push(255);
                    data.push(val);
                }
                (false, data)
            }
            ctext::SwashContent::Color => (true, image.data),
            ctext::SwashContent::SubpixelMask => {
                todo!()
            }
        };

        let alloc = if let Some(alloc) = self.textures.last_mut()?.allocate(w, h) {
            alloc
        } else {
            let mut texture = FontAtlasTexture::new(wgpu);
            let alloc = texture.allocate(w, h)?;
            self.textures.push(texture);
            alloc
        };

        let font_tex = self.textures.last()?;
        let alloc_rect = alloc.rectangle;

        wgpu.queue.write_texture(
            wgpu::TexelCopyTextureInfoBase {
                texture: &font_tex.texture.raw(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: alloc_rect.min.x as u32,
                    y: alloc_rect.min.y as u32,
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

        let pos = Vec2::new(x as f32, -y as f32);
        let size = Vec2::new(w as f32, h as f32);
        let uv_min =
            Vec2::new(alloc_rect.min.x as f32, alloc_rect.min.y as f32) / font_tex.size as f32;
        // TODO: check
        let uv_max = uv_min + size / font_tex.size as f32;

        let meta = GlyphMeta {
            pos,
            size,
            uv_min,
            uv_max,
            has_color,
        };

        self.glyph_cache.insert(
            glyph,
            GlyphEntry {
                tex_indx: self.textures.len() - 1,
                meta,
            },
        );

        Some(Glyph {
            texture: &font_tex.texture,
            meta,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextMeta {
    pub string: String,
    pub font_size_i: u64,
    pub line_height_i: u64,
    pub width_i: Option<u64>,
    pub height_i: Option<u64>,
}

impl TextMeta {
    pub const SIZE_RESOLUTION: f32 = 1024.0;

    pub fn new(text: String, font_size: f32, line_height: f32) -> Self {
        Self {
            string: text,
            font_size_i: (font_size * Self::SIZE_RESOLUTION) as u64,
            line_height_i: (line_height * Self::SIZE_RESOLUTION) as u64,
            width_i: None,
            height_i: None,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width_i = Some((width * Self::SIZE_RESOLUTION) as u64);
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height_i = Some((height * Self::SIZE_RESOLUTION) as u64);
        self
    }

    pub fn width(&self) -> Option<f32> {
        self.width_i.map(|w| w as f32 / Self::SIZE_RESOLUTION)
    }

    pub fn height(&self) -> Option<f32> {
        self.height_i.map(|h| h as f32 / Self::SIZE_RESOLUTION)
    }

    pub fn line_height(&self) -> f32 {
        self.line_height_i as f32 / Self::SIZE_RESOLUTION
    }

    pub fn font_size(&self) -> f32 {
        self.font_size_i as f32 / Self::SIZE_RESOLUTION
    }

    pub fn scaled_line_height(&self) -> f32 {
        self.line_height() * self.font_size()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextGlyphLayout {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
    pub height: f32,
}

pub type TextCache = HashMap<TextMeta, TextGlyphLayout>;

#[vertex]
pub struct Vertex {
    pub pos: Vec2,
    pub uv: Vec2,
    pub tex: u32,
    pub _pad: u32,
    pub col: RGBA,
}

impl Vertex {
    pub fn color(pos: Vec2, col: RGBA) -> Self {
        Self {
            pos,
            col,
            uv: Vec2::ZERO,
            tex: 0,
            _pad: 0,
        }
    }

    pub fn uv(pos: Vec2, uv: Vec2, tex: u32) -> Self {
        Self {
            pos,
            uv,
            tex,
            col: RGBA::ZERO,
            _pad: 0,
        }
    }
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

fn build_bind_group(
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

fn tessellate_uv_rect(rect: Rect, uv_min: Vec2, uv_max: Vec2, tex: u32) -> ([Vertex; 4], [u32; 6]) {
    let tl = Vertex::uv(rect.min, uv_min, tex);
    let tr = Vertex::uv(rect.min.with_x(rect.max.x), uv_min.with_x(uv_max.x), tex);
    let bl = Vertex::uv(rect.min.with_y(rect.max.y), uv_min.with_y(uv_max.y), tex);
    let br = Vertex::uv(rect.max, uv_max, tex);

    ([bl, br, tr, tl], [0, 1, 3, 1, 2, 3])
}

fn tessellate_line(
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

    let mut builder = BuffersBuilder::new(&mut buffers, |v: StrokeVertex| {
        Vertex::color(Vec2::new(v.position().x, v.position().y), col)
    });

    if let Err(e) = tess.tessellate_path(path.as_slice(), &options, &mut builder) {
        log::error!("Stroke tessellation failed: {:?}", e);
        return (Vec::new(), Vec::new());
    }

    (buffers.vertices, buffers.indices)
}

fn tessellate_fill(points: &[Vec2], fill: RGBA) -> (Vec<Vertex>, Vec<u32>) {
    use lyon::tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers,
    };
    if points.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let path = path_from_points(points, true);

    let mut buffers = VertexBuffers::<Vertex, u32>::new();
    let mut tess = FillTessellator::new();
    let mut builder = BuffersBuilder::new(&mut buffers, |v: FillVertex| {
        Vertex::color(Vec2::new(v.position().x, v.position().y), fill)
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

pub struct DrawList {
    pub vtx_buffer: Vec<Vertex>,
    pub idx_buffer: Vec<u32>,
    pub screen_size: Vec2,

    pub path: Vec<Vec2>,
    pub path_closed: bool,

    pub resolution: f32,

    pub font: ctext::FontSystem,
    pub text_swash_cache: ctext::SwashCache,
    pub font_atlas: FontAtlas,
    pub white_texture: gpu::Texture,
    pub text_cache: TextCache,
    pub text_cache_2: TextCache,
    pub text_size_cache: HashMap<TextMeta, f32>,

    pub wgpu: WGPUHandle,
}

// fn vtx(pos: impl Into<Vec2>, col: impl Into<RGBA>) -> Vertex {
//     Vertex {
//         pos: pos.into(),
//         col: col.into(),
//     }
// }

impl DrawList {
    pub fn new(wgpu: WGPUHandle) -> Self {
        Self {
            vtx_buffer: Vec::new(),
            idx_buffer: Vec::new(),
            screen_size: Vec2::ONE,
            path: Vec::new(),
            path_closed: false,
            resolution: 8.0,

            font: ctext::FontSystem::new(),
            text_swash_cache: ctext::SwashCache::new(),
            font_atlas: FontAtlas::new(&*wgpu),
            white_texture: gpu::Texture::create(&*wgpu, 1, 1, &RGBA::INDIGO.as_bytes()),
            text_cache: TextCache::new(),
            text_cache_2: TextCache::new(),
            text_size_cache: HashMap::new(),
            wgpu,
        }
    }

    pub fn clear(&mut self) {
        self.vtx_buffer.clear();
        self.idx_buffer.clear();
        self.path_clear();

        std::mem::swap(&mut self.text_cache, &mut self.text_cache_2);
        self.text_cache_2 = TextCache::new();
    }

    pub fn extend(
        &mut self,
        v: impl IntoIterator<Item = Vertex>,
        i: impl IntoIterator<Item = u32>,
    ) {
        let off = self.vtx_buffer.len() as u32;
        self.vtx_buffer.extend(v);
        self.idx_buffer.extend(i.into_iter().map(|i| i + off))
    }

    pub fn draw_uv_rect(&mut self, rect: Rect, uv_min: Vec2, uv_max: Vec2) {
        let (verts, indxs) = tessellate_uv_rect(rect, uv_min, uv_max, 1);
        self.extend(verts, indxs)
    }

    pub fn draw_text_layout(&mut self, layout: &TextGlyphLayout, pos: Vec2) {
        for glyph in &layout.glyphs {
            let rect = Rect::from_min_size(glyph.pos + pos, glyph.size);

            self.draw_uv_rect(rect, glyph.uv_min, glyph.uv_max);
        }
    }

    pub fn register_text(&mut self, text: TextMeta) -> &TextGlyphLayout {
        let text_str = text.string.clone();
        let text_width = text.width();
        let text_height = text.height();
        let font_size = text.font_size();
        // TODO: check
        let line_height = text.scaled_line_height();

        if let Some(layout) = self.text_cache.remove(&text) {
            // self.render_text()
            // log::info!("{text_str}: {:#?}", layout.glyphs);
            self.text_cache_2.insert(text.clone(), layout);
            return self.text_cache_2.get(&text).unwrap();
        } else if self.text_cache_2.contains_key(&text) {
            return self.text_cache_2.get(&text).unwrap()
        }
        // } else if let Some(layout) = self.text_cache_2.get(&text) {
        //     return layout;
        // }

        let mut buffer = ctext::Buffer::new(
            &mut self.font,
            ctext::Metrics {
                font_size,
                line_height,
            },
        );
        buffer.set_text(
            &mut self.font,
            &text_str,
            &ctext::Attrs::new().family(ctext::Family::SansSerif),
            ctext::Shaping::Advanced,
        );
        buffer.set_size(&mut self.font, text_width, text_height);
        buffer.shape_until_scroll(&mut self.font, false);

        let mut glyphs = Vec::new();

        let mut width = 0.0;
        let mut height = 0.0;

        for run in buffer.layout_runs() {

            width = run.line_w.max(width);
            height = run.line_height.max(height);

            for run_glyph in run.glyphs {
                let mut phys = run_glyph.physical((0.0, 0.0), 1.0);
                phys.cache_key.x_bin = ctext::SubpixelBin::Zero;
                phys.cache_key.y_bin = ctext::SubpixelBin::Zero;

                if let Some(glyph) = self.font_atlas.get_glyph(
                    phys.cache_key,
                    &mut self.font,
                    &mut self.text_swash_cache,
                    &self.wgpu,
                ) {
                    // TODO DPI
                    let pos = Vec2::new(phys.x as f32, phys.y as f32 + run.line_y) + glyph.meta.pos;
                    let size = glyph.meta.size;
                    let uv_min = glyph.meta.uv_min;
                    let uv_max = glyph.meta.uv_max;
                    let has_color = glyph.meta.has_color;
                    let texture = glyph.texture.clone();

                    glyphs.push(ShapedGlyph {
                        texture,
                        pos,
                        size,
                        uv_min,
                        uv_max,
                        has_color,
                    });
                }
            }
        }


        // margin of error
        width += 0.1;
        height += 0.1;
        log::trace!("register text: {text_str}");
        let layout = TextGlyphLayout { glyphs, width, height };
        self.text_cache_2.insert(text.clone(), layout);
        self.text_cache_2.get(&text).unwrap()
    }

    pub fn draw_text(&mut self, text: TextMeta, pos: Vec2) {
        let layout = self.register_text(text).clone();

        for glyph in &layout.glyphs {
            let rect = Rect::from_min_size(glyph.pos + pos, glyph.size);

            self.draw_uv_rect(rect, glyph.uv_min, glyph.uv_max);
        }
    }

    // pub fn draw_text2(&mut self, text: TextMeta, pos: Vec2) {
    //     let text_str = text.string.clone();
    //     let text_width = text.width();
    //     let text_height = text.height();
    //     let font_size = text.font_size();
    //     // TODO: check
    //     let line_height = text.line_height();

    //     // TODO: dpi scale
    //     if let Some(layout) = self.text_cache.remove(&text) {
    //         // self.render_text()
    //         // log::info!("{text_str}: {:#?}", layout.glyphs);
    //         self.draw_text_layout(&layout, pos);
    //         self.text_cache_2.insert(text, layout);
    //         return;
    //     } else if let Some(layout) = self.text_cache_2.get(&text) {
    //         let layout = layout.clone();
    //         self.draw_text_layout(&layout, pos);
    //         return;
    //     }

    //     let mut buffer = ctext::Buffer::new(
    //         &mut self.font,
    //         ctext::Metrics {
    //             font_size,
    //             line_height,
    //         },
    //     );
    //     buffer.set_text(
    //         &mut self.font,
    //         &text_str,
    //         &ctext::Attrs::new().family(ctext::Family::SansSerif),
    //         ctext::Shaping::Advanced,
    //     );
    //     buffer.set_size(&mut self.font, text_width, text_height);
    //     buffer.shape_until_scroll(&mut self.font, false);

    //     let mut glyphs = Vec::new();

    //     let mut width = 0.0;
    //     let mut height = 0.0;

    //     for run in buffer.layout_runs() {

    //         width = run.line_w.max(width);
    //         height = run.line_height.max(height);

    //         for run_glyph in run.glyphs {
    //             let mut phys = run_glyph.physical((0.0, 0.0), 1.0);
    //             phys.cache_key.x_bin = ctext::SubpixelBin::Zero;
    //             phys.cache_key.y_bin = ctext::SubpixelBin::Zero;

    //             if let Some(glyph) = self.font_atlas.get_glyph(
    //                 phys.cache_key,
    //                 &mut self.font,
    //                 &mut self.text_swash_cache,
    //                 &self.wgpu,
    //             ) {
    //                 // TODO DPI
    //                 let pos = Vec2::new(phys.x as f32, phys.y as f32 + run.line_y) + glyph.meta.pos;
    //                 let size = glyph.meta.size;
    //                 let uv_min = glyph.meta.uv_min;
    //                 let uv_max = glyph.meta.uv_max;
    //                 let has_color = glyph.meta.has_color;
    //                 let texture = glyph.texture.clone();

    //                 glyphs.push(ShapedGlyph {
    //                     texture,
    //                     pos,
    //                     size,
    //                     uv_min,
    //                     uv_max,
    //                     has_color,
    //                 });
    //             }
    //         }
    //     }

    //     let layout = TextGlyphLayout { glyphs, width, height };
    //     self.draw_text_layout(&layout, pos);
    //     self.text_cache_2.insert(text, layout);
    // }

    pub fn measure_text_size(&mut self, text: TextMeta) -> Vec2 {
        let layout = self.register_text(text);
        Vec2::new(layout.width, layout.height)
    }

    pub fn draw_widget(&mut self, rect: Rect, opt: &WidgetOpt) {
        self.path_rect(rect.min, rect.max, opt.corner_radius);

        if opt.flags.contains(WidgetFlags::DRAW_FILL) {
            let (vtx, idx) = tessellate_fill(&self.path, opt.fill);
            self.extend(vtx, idx);
        }

        if opt.flags.contains(WidgetFlags::DRAW_TEXT) {
            // self.draw_uv_rect(rect, Vec2::ZERO, Vec2::splat(1.0));
            // let text = TextMeta::new(opt.text.clone().unwrap_or("".into()), opt.font_size, 0.0);
            self.draw_text(opt.text_meta(), rect.min)
        }

        if opt.flags.contains(WidgetFlags::DRAW_OUTLINE) {
            self.path_clear();
            self.path_rect(rect.min, rect.max, opt.corner_radius);
            let (vtx, idx) = tessellate_line(&self.path, opt.outline_col, opt.outline_width, true);
            self.extend(vtx, idx);
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

impl gpu::RenderPassHandle for DrawList {
    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        // TODO: reuse vertex and index buffer
        // TODO: allocate large buffer e.g. 2048 rects

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

        let global_uniform = GlobalUniform {
            proj: Mat4::orthographic_lh(
                0.0,
                self.screen_size.x,
                self.screen_size.y,
                0.0,
                -1.0,
                0.0,
            ),
        };
        // .build_bind_group(wgpu);

        let bind_group = build_bind_group(
            global_uniform,
            self.font_atlas.textures.last().unwrap().texture.view(),
            wgpu,
        );

        // println!("{}", self.font_atlas.textures.len());

        rpass.set_bind_group(0, &bind_group, &[]);

        rpass.set_vertex_buffer(0, vtx.slice(..));
        rpass.set_index_buffer(idx.slice(..), wgpu::IndexFormat::Uint32);

        rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

        rpass.draw_indexed(0..self.idx_buffer.len() as u32, 0, 0..1);
    }
}

pub struct UiShader;

impl gpu::ShaderHandle for UiShader {
    const RENDER_PIPELINE_ID: gpu::ShaderID = "ui_shader";

    fn build_pipeline(&self, desc: &gpu::ShaderGenerics<'_>, wgpu: &WGPU) -> wgpu::RenderPipeline {
        const SHADER_SRC: &str = r#"


            @rust struct Vertex {
                pos: vec2<f32>,
                uv: vec2<f32>,
                col: vec4<f32>,
                tex: u32,
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
                @location(1) uv: vec2<f32>,
                @location(2) tex: u32,
            };

            @vertex
                fn vs_main(
                    v: Vertex,
                ) -> VSOut {
                    var out: VSOut;

                    if v.uv.x + v.uv.y != 0.0 {
                        out.color = vec4(v.uv, 0.0, 1.0);
                    } else {
                        out.color = v.col;
                    }

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
                    if in.tex == 1 {
                        return textureSample(texture, samp, in.uv);
                    } else {
                        return in.color;
                    }
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

        let shader_src = gpu::process_shader_code(SHADER_SRC, &desc).unwrap();
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
            .sample_count(gpu::Renderer::multisample_count())
            .build(&wgpu.device)
    }
}
