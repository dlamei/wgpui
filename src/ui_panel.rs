use cosmic_text as ctext;
use glam::{Mat4, UVec2, Vec2};
use std::{
    cell::{Ref, RefCell},
    fmt, // added fmt
    hash,
};
use wgpu::util::DeviceExt;

use crate::{
    core::{Axis, Dir},
    rect::Rect,
    ui::{DrawList, Id, IdMap, RootId},
};

macros::flags!(PanelFlag:
    NO_TITLEBAR,
    NO_FOCUS,
    NO_MOVE,
    NO_RESIZE,
    // TODO[NOTE]: what / when / how to use
    NO_INPUT,
    ONLY_MOVE_FROM_TITLEBAR,
    DRAW_H_SCROLLBAR,
    DRAW_V_SCROLLBAR,
    NO_DOCKING,
    DOCK_OVER,
    DONT_KEEP_SCROLLBAR_PAD,
    DONT_CLIP_CONTENT,

    USE_PARENT_DRAWLIST,
    USE_PARENT_CLIP,
    IS_CHILD,
);

#[derive(Clone, Debug)]
pub struct Panel {
    pub name: String,
    pub id: Id,
    /// set active_id to this id to start dragging the panel
    pub move_id: Id,
    pub flags: PanelFlag,

    pub root: Id,
    // pub nav_root: Id,
    pub parent_id: Id,
    pub child_id: Id,
    // TODO[NOTE]: not implemented yet
    pub children: Vec<Id>,

    pub dock_id: Id,

    pub padding: f32,
    pub titlebar_height: f32,
    pub title_handle_rect: Rect,

    pub scrollbar_width: f32,
    pub scrollbar_padding: f32,

    /// pos of the panel at draw time
    ///
    /// preserved over frames, does not include outline
    pub pos: Vec2,

    /// size of the panel at draw time
    ///
    /// preserved over frames
    pub size: Vec2,

    /// full size of the panel, i.e. from top left to bottom right corner, including the titlebar
    ///
    /// does not include outline
    pub full_size: Vec2,

    pub size_pre_dock: Vec2,

    pub full_rect: Rect,
    pub clip_rect: Rect,

    pub position_bounds: Rect,
    /// determines how the panels position is constraint
    ///
    /// true => panel cannot exit bounds \
    /// false => panel cannot fully exit bounds
    // TODO[CHECK]: not used
    pub clamp_position_to_bounds: bool,

    // TODO[CHECK]: currently only used when placing the items. i.e. cursor position is not offset
    // by scroll, scroll is only added when generating the item rectangle
    pub scroll: Vec2,

    // scroll will take on this value next frame
    // we do this because sizing is computed based on cursor positions and cursor positions
    // are affected by scrolling leading to a feedback loop
    // TODO[CHECK]: currently we only clamp the scroll at the next begin(), i.e. when applying to
    // scroll. otherwise panel does not scroll back automatically when resizing (why?)
    pub next_scroll: Vec2,
    pub indent: f32,

    /// size of the content of a panel
    ///
    /// computed based on cursor.content_start_pos and cursor.max_pos
    pub full_content_size: Vec2,
    pub explicit_size: Vec2,

    pub outline_offset: f32,

    pub min_size: Vec2,
    pub max_size: Vec2,

    pub draw_order: usize,

    pub last_frame_used: u64,
    pub frame_created: u64,
    pub close_pressed: bool,
    pub is_window_panel: bool,

    // try to not borrow outside of impl Panel { ... }
    pub drawlist: DrawList,
    pub drawlist_over: DrawList,
    pub id_stack: RefCell<Vec<Id>>,
    pub _cursor: RefCell<Cursor>,
    pub scroll_offset: f32,
}

// impl fmt::Debug for Panel {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         f.debug_struct("Panel")
//             .field("name", &self.name)
//             .field("id", &format!("{}", self.id))
//             .field("order", &self.draw_order)
//             .field("pos", &self.pos)
//             .field("size", &self.size)
//             .field("full_size", &self.size)
//             .field("content_size", &self.size)
//             .field("exeplicit_size", &self.explicit_size)
//             .field("min_size", &self.min_size)
//             .field("max_size", &self.max_size)
//             .finish_non_exhaustive()
//     }
// }

