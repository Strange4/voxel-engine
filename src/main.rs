use std::time::Duration;

use voxel_engine::engine::HeadlessEngine;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;

mod app;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}
