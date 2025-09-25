use glam::{Mat4, UVec2, UVec4, Vec2, Vec4};
use macros::vertex;
use wgpu::util::DeviceExt;

use std::{
    cell::RefCell,
    collections::VecDeque,
    fmt,
    hash::{Hash, Hasher},
    ops,
    sync::Arc,
};

use crate::{
    RGBA, ctext,
    gpu::{self, ShaderHandle, Vertex as VertexTyp, VertexDesc, WGPU, WGPUHandle, Window},
    mouse::{CursorIcon, MouseBtn, MouseRec, MouseState},
    rect::Rect,
    ui_draw::{DrawList, TextMeta},
    utils::{Duration, HashMap, Instant},
};

macro_rules! sig_bits {
    ($n:literal) => { 1 << $n };
    ($i:ident) => { Signals::$i.bits() };
    ($($x:tt)|+) => {
        $(sig_bits!($x) | )* 0
    }
}

use bitflags::bitflags as flags;

flags! {
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

impl<T> From<[T; 2]> for PerAxis<T> {
    fn from(value: [T; 2]) -> Self {
        Self(value)
    }
}

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
        let mut hasher = ahash::AHasher::default();
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

flags! {
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
        let mut min = self.rect.min;
        let mut max = self.rect.max;
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
    TopLeft,
    Center,
}

pub struct State {
    pub mouse: MouseState,
    pub frame_count: u64,

    pub widgets: HashMap<WidgetId, Widget>,
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
            next_widget_placement: PerAxis([Placement::TopLeft; 2]),
            roots: Vec::new(),
            curr_widget_action: WidgetAction::None,
            hot_id: WidgetId::NULL,
            active_id: WidgetId::NULL,
            mouse: MouseState::new(),
            frame_count: 0,
            widgets: HashMap::default(),
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

        let mut resize_dir = None;
        if !self.window.is_maximized() {
            resize_dir = is_in_resize_region(w_rect, self.mouse.pos, self.resize_threshold);
        }

        let lft_btn = button == MouseBtn::Left;

        if self.window.is_decorated() {
            return;
        }

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
            let (win_id, _) = self.begin_widget(
                title,
                WidgetOpt::new()
                    .fill(bg_col)
                    .size_px(win_size.x as f32, win_size.y as f32)
                    .padding(padding),
            );
            self[win_id].rect = Rect::from_min_size(Vec2::ZERO, win_size);
            let win_rect = self[win_id].rect;
            return;
        }

        self.set_cursor(0.0, 0.0);
        let (id, _) = self.begin_widget(
            "window bar#",
            WidgetOpt::new()
                .size_px(win_size.x, self.custom_tab_height)
                .fill(tab_col)
                .layout_h()
                .padding_dir(Padding::new(0.0, 0.0, 5.0, 5.0))
                .spacing(10.0),
        );
        self[id].rect =
            Rect::from_min_size(Vec2::ZERO, Vec2::new(win_size.x, self.custom_tab_height));

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
                    .layout_h(),
            );

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

        let (win_id, _) = self.begin_widget(
            title,
            WidgetOpt::new()
                .fill(RGBA::RED)
                .size_px(win_size.x as f32, win_size.y as f32)
                .padding(padding),
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

    pub fn add_debug_window(&mut self, dt: Duration) {
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
            "mouse",
            WidgetOpt::new()
                .text(&format!("pos: {}", self.mouse.pos), 32.0)
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

        // self.mouse.clear_released();
        self.mouse.end_frame();

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
            let mut hasher = ahash::AHasher::default();
            p_id.hash(&mut hasher);
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
    }

    // TODO[NOTE]: moving a widget leads to its children being a frame behind
    fn handle_widget_action(&mut self) {
        let m_start = self.mouse.drag_start(MouseBtn::Left).unwrap_or(Vec2::NAN);
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

            if self.mouse.released(MouseBtn::Left) {
                signal |= Signals::RELEASED_LEFT
            }
            if self.mouse.released(MouseBtn::Right) {
                signal |= Signals::RELEASED_RIGHT
            }
            if self.mouse.released(MouseBtn::Middle) {
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

        let mut rect = Rect::from_min_size(self.cursor, widget_size);
        let off_x = match self.next_widget_placement.x() {
            Placement::TopLeft => 0.0,
            Placement::Center => -widget_size.x / 2.0,
        };
        let off_y = match self.next_widget_placement.y() {
            Placement::TopLeft => 0.0,
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
        self.next_widget_placement = PerAxis([Placement::TopLeft; 2]);

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
            self.draw.as_wireframe(2.0);
        }

        self.prev_n_draw_calls = self.draw.draw_buffer.chunks.len() as u32;
    }

    pub fn mouse_draggin_outside(&self, m: MouseBtn) -> bool {
        let size = self.draw.screen_size;
        let pos = self.mouse.pos;

        pos.x < 0.0 || pos.x > size.x || pos.y < 0.0 || pos.y > size.y
    }
}