impl Panel {
    pub fn new(name: impl Into<String>) -> Self {
        let name: String = name.into();
        let id = Id::from_str(&name);
        Self {
            name,
            id,
            parent_id: Id::NULL,
            child_id: Id::NULL,
            children: vec![],
            dock_id: Id::NULL,
            root: Id::NULL,
            // nav_root: Id::NULL,
            flags: PanelFlag::NONE,
            padding: 0.0,
            scrollbar_width: 0.0,
            scrollbar_padding: 0.0,
            // spacing: 10.0,
            pos: Vec2::splat(30.0),
            scroll: Vec2::ZERO,
            next_scroll: Vec2::ZERO,
            indent: 0.0,

            full_content_size: Vec2::ZERO,
            full_size: Vec2::ZERO,
            full_rect: Rect::ZERO,
            clip_rect: Rect::ZERO,

            position_bounds: Rect::ZERO,
            clamp_position_to_bounds: false,

            explicit_size: Vec2::NAN,
            outline_offset: 0.0,
            draw_order: 0,
            // bg_color: RGBA::ZERO,
            titlebar_height: 0.0,
            title_handle_rect: Rect::ZERO,
            move_id: Id::NULL,
            size: Vec2::ZERO,
            size_pre_dock: Vec2::NAN,
            min_size: Vec2::ZERO,
            max_size: Vec2::ZERO,
            frame_created: 0,
            last_frame_used: 0,
            // draw_list: DrawList::new(),
            // id_stack: Vec::new(),
            close_pressed: false,
            is_window_panel: false,

            drawlist: DrawList::new(),
            drawlist_over: DrawList::new(),
            id_stack: RefCell::new(Vec::new()),
            _cursor: RefCell::new(Cursor::default()),
            scroll_offset: 0.0,
        }
    }

    pub fn panel_min_size(&self) -> Vec2 {
        let pad = 2.0 * self.padding;
        (self.title_handle_rect.size() + pad).max(self.min_size)
        // Vec2::new(pad, self.titlebar_height + pad).max(self.min_size)
    }

    pub fn panel_max_size(&self) -> Vec2 {
        self.max_size
    }

    pub fn panel_rect_with_outline(&self) -> Rect {
        let off = Vec2::splat(self.outline_offset);
        Rect::from_min_max(self.pos - off, self.pos + self.size + off)
    }

    pub fn panel_rect(&self) -> Rect {
        Rect::from_min_max(self.pos, self.pos + self.size)
    }

    pub fn needs_scrollbars(&self) -> (bool, bool) {
        let base = self.visible_content_start_pos();
        let full = Rect::from_min_max(base, base + self.full_content_size);

        let max_view_full = self.pos + self.size - Vec2::splat(self.padding);
        let min_view = base;
        let scrollbar_space = self.scrollbar_width + self.scrollbar_padding;

        let full_width = full.width();
        let full_height = full.height();
        let view_width = max_view_full.x - min_view.x;
        let view_height = max_view_full.y - min_view.y;

        // Check all 4 possible states explicitly

        // State 1: Neither scrollbar
        let w_none = view_width;
        let h_none = view_height;
        let valid_none = full_width <= w_none && full_height <= h_none;

        // State 2: Only horizontal scrollbar
        let w_h = view_width;
        let h_h = view_height - scrollbar_space;
        let valid_h = full_width <= w_h && full_height > h_h && full_height <= view_height;

        // State 3: Only vertical scrollbar
        let w_v = view_width - scrollbar_space;
        let h_v = view_height;
        let valid_v = full_width > w_v && full_width <= view_width && full_height <= h_v;

        // State 4: Both scrollbars
        let w_both = view_width - scrollbar_space;
        let h_both = view_height - scrollbar_space;
        let valid_both = full_width > w_both || full_height > h_both;

        // Return the first valid state in priority order
        let (x, y) = if valid_none {
            (false, false)
        } else if valid_h {
            (true, false)
        } else if valid_v {
            (false, true)
        } else {
            (true, true)
        };

        (
            x || !self.flags.has(PanelFlag::DONT_KEEP_SCROLLBAR_PAD)
                && self.flags.has(PanelFlag::DRAW_H_SCROLLBAR),
            y || !self.flags.has(PanelFlag::DONT_KEEP_SCROLLBAR_PAD)
                && self.flags.has(PanelFlag::DRAW_V_SCROLLBAR),
        )
    }

    pub(crate) fn scroll_min(&self) -> Vec2 {
        // use the unscrolled content origin (cursor.content_start_pos) so bounds don't depend on self.scroll
        let origin = self._cursor.borrow().content_start_pos;
        let full_end = origin + self.full_content_size;
        let visible_end = self.visible_content_end_pos();

        let x = (visible_end.x - full_end.x).min(0.0);
        let y = (visible_end.y - full_end.y).min(0.0);

        Vec2::new(x, y)
    }

    pub(crate) fn scrolling_past_bounds(&self, delta: Vec2) -> bool {
        let scroll = (self.scroll + delta)
            .min(self.scroll_max())
            .max(self.scroll_min());

        scroll == self.scroll
    }

    // Replace scroll_max() with:
    pub fn scroll_max(&self) -> Vec2 {
        Vec2::ZERO
        // let origin = self._cursor.borrow().content_start_pos;
        // let visible_start = self.visible_content_start_pos(); // this is the visible content origin

        // let x = (visible_start.x - origin.x).max(0.0);
        // let y = (visible_start.y - origin.y).max(0.0);

        // Vec2::new(x, y)
    }

