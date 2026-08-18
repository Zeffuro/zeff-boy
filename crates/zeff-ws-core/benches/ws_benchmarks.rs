use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use zeff_ws_core::emulator::Emulator;

const WARMUP_FRAMES: usize = 10;

fn build_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x1_0000];
    rom[..4].copy_from_slice(&[0x90, 0x90, 0xEB, 0xFC]);
    rom[0xFFF0..0xFFF5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);

    let footer = rom.len() - 10;
    rom[footer..footer + 8].fill(0);
    rom[footer + 4] = 0x01;
    let checksum = rom[..footer + 8]
        .iter()
        .fold(0u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn emulator() -> Emulator {
    let rom = build_rom();
    let mut emu = Emulator::from_rom_data(&rom).expect("synthetic ROM should load");
    emu.set_apu_sample_generation_enabled(false);
    for _ in 0..WARMUP_FRAMES {
        emu.step_frame();
    }
    emu
}

fn bench_instruction_step(c: &mut Criterion) {
    let mut emu = emulator();
    c.bench_function("ws_instruction_step", |b| {
        b.iter(|| black_box(emu.step_instruction()))
    });
}

fn bench_frame_step(c: &mut Criterion) {
    let mut emu = emulator();
    c.bench_function("ws_frame_step", |b| {
        b.iter(|| {
            emu.step_frame();
            black_box(emu.frame_count());
        })
    });
}

fn bench_cpu_memory_access(c: &mut Criterion) {
    let mut emu = emulator();

    c.bench_function("ws_cpu_memory_access", |b| {
        b.iter(|| {
            let value = emu.cpu_peek8(0x0000);
            emu.cpu_write8(0x0000, value.wrapping_add(1));
            black_box(value);
        });
    });
}

fn bench_state_encode(c: &mut Criterion) {
    let emu = emulator();
    c.bench_function("ws_state_encode", |b| {
        b.iter(|| black_box(emu.encode_state().expect("state should encode")))
    });
}

fn bench_state_load(c: &mut Criterion) {
    let mut emu = emulator();
    let state = emu.encode_state().expect("state should encode");
    c.bench_function("ws_state_load", |b| {
        b.iter(|| {
            emu.load_state(black_box(&state))
                .expect("state should load");
            black_box(emu.cpu_cycles());
        })
    });
}

criterion_group!(
    benches,
    bench_instruction_step,
    bench_cpu_memory_access,
    bench_frame_step,
    bench_state_encode,
    bench_state_load,
);
criterion_main!(benches);
