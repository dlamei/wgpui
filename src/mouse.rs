use std::{fmt, ops};

use glam::Vec2;

use crate::core::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MouseBtn {
    Left = 0,
    Right = 1,
    Middle = 2,
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PerButton<T>(pub [T; 3]);

impl<T> ops::Index<MouseBtn> for PerButton<T> {
    type Output = T;

    fn index(&self, index: MouseBtn) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl<T> ops::IndexMut<MouseBtn> for PerButton<T> {
    fn index_mut(&mut self, index: MouseBtn) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MouseState {
    pub pos: Vec2,
    pub prev_pos: Vec2,
    pub buttons: PerButton<ButtonState>,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            pos: Vec2::NAN,
            prev_pos: Vec2::NAN,
            buttons: PerButton([ButtonState::new(); 3]),
        }
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.prev_pos = self.pos;
        self.pos = Vec2::new(x, y);

        for b in [MouseBtn::Left, MouseBtn::Right, MouseBtn::Middle] {
            self.buttons[b].update_pos(self.pos);
        }
    }

    pub fn drag_start(&self, button: MouseBtn) -> Option<Vec2> {
        let b = self.buttons[button];
        if b.dragging || b.released {
            b.press_start_pos
        } else {
            None
        }
    }

    pub fn set_button_press(&mut self, button: MouseBtn, pressed: bool) {
        self.buttons[button].set_press(self.pos, pressed);
    }

    pub fn released(&self, btn: MouseBtn) -> bool {
        self.buttons[btn].released
    }

    pub fn pressed(&self, btn: MouseBtn) -> bool {
        self.buttons[btn].pressed
    }

    pub fn clicked(&self, btn: MouseBtn) -> bool {
        self.buttons[btn].clicked()
    }

    pub fn double_pressed(&self, btn: MouseBtn) -> bool {
        self.buttons[btn].double_pressed()
    }

    pub fn double_clicked(&self, btn: MouseBtn) -> bool {
        self.buttons[btn].double_clicked()
    }
    
    pub fn triple_clicked(&self, btn: MouseBtn) -> bool {
        self.buttons[btn].triple_clicked()
    }

    pub fn drag_delta(&self, btn: MouseBtn) -> Option<Vec2> {
        self.buttons[btn].get_drag_delta(self.pos)
    }

    pub fn dragging(&self, btn: MouseBtn) -> bool {
        self.drag_delta(btn).is_some()
    }

    pub fn double_click_dragging(&self, btn: MouseBtn) -> bool {
        self.drag_delta(btn).is_some() && self.click_count(btn) == 2
    }

    pub fn click_count(&self, btn: MouseBtn) -> u16 {
        self.buttons[btn].get_click_count()
    }

    pub fn end_frame(&mut self) {
        for b in [MouseBtn::Left, MouseBtn::Right, MouseBtn::Middle] {
            self.buttons[b].end_frame();
        }
    }

    pub fn reset(&mut self) {
        for b in [MouseBtn::Left, MouseBtn::Right, MouseBtn::Middle] {
            self.buttons[b].reset();
        }
    }
}

impl fmt::Display for MouseState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "MouseState {{")?;
        writeln!(f, "  pos: ({:.1}, {:.1})", self.pos.x, self.pos.y)?;

        // let delta = self.drag_delta();
        // if delta.x != 0.0 || delta.y != 0.0 {
        //     writeln!(f, "  dlta: ({:.1}, {:.1})", delta.x, delta.y)?;
        // }

        writeln!(f, "  left: {}", self.buttons[MouseBtn::Left])?;
        writeln!(f, "  rght: {}", self.buttons[MouseBtn::Right])?;
        writeln!(f, "  mddl: {}", self.buttons[MouseBtn::Middle])?;
        write!(f, "}}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonState {
    pub last_press_time: Instant,
    pub last_release_time: Option<Instant>,
    pub click_count: Option<(u16, Instant)>,
    pub pressed: bool,
    pub released: bool,
    pub dragging: bool,
    pub press_start_pos: Option<Vec2>,
    pub click_threshold: Duration,
    pub drag_threshold: f32,
    pub multi_click_timeout: Duration,
}

impl fmt::Display for ButtonState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.dragging {
            "dragging"
        } else if self.pressed {
            "pressed"
        } else if self.clicked() {
            "clicked"
        } else if self.released {
            "released"
        } else {
            "none"
        };

        write!(f, "{}", status)?;

        if self.pressed {
            let press_start_pos = self.press_start_pos.unwrap();
            write!(f, " @({:.1}, {:.1})", press_start_pos.x, press_start_pos.y)?;
            // if let Some(press_time) = self.press_time {
            //     write!(f, " {}ms", press_time.elapsed().as_millis())?;
            // }
        }

        let click_count = self.get_click_count();
        if click_count != 0 {
            write!(f, " [{click_count} CLICKS]")?;
        }

        Ok(())
    }
}

impl ButtonState {
    pub fn new() -> Self {
        Self {
            last_press_time: Instant::now(),
            last_release_time: None,
            released: false,
            click_count: None,
            pressed: false,
            dragging: false,
            press_start_pos: None,
            click_threshold: Duration::from_millis(200), // Max time for a click
            drag_threshold: 5.0,                         // Min distance to consider a drag
            multi_click_timeout: Duration::from_millis(400), // Time window for multi-clicks
        }
    }

