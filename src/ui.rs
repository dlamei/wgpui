use cosmic_text as ctext;
use glam::{Mat4, UVec2, Vec2};
use std::{
    cell::{Ref, RefCell}, char::MAX, fmt, hash, rc::Rc
};
use wgpu::util::DeviceExt;

use crate::{
    Vertex as VertexTyp,
    core::{
        ArrVec, Axis, DataMap, Dir, HashMap, HashSet, Instant, RGBA, id_type, stacked_fields_struct,
    },
    gpu::{self, RenderPassHandle, ShaderHandle, WGPU, WGPUHandle, Window, WindowId},
    mouse::{Clipboard, CursorIcon, MouseBtn, MouseState},
    rect::Rect,
};

pub use crate::ui_context::*;
pub use crate::ui_panel::*;

// TODO[NOTE]: when docked there sometimes is a border a bit wider then it should be
// TODO[NOTE]: framepadding style?
// TODO[BUG]: stack overflow when resizing / maybe dragging scrollbar at the dock split? i think
// its happens when dragging the lower panel of a vertically splitted node. we try to dock an
// already docked node

// BEGIN TYPES
//---------------------------------------------------------------------------------------

id_type!(Id);
id_type!(TextureId);

impl TextureId {
    pub const WHITE: Self = Self::NULL;
    pub const GLYPH: Self = Self(1);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootId {
    Panel(Id),
    Dock(Id),
}

impl Id {
    pub fn from_str(str: &str) -> Id {
        use hash::{Hash, Hasher};
        let str = match str.find("##") {
            Some(idx) => &str[idx..],
            None => &str,
        };

        Self::from_hash(&str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub width: f32,
    pub place: OutlinePlacement,
    pub col: RGBA,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum OutlinePlacement {
    Outer,
    #[default]
    Center,
    Inner,
}

impl Outline {
    pub fn new(col: RGBA, width: f32) -> Self {
        Self {
            width,
            col,
            place: OutlinePlacement::default(),
        }
    }

    pub fn offset(&self) -> f32 {
        match self.place {
            OutlinePlacement::Outer => self.width,
            OutlinePlacement::Center => self.width / 2.0,
            OutlinePlacement::Inner => 0.0,
        }
    }

    pub fn outer(col: RGBA, width: f32) -> Self {
        Self::new(col, width).with_place(OutlinePlacement::Outer)
    }

    pub fn inner(col: RGBA, width: f32) -> Self {
        Self::new(col, width).with_place(OutlinePlacement::Inner)
    }

    pub fn center(col: RGBA, width: f32) -> Self {
        Self::new(col, width).with_place(OutlinePlacement::Center)
    }

    pub fn none() -> Self {
        Self::new(RGBA::ZERO, 0.0)
    }

    pub fn with_place(mut self, place: OutlinePlacement) -> Self {
        self.place = place;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRadii {
    pub tl: f32,
    pub tr: f32,
    pub bl: f32,
    pub br: f32,
}

impl From<f32> for CornerRadii {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl CornerRadii {
    pub fn new(tl: f32, tr: f32, bl: f32, br: f32) -> Self {
        Self { tl, tr, bl, br }
    }

    pub fn all(r: f32) -> Self {
        Self::new(r, r, r, r)
    }

    pub fn zero() -> Self {
        Self::all(0.0)
    }

    pub fn top(r: f32) -> Self {
        Self::new(r, r, 0.0, 0.0)
    }

    pub fn bottom(r: f32) -> Self {
        Self::new(0.0, 0.0, r, r)
    }

    pub fn any_round_corners(&self) -> bool {
        !(self.tl == 0.0 && self.tr == 0.0 && self.bl == 0.0 && self.br == 0.0)
    }
}

stacked_fields_struct!(Style {
    titlebar_color: RGBA,
    titlebar_height: f32,
    window_titlebar_height: f32,

    line_height: f32,
    text_size: f32,
    text_col: RGBA,

    btn_roundness: f32,

    btn_default: RGBA,
    btn_hover: RGBA,
    btn_press: RGBA,
    btn_press_text: RGBA,

    window_bg: RGBA,

    panel_bg: RGBA,
    panel_dark_bg: RGBA,

    panel_corner_radius: f32,
    panel_outline: Outline,
    panel_hover_outline: Outline,
    panel_padding: f32,

    scrollbar_width: f32,
    scrollbar_padding: f32,

    spacing_h: f32,
    spacing_v: f32,

    red: RGBA,
});

impl StyleTable {
    pub fn btn_corner_radius(&self) -> f32 {
        self.btn_roundness() * self.line_height()
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct NextPanelData {
    pub initial_width: f32,
    pub initial_height: f32,
    pub initial_pos: Vec2,

    pub pos: Vec2,
    pub placement: PanelPlacement,
    pub size: Vec2,
    pub min_size: Vec2,
    pub max_size: Vec2,
    pub content_size: Option<Vec2>,
}

impl Default for NextPanelData {
    fn default() -> Self {
        Self::new()
    }
}

impl NextPanelData {
    pub fn new() -> Self {
        Self {
            initial_width: f32::NAN,
            initial_height: f32::NAN,
            initial_pos: Vec2::NAN,

            pos: Vec2::NAN,
            placement: PanelPlacement::TopLeft,
            size: Vec2::NAN,
            // set both to infinity as default
            min_size: Vec2::ZERO,
            max_size: Vec2::INFINITY,
            content_size: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrevItemData {
    pub id: Id,
    pub rect: Rect,
    pub clipped_rect: Rect,
    pub is_clipped: bool,
    pub is_hidden: bool,
    pub is_active: bool,
}

impl PrevItemData {
    pub fn new() -> Self {
        Self {
            id: Id::NULL,
            rect: Rect::ZERO,
            clipped_rect: Rect::ZERO,
            is_clipped: false,
            is_hidden: false,
            is_active: false,
        }
    }

    pub fn reset(&mut self) {
        *self = PrevItemData::new()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layout {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PanelPlacement {
    #[default]
    TopLeft,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelAction {
    DragSplit {
        dir: Dir,
        dock_split_id: Id,
        prev_ratio: f32,
    },
    Resize {
        dir: Dir,
        id: Id,
        prev_rect: Rect,
    },
    Move {
        start_pos: Vec2,
        id: Id,
        dock_target: Id,
        drag_by_titlebar: bool,
        drag_by_title_handle: bool,
        // left-click cancels docking for the current panel being dragged
        cancelled_docking: bool,
    },
    Scroll {
        axis: usize,
        start_scroll: Vec2,
        press_offset: Vec2,
        scroll_rect: Rect,
        id: Id,
    },
    None,
}

impl fmt::Display for PanelAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DragSplit {
                dir,
                dock_split_id: split_dock_id,
                prev_ratio,
            } => {
                write!(f, "DRAG_SPLIT[{dir:?}] {{ {split_dock_id}, {prev_ratio} }}")
            }
            Self::Resize { dir, id, prev_rect } => {
                write!(f, "RESIZE[{dir:?}] {{ {id}, {prev_rect} }}")
            }
            Self::Move {
                start_pos,
                id,
                dock_target,
                cancelled_docking,
                drag_by_titlebar,
                drag_by_title_handle,
            } => write!(
                f,
                "MOVE {{ {id}, {start_pos}, dock_target: {dock_target}, cancel_dock: {cancelled_docking}, drag_tb: {drag_by_titlebar}, drag_title: {drag_by_title_handle} }}"
            ),
            Self::Scroll {
                start_scroll: start_offset,
                id,
                ..
            } => write!(f, "SCROLL {{ {id}, {start_offset} }}"),
            Self::None => write!(f, "NONE"),
        }
    }
}

impl PanelAction {
    pub fn is_none(&self) -> bool {
        match self {
            Self::None => true,
            _ => false,
        }
    }

    pub fn is_resize(&self) -> bool {
        matches!(self, Self::Resize { .. })
    }

    pub fn is_move(&self) -> bool {
        matches!(self, Self::Move { .. })
    }

    pub fn is_scroll(&self) -> bool {
        matches!(self, Self::Scroll { .. })
    }
}

#[derive(Debug, Default, Clone)]
pub struct IdMap<T> {
    pub map: HashMap<Id, T>,
}

impl<T> IdMap<T> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn contains_id(&self, id: Id) -> bool {
        if id.is_null() {
            return false;
        }
        self.map.contains_key(&id)
    }

    pub fn get(&self, id: Id) -> Option<&T> {
        if id.is_null() {
            return None;
        }
        self.map.get(&id)
    }

    pub fn get_mut(&mut self, id: Id) -> Option<&mut T> {
        if id.is_null() {
            return None;
        }
        self.map.get_mut(&id)
    }

    // pub fn hot(&self) -> Option<&Panel> {
    //     self.get(self.hot_id)
    // }

    // pub fn active(&self) -> Option<&Panel> {
    //     self.get(self.active_id)
    // }

    // pub fn current(&self) -> &Panel {
    //     self.get(self.current_id).unwrap()
    // }

    pub fn insert(&mut self, id: Id, panel: T) {
        assert!(!id.is_null());
        self.map.insert(id, panel);
    }

    pub fn remove(&mut self, id: Id) {
        assert!(!id.is_null());
        self.map.remove(&id);
    }

    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, Id, T> {
        (&self.map).iter()
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&Id, &mut T) -> bool,
    {
        self.map.retain(f);
    }
}

impl<T> IntoIterator for IdMap<T> {
    type Item = (Id, T);
    type IntoIter = std::collections::hash_map::IntoIter<Id, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a IdMap<T> {
    type Item = (&'a Id, &'a T);
    type IntoIter = std::collections::hash_map::Iter<'a, Id, T>;
    fn into_iter(self) -> Self::IntoIter {
        (&self.map).iter()
    }
}

impl<'a, T> IntoIterator for &'a mut IdMap<T> {
    type Item = (&'a Id, &'a mut T);
    type IntoIter = std::collections::hash_map::IterMut<'a, Id, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter_mut()
    }
}

impl<T> FromIterator<(Id, T)> for IdMap<T> {
    fn from_iter<I: IntoIterator<Item = (Id, T)>>(iter: I) -> Self {
        IdMap {
            map: HashMap::from_iter(iter),
        }
    }
}

impl<T> Extend<(Id, T)> for IdMap<T> {
    fn extend<I: IntoIterator<Item = (Id, T)>>(&mut self, iter: I) {
        self.map.extend(iter);
    }
}

impl<T> std::ops::Index<Id> for IdMap<T> {
    type Output = T;

    fn index(&self, id: Id) -> &Self::Output {
        self.get(id).unwrap()
    }
}

impl<T> std::ops::IndexMut<Id> for IdMap<T> {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        self.get_mut(id).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabBar {
    pub panel_id: Id,
    pub id: Id,
    pub selected_tab_id: Id,
    // pub next_selected_tab_id: Id,
    pub bar_rect: Rect,
    pub cursor_backup: Cursor,
    pub tabs: Vec<TabItem>,
    pub is_dragging: bool,
    pub dragging_offset: f32,
    // Horizontal scroll offset when tabs overflow the bar rect
    pub scroll_offset: f32,
    // Cached total width of all tabs (including gaps)
    pub total_width: f32,
}

impl TabBar {
    pub fn new() -> Self {
        Self {
            panel_id: Id::NULL,
            id: Id::NULL,
            cursor_backup: Cursor::default(),
            selected_tab_id: Id::NULL,
            // next_selected_tab_id: Id::NULL,
            bar_rect: Rect::ZERO,
            tabs: vec![],
            is_dragging: false,
            dragging_offset: f32::NAN,
            scroll_offset: 0.0,
            total_width: 0.0,
        }
    }

    pub fn layout_tabs(&mut self) {
        let mut offset = 0.0;
        for tab in &mut self.tabs {
            tab.offset = offset;
            offset += tab.width;
            offset += 5.0;
        }
        self.total_width = offset.max(0.0);
    }

    pub fn find_tab(&self, id: Id) -> Option<&TabItem> {
        assert!(!id.is_null());
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn find_mut_tab(&mut self, id: Id) -> Option<&mut TabItem> {
        assert!(!id.is_null());
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn get_insert_pos(&self, pos: f32, width: f32, current_idx: usize) -> usize {
        if self.tabs.is_empty() {
            return 0;
        }

        let drag_center = pos + width * 0.5;

        // Add deadzone: require crossing significantly past the midpoint to trigger a swap
        let deadzone = if current_idx < self.tabs.len() {
            self.tabs[current_idx].width * 0.25 // 25% of current tab width
        } else {
            20.0 // Default deadzone
        };

        // Find which tab position the drag center belongs to
        let mut insert_idx = 0;

        for (i, tab) in self.tabs.iter().enumerate() {
            // account for horizontal scrolling when computing positions
            let tab_start = self.bar_rect.min.x + tab.offset - self.scroll_offset;
            let tab_end = tab_start + tab.width;
            let tab_center = tab_start + tab.width * 0.5;

            if i == current_idx {
                // Skip the current tab in calculations
                continue;
            }

            // Check if drag center is past this tab's adjusted center
            let threshold = if i < current_idx {
                // Moving left: need to cross center + deadzone
                tab_center + deadzone
            } else {
                // Moving right: need to cross center - deadzone
                tab_center - deadzone
            };

            if drag_center < threshold {
                insert_idx = i;
                break;
            }
            insert_idx = i + 1;
        }

        // Adjust for removal of current tab
        if insert_idx > current_idx {
            insert_idx -= 1;
        }

        insert_idx.min(self.tabs.len().saturating_sub(1))
    }

    pub fn move_tab(&mut self, orig: usize, new: usize) {
        if orig >= self.tabs.len() || new >= self.tabs.len() || orig == new {
            return;
        }

        let item = self.tabs.remove(orig);
        self.tabs.insert(new, item);

        self.layout_tabs();
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TabItem {
    pub id: Id,
    pub width: f32,
    pub offset: f32,
    pub close_pressed: bool,
}

#[derive(Debug, Clone)]
pub struct TextInputState {
    pub id: Id,
    pub edit: ctext::Editor<'static>,
    pub fonts: FontTable,
    pub multiline: bool,
}

impl std::hash::Hash for TextInputState {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.fonts.hash(state);
        self.multiline.hash(state);
        self.id.hash(state);
    }
}

impl TextInputState {
    pub fn new(id: Id, mut fonts: FontTable, text: TextItem, multiline: bool) -> Self {
        let mut buffer = ctext::Buffer::new(
            &mut fonts.sys(),
            ctext::Metrics {
                font_size: text.font_size(),
                line_height: text.scaled_line_height(),
            },
        );

        let font_attrib = fonts.get_font_attrib(text.font);
        buffer.set_text(
            &mut fonts.sys(),
            &text.string,
            &font_attrib,
            ctext::Shaping::Advanced,
        );

        let edit = ctext::Editor::new(buffer);

        Self {
            id,
            edit,
            fonts,
            multiline,
        }
    }

    pub fn layout_text(&self, cache: &mut GlyphCache, wgpu: &WGPU) -> ShapedText {
        use ctext::Edit;

        let buffer = match self.edit.buffer_ref() {
            ctext::BufferRef::Owned(b) => b,
            _ => panic!(),
        };

        let mut glyphs = Vec::new();
        let mut width = 0.0;
        let mut height = 0.0;

        for run in buffer.layout_runs() {
            width = run.line_w.max(width);
            // TODO[CHECK]: is it the sum?
            // height = run.line_height.max(height);
            height += run.line_height;

            for g in run.glyphs {
                let g_phys = g.physical((0.0, 0.0), 1.0);
                let mut key = g_phys.cache_key;
                // TODO[CHECK]: what does this do
                key.x_bin = ctext::SubpixelBin::Three;
                key.y_bin = ctext::SubpixelBin::Three;

                if let Some(mut glyph) = cache.get_glyph(key, wgpu) {
                    glyph.meta.pos += Vec2::new(g_phys.x as f32, g_phys.y as f32 + run.line_y);
                    glyphs.push(glyph);
                }
            }
        }

        let text = ShapedText {
            glyphs,
            width,
            height,
        };
        text
    }

    pub fn has_selection(&self) -> bool {
        use ctext::Edit;
        self.edit.selection_bounds().is_some()
    }

    pub fn copy_selection(&self) -> Option<String> {
        use ctext::Edit;
        self.edit.copy_selection()
    }

    pub fn copy_all(&self) -> String {
        use ctext::Edit;
        let mut text = String::new();

        self.edit.with_buffer(|buf| {
            let n_lines = buf.lines.len();
            for (i, line) in buf.lines.iter().enumerate() {
                text.push_str(line.text());
                if i != n_lines - 1 {
                    text.push('\n');
                }
            }
        });

        text
    }

    pub fn paste(&mut self, text: &str) {
        use ctext::Edit;
        self.edit.insert_string(text, None)
    }

    pub fn delete(&mut self) {
        use ctext::{Action, Edit};
        self.edit.action(&mut self.fonts.sys(), Action::Delete);
    }

    pub fn delete_selection(&mut self) {
        use ctext::Edit;
        self.edit.delete_selection();
    }

    pub fn enter(&mut self) {
        use ctext::{Action, Edit};
        if self.multiline {
            self.edit.action(&mut self.fonts.sys(), Action::Enter);
        }
    }

    pub fn escape(&mut self) {
        use ctext::{Action, Edit};
        self.edit.action(&mut self.fonts.sys(), Action::Escape);
    }

    pub fn backspace(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion};
        let ctrl = mods.control_key();

        let sys = &mut self.fonts.sys();

        if ctrl && self.edit.selection_bounds().is_none() {
            let end = self.edit.cursor();
            self.edit.action(sys, Action::Motion(Motion::LeftWord));
            let start = self.edit.cursor();
            self.edit.delete_range(start, end);
        } else {
            self.edit.action(sys, Action::Backspace)
        }
    }

    pub fn deselect_all(&mut self) {
        use ctext::{Edit, Selection};
        if self.has_selection() {
            self.escape()
        }
    }

    pub fn select_all(&mut self) {
        use ctext::{Edit, Selection};
        let mut line_start = 0;
        let mut indx_start = 0;
        let mut line_end = 0;
        let mut indx_end = 0;
        self.edit.with_buffer(|buff| {
            if !buff.lines.is_empty() {
                line_end = buff.lines.len() - 1;
                indx_end = buff.lines[line_end].text().len();
            }
        });
        let end = ctext::Cursor::new(line_end, indx_end);
        let start = ctext::Cursor::new(line_start, indx_start);
        self.edit.set_cursor(start);
        self.edit.set_selection(Selection::Normal(end));
    }

    pub fn move_cursor_up(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let start = edit.cursor();
            // if ctrl {
            //     edit.action(sys, Action::Motion(Motion::UpWord));
            // } else {
            edit.action(sys, Action::Motion(Motion::Up));
            // }
            edit.set_selection(Selection::Normal(start));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::Up));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Up));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(start);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Up))
            }
        }
    }

    pub fn move_cursor_down(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let start = edit.cursor();
            // if ctrl {
            //     edit.action(sys, Action::Motion(Motion::DownWord));
            // } else {
            edit.action(sys, Action::Motion(Motion::Down));
            // }
            edit.set_selection(Selection::Normal(start));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::Down));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Down));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(end);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Down))
            }
        }
    }

    pub fn move_cursor_right(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let start = edit.cursor();
            if ctrl {
                edit.action(sys, Action::Motion(Motion::RightWord));
            } else {
                edit.action(sys, Action::Motion(Motion::Right));
            }
            edit.set_selection(Selection::Normal(start));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::RightWord));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Right));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(end);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Right))
            }
        }
    }

    pub fn move_cursor_left(&mut self, mods: &winit::keyboard::ModifiersState) {
        use ctext::{Action, Edit, Motion, Selection};

        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let has_sel = self.has_selection();
        let sys = &mut self.fonts.sys();

        let edit = &mut self.edit;

        if !has_sel && shift {
            let end = edit.cursor();
            if ctrl {
                edit.action(sys, Action::Motion(Motion::LeftWord));
            } else {
                edit.action(sys, Action::Motion(Motion::Left));
            }
            edit.set_selection(Selection::Normal(end));
            return;
        }

        if ctrl {
            edit.action(sys, Action::Motion(Motion::LeftWord));
        }
        if shift {
            edit.action(sys, Action::Motion(Motion::Left));
        } else {
            if let Some((start, end)) = edit.selection_bounds() {
                edit.set_cursor(start);
                edit.set_selection(Selection::None)
            } else {
                edit.action(sys, Action::Motion(Motion::Left))
            }
        }
    }

    pub fn mouse_pressed(&mut self, pos: Vec2) {
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit
            .action(&mut self.fonts.sys(), Action::Click { x: pos.x, y: pos.y })
    }

    pub fn mouse_double_clicked(&mut self, pos: Vec2) {
        // TODO[BUG]: if the cursor is between two words both words are selected
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit.action(
            &mut self.fonts.sys(),
            Action::DoubleClick { x: pos.x, y: pos.y },
        )
    }

    pub fn mouse_triple_clicked(&mut self, pos: Vec2) {
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit.action(
            &mut self.fonts.sys(),
            Action::TripleClick { x: pos.x, y: pos.y },
        )
    }

    // TODO[NOTE]: on first / last line we should not do wrapping selection
    pub fn mouse_dragging(&mut self, pos: Vec2) {
        use ctext::{Action, Edit};
        let mut pos = pos.as_ivec2();
        if !self.multiline {
            pos.y = 0;
        }
        self.edit
            .action(&mut self.fonts.sys(), Action::Drag { x: pos.x, y: pos.y })
    }
}

