mod gpu;
mod rect;
mod ui;
mod utils;

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use glam::{UVec2, Vec2, Vec3, Vec4};
use gpu::{ResourceCache, WGPU};
use utils::RGBA;
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    window::Window,
};

pub use gpu::Vertex;
pub use macros::vertex_struct;

pub extern crate self as wgpui;

pub use gpu::AsVertexFormat;

pub enum AppSetup {
    UnInit {
        window: Option<Arc<Window>>,
        #[cfg(target_arch = "wasm32")]
        renderer_rec: Option<futures::channel::oneshot::Receiver<Renderer>>,
    },
    Init(App),
}

impl Default for AppSetup {
    fn default() -> Self {
        Self::UnInit {
            window: None,
            #[cfg(target_arch = "wasm32")]
            renderer_rec: None,
        }
    }
}

impl AppSetup {
    pub fn is_init(&self) -> bool {
        matches!(self, Self::Init(_))
    }

    pub fn init_app(window: Arc<Window>, renderer: Renderer) -> App {
        let scale_factor = window.scale_factor() as f32;

        let wgpu = &renderer.wgpu;

        App {
            renderer,
            window,
            last_size: UVec2::ONE,
            prev_frame_time: Instant::now(),
            delta_time: Duration::ZERO,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn resumed_native(&mut self, event_loop: &ActiveEventLoop) {
        if self.is_init() {
            return;
        }

        let window = event_loop
            .create_window(winit::window::Window::default_attributes().with_title("Atlas"))
            .unwrap();

        let window_handle = Arc::new(window);
        // self.window = Some(window_handle.clone());

        let size = window_handle.inner_size();
        let scale_factor = window_handle.scale_factor() as f32;

        let window_handle_2 = window_handle.clone();
        let renderer = pollster::block_on(async move {
            Renderer::new_async(window_handle_2, size.width, size.height).await
        });

        *self = Self::Init(Self::init_app(window_handle, renderer));
    }

    fn try_init(&mut self) -> Option<&mut App> {
        if let Self::Init(app) = self {
            return Some(app);
        }

        #[cfg(target_arch = "wasm32")]
        {
            let Self::UnInit {
                window,
                renderer_rec,
            } = self
            else {
                panic!();
            };
            // let mut renderer_received = false;
            if let Some(receiver) = renderer_rec.as_mut() {
                if let Ok(Some(renderer)) = receiver.try_recv() {
                    *self = Self::Init(Self::init_app(window.as_ref().unwrap().clone(), renderer));
                    if let Self::Init(app) = self {
                        return Some(app);
                    }
                }
            }
        }

        None
    }
}

impl ApplicationHandler for AppSetup {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_arch = "wasm32"))]
        self.resumed_native(event_loop);
        #[cfg(target_arch = "wasm32")]
        self.resumed_wasm(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let Some(app) = self.try_init() {
            app.on_window_event(event_loop, window_id, event);
        }
    }

    // fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    //         println!("waiting... ");
    //     if let Some(app) = self.try_init() {
    //         app.window.request_redraw();
    //     }
    // }
}

pub struct App {
    renderer: Renderer,

    prev_frame_time: Instant,
    delta_time: Duration,

    last_size: UVec2,
    window: Arc<Window>,
}

impl App {
    fn on_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        use WindowEvent as WE;
        if self.window.id() != window_id {
            return;
        }

        self.on_update();