    pub fn end_frame(&mut self) {
        self.released = false;

        let now = Instant::now();
        if let Some((_, click_time)) = self.click_count {
            if now.duration_since(click_time) > self.multi_click_timeout {
                self.click_count = None;
            }
        }
    }

    pub fn with_thresholds(
        click_threshold: Duration,
        drag_threshold: f32,
        multi_click_timeout: Duration,
    ) -> Self {
        Self {
            click_threshold,
            drag_threshold,
            multi_click_timeout,
            ..Self::new()
        }
    }

    pub fn set_press(&mut self, pos: Vec2, press: bool) {
        let now = Instant::now();

        if press && !self.pressed {
            // Button just pressed
            self.pressed = true;
            self.last_press_time = now;
            self.press_start_pos = Some(pos);
        } else if !press && self.pressed {
            // Button just released
            self.dragging = false;
            self.released = true;
            self.pressed = false;
            self.last_release_time = Some(now);

            let press_duration = now.duration_since(self.last_press_time);
            let is_quick_press = press_duration < self.click_threshold;
            let is_within_drag_threshold = self
                .press_start_pos
                .map(|start_pos| pos.distance(start_pos) < self.drag_threshold)
                .unwrap_or(true);

            // Only register as click if it was quick and didn't move too much
            if is_quick_press && is_within_drag_threshold {
                self.add_click(now);
            } else {
                // Reset click count if this was a long press or drag
                self.click_count = None;
            }
        }
    }

    fn add_click(&mut self, click_time: Instant) {
        match self.click_count {
            None => {
                self.click_count = Some((1, click_time));
            }
            Some((count, first_click_time)) => {
                // Check if this click is within the multi-click timeout
                if click_time.duration_since(first_click_time) < self.multi_click_timeout {
                    self.click_count = Some((count + 1, first_click_time));
                } else {
                    // Start a new click sequence
                    self.click_count = Some((1, click_time));
                }
            }
        }
    }

    pub fn get_click_count(&self) -> u16 {
        self.click_count.map(|(count, _)| count).unwrap_or(0)
    }

    pub fn clicked(&self) -> bool {
        self.get_click_count() > 0 && self.released == true
    }

    pub fn double_pressed(&self) -> bool {
        self.get_click_count() == 1 && self.pressed
    }

    pub fn double_clicked(&self) -> bool {
        self.get_click_count() == 2 && self.released == true
    }

    pub fn triple_clicked(&self) -> bool {
        self.get_click_count() == 2 && self.released == true
    }

    pub fn update_pos(&mut self, pos: Vec2) {
        if let Some(start_pos) = self.press_start_pos {
            let delta = Vec2::new(pos.x - start_pos.x, pos.y - start_pos.y);
            if self.pressed && delta.length() > self.drag_threshold {
                self.dragging = true;
            }
        }
    }

    pub fn get_drag_delta(&self, current_pos: Vec2) -> Option<Vec2> {
        if self.dragging {
            let delta = current_pos - self.press_start_pos.unwrap();
            Some(delta)
        } else {
            None
        }
    }

    pub fn get_press_duration(&self) -> Option<Duration> {
        if self.pressed {
            Some(Instant::now().duration_since(self.last_press_time))
        } else if let Some(release_time) = self.last_release_time {
            Some(release_time.duration_since(self.last_press_time))
        } else {
            None
        }
    }

    pub fn reset(&mut self) {
        self.click_count = None;
        self.pressed = false;
        self.press_start_pos = None;
    }
}


#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CursorIcon {
    #[default]
    Default,

    Pointer,
    Text,

    ResizeN,
    ResizeNE,
    ResizeE,
    ResizeSE,
    ResizeS,
    ResizeSW,
    ResizeW,
    ResizeNW,

    MoveH,
    MoveV,
}

impl CursorIcon {
    pub fn is_resize(self) -> bool {
        matches!(
            self,
            Self::ResizeN
                | Self::ResizeNE
                | Self::ResizeE
                | Self::ResizeSE
                | Self::ResizeS
                | Self::ResizeSW
                | Self::ResizeW
                | Self::ResizeNW
        )
    }
}

impl From<CursorIcon> for winit::window::Cursor {
    fn from(value: CursorIcon) -> Self {
        use CursorIcon as CI;
        use winit::window::CursorIcon as WCI;
        match value {
            CI::Default => WCI::Default,
            CI::Pointer => WCI::Pointer,
            CI::Text => WCI::Text,
            CI::ResizeN => WCI::NResize,
            CI::ResizeNE => WCI::NeResize,
            CI::ResizeE => WCI::EResize,
            CI::ResizeSE => WCI::SeResize,
            CI::ResizeS => WCI::SResize,
            CI::ResizeSW => WCI::SwResize,
            CI::ResizeW => WCI::WResize,
            CI::ResizeNW => WCI::NwResize,
            CI::MoveH => WCI::EwResize,
            CI::MoveV => WCI::NsResize,
        }
        .into()
    }
}
