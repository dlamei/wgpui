pub mod app;
mod gpu;
mod mouse;
mod rect;
mod ui;
mod ui2;
mod ui_draw;
mod utils;

use std::sync::Arc;

use glam::Vec4;
use gpu::{VertexDesc, WGPU};
use macros::vertex;
use utils::RGBA;
use wgpu::util::DeviceExt;

pub extern crate self as wgpui;

pub use gpu::AsVertexFormat;
pub use gpu::Vertex;

#[vertex]
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