//---------------------------------------------------------------------------------------
// END TYPES

// BEGIN FLAGS
//---------------------------------------------------------------------------------------

macros::flags!(ItemFlags: 
    SET_ACTIVE_ON_PRESS,
    SET_ACTIVE_ON_CLICK,
    SET_ACTIVE_ON_RELEASE,
);

macros::flags!(TextInputFlags:
    MULTILINE,
    SELECT_ON_ACTIVE,
);

macros::flags!(
    Signal:

    JUST_PRESSED_LEFT,
    JUST_PRESSED_MIDDLE,
    JUST_PRESSED_RIGHT,
    JUST_PRESSED_KEYBOARD,

    PRESSED_LEFT,
    PRESSED_MIDDLE,
    PRESSED_RIGHT,
    PRESSED_KEYBOARD,

    DRAGGING_LEFT,
    DRAGGING_MIDDLE,
    DRAGGING_RIGHT,

    RELEASED_LEFT,
    RELEASED_MIDDLE,
    RELEASED_RIGHT,

    CLICKED_LEFT,
    CLICKED_MIDDLE,
    CLICKED_RIGHT,

    DOUBLE_CLICKED_LEFT,
    DOUBLE_CLICKED_MIDDLE,
    DOUBLE_CLICKED_RIGHT,

    DOUBLE_PRESSED_LEFT,
    DOUBLE_PRESSED_MIDDLE,
    DOUBLE_PRESSED_RIGHT,

    TRIPLE_CLICKED_LEFT,
    TRIPLE_CLICKED_MIDDLE,
    TRIPLE_CLICKED_RIGHT,

    MOUSE_OVER,
    HOVERING |= MOUSE_OVER,

    GAINED_KEYBOARD_FOCUS,
);

