use std::{collections::HashMap, fmt};

use cosmic_text as ctext;
use glam::{Mat4, Vec2};
use macros::vertex;
use wgpu::util::DeviceExt;

use crate::{
    Vertex as VertexTyp,
    gpu::{self, ShaderHandle, WGPU, WGPUHandle},
    rect::Rect,
    ui::{WidgetFlags, WidgetOpt},
    utils::RGBA,
};

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

pub fn tessellate_convex_fill(
    points: &[Vec2],
    col: RGBA,
    antialias: bool,
) -> (Vec<Vertex>, Vec<u32>) {
    let n = points.len();
    if n < 3 {
        return (Vec::new(), Vec::new());
    }

    if !antialias {
        let mut verts = Vec::new();
        let mut idxs = Vec::new();
        // no-AA: just triangulate polygon fan
        for p in points {
            verts.push(Vertex::color(*p, col));
        }

        for i in 2..n {
            idxs.extend_from_slice(&[0, (i - 1) as u32, i as u32]);
        }
        return (verts, idxs);
    }

    const AA_SIZE: f32 = 1.0;
    const EPS: f32 = 1e-12;
    let col_trans = RGBA::rgba_f(col.r, col.g, col.b, 0.0);
    let mut verts = Vec::with_capacity(n * 2);
    let mut idxs = Vec::with_capacity((n - 2) * 3 + n * 6);

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

pub struct DrawList {
    pub gpu_vertices: wgpu::Buffer,
    pub gpu_indices: wgpu::Buffer,

    pub draw_buffer: DrawBuffer,
    pub screen_size: Vec2,

    pub path: Vec<Vec2>,
    pub path_closed: bool,

    pub resolution: f32,
    pub antialias: bool,

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
            gpu_vertices,
            gpu_indices,
            screen_size: Vec2::ONE,
            path: Vec::new(),
            path_closed: false,
            resolution: 20.0,
            antialias: true,
            draw_buffer: DrawBuffer::new(
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
        self.path_clear();
        self.draw_buffer.clear();

        std::mem::swap(&mut self.text_cache, &mut self.text_cache_2);
        self.text_cache_2 = TextCache::new();
    }

    pub fn draw_uv_rect(&mut self, rect: Rect, uv_min: Vec2, uv_max: Vec2, tint: RGBA) {
        let (verts, indxs) = tessellate_uv_rect(rect, uv_min, uv_max, 1, tint);
        self.draw_buffer.push(&verts, &indxs)
    }

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
            let (vtx, idx) = tessellate_convex_fill(&self.path, opt.fill_color, self.antialias);
            self.draw_buffer.push(&vtx, &idx);
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
            self.draw_buffer.push(&vtx, &idx);
        }

        self.path_clear();
    }

    // AI SLOP
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

        vtx.iter_mut().enumerate().for_each(|(i, v)| {
            v.col = cols[i % cols.len()];
        });

        self.draw_buffer.push(&vtx, &idx);
        // self.draw_memory.push(&vtx, &idx);

        self.path_clear();
    }

    pub fn as_wireframe(&mut self, thickness: f32) {
        self.path_clear();

        let memory = self.draw_buffer.clone();
        self.draw_buffer.clear();

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
        self.draw_buffer.chunks.len() as u32
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        self.draw_multiple(rpass, wgpu, 0);
    }

    fn draw_multiple<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU, i: u32) {
        let proj =
            Mat4::orthographic_lh(0.0, self.screen_size.x, self.screen_size.y, 0.0, -1.0, 1.0);

        let global_uniform = GlobalUniform::new(self.screen_size, proj);

        let bind_group = build_bind_group(
            global_uniform,
            self.font_atlas.textures.last().unwrap().texture.view(),
            wgpu,
        );

        let (verts, indxs) = self.draw_buffer.get_chunk_data(i as usize).unwrap();

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
pub struct DrawBuffer {
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

impl Default for DrawBuffer {
    fn default() -> Self {
        // 2^16
        const MAX_VERTEX_COUNT: usize = 65_536;
        // 2^17
        const MAX_INDEX_COUNT: usize = 131_072;
        Self::new(MAX_VERTEX_COUNT, MAX_INDEX_COUNT)
    }
}

impl DrawBuffer {
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
