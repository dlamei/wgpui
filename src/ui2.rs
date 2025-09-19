use std::{fmt, rc::Rc};

use bitflags::bitflags as flags;
use glam::Vec2;
use rustc_hash::FxHashMap;

use crate::{
    gpu::{WGPUHandle, Window},
    rect::Rect,
    ui::Placement,
    ui_draw::{self, DrawBuffer, DrawList as DrawCalls, Vertex},
    utils::RGBA,
};

pub struct State {
    pub mouse_pos: Vec2,
    pub panels: FxHashMap<Id, Panel>,
    pub draw: DrawCalls,

    pub draw_order: Vec<Id>,
    pub active_id: Id,
    pub active_window_id: Id,
    pub move_id: Id,

    pub frame_count: u64,
    pub draw_debug: bool,

    pub window: Window,
}

impl State {
    pub fn new(wgpu: WGPUHandle, window: Window) -> Self {
        Self {
            mouse_pos: Vec2::ZERO,
            panels: Default::default(),
            draw: ui_draw::DrawList::new(wgpu),
            active_id: Id::NULL,
            active_window_id: Id::NULL,
            move_id: Id::NULL,
            window,
            draw_order: Vec::new(),
            draw_debug: false,
            frame_count: 0,
        }
    }

    pub fn begin(&mut self, name: &str) {
        let mut newly_created = false;
        let mut id = self.find_panel_by_name(name);
        if id.is_null() {
            id = self.create_panel(name);
            newly_created = true;
        }

        let p = self.panels.get_mut(&id).unwrap();
        if newly_created {
            p.draw_order = self.draw_order.len();
            self.draw_order.push(id);
        }
        // let first_begin_this_frame = p.last_frame_used == self.frame_count;
        p.last_frame_used = self.frame_count;
    }

    pub fn create_panel(&mut self, name: &str) -> Id {
        let mut f = Panel::new(name);
        let id = f.id;
        f.frame_created = self.frame_count;
        self.panels.insert(id, f);
        id
    }

    pub fn find_panel_by_name(&self, name: &str) -> Id {
        let id = Id::from_str(name);
        if self.panels.contains_key(&id) {
            id
        } else {
            Id::NULL
        }
    }

    pub fn get_panel_mut(&mut self, id: Id) -> &mut Panel {
        self.panels.get_mut(&id).unwrap()
    }

    pub fn brin_panel_to_front(&mut self, panel_id: Id) {
        let p = &self.panels[&panel_id];
        assert!(p.tmp.root == panel_id);
        let curr_order = p.draw_order;
        assert!(self.draw_order[curr_order] == panel_id);
        if *self.draw_order.last().unwrap() == panel_id {
            return;
        }

        let new_order = self.draw_order.len() - 1;
        for i in curr_order..new_order {
            self.draw_order[i] = self.draw_order[i + 1];
            self.get_panel_mut(self.draw_order[i]).draw_order -= 1;
            assert!(self.panels[&self.draw_order[i]].draw_order == i);
        }

        self.draw_order[new_order] = panel_id;
        self.get_panel_mut(panel_id).draw_order = new_order;
    }

    pub fn start_ui(&mut self) {
        self.draw.clear();
        self.draw.screen_size = self.window.window_size();
    }

    pub fn end_ui(&mut self) {
        assert!(!self.panels.contains_key(&Id::NULL));
        self.build_draw_data();
        self.frame_count += 1;
    }