    // fn scroll_min(&self) -> Vec2 {
    //     let full = self.content_start_pos();
    //     let visible = self.visible_content_start_pos();

    //     let x = if visible.x > full.x {
    //         full.x - visible.x
    //     } else {
    //         0.0
    //     };

    //     let y = if visible.y > full.y {
    //         full.y - visible.y
    //     } else {
    //         0.0
    //     };

    //     Vec2::new(x, y)
    // }

    // pub fn scroll_max(&self) -> Vec2 {
    //     let full = self.content_end_pos();
    //     let visible = self.visible_content_end_pos();

    //     let x = if visible.x < full.x {
    //         visible.x - full.x
    //     } else {
    //         0.0
    //     };

    //     let y = if visible.y < full.y {
    //         visible.y - full.y
    //     } else {
    //         0.0
    //     };

    //     Vec2::new(x, y)
    // }

    // pub fn set_scroll(&mut self, scroll: Vec2) {
    //     let min = self.scroll_min();
    //     let max = self.scroll_max();
    //     self.next_scroll = scroll.min(max).max(min);
    // }

    pub fn set_scroll(&mut self, delta: Vec2) {
        // self.next_scroll = self.scroll + delta;
        self.next_scroll = self.scroll + delta;
        // self.set_scroll(self.scroll + delta);
    }

    pub fn visible_content_rect(&self) -> Rect {
        Rect::from_min_max(
            self.visible_content_start_pos(),
            self.visible_content_end_pos(),
        )
    }

    pub fn full_content_rect(&self) -> Rect {
        Rect::from_min_max(self.content_start_pos(), self.content_end_pos())
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.drawlist.current_clip_rect()
    }

    pub fn push_id(&self, id: Id) {
        self.id_stack.borrow_mut().push(id);
    }

    pub fn pop_id(&self) -> Id {
        self.id_stack.borrow_mut().pop().unwrap()
    }

    pub fn gen_id(&self, label: impl hash::Hash) -> Id {
        use std::hash::{Hash, Hasher};
        let ids = &self.id_stack.borrow();
        let seed = ids.last().expect("at least self.id should be in the stack");
        let mut hasher = ahash::AHasher::default();
        seed.hash(&mut hasher);
        label.hash(&mut hasher);
        Id(hasher.finish().max(1))
    }

    pub fn clear_temp_data(&mut self) {
        self.drawlist.clear();
        self.drawlist_over.clear();
        self.root = Id::NULL;
    }

    pub fn id_stack_ref(&self) -> Ref<'_, Vec<Id>> {
        self.id_stack.borrow()
    }

    pub fn set_cursor_pos(&self, pos: Vec2) {
        self._cursor.borrow_mut().pos = pos;
    }

    pub fn init_content_cursor(&self, pos: Vec2) {
        let mut c = self._cursor.borrow_mut();
        c.content_start_pos = pos;
        c.pos = pos;
        c.max_pos = pos;
    }

    pub fn cursor_pos(&self) -> Vec2 {
        // self._cursor.borrow().pos.round()
        self._cursor.borrow().pos + self.scroll
    }

    pub fn cursor_max_pos(&self) -> Vec2 {
        self._cursor.borrow().max_pos.round()
        // self._cursor.borrow().max_pos + self.scroll
    }

    pub fn visible_content_end_pos(&self) -> Vec2 {
        // self.visible_content_start_pos() + self.size()
        let mut max = self.pos + self.size - Vec2::splat(self.padding);
        let (x_scroll, y_scroll) = self.needs_scrollbars();

        let flags = self.flags;
        if x_scroll {
            max.x = max.x - self.scrollbar_width - self.scrollbar_padding;
        }

        if y_scroll {
            max.y = max.y - self.scrollbar_width - self.scrollbar_padding;
        }

        max.round()
    }

    pub fn titlebar_rect(&self) -> Rect {
        if self.flags.has(PanelFlag::NO_TITLEBAR) {
            return Rect::ZERO;
        } else {
            Rect::from_min_size(self.pos, Vec2::new(self.size.x, self.titlebar_height))
        }
    }

    pub fn content_end_pos(&self) -> Vec2 {
        let pos = self._cursor.borrow().content_start_pos + self.full_content_size; // + self.scroll; // + self.scroll + self.full_content_size
        pos.round()
    }

    pub fn content_start_pos(&self) -> Vec2 {
        let pos = self._cursor.borrow().content_start_pos; // + self.scroll;
        pos.round()
    }

    pub fn visible_content_start_pos(&self) -> Vec2 {
        let pos = self.pos + Vec2::new(0.0, self.titlebar_height) + Vec2::splat(self.padding);
        pos.round()
    }

    // TODO[CHECK]: when / how / what does this exactly do
    /// sets the new panel position
    ///
    /// will also update the cursor so we dont get items lagging behind
    pub fn move_panel_to(&mut self, pos: Vec2) {
        let pos = pos.round();
        let mut c = self._cursor.get_mut();
        let prev_pos = self.pos;
        self.pos = pos;

        // TODO[CHECK]: scroll?
        let pos_d = c.pos - prev_pos;
        let max_pos_d = c.max_pos - prev_pos;
        let content_start_pos_d = c.content_start_pos - prev_pos;

        c.pos = pos_d + pos;
        c.max_pos = max_pos_d + pos;
        c.content_start_pos = content_start_pos_d + pos;
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub pos: Vec2,
    pub max_pos: Vec2,
    pub content_start_pos: Vec2,
    pub pos_prev_line: Vec2,
    pub line_height: f32,
    pub prev_line_height: f32,
    pub is_same_line: bool,

    pub indent: f32,
}

