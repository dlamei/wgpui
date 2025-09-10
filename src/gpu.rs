use std::{
    cell::RefCell,
    collections::HashMap,
    fmt, hash,
    sync::{Arc, Mutex},
};

use glam::Vec2;

use crate::utils;

#[derive(Debug, Clone)]
pub struct Texture {
    data: Arc<(wgpu::Texture, wgpu::TextureView)>,
}

impl PartialEq for Texture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }
}

impl Eq for Texture {}

impl Texture {
    pub fn new(texture: wgpu::Texture, texture_view: wgpu::TextureView) -> Self {
        Self {
            data: Arc::new((texture, texture_view)),
        }
    }

    pub fn raw(&self) -> &wgpu::Texture {
        &self.data.0
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.data.1
    }

    pub fn create_with_usage(
        wgpu: &WGPU,
        width: u32,
        height: u32,
        usage: wgpu::TextureUsages,
    ) -> Self {
        let texture_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = wgpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | usage,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&Default::default());

        Self::new(texture, texture_view)
    }

    pub fn create_render_texture(wgpu: &WGPU, width: u32, height: u32) -> Self {
        Self::create_with_usage(
            wgpu,
            width,
            height,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        )
    }

    pub fn create(wgpu: &WGPU, width: u32, height: u32, data: &[u8]) -> Self {
        assert_eq!((width * height * 4) as usize, data.len());

        let texture = Self::create_with_usage(wgpu, width, height, wgpu::TextureUsages::COPY_DST);

        wgpu.queue.write_texture(
            wgpu::TexelCopyTextureInfoBase {
                texture: texture.raw(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        texture
    }

    pub fn width(&self) -> u32 {
        self.raw().width()
    }

    pub fn height(&self) -> u32 {
        self.raw().height()
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width() as f32, self.height() as f32)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VertexDesc {
    pub label: &'static str,
    pub attributes: Vec<wgpu::VertexAttribute>,
    pub members: Vec<&'static str>,
    pub instanced: bool,
    pub uniform: bool,
    pub byte_size: usize,
}

/// sync structs tagged with @rust with the provided shader templates
/// 
pub fn pre_process_shader_code(
    code: &str,
    structs_desc: &ShaderTemplates<'_>, // struct_names: &[&str; N],
) -> Result<String, String> {
    let reqs = PipelineRequirement::parse_all(code);

    if reqs.len() != structs_desc.len() {
        log::warn!(
            "missmatch, required: {:?},\nfound: {:?}",
            reqs,
            structs_desc
        );
        return Err(format!(
            "number of required structs ({}), must match number of provided descriptions ({})",
            reqs.len(),
            structs_desc.len()
        ));
    }

    // check compatibility
    for (req, (desc, name)) in reqs.iter().zip(structs_desc.iter()) {
        if &req.name != name {
            return Err(format!("expected: '{}', found: '{name}'", req.name));
        }

        if req.fields.len() < desc.members.len() && !req.allow_extra {
            return Err(format!(
                "requirement for '{}' does not allow variadic number of fields",
                req.name
            ));
        }

        for (field_name, req_typ) in &req.fields {
            let found = desc
                .members
                .iter()
                .zip(desc.attributes.iter())
                .find(|(member_name, _)| *member_name == field_name);

            let Some((_, attr)) = found else {
                return Err(format!(
                    "description '{name}' does not contain '{}'",
                    field_name
                ));
            };

            let Some(desc_wgsl_typ) = vertex_format_to_wgsl(attr.format) else {
                return Err(format!("unsupported format: {:?}", attr.format));
            };

            if req_typ != desc_wgsl_typ {
                return Err(format!(
                    "type missmatch, expected: {req_typ}, found: {desc_wgsl_typ}"
                ));
            }
        }
    }

    // remove @rust ...
    let mut clean_code = String::new();
    let mut chars = code.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '@' {
            let remaining: String = chars.clone().collect();
            if remaining.starts_with("rust struct") {
                for ch in chars.by_ref() {
                    if ch == '}' {
                        break;
                    }
                }
                continue;
            }
        }
        clean_code.push(ch);
    }

    // gen. wgsl structs

    let mut wgsl_structs = String::new();
    let mut location = 0;

    for (desc, name) in structs_desc.iter() {
        let mut struct_str = format!("\nstruct {name} {{\n");

        for (attrib, member) in desc.attributes.iter().zip(&desc.members) {
            let ty = vertex_format_to_wgsl(attrib.format).unwrap();
            struct_str.push_str(&format!("@location({location}) {}: {},\n", member, ty));
            location += 1;
        }

        wgsl_structs.push_str(&struct_str);
        wgsl_structs.push_str("}\n");
    }

    let mut res = "\n//////////////// GENERATED ////////////////\n".to_string();
    res.push_str(&wgsl_structs);
    res.push_str("\n///////////////////////////////////////////\n\n");
    res.push_str(&clean_code);

    log::trace!("generated shader:\n{res}");

    Ok(res)
}

pub trait Vertex: Sized + Copy + bytemuck::Pod + bytemuck::Zeroable {
    const VERTEX_LABEL: &'static str;
    const VERTEX_ATTRIBUTES: &'static [wgpu::VertexAttribute];
    const VERTEX_MEMBERS: &'static [&'static str];

    fn instance_desc() -> VertexDesc {
        let mut desc = Self::desc();
        desc.instanced = true;
        desc
    }

    fn uniform_desc() -> VertexDesc {
        let mut desc = Self::desc();
        desc.uniform = true;
        desc
    }

    fn desc() -> VertexDesc {
        VertexDesc {
            label: Self::VERTEX_LABEL,
            attributes: Self::VERTEX_ATTRIBUTES.to_vec(),
            members: Self::VERTEX_MEMBERS.to_vec(),
            instanced: false,
            uniform: false,
            byte_size: std::mem::size_of::<Self>(),
        }
    }

    // fn vertex_attributes_offset(offset: u32) -> Vec<wgpu::VertexAttribute> {
    //     Self::VERTEX_ATTRIBUTES
    //         .iter()
    //         .copied()
    //         .map(|mut attrib| {
    //             attrib.shader_location += offset;
    //             attrib
    //         })
    //         .collect()
    // }

    fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        Self::buffer_layout_with_attributes(Self::VERTEX_ATTRIBUTES)
    }

    fn instance_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        let mut layout = Self::buffer_layout();
        layout.step_mode = wgpu::VertexStepMode::Instance;
        layout
    }

    fn buffer_layout_with_attributes<'a>(
        attribs: &'a [wgpu::VertexAttribute],
    ) -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: attribs,
        }
    }
}

