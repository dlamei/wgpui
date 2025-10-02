use std::sync::Arc;

use glam::{UVec2, Vec2};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    window::Window as WinitWindow,
};

use crate::{
    Vertex, VertexPosCol, build,
    core::{self, Duration, HashMap, Instant, RGBA},
    gpu::{self, WGPU, WGPUHandle, Window, WindowId},
    mouse::{self, MouseBtn},
    rect::Rect,
    ui,
};

#[derive(Debug, Clone)]
pub struct ClearScreen(pub RGBA);

impl gpu::RenderPassHandle for ClearScreen {
    const LABEL: &'static str = "clear_screen_pass";

    fn load_op(&self) -> wgpu::LoadOp<wgpu::Color> {
        wgpu::LoadOp::Clear(self.0.into())
    }

    fn store_op(&self) -> wgpu::StoreOp {
        wgpu::StoreOp::Store
    }

    fn draw<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, wgpu: &WGPU) {}
}

pub enum AppSetup {
    UnInit {
        // window: Option<WinitWindow>,
        created_window: bool,
        #[cfg(target_arch = "wasm32")]
        renderer_rec: Option<futures::channel::oneshot::Receiver<(WGPU, Window)>>,
    },
    Init(App),
}

impl Default for AppSetup {
    fn default() -> Self {
        Self::UnInit {
            // window: None,
            created_window: false,
            #[cfg(target_arch = "wasm32")]
            renderer_rec: None,
        }
    }
}

fn load_window_icon() -> winit::window::Icon {
    let icon_bytes = include_bytes!("../res/icon2.png");
    let img = image::load_from_memory(icon_bytes).unwrap().into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    winit::window::Icon::from_rgba(rgba, width, height).unwrap()
}

impl AppSetup {
    pub fn is_init(&self) -> bool {
        matches!(self, Self::Init(_))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn resumed_native(&mut self, event_loop: &ActiveEventLoop) {
        if self.is_init() {
            return;
        }

        let mut attribs = WinitWindow::default_attributes()
            .with_title("Atlas")
            .with_decorations(false)
            // .with_resizable(true)
            .with_window_icon(Some(load_window_icon()));

        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attribs =
                attribs.with_corner_preference(winit::platform::windows::CornerPreference::Round);
        }

        let window = event_loop.create_window(attribs).unwrap();

        // self.window = Some(window_handle.clone());

        let size = window.inner_size();
        // let scale_factor = window_handle.scale_factor() as f32;
        // let window_handle_2 = window_handle.clone();

        let (window, wgpu) = core::futures::wait_for(async move {
            WGPU::new_async(window, size.width, size.height).await
        });

        *self = Self::Init(App::new(window, wgpu));
    }

    #[cfg(target_arch = "wasm32")]
    fn resumed_wasm(&mut self, event_loop: &ActiveEventLoop) {
        let mut attributes = WinitWindow::default_attributes().with_title("Atlas");

        use wasm_bindgen::JsCast;
        use winit::platform::web::WindowAttributesExtWebSys;
        let canvas = wgpu::web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .get_element_by_id("canvas")
            .unwrap()
            .dyn_into::<wgpu::web_sys::HtmlCanvasElement>()
            .unwrap();
        let canvas_width = canvas.width().max(1);
        let canvas_height = canvas.height().max(1);
        attributes = attributes.with_canvas(Some(canvas));

        if let Ok(new_window) = event_loop.create_window(attributes) {
            if let Self::UnInit {
                // window,
                created_window,
                renderer_rec,
            } = self
            {
                // let first_window_handle = window.is_none();

                if !*created_window {
                    let (sender, receiver) = futures::channel::oneshot::channel();
                    // self.renderer_rec = Some(receiver);
                    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

                    console_log::init().expect("Failed to initialize logger!");
                    log::info!("Canvas dimensions: ({canvas_width} x {canvas_height})");

                    wasm_bindgen_futures::spawn_local(async move {
                        let (wgpu, window) =
                            WGPU::new_async(new_window, canvas_width, canvas_height).await;
                        if sender.send((wgpu, window)).is_err() {
                            log::error!("Failed to create and send renderer!");
                        }
                    });

                    // *window = Some(window_handle);
                    *created_window = true;
                    *renderer_rec = Some(receiver);
                }
            }
        }
    }

    fn init_unwrap(&mut self) -> &mut App {
        match self {
            AppSetup::Init(app) => app,
            _ => panic!(),
        }
    }