macros::flags!(DockNodeFlag:
    NO_BRING_TO_FRONT,
    ALLOW_SINGLE_LEAF,
);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockNodeKind {
    Split {
        // first id is top / left, second id is bottom / right depending on axis
        children: [Id; 2],
        axis: Axis,
        ratio: f32,
    },
    Leaf,
}

impl DockNodeKind {
    pub fn is_split(&self) -> bool {
        matches!(self, DockNodeKind::Split { .. })
    }

    pub fn is_leaf(&self) -> bool {
        matches!(self, DockNodeKind::Leaf)
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DockNode {
    pub label: Option<&'static str>,
    pub id: Id,
    pub parent_id: Id,
    pub kind: DockNodeKind,
    pub rect: Rect,
    pub panel_id: Id,
    pub flags: DockNodeFlag,
}

impl fmt::Display for DockNodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DockNodeKind::Leaf => write!(f, "Leaf"),
            DockNodeKind::Split {
                children,
                axis,
                ratio,
            } => {
                let axis_str = match axis {
                    Axis::X => "X",
                    Axis::Y => "Y",
                };
                write!(f, "Split[{axis_str}, {}, {}]", children[0], children[1],)
            }
        }
    }
}

impl fmt::Display for DockNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DockNode {{ {}id: {}, kind: {}, panel: {}, {}, parent: {} }}",
            self.label.map(|l| l.to_string() + ", ").unwrap_or_default(),
            self.id,
            self.kind,
            self.panel_id,
            self.flags,
            self.parent_id,
        )
    }
}

#[derive(Debug, Clone)]
pub struct DockTree {
    pub nodes: IdMap<DockNode>,
}

impl fmt::Display for DockTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "DockTree {{")?;

        for (_, n) in &self.nodes {
            writeln!(f, "{}", n)?;
        }
        write!(f, "}}")
    }
}

impl DockTree {
    pub fn new() -> Self {
        Self {
            nodes: IdMap::new(),
            // roots: vec![],
        }
    }

    pub fn recompute_rects(&mut self, node_id: Id, root_rect: Rect) {
        let n = &mut self.nodes[node_id];
        n.rect = root_rect;

        match n.kind {
            DockNodeKind::Split {
                children: [n1, n2],
                axis,
                ratio,
            } => {
                let mut n1_size = root_rect.size();
                let mut n2_size = root_rect.size();
                n1_size[axis as usize] *= ratio;
                n2_size[axis as usize] *= 1.0 - ratio;

                let mut n1_rect = root_rect;
                n1_rect.set_size(n1_size);

                let mut n2_rect = root_rect;
                // shift min of n2 along the split axis by n1's size in that axis
                n2_rect.min[axis as usize] += n1_size[axis as usize];
                n2_rect.set_size(n2_size);

                self.recompute_rects(n1, n1_rect);
                self.recompute_rects(n2, n2_rect);
            }
            DockNodeKind::Leaf => {
                n.rect = root_rect;
            }
        }
    }

    pub fn add_root_ex(&mut self, rect: Rect, panel_id: Id, flags: DockNodeFlag) -> Id {
        let id = Id::from_hash(&panel_id);
        let node = DockNode {
            label: None,
            id,
            kind: DockNodeKind::Leaf,
            rect,
            parent_id: Id::NULL,
            panel_id,
            flags,
            // panel: panel_id,
        };

        self.nodes.insert(id, node);
        // self.roots.push(id);
        id
    }

    pub fn add_root(&mut self, rect: Rect, panel_id: Id) -> Id {
        self.add_root_ex(rect, panel_id, DockNodeFlag::NONE)
    }

