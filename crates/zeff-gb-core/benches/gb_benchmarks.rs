use criterion::{Criterion, criterion_group, criterion_main};
use zeff_gb_core::emulator::Emulator;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

fn build_minimal_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];

    let mut checksum: u8 = 0;
    for &byte in &rom[0x134..=0x14C] {
        checksum = checksum.wrapping_sub(byte).wrapping_sub(1);
    }
    rom[0x14D] = checksum;

    rom[0x150] = 0x00; // NOP
    rom[0x151] = 0x00; // NOP
    rom[0x152] = 0x18; // JR -2
    rom[0x153] = 0xFE;

    rom
}

fn create_emulator() -> Emulator {
    let rom = build_minimal_rom();
    Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
        .expect("emulator should initialize from minimal ROM")
}

fn bench_step_instruction(c: &mut Criterion) {
    let mut emu = create_emulator();

    for _ in 0..10 {
        emu.step_frame();
    }

    c.bench_function("step_instruction", |b| {
        b.iter(|| {
            let _ = emu.step_instruction();
        });
    });
}

fn bench_step_frame(c: &mut Criterion) {
    let mut emu = create_emulator();

    // Warm up
    for _ in 0..10 {
        emu.step_frame();
    }

    c.bench_function("step_frame", |b| {
        b.iter(|| {
            emu.step_frame();
        });
    });
}

fn bench_save_state_encode(c: &mut Criterion) {
    let mut emu = create_emulator();

    for _ in 0..60 {
        emu.step_frame();
    }

    c.bench_function("save_state_encode", |b| {
        b.iter(|| {
            let _ = emu.encode_state_bytes().unwrap();
        });
    });
}

fn bench_save_state_roundtrip(c: &mut Criterion) {
    let mut emu = create_emulator();

    for _ in 0..60 {
        emu.step_frame();
    }
    let state = emu.encode_state_bytes().unwrap();

    c.bench_function("save_state_roundtrip", |b| {
        b.iter(|| {
            emu.load_state_from_bytes(state.clone()).unwrap();
        });
    });
}

fn bench_audio_drain(c: &mut Criterion) {
    let mut emu = create_emulator();
    emu.set_sample_rate(48000);
    emu.set_apu_sample_generation_enabled(true);

    emu.step_frame();

    c.bench_function("audio_drain", |b| {
        b.iter(|| {
            emu.step_frame();
            let samples = emu.drain_audio_samples();
            std::hint::black_box(samples);
        });
    });
}

fn bench_audio_drain_into(c: &mut Criterion) {
    let mut emu = create_emulator();
    emu.set_sample_rate(48000);
    emu.set_apu_sample_generation_enabled(true);

    let mut buf = Vec::with_capacity(4096);

    emu.step_frame();

    c.bench_function("audio_drain_into_reuse", |b| {
        b.iter(|| {
            emu.step_frame();
            buf.clear();
            emu.drain_audio_samples_into(&mut buf);
            std::hint::black_box(&buf);
        });
    });
}

criterion_group!(
    benches,
    bench_step_instruction,
    bench_step_frame,
    bench_save_state_encode,
    bench_save_state_roundtrip,
    bench_audio_drain,
    bench_audio_drain_into,
);
criterion_main!(benches);