macro_rules! sig_fn {
    ($fn_name:ident => $($x:ident),*) => {
        impl Signal {
            pub const fn $fn_name(&self) -> bool {
                // let flag = Signals::from_bits($x).unwrap();
                $(self.contains(Signal::$x) || )* false
            }
        }
    }
}

sig_fn!(hovering => HOVERING);
sig_fn!(mouse_over => MOUSE_OVER);
sig_fn!(just_pressed => JUST_PRESSED_LEFT, JUST_PRESSED_KEYBOARD);
sig_fn!(pressed => PRESSED_LEFT, PRESSED_KEYBOARD);
sig_fn!(clicked => CLICKED_LEFT, PRESSED_KEYBOARD);
sig_fn!(double_clicked => DOUBLE_CLICKED_LEFT);
sig_fn!(double_pressed => DOUBLE_PRESSED_LEFT);
sig_fn!(dragging => DRAGGING_LEFT);
sig_fn!(released => RELEASED_LEFT);
sig_fn!(keyboard_focused => GAINED_KEYBOARD_FOCUS);

// impl fmt::Display for Signal {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         if *self == Self::NONE {
//             return write!(f, "NONE");
//         }

//         let names = self
//             .iter_names()
//             .map(|(name, _)| name.to_string())
//             .collect::<Vec<_>>();
//         write!(f, "{}", names.join("|"))
//     }
// }

//---------------------------------------------------------------------------------------
// END FLAGS

// BEGIN DRAW LIST
//---------------------------------------------------------------------------------------

/// A single draw command
#[derive(Debug, Clone, Copy)]
pub struct DrawCmd {
    pub texture_id: TextureId,
    pub vtx_offset: usize,
    pub vtx_count: usize,
    pub idx_offset: usize,
    pub idx_count: usize,

    pub clip_rect: Rect,
    pub clip_rect_used: bool,
}

impl Default for DrawCmd {
    fn default() -> Self {
        Self {
            texture_id: TextureId::NULL,
            vtx_offset: 0,
            vtx_count: 0,
            idx_offset: 0,
            idx_count: 0,
            clip_rect: Rect::NAN,
            clip_rect_used: false,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct DrawList {
    pub data: Rc<RefCell<DrawListData>>,
    pub draw_clip_rect: bool,
}

impl DrawList {
    pub fn new() -> Self {
        let data = Rc::new(RefCell::new(DrawListData::new()));
        Self {
            data,
            draw_clip_rect: false,
        }
    }

    pub fn commands(&self) -> Ref<'_, [DrawCmd]> {
        Ref::map(self.data.borrow(), |data| data.cmd_buffer.as_slice())
    }

    pub fn vtx_slice(&self, range: std::ops::Range<usize>) -> Ref<'_, [Vertex]> {
        Ref::map(self.data.borrow(), |data| &data.vtx_buffer[range])
    }

    pub fn idx_slice(&self, range: std::ops::Range<usize>) -> Ref<'_, [u32]> {
        Ref::map(self.data.borrow(), |data| &data.idx_buffer[range])
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.data.borrow().clip_rect
        // .clip_stack
        // .last()
        // .copied()
        // .unwrap_or(Rect::INFINITY)
    }

    pub fn add_draw_rect(&self, rect: DrawRect) {
        self.data.borrow_mut().add_rect_rounded(
            rect.min,
            rect.max,
            rect.uv_min,
            rect.uv_max,
            rect.texture_id,
            rect.fill,
            rect.outline,
            rect.corners,
        );
    }

    pub fn clear(&self) {
        let mut data = self.data.borrow_mut();
        data.clear();
    }

    pub fn draw(&self, itm: impl DrawableRects) {
        itm.add_to_drawlist(self);
    }

    pub fn pop_clip_rect_n(&self, n: u32) {
        let mut data = self.data.borrow_mut();
        for _ in 0..n {
            data.pop_clip_rect();
        }
    }

    pub fn pop_clip_rect(&self) -> Rect {
        self.data.borrow_mut().pop_clip_rect()
    }

    pub fn push_merged_clip_rect(&self, rect: Rect) {
        self.data.borrow_mut().push_merged_clip_rect(rect);
        if self.draw_clip_rect {
            self.add_draw_rect(rect.draw_rect().outline(Outline::inner(RGBA::RED, 2.0)));
        }
    }

    pub fn push_clip_rect(&self, rect: Rect) {
        self.data.borrow_mut().push_clip_rect(rect);
        if self.draw_clip_rect {
            self.add_draw_rect(rect.draw_rect().outline(Outline::inner(RGBA::RED, 2.0)));
        }
    }
    // pub fn vertices(&self) -> Ref<'_, [Vertex]> {
    //     Ref::map(self.data.borrow(), |data| &data.vtx_buffer)
    // }

    // pub fn indices(&self) -> Ref<'_, [u32]> {
    //     Ref::map(self.data.borrow(), |data| &data.idx_buffer)
    // }

    // /// Draw shaped text with optional selection and cursor.
    // /// - `selection_range`: Some((start_glyph_idx, end_glyph_idx)) where `end` is exclusive.
    // /// - `cursor_x`: x position in text-local coordinates (relative to `pos`) where the caret should be drawn.
    // /// - `selection_color` is used to draw the highlight rectangle(s).
    // /// - `selected_text_color` is used to color glyphs inside the selection.
    // pub fn add_editable_text(
    //     &mut self,
    //     pos: Vec2,
    //     text: &ShapedText,
    //     text_color: RGBA,
    //     selection_range: Option<(usize, usize)>,
    //     selection_color: RGBA,
    //     selected_text_color: RGBA,
    //     cursor_x: Option<f32>,
    //     cursor_color: RGBA,
    // ) {
    //     // Draw selection as merged rectangles across contiguous glyphs
    //     if let Some((sel_start, sel_end)) = selection_range {
    //         if sel_start < sel_end && !text.glyphs.is_empty() {
    //             let mut in_range = false;
    //             let mut range_min_x = 0.0f32;
    //             let mut range_max_x = 0.0f32;

    //             for (i, g) in text.glyphs.iter().enumerate() {
    //                 let g_min = g.meta.pos + pos;
    //                 let g_max = g_min + g.meta.size;

    //                 if i >= sel_start && i < sel_end {
    //                     if !in_range {
    //                         in_range = true;
    //                         range_min_x = g_min.x;
    //                         range_max_x = g_max.x;
    //                     } else {
    //                         range_max_x = range_max_x.max(g_max.x);
    //                     }
    //                 } else if in_range {
    //                     // self.rect(
    //                     //     Vec2::new(range_min_x, pos.y),
    //                     //     Vec2::new(range_max_x, pos.y + text.height),
    //                     // )
    //                     // .fill(selection_color)
    //                     // .add();
    //                     in_range = false;
    //                 }
    //             }

    //             if in_range {
    //                 self.add(
    //                     Rect::from_min_size(
    //                     Vec2::new(range_min_x, pos.y),
    //                     Vec2::new(range_max_x, pos.y + text.height))
    //                     .draw_rect().fill(selection_color),
    //                 );
    //                 // .fill(selection_color)
    //                 // .add();
    //             }

    //             // Special-case: empty text or selection that extends past last glyph -> highlight to end
    //             if text.glyphs.is_empty() {
    //                 // self.rect(
    //                 self.add(Rect::from_min_max(
    //                     Vec2::new(pos.x, pos.y),
    //                     Vec2::new(pos.x + text.width, pos.y + text.height)).draw_rect().fill(selection_color));
    //                 // )
    //                 // .fill(selection_color)
    //                 // .add();
    //             } else if sel_end > text.glyphs.len() {
    //                 // If selection extends beyond last glyph, ensure we cover to end of line
    //                 let last = &text.glyphs.last().unwrap();
    //                 let last_min = last.meta.pos + pos;
    //                 let end_x = pos.x + text.width.max(last_min.x + last.meta.size.x);
    //                 self.rect(
    //                     Rect::from_min_max(
    //                     Vec2::new(last_min.x + last.meta.size.x, pos.y),
    //                     Vec2::new(end_x, pos.y + text.height)),
    //                 )
    //                 .fill(selection_color)
    //                 .add();
    //             }
    //         }
    //     }

    //     // Draw cursor (thin vertical rectangle)
    //     if let Some(cx_rel) = cursor_x {
    //         let cx = pos.x + cx_rel;
    //         let caret_w = 1.0_f32;
    //         self.rect(
    //             Vec2::new(cx, pos.y),
    //             Vec2::new(cx + caret_w, pos.y + text.height),
    //         )
    //         .fill(cursor_color)
    //         .add();
    //     }

    //     // Draw glyphs (texture quads) with selected_text_color when inside selection
    //     for (i, g) in text.glyphs.iter().enumerate() {
    //         let min = g.meta.pos + pos;
    //         let max = min + g.meta.size;
    //         let uv_min = g.meta.uv_min;
    //         let uv_max = g.meta.uv_max;

    //         let glyph_color = match selection_range {
    //             Some((s, e)) if i >= s && i < e => selected_text_color,
    //             _ => text_color,
    //         };

    //         self.rect(min, max)
    //             .texture_uv(uv_min, uv_max, 1)
    //             .fill(glyph_color)
    //             .add()
    //     }
    // }
}

/// The draw list itself: holds geometry and draw commands
#[derive(Clone)]
pub struct DrawListData {
    pub vtx_buffer: Vec<Vertex>,
    pub idx_buffer: Vec<u32>,
    pub cmd_buffer: Vec<DrawCmd>,

    pub resolution: f32,
    pub path: Vec<Vec2>,
    pub clip_rect: Rect,
    pub clip_stack: Vec<Rect>,

    pub circle_max_err: f32,
    pub clip_content: bool,
}

impl fmt::Debug for DrawListData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrawList")
            .field("vtx_buffer_size", &self.vtx_buffer.len())
            .field("idx_buffer_size", &self.idx_buffer.len())
            .field("cmd_buffer", &self.cmd_buffer)
            .field("resolution", &self.resolution)
            .field("path", &self.path)
            .finish()
    }
}

impl Default for DrawListData {
    fn default() -> Self {
        Self {
            vtx_buffer: vec![],
            idx_buffer: vec![],
            cmd_buffer: vec![],
            resolution: 20.0,
            path: vec![],
            clip_stack: vec![],
            clip_rect: Rect::INFINITY,

            circle_max_err: 0.3,
            clip_content: true,
        }
    }
}

fn calc_circle_segment_count(rad: f32, max_err: f32) -> u8 {
    use std::f32::consts::PI;
    let tmp = (PI / (1.0 - rad.min(max_err) / rad).cos()).ceil() as u32;
    tmp.clamp(4, 512) as u8
}

