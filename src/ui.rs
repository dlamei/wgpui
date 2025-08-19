use glam::{UVec2, Vec2, Vec3, Vec4};
use rustc_hash::FxHashMap;

use std::{
    fmt,
    hash::{Hash, Hasher},
    ops,
};

use macros::vertex_struct;

use crate::{
    RGBA,
    rect::{self, Rect},
    utils::RGB,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u64);

impl NodeId {
    pub const NULL: NodeId = NodeId(0);

    pub fn from_str(s: &str) -> Self {
        let mut hasher = rustc_hash::FxHasher::default();
        s.hash(&mut hasher);
        Self(hasher.finish().max(1))
    }

    pub fn child(self, child: &str) -> Self {
        let mut hasher = rustc_hash::FxHasher::default();
        self.0.hash(&mut hasher);
        child.hash(&mut hasher);
        Self(hasher.finish().max(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    X = 0,
    Y = 1,
}

pub enum MouseButton {
    Left,
    Right,
    Middle,
}

pub enum SizeUnit {
    Null,
    Pixels(f32),
    Text,
    Percent(f32),
    ChildrenSum,
}

pub struct Size {
    val: SizeUnit,
    strictness: f32,
}

pub struct Node {
    id: NodeId,

    first: NodeId,
    last: NodeId,
    next: NodeId,
    prev: NodeId,
    parent: NodeId,

    last_frame_used: u64,

    fixed_pos: Vec2,
    fixed_size: Vec2,
    min_size: Vec2,

    pref_size: [Size; 2],
    child_layout_axis: Axis,

    comp_rel_pos: Vec2,
    comp_size: Vec2,
    rect: Rect,

    background_col: RGBA,
}

macro_rules! sig_bits {
    ($n:literal) => { 1 << $n };
    ($i:ident) => { SignalFlags::$i.bits() };
    ($($x:tt)|+) => {
        $(sig_bits!($x) | )* 0
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct SignalFlags: u32 {
        const PRESS_L = 1 << 0;
        const PRESS_M = 1 << 1;
        const PRESS_R = 1 << 2;

        const DRAG_L = 1 << 3;
        const DRAG_M = 1 << 4;
        const DRAG_R = 1 << 5;

        const DOUBLE_DRAG_L = 1 << 6;
        const DOUBLE_DRAG_M = 1 << 7;
        const DOUBLE_DRAG_R = 1 << 8;

        const RELEASE_L = 1 << 9;
        const RELEASE_M = 1 << 10;
        const RELEASE_R = 1 << 11;

        const CLICK_L = 1 << 12;
        const CLICK_M = 1 << 13;
        const CLICK_R = 1 << 14;

        const DOUBLE_CLICK_L = 1 << 15;
        const DOUBLE_CLICK_M = 1 << 16;
        const DOUBLE_CLICK_R = 1 << 17;

        const HOVERING = 1 << 18;
        const MOUSE_OVER = 1 << 19; // may be occluded

        const PRESS_KEYBOARD = 1 << 20;

        // const PRESS = sig_bit!(PRESS_L | PRESS_KEYBOARD);
        // const RELEASE = sig_bit!(RELEASE_L);
        // const CLICK = sig_bit!(CLICK_L | PRESS_KEYBOARD);
        // const DOUBLE_CLICK = sig_bit!(DOUBLE_CLICK_L);
        // const DRAG = sig_bit!(DRAG_L);
    }
}

macro_rules! sig_fn {
    ($fn_name:ident => $($x:tt)*) => {
        impl SignalFlags {
            pub const fn $fn_name(&self) -> bool {
                let flag = SignalFlags::from_bits(sig_bits!($($x)*)).unwrap();
                self.contains(flag)
            }
        }
    }
}

sig_fn!(pressed => PRESS_L | PRESS_KEYBOARD);
sig_fn!(clicked => CLICK_L | PRESS_KEYBOARD);
sig_fn!(double_clicked => DOUBLE_CLICK_L);
sig_fn!(dragging => DRAG_L);
sig_fn!(released => RELEASE_L);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MouseState {
    pub left: bool,
    pub middle: bool,
    pub right: bool,
}

pub struct State {
    pub mouse_pos: Vec2,
    pub mouse_press: MouseState,
    pub mouse_drag_start: Option<Vec2>,

    pub root: NodeId,

    pub hot_node: NodeId,
    pub active_node: NodeId,

    pub cached_nodes: FxHashMap<NodeId, Node>,
}

vertex_struct!(RectInst {
    top_left(0): Vec2,
    size(1): Vec2,
});