pub trait AsVertexFormat {
    const VERTEX_FORMAT: wgpu::VertexFormat;
    const WGSL: Option<&'static str>;
}

// macro_rules! impl_as_vertex_fmt {
//     ($ty:ty: $fmt:ident) => {
//         impl AsVertexFormat for $ty {
//             const FORMAT: wgpu::VertexFormat = wgpu::VertexFormat::$fmt;
//         }
//     };
// }

// macro_rules! impl_as_vertex_fmt {
//     ($( $ty:ty: $fmt:ident ),* $(,)?) => {
//         $(
//             impl AsVertexFormat for $ty {
//                 const FORMAT: wgpu::VertexFormat = wgpu::VertexFormat::$fmt;
//             }
//         )*
//     };
// }

macro_rules! impl_as_vertex_fmt {
    // single entry, optionally with WGSL
    ($($ty:ty : $fmt:ident $( : $wgsl:expr )?),* $(,)?) => {
        $(
            impl AsVertexFormat for $ty {
                const VERTEX_FORMAT: wgpu::VertexFormat = wgpu::VertexFormat::$fmt;
                const WGSL: Option<&'static str> = impl_as_vertex_fmt!(@wgsl $($wgsl)?);
            }
        )*

        pub fn vertex_format_to_wgsl(fmt: wgpu::VertexFormat) -> Option<&'static str> {
            match fmt {
                $(
                    wgpu::VertexFormat::$fmt => {
                        None$(.or(Some($wgsl)))?
                    }
                ),*
                _ => None
            }
        }
    };

    // helper to expand WGSL presence
    (@wgsl $wgsl:expr) => { Some($wgsl) };
    (@wgsl) => { None };
}