impl DrawListData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.vtx_buffer.clear();
        self.idx_buffer.clear();
        self.cmd_buffer.clear();
        self.path.clear();
        self.clip_stack.clear();
    }

    fn calc_circle_segment_count(&self, radius: f32) -> u8 {
        calc_circle_segment_count(radius, self.circle_max_err)
    }

    pub fn set_clip_rect(&mut self, rect: Rect) {
        if rect == Rect::ZERO {
            log::warn!("zero clip rect set");
        }
        let cmd = self.current_draw_cmd();

        if cmd.clip_rect.is_nan() {
            cmd.clip_rect = rect;
        } else if cmd.clip_rect != rect {
            let cmd = self.begin_new_draw_cmd();
            cmd.clip_rect = rect;
            cmd.clip_rect_used = false;
        }
    }

    pub fn push_merged_clip_rect(&mut self, rect: Rect) {
        if !self.clip_content {
            return;
        }
        let curr_clip = self.clip_rect;
        let clip = rect.intersect(curr_clip);
        self.clip_stack.push(self.clip_rect);
        self.set_clip_rect(clip);
        self.clip_rect = clip;
    }

    pub fn push_clip_rect(&mut self, rect: Rect) {
        if !self.clip_content {
            return;
        }
        self.clip_stack.push(self.clip_rect);
        self.set_clip_rect(rect);
        self.clip_rect = rect;
    }

    pub fn pop_clip_rect(&mut self) -> Rect {
        if !self.clip_content {
            return Rect::INFINITY;
        }
        // let rect = self.clip_stack.pop().unwrap();
        self.clip_rect = self.clip_stack.pop().unwrap();
        self.set_clip_rect(self.clip_rect);
        self.clip_rect
    }

    pub fn current_draw_cmd(&mut self) -> &mut DrawCmd {
        if self.cmd_buffer.is_empty() {
            self.cmd_buffer.push(DrawCmd::default())
        }
        self.cmd_buffer.last_mut().unwrap()
    }

    // pub fn current_clip_rect(&self) -> Rect {
    //     // *self.clip_stack.last().unwrap()
    //     self.clip_stack.last().copied().unwrap_or(Rect::INFINITY)
    // }

    pub fn finish_draw_cmd(&mut self) {
        if self.cmd_buffer.is_empty() {
            log::warn!("finishing empty draw command");
        }
        // finish by starting a new command
        let _ = self.begin_new_draw_cmd();
    }

    pub fn begin_new_draw_cmd(&mut self) -> &mut DrawCmd {
        let last = self.cmd_buffer.last().copied();
        // if let Some(last) = last {
        //     if last.vtx_count == 0 {
        //         return self.cmd_buffer.last_mut().unwrap();
        //     }
        // }

        self.cmd_buffer.push(DrawCmd::default());
        let cmd = self.cmd_buffer.last_mut().unwrap();
        cmd.vtx_offset = self.vtx_buffer.len();
        cmd.idx_offset = self.idx_buffer.len();

        if let Some(last) = last {
            cmd.texture_id = last.texture_id;
            cmd.clip_rect = last.clip_rect;
            cmd.clip_rect_used = last.clip_rect_used;
        }
        cmd
    }

    pub fn push_texture(&mut self, tex_id: TextureId) {
        if tex_id == TextureId::WHITE {
            return;
        }
        let cmd = self.current_draw_cmd();

        if cmd.texture_id == TextureId::WHITE {
            cmd.texture_id = tex_id;
            return;
        }
        // TODO[CHECK]: is this valid?
        // if cmd.texture_id == 0 {
        //     cmd.texture_id = tex_id;
        // }

        if cmd.texture_id != tex_id {
            let cmd = self.begin_new_draw_cmd();
            cmd.texture_id = tex_id;
        }
    }

    #[inline]
    pub fn push_vtx_idx(&mut self, vtx: &[Vertex], idx: &[u32]) {
        let cmd = self.current_draw_cmd();
        let base = cmd.vtx_count as u32;

        self.vtx_buffer.extend_from_slice(vtx);
        self.idx_buffer.extend(idx.into_iter().map(|i| base + i));

        let cmd = self.current_draw_cmd();
        cmd.vtx_count += vtx.len();
        cmd.idx_count += idx.len();
    }

    pub fn circle(&mut self, center: Vec2, radius: f32) -> DrawRect {
        let r = Vec2::splat(radius);
        let min = center - r;
        let max = center + r;

        DrawRect {
            // draw_list: self,
            min,
            max,
            uv_min: Vec2::ZERO,
            uv_max: Vec2::ONE,
            texture_id: TextureId::WHITE,
            fill: RGBA::ZERO,
            outline: Outline::none(),
            corners: CornerRadii::all(radius),
        }
    }

    // pub fn add_text(&mut self, pos: Vec2, text: &ShapedText, col: RGBA) {
    //     for g in text.glyphs.iter() {
    //         let min = g.meta.pos + pos;
    //         let max = min + g.meta.size;
    //         let uv_min = g.meta.uv_min;
    //         let uv_max = g.meta.uv_max;

    //         self.rect(min, max)
    //             .texture_uv(uv_min, uv_max, 1)
    //             .fill(col)
    //             .add()
    //     }
    // }

    //     #[inline]
    //     pub fn push_clipped_vtx_idx(&mut self, vtx: &[Vertex], idx: &[u32]) {
    //         let cmd = self.current_draw_cmd();
    //         let base = cmd.vtx_count as u32;
    //         let clip = self.current_clip_rect();

    //         fn lerp(a: f32, b: f32, t: f32) -> f32 {
    //             a + (b - a) * t
    //         }

    //         fn interp_vertex(a: &Vertex, b: &Vertex, t: f32) -> Vertex {
    //             let mut out = a.clone();
    //             out.pos.x = lerp(a.pos.x, b.pos.x, t);
    //             out.pos.y = lerp(a.pos.y, b.pos.y, t);
    //             out.uv.x = lerp(a.uv.x, b.uv.x, t);
    //             out.uv.y = lerp(a.uv.y, b.uv.y, t);
    //             out.col = a.col.lerp(b.col, t);
    //             out
    //         }

    //         // Pre-allocate and reuse temporary buffers to avoid per-triangle allocations
    //         let tri_count = idx.len() / 3;
    //         let mut out_vtxs: Vec<Vertex> = Vec::with_capacity(tri_count * 6); // triangle clipped -> <= ~6 verts typically
    //         let mut out_idx: Vec<u32> = Vec::with_capacity(tri_count * 6);
    //         let mut poly: Vec<Vertex> = Vec::with_capacity(8);
    //         let mut tmp: Vec<Vertex> = Vec::with_capacity(8);

    //         for tri in idx.chunks_exact(3) {
    //             let i0 = tri[0] as usize;
    //             let i1 = tri[1] as usize;
    //             let i2 = tri[2] as usize;
    //             let v0 = vtx[i0].clone();
    //             let v1 = vtx[i1].clone();
    //             let v2 = vtx[i2].clone();

    //             // trivial reject
    //             if (v0.pos.x < clip.min.x && v1.pos.x < clip.min.x && v2.pos.x < clip.min.x)
    //                 || (v0.pos.x > clip.max.x && v1.pos.x > clip.max.x && v2.pos.x > clip.max.x)
    //                 || (v0.pos.y < clip.min.y && v1.pos.y < clip.min.y && v2.pos.y < clip.min.y)
    //                 || (v0.pos.y > clip.max.y && v1.pos.y > clip.max.y && v2.pos.y > clip.max.y)
    //             {
    //                 continue;
    //             }

    //             poly.clear();
    //             poly.push(v0);
    //             poly.push(v1);
    //             poly.push(v2);

    //             // Helper macro-like inline to clip one edge into tmp, then swap poly/tmp
    //             macro_rules! clip_edge {
    //                 ($inside:expr, $intersect_t:expr) => {
    //                     tmp.clear();
    //                     if !poly.is_empty() {
    //                         for i in 0..poly.len() {
    //                             let a = &poly[i];
    //                             let b = &poly[(i + 1) % poly.len()];
    //                             let ina = $inside(a);
    //                             let inb = $inside(b);
    //                             if ina && inb {
    //                                 tmp.push(b.clone());
    //                             } else if ina && !inb {
    //                                 let t = $intersect_t(a, b);
    //                                 tmp.push(interp_vertex(a, b, t));
    //                             } else if !ina && inb {
    //                                 let t = $intersect_t(a, b);
    //                                 tmp.push(interp_vertex(a, b, t));
    //                                 tmp.push(b.clone());
    //                             }
    //                         }
    //                     }
    //                     std::mem::swap(&mut poly, &mut tmp);
    //                 };
    //             }

    //             // left  : x >= clip.min.x
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.x >= clip.min.x,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dx = b.pos.x - a.pos.x;
    //                     if dx.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.min.x - a.pos.x) / dx
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             // right : x <= clip.max.x
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.x <= clip.max.x,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dx = b.pos.x - a.pos.x;
    //                     if dx.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.max.x - a.pos.x) / dx
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             // top   : y >= clip.min.y
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.y >= clip.min.y,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dy = b.pos.y - a.pos.y;
    //                     if dy.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.min.y - a.pos.y) / dy
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             // bottom: y <= clip.max.y
    //             clip_edge!(
    //                 |p: &Vertex| p.pos.y <= clip.max.y,
    //                 |a: &Vertex, b: &Vertex| {
    //                     let dy = b.pos.y - a.pos.y;
    //                     if dy.abs() < 1e-6 {
    //                         0.0
    //                     } else {
    //                         (clip.max.y - a.pos.y) / dy
    //                     }
    //                     .clamp(0.0, 1.0)
    //                 }
    //             );
    //             if poly.len() < 3 {
    //                 continue;
    //             }

    //             let start = out_vtxs.len() as u32;
    //             out_vtxs.extend_from_slice(&poly);

    //             let vcount = poly.len() as u32;
    //             // fan-triangulate the clipped polygon
    //             for i in 1..(vcount - 1) {
    //                 out_idx.push(base + start + 0);
    //                 out_idx.push(base + start + i);
    //                 out_idx.push(base + start + (i + 1));
    //             }
    //         }

    //         if !out_vtxs.is_empty() {
    //             self.vtx_buffer.extend_from_slice(&out_vtxs);
    //         }
    //         if !out_idx.is_empty() {
    //             self.idx_buffer.extend_from_slice(&out_idx);
    //         }

    //         let cmd = self.current_draw_cmd();
    //         cmd.vtx_count += out_vtxs.len();
    //         cmd.idx_count += out_idx.len();
    //     }

    //     #[inline]
    //     pub fn push_clipped_vtx_idx2(&mut self, vtx: &[Vertex], idx: &[u32]) {
    //         let cmd = self.current_draw_cmd();
    //         let base = cmd.vtx_count as u32;
    //         let clip = self.current_clip_rect();

    //         let mut kept: Vec<u32> = Vec::with_capacity(idx.len());
    //         for tri in idx.chunks_exact(3) {
    //             let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
    //             let (v0, v1, v2) = (vtx[i0], vtx[i1], vtx[i2]);

    //             if (v0.pos.x < clip.min.x && v1.pos.x < clip.min.x && v2.pos.x < clip.min.x)
    //                 || (v0.pos.x > clip.max.x && v1.pos.x > clip.max.x && v2.pos.x > clip.max.x)
    //                 || (v0.pos.y < clip.min.y && v1.pos.y < clip.min.y && v2.pos.y < clip.min.y)
    //                 || (v0.pos.y > clip.max.y && v1.pos.y > clip.max.y && v2.pos.y > clip.max.y)
    //             {
    //                 continue;
    //             }

    //             kept.push(base + tri[0]);
    //             kept.push(base + tri[1]);
    //             kept.push(base + tri[2]);
    //         }

    //         self.vtx_buffer.extend_from_slice(vtx);
    //         if !kept.is_empty() {
    //             self.idx_buffer.extend_from_slice(&kept);
    //         }

    //         let cmd = self.current_draw_cmd();
    //         cmd.vtx_count += vtx.len();
    //         cmd.idx_count += kept.len();
    //     }

    pub fn add_rect_rounded(
        &mut self,
        mut min: Vec2,
        mut max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: TextureId,
        tint: RGBA,
        outline: Outline,
        corners: CornerRadii,
    ) {
        if !corners.any_round_corners() {
            return self.add_rect(min, max, uv_min, uv_max, tex_id, tint, outline);
        }

        let offset = Vec2::splat(outline.offset());

        let clip = self.clip_rect;
        let bb = Rect::from_min_max(min - offset, max + offset);
        // if !(clip.contains(min - offset) || clip.contains(max + offset)) {
        if !clip.overlaps(bb) {
            return;
        }

        // log::info!("clip?: clip: {clip}, bb: {bb}");
        if !clip.contains(bb.min) || !clip.contains(bb.max) {
            // log::info!("clipped: clip: {clip}, bb: {bb}");
            self.current_draw_cmd().clip_rect_used = true;
        }

        self.push_texture(tex_id);

        // account for outline placement as original did
        if outline.width != 0.0 {
            let offset = match outline.place {
                OutlinePlacement::Center => 0.0,
                OutlinePlacement::Inner => -outline.width * 0.5,
                OutlinePlacement::Outer => outline.width * 0.5,
            };
            min -= Vec2::splat(offset);
            max += Vec2::splat(offset);
        }

        self.path_clear();
        self.path_rect(min, max, corners);

        let start = self.vtx_buffer.len();
        let (vtx, idx) = tessellate_convex_fill(&self.path, tint, true);
        self.push_vtx_idx(&vtx, &idx);
        let end = start + vtx.len();
        if tex_id != TextureId::WHITE {
            self.distribute_uvs(start, end, min, max, uv_min, uv_max, true, tex_id);
        }

        if outline.width != 0.0 {
            let (vtx_o, idx_o) = tessellate_line(&self.path, outline.col, outline.width, true);
            self.push_vtx_idx(&vtx_o, &idx_o);
        }

        self.path_clear();
    }

    fn push_rect_vertices(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        color: RGBA,
        tex_id: TextureId,
    ) {
        const QUAD_IDX: [u32; 6] = [0, 1, 2, 0, 2, 3];

        let raw_tex_id = tex_id.0 as u32;

        let vertices = [
            Vertex::new(
                Vec2::new(min.x, max.y),
                color,
                Vec2::new(uv_min.x, uv_max.y),
                raw_tex_id,
            ),
            Vertex::new(max, color, uv_max, raw_tex_id),
            Vertex::new(
                Vec2::new(max.x, min.y),
                color,
                Vec2::new(uv_max.x, uv_min.y),
                raw_tex_id,
            ),
            Vertex::new(min, color, uv_min, raw_tex_id),
        ];

        self.push_vtx_idx(&vertices, &QUAD_IDX);
    }

    pub fn path_clear(&mut self) {
        self.path.clear();
    }

    pub fn path_to(&mut self, p: Vec2) {
        self.path.push(p);
    }

    pub fn path_rect(&mut self, min: Vec2, max: Vec2, corners: CornerRadii) {
        const PI: f32 = std::f32::consts::PI;

        let r0 = corners.tl;
        let r1 = corners.tr;
        let r2 = corners.br;
        let r3 = corners.bl;
        self.path_to(Vec2::new(min.x + r0, min.y));

        self.path_to(Vec2::new(max.x - r1, min.y));
        if r1 > 0.0 {
            self.path_arc(Vec2::new(max.x - r1, min.y + r1), r1, PI / 2.0, -PI / 2.0);
        }

        self.path_to(Vec2::new(max.x, min.y + r1));
        self.path_to(Vec2::new(max.x, max.y - r2));
        if r2 > 0.0 {
            self.path_arc(Vec2::new(max.x - r2, max.y - r2), r2, 0.0, -PI / 2.0);
        }

        self.path_to(Vec2::new(max.x - r2, max.y));
        self.path_to(Vec2::new(min.x + r3, max.y));
        if r3 > 0.0 {
            self.path_arc(Vec2::new(min.x + r3, max.y - r3), r3, -PI / 2.0, -PI / 2.0);
        }

        self.path_to(Vec2::new(min.x, max.y - r3));
        self.path_to(Vec2::new(min.x, min.y + r0));
        if r0 > 0.0 {
            self.path_arc(Vec2::new(min.x + r0, min.y + r0), r0, PI, -PI / 2.0);
        }
    }

    pub fn path_arc(&mut self, center: Vec2, radius: f32, start_angle: f32, sweep_angle: f32) {
        if radius == 0.0 || sweep_angle == 0.0 {
            return;
        }

        // maximum angular step so chord length ≤ resolution
        let segments = self.calc_circle_segment_count(radius);

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

    pub fn distribute_uvs(
        &mut self,
        vert_start: usize,
        vert_end: usize,
        a: Vec2,
        b: Vec2,
        uv_a: Vec2,
        uv_b: Vec2,
        clamp: bool,
        tex_id: TextureId,
    ) {
        if vert_end <= vert_start || vert_end > self.vtx_buffer.len() {
            return;
        }

        let raw_tex_id = tex_id.0 as u32;

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
            vert.tex = raw_tex_id;
        }
    }

    pub fn add_rect(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: TextureId,
        tint: RGBA,
        outline: Outline,
    ) {
        // Fast path: opaque solid fill with outline (no texture)
        if tex_id == TextureId::WHITE && tint.a == 1.0 && outline.width > 0.0 {
            self.add_solid_rect_with_outline(min, max, uv_min, uv_max, tint, outline);
            return;
        }

        self.add_simple_rect(min, max, uv_min, uv_max, tex_id, tint);

        if outline.width > 0.0 {
            let clip = self.clip_rect;
            if let Some(crect) = Rect::from_min_max(min, max).clip(clip) {
                self.add_rect_outline(crect.min, crect.max, outline);
            }
        }
    }

    fn add_solid_rect_with_outline(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tint: RGBA,
        outline: Outline,
    ) {
        let clip = self.clip_rect;

        // Draw outline background first
        let outset = outline.width * 0.5;
        let outline_min = min - Vec2::splat(outset);
        let outline_max = max + Vec2::splat(outset);

        if let Some(outline_clip) = Rect::from_min_max(outline_min, outline_max).clip(clip) {
            let outline_uvs = compute_proportional_uvs(
                outline_min,
                outline_max,
                outline_clip.min,
                outline_clip.max,
                uv_min,
                uv_max,
            );
            self.push_rect_vertices(
                outline_clip.min,
                outline_clip.max,
                outline_uvs.0,
                outline_uvs.1,
                outline.col,
                TextureId::WHITE,
            );
        }

        // Draw fill on top
        if let Some(fill_clip) = Rect::from_min_max(min, max).clip(clip) {
            let fill_uvs =
                compute_clipped_uvs(min, max, fill_clip.min, fill_clip.max, uv_min, uv_max);
            self.push_rect_vertices(
                fill_clip.min,
                fill_clip.max,
                fill_uvs.0,
                fill_uvs.1,
                tint,
                TextureId::WHITE,
            );
        }
    }

    fn add_simple_rect(
        &mut self,
        min: Vec2,
        max: Vec2,
        uv_min: Vec2,
        uv_max: Vec2,
        tex_id: TextureId,
        tint: RGBA,
    ) {
        let clip = self.clip_rect;
        let Some(crect) = Rect::from_min_max(min, max).clip(clip) else {
            return;
        };

        self.push_texture(tex_id);
        let clipped_uvs = compute_clipped_uvs(min, max, crect.min, crect.max, uv_min, uv_max);

        let start = self.vtx_buffer.len();
        self.push_rect_vertices(
            crect.min,
            crect.max,
            clipped_uvs.0,
            clipped_uvs.1,
            tint,
            tex_id,
        );

        if tex_id != TextureId::WHITE {
            let end = start + 4;
            self.distribute_uvs(
                start,
                end,
                crect.min,
                crect.max,
                clipped_uvs.0,
                clipped_uvs.1,
                true,
                tex_id,
            );
        }
    }

    // TODO[NOTE]: add clip?
    // TODO[NOTE]: consider outline placement for clipping
    fn add_rect_outline(&mut self, min: Vec2, max: Vec2, outline: Outline) {
        let pts = [
            Vec2::new(min.x, max.y), // bottom-left
            max,                     // top-right
            Vec2::new(max.x, min.y), // top-left
            min,                     // bottom-right
        ];
        let (vtx, idx) = tessellate_line(&pts, outline.col, outline.width, true);
        self.push_vtx_idx(&vtx, &idx);
    }
}