    pub fn get_neighbor(&self, id: Id, dir: Dir) -> Id {
        fn descend_to_leaf(tree: &DockTree, mut node_id: Id, child_idx: usize) -> Id {
            loop {
                let node = &tree.nodes[node_id];
                match &node.kind {
                    DockNodeKind::Split { children, .. } => {
                        node_id = children[child_idx];
                        // node_id = if prefer_rightmost {
                        //     children[1]
                        // } else {
                        //     children[0]
                        // };
                    }
                    DockNodeKind::Leaf => return node_id,
                }
            }
        }

        let (target_axis, c_idx) = match dir {
            Dir::N => (Axis::Y, 1),
            Dir::E => (Axis::X, 0),
            Dir::S => (Axis::Y, 0),
            Dir::W => (Axis::X, 1),
            _ => panic!(),
        };

        let mut cur = id;
        while !cur.is_null() {
            let node = &self.nodes[cur];
            let parent = node.parent_id;
            if parent.is_null() {
                break;
            }
            let pnode = &self.nodes[parent];
            if let DockNodeKind::Split { children, axis, .. } = &pnode.kind {
                let child_index = if children[0] == cur {
                    0
                } else if children[1] == cur {
                    1
                } else {
                    cur = parent;
                    continue;
                };
                if *axis == target_axis && child_index == c_idx {
                    let sibling = children[1 - child_index];
                    return descend_to_leaf(self, sibling, c_idx);
                }
            }
            cur = parent;
        }
        Id::NULL
    }

    pub fn get_neighbors(&self, id: Id) -> [Id; 4] {
        [Dir::N, Dir::E, Dir::S, Dir::W].map(|dir| self.get_neighbor(id, dir))
    }

    pub fn set_split_ratio(&mut self, split_id: Id, new_ratio: f32) {
        assert!(self.nodes[split_id].kind.is_split());

        // set root ratio and immediate children rects
        let (root_children, root_axis) = match self.nodes[split_id].kind {
            DockNodeKind::Split { children, axis, .. } => (children, axis),
            _ => return,
        };
        if let DockNodeKind::Split { ratio, .. } = &mut self.nodes[split_id].kind {
            *ratio = new_ratio
        }

        let ax = root_axis as usize;
        let split_rect = self.nodes[split_id].rect;
        let mut n1_size = split_rect.size();
        let mut n2_size = split_rect.size();
        n1_size[ax] *= new_ratio;
        n2_size[ax] *= 1.0 - new_ratio;

        let mut n1_rect = split_rect;
        n1_rect.set_size(n1_size);

        let mut n2_rect = split_rect;
        n2_rect.min[ax] += n1_size[ax];
        n2_rect.set_size(n2_size);

        self.nodes[root_children[0]].rect = n1_rect;
        self.nodes[root_children[1]].rect = n2_rect;

        // single-pass descent: for every split encountered, compute its new ratio from current child rects,
        // apply it, set its children rects, and continue.
        let mut stack = vec![root_children[0], root_children[1]];
        while let Some(node_id) = stack.pop() {
            let kind = self.nodes[node_id].kind;
            if let DockNodeKind::Split { children, axis, .. } = kind {
                let ax = axis as usize;

                // read current rects (copies) to avoid borrow conflicts
                let node_rect = self.nodes[node_id].rect;
                let left_rect = self.nodes[children[0]].rect;

                // absolute split position is the max of the left/top child along axis
                let split_pos = left_rect.max[ax];
                let size = node_rect.size()[ax];
                let computed = (split_pos - node_rect.min[ax]) / size;

                if let DockNodeKind::Split { ratio, .. } = &mut self.nodes[node_id].kind {
                    *ratio = computed;
                }

                // apply children rects based on the newly computed ratio
                let mut c1_size = node_rect.size();
                let mut c2_size = c1_size;
                c1_size[ax] *= computed;
                c2_size[ax] *= 1.0 - computed;

                let mut c1_rect = node_rect;
                c1_rect.set_size(c1_size);

                let mut c2_rect = node_rect;
                c2_rect.min[ax] += c1_size[ax];
                c2_rect.set_size(c2_size);

                self.nodes[children[0]].rect = c1_rect;
                self.nodes[children[1]].rect = c2_rect;

                stack.push(children[0]);
                stack.push(children[1]);
            }
        }
    }

