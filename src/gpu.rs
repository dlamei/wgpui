use std::{
    collections::HashMap,
    fmt, hash,
    sync::{Arc, Mutex},
};

use crate::{RenderTarget, UUID, utils};

pub trait Vertex: Sized + Copy + bytemuck::Pod + bytemuck::Zeroable {
    const VERTEX_LABEL: &'static str;
    const VERTEX_ATTRIBUTES: &'static [wgpu::VertexAttribute];
    const VERTEX_MEMBERS: &'static [&'static str];

    fn vertex_attributes_offset(offset: u32) -> Vec<wgpu::VertexAttribute> {
        Self::VERTEX_ATTRIBUTES
            .iter()
            .copied()
            .map(|mut attrib| {
                attrib.shader_location += offset;
                attrib
            })
            .collect()
    }

    fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        Self::buffer_layout_with_attributes(&Self::VERTEX_ATTRIBUTES)
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

    fn as_wgsl_struct(name: &str) -> String {
        assert_eq!(
            Self::VERTEX_ATTRIBUTES.len(),
            Self::VERTEX_MEMBERS.len(),
            "VERTEX_ATTRIBUTES and VERTEX_MEMBERS must have the same length"
        );

        let mut out = format!("struct {} {{\n", name);
        for (attr, member_name) in Self::VERTEX_ATTRIBUTES
            .iter()
            .zip(Self::VERTEX_MEMBERS.iter())
        {
            let ty = vertex_format_to_wgsl(attr.format)
                .unwrap_or_else(|| panic!("Unsupported vertex format: {:?}", attr.format));
            out.push_str(&format!(
                "    @location({}) {}: {},\n",
                attr.shader_location, member_name, ty
            ));
        }
        out.push('}');
        out
    }

    /// Process shader code by extracting requirements, checking compatibility, and injecting WGSL struct
    fn process_shader_code(
        shader_code: &str,
        struct_name: &str,
    ) -> Result<String, ShaderProcessingError> {
        // Parse requirements from the shader
        let requirements = PipelineRequirement::parse_requirements(shader_code);

        // Check compatibility
        Self::check_compatibility(&requirements)?;

        // Remove rust requirements and inject WGSL struct
        let cleaned_shader = Self::remove_rust_requirements(shader_code);
        let wgsl_struct = Self::as_wgsl_struct(struct_name);

        // Insert the WGSL struct at the beginning of the shader
        Ok(format!("{}\n\n{}", wgsl_struct, cleaned_shader))
    }

    /// Check if this vertex type is compatible with the shader requirements
    fn check_compatibility(
        requirements: &[PipelineRequirement],
    ) -> Result<(), ShaderProcessingError> {
        for req in requirements {
            if req.name == Self::VERTEX_LABEL || req.name == "Vertex" {
                // Check if we have all required fields
                for (field_name, expected_type) in &req.fields {
                    let found = Self::VERTEX_MEMBERS
                        .iter()
                        .zip(Self::VERTEX_ATTRIBUTES.iter())
                        .find(|(member_name, _)| *member_name == field_name);

                    if let Some((_, attr)) = found {
                        let actual_wgsl_type = vertex_format_to_wgsl(attr.format)
                            .ok_or_else(|| ShaderProcessingError::UnsupportedFormat(attr.format))?;

                        if actual_wgsl_type != expected_type {
                            return Err(ShaderProcessingError::TypeMismatch {
                                field: field_name.clone(),
                                expected: expected_type.clone(),
                                actual: actual_wgsl_type.to_string(),
                            });
                        }
                    } else if !req.allow_extra {
                        return Err(ShaderProcessingError::MissingField(field_name.clone()));
                    }
                }

                // Check if we have extra fields that aren't allowed
                if !req.allow_extra {
                    for member_name in Self::VERTEX_MEMBERS {
                        if !req.fields.contains_key(*member_name) {
                            return Err(ShaderProcessingError::ExtraField(member_name.to_string()));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Remove @rust struct requirements from shader code
    fn remove_rust_requirements(shader_code: &str) -> String {
        let mut result = String::new();
        let mut chars = shader_code.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '@' {
                // Check if this is the start of "@rust struct"
                let remaining: String = chars.clone().collect();
                if remaining.starts_with("rust struct") {
                    // Skip until we find the closing brace
                    while let Some(ch) = chars.next() {
                        if ch == '}' {
                            break;
                        }
                    }
                    continue;
                }
            }
            result.push(ch);
        }

        result
    }

    fn shader_uuid<P: crate::ShaderHandle>() -> UUID {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        P::RENDER_PIPELINE_ID.hash(&mut hasher);
        Self::VERTEX_ATTRIBUTES.hash(&mut hasher);
        Self::VERTEX_MEMBERS.hash(&mut hasher);
        UUID(hasher.finish())
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
    pub wgpu: WGPU,
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

pub struct WGPU {
    pub pipeline_cache: Mutex<ResourceCache<UUID, wgpu::RenderPipeline>>,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface_format: wgpu::TextureFormat,
}

impl WGPU {
    pub fn width(&self) -> u32 {
        self.surface_config.width.max(1)
    }

    pub fn height(&self) -> u32 {
        self.surface_config.height.max(1)
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.width() as f32 / self.height() as f32
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width.max(1);
        self.surface_config.height = height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
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
            backends: wgpu::Backends::GL,
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
            present_mode: wgpu::PresentMode::Fifo,
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
            surface_config,
            surface_format,
        }
    }
}

pub struct PipelineBuilder<'a> {
    pub label: Option<&'a str>,
    pub shader_source: &'a str,
    pub vertex_entry: &'a str,
    pub fragment_entry: &'a str,
    pub vertex_buffers: &'a [wgpu::VertexBufferLayout<'a>],
    pub bind_group_layouts: &'a [&'a wgpu::BindGroupLayout],
    pub surface_format: wgpu::TextureFormat,
    pub blend_state: Option<wgpu::BlendState>,
    pub primitive_topology: wgpu::PrimitiveTopology,
    pub cull_mode: Option<wgpu::Face>,
    pub depth_format: Option<wgpu::TextureFormat>,
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

    pub fn vertex_buffers(mut self, buffers: &'a [wgpu::VertexBufferLayout<'a>]) -> Self {
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

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: self.label,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(self.vertex_entry),
                buffers: self.vertex_buffers,
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
                count: 1,
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
    pub fn parse_requirements(src: &str) -> Vec<PipelineRequirement> {
        let mut out = Vec::new();
        let mut search_start = 0;

        while let Some(start) = src[search_start..].find("@rust struct") {
            let absolute_start = search_start + start;
            let rest = &src[absolute_start + "@rust struct".len()..];

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

                    search_start = absolute_start + "@rust struct".len() + rest_after_name.len();
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

#[derive(Debug, Clone)]
pub enum ShaderProcessingError {
    MissingField(String),
    ExtraField(String),
    TypeMismatch {
        field: String,
        expected: String,
        actual: String,
    },
    UnsupportedFormat(wgpu::VertexFormat),
}

impl std::fmt::Display for ShaderProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShaderProcessingError::MissingField(field) => {
                write!(f, "Missing required field: {}", field)
            }
            ShaderProcessingError::ExtraField(field) => {
                write!(f, "Extra field not allowed: {}", field)
            }
            ShaderProcessingError::TypeMismatch {
                field,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Type mismatch for field '{}': expected '{}', got '{}'",
                    field, expected, actual
                )
            }
            ShaderProcessingError::UnsupportedFormat(format) => {
                write!(f, "Unsupported vertex format: {:?}", format)
            }
        }
    }
}

impl std::error::Error for ShaderProcessingError {}