fn compute_clipped_uvs(
    omin: Vec2,
    omax: Vec2,
    cmin: Vec2,
    cmax: Vec2,
    uv_min: Vec2,
    uv_max: Vec2,
) -> (Vec2, Vec2) {
    let orig_size = omax - omin;
    let clipped_offset = cmin - omin;
    let clipped_size = cmax - cmin;

    let mut result_uv_min = uv_min;
    let mut result_uv_max = uv_max;

    if orig_size.x != 0.0 {
        let start_ratio = clipped_offset.x / orig_size.x;
        let end_ratio = (clipped_offset.x + clipped_size.x) / orig_size.x;
        let uv_range = uv_max.x - uv_min.x;
        result_uv_min.x = uv_min.x + start_ratio * uv_range;
        result_uv_max.x = uv_min.x + end_ratio * uv_range;
    }

    if orig_size.y != 0.0 {
        let start_ratio = clipped_offset.y / orig_size.y;
        let end_ratio = (clipped_offset.y + clipped_size.y) / orig_size.y;
        let uv_range = uv_max.y - uv_min.y;
        result_uv_min.y = uv_min.y + start_ratio * uv_range;
        result_uv_max.y = uv_min.y + end_ratio * uv_range;
    }

    (result_uv_min, result_uv_max)
}

