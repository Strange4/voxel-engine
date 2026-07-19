use egui::{Align2, Slider, Ui};
use egui_file_dialog::FileDialog;
use egui_winit_vulkano::Gui;
use glam::Vec3;
use std::{collections::HashSet, f32, path::Path, sync::Arc, thread, time::Instant};
use voxel_engine::{
    camera::Camera,
    engine::{Engine, WindowedEngine},
    voxel_data::XYZIVoxelData,
    voxel_loader::VoxFile,
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
    file_dialog: FileDialog,
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
                self.handle_camera_movement(delta_time);

                // change the settings before drawing
                self.handle_settings_change();

                let window = self.window.as_ref().unwrap();
                let engine = self.engine.as_mut().unwrap();

                // render the frame + gui
                let rendering_time_ns = engine.image_draw_time_ns();
                engine.draw_with_gui(window, |gui| {
                    Self::render_gui(
                        gui,
                        &mut self.settings,
                        rendering_time_ns,
                        &mut self.file_dialog,
                    );
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
        let model = XYZIVoxelData::from_vox_file(
            VoxFile::load_vox_file(Path::new("models/monu9.vox")).unwrap(),
        )
        .unwrap();
        let mut camera = Camera::new_at(5.0, 5.0, -5.0);
        camera.direction = Vec3::new(0.0, 0.0, 1.0).normalize();
        Self {
            window: None,
            engine: None,
            settings: AppSettings {
                camera_settings: CameraSettings {
                    camera_centered: false,
                    speed: 25.0,
                    camera,
                    has_changed: true,
                },
                shader_settings: ShaderSettings {
                    render_type: RenderType::Albedo,
                    has_changed: true,
                },
                image_settings: ImageSettings {
                    resolution: [0, 0],
                    resolution_multiplier: 1.0,
                    has_changed: true,
                },
                model_settings: ModelSettings {
                    model: Some(model),
                    has_changed: true,
                },
            },
            held_down_keys: HashSet::new(),
            last_draw_time: Instant::now(),
            file_dialog: FileDialog::new(),
        }
    }

    fn handle_camera_movement(&mut self, delta_time: f32) {
        let has_changed = if self.settings.camera_settings.camera_centered {
            self.handle_spherical_camera_movement(delta_time)
        } else {
            self.handle_free_camera_movement(delta_time)
        };

        self.settings.camera_settings.has_changed |= has_changed;
    }

    fn handle_free_camera_movement(&mut self, delta_time: f32) -> bool {
        let held_keys = &self.held_down_keys;
        let speed_boost = if held_keys.contains(&KeyCode::ShiftLeft) {
            1.5
        } else {
            1.0
        };
        let rotation_speed = self.settings.camera_settings.speed * 0.1 * delta_time * speed_boost;
        let translation_speed = self.settings.camera_settings.speed * delta_time * speed_boost;
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
        handled
    }

    fn handle_spherical_camera_movement(&mut self, delta_time: f32) -> bool {
        let center = Vec3::ZERO;
        let held_keys = &self.held_down_keys;
        let speed_boost = if held_keys.contains(&KeyCode::ShiftLeft) {
            1.5
        } else {
            1.0
        };
        let radial_speed = self.settings.camera_settings.speed * 0.1 * delta_time * speed_boost;
        let translation_speed = self.settings.camera_settings.speed * delta_time * speed_boost;

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

        handled
    }

    fn render_gui(
        gui: &mut Gui,
        settings: &mut AppSettings,
        rendering_time_ns: f64,
        file_dialog: &mut FileDialog,
    ) {
        let ctx = gui.context();
        let render_time_ms = rendering_time_ns / 1_000_000.0;
        let mut camera_changed = false;
        let mut shader_settings_changed = false;
        let mut image_settings_changed = false;
        egui::Window::new("Settings")
            .anchor(Align2::LEFT_TOP, [5.0, 5.0])
            .show(&ctx, |ui| {
                ui.label(format!("Rendering Time: {render_time_ms:0>6.3}ms"));

                if ui.button("Set Model").clicked() {
                    file_dialog.pick_file();
                }

                file_dialog.update(&ctx);

                Self::handle_file_dialog(file_dialog, ui, settings);

                ui.collapsing("Image", |ui| {
                    ui.horizontal(|ui| {
                        image_settings_changed |= ui
                            .add(
                                Slider::new(
                                    &mut settings.image_settings.resolution_multiplier,
                                    0.1..=1.0,
                                )
                                .text("Resolution: ")
                                .show_value(false),
                            )
                            .changed();
                        let size = settings.image_settings.output_image_size();
                        let width = size[0];
                        let height = size[1];
                        ui.label(format!("{width}x{height}"));
                    });
                });

                // Camera Drawing
                ui.collapsing("Camera", |ui| {
                    let position = settings.camera_settings.camera.position;
                    ui.label(format!(
                        "Position: ({:.2}, {:.2}, {:.2})",
                        position.x, position.y, position.z
                    ));
                    ui.add(
                        Slider::new(&mut settings.camera_settings.speed, 0.0..=100.0).text("Speed"),
                    );

                    camera_changed |= ui
                        .add(
                            Slider::new(
                                &mut settings.camera_settings.camera.field_of_view,
                                10.0..=90.0,
                            )
                            .text("Field of view"),
                        )
                        .changed();

                    let camera_handling_changed = ui
                        .checkbox(&mut settings.camera_settings.camera_centered, "Center")
                        .changed();

                    if camera_handling_changed && settings.camera_settings.camera_centered {
                        settings.camera_settings.camera.look_at(Vec3::ZERO);
                    }
                });

                ui.collapsing("Render Type", |ui| {
                    ui.vertical(|ui| {
                        for render_type in RenderType::iter() {
                            shader_settings_changed |= ui
                                .selectable_value(
                                    &mut settings.shader_settings.render_type,
                                    render_type,
                                    format!("{render_type:?}"),
                                )
                                .changed();
                        }
                    });
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
            let flags = 1 << (self.settings.shader_settings.render_type as u8);
            engine.set_shader_flags(flags);
            self.settings.shader_settings.has_changed = false;
        }

        if self.settings.image_settings.has_changed {
            engine.resize_output_image(self.settings.image_settings.output_image_size());
        }
        if self.settings.model_settings.has_changed {
            let model = self.settings.model_settings.model.take();
            if let Some(model) = model {
                engine.set_model(model);
            }
            self.settings.model_settings.has_changed = false;
        }
    }

    fn handle_file_dialog(file_dialog: &mut FileDialog, ui: &mut Ui, settings: &mut AppSettings) {
        let path = file_dialog.take_picked();
        if path.is_none() {
            return;
        }
        let path = path.unwrap();
        let vox_file = VoxFile::load_vox_file(&path);
        if vox_file.is_err() {
            ui.label(format!(
                "Failed to load vox file: {:?}",
                vox_file.err().unwrap()
            ));
            return;
        }
        let vox_file = vox_file.unwrap();
        let model = XYZIVoxelData::from_vox_file(vox_file);

        if model.is_err() {
            ui.label(format!("Failed to load model: {:?}", model.err().unwrap()));
            return;
        }
        let model = model.unwrap();

        settings.model_settings.model = Some(model);
        settings.model_settings.has_changed = true;
    }
}

struct AppSettings {
    camera_settings: CameraSettings,
    shader_settings: ShaderSettings,
    image_settings: ImageSettings,
    model_settings: ModelSettings,
}

struct CameraSettings {
    camera_centered: bool,
    speed: f32,
    camera: Camera,
    has_changed: bool,
}

struct ModelSettings {
    model: Option<XYZIVoxelData>,
    has_changed: bool,
}

struct ShaderSettings {
    render_type: RenderType,
    has_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RenderType {
    TraversalSteps,
    HitNormals,
    HitPosition,
    Albedo,
}

impl RenderType {
    fn iter() -> impl Iterator<Item = Self> {
        [
            Self::TraversalSteps,
            Self::HitNormals,
            Self::HitPosition,
            Self::Albedo,
        ]
        .into_iter()
    }
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
