use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use zeff_coleco_core::Emulator;
use zeff_coleco_core::constants::BIOS_SIZE;

const WARMUP_FRAMES: usize = 16;

fn create_emulator() -> Emulator {
    let mut bios = vec![0; BIOS_SIZE];
    bios[..7].copy_from_slice(&[0x21, 0x00, 0x60, 0x34, 0xC3, 0x03, 0x00]);
    let mut cartridge = vec![0; 8 * 1024];
    cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
    let mut emulator = Emulator::new(&cartridge, &bios, 48_000).expect("benchmark machine");
    emulator.set_audio_generation_enabled(false);
    emulator
}

fn warm_up(emulator: &mut Emulator) {
    for _ in 0..WARMUP_FRAMES {
        emulator.step_frame();
    }
}

fn bench_instruction_step(c: &mut Criterion) {
    let mut emulator = create_emulator();
    warm_up(&mut emulator);
    c.bench_function("coleco_instruction_step", |b| {
        b.iter(|| black_box(emulator.step_instruction()));
    });
}

fn bench_cpu_memory_access(c: &mut Criterion) {
    let mut emulator = create_emulator();
    c.bench_function("coleco_cpu_memory_access", |b| {
        b.iter(|| {
            let value = emulator.bus().cpu_read(0x6000);
            emulator.bus_mut().cpu_write(0x6000, value.wrapping_add(1));
            black_box(value);
        });
    });
}

fn bench_frame_step(c: &mut Criterion) {
    let mut emulator = create_emulator();
    warm_up(&mut emulator);
    c.bench_function("coleco_frame_step", |b| {
        b.iter(|| {
            emulator.step_frame();
            black_box(emulator.frame_count());
        });
    });
}

fn bench_state_encode(c: &mut Criterion) {
    let mut emulator = create_emulator();
    warm_up(&mut emulator);
    c.bench_function("coleco_state_encode", |b| {
        b.iter(|| black_box(emulator.save_state().expect("state encode")));
    });
}

fn bench_state_load(c: &mut Criterion) {
    let mut emulator = create_emulator();
    warm_up(&mut emulator);
    let state = emulator.save_state().expect("state encode");
    c.bench_function("coleco_state_load", |b| {
        b.iter(|| {
            emulator.load_state(black_box(&state)).expect("state load");
            black_box(emulator.frame_count());
        });
    });
}

criterion_group!(
    benchmarks,
    bench_instruction_step,
    bench_cpu_memory_access,
    bench_frame_step,
    bench_state_encode,
    bench_state_load,
);
criterion_main!(benchmarks);