fn compute_proportional_uvs(
    orig_min: Vec2,
    orig_max: Vec2,
    target_min: Vec2,
    target_max: Vec2,
    uv_min: Vec2,
    uv_max: Vec2,
) -> (Vec2, Vec2) {
    let orig_size = orig_max - orig_min;
    let uv_size = uv_max - uv_min;

    if orig_size.x == 0.0 || orig_size.y == 0.0 {
        return (uv_min, uv_max);
    }

    let start_offset = target_min - orig_min;
    let end_offset = target_max - orig_min;

    let uv_start = Vec2::new(
        uv_min.x + (start_offset.x / orig_size.x) * uv_size.x,
        uv_min.y + (start_offset.y / orig_size.y) * uv_size.y,
    );

    let uv_end = Vec2::new(
        uv_min.x + (end_offset.x / orig_size.x) * uv_size.x,
        uv_min.y + (end_offset.y / orig_size.y) * uv_size.y,
    );

    (uv_start, uv_end)
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRect {
    // pub draw_list: &'a mut DrawList,
    pub min: Vec2,
    pub max: Vec2,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub texture_id: TextureId,
    pub fill: RGBA,
    pub outline: Outline,
    pub corners: CornerRadii,
}

impl ShapedText {
    pub fn draw_rects(&self, pos: Vec2, col: RGBA) -> Vec<DrawRect> {
        let mut rects = Vec::new();
        for g in self.glyphs.iter() {
            let min = g.meta.pos + pos;
            let max = min + g.meta.size;
            let uv_min = g.meta.uv_min;
            let uv_max = g.meta.uv_max;

            rects.push(
                DrawRect::new(min, max)
                    .fill(col)
                    .texture(TextureId::GLYPH)
                    .uv(uv_min, uv_max),
            );
            // DrawRect::new(min, max)
            //     .texture(1)
            //     .uv(uv_min, uv_max)
            // self.rect(min, max)
            //     .texture_uv(uv_min, uv_max, 1)
            //     .fill(col)
            //     .add()
        }
        rects
    }
}

pub trait DrawableRects {
    fn add_to_drawlist(self, drawlist: &DrawList);
}

impl<'a, I> DrawableRects for I
where
    I: IntoIterator,
    I::Item: DrawableRects,
{
    fn add_to_drawlist(self, drawlist: &DrawList) {
        for drawable in self.into_iter() {
            drawable.add_to_drawlist(drawlist);
        }
    }
}

impl DrawableRects for DrawRect {
    fn add_to_drawlist(self, drawlist: &DrawList) {
        drawlist.data.borrow_mut().add_rect_rounded(
            self.min,
            self.max,
            self.uv_min,
            self.uv_max,
            self.texture_id,
            self.fill,
            self.outline,
            self.corners,
        );
    }
}

impl Rect {
    pub fn draw_rect(self) -> DrawRect {
        DrawRect::new(self.min, self.max)
    }
}

impl DrawRect {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        DrawRect {
            min,
            max,
            uv_min: Vec2::ZERO,
            uv_max: Vec2::ONE,
            texture_id: TextureId::WHITE,
            fill: RGBA::ZERO,
            outline: Outline::none(),
            corners: CornerRadii::zero(),
        }
    }

    pub fn offset(mut self, offset: Vec2) -> Self {
        self.min += offset;
        self.max += offset;
        self
    }

    pub fn fill(mut self, fill: RGBA) -> Self {
        self.fill = fill;
        self
    }

    pub fn outline(mut self, outline: Outline) -> Self {
        self.outline = outline;
        self
    }

    pub fn uv(mut self, uv_min: Vec2, uv_max: Vec2) -> Self {
        self.uv_min = uv_min;
        self.uv_max = uv_max;
        self
    }

    // pub fn texture_uv(mut self, uv_min: Vec2, uv_max: Vec2, id: u32) -> Self {
    //     self.uv_min = uv_min;
    //     self.uv_max = uv_max;
    //     self.texture_id = id;
    //     if self.fill.a == 0.0 {
    //         self.fill = RGBA::WHITE
    //     }
    //     self
    // }

    pub fn texture(mut self, id: TextureId) -> Self {
        self.texture_id = id;
        if self.fill.a == 0.0 {
            self.fill = RGBA::WHITE
        }
        self
    }

    pub fn circle(mut self) -> Self {
        let width = self.max.x - self.min.x;
        let height = self.max.y - self.min.y;
        let rad = width.min(height) / 2.0;
        self.corners(CornerRadii::all(rad))
    }

    pub fn corners(mut self, corners: impl Into<CornerRadii>) -> Self {
        self.corners = corners.into();
        self
    }

    // pub fn add(self) {
    //     self.draw_list.add_rect_rounded(
    //         self.min,
    //         self.max,
    //         self.uv_min,
    //         self.uv_max,
    //         self.texture_id,
    //         self.fill,
    //         self.outline,
    //         self.corners,
    //     )
    // }
}

//---------------------------------------------------------------------------------------
// END DRAW LIST

//---------------------------------------------------------------------------------------
// BEGIN TEXT

pub type TextItemCache = HashMap<TextItem, ShapedText>;
pub type FontId = u64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMeta {
    pub pos: Vec2,
    pub size: Vec2,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub texture: gpu::Texture,
    pub meta: GlyphMeta,
}

#[derive(Debug, Clone)]
pub struct ShapedText {
    pub glyphs: Vec<Glyph>,
    pub width: f32,
    pub height: f32,
}

impl ShapedText {
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextItem {
    // pub font: FontId,
    pub font: &'static str,
    pub string: String,
    pub font_size_i: u64,
    pub line_height_i: u64,
    pub width_i: Option<u64>,
    pub height_i: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FontTable {
    // pub id_to_name: Vec<(FontId, String)>,
    pub sys: Rc<RefCell<ctext::FontSystem>>,
}

impl std::hash::Hash for FontTable {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.sys).hash(state);
    }
}

impl FontTable {
    pub fn new() -> Self {
        Self {
            // id_to_name: Default::default(),
            sys: Rc::new(RefCell::new(ctext::FontSystem::new())),
        }
    }

    pub fn sys(&mut self) -> std::cell::RefMut<'_, ctext::FontSystem> {
        self.sys.borrow_mut()
    }
    // TODO[NOTE] remove font id?
    pub fn load_font(&mut self, name: &str, bytes: Vec<u8>) {
        let mut sys = self.sys();
        let db = sys.db_mut();
        let ids = db.load_font_source(ctext::fontdb::Source::Binary(std::sync::Arc::new(bytes)));
        // self.id_to_name.push((id, name.to_string()));
    }

    pub fn get_font_attrib<'a>(&self, name: &'a str) -> ctext::Attrs<'a> {
        // let name = self.id_to_name.get(&id).unwrap();
        let attribs = ctext::Attrs::new().family(ctext::Family::Name(name));
        attribs
    }
}

impl TextItem {
    pub fn layout(&self, fonts: &mut FontTable, cache: &mut GlyphCache, wgpu: &WGPU) -> ShapedText {
        let mut buffer = ctext::Buffer::new(
            &mut fonts.sys(),
            ctext::Metrics {
                font_size: self.font_size(),
                line_height: self.scaled_line_height(),
            },
        );

        let font_attrib = fonts.get_font_attrib(self.font);
        buffer.set_size(&mut fonts.sys(), self.width(), self.height());
        buffer.set_text(
            &mut fonts.sys(),
            &self.string,
            &font_attrib,
            ctext::Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut fonts.sys(), false);

        let mut glyphs = Vec::new();
        let mut width = 0.0;
        let mut height = 0.0;

        for run in buffer.layout_runs() {
            width = run.line_w.max(width);
            // TODO[CHECK]: is it the sum?
            // height = run.line_height.max(height);
            height += run.line_height;

            for g in run.glyphs {
                let g_phys = g.physical((0.0, 0.0), 1.0);
                let mut key = g_phys.cache_key;
                // TODO[CHECK]: what does this do
                key.x_bin = ctext::SubpixelBin::Three;
                key.y_bin = ctext::SubpixelBin::Three;

                if let Some(mut glyph) = cache.get_glyph(key, wgpu) {
                    glyph.meta.pos += Vec2::new(g_phys.x as f32, g_phys.y as f32 + run.line_y);
                    glyphs.push(glyph);
                }
            }
        }

        let text = ShapedText {
            glyphs,
            width,
            height,
        };
        text
    }
}

// fn shape_text_item(
//     itm: TextItem,
//     fonts: &mut FontTable,
//     cache: &mut GlyphCache,
//     wgpu: &WGPU,
// ) -> ShapedText {
//     let mut buffer = ctext::Buffer::new(
//         &mut fonts.sys,
//         ctext::Metrics {
//             font_size: itm.font_size(),
//             line_height: itm.scaled_line_height(),
//         },
//     );

//     let font_attrib = fonts.get_font_attrib(itm.font);
//     buffer.set_size(&mut fonts.sys, itm.width(), itm.height());
//     buffer.set_text(
//         &mut fonts.sys,
//         &itm.string,
//         &font_attrib,
//         ctext::Shaping::Advanced,
//     );
//     buffer.shape_until_scroll(&mut fonts.sys, false);

//     let mut glyphs = Vec::new();
//     let mut width = 0.0;
//     let mut height = 0.0;

//     for run in buffer.layout_runs() {
//         width = run.line_w.max(width);
//         // TODO[CHECK]: is it the sum?
//         // height = run.line_height.max(height);
//         height += run.line_height;

//         for g in run.glyphs {
//             let g_phys = g.physical((0.0, 0.0), 1.0);
//             let mut key = g_phys.cache_key;
//             // TODO[CHECK]: what does this do
//             key.x_bin = ctext::SubpixelBin::Three;
//             key.y_bin = ctext::SubpixelBin::Three;

//             if let Some(mut glyph) = cache.get_glyph(key, fonts, wgpu) {
//                 glyph.meta.pos += Vec2::new(g_phys.x as f32, g_phys.y as f32 + run.line_y);
//                 glyphs.push(glyph);
//             }
//         }
//     }

//     let text = ShapedText {
//         glyphs,
//         width,
//         height,
//     };
//     text
// }

impl TextItem {
    pub const RESOLUTION: f32 = 1024.0;

    pub fn new(text: String, font_size: f32, line_height: f32, font: &'static str) -> Self {
        Self {
            font,
            string: text,
            font_size_i: (font_size * Self::RESOLUTION) as u64,
            line_height_i: (line_height * Self::RESOLUTION) as u64,
            width_i: None,
            height_i: None,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width_i = Some((width * Self::RESOLUTION) as u64);
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height_i = Some((height * Self::RESOLUTION) as u64);
        self
    }

    pub fn width(&self) -> Option<f32> {
        self.width_i.map(|w| w as f32 / Self::RESOLUTION)
    }

    pub fn height(&self) -> Option<f32> {
        self.height_i.map(|h| h as f32 / Self::RESOLUTION)
    }

    pub fn line_height(&self) -> f32 {
        self.line_height_i as f32 / Self::RESOLUTION
    }

    pub fn font_size(&self) -> f32 {
        self.font_size_i as f32 / Self::RESOLUTION
    }

    pub fn scaled_line_height(&self) -> f32 {
        self.line_height() * self.font_size()
    }
}

pub struct GlyphCache {
    pub texture: gpu::Texture,
    pub alloc: etagere::AtlasAllocator,
    pub min_alloc_uv: Vec2,
    pub max_alloc_uv: Vec2,
    pub size: u32,
    pub cached_glyphs: HashMap<ctext::CacheKey, GlyphMeta>,
    pub swash_cache: ctext::SwashCache,
    pub fonts: FontTable,
}

// TODO[NOTE]: dealloc with garbage collector

impl GlyphCache {
    pub fn new(wgpu: &WGPU, fonts: FontTable) -> Self {
        const SIZE: u32 = 1024;
        let size = SIZE.min(wgpu.device.limits().max_texture_dimension_2d);

        let texture = wgpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_cache_texture"),
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
            etagere::AtlasAllocator::new(etagere::Size::new(size as i32 + 3, size as i32 + 3));
        let texture = gpu::Texture::new(texture, texture_view);

        Self {
            texture,
            min_alloc_uv: Vec2::INFINITY,
            max_alloc_uv: Vec2::ZERO,
            alloc,
            size,
            cached_glyphs: Default::default(),
            swash_cache: ctext::SwashCache::new(),
            fonts,
        }
    }

    pub fn get_glyph(&mut self, glyph_key: ctext::CacheKey, wgpu: &WGPU) -> Option<Glyph> {
        if let Some(&meta) = self.cached_glyphs.get(&glyph_key) {
            return Some(Glyph {
                texture: self.texture.clone(),
                meta,
            });
        }

        self.alloc_new_glyph(glyph_key, wgpu)
    }

    pub fn alloc_rect(&mut self, mut w: u32, mut h: u32) -> Rect {
        // TODO[CHECK]: account for roundoff error?
        w += 1;
        h += 1;
        let alloc = self
            .alloc
            .allocate(etagere::Size::new(w as i32, h as i32))
            .unwrap();

        let r = alloc.rectangle;

        let min = Vec2::new(r.min.x as f32, r.min.y as f32);
        let max = Vec2::new(r.max.x as f32, r.max.y as f32);

        self.min_alloc_uv = self.min_alloc_uv.min(min / self.texture.size());
        self.max_alloc_uv = self.max_alloc_uv.max(max / self.texture.size());

        Rect::from_min_max(min, max)
    }

