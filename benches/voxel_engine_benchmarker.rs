use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use voxel_engine::{
    camera::Camera,
    engine::{Engine, HeadlessEngine},
};

pub fn drawing_benchmark(c: &mut Criterion) {
    let mut engine = Engine::<HeadlessEngine>::new([1920, 1080]);

    engine.set_camera(Camera::new_at(0.0, -50.0, -50.0));

    c.bench_function("GPU draw time", |b| {
        b.iter_custom(|iter_count| {
            let mut sum = Duration::ZERO;
            let mut previous_time = Duration::ZERO;
            for _ in 0..iter_count {
                // timings aren't available sometimes so we'll take the previous timing
                let duration = match engine.draw() {
                    // TODO: figure out why the headless engine takes 90ms for one frame 😭
                    Some(time) => {
                        previous_time = Duration::from_nanos(time as u64);
                        previous_time
                    }
                    None => previous_time,
                };
                sum += duration;
            }
            sum
        })
    });

    c.bench_function("CPU draw time", |b| {
        b.iter(|| engine.draw());
    });
}

criterion_group!(benches, drawing_benchmark);
criterion_main!(benches);