    pub fn get_split_range(&self, id: Id) -> (f32, f32) {
        // find axis and children for this split
        let (axis, children) = match self.nodes[id].kind {
            DockNodeKind::Split { axis, children, .. } => (axis as usize, children),
            _ => return (0.0, 0.0),
        };

        // collect all leaf rects under a subtree
        fn collect_leaf_rects(tree: &DockTree, start: Id, out: &mut Vec<Rect>) {
            match tree.nodes[start].kind {
                DockNodeKind::Leaf => out.push(tree.nodes[start].rect),
                DockNodeKind::Split { children, .. } => {
                    collect_leaf_rects(tree, children[0], out);
                    collect_leaf_rects(tree, children[1], out);
                }
            }
        }

        // left subtree: children[0], right subtree: children[1]
        let mut left_leaf_rects: Vec<Rect> = Vec::new();
        let mut right_leaf_rects: Vec<Rect> = Vec::new();
        collect_leaf_rects(self, children[0], &mut left_leaf_rects);
        collect_leaf_rects(self, children[1], &mut right_leaf_rects);

        // left limit: split position must be >= max(left_leaf.rect.min[axis])
        let left_limit = left_leaf_rects
            .iter()
            .map(|r| r.min[axis])
            .fold(f32::NEG_INFINITY, f32::max);

        // right limit: split position must be <= min(right_leaf.rect.max[axis])
        let right_limit = right_leaf_rects
            .iter()
            .map(|r| r.max[axis])
            .fold(f32::INFINITY, f32::min);

        // clamp to containing rect of this split node as a safety net
        let node_rect = self.nodes[id].rect;
        let node_min = node_rect.min[axis];
        let node_max = node_rect.max[axis];

        let left_limit = left_limit.max(node_min);
        let right_limit = right_limit.min(node_max);

        (left_limit, right_limit)
    }

    // pub fn get_split_range(&self, id: Id) -> f32 {
    //     fn descend_to_leaf(tree: &DockTree, mut node_id: Id, child_idx: usize) -> Id {
    //         loop {
    //             let node = &tree.nodes[node_id];
    //             match &node.kind {
    //                 DockNodeKind::Split { children, .. } => {
    //                     node_id = children[child_idx];
    //                     // node_id = if prefer_rightmost {
    //                     //     children[1]
    //                     // } else {
    //                     //     children[0]
    //                     // };
    //                 }
    //                 DockNodeKind::Leaf => return node_id,
    //             }
    //         }
    //     }

    //     let split_node = self.nodes[id];
    //     let DockNodeKind::Split { children, axis, .. } = split_node.kind else {
    //         panic!()
    //     };

    //     let c1 = descend_to_leaf(self, children[0], 1);
    //     let c2 = descend_to_leaf(self, children[1], 0);
    //     let r1 = self.nodes[c1].rect;
    //     let r2 = self.nodes[c2].rect;

    //     match axis {
    //         Axis::X => r2.right() - r1.left(),
    //         Axis::Y => r2.bottom() - r1.top(),
    //     }
    // }

    pub fn get_split_node(&self, id: Id, dir: Dir) -> Id {
        let (target_axis, c_idx) = match dir {
            Dir::N => (Axis::Y, 1),
            Dir::E => (Axis::X, 0),
            Dir::S => (Axis::Y, 0),
            Dir::W => (Axis::X, 1),
            _ => panic!(),
        };

        let mut cur = id;
        while !cur.is_null() {
            let node = &self.nodes[cur];
            let parent = node.parent_id;
            if parent.is_null() {
                break;
            }
            let pnode = &self.nodes[parent];
            if let DockNodeKind::Split { children, axis, .. } = &pnode.kind {
                let child_index = if children[0] == cur {
                    0
                } else if children[1] == cur {
                    1
                } else {
                    cur = parent;
                    continue;
                };
                if *axis == target_axis && child_index == c_idx {
                    return parent;
                }
            }
            cur = parent;
        }
        Id::NULL
    }

    pub fn get_leafs(&self, mut node_id: Id) -> Vec<Id> {
        let root = self.get_root(node_id);
        let mut out = Vec::new();
        let mut stack = vec![root];

        while let Some(cur) = stack.pop() {
            match &self.nodes[cur].kind {
                DockNodeKind::Split { children, .. } => {
                    stack.push(children[1]);
                    stack.push(children[0]);
                }
                DockNodeKind::Leaf => {
                    out.push(cur);
                }
            }
        }

        out
    }

    pub fn get_tree(&self, mut node_id: Id) -> Vec<Id> {
        let root = self.get_root(node_id);
        let mut out = Vec::new();
        let mut stack = vec![root];

        while let Some(cur) = stack.pop() {
            out.push(cur);
            match &self.nodes[cur].kind {
                DockNodeKind::Split { children, .. } => {
                    stack.push(children[1]);
                    stack.push(children[0]);
                }
                DockNodeKind::Leaf => {}
            }
        }

        out
    }

    pub fn get_root(&self, mut node_id: Id) -> Id {
        let mut node = &self.nodes[node_id];
        while !node.parent_id.is_null() {
            node = &self.nodes[node.parent_id];
        }

        node.id
    }