        match event {
            WE::RedrawRequested => {
                self.on_redraw(event_loop);
            }
            WE::KeyboardInput { event, .. } => {
                self.on_keyboard(&event, event_loop);
            }
            WE::Resized(PhysicalSize { width, height }) => {
                let (width, height) = (width.max(1), height.max(1));
                self.last_size = (width, height).into();
                self.resize(width, height);
            }
            WE::CloseRequested => event_loop.exit(),
            _ => (),
        }
    }

    fn on_update(&mut self) {
        // println!("{:#?}", self.renderer.wgpu.pipeline_cache);
    }

    fn on_keyboard(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                event_loop.exit();
            }
            PhysicalKey::Code(KeyCode::KeyR) => {
                let shader = ColorTint(RGBA::rand());
                shader.try_rebuild::<VertexPosCol>(&self.renderer.wgpu);
            }
            _ => (),
        }
    }

    fn on_redraw(&mut self, event_loop: &ActiveEventLoop) {
        let prev_time = self.prev_frame_time;
        let curr_time = Instant::now();
        let dt = curr_time - prev_time;
        self.prev_frame_time = curr_time;
        self.delta_time = dt;

        self.window.pre_present_notify();
        let status = self.renderer.prepare_frame();
        match status {
            Ok(_) => (),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let size = self.window.inner_size();
                self.renderer.resize(size.width, size.height);
                return;
            }
            Err(e) => {
                log::error!("prepare_frame: {e}");
                panic!();
            }
        }

        {
            let clear_screen = ClearScreen("#242933".into());
            let dbg_tri = DbgTriangle::new((255, 50, 50).into(), &self.renderer.wgpu);

            let mut surface = self.renderer.surface_target();
            surface.render(&clear_screen);
            surface.render(&dbg_tri);
        }
        self.renderer.present_frame();
        self.window.request_redraw();
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.renderer.resize(w, h);
    }
}