impl_as_vertex_fmt! {
    u8: Uint8,
    [u8; 1]: Uint8,
    [u8; 2]: Uint8x2,
    [u8; 4]: Uint8x4,

    i8: Sint8,
    [i8; 1]: Sint8,
    [i8; 2]: Sint8x2,
    [i8; 4]: Sint8x4,

    u16: Uint16,
    [u16; 1]: Uint16,
    [u16; 2]: Uint16x2,
    [u16; 4]: Uint16x4,

    i16: Sint16,
    [i16; 1]: Sint16,
    [i16; 2]: Sint16x2,
    [i16; 4]: Sint16x4,

    u32: Uint32: "u32",
    [u32; 1]: Uint32: "u32",
    [u32; 2]: Uint32x2: "vec2<u32>",
    [u32; 3]: Uint32x3: "vec3<u32>",
    [u32; 4]: Uint32x4: "vec4<u32>",

    i32: Sint32: "i32",
    [i32; 1]: Sint32: "i32",
    [i32; 2]: Sint32x2: "vec2<i32>",
    [i32; 3]: Sint32x3: "vec3<i32>",
    [i32; 4]: Sint32x4: "vec4<i32>",

    f32: Float32: "f32",
    [f32; 1]: Float32: "f32",
    [f32; 2]: Float32x2: "vec2<f32>",
    [f32; 3]: Float32x3: "vec3<f32>",
    [f32; 4]: Float32x4: "vec4<f32>",

    f64: Float64: "f64",
    [f64; 1]: Float64: "f64",
    [f64; 2]: Float64x2: "vec2<f64>",
    [f64; 3]: Float64x3: "vec3<f64>",
    [f64; 4]: Float64x4: "vec4<f64>",

    glam::UVec2: Uint32x2: "vec2<u32>",
    glam::UVec3: Uint32x3: "vec3<u32>",
    glam::UVec4: Uint32x4: "vec4<u32>",

    glam::IVec2: Sint32x2: "vec2<i32>",
    glam::IVec3: Sint32x3: "vec3<i32>",
    glam::IVec4: Sint32x4: "vec4<i32>",

    glam::Vec2: Float32x2: "vec2<f32>",
    glam::Vec3: Float32x3: "vec3<f32>",
    glam::Vec4: Float32x4: "vec4<f32>",

    utils::RGB: Float32x3: "vec3<f32>",
    utils::RGBA: Float32x4: "vec4<f32>",
}

pub struct Renderer {
    pub framebuffer_msaa: Option<wgpu::TextureView>,
    pub framebuffer_resolve: wgpu::TextureView,
    pub depthbuffer: wgpu::TextureView,
    pub active_surface: Option<wgpu::SurfaceTexture>,
    pub wgpu: WGPUHandle,
}

