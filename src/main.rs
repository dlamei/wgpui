fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            unsafe {
                std::env::remove_var("WAYLAND_DISPLAY");
            }
        }

        unsafe {
            std::env::set_var("RUST_BACKTRACE", "1");
        }

        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            // .filter_module("cranelift_jit::backend", log::LevelFilter::Warn)
            .filter_module("wgpui", log::LevelFilter::Trace)
            // .filter_module("atlas", log::LevelFilter::Info)
            // .filter_module("wgpu_hal::auxil::dxgi", log::LevelFilter::Error)
            .filter_module("wgpu_hal", log::LevelFilter::Warn)
            .format_timestamp(None)
            .init();
    }

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let mut app = wgpui::app::AppSetup::default();
    event_loop.run_app(&mut app).unwrap();
}