vertex_struct!(VertexPosCol {
    pos(0): Vec4,
    col(1): RGBA,
});

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
        rpass.set_pipeline(&col.get_vertex_pipeline::<VertexPosCol>(wgpu));
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

    fn build_pipeline<V: Vertex>(&self, wgpu: &WGPU) -> wgpu::RenderPipeline {
        log::trace!(
            "[pipeline] {}: build with color: {}",
            Self::RENDER_PIPELINE_ID,
            self.0
        );
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
                    // out.pos = vec4<f32>(position, 1.0);
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

        let prcs = V::process_shader_code(&shader_src, "Vertex");
        let src = match &prcs {
            Ok(prcs_src) => prcs_src,
            Err(err) => {
                log::error!("could not process shader: {err}");
                panic!();
            }
        };

        gpu::PipelineBuilder::new(&src, wgpu.surface_format)
            .label("debug_triangle_pipeline")
            .vertex_buffers(&[V::buffer_layout()])
            .build(&wgpu.device)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UUID(pub u64);

pub trait ShaderHandle {
    const RENDER_PIPELINE_ID: ShaderID;
    fn build_pipeline<V: Vertex>(&self, wgpu: &WGPU) -> wgpu::RenderPipeline;

    fn pipeline_generic_id() -> UUID {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        Self::RENDER_PIPELINE_ID.hash(&mut hasher);
        UUID(hasher.finish())
    }

    fn pipeline_vertex_id<V: Vertex>() -> UUID {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        Self::RENDER_PIPELINE_ID.hash(&mut hasher);
        V::VERTEX_ATTRIBUTES.hash(&mut hasher);
        V::VERTEX_MEMBERS.hash(&mut hasher);
        UUID(hasher.finish())
    }

    fn should_rebuild(&self) -> bool {
        false
    }

    fn try_rebuild<V: Vertex>(&self, wgpu: &WGPU) {
        log::info!(
            "[pipeline] {}: rebuild for vertex ({})",
            Self::RENDER_PIPELINE_ID,
            V::VERTEX_LABEL
        );
        wgpu.register_pipeline(
            Self::pipeline_vertex_id::<V>(),
            self.build_pipeline::<V>(wgpu),
        );
    }

    fn get_vertex_pipeline<V: Vertex>(&self, wgpu: &WGPU) -> Arc<wgpu::RenderPipeline> {
        if self.should_rebuild() {
            self.try_rebuild::<V>(wgpu);
            // wgpu.register_pipeline(Self::pipeline_vertex_id::<V>(), self.build_pipeline::<V>(wgpu))
        }
        wgpu.get_or_init_pipeline(Self::pipeline_vertex_id::<V>(), || {
            log::info!(
                "[pipeline] {}: build for vertex ({})",
                Self::RENDER_PIPELINE_ID,
                V::VERTEX_LABEL
            );
            self.build_pipeline::<V>(wgpu)
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

pub struct Renderer {
    framebuffer_msaa: Option<wgpu::TextureView>,
    framebuffer_resolve: wgpu::TextureView,
    depthbuffer: wgpu::TextureView,
    active_surface: Option<wgpu::SurfaceTexture>,
    wgpu: WGPU,
}

pub struct RenderTarget<'a> {
    target_view: wgpu::TextureView,
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
                resolve_target: None,
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

impl Renderer {
    pub fn surface_target(&mut self) -> RenderTarget<'_> {
        let Some(surface_texture) = &mut self.active_surface else {
            log::error!("Renderer::prepare_frame must be called before calling this function");
            panic!();
        };

        let surface_texture_view =
            surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor {
                    label: wgpu::Label::default(),
                    aspect: wgpu::TextureAspect::default(),
                    format: Some(self.wgpu.surface_format),
                    dimension: None,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                    usage: None,
                });

        let encoder = self
            .wgpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("renderpass_encoder"),
            });

        RenderTarget {
            target_view: surface_texture_view,
            encoder: std::mem::ManuallyDrop::new(encoder),
            wgpu: &self.wgpu,
        }
    }

    pub fn prepare_frame(&mut self) -> Result<(), wgpu::SurfaceError> {
        if self.active_surface.is_some() {
            log::error!("Renderer::prepare_frame called with active surface!");
            panic!();
        }

        let surface_texture = self.wgpu.surface.get_current_texture()?;

        self.active_surface = Some(surface_texture);
        Ok(())
    }

    pub fn present_frame(&mut self) {
        if let Some(surface) = self.active_surface.take() {
            surface.present();
            self.active_surface = None;
        }
    }

    pub async fn new_async(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Self {
        let wgpu = WGPU::new_async(window, width, height).await;

        let framebuffer_msaa = Self::create_framebuffer_msaa_texture(&wgpu, width, height);
        let framebuffer_resolve = Self::create_framebuffer_resolve_texture(&wgpu, width, height);
        let depthbuffer = Self::create_depthbuffer(&wgpu, width, height);

        Self {
            framebuffer_msaa,
            framebuffer_resolve,
            depthbuffer,
            active_surface: None,
            wgpu,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.wgpu.resize(width, height);
        self.framebuffer_msaa = Self::create_framebuffer_msaa_texture(&self.wgpu, width, height);
        self.framebuffer_resolve =
            Self::create_framebuffer_resolve_texture(&self.wgpu, width, height);
        self.depthbuffer = Self::create_depthbuffer(&self.wgpu, width, height);
    }

    pub fn create_framebuffer_resolve_texture(
        wgpu: &WGPU,
        width: u32,
        height: u32,
    ) -> wgpu::TextureView {
        let width = width.max(1);
        let height = height.max(1);
        let texture = wgpu.device.create_texture(
            &(wgpu::TextureDescriptor {
                label: Some("Framebuffer Resolve Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }),
        );
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: Some(wgpu.surface_format),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            base_array_layer: 0,
            array_layer_count: None,
            mip_level_count: None,
            usage: None,
        })
    }

    pub fn depth_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::Depth32Float
    }

    pub const fn use_multisample() -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return true;
        #[cfg(target_arch = "wasm32")]
        return false;
    }

    pub fn multisample_state() -> wgpu::MultisampleState {
        if Self::use_multisample() {
            wgpu::MultisampleState {
                mask: !0,
                alpha_to_coverage_enabled: false,
                count: 4,
            }
        } else {
            Default::default()
        }
    }

    pub fn create_framebuffer_msaa_texture(
        wgpu: &WGPU,
        width: u32,
        height: u32,
    ) -> Option<wgpu::TextureView> {
        let width = width.max(1);
        let height = height.max(1);
        if !Self::use_multisample() {
            return None;
        }

        let texture = wgpu.device.create_texture(
            &(wgpu::TextureDescriptor {
                label: Some("Framebuffer Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 4,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }),
        );
        Some(texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: Some(wgpu.surface_format),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            base_array_layer: 0,
            array_layer_count: None,
            mip_level_count: None,
            usage: None,
        }))
    }

    pub fn create_depthbuffer(wgpu: &WGPU, width: u32, height: u32) -> wgpu::TextureView {
        let width = width.max(1);
        let height = height.max(1);
        let texture = wgpu.device.create_texture(
            &(wgpu::TextureDescriptor {
                label: Some("Depth Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: if Self::use_multisample() { 4 } else { 1 },
                dimension: wgpu::TextureDimension::D2,
                format: Self::depth_format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }),
        );
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: Some(Self::depth_format()),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            base_array_layer: 0,
            array_layer_count: None,
            mip_level_count: None,
            usage: None,
        })
    }
}
