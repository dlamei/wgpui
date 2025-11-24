pub mod app;
mod core;
mod gpu;
mod mouse;
pub mod rect;
mod ui;
mod ui_context;
mod ui_items;
mod ui_panel;

use std::sync::Arc;

use core::RGBA;
use glam::Vec4;
use gpu::{VertexDesc, WGPU};
use wgpu::util::DeviceExt;

pub extern crate self as wgpui;

pub use gpu::AsVertexFormat;
pub use gpu::Vertex;

#[macros::vertex]
pub struct VertexPosCol {
    pub pos: Vec4,
    pub col: RGBA,
}

pub(crate) use cosmic_text as ctext;

macro_rules! build {
    ($constructor:expr;  { $(. $field:ident = $value:expr;)* }) => {{
        let mut obj = $constructor;
        $(
            obj.$field = $value;
        )*
        obj
    }};
}
pub(crate) use build;
