use crate::engine::Engine;
use core::f32;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes},
};

const RADIUS_MOVE_AMOUNT: f32 = f32::consts::FRAC_PI_8 / 2.0;

#[derive(Default)]
pub struct App {
    window: Option<Arc<Window>>,
    engine: Option<Engine>,
    window_resize: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = WindowAttributes::default().with_title("Voxel Engine");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
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
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key_code),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match key_code {
                KeyCode::ArrowRight => engine.move_horizontally(RADIUS_MOVE_AMOUNT),
                KeyCode::ArrowLeft => engine.move_horizontally(-RADIUS_MOVE_AMOUNT),
                KeyCode::ArrowDown => engine.move_vertically(-RADIUS_MOVE_AMOUNT),
                KeyCode::ArrowUp => engine.move_vertically(RADIUS_MOVE_AMOUNT),
                KeyCode::KeyW => engine.move_towards(-1.0),
                KeyCode::KeyS => engine.move_towards(1.0),
                _ => {}
            },
            _ => {}
        };
    }
}
