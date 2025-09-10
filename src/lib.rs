pub mod app;
mod gpu;
mod mouse;
mod rect;
mod ui;
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

// pub struct DbgTriangle {
//     vertex_buffer: wgpu::Buffer,
//     color: RGBA,
// }

// impl DbgTriangle {
//     pub fn new(color: RGBA, wgpu: &WGPU) -> Self {
//         let vertices = [
//             VertexPosCol {
//                 pos: [-0.5, -0.5, 0.0, 1.0].into(),
//                 col: RGBA::RED,
//             },
//             VertexPosCol {
//                 pos: [0.0, 0.5, 0.0, 1.0].into(),
//                 col: RGBA::GREEN, // green
//             },
//             VertexPosCol {
//                 pos: [0.5, -0.25, 0.0, 1.0].into(),
//                 col: RGBA::BLUE, // blue
//             },
//         ];

//         let vertex_buffer = wgpu
//             .device
//             .create_buffer_init(&wgpu::util::BufferInitDescriptor {
//                 label: Some("debug_triangle_vertex_buffer"),
//                 contents: bytemuck::cast_slice(&vertices),
//                 usage: wgpu::BufferUsages::VERTEX,
//             });

//         Self {
//             vertex_buffer,
//             color,
//         }
//     }
// }

// impl RenderPassHandle for DbgTriangle {
//     fn load_op(&self) -> wgpu::LoadOp<wgpu::Color> {
//         wgpu::LoadOp::Load
//     }

//     fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
//         let col = ColorTint(self.color);
//         // rpass.set_pipeline(&col.get_pipeline(wgpu));
//         // rpass.set_pipeline(&col.get_vertex_pipeline::<ui::VertexRect>(wgpu));
//         rpass.set_pipeline(&col.get_pipeline(&[(&VertexPosCol::desc(), "Vertex")], wgpu));
//         rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
//         rpass.draw(0..3, 0..1);
//     }
// }

#[derive(Debug, Clone)]
pub struct ClearScreen(pub RGBA);

impl gpu::RenderPassHandle for ClearScreen {
    fn load_op(&self) -> wgpu::LoadOp<wgpu::Color> {
        wgpu::LoadOp::Clear(self.0.into())
    }

    fn store_op(&self) -> wgpu::StoreOp {
        wgpu::StoreOp::Store
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {}
}