    pub fn merge_nodes(&mut self, target_id: Id, docking_id: Id, mut ratio: f32, dir: Dir) -> Id {
        assert!(ratio <= 1.0 && ratio > 0.0);
        assert!(self.nodes[target_id].kind.is_leaf());

        let original = self.nodes[target_id].clone();
        let parent_rect = original.rect;

        // create a new id for the existing content that used to live in `target_id`
        let new_old_id = Id::from_hash(&(original.id.0 + 1));

        let mut old_leaf = original;
        old_leaf.id = new_old_id;
        old_leaf.parent_id = target_id;
        old_leaf.kind = DockNodeKind::Leaf;
        old_leaf.rect = Rect::NAN;

        match dir {
            Dir::E | Dir::S => ratio = 1.0 - ratio,
            _ => (),
        }

        // set docking node's parent to the target (assumes docking node is a root or otherwise handled by caller)
        self.nodes[docking_id].parent_id = target_id;

        let axis = match dir {
            Dir::N | Dir::S => Axis::Y,
            Dir::E | Dir::W => Axis::X,
            _ => unreachable!(),
        };

        // children ordering: children[0] is top/left, children[1] is bottom/right
        let (c0, c1) = match dir {
            Dir::W | Dir::N => (docking_id, new_old_id), // docking goes to left/top
            _ => (new_old_id, docking_id),               // docking goes to right/bottom
        };

        // replace target node kind with a split and insert the preserved old leaf
        self.nodes[target_id].panel_id = Id::NULL;
        self.nodes[target_id].kind = DockNodeKind::Split {
            children: [c0, c1],
            axis,
            ratio,
        };

        self.nodes.insert(new_old_id, old_leaf);

        self.recompute_rects(target_id, parent_rect);
        new_old_id
    }

    // returns id of the other node if only two are left and the dock tree is removed
    pub fn undock_node(
        &mut self,
        node_id: Id,
        panels: &mut IdMap<Panel>,
        draworder: &mut Vec<RootId>,
    ) {
        use DockNodeKind as DNK;
        let n = self.nodes[node_id];
        assert!(panels[n.panel_id].dock_id == node_id);
        assert!(n.kind.is_leaf());

        // undock this panel
        // panels[n.panel_id].dock_id = Id::NULL;
        let p = &mut panels[n.panel_id];
        p.dock_id = Id::NULL;
        p.size = p.size_pre_dock;
        p.size_pre_dock = Vec2::NAN;

        let dock_root = self.get_root(n.id);

        let init_new_root_panel = |id: Id, draworder: &mut Vec<RootId>| {
            let idx = draworder
                .iter()
                .position(|&i| i == RootId::Dock(dock_root))
                .unwrap();
            draworder.insert(idx + 1, RootId::Panel(id));
        };

        let remove_dock_root = |draworder: &mut Vec<RootId>| {
            let idx = draworder
                .iter()
                .position(|&i| i == RootId::Dock(dock_root))
                .unwrap();
            draworder.remove(idx);
        };

        let replace_dock_root = |new_root: Id, draworder: &mut Vec<RootId>| {
            let idx = draworder
                .iter()
                .position(|&i| i == RootId::Dock(dock_root))
                .unwrap();
            draworder[idx] = RootId::Dock(new_root);
        };

        init_new_root_panel(n.panel_id, draworder);

        // if it's a root leaf just remove it
        if n.parent_id.is_null() {
            self.nodes.remove(n.id);
            remove_dock_root(draworder);
            return;
        }

        let parent_id = n.parent_id;
        let parent = self.nodes[parent_id];
        match parent.kind {
            DNK::Split { children, .. } => {
                let rem_id = if children[0] == node_id {
                    children[1]
                } else {
                    assert!(children[1] == node_id);
                    children[0]
                };

                let grand_id = parent.parent_id;

                if grand_id.is_null() {
                    match self.nodes[rem_id].kind {
                        DNK::Leaf => {
                            // If the parent root had ALLOW_SINGLE_LEAF, promote the remaining leaf to be the new root.
                            if self.nodes[parent_id].flags.has(DockNodeFlag::ALLOW_SINGLE_LEAF) {
                                // promote rem_id to be the new root, inherit parent's flags and rect
                                self.nodes[rem_id].parent_id = Id::NULL;
                                // merge flags so the new root retains parent's special flags
                                self.nodes[rem_id].flags = self.nodes[rem_id].flags | parent.flags;
                                // remove the old split and the undocked node, then recompute rects from promoted root
                                let parent_rect = parent.rect;
                                self.nodes.remove(n.id);
                                self.nodes.remove(parent_id);
                                self.recompute_rects(rem_id, parent_rect);
                                replace_dock_root(rem_id, draworder);
                            } else {
                                // two-leaf root: remove both children and the parent, undock sibling too
                                let rem_panel_id = self.nodes[rem_id].panel_id;
                                panels[rem_panel_id].dock_id = Id::NULL;

                                // set sibling to size of the root
                                let parent_rect = parent.rect;
                                panels[rem_panel_id].size = parent_rect.size();
                                panels[rem_panel_id].pos = parent_rect.min;

                                self.nodes.remove(rem_id);
                                self.nodes.remove(n.id);
                                self.nodes.remove(parent_id);

                                init_new_root_panel(rem_panel_id, draworder);
                                remove_dock_root(draworder);
                            }
                        }
                        DNK::Split { .. } => {
                            // promote rem_id (a subtree) to be the new root
                            self.nodes[rem_id].parent_id = Id::NULL;

                            self.nodes.remove(n.id);
                            self.nodes.remove(parent_id);

                            let parent_rect = parent.rect;
                            // self.nodes[rem_id].rect = parent_rect;
                            self.recompute_rects(rem_id, parent_rect);
                            replace_dock_root(rem_id, draworder);
                        }
                    }
                    // match self.nodes[rem_id].kind {

                    //     DNK::Leaf => {
                    //         if !self.nodes[rem_id]
                    //             .flags
                    //             .has(DockNodeFlag::ALLOW_SINGLE_LEAF)
                    //         {
                    //             // two-leaf root: remove both children and the parent, undock sibling too
                    //             let rem_panel_id = self.nodes[rem_id].panel_id;
                    //             panels[rem_panel_id].dock_id = Id::NULL;

                    //             // set sibling to size of the root
                    //             let parent_rect = parent.rect;
                    //             panels[rem_panel_id].size = parent_rect.size();
                    //             panels[rem_panel_id].pos = parent_rect.min;

                    //             self.nodes.remove(rem_id);
                    //             self.nodes.remove(n.id);
                    //             self.nodes.remove(parent_id);

                    //             init_new_root_panel(rem_panel_id, draworder);
                    //             remove_dock_root(draworder);
                    //         }
                    //     }
                    //     DNK::Split { .. } => {
                    //         // promote rem_id to be the new root
                    //         self.nodes[rem_id].parent_id = Id::NULL;

                    //         self.nodes.remove(n.id);
                    //         self.nodes.remove(parent_id);

                    //         let parent_rect = parent.rect;
                    //         // self.nodes[rem_id].rect = parent_rect;
                    //         self.recompute_rects(rem_id, parent_rect);
                    //         replace_dock_root(rem_id, draworder);
                    //     }
                    // }
                } else {
                    // replace parent with rem_id in grandparent
                    {
                        let gp = &mut self.nodes[grand_id];
                        match &mut gp.kind {
                            DNK::Split { children, .. } => {
                                if children[0] == parent_id {
                                    children[0] = rem_id;
                                } else {
                                    assert!(children[1] == parent_id);
                                    children[1] = rem_id;
                                }
                            }
                            DNK::Leaf => unreachable!(),
                        }
                    }

                    self.nodes[rem_id].parent_id = grand_id;

                    self.nodes.remove(n.id);
                    self.nodes.remove(parent_id);

                    let parent_rect = parent.rect;
                    self.recompute_rects(rem_id, parent_rect);
                }
            }
            DNK::Leaf => unreachable!(),
        }
    }

