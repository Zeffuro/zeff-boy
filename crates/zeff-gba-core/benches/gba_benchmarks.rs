use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use zeff_gba_core::emulator::Emulator;

const WARMUP_FRAMES: usize = 8;

fn synthetic_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[..4].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
    rom[0xA0..0xA4].copy_from_slice(b"ZEFF");
    rom[0xAC..0xB0].copy_from_slice(b"ZBCH");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    rom
}

fn emulator() -> Emulator {
    let mut emu = Emulator::new(&synthetic_rom(), 48_000).expect("synthetic ROM should load");
    emu.set_apu_sample_generation_enabled(false);
    emu.set_apu_debug_capture_enabled(false);
    for _ in 0..WARMUP_FRAMES {
        emu.step_frame();
    }
    emu
}

fn bench_step_instruction(c: &mut Criterion) {
    let mut emu = emulator();

    c.bench_function("gba_step_instruction", |b| {
        b.iter(|| black_box(emu.step_instruction()));
    });
}

fn bench_step_frame(c: &mut Criterion) {
    let mut emu = emulator();

    c.bench_function("gba_step_frame", |b| {
        b.iter(|| {
            emu.step_frame();
            black_box(emu.frame_count());
        });
    });
}

fn bench_cpu_memory_access(c: &mut Criterion) {
    let mut emu = emulator();

    c.bench_function("gba_cpu_memory_access", |b| {
        b.iter(|| {
            let value = emu.cpu_peek8(0x0200_0000);
            emu.cpu_write8(0x0200_0000, value.wrapping_add(1));
            black_box(value);
        });
    });
}

fn bench_save_state_encode(c: &mut Criterion) {
    let emu = emulator();

    c.bench_function("gba_save_state_encode", |b| {
        b.iter(|| black_box(emu.encode_state().expect("state should encode")));
    });
}

fn bench_save_state_roundtrip(c: &mut Criterion) {
    let mut emu = emulator();

    c.bench_function("gba_save_state_roundtrip", |b| {
        b.iter(|| {
            let state = black_box(emu.encode_state().expect("state should encode"));
            emu.load_state(black_box(&state))
                .expect("state should decode");
            black_box(emu.cpu_cycles());
        });
    });
}

criterion_group!(
    benches,
    bench_step_instruction,
    bench_cpu_memory_access,
    bench_step_frame,
    bench_save_state_encode,
    bench_save_state_roundtrip,
);
criterion_main!(benches);
