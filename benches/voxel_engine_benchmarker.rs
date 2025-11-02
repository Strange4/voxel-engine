use std::{f32, time::Duration};

use criterion::{Criterion, criterion_group, criterion_main};
use voxel_engine::engine::{BetterEngine, HeadlessEngine};

pub fn drawing_benchmark(c: &mut Criterion) {
    let mut engine = BetterEngine::<HeadlessEngine>::new([1920, 1080]);

    c.bench_function("GPU draw time", |b| {
        b.iter_custom(|iter_count| {
            let mut sum = Duration::ZERO;
            let mut previous_time = Duration::ZERO;
            for _ in 0..iter_count {
                // timings aren't available sometimes so we'll take the previous timing
                let duration = match engine.draw() {
                    Some(time) => {
                        previous_time = Duration::from_nanos(time as u64);
                        previous_time
                    },
                    None => previous_time,
                };
                sum += duration;
            }
            sum
        })
    });

    engine.move_horizontally(f32::consts::FRAC_PI_4);
    engine.move_vertically(f32::consts::FRAC_PI_4);

    c.bench_function("GPU draw time big area angle", |b| {
        b.iter_custom(|iter_count| {
            let mut sum = Duration::ZERO;
            let mut previous_time = Duration::ZERO;
            for _ in 0..iter_count {
                // timings aren't available sometimes so we'll take the previous timing
                let duration = match engine.draw() {
                    Some(time) => {
                        previous_time = Duration::from_nanos(time as u64);
                        previous_time
                    },
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
