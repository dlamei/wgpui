use glam::{Mat4, UVec2, UVec4, Vec2, Vec4};
use macros::vertex;
use rustc_hash::FxHashMap;
use wgpu::util::DeviceExt;

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
    gpu::{self, ShaderHandle, Vertex as VertexTyp, VertexDesc, WGPU, WGPUHandle, Window},
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

impl<T> PerAxis<T> {
    pub fn x(&self) -> &T {
        &self.0[0]
    }

    pub fn y(&self) -> &T {
        &self.0[1]
    }
}

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

impl fmt::Debug for WidgetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ID({self})")
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SizeTyp {
    Px(f32),
    Fit,
    Text,
}

impl SizeTyp {
    pub fn is_fit(&self) -> bool {
        matches!(self, Self::Fit)
    }

    pub fn is_px(&self) -> bool {
        matches!(self, Self::Px(_))
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text)
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

    // pub fn min_px_bound(&self) -> Vec2 {
    //     let min_x = match self.min[Axis::X] {
    //         SizeTyp::Px(x) => x,
    //         SizeTyp::Fit => 0.0,
    //     };
    //     let min_y = match self.min[Axis::Y] {
    //         SizeTyp::Px(y) => y,
    //         SizeTyp::Fit => 0.0,
    //     };

    //     Vec2::new(min_x, min_y)
    // }

    // pub fn max_px_bound(&self) -> Vec2 {
    //     let min_x = match self.max[Axis::X] {
    //         SizeTyp::Px(x) => x,
    //         SizeTyp::Fit => f32::INFINITY,
    //     };
    //     let min_y = match self.max[Axis::Y] {
    //         SizeTyp::Px(y) => y,
    //         SizeTyp::Fit => f32::INFINITY,
    //     };

    //     Vec2::new(min_x, min_y)
    // }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetOpt {
    pub fill_color: RGBA,
    pub outline_color: RGBA,
    pub outline_width: f32,
    pub corner_radius: f32,

    /// defines the size of the widget
    ///
    /// used during creation and if the widget is resizable can be overwritten
    pub size: PerAxis<SizeTyp>,

    /// defines the minimum size of a resizable widget
    ///
    /// if resizable is not set, this is ignored
    pub min_size: PerAxis<SizeTyp>,

    /// defines the maximum size of a resizable widget
    ///
    /// if resizable is not set, this is ignored
    pub max_size: PerAxis<SizeTyp>,

    pub text: Option<String>,
    pub font_size: f32,
    pub text_color: RGBA,

    // temp
    pub tex_id: u32,

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

            // set x value
            pub fn [<size_x_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.size[Axis::X] = $e;
                self
            }

            // set y value
            pub fn [<size_y_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.size[Axis::Y] = $e;
                self
            }

            // set min value for x and y
            pub fn [<size_ $kind>](self, $([< $x _x >]:$ty,)* $([< $x _y >]:$ty),*) -> Self {
                self
                    .[<size_x_ $kind>]($([< $x _x >]),*)
                    .[<size_y_ $kind>]($([< $x _y >]),*)
            }

            // set min x value
            pub fn [<size_min_x_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.min_size[Axis::X] = $e;
                self
            }

            // set min y value
            pub fn [<size_min_y_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.min_size[Axis::Y] = $e;
                self
            }

            // set max x value
            pub fn [<size_max_x_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.max_size[Axis::X] = $e;
                self
            }

            // set max y value
            pub fn [<size_max_y_ $kind>](mut self, $($x:$ty),*) -> Self {
                self.max_size[Axis::Y] = $e;
                self
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

        }
    }
}

impl WidgetOpt {
    pub fn new() -> Self {
        Self {
            fill_color: RGBA::ZERO,
            outline_color: RGBA::ZERO,
            outline_width: 0.0,
            corner_radius: 0.0,
            // size: PerAxis([SizeTyp::Fit; 2]),
            size: PerAxis([SizeTyp::Fit; 2]),
            min_size: PerAxis([SizeTyp::Px(0.0); 2]),
            max_size: PerAxis([SizeTyp::Px(f32::INFINITY); 2]),
            // min_size: Vec2::ZERO,
            pos: None,

            text: None,
            font_size: 0.0,
            text_color: RGBA::WHITE,

            tex_id: 0,

            flags: WidgetFlags::NONE,
            layout: Default::default(),
            padding: Padding::ZERO,
            margin: Margin::ZERO,
            spacing: 0.0,
        }
    }

    widget_opt_size_fn!(px(px: f32) SizeTyp::Px(px));
    widget_opt_size_fn!(fit() SizeTyp::Fit);
    widget_opt_size_fn!(text() SizeTyp::Text);

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    // TODO[BUG]: margin is broken, test multiple widgets in a line wiht margins
    pub fn margin(mut self, m: f32) -> Self {
        self.margin = Margin::all(m);
        self
    }

    pub fn padding_dir(mut self, p: Padding) -> Self {
        self.padding = p;
        self
    }
    pub fn padding(mut self, m: f32) -> Self {
        self.padding = Padding::all(m);
        self
    }

    pub fn fill(mut self, fill: RGBA) -> Self {
        self.fill_color = fill;
        self.flags |= WidgetFlags::DRAW_FILL;
        self
    }

    pub fn text(mut self, s: impl Into<String>, font_size: f32) -> Self {
        self.text = Some(s.into());
        self.font_size = font_size;
        self.flags |= WidgetFlags::DRAW_TEXT;
        self
    }

    pub fn text_color(mut self, color: RGBA) -> Self {
        self.text_color = color;
        self
    }

    pub fn outline(mut self, col: RGBA, width: f32) -> Self {
        self.outline_color = col;
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
        let line_height = 1.0;
        TextMeta::new(
            self.text.clone().unwrap_or("".into()),
            self.font_size,
            line_height,
        )
    }

