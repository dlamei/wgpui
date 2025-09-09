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

pub struct DbgTriangle {
    vertex_buffer: wgpu::Buffer,
    color: RGBA,
}

impl DbgTriangle {
    pub fn new(color: RGBA, wgpu: &WGPU) -> Self {
        let vertices = [
            VertexPosCol {
                pos: [-0.5, -0.5, 0.0, 1.0].into(),
                col: RGBA::RED,
            },
            VertexPosCol {
                pos: [0.0, 0.5, 0.0, 1.0].into(),
                col: RGBA::GREEN, // green
            },
            VertexPosCol {
                pos: [0.5, -0.25, 0.0, 1.0].into(),
                col: RGBA::BLUE, // blue
            },
        ];

        let vertex_buffer = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("debug_triangle_vertex_buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        Self {
            vertex_buffer,
            color,
        }
    }
}

impl RenderPassHandle for DbgTriangle {
    fn load_op(&self) -> wgpu::LoadOp<wgpu::Color> {
        wgpu::LoadOp::Load
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        let col = ColorTint(self.color);
        // rpass.set_pipeline(&col.get_pipeline(wgpu));
        // rpass.set_pipeline(&col.get_vertex_pipeline::<ui::VertexRect>(wgpu));
        rpass.set_pipeline(&col.get_pipeline(&[(&VertexPosCol::desc(), "Vertex")], wgpu));
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.draw(0..3, 0..1);
    }
}

#[derive(Debug, Clone)]
pub struct ClearScreen(pub RGBA);

impl RenderPassHandle for ClearScreen {
    fn load_op(&self) -> wgpu::LoadOp<wgpu::Color> {
        wgpu::LoadOp::Clear(self.0.into())
    }

    fn store_op(&self) -> wgpu::StoreOp {
        wgpu::StoreOp::Store
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {}
}

pub type ShaderID = &'static str;

pub struct ColorTint(pub RGBA);

impl ShaderHandle for ColorTint {
    const RENDER_PIPELINE_ID: ShaderID = "color_tint";

    fn build_pipeline(&self, desc: &ShaderGenerics<'_>, wgpu: &WGPU) -> wgpu::RenderPipeline {
        const SHADER_SRC: &str = r#"
            struct VSOut {
                @builtin(position) pos: vec4<f32>,
                @location(0) color: vec4<f32>,
            };

            @rust struct Vertex {
                pos: vec4<f32>,
                col: vec4<f32>,
                ...
            }

            @vertex
                fn vs_main(
                    v: Vertex
                ) -> VSOut {
                    var out: VSOut;
                    out.pos = v.pos;
                    out.color = v.col * $COLOR;
                    return out;
                }

            @fragment
                fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
                    return in.color;
                }
            "#;
        let shader_src = SHADER_SRC.replace("$COLOR", &self.0.as_wgsl_vec4());

        // let prcs = V::process_shader_code(&shader_src, "Vertex");
        let src = match gpu::process_shader_code(&shader_src, desc) {
            Ok(prcs_src) => prcs_src,
            Err(err) => {
                log::error!("could not process shader: {err}");
                panic!();
            }
        };

        let vertices = desc.iter().map(|d| d.0).collect::<Vec<_>>();
        gpu::PipelineBuilder::new(&src, wgpu.surface_format)
            .label("color_tint_triangle_pipeline")
            .vertex_buffers(&vertices)
            .build(&wgpu.device)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UUID(pub u64);

pub type ShaderGenerics<'a> = [(&'a VertexDesc, &'a str)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShaderTyp {
    Vertex,
    Instance,
    Uniform,
}

pub trait ShaderHandle {
    const RENDER_PIPELINE_ID: ShaderID;
    fn build_pipeline(&self, desc: &ShaderGenerics<'_>, wgpu: &WGPU) -> wgpu::RenderPipeline;

    fn pipeline_generic_id() -> UUID {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        Self::RENDER_PIPELINE_ID.hash(&mut hasher);
        UUID(hasher.finish())
    }

    fn pipeline_vertex_id(desc: &ShaderGenerics<'_>) -> UUID {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        Self::RENDER_PIPELINE_ID.hash(&mut hasher);
        for (d, _) in desc {
            d.attributes.hash(&mut hasher);
            d.members.hash(&mut hasher);
        }
        UUID(hasher.finish())
    }

    fn should_rebuild(&self) -> bool {
        false
    }

    fn try_rebuild(&self, desc: &ShaderGenerics<'_>, wgpu: &WGPU) {
        log::info!(
            "[pipeline] {}: rebuild for vertex ({:?})",
            Self::RENDER_PIPELINE_ID,
            desc.iter().map(|d| d.0.label).collect::<Vec<_>>(),
        );
        wgpu.register_pipeline(
            Self::pipeline_vertex_id(desc),
            self.build_pipeline(desc, wgpu),
        );
    }

    fn get_pipeline(&self, desc: &ShaderGenerics<'_>, wgpu: &WGPU) -> Arc<wgpu::RenderPipeline> {
        if self.should_rebuild() {
            self.try_rebuild(desc, wgpu);
        }

        wgpu.get_or_init_pipeline(Self::pipeline_vertex_id(desc), || {
            log::info!(
                "[pipeline] {}: build for vertex ({:?})",
                Self::RENDER_PIPELINE_ID,
                desc.iter().map(|d| d.0.label).collect::<Vec<_>>(),
            );
            self.build_pipeline(desc, wgpu)
        })
    }

    // fn get_pipeline_directly(&self, wgpu: &WGPU) -> Arc<wgpu::RenderPipeline> {
    //     if self.should_rebuild() {
    //         wgpu.register_pipeline(Self::pipeline_generic_id(), self.build_pipeline::<V>(wgpu))
    //     }
    //     wgpu.get_or_init_pipeline(Self::pipeline_generic_id(), || {
    //         self.build_pipeline::<V>(wgpu)
    //     })
    // }
}

pub trait RenderPassHandle {
    fn load_op(&self) -> wgpu::LoadOp<wgpu::Color> {
        wgpu::LoadOp::Load
    }
    fn store_op(&self) -> wgpu::StoreOp {
        wgpu::StoreOp::Store
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU);
}

pub struct RenderTarget<'a> {
    target_view: wgpu::TextureView,
    resolve_view: Option<wgpu::TextureView>,
    encoder: std::mem::ManuallyDrop<wgpu::CommandEncoder>,
    wgpu: &'a WGPU,
}

impl<'a> Drop for RenderTarget<'a> {
    fn drop(&mut self) {
        unsafe {
            let encoder = std::mem::ManuallyDrop::take(&mut self.encoder);
            self.wgpu.queue.submit(Some(encoder.finish()));
        }
    }
}

impl<'a> RenderTarget<'a> {
    pub fn render<RH: RenderPassHandle>(&mut self, rh: &RH) {
        let mut rpass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.target_view,
                resolve_target: self.resolve_view.as_ref(),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: rh.load_op(),
                    store: rh.store_op(),
                },
            })],
            depth_stencil_attachment: None,
            label: Some("main render pass"),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        rh.draw(&mut rpass, self.wgpu);
    }
}