    pub fn alloc_data(&mut self, w: u32, h: u32, data: &[u8], wgpu: &WGPU) -> Option<Rect> {
        assert_eq!(w * h * 4, data.len() as u32);
        let rect = self.alloc_rect(w, h);

        wgpu.queue.write_texture(
            wgpu::TexelCopyTextureInfoBase {
                texture: &self.texture.raw(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.min.x as u32,
                    y: rect.min.y as u32,
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

        let tex_size = self.texture.width();
        assert!(self.texture.height() == tex_size);
        // let pos = Vec2::new(x as f32, -y as f32);
        let size = Vec2::new(w as f32, h as f32);
        let uv_min = Vec2::new(rect.min.x as f32, rect.min.y as f32) / tex_size as f32;
        let uv_max = uv_min + size / tex_size as f32;

        Some(Rect::from_min_max(uv_min, uv_max))
    }

    pub fn alloc_new_glyph(&mut self, glyph_key: ctext::CacheKey, wgpu: &WGPU) -> Option<Glyph> {
        let img = self
            .swash_cache
            .get_image_uncached(&mut self.fonts.sys(), glyph_key)?;
        let x = img.placement.left;
        let y = img.placement.top;
        let w = img.placement.width;
        let h = img.placement.height;

        let (has_color, data) = match img.content {
            ctext::SwashContent::Mask => {
                let mut data = Vec::new();
                data.reserve_exact((w * h * 4) as usize);
                for val in img.data {
                    data.push(255);
                    data.push(255);
                    data.push(255);
                    data.push(val);
                }
                (false, data)
            }
            ctext::SwashContent::Color => (true, img.data),
            ctext::SwashContent::SubpixelMask => {
                unimplemented!()
            }
        };

        let uv_rect = self.alloc_data(w, h, &data, wgpu)?;
        let pos = Vec2::new(x as f32, -y as f32);
        let size = Vec2::new(w as f32, h as f32);

        let meta = GlyphMeta {
            pos,
            size,
            uv_min: uv_rect.min,
            uv_max: uv_rect.max,
        };
        self.cached_glyphs.insert(glyph_key, meta);

        Some(Glyph {
            texture: self.texture.clone(),
            meta,
        })
    }
}

pub mod phosphor_font {
    // from https://phosphoricons.com/
    pub const X: &'static str = "\u{E4F6}";
    pub const MAXIMIZE_OFF: &'static str = "\u{E0F8}";
    pub const MAXIMIZE: &'static str = "\u{E3F0}";
    pub const MINIMIZE: &'static str = "\u{E32A}";
    pub const CARET_RIGHT: &'static str = "\u{E13A}";
    pub const CARET_DOWN: &'static str = "\u{E136}";
}

//---------------------------------------------------------------------------------------
// END TEXT

// BEGIN RENDER
//---------------------------------------------------------------------------------------

pub const MAX_N_TEXTURES_PER_DRAW_CALL: usize = 8;

pub struct RenderData {
    pub gpu_vertices: wgpu::Buffer,
    pub gpu_indices: wgpu::Buffer,

    pub call_list: DrawCallList,
    pub screen_size: Vec2,

    pub antialias: bool,

    pub white_texture: gpu::Texture,
    // pub glyph_texture: gpu::Texture,
    /// registered textures
    /// 
    /// texture id is defined as the index + 1 in this array, 0 is reserved for white texture
    pub texture_reg: Vec<gpu::Texture>,

    pub wgpu: WGPUHandle,
}

impl RenderData {
    /// 2^16
    pub const MAX_VERTEX_COUNT: u64 = 65_536;
    // 2^17
    pub const MAX_INDEX_COUNT: u64 = 131_072;

    pub fn new(glyph_texture: gpu::Texture, wgpu: WGPUHandle) -> Self {
        // let mut font_db = ctext::fontdb::Database::new();
        // font_db.load_font_data(include_bytes!("../res/Roboto.ttf").to_vec());
        // // font_db.load_font_data(include_bytes!("CommitMono-400-Regular.otf").to_vec());
        // // font_db.load_font_data(include_bytes!("CommitMono-500-Regular.otf").to_vec());
        // let mut icon_font_db = ctext::fontdb::Database::new();
        // icon_font_db.load_font_data(include_bytes!("../res/Phosphor.ttf").to_vec());

        let white_texture = gpu::Texture::create_with_usage(&wgpu, 1, 1, wgpu::TextureUsages::TEXTURE_BINDING, &[255, 255, 255, 255]);

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

        let texture_reg = vec![glyph_texture];

        Self {
            gpu_vertices,
            gpu_indices,
            screen_size: Vec2::ONE,
            antialias: true,
            call_list: DrawCallList::new(
                Self::MAX_VERTEX_COUNT as usize,
                Self::MAX_INDEX_COUNT as usize,
            ),
            white_texture,
            texture_reg,
            wgpu,
        }
    }

    pub fn push_drawlist(&mut self, list: &DrawList) {
        for cmd in list.commands().iter(){
            let vtx = &list.vtx_slice(cmd.vtx_offset..cmd.vtx_offset + cmd.vtx_count);
            let idx = &list.idx_slice(cmd.idx_offset..cmd.idx_offset + cmd.idx_count);

            let mut curr_clip = self.call_list.current_clip_rect();
            curr_clip.min = curr_clip.min.max(Vec2::ZERO);
            curr_clip.max = curr_clip.max.min(self.screen_size);

            let mut clip = cmd.clip_rect;
            clip.min = clip.min.max(Vec2::ZERO);
            clip.max = clip.max.min(self.screen_size);

            // draw_buff.set_clip_rect(cmd.clip_rect);
            if cmd.clip_rect_used {
                self.call_list.set_clip_rect(cmd.clip_rect);
            } else if !self.call_list.current_clip_rect().contains_rect(clip) {
                self.call_list.set_clip_rect(Rect::from_min_size(Vec2::ZERO, self.screen_size));
            }
            
            self.call_list.push_texture(cmd.texture_id);
            self.call_list.push(vtx, idx); 
        }
    }

    pub fn clear(&mut self) {
        self.call_list.clear();
    }
}

impl RenderPassHandle for RenderData {
    const LABEL: &'static str = "draw_list_render_pass";

    fn n_render_passes(&self) -> u32 {
        self.call_list.calls.len() as u32
        // 1
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {
        self.draw_multiple(rpass, wgpu, 0);

        // let proj =
        //     Mat4::orthographic_lh(0.0, self.screen_size.x, self.screen_size.y, 0.0, -1.0, 1.0);

        // let global_uniform = GlobalUniform::new(self.screen_size, proj);

        // let bind_group = build_bind_group(global_uniform, self.glyph_texture.view(), wgpu);

        // // if self.call_list.vtx_alloc.len() * std::mem::size_of::<Vertex>() >= self.gpu_vertices.size() as usize {
        // //     self.gpu_vertices = wgpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        // //         label: Some("draw_list_vertex_buffer"),
        // //         usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        // //         contents: bytemuck::cast_slice(&self.call_list.vtx_alloc),
        // //     });
        // // } else {
        // //     wgpu.queue
        // //         .write_buffer(&self.gpu_vertices, 0, bytemuck::cast_slice(&self.call_list.vtx_alloc));
        // // }

        // // if self.call_list.idx_alloc.len() * std::mem::size_of::<Vertex>() >= self.gpu_indices.size() as usize {
        // //     self.gpu_indices = wgpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        // //         label: Some("draw_list_index_buffer"),
        // //         usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::INDEX,
        // //         contents: bytemuck::cast_slice(&self.call_list.idx_alloc),
        // //     });
        // // } else {
        // //     wgpu.queue
        // //         .write_buffer(&self.gpu_indices, 0, bytemuck::cast_slice(&self.call_list.idx_alloc));
        // // }

        // // let (verts, indxs, clip) = self.call_list.get_draw_call_data(i).unwrap();
        // let mut i = 0;
        // // println!("n_calls: {}", self.call_list.calls.len());
        // for call in &self.call_list.calls {
        //     // i += 1;

        //     // if i != 2 && self.call_list.calls.len() == 3 {
        //     //     continue
        //     // }
        //     let clip = call.clip_rect;
        //     rpass.set_bind_group(0, &bind_group, &[]);
        //     rpass.set_vertex_buffer(0, self.gpu_vertices.slice(..));
        //     rpass.set_index_buffer(self.gpu_indices.slice(..), wgpu::IndexFormat::Uint32);
        //     rpass.set_pipeline(&UiShader.get_pipeline(&[(&Vertex::desc(), "Vertex")], wgpu));

        //     let target_size = self.screen_size.floor().as_uvec2();
        //     let clip_min = clip.min.as_uvec2().max(UVec2::ZERO).min(target_size);
        //     let clip_max = clip.max.as_uvec2().max(clip_min).min(target_size);
        //     let clip_size = clip_max - clip_min;

        //     // let clip_min = clip.min.as_uvec2().clamp(Vec2::ZERO, target_size);
        //     // let clip_size = clip.size().as_uvec2().clamp(Vec2::ZERO, target_size);
        //     rpass.set_scissor_rect(clip_min.x, clip_min.y, clip_size.x, clip_size.y);

        //     let idx_offset = call.idx_ptr as u32;
        //     let vtx_offset = call.vtx_ptr as i32;
        //     let n_idx = call.n_idx as u32;
        //     rpass.draw_indexed(idx_offset..idx_offset + n_idx, vtx_offset, 0..1);
        // }
    }

    fn draw_multiple<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU, i: u32) {
        let proj =
            Mat4::orthographic_lh(0.0, self.screen_size.x, self.screen_size.y, 0.0, -1.0, 1.0);

        let global_uniform = GlobalUniform::new(self.screen_size, proj);

        // let bind_group = build_bind_group(global_uniform, self.glyph_texture.view(), wgpu);
        let mut tex_views = self.call_list.calls[i as usize]
            .textures
            .iter()
            .map(|&tex_id| self.texture_reg[tex_id as usize - 1].view().clone())
            .collect::<Vec<_>>();

        while tex_views.len() < MAX_N_TEXTURES_PER_DRAW_CALL {
            tex_views.push(self.white_texture.view().clone());
        }


        let bind_group = build_bind_group(global_uniform, &tex_views, wgpu);

        let (verts, indxs, clip) = self.call_list.get_draw_call_data(i).unwrap();

        wgpu.queue
            .write_buffer(&self.gpu_vertices, 0, bytemuck::cast_slice(verts));
        wgpu.queue
            .write_buffer(&self.gpu_indices, 0, bytemuck::cast_slice(indxs));

        rpass.set_bind_group(0, &bind_group, &[]);
        rpass.set_vertex_buffer(0, self.gpu_vertices.slice(..));
        rpass.set_index_buffer(self.gpu_indices.slice(..), wgpu::IndexFormat::Uint32);
        
        let desc = Vertex::desc();
        let config = gpu::ShaderBuildConfig::new([(&desc, "Vertex")]);
        rpass.set_pipeline(&UiShader.get_pipeline(config, wgpu));

        let target_size = self.screen_size.as_uvec2();
        let clip_min = clip.min.as_uvec2().max(UVec2::ZERO).min(target_size);
        let clip_max = clip.max.as_uvec2().max(clip_min).min(target_size);
        let clip_size = clip_max - clip_min;

        // let clip_min = clip.min.as_uvec2().clamp(Vec2::ZERO, target_size);
        // let clip_size = clip.size().as_uvec2().clamp(Vec2::ZERO, target_size);
        rpass.set_scissor_rect(clip_min.x, clip_min.y, clip_size.x, clip_size.y);

        rpass.draw_indexed(0..indxs.len() as u32, 0, 0..1);
    }
}

/// Represents a contiguous segment of vertex and index data
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawCall {
    pub clip_rect: Rect,
    pub vtx_ptr: usize,
    pub idx_ptr: usize,
    pub n_vtx: usize,
    pub n_idx: usize,
    pub textures: ArrVec<u32, MAX_N_TEXTURES_PER_DRAW_CALL>,
}

impl DrawCall {
    pub fn new() -> Self {
        Self {
            clip_rect: Rect::ZERO,
            vtx_ptr: 0,
            idx_ptr: 0,
            n_vtx: 0,
            n_idx: 0,
            textures: ArrVec::new(),
        }
    }
}

/// A chunked buffer storing vertices and indices,
///
/// Allowing multiple render passes
/// when a single draw exceeds GPU limits or predefined chunk sizes.
#[derive(Clone)]
pub struct DrawCallList {
    pub max_vtx_per_chunk: usize,
    pub max_idx_per_chunk: usize,
    pub vtx_alloc: Vec<Vertex>,
    pub idx_alloc: Vec<u32>,
    /// Current write offset in `vtx_alloc`.
    pub vtx_ptr: usize,
    /// Current write offset in `idx_alloc`.
    pub idx_ptr: usize,
    pub calls: Vec<DrawCall>,
}

impl fmt::Debug for DrawCallList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrawCallList")
            .field("max_vtx_per_chunk", &self.max_vtx_per_chunk)
            .field("max_idx_per_chunk", &self.max_idx_per_chunk)
            .field("vtx_alloc", &self.vtx_alloc.len())
            .field("idx_alloc", &self.idx_alloc.len())
            .field("vtx_ptr", &self.vtx_ptr)
            .field("idx_ptr", &self.idx_ptr)
            .field("calls", &self.calls)
            .finish()
    }
}

impl DrawCallList {
    pub fn clear(&mut self) {
        self.calls.clear();
        self.vtx_ptr = 0;
        self.idx_ptr = 0;
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn new(max_vtx_per_chunk: usize, max_idx_per_chunk: usize) -> Self {
        // let max_idx_per_chunk = usize::MAX;
        // let max_vtx_per_chunk = usize::MAX;
        Self {
            max_vtx_per_chunk,
            max_idx_per_chunk,
            vtx_alloc: vec![],
            idx_alloc: vec![],
            vtx_ptr: 0,
            idx_ptr: 0,
            calls: vec![],
        }
    }

    pub fn get_draw_call_data(&self, chunk_idx: u32) -> Option<(&[Vertex], &[u32], Rect)> {
        self.calls.get(chunk_idx as usize).map(|chunk| {
            let vtx_slice = &self.vtx_alloc[chunk.vtx_ptr..chunk.vtx_ptr + chunk.n_vtx];
            let idx_slice = &self.idx_alloc[chunk.idx_ptr..chunk.idx_ptr + chunk.n_idx];
            (vtx_slice, idx_slice, chunk.clip_rect)
        })
    }


    pub fn push_texture(&mut self, texture_id: TextureId) {
        let raw_tex_id = texture_id.0 as u32;
        if self.calls.is_empty() {
            self.calls.push(DrawCall::new());
        }

        let mut c = self.calls.last_mut().unwrap();

        // skip if texture is white (always bound) or already present
        if texture_id == TextureId::WHITE || c.textures.iter().any(|&id| id == raw_tex_id) {
            return;
        }

        if c.textures.len() >= MAX_N_TEXTURES_PER_DRAW_CALL {
            let prev_clip = self.calls.last().unwrap().clip_rect;
            self.calls.push(DrawCall {
                clip_rect: prev_clip,
                vtx_ptr: self.vtx_ptr,
                idx_ptr: self.idx_ptr,
                n_vtx: 0,
                n_idx: 0,
                textures: ArrVec::new(),
            });

            c = self.calls.last_mut().unwrap();
        }

        c.textures.push(raw_tex_id);
    }

    fn texture_binding(&self, raw_tex_id: u32) -> u32 {
        if raw_tex_id == 0 {
            return 0;
        }

        let c = self.calls.last().unwrap();
        for (i, &id) in c.textures.iter().enumerate() {
            if id == raw_tex_id {
                return (i + 1) as u32;
            }
        }

        panic!("texture id {} not found in current draw call", raw_tex_id);
    }

    // assumes all vertices use the same texture (or no texture)
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

        if self.calls.is_empty() {
            self.calls.push(DrawCall::new());
        }

        let c = *self.calls.last().unwrap();

        if c.n_vtx + vtx.len() > self.max_vtx_per_chunk
            || c.n_idx + idx.len() > self.max_idx_per_chunk
        {
            let prev_clip = self.calls.last().unwrap().clip_rect;
            let prev_textures = self.calls.last().unwrap().textures;
            self.calls.push(DrawCall {
                clip_rect: prev_clip,
                vtx_ptr: self.vtx_ptr,
                idx_ptr: self.idx_ptr,
                n_vtx: 0,
                n_idx: 0,
                textures: prev_textures,
            });
        }

        let c = self.calls.last_mut().unwrap();

        if self.vtx_alloc.len() < self.vtx_ptr + vtx.len() {
            self.vtx_alloc
                .resize(self.vtx_ptr + vtx.len(), Vertex::ZERO);
        }

        if self.idx_alloc.len() < self.idx_ptr + idx.len() {
            self.idx_alloc.resize(self.idx_ptr + idx.len(), 0);
        }
        let mut texture_id = 0;
        // copy vertices, remap texture ids
        self.vtx_alloc[self.vtx_ptr..self.vtx_ptr + vtx.len()]
            .iter_mut()
            .zip(vtx.iter())
            .for_each(|(dst, &src)| {
            if src.tex != 0 && texture_id == 0 {
                texture_id = texture_id.max(src.tex);
            }

            if src.tex != 0 && texture_id != 0 && src.tex != texture_id {
                panic!("Mixing multiple textures in a single draw call is not supported. {} != {}", texture_id, src.tex);
            }

            *dst = src;
            dst.tex = if src.tex == 0 {
                0
            } else {
                c.textures.iter().position(|&id| id == src.tex).unwrap() as u32 + 1
            };
            });

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

    pub fn set_clip_rect(&mut self, rect: Rect) {
        if rect == Rect::ZERO {
            log::warn!("zero clip rect set");
        }
        if self.calls.is_empty() {
            self.calls.push(DrawCall::new());
        }

        let c = self.calls.last_mut().unwrap();
        if c.clip_rect == Rect::ZERO {
            c.clip_rect = rect
        } else if c.clip_rect != rect {
            self.calls.push(DrawCall {
                clip_rect: rect,
                vtx_ptr: self.vtx_ptr,
                idx_ptr: self.idx_ptr,
                n_vtx: 0,
                n_idx: 0,
                textures: ArrVec::new(),
            });
            // let c = self.calls.last_mut().unwrap();
            // c.clip_rect = rect;
        }
    }

    pub fn current_clip_rect(&self) -> Rect {
        self.calls.last().unwrap().clip_rect
    }
}

pub struct UiShader;

impl gpu::ShaderHandle for UiShader {
    const RENDER_PIPELINE_ID: gpu::ShaderID = "ui_shader";

