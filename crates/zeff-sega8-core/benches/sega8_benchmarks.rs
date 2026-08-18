use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::cartridge::SystemHint;

const WARMUP_FRAMES: usize = 16;

fn build_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[..7].copy_from_slice(&[0x3E, 0x42, 0x3C, 0x3D, 0xC3, 0x00, 0x00]);
    rom[0x7FF0..0x7FF8].copy_from_slice(b"TMR SEGA");
    rom[0x7FFF] = 0x4C;
    rom
}

fn create_emulator() -> Emulator {
    let rom = build_rom();
    let mut emu = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem)
        .expect("benchmark ROM should load");
    emu.set_apu_sample_generation_enabled(false);
    emu
}

fn warm_up(emu: &mut Emulator) {
    for _ in 0..WARMUP_FRAMES {
        emu.step_frame();
    }
}

fn bench_instruction_step(c: &mut Criterion) {
    let mut emu = create_emulator();
    warm_up(&mut emu);

    c.bench_function("sega8_instruction_step", |b| {
        b.iter(|| black_box(emu.step_instruction()));
    });
}

fn bench_frame_step(c: &mut Criterion) {
    let mut emu = create_emulator();
    warm_up(&mut emu);

    c.bench_function("sega8_frame_step", |b| {
        b.iter(|| {
            emu.step_frame();
            black_box(emu.frame_count());
        });
    });
}

fn bench_cpu_memory_access(c: &mut Criterion) {
    let mut emu = create_emulator();

    c.bench_function("sega8_cpu_memory_access", |b| {
        b.iter(|| {
            let value = emu.cpu_peek8(0xC000);
            emu.cpu_write8(0xC000, value.wrapping_add(1));
            black_box(value);
        });
    });
}

fn bench_state_encode(c: &mut Criterion) {
    let mut emu = create_emulator();
    warm_up(&mut emu);

    c.bench_function("sega8_state_encode", |b| {
        b.iter(|| black_box(emu.encode_state().expect("state should encode")));
    });
}

fn bench_state_load(c: &mut Criterion) {
    let mut emu = create_emulator();
    warm_up(&mut emu);
    let state = emu.encode_state().expect("state should encode");

    c.bench_function("sega8_state_load", |b| {
        b.iter(|| {
            emu.load_state(black_box(&state))
                .expect("state should load");
            black_box(emu.frame_count());
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
