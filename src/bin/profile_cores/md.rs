use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::{Duration, Instant};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::constants::{MAX_CARTRIDGE_SIZE, MAX_STATE_BYTES};
use zeff_md_core::{ControllerKind, ControllerPort, Emulator, LoadConfig, Region};

fn read_bounded(path: &std::ffi::OsStr, limit: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .expect("open MD profile input")
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .expect("read MD profile input");
    assert!(bytes.len() <= limit, "MD profile input exceeds size bound");
    bytes
}

fn step_field(machine: &mut Emulator) {
    let ticks = machine.region().frame_ticks();
    let now = machine.timing_snapshot().now().get();
    let end = (now / ticks)
        .checked_add(1)
        .and_then(|field| field.checked_mul(ticks))
        .expect("MD profile clock overflow");
    machine.clear_frame_ready();
    while machine.timing_snapshot().now().get() < end {
        machine
            .step_instruction()
            .expect("MD profile machine fault");
    }
    assert_eq!(machine.video_error(), None, "MD profile video fault");
}

pub(super) fn profile(frames: u32, audio_enabled: bool, trace_enabled: bool) {
    assert!(!trace_enabled, "MD instruction tracing is not supported");
    let region = match std::env::var("ZEFF_PROFILE_MD_REGION").as_deref() {
        Ok("ntsc") => Region::NtscUs,
        Ok("japan") => Region::NtscJapan,
        Ok("pal") => Region::Pal,
        _ => panic!("ZEFF_PROFILE_MD_REGION must explicitly be ntsc, japan or pal"),
    };
    let controller = match std::env::var("ZEFF_PROFILE_MD_CONTROLLER").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("3") => ControllerKind::ThreeButton,
        Ok("6") => ControllerKind::SixButton,
        _ => panic!("ZEFF_PROFILE_MD_CONTROLLER must be 3 or 6"),
    };
    let path = std::env::var_os("ZEFF_PROFILE_MD_ROM")
        .expect("ZEFF_PROFILE_MD_ROM must name an unpacked raw MD ROM");
    let rom = read_bounded(&path, MAX_CARTRIDGE_SIZE);
    let rom_hash = Sha256::digest(&rom);
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .expect("load raw MD profile ROM");
    for port in [ControllerPort::One, ControllerPort::Two] {
        machine.input_mut().set_controller_kind(port, controller);
    }
    let initial_state_hash = std::env::var_os("ZEFF_PROFILE_MD_STATE").map(|path| {
        let state = read_bounded(&path, MAX_STATE_BYTES);
        machine.decode_state(&state).expect("load MD profile state");
        Sha256::digest(&state)
    });
    machine.set_apu_sample_generation_enabled(audio_enabled);
    let samples_per_field =
        (u64::from(machine.sample_rate()) * 2 * region.frame_ticks()).div_ceil(region.master_hz());
    let mut audio = Vec::with_capacity(samples_per_field as usize + 2);
    machine.drain_audio_samples_into(&mut audio);
    audio.clear();
    let warmup = std::env::var("ZEFF_PROFILE_WARMUP_FRAMES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(10);
    for _ in 0..warmup {
        step_field(&mut machine);
        machine.drain_audio_samples_into(&mut audio);
        audio.clear();
    }

    let start_ticks = machine.timing_snapshot().now().get();
    let mut hasher = Sha256::new();
    let mut sample_count = 0_u64;
    let mut nonzero_samples = 0_u64;
    let mut peak_abs = 0_f32;
    let mut elapsed = Duration::ZERO;
    super::reset_allocation_counts();
    for _ in 0..frames {
        let start = Instant::now();
        step_field(&mut machine);
        machine.drain_audio_samples_into(&mut audio);
        elapsed += start.elapsed();
        sample_count += audio.len() as u64;
        for &sample in &audio {
            assert!(sample.is_finite(), "non-finite MD profile audio");
            hasher.update(sample.to_le_bytes());
            nonzero_samples += u64::from(sample != 0.0);
            peak_abs = peak_abs.max(sample.abs());
        }
        audio.clear();
    }
    let (allocations, reallocations, allocated_bytes) = super::allocation_counts();
    let master_ticks = machine.timing_snapshot().now().get() - start_ticks;
    let fps = f64::from(frames) / elapsed.as_secs_f64();
    let million_ticks_per_second = master_ticks as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    let target_fps = region.master_hz() as f64 / region.frame_ticks() as f64;
    println!(
        "MD {region:?} audio={audio_enabled} controller={controller:?}: {frames} fields {elapsed:.2?} {fps:.2} fps target {target_fps:.3} {million_ticks_per_second:.2} M master ticks / s"
    );
    println!(
        "  {allocations} alloc {reallocations} realloc {:.1} KiB; warmup {warmup} fields",
        allocated_bytes as f64 / 1024.0
    );
    println!("  ROM {}", super::hash_string(&rom_hash));
    if let Some(hash) = initial_state_hash {
        println!("  loaded state {}", super::hash_string(&hash));
    }
    let state = machine.encode_state().expect("encode MD profile state");
    println!(
        "  framebuffer {}  state {}  audio {}",
        super::hash_string(&Sha256::digest(machine.framebuffer())),
        super::hash_string(&Sha256::digest(&state)),
        super::hash_string(&hasher.finalize()),
    );
    println!(
        "  PCM {sample_count} samples {nonzero_samples} nonzero peak {peak_abs}; dimensions {:?} ticks {master_ticks}",
        machine.framebuffer_dimensions()
    );
}
