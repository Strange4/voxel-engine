use egui::{Align2, RichText};
use egui_winit_vulkano::Gui;
use glam::Vec3;
use std::{collections::HashSet, f32, sync::Arc, time::Instant};
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

pub struct App {
    window: Option<Arc<Window>>,
    engine: Option<Engine<WindowedEngine>>,
    settings: AppSettings,
    window_resize: bool,
    held_down_keys: HashSet<KeyCode>,
    last_draw_time: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = WindowAttributes::default().with_title("Voxel Engine");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        self.window = Some(window.clone());
        let mut engine = Engine::<WindowedEngine>::new(window, event_loop);
        engine.set_camera(self.settings.camera_settings.camera);
        self.engine = Some(engine);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let gui_event = self.engine.as_mut().unwrap().handle_event(&event);
        if gui_event {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => self.window_resize = true,
            WindowEvent::Focused(_) => self.window_resize = true,
            WindowEvent::RedrawRequested => {
                let delta_time = self.last_draw_time.elapsed().as_secs_f32();
                self.last_draw_time = Instant::now();
                let window = self.window.as_ref().unwrap();
                let engine = self.engine.as_mut().unwrap();

                if self.settings.has_changed {
                    engine.set_camera(self.settings.camera_settings.camera);
                    self.settings.has_changed = false;
                }

                let rendering_time_ns = engine.image_draw_time_ns();
                engine.draw_with_gui(window, self.window_resize, |gui| {
                    Self::render_gui(gui, &mut self.settings, rendering_time_ns);
                });
                self.window_resize = false;
                window.request_redraw();
                if self.handle_camera_movement(delta_time) {
                    self.settings.has_changed = true;
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key_code),
                        state,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                if state == ElementState::Pressed {
                    self.held_down_keys.insert(key_code);
                } else {
                    self.held_down_keys.remove(&key_code);
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
            settings: AppSettings {
                camera_settings: CameraSettings {
                    camera_centered: true,
                    speed: 25.0,
                    camera: Camera::new_at(50.0, -50.0, -50.0),
                },
                has_changed: true,
            },
            held_down_keys: HashSet::new(),
            window_resize: false,
            last_draw_time: Instant::now(),
        }
    }

    fn handle_camera_movement(&mut self, delta_time: f32) -> bool {
        if self.settings.camera_settings.camera_centered {
            return self.handle_spherical_camera_movement(delta_time);
        } else {
            return self.handle_free_camera_movement(delta_time);
        }
    }

    fn handle_free_camera_movement(&mut self, delta_time: f32) -> bool {
        let rotation_speed = self.settings.camera_settings.speed * 0.1 * delta_time;
        let translation_speed = self.settings.camera_settings.speed * delta_time;
        let held_keys = &self.held_down_keys;
        let mut handled = false;

        if held_keys.contains(&KeyCode::ArrowRight) {
            self.settings
                .camera_settings
                .camera
                .rotate_horizontally(-rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowLeft) {
            self.settings
                .camera_settings
                .camera
                .rotate_horizontally(rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowDown) {
            self.settings
                .camera_settings
                .camera
                .rotate_vertically(-rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowUp) {
            self.settings
                .camera_settings
                .camera
                .rotate_vertically(rotation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyW) {
            self.settings
                .camera_settings
                .camera
                .move_forward(translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyS) {
            self.settings
                .camera_settings
                .camera
                .move_forward(-translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyD) {
            self.settings
                .camera_settings
                .camera
                .move_right(translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyA) {
            self.settings
                .camera_settings
                .camera
                .move_right(-translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::Space) {
            self.settings
                .camera_settings
                .camera
                .move_up(translation_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::KeyC) {
            self.settings
                .camera_settings
                .camera
                .move_up(-translation_speed);
            handled = true;
        }
        return handled;
    }

    fn handle_spherical_camera_movement(&mut self, delta_time: f32) -> bool {
        let center = Vec3::ZERO;
        let radial_speed = self.settings.camera_settings.speed * 0.1 * delta_time;
        let translation_speed = self.settings.camera_settings.speed * delta_time;

        let held_keys = &self.held_down_keys;

        if held_keys.contains(&KeyCode::ArrowRight) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, 0.0, radial_speed)
        }
        if held_keys.contains(&KeyCode::ArrowLeft) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, 0.0, -radial_speed)
        }
        if held_keys.contains(&KeyCode::ArrowDown) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, radial_speed, 0.0)
        }
        if held_keys.contains(&KeyCode::ArrowUp) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, -radial_speed, 0.0)
        }
        if held_keys.contains(&KeyCode::KeyW) {
            self.settings
                .camera_settings
                .camera
                .move_forward(translation_speed)
        }
        if held_keys.contains(&KeyCode::KeyS) {
            self.settings
                .camera_settings
                .camera
                .move_forward(-translation_speed)
        }

        return true;
    }

    fn render_gui(gui: &mut Gui, settings: &mut AppSettings, rendering_time_ns: f64) {
        let ctx = gui.context();
        let render_time_ms = rendering_time_ns / 1_000_000.0;
        let mut settings_changed = false;
        egui::Window::new("Settings")
            .anchor(Align2::LEFT_TOP, [5.0, 5.0])
            .show(&ctx, |ui| {
                ui.label(format!("Rendering Time: {render_time_ms:.3}ms"));
                ui.label(RichText::new("Camera").size(20.0));
                settings_changed = ui
                    .add(
                        egui::Slider::new(&mut settings.camera_settings.speed, 0.0..=100.0)
                            .text("Speed"),
                    )
                    .changed()
                    || settings_changed;

                settings_changed = ui
                    .add(
                        egui::Slider::new(
                            &mut settings.camera_settings.camera.field_of_view,
                            10.0..=90.0,
                        )
                        .text("Field of view"),
                    )
                    .changed()
                    || settings_changed;

                let camera_handling_changed = ui
                    .add(egui::Checkbox::new(
                        &mut settings.camera_settings.camera_centered,
                        "Center",
                    ))
                    .changed();
                settings_changed = camera_handling_changed || settings_changed;

                if camera_handling_changed && settings.camera_settings.camera_centered {
                    settings.camera_settings.camera.look_at(Vec3::ZERO);
                }
            });

        settings.has_changed = settings_changed;
    }
}

struct AppSettings {
    camera_settings: CameraSettings,
    has_changed: bool,
}

struct CameraSettings {
    camera_centered: bool,
    speed: f32,
    camera: Camera,
}