    fn try_init(&mut self) -> Option<&mut App> {
        if let Self::Init(app) = self {
            return Some(app);
        }

        #[cfg(target_arch = "wasm32")]
        {
            let Self::UnInit {
                created_window,
                renderer_rec,
            } = self
            else {
                unreachable!();
            };
            // let mut renderer_received = false;
            use winit::platform::web::WindowExtWebSys;
            if let Some(receiver) = renderer_rec.as_mut() {
                if let Ok(Some((wgpu, window))) = receiver.try_recv() {
                    let window_id = window.id;
                    window.raw.set_prevent_default(false);
                    window.request_redraw();
                    let size = window.window_size();
                    *self = Self::Init(App::new(wgpu, window));
                    let app = self.init_unwrap();
                    app.ui2
                        .resize_window(window_id, size.x as u32, size.y as u32);
                    return Some(self.init_unwrap());
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
}

pub struct App {
    pub ui: ui::Context,

    pub mouse_pos: Vec2,

    pub prev_frame_time: Instant,
    pub delta_time: Duration,

    pub wgpu: WGPUHandle,
    pub main_window: WindowId,
    // pub windows: HashMap<WindowId, Window>,
}

impl App {
    pub fn new(wgpu: WGPU, window: Window) -> Self {
        let wgpu = Arc::new(wgpu);
        let main_window = window.id;
        // let mut windows = HashMap::new();
        // windows.insert(main_window, window.clone());
        Self {
            ui: ui::Context::new(wgpu.clone(), window),
            prev_frame_time: Instant::now(),
            delta_time: Duration::ZERO,
            mouse_pos: Vec2::NAN,
            wgpu,
            main_window,
        }
    }

    fn on_window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        use WindowEvent as WE;
        // if self.window.id() != window_id {
        //     return;
        // // }
        let w_size = self.ui.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);

        match event {
            WE::CursorMoved { position: pos, .. } => {
                self.mouse_pos = (pos.x as f32, pos.y as f32).into();
                self.ui.set_mouse_pos(self.mouse_pos.x, self.mouse_pos.y);
                if id == self.ui.window.id && !self.ui.window.raw.has_focus() {
                    self.on_update(event_loop);
                    self.on_redraw(event_loop, id);

                    // println!("request redraw");
                    // self.ui2.window.request_redraw();
                }
                // self.windows.get_mut(&id).unwrap().on_mouse_moved(self.mouse_pos);
            }
            WE::CursorEntered { .. } => {
                // self.ui.cursor_in_window = true;
            }
            WE::CursorLeft { .. } => {
                // self.ui.mouse_in_window = false;
                // self.ui.cursor_in_window = true;
            }

            WE::MouseInput { state, button, .. } => {
                use winit::event::{ElementState, MouseButton};
                let pressed = match state {
                    ElementState::Pressed => true,
                    ElementState::Released => false,
                };

                match button {
                    MouseButton::Left => {
                        self.ui.set_mouse_press(MouseBtn::Left, pressed);
                    }
                    MouseButton::Middle => {
                        self.ui.set_mouse_press(MouseBtn::Middle, pressed);
                    }
                    MouseButton::Right => {
                        self.ui.set_mouse_press(MouseBtn::Left, pressed);
                    }
                    _ => (),
                }
            }
            WE::RedrawRequested => {
                if id == self.main_window {
                    self.on_update(event_loop);
                    let pid = self.ui.get_root_panel();
                    if self.ui.close_pressed {
                        event_loop.exit();
                    }
                }
                self.on_redraw(event_loop, id);
            }
            WE::KeyboardInput { event, .. } => {
                self.on_keyboard(&event, event_loop);
            }
            WE::Resized(PhysicalSize { width, height }) => {
                let (width, height) = (width.max(1), height.max(1));
                self.ui.resize_window(id, width, height);

                // self.windows
                //     .get_mut(&id)
                //     .unwrap()
                //     .resize(width, height, &self.wgpu.device);
            }
            WE::CloseRequested => event_loop.exit(),
            _ => (),
        }
    }

    fn on_update(&mut self, event_loop: &ActiveEventLoop) {
        let ui = &mut self.ui;
        ui.begin_frame();

        // if let Some(p) = ui.get_panel_with_name("Debug") {
        //     ui.next.min_size = p.full_size;
        // }
        ui.begin_ex("Debug", ui::PanelFlags::NO_TITLEBAR);
        ui.set_current_panel_min_size(|prev, full, content| full);

        if ui.button("test button") {
            println!("test button pressed");
        }
        ui.button("the quick brown fox jumps over the lazy dog");
        ui.text("Hello World");
        ui.end();

        for i in 0..4 {
            ui.begin(format!("test window##{i}"));
            ui.button("test button");
            ui.same_line();
            ui.checkbox_intern("checkbox");
            ui.same_line();
            ui.button("test button");

            ui.switch_intern("test");
            ui.same_line();
            ui.button("the quick brown fox jumps over the lazy dog");

            ui.slider_f32_intern("test slider", 0.0, 10.0);
            ui.end();
        }

        ui.debug_window();

        ui.end_frame(event_loop);
    }

    fn on_keyboard(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event.physical_key {
            PhysicalKey::Code(KeyCode::KeyD) => if event.state.is_pressed() {},
            _ => (),
        }
    }

    fn on_redraw(&mut self, event_loop: &ActiveEventLoop, id: WindowId) {
        let prev_time = self.prev_frame_time;
        let curr_time = Instant::now();
        let dt = curr_time - prev_time;
        self.prev_frame_time = curr_time;
        self.delta_time = dt;

        {
            let window = self.ui.get_mut_window(id);
            let Some(mut target) = window.prepare_frame(&self.wgpu) else {
                return;
            };

            target.render(&ClearScreen(RGBA::rgba_f(0.0, 0.0, 0.0, 0.0)));
            target.render(&self.ui.draw);
        }

        let window = self.ui.get_mut_window(id);
        window.present_frame();
        window.request_redraw();
    }
}
