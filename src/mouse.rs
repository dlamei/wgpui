use std::{fmt, ops};

use glam::Vec2;

use crate::utils::{Duration, Instant};

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

#[derive(Clone, Copy, Default, PartialEq)]
struct ButtonRec {
    pressed: bool,
    double_press: bool,
    // todo maybe use option?
    press_start_pos: Vec2,
    press_time: Option<Instant>,
    last_release_time: Option<Instant>,
    last_press_was_short: bool,
    dragging: bool,

    released: bool,
}

impl fmt::Display for ButtonRec {
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
            write!(
                f,
                " @({:.1}, {:.1})",
                self.press_start_pos.x, self.press_start_pos.y
            )?;
            if let Some(press_time) = self.press_time {
                write!(f, " {}ms", press_time.elapsed().as_millis())?;
            }
        }

        if self.double_press {
            write!(f, " [DOUBLE]")?;
        }

        Ok(())
    }
}

impl fmt::Debug for ButtonRec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MouseRec {
    pub pos: Vec2,
    pub prev_pos: Vec2,
    pub buttons: PerButton<ButtonRec>,

    // Configuration
    pub double_click_time: Duration,
    pub drag_threshold: f32,
}

impl Default for MouseRec {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseRec {
    pub fn new() -> Self {
        Self {
            pos: Vec2::NAN,
            prev_pos: Vec2::NAN,
            buttons: PerButton::default(),
            double_click_time: Duration::from_millis(150),
            drag_threshold: 5.0,
        }
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.prev_pos = self.pos;
        self.pos = Vec2::new(x, y);

        // Update drag states for all pressed buttons
        for button in [MouseBtn::Left, MouseBtn::Right, MouseBtn::Middle] {
            let state = &mut self.buttons[button];
            if state.pressed && !state.dragging {
                let distance = self.pos.distance(state.press_start_pos);
                if distance > self.drag_threshold {
                    state.dragging = true;
                }
            }
        }
    }

    pub fn poll_released(&mut self, btn: MouseBtn) -> bool {
        if self.buttons[btn].released {
            self.buttons[btn].released = false;
            true
        } else {
            false
        }
    }

    pub fn clear_released(&mut self) {
        self.buttons[MouseBtn::Left].released = false;
        self.buttons[MouseBtn::Middle].released = false;
        self.buttons[MouseBtn::Right].released = false;
    }

    pub fn set_button_press(&mut self, button: MouseBtn, pressed: bool) {
        let state = &mut self.buttons[button];
        let was_pressed = state.pressed;

        if pressed && !was_pressed {
            let now = Instant::now();
            state.pressed = true;
            state.press_start_pos = self.pos;
            state.press_time = Some(now);
            state.dragging = false;

            state.double_press = if let Some(last_release) = state.last_release_time {
                now.duration_since(last_release) <= self.double_click_time
                    && state.last_press_was_short
            } else {
                false
            };
        } else if !pressed && was_pressed {
            let now = Instant::now();
            state.pressed = false;
            state.dragging = false;
            state.released = true;

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

    pub fn drag_delta(&self) -> Vec2 {
        self.pos - self.prev_pos
    }

    pub fn pressed(&self, button: MouseBtn) -> bool {
        self.buttons[button].pressed
    }

    pub fn dragging(&self, button: MouseBtn) -> bool {
        self.buttons[button].dragging
    }

    pub fn drag_start(&self, button: MouseBtn) -> Vec2 {
        self.buttons[button].press_start_pos
    }

    // pub fn clicked(&self, button: MouseBtn) -> bool {
    //     let state = &self.buttons[button];
    //     state.pressed
    //         && state
    //             .press_time
    //             .map_or(false, |t| t.elapsed() < Duration::from_millis(16))
    // }

    pub fn double_clicked(&self, button: MouseBtn) -> bool {
        self.buttons[button].double_press
    }
}

impl fmt::Display for MouseRec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "MouseState {{")?;
        writeln!(f, "  pos: ({:.1}, {:.1})", self.pos.x, self.pos.y)?;

        let delta = self.drag_delta();
        if delta.x != 0.0 || delta.y != 0.0 {
            writeln!(f, "  dlta: ({:.1}, {:.1})", delta.x, delta.y)?;
        }

        writeln!(f, "  left: {}", self.buttons[MouseBtn::Left])?;
        writeln!(f, "  rght: {}", self.buttons[MouseBtn::Right])?;
        writeln!(f, "  mddl: {}", self.buttons[MouseBtn::Middle])?;
        write!(f, "}}")
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
        use winit::window::CursorIcon as WCI;
        match value {
            CursorIcon::Default => WCI::Default,
            CursorIcon::Pointer => WCI::Pointer,
            CursorIcon::Text => WCI::Text,
            CursorIcon::ResizeN => WCI::NResize,
            CursorIcon::ResizeNE => WCI::NeResize,
            CursorIcon::ResizeE => WCI::EResize,
            CursorIcon::ResizeSE => WCI::SeResize,
            CursorIcon::ResizeS => WCI::SResize,
            CursorIcon::ResizeSW => WCI::SwResize,
            CursorIcon::ResizeW => WCI::WResize,
            CursorIcon::ResizeNW => WCI::NwResize,
        }
        .into()
    }
}