    pub fn pos_px(mut self, x: f32, y: f32) -> Self {
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

        // TODO[NOTE]: add outline hoverable?
        self.flags |= WidgetFlags::HOVERABLE;
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub id: WidgetId,
    pub parent: WidgetId,

    // siblings
    pub next: WidgetId,
    pub prev: WidgetId,

    // children
    pub first: WidgetId,
    pub last: WidgetId,
    pub n_children: u64,

    pub opt: WidgetOpt,

    /// depending on layout direction sum or max of children sizes + margins and spacing
    pub fit_size: Vec2,
    /// size of the widgets text, zero if text is empty
    pub text_size: Vec2,
    /// final shape of the widget when drawn
    pub rect: Rect,

    pub last_frame_used: u64,
    pub frame_created: u64,
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

impl Widget {
    pub fn new(id: WidgetId, opt: WidgetOpt) -> Self {
        Self {
            id,
            parent: WidgetId::NULL,
            first: WidgetId::NULL,
            last: WidgetId::NULL,
            n_children: 0,
            next: WidgetId::NULL,
            prev: WidgetId::NULL,
            rect: Rect::ZERO,
            fit_size: Vec2::ZERO,
            text_size: Vec2::ZERO,
            last_frame_used: 0,
            frame_created: 0,
            opt,
        }
    }

    /// clear data that must be re-computed every frame
    pub fn reset_frame_data(&mut self) {
        self.n_children = 0;
        self.parent = WidgetId::NULL;
        self.next = WidgetId::NULL;
        self.prev = WidgetId::NULL;
        self.first = WidgetId::NULL;
        self.last = WidgetId::NULL;
    }

    pub fn is_point_over(&self, point: Vec2, threshold: f32) -> bool {
        let off = Vec2::splat(self.opt.outline_width) / 2.0 + Vec2::splat(threshold);
        let mut min = self.rect.min ;
        let mut max = self.rect.max ;
        // if self.opt.flags.resizable() {
            min = min - off;
            max = max + off;
        // }
        Rect::from_min_max(min, max).contains(point)

    }

    pub fn is_in_resize_region(&self, pnt: Vec2, threshold: f32) -> Option<Dir> {
        let r = self.rect;
        let flags = self.opt.flags;
        let thr = threshold + self.opt.outline_width / 2.0;

        let in_corner_region =
            |corner: Vec2| -> bool { corner.distance_squared(pnt) <= thr.powi(2) };

        if in_corner_region(r.right_top()) && flags.resizable_ne() {
            Some(Dir::NE)
        } else if in_corner_region(r.right_bottom()) && flags.resizable_se() {
            Some(Dir::SE)
        } else if in_corner_region(r.left_bottom()) && flags.resizable_sw() {
            Some(Dir::SW)
        } else if in_corner_region(r.left_top()) && flags.resizable_nw() {
            Some(Dir::NW)
        } else {
            let top_y = r.left_top().y;
            let bottom_y = r.left_bottom().y;
            let left_x = r.left_top().x;
            let right_x = r.right_top().x;

            if (pnt.y - top_y).abs() <= thr
                && pnt.x >= left_x + thr
                && pnt.x <= right_x - thr
                && flags.resizable_n()
            {
                Some(Dir::N)
            } else if (pnt.y - bottom_y).abs() <= thr
                && pnt.x >= left_x + thr
                && pnt.x <= right_x - thr
                && flags.resizable_s()
            {
                Some(Dir::S)
            } else if (pnt.x - right_x).abs() <= thr
                && pnt.y >= top_y + thr
                && pnt.y <= bottom_y - thr
                && flags.resizable_e()
            {
                Some(Dir::E)
            } else if (pnt.x - left_x).abs() <= thr
                && pnt.y >= top_y + thr
                && pnt.y <= bottom_y - thr
                && flags.resizable_w()
            {
                Some(Dir::W)
            } else {
                None
            }
        }
    }

    // TODO[NOTE]: how do we handle if e.g. min is larger than max?
    // here the max is just the bigger of raw_min and raw_max
    // pub fn compute_max_size(&self) -> Vec2 {
    //     let rmax = self.compute_raw_max_size();
    //     let rmin = self.compute_raw_min_size();
    //     rmin.max(rmax)
    // }

    // pub fn compute_min_size(&self) -> Vec2 {
    //     let rmax = self.compute_raw_max_size();
    //     let rmin = self.compute_raw_min_size();
    //     rmin.min(rmax)
    // }

    pub fn compute_min_size(&self) -> Vec2 {
        let min = self.opt.outline_width.max(self.opt.corner_radius * 2.0);
        let min = Vec2::splat(min).max(self.opt.padding.axis_sum());

        let min_x = match self.opt.min_size[Axis::X] {
            SizeTyp::Px(x) => x,
            SizeTyp::Fit => self.fit_size.x,
            SizeTyp::Text => self.text_size.x,
        };
        let min_y = match self.opt.min_size[Axis::Y] {
            SizeTyp::Px(y) => y,
            SizeTyp::Fit => self.fit_size.y,
            SizeTyp::Text => self.text_size.y,
        };
        min.max(Vec2::new(min_x, min_y))
    }

    pub fn compute_max_size(&self) -> Vec2 {
        let max_x = match self.opt.max_size[Axis::X] {
            SizeTyp::Px(x) => x,
            SizeTyp::Fit => self.fit_size.x,
            SizeTyp::Text => self.text_size.x,
        };
        let max_y = match self.opt.max_size[Axis::Y] {
            SizeTyp::Px(y) => y,
            SizeTyp::Fit => self.fit_size.y,
            SizeTyp::Text => self.text_size.y,
        };
        Vec2::new(max_x, max_y)
    }
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

    pub fn as_winit_resize(&self) -> winit::window::ResizeDirection {
        use winit::window::ResizeDirection as RD;
        match self {
            Dir::N => RD::North,
            Dir::NE => RD::NorthEast,
            Dir::E => RD::East,
            Dir::SE => RD::SouthEast,
            Dir::S => RD::South,
            Dir::SW => RD::SouthWest,
            Dir::W => RD::West,
            Dir::NW => RD::NorthWest,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WidgetAction {
    Resize {
        dir: Dir,
        id: WidgetId,
        prev_rect: Rect,
    },
    Drag {
        start_pos: Vec2,
        id: WidgetId,
    },
    ResizeWindow {
        dir: Dir,
    },
    DragWindow,

    None,
}

impl WidgetAction {
    pub fn is_none(&self) -> bool {
        match self {
            Self::None => true,
            _ => false,
        }
    }

    pub fn is_window_action(&self) -> bool {
        match self {
            Self::DragWindow { .. } | Self::ResizeWindow { .. } => true,
            _ => false,
        }
    }
}

impl fmt::Display for WidgetAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resize { dir, id, prev_rect } => {
                write!(f, "RESIZE[{dir:?}] {{ {id}, {prev_rect} }}")
            }
            Self::Drag { start_pos, id } => write!(f, "MOVE {{ {id}, {start_pos} }}"),

            Self::ResizeWindow { dir } => {
                write!(f, "RESIZE_WINDOW[{dir:?}]")
            }
            Self::DragWindow => write!(f, "MOVE_WINDOW"),
            Self::None => write!(f, "NONE"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Placement {
    #[default]
    Min,
    Center,
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
    pub next_widget_placement: PerAxis<Placement>,

    /// generate triangle vertex & index buffers
    pub draw: DrawList,

    pub resize_threshold: f32,
    pub curr_widget_action: WidgetAction,

    pub cursor_icon: CursorIcon,
    pub cursor_icon_changed: bool,

    pub prev_n_draw_calls: u32,

    pub draw_dbg_wireframe: bool,

    pub custom_tab_height: f32,

    pub window_id: WidgetId,
    pub cursor_in_window: bool,
    pub window: Window,
    // pub window: Window,
}

impl ops::Index<WidgetId> for State {
    type Output = Widget;

    fn index(&self, index: WidgetId) -> &Self::Output {
        self.widgets.get(&index).unwrap()
    }
}

impl ops::IndexMut<WidgetId> for State {
    fn index_mut(&mut self, index: WidgetId) -> &mut Self::Output {
        self.widgets.get_mut(&index).unwrap()
    }
}

impl State {
    pub fn new(wgpu: WGPUHandle, window: Window) -> Self {
        Self {
            draw: DrawList::new(wgpu),
            cursor_icon: CursorIcon::Default,
            cursor_icon_changed: false,
            cursor: Vec2::ZERO,
            next_widget_placement: PerAxis([Placement::Min; 2]),
            roots: Vec::new(),
            curr_widget_action: WidgetAction::None,
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
            prev_n_draw_calls: 0,
            custom_tab_height: 40.0,
            window_id: WidgetId::NULL,
            window,
            cursor_in_window: true,
        }
    }

    pub fn set_mouse_press(&mut self, button: MouseBtn, press: bool) {
        self.mouse.set_button_press(button, press);

        let w_size = self.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);

        let resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold);
        let lft_btn = button == MouseBtn::Left;

        if !self.window.is_decorated() {
            if press && lft_btn {
                if let Some(dir) = resize_dir {
                    self.curr_widget_action = WidgetAction::ResizeWindow { dir };
                    self.window.start_drag_resize_window(dir)
                } else if self.mouse.pos.y <= self.custom_tab_height {
                    self.curr_widget_action = WidgetAction::DragWindow;
                    self.window.start_drag_window()
                }
            }

            if !press && lft_btn && self.curr_widget_action.is_window_action() {
                self.curr_widget_action = WidgetAction::None;
            }
        }
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.mouse.set_mouse_pos(x, y);

        let w_size = self.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);
        let resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold);

        if let Some(dir) = resize_dir {
            self.set_cursor_icon(dir.as_cursor());
        } else if self.cursor_icon.is_resize() {
            self.set_cursor_icon(CursorIcon::Default);
        }
        // }
    }

    pub fn set_next_placement_x(&mut self, p: Placement) {
        self.next_widget_placement[Axis::X] = p;
    }
    pub fn set_next_placement_y(&mut self, p: Placement) {
        self.next_widget_placement[Axis::Y] = p;
    }

    pub fn set_next_placement(&mut self, px: Placement, py: Placement) {
        self.set_next_placement_x(px);
        self.set_next_placement_y(py);
    }

    pub fn start_frame(&mut self) {
        self.draw.clear();
        self.id_stack.clear();
        self.roots.clear();
        self.widget_stack.clear();
        self.cursor = Vec2::ZERO;

        self.draw.screen_size = self.window.window_size();

        match self.curr_widget_action {
            WidgetAction::Resize { dir, .. } | WidgetAction::ResizeWindow { dir } => {
                self.set_cursor_icon(dir.as_cursor());
            }
            _ => (),
        }
        // if self.curr_widget_action.is_none() {
        //     self.set_cursor_icon(CursorIcon::Default)
        // }

        // self.draw.draw_uv_rect(
        //     Rect::from_min_max(Vec2::ZERO, Vec2::splat(800.0)),
        //     Vec2::ZERO,
        //     Vec2::splat(1.0),
        // );
    }

    pub fn begin_window(&mut self, title: &str) {
        let padding = 5.0;

        // without padding
        let tab_height = self.custom_tab_height - 2.0 * padding;
        let bg_col = RGBA::hex("#242933");
        let tab_col = RGBA::hex("#2e2e2e");
        let tab_col = bg_col;

        let win_size = self.window.window_size();
        // let mut opt = WidgetOpt::new()
            // .resizable()
            // .draggable()
            // .fill()
            // .corner_radius(10.0)
            // .padding(padding)
            // .size_fit();

        // if let Some(bg) = bg {
        //     opt = opt.fill(bg);
        // }
        // if let Some((col, width)) = border {
        //     opt = opt.outline(col, width);
        // }


        if self.window.is_decorated() {
            let (win_id, _) = self.begin_widget(title, 
                WidgetOpt::new()
                .fill(bg_col)
                .size_px(win_size.x as f32, win_size.y as f32)
                .padding(padding)
            );
            self[win_id].rect = Rect::from_min_size(Vec2::ZERO, win_size);
            let win_rect = self[win_id].rect;
            return
        }

        self.set_cursor(0.0, 0.0);
        let (id, _) = self.begin_widget(
            "window bar#",
            WidgetOpt::new()
            .size_px(win_size.x, self.custom_tab_height)
            .fill(tab_col)
            .layout_h()
            .padding_dir(Padding::new(0.0, 0.0, 5.0, 5.0))
            .spacing(10.0)
        );
        self[id].rect = Rect::from_min_size(Vec2::ZERO, Vec2::new(win_size.x, self.custom_tab_height));

        self.add_label(title, 25.0);

        let mut add_icon = |ui: &mut State, name: &str| -> bool {
            let id = ui.next_id(name);
            let mut is_hover = false;
            let mut is_active = false;
            if let Some(w) = ui.widgets.get(&id) {
                is_hover = ui.is_hot(id);
                is_active = ui.is_active(id);
            }

            let mut fill = if is_hover {
                RGBA::hex("#3e4759")
            } else {
                tab_col
            };

            let (_, sig) = ui.begin_widget(
                name,
                WidgetOpt::new()
                .text(name, 30.0)
                .size_text()
                .fill(fill)
                .padding(5.0)
                .corner_radius(5.0)
                .clickable()
                .layout_h());

            ui.end_widget();
            sig.released()
        };

        self.offset_cursor_x(win_size.x - 300.0);
        if add_icon(self, "min") {
            self.window.minimize();
        }
        if add_icon(self, "max") {
            self.window.toggle_maximize();
        }
        if add_icon(self, "ext") {
            std::process::exit(0)
        }
        self.end_widget();


        let (win_id, _) = self.begin_widget(title, 
            WidgetOpt::new()
            .fill(bg_col)
            .size_px(win_size.x as f32, win_size.y as f32)
            .padding(padding)
        );
        self[win_id].rect = Rect::from_min_max(Vec2::new(0.0, self.custom_tab_height), win_size);

    }

    pub fn end_window(&mut self) {
        // window
        self.end_widget();
    }

    pub fn add_window(&mut self, label: &str) {
        let win_size = self.window.window_size();
        let (id, _) = self.begin_widget(label, WidgetOpt::new().layout_h());

        if self.add_button("close") {
            log::info!("close")
        }
        let rect = &mut self[id].rect;
        *rect = Rect::from_min_size(Vec2::ZERO, win_size);

        self.end_widget();
        self.window_id = id;
    }

    pub fn debug_window(&mut self, dt: Duration) {
        self.begin_widget(
            "debug",
            WidgetOpt::new()
                .fill(RGBA::INDIGO)
                .size_fit()
                .draggable()
                .pos_px(10.0, 10.0)
                .padding(40.0)
                .corner_radius(10.0)
                .spacing(18.0)
                .outline(RGBA::DARK_BLUE, 5.0),
        );

        self.add_label("Debug", 128.0);
        self.offset_cursor_y(32.0);

        self.add_widget(
            "dt",
            WidgetOpt::new()
                .text(&format!("dt: {dt:?}"), 32.0)
                .size_text(),
        );
        self.end_widget();

        self.add_widget(
            "hot",
            WidgetOpt::new()
                .text(&format!("hot: {}", self.hot_id), 32.0)
                .size_text(),
        );
        self.end_widget();

        self.add_widget(
            "active",
            WidgetOpt::new()
                .text(&format!("active: {}", self.active_id), 32.0)
                .size_text(),
        );
        self.end_widget();

        self.add_widget(
            "action",
            WidgetOpt::new()
                .text(&format!("action: {:?}", self.curr_widget_action), 32.0)
                .size_text(),
        );
        self.end_widget();

        self.add_widget(
            "n_draw_calls",
            WidgetOpt::new()
                .text(
                    &format!("n. of draw calls: {}", self.prev_n_draw_calls),
                    32.0,
                )
                .size_text(),
        );
        self.end_widget();

        let mut opt = WidgetOpt::new().size_px(500.0, 500.0);
        opt.tex_id = 1;
        self.add_widget("texture atlas", opt);

        // let tex_size = Vec2::splat(100.0);
        // self.draw.draw_uv_rect(Rect::from_min_size(self.cursor, tex_size), Vec2::ZERO, Vec2::splat(1.0), RGBA::WHITE);
        // self.offset_cursor_y(tex_size.y);

        self.end_widget();

        self.end_widget();
    }

    pub fn end_frame(&mut self) {
        if !self.id_stack.is_empty() {
            log::warn!("end_frame: id_stack is not empty at frame end");
        }
        if !self.widget_stack.is_empty() {
            log::warn!("end_frame: widget_stack is not empty at frame end");
        }

        if let Some(w) = self.widgets.get(&WidgetId::NULL) {
            log::warn!("widget should not have null as id:\n{:?}", w);
        }

        self.prune_unused_nodes();

        self.update_hot_widget();
        self.update_active_widget();
        self.handle_widget_action();
        self.update_cursor_icon();
        self.cursor_icon_changed = false;

        self.mouse.clear_released();

        let active_root = self.get_root(self.active_id);

        // we want to bring the active ui tree to the front, but must keep the draw order otherwise
        // the same
        let mut roots_draw_order = self.roots.clone();
        roots_draw_order.sort_by_key(|x| self.draw_order.iter().position(|y| y == x).unwrap());

        self.draw_order.clear();

        for r in roots_draw_order {
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
            self.window.set_cursor_icon(self.cursor_icon)
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

    /// provides the id of the next widget given the widgets label
    ///
    pub fn next_id(&self, str: &str) -> WidgetId {
        self.id_from_str(str)
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
        let mut c_id = self.widgets[&id].first;
        std::iter::from_fn(move || {
            if c_id.is_null() {
                None
            } else {
                let c = &self.widgets[&c_id];
                c_id = c.next;
                Some(c)
            }
        })
    }

    pub fn iter_children_ids(&self, id: WidgetId) -> impl Iterator<Item = WidgetId> {
        let mut c_id = self.widgets[&id].first;
        std::iter::from_fn(move || {
            if c_id.is_null() {
                None
            } else {
                let c = &self.widgets[&c_id];
                c_id = c.next;
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
        // let id = self.hot_id;
        if self.hot_id.is_null() {
            return;
        }

        let w = self.widgets.get(&self.hot_id).unwrap();
        let w_rect = w.rect;
        let flags = w.opt.flags;

        let threshold = if w.opt.flags.resizable() {
            self.resize_threshold
        } else {
            0.0
        };
        if !w.is_point_over(self.mouse.pos, threshold) {
            self.hot_id = WidgetId::NULL;
            return;
        }

        if flags.clickable() && self.mouse.pressed(MouseBtn::Left) {
            // if mouse is pressed hot turns active
            self.active_id = self.hot_id;
        } else if self.mouse.pressed(MouseBtn::Left) {
            // if mosue is presed but the current hot widget is not clickable search for topmost
            // clickable widget under mouse and set it to active
            for &id in self.draw_order.iter().rev() {
                let w = &self[id];
                if w.opt.flags.clickable() && w.is_point_over(self.mouse.pos, 0.0) {
                    self.active_id = id;
                    break;
                }
            }
        }

        // TODO[BUG]: starting a drag and then crossing the outline should not result in resizing
        if self.curr_widget_action.is_none() && w.opt.flags.resizable() {
            if let Some(dir) = w.is_in_resize_region(self.mouse.pos, self.resize_threshold) {
                self.set_cursor_icon(dir.as_cursor());

                if self.mouse.pressed(MouseBtn::Left) {
                    self.curr_widget_action = WidgetAction::Resize {
                        dir,
                        id: self.hot_id,
                        prev_rect: w_rect,
                    };
                    self.active_id = self.hot_id;
                }
            }
        }
        // let mut can_resize = None;
        // if flags.resizable() && self.curr_widget_action.is_none() {
        // let r = &w_rect;
        // let m = self.mouse.pos;

        // let thr = self.resize_threshold + w.opt.outline_width / 2.0;

        // let in_corner_region =
        //     |corner: Vec2| -> bool { corner.distance_squared(m) <= thr.powi(2) };

        // if in_corner_region(r.right_top()) && flags.resizable_ne() {
        //     can_resize = Some(Dir::NE)
        // } else if in_corner_region(r.right_bottom()) && flags.resizable_se() {
        //     can_resize = Some(Dir::SE)
        // } else if in_corner_region(r.left_bottom()) && flags.resizable_sw() {
        //     can_resize = Some(Dir::SW)
        // } else if in_corner_region(r.left_top()) && flags.resizable_nw() {
        //     can_resize = Some(Dir::NW)
        // } else {
        //     let top_y = r.left_top().y;
        //     let bottom_y = r.left_bottom().y;
        //     let left_x = r.left_top().x;
        //     let right_x = r.right_top().x;

        //     if (m.y - top_y).abs() <= thr
        //         && m.x >= left_x + thr
        //         && m.x <= right_x - thr
        //         && flags.resizable_n()
        //     {
        //         can_resize = Some(Dir::N)
        //     } else if (m.y - bottom_y).abs() <= thr
        //         && m.x >= left_x + thr
        //         && m.x <= right_x - thr
        //         && flags.resizable_s()
        //     {
        //         can_resize = Some(Dir::S)
        //     } else if (m.x - right_x).abs() <= thr
        //         && m.y >= top_y + thr
        //         && m.y <= bottom_y - thr
        //         && flags.resizable_e()
        //     {
        //         can_resize = Some(Dir::E)
        //     } else if (m.x - left_x).abs() <= thr
        //         && m.y >= top_y + thr
        //         && m.y <= bottom_y - thr
        //         && flags.resizable_w()
        //     {
        //         can_resize = Some(Dir::W)
        //     }
        // }

        //     if let Some(dir) = can_resize {
        //         self.set_cursor_icon(dir.as_cursor());

        //         if self.mouse.pressed(MouseBtn::Left) {
        //             self.curr_widget_action = Some(WidgetAction::Resize {
        //                 dir,
        //                 id: self.hot_id,
        //                 prev_rect: w_rect,
        //             });
        //             self.active_id = self.hot_id;
        //         }
        //     }
        // }
    }

    // TODO[NOTE]: moving a widget leads to its children being a frame behind
    fn handle_widget_action(&mut self) {
        let m_start = self.mouse.drag_start(MouseBtn::Left);
        let m_delta = self.mouse.pos - m_start;

        match self.curr_widget_action {
            WidgetAction::Resize { dir, id, prev_rect } => {
                if !self.mouse.pressed(MouseBtn::Left) {
                    self.set_cursor_icon(CursorIcon::Default);
                    self.curr_widget_action = WidgetAction::None;
                    return;
                }
                let w = self.widgets.get_mut(&id).unwrap();

                let min_size = w.compute_min_size();
                let max_size = w.compute_max_size();

                let mut r = prev_rect;

                if dir.has_n() {
                    r.min.y += m_delta.y;
                    if r.height() < min_size.y {
                        r.min.y = r.max.y - min_size.y;
                    }
                    if r.height() > max_size.y {
                        r.min.y = r.max.y - max_size.y;
                    }
                }
                if dir.has_s() {
                    r.max.y += m_delta.y;
                    if r.height() < min_size.y {
                        r.max.y = r.min.y + min_size.y;
                    }
                    if r.height() > max_size.y {
                        r.max.y = r.min.y + max_size.y;
                    }
                }
                if dir.has_w() {
                    r.min.x += m_delta.x;
                    if r.width() < min_size.x {
                        r.min.x = r.max.x - min_size.x;
                    }
                    if r.width() > max_size.x {
                        r.min.x = r.max.x - max_size.x;
                    }
                }
                if dir.has_e() {
                    r.max.x += m_delta.x;
                    if r.width() < min_size.x {
                        r.max.x = r.min.x + min_size.x;
                    }
                    if r.width() > max_size.x {
                        r.max.x = r.min.x + max_size.x;
                    }
                }

                w.rect = r;
            }
            WidgetAction::Drag { start_pos, id } => {
                if !self.mouse.pressed(MouseBtn::Left) {
                    self.curr_widget_action = WidgetAction::None;
                    return;
                }
                let w = self.widgets.get_mut(&id).unwrap();
                let size = w.rect.size();

                // NOTE: cancel action
                if self.mouse.pressed(MouseBtn::Right) {
                    w.rect = w.rect.translate(start_pos - w.rect.min);
                    // disable action and selection
                    self.curr_widget_action = WidgetAction::None;
                    self.active_id = WidgetId::NULL;
                    return;
                }
                let pos = start_pos + m_delta;
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

        if !self.mouse.dragging(MouseBtn::Left)
            && self.mouse.pressed(MouseBtn::Left)
            && !w.is_point_over(self.mouse.pos, 0.0)
        {
            self.active_id = WidgetId::NULL;
            return;
        }

        // if mouse is dragged over active draggable widget and we are not performing any action,
        // start moving the widget
        if self.mouse.dragging(MouseBtn::Left)
            && w.opt.flags.draggable()
            && w.is_point_over(self.mouse.pos, 0.0)
            && self.curr_widget_action.is_none()
        {
            self.curr_widget_action = WidgetAction::Drag {
                start_pos: w.rect.min,
                id: w.id,
            };
        }
    }

    pub fn handle_signal_of_id(&mut self, id: WidgetId) -> Signals {
        let Some(w) = self.widgets.get(&id) else {
            return Signals::NONE;
        };

        let mut signal = Signals::NONE;

        let threshold = if w.opt.flags.resizable() {
            self.resize_threshold
        } else {
            0.0
        };
        if w.opt.flags.hoverable() && w.is_point_over(self.mouse.pos, threshold) {
            signal |= Signals::MOUSE_OVER;
        }

        if signal.mouse_over() && !self.mouse.dragging(MouseBtn::Left) {
            if self.hot_id.is_null() || self.is_id_over(id, self.hot_id) {
                self.hot_id = id;
            }
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

    pub fn is_hot(&self, id: WidgetId) -> bool {
        id == self.hot_id
    }

    pub fn is_active(&self, id: WidgetId) -> bool {
        id == self.active_id
    }

    pub fn add_label(&mut self, label: &str, text_size: f32) -> (WidgetId, Signals) {
        let mut opt = WidgetOpt::new()
            .text(label, text_size)
            .size_text()
            .padding(8.0);

        let (id, signal) = self.begin_widget(label, opt);

        self.end_widget();
        (id, signal)
    }

    pub fn add_button_impl(&mut self, label: &str, size: f32) -> (WidgetId, Signals) {
        // TODO[BUG]: when performing a double press with the first press being on the button, and
        // second outside we still set button to active

        let id = self.next_id(label);

        let btn_default_fill = RGBA::hex("#0F1113");
        let btn_default_outline = RGBA::hex("#232629");
        let text_default = RGBA::hex("#7e7e7e");

        let btn_hover_fill = RGBA::hex("#141618");
        let btn_hover_outline = RGBA::hex("#4f565d");
        let text_hover = RGBA::hex("#f9f9f9");

        // let btn_active_fill = RGBA::hex("#0B0C0D");
        let btn_active_fill = RGBA::hex("#e65858");
        let btn_active_outline = RGBA::hex("#18191A");
        let text_active = text_default;

        let (fill, outline, text) = if self.is_active(id) && self.mouse.pressed(MouseBtn::Left) {
            (btn_active_fill, btn_hover_outline, text_hover)
        } else if self.is_hot(id) {
            (btn_hover_fill, btn_hover_outline, text_hover)
        } else {
            (btn_default_fill, btn_default_outline, text_default)
        };

        let mut opt = WidgetOpt::new()
            .text(label, size)
            .text_color(text)
            .size_text()
            .padding(8.0)
            .fill(fill)
            .clickable()
            .corner_radius(10.0)
            .outline(outline, 5.0);

        let (id, signal) = self.begin_widget(label, opt);

        self.end_widget();
        (id, signal)
    }

    pub fn add_button(&mut self, label: &str) -> bool {
        self.add_button_impl(label, 32.0).1.released()
    }

    pub fn offset_cursor_y(&mut self, y: f32) {
        self.cursor.y += y;
    }

    pub fn offset_cursor_x(&mut self, x: f32) {
        self.cursor.x += x;
    }


    pub fn set_cursor_y(&mut self, y: f32) {
        self.cursor.y = y;
    }

    pub fn set_cursor_x(&mut self, x: f32) {
        self.cursor.x = x;
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) {
        self.set_cursor_x(x);
        self.set_cursor_y(y);
    }

    pub fn offset_cursor(&mut self, x: f32, y: f32) {
        self.offset_cursor_x(x);
        self.offset_cursor_y(y);
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
        let padding = opt.padding;

        // if parent is none we have a new root
        if parent_id.is_null() {
            self.roots.push(id);
        }

        // TODO[CHECK]
        if let Some(pos) = opt.pos {
            self.cursor = pos;
        }

        // offset by widgets margin before placing it
        self.cursor.x += opt.margin.left;
        self.cursor.y += opt.margin.top;

        // if widget is root we draw at same position as last frame
        let w = self.widgets.get(&id);
        if parent_id.is_null() {
            if let Some(w) = w {
                self.cursor = w.rect.min;
            }
        }

        // remeasure text_size because it could have changed
        let text_size = self.draw.measure_text_size(opt.text_meta());

        let widget_size: Vec2 = if let Some(w) = w {
            // use previous size if available
            let mut size = w.rect.size();
            match opt.size.x() {
                SizeTyp::Px(x) => size.x = *x,
                SizeTyp::Text => size.x = text_size.x,
                SizeTyp::Fit => (),
            }
            match opt.size.y() {
                SizeTyp::Px(y) => size.y = *y,
                SizeTyp::Text => size.y = text_size.y,
                SizeTyp::Fit => (),
            }
            size
        } else {
            // init size with fixed size if available + padding
            let x = match opt.size[Axis::X] {
                SizeTyp::Px(x) => x,
                SizeTyp::Fit => 0.0,
                SizeTyp::Text => text_size.x,
            } + opt.padding.sum_along_axis(Axis::X);

            let y = match opt.size[Axis::Y] {
                SizeTyp::Px(y) => y,
                SizeTyp::Fit => 0.0,
                SizeTyp::Text => text_size.y,
            } + opt.padding.sum_along_axis(Axis::Y);
            Vec2::new(x, y)
        };

        let mut rect = Rect::from_min_size(self.cursor.round(), widget_size);
        let off_x = match self.next_widget_placement.x() {
            Placement::Min => 0.0,
            Placement::Center => -widget_size.x / 2.0,
        };
        let off_y = match self.next_widget_placement.y() {
            Placement::Min => 0.0,
            Placement::Center => -widget_size.y / 2.0,
        };
        rect = rect.translate(Vec2::new(off_x, off_y));

        // update existing or init
        if let Some(w) = self.widgets.get_mut(&id) {
            // w.rect = Rect::from_min_size(self.cursor, widget_size);
            w.opt = opt;
            w.text_size = text_size;
        } else {
            let mut w = Widget::new(id, opt);
            // w.rect = Rect::from_min_size(self.cursor, widget_size);
            w.text_size = text_size;
            w.frame_created = self.frame_count;
            self.widgets.insert(id, w);
            self.draw_order.push(id);
        }

        let mut prev_sibling = WidgetId::NULL;
        if let Some(p) = self.parent_widget_mut() {
            let last = p.last;
            p.last = id;
            if p.n_children == 0 {
                p.first = id;
            }
            p.n_children += 1;
            prev_sibling = last;
        }

        let w = self.widgets.get_mut(&id).unwrap();
        w.last_frame_used = self.frame_count;
        w.rect = rect;

        // clear non persistant data, e.g. children
        w.reset_frame_data();
        w.parent = parent_id;
        w.prev = prev_sibling;
        w.text_size = text_size;

        if !prev_sibling.is_null() {
            let sib = self.widgets.get_mut(&prev_sibling).unwrap();
            sib.next = id;
        }

        // offset cursor by padding
        // for children
        self.cursor.x += padding.left;
        self.cursor.y += padding.top;

        self.id_stack.push(id);
        self.widget_stack.push(id);
        self.next_widget_placement = PerAxis([Placement::Min; 2]);

        id
    }

    pub fn end_widget(&mut self) -> WidgetId {
        // pop from stack
        self.id_stack.pop();
        let id = self
            .widget_stack
            .pop()
            .expect("called end_widget on empty widget stack");
        let w = self.widgets.get(&id).unwrap();

        let rect = w.rect;
        let margin = w.opt.margin;
        let padding = w.opt.padding;
        let p_id = w.parent;

        let fit_size = self.measure_children(w);
        let text_size = w.text_size;
        let w = self.widgets.get_mut(&id).unwrap();
        w.fit_size = fit_size;

        // set fit size directly if not resizable
        // or if widget was created this frame
        // TODO[NOTE]: maybe allow for resizable widgets to be resized when children are resized,
        // resizable widgets get resized only at creation and when out of bounds
        if (!w.opt.flags.resizable_x() || w.frame_created == self.frame_count)
        // && w.opt.size.x().is_fit()
        {
            if w.opt.size.x().is_fit() {
                w.rect
                    .set_width(fit_size.x + padding.sum_along_axis(Axis::X))
            }
            if w.opt.size.x().is_text() {
                w.rect
                    .set_width(text_size.x + padding.sum_along_axis(Axis::X))
            }
        }
        if (!w.opt.flags.resizable_y() || w.frame_created == self.frame_count)
        // && w.opt.size.y().is_fit()
        {
            if w.opt.size.y().is_fit() {
                w.rect
                    .set_height(fit_size.y + padding.sum_along_axis(Axis::Y))
            }
            if w.opt.size.y().is_text() {
                w.rect
                    .set_height(text_size.y + padding.sum_along_axis(Axis::Y))
            }
        }

        // if resizable clamp to updated min size
        let min_size = w.compute_min_size();
        let max_size = w.compute_max_size();

        let curr_size = w.rect.size();
        if w.opt.flags.resizable_x() {
            w.rect
                .set_width(curr_size.x.max(min_size.x).min(max_size.x));
        }
        if w.opt.flags.resizable_y() {
            w.rect
                .set_height(curr_size.y.max(min_size.y).min(max_size.y));
        }

        // offset cursor to end of widget
        self.cursor = w.rect.min;
        if let Some(p) = self.widgets.get(&p_id) {
            match p.opt.layout {
                Layout::Vertical => self.cursor.y = rect.max.y + margin.bottom + p.opt.spacing,
                Layout::Horizontal => self.cursor.x = rect.max.x + margin.right + p.opt.spacing,
            }
        }

        id
    }

    // TODO[BUG]: children padding is larger at the bottom
    fn measure_children(&self, w: &Widget) -> Vec2 {
        // TODO[NOTE]: maybe just use bounds?
        let mut total_size = Vec2::ZERO;
        let mut max_size = Vec2::ZERO;
        let mut margins = Vec2::ZERO;
        let n_children = w.n_children;

        for c in self.iter_children(w.id) {
            let size = c.rect.size();
            for axis in [Axis::X, Axis::Y] {
                let a = axis as usize;
                total_size[a] += size[a];
                max_size[a] = max_size[a].max(size[a]);
                margins[a] += c.opt.margin.sum_along_axis(axis);
            }
        }

        let mut result = Vec2::ZERO;
        for axis in [Axis::X, Axis::Y] {
            let content_size = if w.opt.layout.axis() == axis {
                total_size[axis as usize] + (n_children.max(1) - 1) as f32 * w.opt.spacing
            } else {
                max_size[axis as usize]
            };

            result[axis as usize] =
                content_size + margins[axis as usize] + w.opt.padding.sum_along_axis(axis);
        }

        result
    }

    fn build_draw_data(&mut self) {
        for id in &self.draw_order {
            let w = self.widgets.get(id).unwrap();
            self.draw.draw_widget(w.rect, &w.opt);
        }

        if self.draw_dbg_wireframe {
            self.draw.debug_wireframe(2.0);
        }

        self.prev_n_draw_calls = self.draw.vtx_idx_buffer.chunks.len() as u32;
    }

    pub fn mouse_draggin_outside(&self, m: MouseBtn) -> bool {
        let size = self.draw.screen_size;
        let pos = self.mouse.pos;

        pos.x < 0.0 || pos.x > size.x || pos.y < 0.0 || pos.y > size.y
    }
}

pub struct AtlasTexture {
    pub texture: gpu::Texture,
    pub alloc: etagere::BucketedAtlasAllocator,
    pub size: u32,
}

impl AtlasTexture {
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
    has_color: bool,
}

impl fmt::Debug for Glyph<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Glyph")
            // .field("texture", &self.texture)
            .field("meta", &self.meta)
            .finish()
    }
}

pub struct FontAtlas {
    pub textures: Vec<AtlasTexture>,
    pub glyph_cache: HashMap<ctext::CacheKey, GlyphEntry>,
}

impl FontAtlas {
    pub fn new(wgpu: &WGPU) -> Self {
        Self {
            textures: vec![AtlasTexture::new(wgpu)],
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
        // dont cache on the cpu?
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
                unimplemented!()
            }
        };

        let alloc = if let Some(alloc) = self.textures.last_mut()?.allocate(w, h) {
            alloc
        } else {
            let mut texture = AtlasTexture::new(wgpu);
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

fn tessellate_uv_rect(
    rect: Rect,
    uv_min: Vec2,
    uv_max: Vec2,
    tex: u32,
    tint: RGBA,
) -> ([Vertex; 4], [u32; 6]) {
    let tl = Vertex::new(rect.min, tint, uv_min, tex);
    let tr = Vertex::new(
        rect.min.with_x(rect.max.x),
        tint,
        uv_min.with_x(uv_max.x),
        tex,
    );
    let bl = Vertex::new(
        rect.min.with_y(rect.max.y),
        tint,
        uv_min.with_y(uv_max.y),
        tex,
    );
    let br = Vertex::new(rect.max, tint, uv_max, tex);

    ([bl, br, tr, tl], [0, 1, 3, 1, 2, 3])
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

pub fn tessellate_fill(points: &[Vec2], col: RGBA) -> (Vec<Vertex>, Vec<u32>) {
    if points.len() < 3 {
        return (Vec::new(), Vec::new());
    }

    const AA_SIZE: f32 = 1.0;
    const EPS: f32 = 1e-12;
    let col_trans = RGBA::rgba_f(col.r, col.g, col.b, 0.0);
    let n = points.len();

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

    let mut verts = Vec::with_capacity(n * 2);
    let mut idxs = Vec::with_capacity((n - 2) * 3 + n * 6);

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
    pub gpu_vertices: wgpu::Buffer,
    pub gpu_indices: wgpu::Buffer,

    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub vtx_idx_buffer: DrawChunks,
    pub screen_size: Vec2,

    pub path: Vec<Vec2>,
    pub path_closed: bool,

    pub resolution: f32,

    pub font: ctext::FontSystem,
    pub font_icon: ctext::FontSystem,
    pub text_swash_cache: ctext::SwashCache,
    pub font_atlas: FontAtlas,
    pub white_texture: gpu::Texture,
    pub text_cache: TextCache,
    pub text_cache_2: TextCache,

    pub wgpu: WGPUHandle,
}

// fn vtx(pos: impl Into<Vec2>, col: impl Into<RGBA>) -> Vertex {
//     Vertex {
//         pos: pos.into(),
//         col: col.into(),
//     }
// }

impl DrawList {
    /// 2^16
    pub const MAX_VERTEX_COUNT: u64 = 65_536;
    // 2^17
    pub const MAX_INDEX_COUNT: u64 = 131_072;

    pub fn new(wgpu: WGPUHandle) -> Self {
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
            vertices: Vec::new(),
            gpu_vertices,
            gpu_indices,
            indices: Vec::new(),
            screen_size: Vec2::ONE,
            path: Vec::new(),
            path_closed: false,
            resolution: 20.0,
            vtx_idx_buffer: DrawChunks::new(
                Self::MAX_VERTEX_COUNT as usize,
                Self::MAX_INDEX_COUNT as usize,
            ),

            // font: ctext::FontSystem::new(),
            font: ctext::FontSystem::new_with_locale_and_db("en_US".to_owned(), font_db),
            font_icon: ctext::FontSystem::new_with_locale_and_db("en_US".to_owned(), icon_font_db),
            text_swash_cache: ctext::SwashCache::new(),
            font_atlas: FontAtlas::new(&*wgpu),
            white_texture: gpu::Texture::create(&*wgpu, 1, 1, &RGBA::INDIGO.as_bytes()),
            text_cache: TextCache::new(),
            text_cache_2: TextCache::new(),

            wgpu,
        }
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.path_clear();
        self.vtx_idx_buffer.clear();

        std::mem::swap(&mut self.text_cache, &mut self.text_cache_2);
        self.text_cache_2 = TextCache::new();
    }

    pub fn draw_uv_rect(&mut self, rect: Rect, uv_min: Vec2, uv_max: Vec2, tint: RGBA) {
        let (verts, indxs) = tessellate_uv_rect(rect, uv_min, uv_max, 1, tint);
        self.vtx_idx_buffer.push(&verts, &indxs)
    }

    // pub fn draw_text_layout(&mut self, layout: &TextGlyphLayout, pos: Vec2, tint: RGBA) {
    //     for glyph in &layout.glyphs {
    //         let rect = Rect::from_min_size(glyph.pos + pos, glyph.size);

    //         self.draw_uv_rect(rect, glyph.uv_min, glyph.uv_max, tint);
    //     }
    // }

    pub fn register_text(&mut self, text: TextMeta) -> &TextGlyphLayout {
        let text_str = text.string.clone();
        let text_width = text.width();
        let text_height = text.height();
        let font_size = text.font_size();
        // TODO[CHECK]: scaled vs non scaled
        let line_height = text.scaled_line_height();

        if let Some(layout) = self.text_cache.remove(&text) {
            // self.render_text()
            // log::info!("{text_str}: {:#?}", layout.glyphs);
            self.text_cache_2.insert(text.clone(), layout);
            return self.text_cache_2.get(&text).unwrap();
        } else if self.text_cache_2.contains_key(&text) {
            return self.text_cache_2.get(&text).unwrap();
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
            &ctext::Attrs::new()
                .family(ctext::Family::SansSerif)
                .weight(ctext::Weight(800)),
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
                // TODO[CHECK]: what does this do
                phys.cache_key.x_bin = ctext::SubpixelBin::Three;
                phys.cache_key.y_bin = ctext::SubpixelBin::Three;

                if let Some(glyph) = self.font_atlas.get_glyph(
                    phys.cache_key,
                    &mut self.font,
                    &mut self.text_swash_cache,
                    &self.wgpu,
                ) {
                    // TODO[NOTE]: add DPI
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
        // width += 0.1;
        // height += 0.1;
        log::trace!("register text: {text_str}");
        let layout = TextGlyphLayout {
            glyphs,
            width,
            height,
        };
        self.text_cache_2.insert(text.clone(), layout);
        self.text_cache_2.get(&text).unwrap()
    }

    pub fn draw_text(&mut self, text: TextMeta, pos: Vec2, color: RGBA) {
        let layout = self.register_text(text).clone();

        for glyph in &layout.glyphs {
            let rect = Rect::from_min_size(glyph.pos + pos, glyph.size);

            self.draw_uv_rect(rect, glyph.uv_min, glyph.uv_max, color);
        }
    }

    pub fn measure_text_size(&mut self, text: TextMeta) -> Vec2 {
        if text.string.is_empty() {
            return Vec2::ZERO;
        }
        let layout = self.register_text(text);
        Vec2::new(layout.width, layout.height)
    }

    pub fn draw_widget(&mut self, rect: Rect, opt: &WidgetOpt) {
        self.path_rect(rect.min, rect.max, opt.corner_radius);

        if opt.flags.contains(WidgetFlags::DRAW_FILL) {
            let (vtx, idx) = tessellate_fill(&self.path, opt.fill_color);
            self.vtx_idx_buffer.push(&vtx, &idx);
        }

        if opt.tex_id != 0 {
            self.draw_uv_rect(rect, Vec2::ZERO, Vec2::splat(1.0), RGBA::WHITE);
        }

        if opt.flags.contains(WidgetFlags::DRAW_TEXT) {
            // self.draw_uv_rect(rect, Vec2::ZERO, Vec2::splat(1.0));
            // let text = TextMeta::new(opt.text.clone().unwrap_or("".into()), opt.font_size, 0.0);
            let pad = Vec2::new(opt.padding.left, opt.padding.top);
            self.draw_text(opt.text_meta(), rect.min + pad, opt.text_color)
        }

        if opt.flags.contains(WidgetFlags::DRAW_OUTLINE) {
            self.path_clear();
            self.path_rect(rect.min, rect.max, opt.corner_radius);
            let (vtx, idx) =
                tessellate_line(&self.path, opt.outline_color, opt.outline_width, true);
            self.vtx_idx_buffer.push(&vtx, &idx);
        }

        self.path_clear();
    }

    pub fn add_rect(&mut self, dr: DrawRect) {
        self.path_rect(dr.rect.min, dr.rect.max, dr.corner_radius);

        if let Some(fill) = dr.fill {
            let (vtx, idx) = tessellate_fill(&self.path, fill);
            self.vtx_idx_buffer.push(&vtx, &idx)
            // self.draw_memory.push(&vtx, &idx);
            // let off = self.vertices.len() as u32;

            // self.vertices.extend(&vtx);
            // self.indices.extend(&idx.into_iter().map(|i| i + off));
        }

        if let Some((col, width)) = dr.outline {
            self.path_clear();
            self.path_rect(dr.rect.min, dr.rect.max, dr.corner_radius);
            let (vtx, idx) = tessellate_line(&self.path, col, width, true);
            // self.draw_memory.push(&vtx, &idx);
            self.vtx_idx_buffer.push(&vtx, &idx)
            // let off = self.vertices.len() as u32;
            // self.vertices.extend(&vtx);
            // self.indices.extend(&idx.into_iter().map(|i| i + off));
        }

        self.path_clear();
    }

    // Here
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
        let (mut vtx, idx) = tessellate_line(&self.path, cols[0], thickness, self.path_closed);
        // let offset = self.vertices.len() as u32;

        vtx.iter_mut().enumerate().for_each(|(i, v)| {
            v.col = cols[i % cols.len()];
        });

        self.vtx_idx_buffer.push(&vtx, &idx);
        // self.draw_memory.push(&vtx, &idx);

        // self.vertices
        //     .extend(vtx.into_iter().enumerate().map(|(i, mut v)| {
        //         v.col = cols[i % cols.len()];
        //         v
        //     }));
        // self.indices.extend(idx.into_iter().map(|i| i + offset));

        self.path_clear();
    }

    pub fn build_path_stroke(&mut self, thickness: f32, col: RGBA) {
        let (mut vtx, idx) = tessellate_line(&self.path, col, thickness, self.path_closed);
        let offset = self.vertices.len() as u32;
        vtx.iter_mut().for_each(|v| {
            v.col = col;
        });
        self.vtx_idx_buffer.push(&vtx, &idx);
        // self.draw_memory.push(&vtx, &idx);
        // self.vertices.extend(vtx.into_iter().map(|mut v| {
        //     v.col = col;
        //     v
        // }));
        // self.indices.extend(idx.into_iter().map(|i| i + offset));
        self.path_clear();
    }

    pub fn debug_wireframe(&mut self, thickness: f32) {
        self.path_clear();

        let memory = self.vtx_idx_buffer.clone();
        self.vtx_idx_buffer.clear();

        for i in 0..memory.chunks.len() {
            let (v, i) = memory.get_chunk_data(i).unwrap();

            for idxs in i.chunks_exact(3) {
                let v0 = v[idxs[0] as usize];
                let v1 = v[idxs[1] as usize];
                let v2 = v[idxs[2] as usize];
                let cols = [v0.col, v1.col, v2.col, v0.col];
                self.path
                    .extend_from_slice(&[v0.pos, v1.pos, v2.pos, v0.pos]);
                self.build_path_stroke_multi_color(thickness, &cols);
            }
        }
    }
}

impl gpu::RenderPassHandle for DrawList {
    const LABEL: &'static str = "draw_list_render_pass";

    fn n_render_passes(&self) -> u32 {
        self.vtx_idx_buffer.chunks.len() as u32
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        self.draw_multiple(rpass, wgpu, 0);
    }

    fn draw_multiple<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU, i: u32) {
        let proj = Mat4::orthographic_lh(
                0.0,
                self.screen_size.x,
                self.screen_size.y,
                0.0,
                -1.0,
                1.0,
            );

        let global_uniform = GlobalUniform::new(self.screen_size, proj);

        let bind_group = build_bind_group(
            global_uniform,
            self.font_atlas.textures.last().unwrap().texture.view(),
            wgpu,
        );

        let (verts, indxs) = self.vtx_idx_buffer.get_chunk_data(i as usize).unwrap();

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
                // @location(0) @interpolate(flat) color: vec4<f32>,
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
pub struct DrawChunks {
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

impl DrawChunks {
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
