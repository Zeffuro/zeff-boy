use criterion::{Criterion, criterion_group, criterion_main};
use std::path::Path;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::MasterTicks;
use zeff_nes_core::emulator::Emulator;

fn build_minimal_nes_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let prg = 16;
    rom[prg] = 0x78;
    rom[prg + 1] = 0xD8;
    rom[prg + 2] = 0xA2;
    rom[prg + 3] = 0xFF;
    rom[prg + 4] = 0x9A;
    rom[prg + 5] = 0xEA;
    rom[prg + 6] = 0x4C;
    rom[prg + 7] = 0x06;
    rom[prg + 8] = 0x80;

    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;

    rom[prg + 0x3FFA] = 0x06;
    rom[prg + 0x3FFB] = 0x80;

    rom[prg + 0x3FFE] = 0x06;
    rom[prg + 0x3FFF] = 0x80;

    rom
}

fn create_emulator() -> Emulator {
    let rom = build_minimal_nes_rom();
    Emulator::new(&rom, 44_100.0).expect("NES emulator should initialize")
}

fn bench_step_instruction(c: &mut Criterion) {
    let mut emu = create_emulator();

    c.bench_function("nes_step_instruction", |b| {
        b.iter(|| std::hint::black_box(emu.step_instruction()));
    });

    let mut traced = create_emulator();
    c.bench_function("nes_step_instruction_with_bus_trace", |b| {
        b.iter(|| std::hint::black_box(traced.step_instruction_with_bus_trace()));
    });
}

fn bench_cpu_memory_access(c: &mut Criterion) {
    let mut emu = create_emulator();

    c.bench_function("nes_cpu_memory_access", |b| {
        b.iter(|| {
            let value = emu.cpu_peek8(0x0000);
            emu.cpu_write8(0x0000, value.wrapping_add(1));
            std::hint::black_box(value);
        });
    });
}

fn build_bus_trace() -> [BusAccessEvent; 7] {
    [
        BusAccessEvent::Write {
            at: Some(MasterTicks::new(0)),
            space: TraceWriteKind::Memory,
            addr: 0x0000,
            old_value: 0,
            written_value: 0x12,
            new_value: 0x12,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        },
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(1)),
            space: TraceWriteKind::Memory,
            addr: 0x0800,
            value: 0x12,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        },
        BusAccessEvent::Write {
            at: Some(MasterTicks::new(2)),
            space: TraceWriteKind::Memory,
            addr: 0x07FF,
            old_value: 0,
            written_value: 0x34,
            new_value: 0x34,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        },
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(3)),
            space: TraceWriteKind::Memory,
            addr: 0x17FF,
            value: 0x34,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        },
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(4)),
            space: TraceWriteKind::Memory,
            addr: 0x8000,
            value: 0x78,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        },
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(5)),
            space: TraceWriteKind::Memory,
            addr: 0xC000,
            value: 0x78,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        },
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(6)),
            space: TraceWriteKind::Memory,
            addr: 0xFFFC,
            value: 0,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        },
    ]
}

fn bench_bus_trace_replay(c: &mut Criterion) {
    let trace = build_bus_trace();
    let mut emu = create_emulator();

    c.bench_function("nes_bus_trace_replay", |b| {
        b.iter(|| {
            for event in &trace {
                match *event {
                    BusAccessEvent::Read { addr, .. } => {
                        std::hint::black_box(emu.cpu_peek8(addr as u16));
                    }
                    BusAccessEvent::Write {
                        addr, new_value, ..
                    } => emu.cpu_write8(addr as u16, new_value as u8),
                }
            }
        });
    });
}

fn bench_step_frame(c: &mut Criterion) {
    let mut emu = create_emulator();

    for _ in 0..10 {
        emu.step_frame();
    }

    c.bench_function("nes_step_frame", |b| {
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

    c.bench_function("nes_save_state_encode", |b| {
        b.iter(|| {
            let _ = emu.encode_state().unwrap();
        });
    });
}

fn bench_save_state_roundtrip(c: &mut Criterion) {
    let mut emu = create_emulator();

    for _ in 0..60 {
        emu.step_frame();
    }
    let state = emu.encode_state().unwrap();

    c.bench_function("nes_save_state_roundtrip", |b| {
        b.iter(|| {
            emu.load_state_from_bytes(state.clone()).unwrap();
        });
    });
}

fn bench_audio_drain(c: &mut Criterion) {
    let mut emu = create_emulator();
    emu.set_apu_sample_generation_enabled(true);

    emu.step_frame();

    c.bench_function("nes_audio_drain", |b| {
        b.iter(|| {
            emu.step_frame();
            let samples = emu.drain_audio_samples();
            std::hint::black_box(samples);
        });
    });
}

// Real ROM benchmarks, manifest: test-roms/nes-bench-roms.txt
fn load_bench_manifest() -> Vec<(String, String)> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-roms/nes-bench-roms.txt");
    let Ok(contents) = std::fs::read_to_string(&manifest) else {
        eprintln!("bench manifest not found: {}", manifest.display());
        return Vec::new();
    };
    contents
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (label, path) = l.split_once('\t')?;
            Some((label.trim().to_string(), path.trim().to_string()))
        })
        .collect()
}

fn bench_nes_real_roms(c: &mut Criterion) {
    let test_roms_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-roms");
    for (label, rom_path) in load_bench_manifest() {
        let full = test_roms_dir.join(&rom_path);
        let Ok(data) = std::fs::read(&full) else {
            eprintln!("skipping {label}: ROM not found at {}", full.display());
            continue;
        };
        let Ok(mut emu) = Emulator::new(&data, 44_100.0) else {
            eprintln!("skipping {label}: failed to load ROM");
            continue;
        };
        for _ in 0..60 {
            emu.step_frame();
        }
        c.bench_function(&label, |b| {
            b.iter(|| emu.step_frame());
        });
    }
}

criterion_group!(
    benches,
    bench_step_instruction,
    bench_cpu_memory_access,
    bench_bus_trace_replay,
    bench_step_frame,
    bench_save_state_encode,
    bench_save_state_roundtrip,
    bench_audio_drain,
    bench_nes_real_roms,
);
criterion_main!(benches);
