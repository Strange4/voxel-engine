use crate::engine::Engine;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes},
};

#[derive(Default)]
pub struct App {
    window: Option<Arc<Window>>,
    engine: Option<Engine>,
    window_resize: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(WindowAttributes::default())
                .unwrap(),
        );
        self.window = Some(window.clone());

        self.engine = Some(Engine::new(window))
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let engine = self.engine.as_mut().unwrap();
        let window = self.window.as_ref().unwrap();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => self.window_resize = true,
            WindowEvent::Focused(_) => self.window_resize = true,
            WindowEvent::RedrawRequested => {
                engine.draw(window, self.window_resize);
                self.window_resize = false;
                window.request_redraw();
            }
            _ => {}
        };
    }
}