impl Renderer {
    pub fn resolve_target(&mut self) -> RenderTarget<'_> {
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
            target_view: self.framebuffer_msaa.clone().unwrap(),
            resolve_view: Some(surface_texture_view),
            encoder: std::mem::ManuallyDrop::new(encoder),
            wgpu: &self.wgpu,
        }
    }

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

        if Self::use_multisample() {
            RenderTarget {
                target_view: self.framebuffer_msaa.clone().unwrap(),
                resolve_view: Some(surface_texture_view),
                encoder: std::mem::ManuallyDrop::new(encoder),
                wgpu: &self.wgpu,
            }
        } else {
            RenderTarget {
                target_view: surface_texture_view,
                resolve_view: None,
                encoder: std::mem::ManuallyDrop::new(encoder),
                wgpu: &self.wgpu,
            }
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

        let framebuffer_msaa = Some(Self::create_framebuffer_msaa_texture(&wgpu, width, height));
        let framebuffer_resolve = Self::create_framebuffer_resolve_texture(&wgpu, width, height);
        let depthbuffer = Self::create_depthbuffer(&wgpu, width, height);

        Self {
            framebuffer_msaa,
            framebuffer_resolve,
            depthbuffer,
            active_surface: None,
            wgpu: wgpu.into(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.wgpu.resize(width, height);
        self.framebuffer_msaa = Some(Self::create_framebuffer_msaa_texture(
            &self.wgpu, width, height,
        ));
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

    // pub const fn use_multisample() -> bool {
    //     #[cfg(not(target_arch = "wasm32"))]
    //     return true;
    //     #[cfg(target_arch = "wasm32")]
    //     return false;
    // }

    pub const fn multisample_count() -> u32 {
        #[cfg(not(target_arch = "wasm32"))]
        return 4;
        #[cfg(target_arch = "wasm32")]
        return 1;
    }

    pub fn use_multisample() -> bool {
        Self::multisample_count() != 1
    }

    pub fn create_framebuffer_msaa_texture(
        wgpu: &WGPU,
        width: u32,
        height: u32,
    ) -> wgpu::TextureView {
        let width = width.max(1);
        let height = height.max(1);

        let texture = wgpu.device.create_texture(
            &(wgpu::TextureDescriptor {
                label: Some("Framebuffer Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: Self::multisample_count(),
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
                sample_count: Self::multisample_count(),
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

#[derive(Debug)]
pub struct ResourceCache<ID, RSRC> {
    pub cache: HashMap<ID, Arc<RSRC>>,
}

impl<ID: Copy + Eq + hash::Hash + fmt::Debug, RSRC> ResourceCache<ID, RSRC> {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    fn register(&mut self, id: ID, pipeline: impl Into<Arc<RSRC>>) {
        self.cache.insert(id, pipeline.into());
    }

    fn get(&self, id: ID) -> Option<Arc<RSRC>> {
        self.cache.get(&id).cloned()
    }

    /// lazy create helper (if you want one-shot creation)
    fn get_or_insert_with<F>(&mut self, id: ID, load_fn: F) -> Arc<RSRC>
    where
        F: FnOnce() -> RSRC,
    {
        self.cache
            .entry(id)
            .or_insert_with(|| Arc::new(load_fn()))
            .clone()
    }
}

pub type WGPUHandle = Arc<WGPU>;

pub struct WGPU {
    pub pipeline_cache: Mutex<ResourceCache<UUID, wgpu::RenderPipeline>>,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: RefCell<wgpu::SurfaceConfiguration>,
    pub surface_format: wgpu::TextureFormat,
}

impl WGPU {
    pub fn width(&self) -> u32 {
        self.surface_config.borrow().width.max(1)
    }

    pub fn height(&self) -> u32 {
        self.surface_config.borrow().height.max(1)
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.width() as f32 / self.height() as f32
    }

    pub fn resize(&self, width: u32, height: u32) {
        self.surface_config.borrow_mut().width = width.max(1);
        self.surface_config.borrow_mut().height = height.max(1);
        self.surface
            .configure(&self.device, &*self.surface_config.borrow());
    }

    pub fn instance() -> wgpu::Instance {
        wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(any(target_os = "linux"))]
            backends: wgpu::Backends::PRIMARY,
            #[cfg(target_os = "macos")]
            backends: wgpu::Backends::METAL,
            #[cfg(target_os = "windows")]
            backends: wgpu::Backends::DX12 | wgpu::Backends::GL,
            #[cfg(target_arch = "wasm32")]
            backends: wgpu::Backends::GL | wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        })
    }

    /// Register a new render pipeline with the given ID
    pub fn register_pipeline(&self, id: UUID, pipeline: wgpu::RenderPipeline) {
        self.pipeline_cache.lock().unwrap().register(id, pipeline);
    }

    /// Get a registered pipeline by ID
    pub fn get_pipeline(&self, id: UUID) -> Option<Arc<wgpu::RenderPipeline>> {
        self.pipeline_cache.lock().unwrap().get(id)
    }

    /// Get or create a pipeline
    pub fn get_or_init_pipeline<F>(&self, id: UUID, load: F) -> Arc<wgpu::RenderPipeline>
    where
        F: FnOnce() -> wgpu::RenderPipeline,
    {
        self.pipeline_cache
            .lock()
            .unwrap()
            .get_or_insert_with(id, load)
            .clone()
    }

    /// Get the current surface texture and its view
    pub fn current_frame(
        &self,
    ) -> Result<(wgpu::SurfaceTexture, wgpu::TextureView), wgpu::SurfaceError> {
        let surface_texture = self.surface.get_current_texture()?;
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        Ok((surface_texture, view))
    }

    pub async fn new_async(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Self {
        let instance = Self::instance();
        let surface = instance.create_surface(window).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to request adapter!");

        let (device, queue) = {
            log::info!("WGPU Adapter Features: {:#?}", adapter.features());
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("WGPU Device"),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,

                    #[cfg(not(target_arch = "wasm32"))]
                    required_features: wgpu::Features::POLYGON_MODE_LINE,
                    #[cfg(target_arch = "wasm32")]
                    required_features: wgpu::Features::default(),

                    #[cfg(not(target_arch = "wasm32"))]
                    required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                    #[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
                    required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                    #[cfg(all(target_arch = "wasm32", feature = "webgl"))]
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                })
                .await
                .expect("Failed to request a device!")
        };

        let surface_capabilities = surface.get_capabilities(&adapter);

        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(surface_capabilities.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            #[cfg(target_arch = "wasm32")]
            present_mode: wgpu::PresentMode::Fifo,
            #[cfg(not(target_arch = "wasm32"))]
            present_mode: wgpu::PresentMode::Immediate,
            alpha_mode: surface_capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        Self {
            pipeline_cache: Mutex::new(ResourceCache::new()),
            surface,
            device,
            queue,
            surface_config: RefCell::new(surface_config),
            surface_format,
        }
    }
}

pub struct PipelineBuilder<'a> {
    pub label: Option<&'a str>,
    pub shader_source: &'a str,
    pub vertex_entry: &'a str,
    pub fragment_entry: &'a str,
    // pub vertex_buffers: &'a [wgpu::VertexBufferLayout<'a>],
    pub vertex_buffers: &'a [&'a VertexDesc],
    pub bind_group_layouts: &'a [&'a wgpu::BindGroupLayout],
    pub surface_format: wgpu::TextureFormat,
    pub blend_state: Option<wgpu::BlendState>,
    pub primitive_topology: wgpu::PrimitiveTopology,
    pub cull_mode: Option<wgpu::Face>,
    pub depth_format: Option<wgpu::TextureFormat>,
    pub sample_count: u32,
}

impl<'a> PipelineBuilder<'a> {
    pub fn new(shader_source: &'a str, surface_format: wgpu::TextureFormat) -> Self {
        Self {
            label: None,
            shader_source,
            vertex_entry: "vs_main",
            fragment_entry: "fs_main",
            vertex_buffers: &[],
            bind_group_layouts: &[],
            surface_format,
            blend_state: Some(wgpu::BlendState::REPLACE),
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            depth_format: None,
            sample_count: 1,
        }
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    pub fn vertex_entry(mut self, entry: &'a str) -> Self {
        self.vertex_entry = entry;
        self
    }

    pub fn fragment_entry(mut self, entry: &'a str) -> Self {
        self.fragment_entry = entry;
        self
    }

    pub fn vertex_buffers(mut self, buffers: &'a [&'a VertexDesc]) -> Self {
        self.vertex_buffers = buffers;
        self
    }

    pub fn bind_groups(mut self, layouts: &'a [&'a wgpu::BindGroupLayout]) -> Self {
        self.bind_group_layouts = layouts;
        self
    }

    pub fn blend_state(mut self, blend: Option<wgpu::BlendState>) -> Self {
        self.blend_state = blend;
        self
    }

    pub fn primitive_topology(mut self, topology: wgpu::PrimitiveTopology) -> Self {
        self.primitive_topology = topology;
        self
    }

    pub fn cull_mode(mut self, cull_mode: Option<wgpu::Face>) -> Self {
        self.cull_mode = cull_mode;
        self
    }

    pub fn depth(mut self, format: wgpu::TextureFormat) -> Self {
        self.depth_format = Some(format);
        self
    }

    pub fn sample_count(mut self, count: u32) -> Self {
        self.sample_count = count;
        self
    }

    pub fn build(self, device: &wgpu::Device) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: self.label,
            source: wgpu::ShaderSource::Wgsl(self.shader_source.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: self.label,
            bind_group_layouts: self.bind_group_layouts,
            push_constant_ranges: &[],
        });

        let depth_stencil = self.depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        let mut buffer_layouts = Vec::new();
        let mut location_offset = 0;

        let mut vertices_attribs: Vec<_> = self
            .vertex_buffers
            .iter()
            .filter_map(|desc| {
                if !desc.uniform {
                    Some(desc.attributes.clone())
                } else {
                    None
                }
            })
            .collect();

        for vertex_attribs in &mut vertices_attribs {
            vertex_attribs.iter_mut().enumerate().for_each(|(i, a)| {
                a.shader_location = location_offset + i as u32;
            });

            location_offset += vertex_attribs.len() as u32;
        }

        for (desc, fixed_attribs) in self.vertex_buffers.iter().zip(vertices_attribs.iter()) {
            let layout = wgpu::VertexBufferLayout {
                array_stride: desc.byte_size as wgpu::BufferAddress,
                step_mode: match desc.instanced {
                    true => wgpu::VertexStepMode::Instance,
                    false => wgpu::VertexStepMode::Vertex,
                },
                attributes: &*fixed_attribs,
            };

            buffer_layouts.push(layout);
        }

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: self.label,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(self.vertex_entry),
                buffers: &buffer_layouts,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(self.fragment_entry),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
                    blend: self.blend_state,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: self.primitive_topology,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: self.cull_mode,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil,
            multisample: wgpu::MultisampleState {
                count: self.sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        })
    }
}

