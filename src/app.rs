use std::sync::Arc;

use glam::{UVec2, Vec2};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    window::Window,
};

use crate::{
    ClearScreen, Vertex, VertexPosCol,
    gpu::Renderer,
    mouse::MouseBtn,
    ui::{self, WidgetOpt},
    utils::{self, Duration, Instant, RGBA},
};

pub enum AppSetup {
    UnInit {
        window: Option<Arc<Window>>,
        #[cfg(target_arch = "wasm32")]
        renderer_rec: Option<futures::channel::oneshot::Receiver<Renderer>>,
    },
    Init(App),
}

impl Default for AppSetup {
    fn default() -> Self {
        Self::UnInit {
            window: None,
            #[cfg(target_arch = "wasm32")]
            renderer_rec: None,
        }
    }
}

fn load_window_icon() -> winit::window::Icon {
    let icon_bytes = include_bytes!("icon2.png");
    let img = image::load_from_memory(icon_bytes).unwrap().into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    winit::window::Icon::from_rgba(rgba, width, height).unwrap()
}

impl AppSetup {
    pub fn is_init(&self) -> bool {
        matches!(self, Self::Init(_))
    }

    pub fn init_app(window: Arc<Window>, renderer: Renderer) -> App {
        // let scale_factor = window.scale_factor() as f32;
        App::new(renderer, window)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn resumed_native(&mut self, event_loop: &ActiveEventLoop) {
        if self.is_init() {
            return;
        }


        let window = event_loop
            .create_window(winit::window::Window::default_attributes()
                .with_title("Atlas")
                .with_window_icon(Some(load_window_icon()))
                ).unwrap();

        let window_handle = Arc::new(window);
        // self.window = Some(window_handle.clone());

        let size = window_handle.inner_size();
        let scale_factor = window_handle.scale_factor() as f32;

        let window_handle_2 = window_handle.clone();
        let renderer = utils::futures::wait_for(async move {
            Renderer::new_async(window_handle_2, size.width, size.height).await
        });
        // let renderer = pollster::block_on(async move {
        //     Renderer::new_async(window_handle_2, size.width, size.height).await
        // });

        *self = Self::Init(Self::init_app(window_handle, renderer));
    }

    #[cfg(target_arch = "wasm32")]
    fn resumed_wasm(&mut self, event_loop: &ActiveEventLoop) {
        let mut attributes = winit::window::Window::default_attributes().with_title("Atlas");

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
        // self.last_size = (canvas_width, canvas_height).into();
        attributes = attributes.with_canvas(Some(canvas));

        if let Ok(new_window) = event_loop.create_window(attributes) {
            if let Self::UnInit {
                window,
                renderer_rec,
            } = self
            {
                let first_window_handle = window.is_none();
                let window_handle = Arc::new(new_window);

                if first_window_handle {
                    let (sender, receiver) = futures::channel::oneshot::channel();
                    // self.renderer_rec = Some(receiver);
                    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

                    console_log::init().expect("Failed to initialize logger!");
                    log::info!("Canvas dimensions: ({canvas_width} x {canvas_height})");

                    let window_handle_2 = window_handle.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let renderer =
                            Renderer::new_async(window_handle_2, canvas_width, canvas_height).await;
                        if sender.send(renderer).is_err() {
                            log::error!("Failed to create and send renderer!");
                        }
                    });

                    *window = Some(window_handle);
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
                window,
                renderer_rec,
            } = self
            else {
                unreachable!();
            };
            // let mut renderer_received = false;
            use winit::platform::web::WindowExtWebSys;
            if let Some(receiver) = renderer_rec.as_mut() {
                if let Ok(Some(renderer)) = receiver.try_recv() {
                    let window = window.as_ref().unwrap().clone();
                    window.set_prevent_default(false);
                    window.request_redraw();
                    let size = window.inner_size();
                    *self = Self::Init(Self::init_app(window, renderer));
                    let app = self.init_unwrap();
                    app.resize(size.width, size.height);
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
    ui: ui::State,

    dbg_wireframe: bool,
    renderer: Renderer,

    prev_frame_time: Instant,
    delta_time: Duration,

    mouse_pos: Vec2,

    last_size: UVec2,
    window: Arc<Window>,
    windows: Vec<Arc<Window>>,
}

impl App {
    pub fn new(renderer: Renderer, window: impl Into<Arc<Window>>) -> Self {
        let window: Arc<_> = window.into();
        Self {
            ui: ui::State::new(renderer.wgpu.clone(), window.clone()),
            dbg_wireframe: false,
            renderer,
            prev_frame_time: Instant::now(),
            delta_time: Duration::ZERO,
            mouse_pos: Vec2::NAN,
            last_size: UVec2::ONE,
            window: window.clone(),
            windows: Vec::new(),
        }
    }

    pub fn window_size(&self) -> UVec2 {
        let size = self.window.inner_size();
        (size.width, size.height).into()
    }

    pub fn width(&self) -> u32 {
        self.window_size().x
    }

    pub fn height(&self) -> u32 {
        self.window_size().y
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.width() as f32 / self.height() as f32
    }

    fn on_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        use WindowEvent as WE;
        // if self.window.id() != window_id {
        //     return;
        // }

        self.on_update();

        match event {
            WE::CursorMoved { position: pos, .. } => {
                self.mouse_pos = (pos.x as f32, pos.y as f32).into();
            }
            WE::MouseInput { state, button, .. } => {
                use winit::event::{ElementState, MouseButton};
                let state = match state {
                    ElementState::Pressed => true,
                    ElementState::Released => false,
                };

                match button {
                    MouseButton::Left => self.ui.set_mouse_press(MouseBtn::Left, state),
                    MouseButton::Right => self.ui.set_mouse_press(MouseBtn::Right, state),
                    MouseButton::Middle => self.ui.set_mouse_press(MouseBtn::Middle, state),
                    _ => (),
                }
            }
            WE::RedrawRequested => {
                self.on_redraw(event_loop);
            }
            WE::KeyboardInput { event, .. } => {
                self.on_keyboard(&event, event_loop);
            }
            WE::Resized(PhysicalSize { width, height }) => {
                let (width, height) = (width.max(1), height.max(1));
                self.last_size = (width, height).into();
                self.resize(width, height);
            }
            WE::CloseRequested => event_loop.exit(),
            _ => (),
        }
    }

    fn on_update(&mut self) {
        self.ui.set_mouse_pos(self.mouse_pos.x, self.mouse_pos.y);
        self.ui.start_frame();

        self.ui.begin_widget(
            "a",
            WidgetOpt::new()
                .fill(RGBA::INDIGO)
                .draggable()
                .spacing(40.0)
                .padding(30.0)
                .size_min_fit()
                .size_max_px(2000.0, 1300.0)
                .resizable()
                .corner_radius(10.0)
                .outline(RGBA::MAGENTA, 5.0)
                .pos_fix(100.0, 100.0), // .size_fit(), // .size_fix(500.0, 300.0)
                                        // .size_fit_x(),
        );

        static mut toggle: bool = false;

        if self.ui.add_button("hello") {
            unsafe {
                toggle = !toggle;
            }
        }

        if unsafe { toggle } {
            self.ui.begin_widget(
                "df",
                WidgetOpt::new()
                    .fill(RGBA::PASTEL_PURPLE)
                    .padding(100.0)
                    .size_px(200.0, 200.0)
            );
            self.ui.end_widget();
        }

        if self.ui.add_button("tes") {
            log::info!("tes");
        }

        if self.ui.add_button("abcdefghijklmnopqrstuvwxyz") {
            log::info!("abcdefghijklmnopqrstuvwxyz");
        }

        self.ui.begin_widget(
            "d",
            WidgetOpt::new()
                .fill(RGBA::PASTEL_PURPLE)
                .spacing(40.0)
                .padding(10.0)
                .layout_h()
                .corner_radius(10.0)
                .outline(RGBA::MAGENTA, 5.0)
                .size_x_fit()
                .size_y_fit(),
        );
        let (_, signal) = self.ui.begin_widget(
            "c",
            WidgetOpt::new()
                .fill(RGBA::GREEN)
                .clickable()
                .resizable()
                .corner_radius(100.0)
                .outline(RGBA::MAGENTA, 5.0)
                .size_min_px(0.0, 0.0),
        );

        if signal.released() {
            log::info!("inner rect released");
        }

        self.ui.end_widget();

        if self.ui.add_button("sdfsdfsdf") {
            log::info!("dsfsdfsfsdf");
        }

        self.ui.end_widget();

        // self.ui_state.add_widget(
        //     "b",
        //     WidgetOpt::new()
        //         .fill(RGBA::BLUE)
        //         .clickable()
        //         .size_fix(50.0, 800.0),
        // );

        // self.ui_state.end_widget();
        self.ui.end_widget();

        self.ui.begin_widget(
            "d",
            WidgetOpt::new()
                .fill(RGBA::BLUE)
                .draggable()
                .spacing(15.0)
                .padding(100.0)
                .resizable()
                .corner_radius(10.0)
                .outline(RGBA::RED, 5.0)
                .pos_fix(100.0, 100.0)
                .size_min_fit(),
        );
        self.ui.end_widget();

        self.ui.draw_dbg_wireframe = self.dbg_wireframe;
        self.ui.end_frame();
    }

    fn on_keyboard(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                event_loop.exit();
            }
            PhysicalKey::Code(KeyCode::KeyD) => {
                if event.state.is_pressed() {
                    self.dbg_wireframe = !self.dbg_wireframe;
                }
            }
            PhysicalKey::Code(KeyCode::KeyR) => {
                if self.windows.len() < 3 {
                    let window = event_loop
                        .create_window(Window::default_attributes())
                        .unwrap();
                    self.windows.push(Arc::new(window))
                }
                // let shader = ColorTint(RGBA::rand());
                // shader.try_rebuild(&[(&VertexPosCol::desc(), "Vertex")], &self.renderer.wgpu);
            }
            _ => (),
        }
    }

    fn on_redraw(&mut self, event_loop: &ActiveEventLoop) {
        let prev_time = self.prev_frame_time;
        let curr_time = Instant::now();
        let dt = curr_time - prev_time;
        self.prev_frame_time = curr_time;
        self.delta_time = dt;


        self.window.pre_present_notify();
        let status = self.renderer.prepare_frame();
        match status {
            Ok(_) => (),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let size = self.window.inner_size();
                self.renderer.resize(size.width, size.height);
                return;
            }
            Err(e) => {
                log::error!("prepare_frame: {e}");
                panic!();
            }
        }

        {
            let mut surface = self.renderer.surface_target();
            surface.render(&ClearScreen(RGBA::PASTEL_MINT));
            surface.render(&self.ui);
        }

        self.renderer.present_frame();
        self.window.request_redraw();
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.renderer.resize(w, h);
    }
}
