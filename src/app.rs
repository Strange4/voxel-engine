use core::f32;
use glam::Vec3;
use std::{collections::HashSet, sync::Arc};
use voxel_engine::{
    camera::Camera,
    engine::{Engine, WindowedEngine},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes},
};

const RADIUS_MOVE_AMOUNT: f32 = f32::consts::FRAC_PI_8 / 2.0;

pub struct App {
    window: Option<Arc<Window>>,
    engine: Option<Engine<WindowedEngine>>,
    camera: Camera,
    settings: AppSettings,
    window_resize: bool,
    held_down_keys: HashSet<KeyCode>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = WindowAttributes::default().with_title("Voxel Engine");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        self.window = Some(window.clone());
        let mut engine = Engine::<WindowedEngine>::new(window, event_loop);
        engine.set_camera(self.camera);
        self.engine = Some(engine);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let engine = self.engine.as_mut().unwrap();
        let window = self.window.as_ref().unwrap();
        if engine.handle_event(&event) {
            return;
        }
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
                        state,
                        ..
                    },
                ..
            } => {
                if state == ElementState::Pressed {
                    self.held_down_keys.insert(key_code);
                } else {
                    self.held_down_keys.remove(&key_code);
                }

                if self.handle_camera_movement() {
                    self.engine.as_mut().unwrap().set_camera(self.camera);
                }
            }
            _ => {}
        };
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            window: None,
            engine: None,
            camera: Camera::new_at(0.0, 0.0, -50.0),
            settings: AppSettings {
                camera_settings: CameraSettings {
                    camera_centered: false,
                    focal_distance: 30.0,
                    aperture: 0.0,
                    speed: 1.0,
                },
            },
            held_down_keys: HashSet::new(),
            window_resize: false,
        }
    }

    fn handle_camera_movement(&mut self) -> bool {
        if self.settings.camera_settings.camera_centered {
            return self.handle_spherical_camera_movement();
        } else {
            return self.handle_free_camera_movement();
        }
    }

    fn handle_free_camera_movement(&mut self) -> bool {
        let rotation_speed = self.settings.camera_settings.speed * RADIUS_MOVE_AMOUNT;
        let translation_speed = self.settings.camera_settings.speed;
        let held_keys = &self.held_down_keys;
        let mut handled = false;

        if held_keys.contains(&KeyCode::ArrowRight) {
            self.camera.rotate_horizontally(-rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowLeft) {
            self.camera.rotate_horizontally(rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowDown) {
            self.camera.rotate_vertically(-rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowUp) {
            self.camera.rotate_vertically(rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyW) {
            self.camera.move_forward(translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyS) {
            self.camera.move_forward(-translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyD) {
            self.camera.move_right(translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyA) {
            self.camera.move_right(-translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::Space) {
            self.camera.move_up(translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyC) {
            self.camera.move_up(-translation_speed);
            handled = true;
        }
        return handled;
    }

    fn handle_spherical_camera_movement(&mut self) -> bool {
        let center = Vec3::new(0.0, 0.0, 0.0);
        let radial_speed = self.settings.camera_settings.speed * RADIUS_MOVE_AMOUNT;
        let translation_speed = self.settings.camera_settings.speed;
        let held_keys = &self.held_down_keys;

        if held_keys.contains(&KeyCode::ArrowRight) {
            self.camera
                .move_spherically_while_looking_at(center, 0.0, radial_speed)
        }
        if held_keys.contains(&KeyCode::ArrowLeft) {
            self.camera
                .move_spherically_while_looking_at(center, 0.0, -radial_speed)
        }
        if held_keys.contains(&KeyCode::ArrowDown) {
            self.camera
                .move_spherically_while_looking_at(center, -radial_speed, 0.0)
        }
        if held_keys.contains(&KeyCode::ArrowUp) {
            self.camera
                .move_spherically_while_looking_at(center, radial_speed, 0.0)
        }
        if held_keys.contains(&KeyCode::KeyW) {
            self.camera.move_forward(translation_speed)
        }
        if held_keys.contains(&KeyCode::KeyS) {
            self.camera.move_forward(-translation_speed)
        }

        return true;
    }
}

struct AppSettings {
    camera_settings: CameraSettings,
}

struct CameraSettings {
    camera_centered: bool,
    focal_distance: f32,
    aperture: f32,
    speed: f32,
}