    pub fn resize(&mut self, node_id: Id, dir: Dir, new_size: Rect) {}

    pub fn split_node2(&mut self, node_id: Id, mut ratio: f32, dir: Dir) -> (Id, Id) {
        assert!(ratio <= 1.0 && ratio > 0.0);
        let node = &self.nodes[node_id];
        assert!(node.kind == DockNodeKind::Leaf);
        let mut n1_id = Id::from_hash(&(node.id.0 + 0));
        let mut n2_id = Id::from_hash(&(node.id.0 + 1));

        match dir {
            Dir::E | Dir::S => ratio = 1.0 - ratio,
            _ => (),
        }

        let parent_rect = node.rect;

        let n1 = DockNode {
            label: None,
            id: n1_id,
            kind: DockNodeKind::Leaf,
            rect: Rect::NAN,
            parent_id: node_id,
            panel_id: Id::NULL,
            flags: DockNodeFlag::NONE,
        };

        let n2 = DockNode {
            label: None,
            id: n2_id,
            kind: DockNodeKind::Leaf,
            rect: Rect::NAN,
            parent_id: node_id,
            panel_id: Id::NULL,
            flags: DockNodeFlag::NONE,
        };

        let axis = match dir {
            Dir::N | Dir::S => Axis::Y,
            Dir::E | Dir::W => Axis::X,
            _ => unreachable!(),
        };

        self.nodes[node_id].panel_id = Id::NULL;
        self.nodes[node_id].kind = DockNodeKind::Split {
            children: [n1_id, n2_id],
            axis,
            ratio,
        };

        self.nodes.insert(n1_id, n1);
        self.nodes.insert(n2_id, n2);

        self.recompute_rects(node_id, parent_rect);

        match dir {
            Dir::W | Dir::N => std::mem::swap(&mut n1_id, &mut n2_id),
            _ => (),
        }

        (n1_id, n2_id)
    }
}
