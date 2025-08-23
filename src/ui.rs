use glam::{Mat4, UVec2, UVec4, Vec2, Vec4};
use macros::vertex;
use rustc_hash::FxHashMap;
use wgpu::util::DeviceExt;

use std::{
    collections::VecDeque, fmt, hash::{Hash, Hasher}, ops, time::{Duration, Instant}
};

use crate::{
    RGBA, RenderPassHandle, ShaderGenerics, ShaderHandle, Vertex, VertexPosCol,
    gpu::{self, VertexDesc, WGPU},
    rect::Rect,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetId(u64);

impl WidgetId {
    pub const NULL: WidgetId = WidgetId(0);

    pub fn from_str(s: &str) -> Self {
        let mut hasher = rustc_hash::FxHasher::default();
        s.hash(&mut hasher);
        Self(hasher.finish().max(1))
    }

    pub fn is_null(&self) -> bool {
        *self == Self::NULL
    }
}

impl Default for WidgetId {
    fn default() -> Self {
        WidgetId::NULL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    X = 0,
    Y = 1,
}

impl Axis {
    pub fn flip(&self) -> Self {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizingTyp {
    Null,
    Fit,
    Grow,
    Fixed(f32),
    Percent(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerAxis<T>(pub [T; 2]);

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

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

    pub fn axis_padding(&self, a: Axis) -> [f32; 2] {
        match a {
            Axis::X => [self.left, self.right],
            Axis::Y => [self.top, self.bottom],
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct WidgetFlags: u32 {
        const NONE              = 0;
        const DRAW_BORDER       = 1 << 0;
        const DRAW_BACKGROUND   = 1 << 1;
        const DRAGGABLE         = 1 << 2;
        const HOVERABLE         = 1 << 3;
    }
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
        const PRESSED_L = 1 << 0;
        const PRESSED_M = 1 << 1;
        const PRESSED_R = 1 << 2;

        const DRAGGING_L = 1 << 3;
        const DRAGGING_M = 1 << 4;
        const DRAGGING_R = 1 << 5;

        const DOUBLE_DRAGGING_L = 1 << 6;
        const DOUBLE_DRAGGING_M = 1 << 7;
        const DOUBLE_DRAGGING_R = 1 << 8;

        const RELEASED_L = 1 << 9;
        const RELEASED_M = 1 << 10;
        const RELEASED_R = 1 << 11;

        const CLICKED_L = 1 << 12;
        const CLICKED_M = 1 << 13;
        const CLICKED_R = 1 << 14;

        const DOUBLE_CLICKED_L = 1 << 15;
        const DOUBLE_CLICKED_M = 1 << 16;
        const DOUBLE_CLICKED_R = 1 << 17;

        const HOVERING = 1 << 18;
        const MOUSE_OVER = 1 << 19; // may be occluded

        const PRESSED_KEYBOARD = 1 << 20;
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

sig_fn!(hovering => HOVERING);
sig_fn!(mouse_over => MOUSE_OVER);
sig_fn!(pressed => PRESSED_L | PRESSED_KEYBOARD);
sig_fn!(clicked => CLICKED_L | PRESSED_KEYBOARD);
sig_fn!(double_clicked => DOUBLE_CLICKED_L);
sig_fn!(dragging => DRAGGING_L);
sig_fn!(released => RELEASED_L);

// #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
// pub struct MouseState {
//     pub left: bool,
//     pub middle: bool,
//     pub right: bool,
// }

// impl ops::Index<MouseButton> for MouseState {
//     type Output = bool;

//     fn index(&self, index: MouseButton) -> &Self::Output {
//         match index {
//             MouseButton::Left => &self.left,
//             MouseButton::Right => &self.right,
//             MouseButton::Middle => &self.middle,
//         }
//     }
// }

// impl ops::IndexMut<MouseButton> for MouseState {
//     fn index_mut(&mut self, index: MouseButton) -> &mut Self::Output {
//         match index {
//             MouseButton::Left => &mut self.left,
//             MouseButton::Right => &mut self.right,
//             MouseButton::Middle => &mut self.middle,
//         }
//     }
// }

#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct GlobalUniform {
    pub proj: Mat4,
}

#[vertex]
pub struct Vertex2D {
    pub pos: Vec2,
}

#[vertex]
pub struct RectInst {
    pub min: Vec2,
    pub max: Vec2,
    pub color: RGBA,
}

pub struct RectRender {
    pub global_data: GlobalUniform,

    pub unit_rectangle: wgpu::Buffer,
    pub global_uniform: wgpu::Buffer,

    pub rect_buffer: wgpu::Buffer,
    pub n_instances: u32,
}

impl RectRender {
    pub fn update_window_size(&mut self, width: u32, height: u32) {
        let aspect = width as f32 / height.max(1) as f32;
        self.global_data.proj =
            Mat4::orthographic_lh(0.0, width as f32, height as f32, 0.0, -1.0, 1.0);
    }

    pub fn update_rect_instances(&mut self, rect_instances: &[RectInst], wgpu: &WGPU) {
        if rect_instances.len() < 1024 {
            // self.rect_buffer = wgpu
            //     .device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            //         label: Some("test"),
            //         contents: bytemuck::cast_slice(rect_instances),
            //         usage: wgpu::BufferUsages::VERTEX,
            //     });
            wgpu.queue
                .write_buffer(&self.rect_buffer, 0, bytemuck::cast_slice(rect_instances));
            self.n_instances = rect_instances.len() as u32;
        }
    }

    pub fn new(wgpu: &WGPU) -> Self {
        // let vertices = [
        //     RectInst {
        //         min: Vec2::new(0.0, 0.0),
        //         max: Vec2::new(200.0, 200.0),
        //         color: RGBA::RED,
        //     },
        //     RectInst {
        //         min: Vec2::new(250.0, 200.0),
        //         max: Vec2::new(200.0, 400.0),
        //         color: RGBA::BLUE,
        //     },
        // ];
        let rect_buffer = wgpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect_instances"),
            size: 1024 * std::mem::size_of::<RectInst>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // let rect_buffer = wgpu
        //     .device
        //     .create_buffer_init(&wgpu::util::BufferInitDescriptor {
        //         label: Some("debug_rect_instance_buffer"),
        //         contents: bytemuck::cast_slice(&vertices),
        //         usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        //     });

        let vertices = [
            Vertex2D {
                pos: Vec2::new(0.0, 0.0),
            },
            Vertex2D {
                pos: Vec2::new(1.0, 0.0),
            },
            Vertex2D {
                pos: Vec2::new(0.0, 1.0),
            },
            Vertex2D {
                pos: Vec2::new(1.0, 0.0),
            },
            Vertex2D {
                pos: Vec2::new(1.0, 1.0),
            },
            Vertex2D {
                pos: Vec2::new(0.0, 1.0),
            },
        ];

        let unit_rectangle = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("debug_unit_rect_vertex_buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let global_data = GlobalUniform {
            proj: Mat4::IDENTITY,
        };

        let global_uniform = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rect_global_uniform_buffer"),
                contents: bytemuck::cast_slice(&[global_data]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        Self {
            rect_buffer,
            n_instances: 2,
            unit_rectangle,
            global_uniform,
            global_data,
        }
    }

    pub fn build_global_bind_group(&self, wgpu: &WGPU) -> wgpu::BindGroup {
        let global_uniform = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rect_global_uniform_buffer"),
                contents: bytemuck::cast_slice(&[self.global_data]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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

impl RenderPassHandle for RectRender {
    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        rpass.set_vertex_buffer(0, self.unit_rectangle.slice(..));
        rpass.set_vertex_buffer(1, self.rect_buffer.slice(..));

        let bind_group = self.build_global_bind_group(wgpu);
        rpass.set_bind_group(0, &bind_group, &[]);

        let shader = RectShader;
        rpass.set_pipeline(&shader.get_pipeline(
            &[
                (&Vertex2D::desc(), "Vertex"),
                (&RectInst::instance_desc(), "RectInst"),
            ],
            wgpu,
        ));

        rpass.draw(0..6, 0..self.n_instances);
    }
}

pub struct RectShader;

impl ShaderHandle for RectShader {
    const RENDER_PIPELINE_ID: crate::ShaderID = "rect_shader";

    fn build_pipeline(&self, desc: &ShaderGenerics<'_>, wgpu: &WGPU) -> wgpu::RenderPipeline {
        const SHADER_SRC: &str = r#"


            @rust struct Vertex {
                pos: vec2<f32>,
            }

            @rust struct RectInst {
                min: vec2<f32>,
                max: vec2<f32>,
                color: vec4<f32>,
                ...
            }

            struct GlobalUniform {
                proj: mat4x4<f32>,
            }

            @group(0) @binding(0)
            var<uniform> global: GlobalUniform;

            struct VSOut {
                @builtin(position) pos: vec4<f32>,
                @location(0) color: vec4<f32>,
            };

            @vertex
                fn vs_main(
                    v: Vertex,
                    r: RectInst,
                ) -> VSOut {
                    var out: VSOut;

                    let size = r.max - r.min;

                    out.color = r.color;
                    out.pos = global.proj * vec4<f32>(
                        v.pos.x * size.x + r.min.x,
                        v.pos.y * size.y + r.min.y,
                        0.0,
                        1.0
                    );

                    return out;
                }


            @fragment
                fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
                    return in.color;
                }
            "#;

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

        let shader_src = gpu::process_shader_code(SHADER_SRC, &desc).unwrap();
        let vertices = desc.iter().map(|d| d.0).collect::<Vec<_>>();
        gpu::PipelineBuilder::new(&shader_src, wgpu.surface_format)
            .label("rect_pipeline")
            .vertex_buffers(&vertices)
            .bind_groups(&[&global_bind_group_layout])
            .build(&wgpu.device)
    }
}

#[derive(Debug, Clone)]
pub struct Widget {
    pub id: WidgetId,
    pub rect: Option<Rect>,
    pub flags: WidgetFlags,
    pub last_frame: u64,

    pub color: RGBA,
    pub style: LayoutStyle,
    pub children: Vec<WidgetId>,
}

impl Widget {
    pub fn new(id: WidgetId) -> Self {
        Self {
            id,
            rect: None,
            flags: WidgetFlags::NONE,
            last_frame: 0,
            color: RGBA::ZERO,
            style: LayoutStyle::default(),
            children: Vec::new(),
        }
    }

    pub fn size(&self) -> Option<Vec2> {
        self.rect.map(|r| r.max - r.min)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum PositionTyp {
    #[default]
    Auto,
    Absolute(Vec2),
    Relative(Vec2),
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutStyle {
    pub direction: Axis,
    pub padding: Padding,
    pub spacing: f32,
    pub width: SizingTyp,
    pub height: SizingTyp,
    pub position: PositionTyp,
    pub min_size: Vec2,
    pub max_size: Vec2,
}

impl LayoutStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn direction(mut self, a: Axis) -> Self {
        self.direction = a;
        self
    }

    pub fn fixed_x(mut self, x: f32) -> Self {
        self.width = SizingTyp::Fixed(x);
        self
    }

    pub fn fixed_y(mut self, x: f32) -> Self {
        self.height = SizingTyp::Fixed(x);
        self
    }

    pub fn fixed(self, x: f32, y: f32) -> Self {
        self.fixed_x(x).fixed_y(y)
    }

    pub fn pad_vert(mut self, p: f32) -> Self {
        self.padding.left = p;
        self.padding.right = p;
        self
    }

    pub fn pad_hor(mut self, p: f32) -> Self {
        self.padding.top = p;
        self.padding.bottom = p;
        self
    }

    pub fn padding(mut self, p: f32) -> Self {
        self.padding = Padding::all(p);
        self
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn fit_x(mut self) -> Self {
        self.width = SizingTyp::Fit;
        self
    }

    pub fn grow_x(mut self) -> Self {
        self.width = SizingTyp::Grow;
        self
    }

    pub fn grow_y(mut self) -> Self {
        self.height = SizingTyp::Grow;
        self
    }

    pub fn grow(self) -> Self {
        self.grow_x().grow_y()
    }

    pub fn fit_y(mut self) -> Self {
        self.height = SizingTyp::Fit;
        self
    }

    pub fn fit(mut self) -> Self {
        self.fit_x().fit_y()
    }

    pub fn absolute(mut self, x: f32, y: f32) -> Self {
        self.position = PositionTyp::Absolute(Vec2::new(x, y));
        self
    }

    pub fn relative(mut self, x: f32, y: f32) -> Self {
        self.position = PositionTyp::Relative(Vec2::new(x, y));
        self
    }

    pub fn auto_position(mut self) -> Self {
        self.position = PositionTyp::Auto;
        self
    }
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            direction: Axis::X,
            padding: Padding::all(4.0),
            spacing: 2.0,
            width: SizingTyp::Fit,
            height: SizingTyp::Fit,
            min_size: Vec2::ZERO,
            max_size: Vec2::INFINITY,
            position: PositionTyp::Auto,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: WidgetId,
    pub computed_size: Vec2,
    pub computed_pos: Vec2,
    pub children: Vec<LayoutNode>,
}


#[derive(Debug, Clone)]
pub struct State {
    pub widgets: FxHashMap<WidgetId, Widget>,
    pub current_frame: u64,
    pub id_stack: Vec<WidgetId>,
    pub mouse: MouseState,
    pub hot_widget: WidgetId,
    pub active_widget: WidgetId,
    pub keyboard_focus: WidgetId,
    pub output_rects: Vec<RectInst>,
    pub layout_root: Option<LayoutNode>,
    pub screen_size: Vec2,
}

impl State {
    pub fn new() -> Self {
        Self {
            widgets: FxHashMap::default(),
            current_frame: 0,
            id_stack: Vec::new(),
            mouse: MouseState::new(),
            hot_widget: WidgetId::NULL,
            active_widget: WidgetId::NULL,
            keyboard_focus: WidgetId::NULL,
            output_rects: Vec::new(),
            layout_root: None,
            screen_size: Vec2::ZERO,
        }
    }

    pub fn set_screen_size(&mut self, size: Vec2) {
        self.screen_size = size;
    }

    pub fn begin_frame(&mut self) {
        self.current_frame += 1;
        self.output_rects.clear();
        self.hot_widget = WidgetId::NULL;
    }

    pub fn end_frame(&mut self) {
        let current_frame = self.current_frame;
        self.widgets
            .retain(|_, widget| current_frame - widget.last_frame < 5);

        if let Some(root) = self.layout_root.clone() {
            self.apply_layout(&root);
        }

        self.generate_render_rects();
    }

    pub fn generate_render_rects(&mut self) {
        
        for (&id, widget) in &self.widgets {
            if let Some(r) = widget.rect {
                let min = r.min;
                let max = r.max;
                let color = widget.color;

                self.output_rects.push(RectInst {
                    min,
                    max,
                    color,
                });
            }
        }
    }

    pub fn push_id(&mut self, id: WidgetId) {
        self.id_stack.push(id);
    }

    pub fn pop_id(&mut self) {
        self.id_stack.pop();
    }

    pub fn current_id(&self) -> Option<WidgetId> {
        self.id_stack.last().copied()
    }

    pub fn build_id(&self, name: &str) -> WidgetId {
        use std::hash::{Hash, Hasher};
        if let Some(p_id) = self.current_id() {
            let mut hasher = rustc_hash::FxHasher::with_seed(p_id.0 as usize);
            name.hash(&mut hasher);
            WidgetId(hasher.finish())
        } else {
            WidgetId::from_str(name)
        }
    }

    pub fn begin_widget(&mut self, name: &str) -> WidgetId {
        let id = self.build_id(name);

        let widget = self.widgets.entry(id).or_insert(Widget::new(id));
        widget.last_frame = self.current_frame;
        widget.children.clear();

        self.push_id(id);
        id
    }

    // pub fn end_widget(&mut self) {
    //     self.pop_id();
    // }

    pub fn set_widget_style(&mut self, id: WidgetId, style: LayoutStyle) {
        if let Some(widget) = self.widgets.get_mut(&id) {
            widget.style = style;
        }
    }

    pub fn set_widget_color(&mut self, id: WidgetId, color: RGBA) {
        if let Some(widget) = self.widgets.get_mut(&id) {
            widget.color = color;
        }
    }

    pub fn set_widget_flags(&mut self, id: WidgetId, flags: WidgetFlags) {
        if let Some(widget) = self.widgets.get_mut(&id) {
            widget.flags = flags;
        }
    }

    pub fn set_widget_position(&mut self, id: WidgetId, position: PositionTyp) {
        if let Some(widget) = self.widgets.get_mut(&id) {
            widget.style.position = position;
        }
    }

    fn calculate_layout(&self, id: WidgetId, available_size: Vec2) -> LayoutNode {
        let widget = &self.widgets[&id];
        let style = &widget.style;

        let mut computed_size = Vec2::ZERO;

        // Size calculation (same as before)
        match style.width {
            SizingTyp::Null => todo!(),
            SizingTyp::Fit => (),
            SizingTyp::Grow => computed_size.x = available_size.x,
            SizingTyp::Fixed(w) => computed_size.x = w,
            SizingTyp::Percent(p) => computed_size.x = available_size.x * p,
        }

        match style.height {
            SizingTyp::Null => todo!(),
            SizingTyp::Fit => (),
            SizingTyp::Grow => computed_size.y = available_size.y,
            SizingTyp::Fixed(w) => computed_size.y = w,
            SizingTyp::Percent(p) => computed_size.y = available_size.y * p,
        }

        let mut children_nodes = Vec::new();
        let mut content_size = Vec2::ZERO;
        let child_available = computed_size - style.padding.axis_sum();

        // Only auto-positioned children contribute to layout flow
        let auto_children: Vec<_> = widget.children.iter()
            .filter(|&&child_id| {
                matches!(self.widgets[&child_id].style.position, PositionTyp::Auto)
            })
            .copied()
            .collect();

        // Calculate layout for auto children
        for &child_id in &auto_children {
            let child_node = self.calculate_layout(child_id, child_available);

            match style.direction {
                Axis::Y => { // Vertical layout
                    content_size.x = content_size.x.max(child_node.computed_size.x);
                    content_size.y += child_node.computed_size.y;
                    if !children_nodes.is_empty() {
                        content_size.y += style.spacing;
                    }
                }
                Axis::X => { // Horizontal layout
                    content_size.x += child_node.computed_size.x;
                    content_size.y = content_size.y.max(child_node.computed_size.y);
                    if !children_nodes.is_empty() {
                        content_size.x += style.spacing;
                    }
                }
            }

            children_nodes.push(child_node);
        }

        // Calculate layout for positioned children (they don't affect parent size)
        for &child_id in &widget.children {
            let child_widget = &self.widgets[&child_id];
            if !matches!(child_widget.style.position, PositionTyp::Auto) {
                let child_node = self.calculate_layout(child_id, self.screen_size);
                children_nodes.push(child_node);
            }
        }

        // Apply fit sizing based on auto children only
        if matches!(style.width, SizingTyp::Fit) {
            computed_size.x = content_size.x + style.padding.sum_along_axis(Axis::X);
        }

        if matches!(style.height, SizingTyp::Fit) {
            computed_size.y = content_size.y + style.padding.sum_along_axis(Axis::Y);
        }

        computed_size = computed_size.clamp(style.min_size, style.max_size);

        LayoutNode {
            id,
            computed_size,
            computed_pos: Vec2::ZERO,
            children: children_nodes,
        }
    }



    // Replace your apply_layout_recursive method with this:
    pub fn apply_layout_recursive(&mut self, node: &LayoutNode, parent_pos: Vec2) {
        let widget = self.widgets.get_mut(&node.id).unwrap();
        let pos = parent_pos + node.computed_pos;
        widget.rect = Some(Rect::from_min_max(pos, pos + node.computed_size));

        let style = widget.style.clone();
        let content_origin = pos + Vec2::new(style.padding.left, style.padding.top);
        let mut auto_child_pos = content_origin;
        
        for child_node in &node.children {
            let child_widget = &self.widgets[&child_node.id];
            let mut positioned_child = child_node.clone();

            match child_widget.style.position {
                PositionTyp::Auto => {
                    // Use automatic layout positioning
                    positioned_child.computed_pos = auto_child_pos - parent_pos;
                    self.apply_layout_recursive(&positioned_child, parent_pos);

                    // Update position for next auto child
                    match style.direction {
                        Axis::Y => {
                            auto_child_pos.y += child_node.computed_size.y + style.spacing;
                        }
                        Axis::X => {
                            auto_child_pos.x += child_node.computed_size.x + style.spacing;
                        }
                    }
                }
                PositionTyp::Absolute(abs_pos) => {
                    // Absolute positioning from screen origin
                    positioned_child.computed_pos = abs_pos;
                    self.apply_layout_recursive(&positioned_child, Vec2::ZERO);
                }
                PositionTyp::Relative(rel_pos) => {
                    // Relative to parent's content area
                    positioned_child.computed_pos = content_origin + rel_pos - parent_pos;
                    self.apply_layout_recursive(&positioned_child, parent_pos);
                }
            }
        }
    }

    // fn calculate_layout(&self, id: WidgetId, available_size: Vec2) -> LayoutNode {
    //     let widget = &self.widgets[&id];
    //     let style = &widget.style;

    //     let mut computed_size = Vec2::ZERO;

    //     match style.width {
    //         SizingTyp::Null => todo!(),
    //         SizingTyp::Fit => (),
    //         SizingTyp::Grow => computed_size.x = available_size.x,
    //         SizingTyp::Fixed(w) => computed_size.x = w,
    //         SizingTyp::Percent(p) => computed_size.x = available_size.x * p,
    //     }

    //     match style.height {
    //         SizingTyp::Null => todo!(),
    //         SizingTyp::Fit => (),
    //         SizingTyp::Grow => computed_size.y = available_size.y,
    //         SizingTyp::Fixed(w) => computed_size.y = w,
    //         SizingTyp::Percent(p) => computed_size.y = available_size.y * p,
    //     }

    //     let mut children_nodes = Vec::new();
    //     let mut content_size = Vec2::ZERO;
    //     let child_available = computed_size - style.padding.axis_sum();

    //     for &child_id in &widget.children {
    //         let child_node = self.calculate_layout(child_id, child_available);

    //         match style.direction {
    //             Axis::X => {
    //                 content_size.x = content_size.x.max(child_node.computed_size.x);
    //                 content_size.y += child_node.computed_size.y;
    //                 if !children_nodes.is_empty() {
    //                     content_size.y += style.spacing;
    //                 }
    //             }
    //             Axis::Y => {
    //                 content_size.x += child_node.computed_size.x;
    //                 content_size.y += content_size.y.max(child_node.computed_size.y);
    //                 if !children_nodes.is_empty() {
    //                     content_size.x += style.spacing;
    //                 }
    //             }
    //         }

    //         children_nodes.push(child_node);
    //     }

    //     if matches!(style.width, SizingTyp::Fit) {
    //         computed_size.x = content_size.x + style.padding.sum_along_axis(Axis::X) * 2.0;
    //     }

    //     if matches!(style.height, SizingTyp::Fit) {
    //         computed_size.y = content_size.y + style.padding.sum_along_axis(Axis::Y) * 2.0;
    //     }

    //     computed_size = computed_size.clamp(style.min_size, style.max_size);

    //     LayoutNode {
    //         id,
    //         computed_size,
    //         computed_pos: Vec2::ZERO,
    //         children: children_nodes,
    //     }
    // }

    pub fn apply_layout(&mut self, root: &LayoutNode) {
        self.apply_layout_recursive(root, Vec2::ZERO)
    }

    // pub fn apply_layout_recursive(&mut self, node: &LayoutNode, parent_pos: Vec2) {
    //     let widget = self.widgets.get_mut(&node.id).unwrap();
    //     let pos = parent_pos + node.computed_pos;
    //     widget.rect = Some(Rect::from_min_max(pos, pos + node.computed_size));

    //     let style = widget.style.clone();
    //     let mut child_pos = pos + Vec2::new(style.padding.left, style.padding.top); // TODO:
    //     // correct?
    //     for child_node in &node.children {
    //         let mut positioned_child = child_node.clone();
    //         positioned_child.computed_pos = child_pos - parent_pos;

    //         self.apply_layout_recursive(&positioned_child, parent_pos);

    //         match style.direction {
    //             Axis::X => {
    //                 child_pos.y += child_node.computed_size.y + style.spacing;
    //             }
    //             Axis::Y => {
    //                 child_pos.x += child_node.computed_size.x + style.spacing;
    //             }
    //         }
    //     }
    // }

    pub fn get_widget_signal(&self, widget_id: WidgetId) -> SignalFlags {
        let mut signals = SignalFlags::empty();

        if let Some(widget) = self.widgets.get(&widget_id) {
            if let Some(r) = widget.rect {
                let mouse_over = r.contains(self.mouse.mouse_pos);
                let was_mouse_over = r.contains(self.mouse.prev_pos);

                if mouse_over {
                    signals = SignalFlags::MOUSE_OVER;
                }
            }
        }
        signals
    }

    pub fn get_render_rects(&self) -> &[RectInst] {
        &self.output_rects
    }

    pub fn begin_at_w_size(&mut self, name: &str, pos: Vec2, size: Vec2) -> WidgetId {
        let id = self.begin_widget(name);

        self.set_widget_style(id, LayoutStyle::new().fixed(size.x, size.y).absolute(pos.x, pos.y).padding(8.0).spacing(4.0).direction(Axis::X));

        id
    }

    pub fn begin_window(&mut self) -> WidgetId {
        let id = self.begin_widget("window_root");

        self.set_widget_style(id, LayoutStyle::new().fixed(self.screen_size.x, self.screen_size.y).padding(8.0).spacing(4.0).direction(Axis::X));
        id
    }

    pub fn begin_grow(&mut self, name: &str) -> WidgetId {
        let id = self.begin_widget(name);

        self.set_widget_style(id, LayoutStyle::new().grow().padding(8.0).spacing(4.0).direction(Axis::X));

        id
    }

    pub fn begin_fit(&mut self, name: &str) -> WidgetId {
        let id = self.begin_widget(name);

        self.set_widget_style(id, LayoutStyle::new().fit().padding(8.0).spacing(4.0).direction(Axis::X));

        id
    }

    pub fn end_widget(&mut self) {
        if let Some(container_id) = self.current_id() {
            let available_size = self.screen_size;
            let layout = self.calculate_layout(container_id, available_size);
            self.layout_root = Some(layout);
        }
        self.pop_id();
    }

    pub fn button(&mut self, text: &str) -> SignalFlags {
        let parent = self.id_stack.last().copied();

        let id = self.begin_widget(text);

        if let Some(p_id) = parent {
            if let Some(p) = self.widgets.get_mut(&p_id) {
                p.children.push(id);
            }
        }

        self.set_widget_style(
            id,
            LayoutStyle::new()
                .fixed(100.0, 30.0)
                .pad_vert(8.0)
                .pad_hor(4.0),
        );
        self.set_widget_color(id, RGBA::BLUE);

        let signals = self.get_widget_signal(id);

        self.end_widget();
        signals
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PerButton<T>(pub [T; 3]);

impl<T> ops::Index<MouseButton> for PerButton<T> {
    type Output = T;

    fn index(&self, index: MouseButton) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl<T> ops::IndexMut<MouseButton> for PerButton<T> {
    fn index_mut(&mut self, index: MouseButton) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}


// #[derive(Clone, Copy, Default, PartialEq)]
// struct ButtonState {
//     pressed: bool,
//     double_press: bool,
//     press_start_pos: Vec2,
//     press_time: Option<Instant>,
//     last_press_time: Option<Instant>,
//     dragging: bool,
// }

#[derive(Clone, Copy, Default, PartialEq)]
struct ButtonState {
    pressed: bool,
    double_press: bool,
    press_start_pos: Vec2,
    press_time: Option<Instant>,
    last_release_time: Option<Instant>,
    last_press_was_short: bool,
    dragging: bool,
}

impl fmt::Display for ButtonState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.dragging {
            "dragging"
        } else if self.pressed {
            "pressed"
        } else {
            "released"
        };
        
        write!(f, "{}", status)?;
        
        if self.pressed {
            write!(f, " @({:.1}, {:.1})", self.press_start_pos.x, self.press_start_pos.y)?;
            if let Some(press_time) = self.press_time {
                write!(f, " {}ms", press_time.elapsed().as_millis())?;
            }
        }
        
        // Show if this is a double click
        if self.double_press {
            write!(f, " [DOUBLE]")?;
        }
        // if self.pressed {
        //     if let (Some(press_time), Some(last_press)) = (self.press_time, self.last_press_time) {
        //         if press_time > last_press && press_time.duration_since(last_press) < Duration::from_millis(300) {
        //             write!(f, " [DOUBLE]")?;
        //         }
        //     }
        // }
        
        Ok(())
    }
}

impl fmt::Debug for ButtonState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MouseState {
    pub mouse_pos: Vec2,
    prev_pos: Vec2,
    buttons: PerButton<ButtonState>,
    
    // Configuration
    double_click_time: Duration,
    drag_threshold: f32,
}

impl Default for MouseState {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            mouse_pos: Vec2::ZERO,
            prev_pos: Vec2::ZERO,
            buttons: PerButton::default(),
            double_click_time: Duration::from_millis(150),
            drag_threshold: 5.0,
        }
    }
    
    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.prev_pos = self.mouse_pos;
        self.mouse_pos = Vec2::new(x, y);
        
        // Update drag states for all pressed buttons
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            let state = &mut self.buttons[button];
            if state.pressed && !state.dragging {
                let distance = self.mouse_pos.distance(state.press_start_pos);
                if distance > self.drag_threshold {
                    state.dragging = true;
                }
            }
        }
    }

    pub fn set_button_press(&mut self, button: MouseButton, pressed: bool) {
        let state = &mut self.buttons[button];
        let was_pressed = state.pressed;

        if pressed && !was_pressed {
            let now = Instant::now();
            state.pressed = true;
            state.press_start_pos = self.mouse_pos;
            state.press_time = Some(now);
            state.dragging = false;

            state.double_press = if let Some(last_release) = state.last_release_time {
                now.duration_since(last_release) <= self.double_click_time && state.last_press_was_short
            } else {
                false
            };

        } else if !pressed && was_pressed {
            let now = Instant::now();
            state.pressed = false;
            state.dragging = false;

            if let Some(press_time) = state.press_time {
                let press_duration = now.duration_since(press_time);
                state.last_press_was_short = press_duration <= self.double_click_time;
            } else {
                state.last_press_was_short = false;
            }

            state.last_release_time = Some(now);
            state.press_time = None;
            state.double_press = false;
        }
    }

    
    // pub fn set_button_press(&mut self, button: MouseButton, pressed: bool) {
    //     let state = &mut self.buttons[button];
    //     let was_pressed = state.pressed;
        
    //     if pressed && !was_pressed {
    //         // Button just pressed
    //         state.pressed = true;
    //         state.press_start_pos = self.mouse_pos;
    //         state.press_time = Some(Instant::now());
    //         state.dragging = false;


    //         if let (Some(press_time), Some(last_press_time)) = (state.press_time, state.last_press_time) {
    //             // Check if this press happened soon after the last release
    //             if press_time.duration_since(last_press_time) < self.double_click_time {
    //                 state.double_press = true;
    //             }
    //         } else {
    //             state.double_press = false;
    //         }

    //     } else if !pressed && was_pressed {
    //         // Button just released
    //         state.pressed = false;
    //         state.dragging = false;
    //         state.last_press_time = state.press_time;
    //         state.press_time = None;
    //         state.double_press = false;

    //     }
    // }

    // Public getters
    pub fn drag_delta(&self) -> Vec2 {
        self.mouse_pos - self.prev_pos
    }

    // Button state queries
    pub fn pressed(&self, button: MouseButton) -> bool {
        self.buttons[button].pressed
    }
    
    pub fn dragging(&self, button: MouseButton) -> bool {
        self.buttons[button].dragging
    }
    
    pub fn drag_start(&self, button: MouseButton) -> Vec2 {
        self.buttons[button].press_start_pos
    }
    
    pub fn just_pressed(&self, button: MouseButton) -> bool {
        let state = &self.buttons[button];
        state.pressed && state.press_time.map_or(false, |t| t.elapsed() < Duration::from_millis(16))
    }
    
    pub fn double_clicked(&self, button: MouseButton) -> bool {
        self.buttons[button].double_press
    }
    
    
}

impl fmt::Display for MouseState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "MouseState {{")?;
        writeln!(f, "  pos: ({:.1}, {:.1})", self.mouse_pos.x, self.mouse_pos.y)?;
        
        let delta = self.drag_delta();
        if delta.x != 0.0 || delta.y != 0.0 {
            writeln!(f, "  delta: ({:.1}, {:.1})", delta.x, delta.y)?;
        }
        
        writeln!(f, "  left: {}", self.buttons[MouseButton::Left])?;
        writeln!(f, "  right: {}", self.buttons[MouseButton::Right])?;
        writeln!(f, "  middle: {}", self.buttons[MouseButton::Middle])?;
        write!(f, "}}")
    }
}