#[derive(Debug)]
pub struct PipelineRequirement {
    pub name: String,
    pub fields: HashMap<String, String>, // name -> type string
    pub allow_extra: bool,
}

impl PipelineRequirement {
    pub fn parse_all(src: &str) -> Vec<PipelineRequirement> {
        let mut out = Vec::new();
        let mut search_start = 0;

        // while let Some(start) = src[search_start..].find("@rust struct")
        for (start, _) in src.match_indices("@rust struct") {
            // let absolute_start = search_start + start;
            let rest = &src[start + "@rust struct".len()..];

            // Parse struct name
            let rest = rest.trim_start();
            let name_end = rest
                .find(|c: char| c.is_whitespace() || c == '{')
                .unwrap_or(rest.len());
            let name = rest[..name_end].trim().to_string();

            // Find opening brace
            let rest_after_name = &rest[name_end..];
            if let Some(open_brace) = rest_after_name.find('{') {
                if let Some(close_brace) = rest_after_name.find('}') {
                    let body = &rest_after_name[open_brace + 1..close_brace];
                    let mut fields = HashMap::new();
                    let mut allow_extra = false;

                    for part in body.split(',') {
                        let part = part.trim();
                        if part.is_empty() {
                            continue;
                        }
                        if part == "..." {
                            allow_extra = true;
                            continue;
                        }
                        if let Some((field_name, field_type)) = part.split_once(':') {
                            fields.insert(
                                field_name.trim().to_string(),
                                field_type.trim().to_string(),
                            );
                        }
                    }

                    out.push(PipelineRequirement {
                        name,
                        fields,
                        allow_extra,
                    });

                    // search_start = absolute_start + "@rust struct".len() + rest_after_name.len();
                } else {
                    break; // Malformed - no closing brace
                }
            } else {
                break; // Malformed - no opening brace
            }
        }

        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UUID(pub u64);

pub type ShaderID = &'static str;

pub type ShaderTemplates<'a> = [(&'a VertexDesc, &'a str)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShaderTyp {
    Vertex,
    Instance,
    Uniform,
}

pub trait ShaderHandle {
    const RENDER_PIPELINE_ID: ShaderID;
    fn build_pipeline(&self, desc: &ShaderTemplates<'_>, wgpu: &WGPU) -> wgpu::RenderPipeline;

    fn pipeline_generic_id() -> UUID {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        Self::RENDER_PIPELINE_ID.hash(&mut hasher);
        UUID(hasher.finish())
    }

    fn pipeline_vertex_id(desc: &ShaderTemplates<'_>) -> UUID {
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

    fn try_rebuild(&self, desc: &ShaderTemplates<'_>, wgpu: &WGPU) {
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

    fn get_pipeline(&self, desc: &ShaderTemplates<'_>, wgpu: &WGPU) -> Arc<wgpu::RenderPipeline> {
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
