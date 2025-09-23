use std::{collections::HashMap, sync::Arc};

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
    gpu::{self, WGPU, WGPUHandle, Window, WindowId},
    mouse::{self, MouseBtn},
    rect::Rect,
    ui::{self, WidgetId, WidgetOpt},
    ui2,
    utils::{self, Duration, Instant, RGBA},
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

        let (window, wgpu) = utils::futures::wait_for(async move {
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
                    window.core.borrow().raw.set_prevent_default(false);
                    window.request_redraw();
                    let size = window.window_size();
                    *self = Self::Init(App::new(wgpu, window));
                    let app = self.init_unwrap();
                    app.resize_main_window(size.x as u32, size.y as u32);
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
    pub ui2: ui2::Context,

    pub dbg_wireframe: bool,
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
            // ui: ui::State::new(wgpu.clone(), window.clone()),
            ui2: ui2::Context::new(wgpu.clone(), window),
            dbg_wireframe: false,
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
        let w_size = self.ui2.window.window_size();
        let w_rect = Rect::from_min_size(Vec2::ZERO, w_size);

        let resize_dir = ui::is_in_resize_region(w_rect, self.mouse_pos, self.ui2.resize_threshold);
        // let mut dragging = false;

        match event {
            WE::CursorMoved { position: pos, .. } => {
                self.mouse_pos = (pos.x as f32, pos.y as f32).into();
                // self.ui.set_mouse_pos(self.mouse_pos.x, self.mouse_pos.y);
                self.ui2.set_mouse_pos(self.mouse_pos.x, self.mouse_pos.y);
                if id == self.ui2.window.id && !self.ui2.window.raw.has_focus() {
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
                        // self.ui.set_mouse_press(MouseBtn::Left, pressed);
                        self.ui2.set_mouse_press(MouseBtn::Left, pressed);
                    }
                    MouseButton::Middle => {
                        // self.ui.set_mouse_press(MouseBtn::Middle, pressed);
                        self.ui2.set_mouse_press(MouseBtn::Middle, pressed);
                    }
                    MouseButton::Right => {
                        // self.ui.set_mouse_press(MouseBtn::Right, pressed);
                        self.ui2.set_mouse_press(MouseBtn::Left, pressed);
                    }
                    _ => (),
                }
            }
            WE::RedrawRequested => {
                if id == self.main_window {
                    self.on_update(event_loop);
                    let pid = self.ui2.find_panel_by_name("#ROOT_PANEL");
                    if self.ui2.close_pressed {
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
                self.ui2.resize_window(id, width, height);

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
        let ui = &mut self.ui2;
        ui.draw_debug = self.dbg_wireframe;
        ui.begin_frame();

        ui.next_panel_data.bg_color = RGBA::MAGENTA;
        ui.next_panel_data.outline = Some((RGBA::DARK_BLUE, 5.0));
        ui.begin("Debug");
        ui.button("test button", RGBA::WHITE);
        ui.button("the quick brown fox jumps over the lazy dog", RGBA::WHITE);
        ui.end();

        ui.end_frame(event_loop);

        // log::info!("");
        // log::info!("item : {}", ui.hot_id);
        // log::info!("panel: {}", ui.hot_panel_id);

        // let ui = &mut self.ui;

        // // ui.set_mouse_pos(self.mouse_pos.x, self.mouse_pos.y);
        // ui.start_frame();

        // ui.begin_window("Atlas");
        // ui.end_window();

        // ui.add_debug_window(self.delta_time);
        // // ui.add_window("window");

        // // ui.begin_window();

        // for i in 0..1 {
        //     ui.begin_widget(
        //         &format!("outer_{i}"),
        //         WidgetOpt::new()
        //             .draggable()
        //             .fill(RGBA::CARMINE)
        //             .corner_radius(40.0)
        //             .outline(RGBA::DARK_BLUE, 10.0)
        //             .padding(25.0)
        //             .size_fit(),
        //     );

        //     ui.add_label(&format!("window: {}", i + 1), 64.0);
        //     ui.offset_cursor_y(24.0);

        //     ui.begin_widget(
        //         "content",
        //         WidgetOpt::new()
        //             .fill(RGBA::INDIGO)
        //             .spacing(40.0)
        //             .padding(30.0)
        //             .size_min_fit()
        //             .size_max_px(1000.0, 1300.0)
        //             .size_fit()
        //             .resizable()
        //             .corner_radius(40.0),
        //     );

        //     static mut toggle: bool = false;

        //     if ui.add_button("hello") {
        //         unsafe {
        //             toggle = !toggle;
        //         }
        //     }

        //     if unsafe { toggle } {
        //         ui.begin_widget(
        //             "toggle_rect",
        //             WidgetOpt::new()
        //                 .fill(RGBA::PASTEL_MINT)
        //                 .padding(100.0)
        //                 .size_px(200.0, 200.0),
        //         );
        //         ui.end_widget();
        //     }

        //     if ui.add_button("test") {
        //         log::info!("test");
        //     }

        //     if ui.add_button("abcdefghijklmnopqrstuvwxyz") {
        //         log::info!("abcdefghijklmnopqrstuvwxyz");
        //     }

        //     let (id, _) = ui.begin_widget(
        //         "box 2",
        //         WidgetOpt::new()
        //             .fill(RGBA::CARMINE)
        //             .spacing(40.0)
        //             .padding(10.0)
        //             .layout_h()
        //             .corner_radius(30.0)
        //             .size_fit(),
        //     );

        //     let (_, signal) = ui.begin_widget(
        //         "green circle",
        //         WidgetOpt::new()
        //             .fill(RGBA::GREEN)
        //             .clickable()
        //             .resizable()
        //             .corner_radius(100.0),
        //     );

        //     if signal.double_clicked() {
        //         log::info!("inner rect released");
        //     }

        //     ui.end_widget();

        //     let offset = ui[id].rect.height() / 2.0;
        //     ui.offset_cursor_y(offset);
        //     ui.set_next_placement_y(ui::Placement::Center);
        //     // ui.set_next_widget_placement(ui::WidgetPlacement::Center);

        //     if ui.add_button("hello world") {
        //         log::info!("hello world");
        //     }

        //     ui.end_widget();

        //     ui.end_widget();
        //     ui.end_widget();
        // }

        // ui.draw_dbg_wireframe = self.dbg_wireframe;

        // ui.end_frame();
    }

    fn on_keyboard(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event.physical_key {
            PhysicalKey::Code(KeyCode::KeyD) => {
                if event.state.is_pressed() {
                    self.dbg_wireframe = !self.dbg_wireframe;
                }
            }
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
            let window = self.ui2.get_mut_window(id);
            let Some(mut target) = window.prepare_frame(&self.wgpu) else {
                return;
            };

            target.render(&ClearScreen(RGBA::rgba_f(0.0, 0.0, 0.0, 0.0)));
            // target.render(&self.ui.draw);
            target.render(&self.ui2.draw);
        }

        let window = self.ui2.get_mut_window(id);
        window.present_frame();
        window.request_redraw();
    }
}