    pub fn build_draw_data(&mut self) {
        let panels = &mut self.panels;
        let draw_buff = &mut self.draw.draw_buffer;

        for (_, f) in panels {
            for cmd in &f.draw_list.cmd_buffer {
                let vtx = &f.draw_list.vtx_buffer[cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count];
                let idx = &f.draw_list.idx_buffer[cmd.idx_offset..cmd.idx_offset + cmd.idx_count];
                draw_buff.push(vtx, idx);
            }
        }

        if self.draw_debug {
            self.draw.as_wireframe(2.0);
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Panel {
    pub name: String,
    pub id: Id,
    pub id_stack: Vec<Id>,
    pub draw_order: usize,

    pub size: Vec2,
    pub pos: Vec2,

    pub draw_list: DrawList,

    pub last_frame_used: u64,
    pub frame_created: u64,

    pub tmp: TempPanelData,
}

impl Panel {
    pub fn new(name: &str) -> Self {
        Self {
            draw_order: 0,
            name: name.to_string(),
            id: Id::from_str(name),
            size: Vec2::ZERO,
            pos: Vec2::ZERO,
            frame_created: 0,
            last_frame_used: 0,
            draw_list: DrawList::new(),
            id_stack: Vec::new(),
            tmp: TempPanelData::default(),
        }
    }

    pub fn push_id(&mut self, id: Id) {
        self.id_stack.push(id);
    }

    pub fn pop_id(&mut self) -> Id {
        self.id_stack.pop().unwrap()
    }

    pub fn gen_id(&self, label: &str) -> Id {
        use std::hash::{Hash, Hasher};
        let seed = self.id_stack.last().unwrap_or(&self.id);
        let mut hasher = rustc_hash::FxHasher::default();
        seed.hash(&mut hasher);
        label.hash(&mut hasher);
        Id(hasher.finish().max(1))
    }

    pub fn clear_temp_data(&mut self) {
        self.tmp = TempPanelData::default();
    }

    pub fn draw_titlebar() {}
}

flags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct PanelFlags: u32 {
        const NONE = 0;
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TempPanelData {
    pub root: Id,
    pub parent: Id,
    pub child_frames: Vec<Id>,

    pub cursor_pos: Vec2,
    pub cursor_max_pos: Vec2,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct NextPanelData {
    pub pos: Option<Vec2>,
    pub placement: Placement,
    pub size: Option<Vec2>,
    pub min_size: Vec2,
    pub max_size: Vec2,
    pub content_size: Option<Vec2>,
}

impl NextPanelData {
}

// pub enum CondFlag {
//     Once,
//     Always,
// }

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(u64);

impl Id {
    pub const NULL: Id = Id(0);

    pub fn from_str(s: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        s.hash(&mut hasher);
        Self(hasher.finish().max(1))
    }

    pub fn is_null(&self) -> bool {
        *self == Self::NULL
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut n = self.0;
        if n == 0 {
            return write!(f, "0");
        }
        let mut buf = Vec::new();
        while n > 0 {
            let rem = (n % 36) as u8;
            let ch = if rem < 10 {
                b'0' + rem
            } else {
                b'A' + (rem - 10)
            };
            buf.push(ch);
            n /= 36;
        }
        buf.reverse();
        let s = std::str::from_utf8(&buf).unwrap();
        write!(f, "{}", s)
    }
}

/// A single draw command
#[derive(Debug, Clone, Copy, Default)]
pub struct DrawCmd {
    pub texture_id: u32,
    pub vtx_offset: usize,
    pub vtx_count: usize,
    pub idx_offset: usize,
    pub idx_count: usize,
}

/// The draw list itself: holds geometry and draw commands
#[derive(Debug, Clone)]
pub struct DrawList {
    pub vtx_buffer: Vec<Vertex>,
    pub idx_buffer: Vec<u32>,
    pub cmd_buffer: Vec<DrawCmd>,

    pub resolution: f32,
    pub path: Vec<Vec2>,
}

impl Default for DrawList {
    fn default() -> Self {
        Self {
            vtx_buffer: vec![],
            idx_buffer: vec![],
            cmd_buffer: vec![],
            resolution: 20.0,
            path: vec![],
        }
    }
}

impl DrawList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn curr_draw_cmd(&mut self) -> &mut DrawCmd {
        if self.cmd_buffer.is_empty() {
            self.cmd_buffer.push(DrawCmd::default())
        }
        self.cmd_buffer.last_mut().unwrap()
    }

    pub fn push_draw_cmd(&mut self) -> &mut DrawCmd {
        self.cmd_buffer.push(DrawCmd::default());
        let cmd = self.cmd_buffer.last_mut().unwrap();
        cmd.vtx_offset = self.vtx_buffer.len();
        cmd.idx_offset = self.idx_buffer.len();
        cmd
    }

    pub fn push_texture(&mut self, tex_id: u32) {
        let cmd = self.curr_draw_cmd();
        if cmd.texture_id != tex_id && tex_id != 0 {
            let cmd = self.push_draw_cmd();
            cmd.texture_id = tex_id;
        }
    }

    #[inline]
    pub fn push_vtx_idx(&mut self, vtx: &[Vertex], idx: &[u32]) {
        let cmd = self.curr_draw_cmd();
        let base = cmd.vtx_count as u32;

        self.vtx_buffer.extend_from_slice(vtx);
        self.idx_buffer.extend(idx.into_iter().map(|i| base + i));

        let cmd = self.curr_draw_cmd();
        cmd.vtx_count += vtx.len();
        cmd.idx_count += idx.len();
    }

    pub fn add_rect_impl(
        &mut self,
        min: Vec2,
        max: Vec2,
        color: RGBA,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: u32,
    ) {
        self.push_texture(tex_id);

        let vtx = [
            Vertex::new(min.with_y(max.y), color, uv_min.with_y(uv_max.y), tex_id),
            Vertex::new(max, color, uv_max, tex_id),
            Vertex::new(min.with_x(max.x), color, uv_min.with_x(uv_max.x), tex_id),
            Vertex::new(min, color, uv_min, tex_id),
        ];

        let idx = [0, 1, 2, 0, 2, 3];

        self.push_vtx_idx(&vtx, &idx);
    }

    pub fn add_rect_uv(&mut self, min: Vec2, max: Vec2, uv_min: Vec2, uv_max: Vec2, tex_id: u32) {
        self.add_rect_impl(min, max, RGBA::WHITE, uv_min, uv_max, tex_id);
    }

    pub fn add_rect_rounded(
        &mut self,
        min: Vec2,
        max: Vec2,
        fill: Option<RGBA>,
        outline: Option<(RGBA, f32)>,
        round: f32,
    ) {
        if round < 0.5 {
            if let Some(fill) = fill {
                self.add_rect_impl(min, max, fill, Vec2::ZERO, Vec2::ZERO, 0);
            }
            if let Some((col, width)) = outline {
                let pts = [min.with_y(max.x), max, max.with_x(max.y), min];
                let (vtx, idx) = tessellate_line(&pts, col, width, true);
                self.push_vtx_idx(&vtx, &idx);
            }
            return;
        }

        self.path_clear();
        self.path_rect(min, max, round);

        if let Some(fill) = fill {
            let (vtx, idx) = tessellate_convex_fill(&self.path, fill, true);
            self.push_vtx_idx(&vtx, &idx);
        }

        if let Some((col, width)) = outline {
            let (vtx, idx) = tessellate_line(&self.path, col, width, true);
            self.push_vtx_idx(&vtx, &idx);
        }
        self.path_clear();
    }

    pub fn path_clear(&mut self) {
        self.path.clear();
    }

    pub fn path_to(&mut self, p: Vec2) {
        self.path.push(p);
    }

    pub fn path_rect(&mut self, min: Vec2, max: Vec2, rad: f32) {
        const PI: f32 = std::f32::consts::PI;
        let rounded = rad != 0.0;

        self.path_to(Vec2::new(min.x + rad, min.y));
        self.path_to(Vec2::new(max.x - rad, min.y));
        if rounded {
            self.path_arc(
                Vec2::new(max.x - rad, min.y + rad),
                rad,
                PI / 2.0,
                -PI / 2.0,
            );
        }

        self.path_to(Vec2::new(max.x, min.y + rad));
        self.path_to(Vec2::new(max.x, max.y - rad));
        if rounded {
            self.path_arc(Vec2::new(max.x - rad, max.y - rad), rad, 0.0, -PI / 2.0);
        }

        self.path_to(Vec2::new(max.x - rad, max.y));
        self.path_to(Vec2::new(min.x + rad, max.y));
        if rounded {
            self.path_arc(
                Vec2::new(min.x + rad, max.y - rad),
                rad,
                -PI / 2.0,
                -PI / 2.0,
            );
        }

        self.path_to(Vec2::new(min.x, max.y - rad));
        self.path_to(Vec2::new(min.x, min.y + rad));
        if rounded {
            self.path_arc(Vec2::new(min.x + rad, min.y + rad), rad, PI, -PI / 2.0);
        }
    }

    pub fn path_arc(&mut self, center: Vec2, radius: f32, start_angle: f32, sweep_angle: f32) {
        if radius == 0.0 || sweep_angle == 0.0 {
            return;
        }

        // maximum angular step so chord length ≤ resolution
        let chord_step = 2.0
            * (self.resolution.max(0.1) / (2.0 * radius))
                .clamp(-1.0, 1.0)
                .asin();

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

    pub fn dist_lin_uv(
        &mut self,
        vert_start: usize,
        vert_end: usize,
        a: Vec2,
        b: Vec2,
        uv_a: Vec2,
        uv_b: Vec2,
        clamp: bool,
        tex_id: u32,
    ) {
        if vert_end <= vert_start || vert_end > self.vtx_buffer.len() {
            return;
        }

        let size = b - a;
        let uv_size = uv_b - uv_a;
        let scale = Vec2::new(
            if size.x != 0.0 {
                uv_size.x / size.x
            } else {
                0.0
            },
            if size.y != 0.0 {
                uv_size.y / size.y
            } else {
                0.0
            },
        );

        let (min_uv, max_uv) = if clamp {
            (uv_a.min(uv_b), uv_a.max(uv_b))
        } else {
            (uv_a, uv_b)
        };

        for vert in &mut self.vtx_buffer[vert_start..vert_end] {
            let mut uv = uv_a + (vert.pos - a) * scale;
            if clamp {
                uv.x = uv.x.clamp(min_uv.x, max_uv.x);
                uv.y = uv.y.clamp(min_uv.y, max_uv.y);
            }
            vert.uv = uv;
            vert.tex = tex_id;
        }
    }
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
