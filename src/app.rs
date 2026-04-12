use egui::{Align2, Slider};
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
    held_down_keys: HashSet<KeyCode>,
    last_draw_time: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = WindowAttributes::default().with_title("Voxel Engine");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

        self.window = Some(window.clone());
        self.settings.image_settings.resolution = window.inner_size().into();

        let engine = Engine::<WindowedEngine>::new(window.inner_size().into(), window, event_loop);
        self.engine = Some(engine);

        // Make sure that we set all the settings before we render anything
        self.handle_settings_change();
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
            WindowEvent::Resized(new_size) => {
                self.settings.image_settings.resolution = new_size.into();
                let window = self.window.as_ref().unwrap();
                let engine = self.engine.as_mut().unwrap();
                engine.resize_window(window);
                engine.resize_output_image(self.settings.image_settings.output_image_size());
            }
            WindowEvent::RedrawRequested => {
                // get delta
                let delta_time = self.last_draw_time.elapsed().as_secs_f32();
                self.last_draw_time = Instant::now();

                // Move the camera
                self.settings.camera_settings.has_changed = self.handle_camera_movement(delta_time);

                // change the settings before drawing
                self.handle_settings_change();

                let window = self.window.as_ref().unwrap();
                let engine = self.engine.as_mut().unwrap();

                // render the frame + gui
                let rendering_time_ns = engine.image_draw_time_ns();
                engine.draw_with_gui(window, |gui| {
                    Self::render_gui(gui, &mut self.settings, rendering_time_ns);
                });

                window.request_redraw();
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
                    has_changed: true,
                },
                shader_settings: ShaderSettings {
                    show_traversal_color: false,
                    has_changed: true,
                },
                image_settings: ImageSettings {
                    resolution: [0, 0],
                    resolution_multiplier: 1.0,
                    has_changed: true,
                },
            },
            held_down_keys: HashSet::new(),
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
        if held_keys.contains(&KeyCode::ControlLeft) {
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

        let mut handled = false;

        if held_keys.contains(&KeyCode::ArrowRight) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, 0.0, radial_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowLeft) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, 0.0, -radial_speed);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowDown) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, radial_speed, 0.0);
            handled = true;
        }
        if held_keys.contains(&KeyCode::ArrowUp) {
            self.settings
                .camera_settings
                .camera
                .move_spherically_while_looking_at(center, -radial_speed, 0.0);
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

        return handled;
    }

    fn render_gui(gui: &mut Gui, settings: &mut AppSettings, rendering_time_ns: f64) {
        let ctx = gui.context();
        let render_time_ms = rendering_time_ns / 1_000_000.0;
        let mut camera_changed = false;
        let mut shader_settings_changed = false;
        let mut image_settings_changed = false;
        egui::Window::new("Settings")
            .anchor(Align2::LEFT_TOP, [5.0, 5.0])
            .show(&ctx, |ui| {
                ui.label(format!("Rendering Time: {render_time_ms:.3}ms"));

                ui.collapsing("Image", |ui| {
                    ui.horizontal(|ui| {
                        image_settings_changed = ui
                            .add(
                                Slider::new(
                                    &mut settings.image_settings.resolution_multiplier,
                                    0.1..=1.0,
                                )
                                .text("Resolution: ")
                                .show_value(false),
                            )
                            .changed()
                            || image_settings_changed;
                        let size = settings.image_settings.output_image_size();
                        let width = size[0];
                        let height = size[1];
                        ui.label(format!("{width}x{height}"));
                    });
                });

                // Camera Drawing
                ui.collapsing("Camera", |ui| {
                    ui.add(
                        Slider::new(&mut settings.camera_settings.speed, 0.0..=100.0).text("Speed"),
                    );

                    camera_changed = ui
                        .add(
                            Slider::new(
                                &mut settings.camera_settings.camera.field_of_view,
                                10.0..=90.0,
                            )
                            .text("Field of view"),
                        )
                        .changed()
                        || camera_changed;

                    let camera_handling_changed = ui
                        .checkbox(&mut settings.camera_settings.camera_centered, "Center")
                        .changed();

                    if camera_handling_changed && settings.camera_settings.camera_centered {
                        settings.camera_settings.camera.look_at(Vec3::ZERO);
                    }
                });

                ui.collapsing("Shader", |ui| {
                    shader_settings_changed = ui
                        .checkbox(
                            &mut settings.shader_settings.show_traversal_color,
                            "Show Traveral Steps",
                        )
                        .changed()
                        || shader_settings_changed;
                });
            });

        settings.camera_settings.has_changed = camera_changed;
        settings.shader_settings.has_changed = shader_settings_changed;
        settings.image_settings.has_changed = image_settings_changed;
    }

    fn handle_settings_change(&mut self) {
        let engine = self.engine.as_mut().unwrap();
        if self.settings.camera_settings.has_changed {
            engine.set_camera(self.settings.camera_settings.camera);
            self.settings.camera_settings.has_changed = false;
        }

        if self.settings.shader_settings.has_changed {
            let flags = self.settings.shader_settings.show_traversal_color as u8;
            engine.set_shader_flags(flags);
            self.settings.shader_settings.has_changed = false;
        }

        if self.settings.image_settings.has_changed {
            engine.resize_output_image(self.settings.image_settings.output_image_size());
        }
    }
}

struct AppSettings {
    camera_settings: CameraSettings,
    shader_settings: ShaderSettings,
    image_settings: ImageSettings,
}

struct CameraSettings {
    camera_centered: bool,
    speed: f32,
    camera: Camera,
    has_changed: bool,
}

struct ShaderSettings {
    show_traversal_color: bool,
    has_changed: bool,
}

struct ImageSettings {
    resolution: [u32; 2],
    resolution_multiplier: f32,
    has_changed: bool,
}

impl ImageSettings {
    fn output_image_size(&self) -> [u32; 2] {
        let multiplier = self.resolution_multiplier;
        let width = (self.resolution[0] as f32 * multiplier) as u32;
        let height = (self.resolution[1] as f32 * multiplier) as u32;
        [width, height]
    }
}