    fn build_pipeline<const N: usize>(&self, config: gpu::ShaderBuildConfig<'_, N>, wgpu: &WGPU) -> wgpu::RenderPipeline {
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

            @rust texture_bindings;


            @fragment
            fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
                
                var col: vec4<f32> = in.color;
                @rust texture_fetch;
            }
            "#;


        let mut bind_group_entries = vec![
            //global uniform
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
        ];

        for i in 0..MAX_N_TEXTURES_PER_DRAW_CALL {
            bind_group_entries.push(wgpu::BindGroupLayoutEntry {
                binding: (i + 2) as u32,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }

        let global_bind_group_layout =
            wgpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &bind_group_entries,
                    label: Some("global_bind_group_layout"),
                });

        let mut shader_src = gpu::pre_process_shader_code(SHADER_SRC, &config.shader_templates).unwrap();

        let mut rust_texture_bindings = String::new();
        let mut rust_texture_fetch = String::new();
        for i in 0..MAX_N_TEXTURES_PER_DRAW_CALL {
            rust_texture_bindings.push_str(&format!("
                @group(0) @binding({})
                var tex{}: texture_2d<f32>;
            ", i + 2, i + 1));
            // rust_texture_fetch.push_str(&format!("
            //     else if in.tex == {}u {{
            //         let c{} = textureSample(tex{}, samp, in.uv) * in.color;
            //         return c{};
            //     }}", i + 1, i + 1, i + 1, i + 1));
        }

        for i in 0..MAX_N_TEXTURES_PER_DRAW_CALL {
            rust_texture_fetch.push_str(&format!("let c{} = textureSample(tex{}, samp, in.uv) * in.color;\n", i + 1, i + 1));
        }

        for i in 0..MAX_N_TEXTURES_PER_DRAW_CALL {
            rust_texture_fetch.push_str(&format!("col = select(col, c{}, in.tex == {}u);\n", i + 1, i + 1));
        }

        rust_texture_fetch.push_str("return col;\n");
        // rust_texture_fetch.push_str("else { return vec4<f32>(1.0, 0.0, 1.0, 1.0); }");

        shader_src = shader_src.replace("@rust texture_bindings;", &rust_texture_bindings);
        shader_src = shader_src.replace("@rust texture_fetch;", &rust_texture_fetch);

        let vertices = config.shader_templates.iter().map(|d| d.0).collect::<Vec<_>>();
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

#[macros::vertex]
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

    // pub fn build_bind_group(&self, wgpu: &WGPU) -> wgpu::BindGroup {
    //     let global_uniform = wgpu
    //         .device
    //         .create_buffer_init(&wgpu::util::BufferInitDescriptor {
    //             label: Some("rect_global_uniform_buffer"),
    //             contents: bytemuck::cast_slice(&[*self]),
    //             usage: wgpu::BufferUsages::UNIFORM,
    //         });

    //     let global_bind_group_layout =
    //         wgpu.device
    //             .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    //                 entries: &[wgpu::BindGroupLayoutEntry {
    //                     binding: 0,
    //                     visibility: wgpu::ShaderStages::VERTEX,
    //                     ty: wgpu::BindingType::Buffer {
    //                         ty: wgpu::BufferBindingType::Uniform,
    //                         has_dynamic_offset: false,
    //                         min_binding_size: None,
    //                     },
    //                     count: None,
    //                 }],
    //                 label: Some("global_bind_group_layout"),
    //             });

    //     wgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
    //         label: Some("global_bind_group"),
    //         layout: &global_bind_group_layout,
    //         entries: &[wgpu::BindGroupEntry {
    //             binding: 0,
    //             resource: global_uniform.as_entire_binding(),
    //         }],
    //     })
    // }
}

pub fn build_bind_group(
    glob: GlobalUniform,
    tex_views: &[wgpu::TextureView],
    wgpu: &WGPU,
) -> wgpu::BindGroup {
    assert!(tex_views.len() == MAX_N_TEXTURES_PER_DRAW_CALL);

    let global_uniform = wgpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rect_global_uniform_buffer"),
            contents: bytemuck::cast_slice(&[glob]),
            usage: wgpu::BufferUsages::UNIFORM,
        });


        let mut layout_entries = vec![
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
    ];

    for i in 0..MAX_N_TEXTURES_PER_DRAW_CALL {
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: (i + 2) as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }


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

    let mut group_entries = vec![
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
    ];

    for i in 0..MAX_N_TEXTURES_PER_DRAW_CALL {
        group_entries.push(wgpu::BindGroupEntry {
            binding: (i + 2) as u32,
            resource: wgpu::BindingResource::TextureView(&tex_views[i]),
        });
    }

    wgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("global_bind_group"),
        layout: &global_bind_group_layout,
        entries: &group_entries,
    })
}

//---------------------------------------------------------------------------------------
// END RENDER
