use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub(super) struct ProfileAudio {
    scratch: Vec<f32>,
    hasher: Sha256,
}

impl ProfileAudio {
    fn new() -> Self {
        Self {
            scratch: Vec::with_capacity(
                zeff_pce_core::hardware::MAX_PSG_SAMPLE_RATE as usize * 2 / 50 + 2,
            ),
            hasher: Sha256::default(),
        }
    }

    fn drain(&mut self, machine: &mut zeff_pce_core::hardware::PceMachine) {
        machine.drain_audio_samples_into(&mut self.scratch);
    }

    fn hash_and_clear(&mut self) {
        for sample in &self.scratch {
            self.hasher.update(sample.to_le_bytes());
        }
        self.scratch.clear();
    }

    fn finalize(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}

pub(super) fn profile_frames(
    label: &str,
    frames: u32,
    machine: &mut zeff_pce_core::hardware::PceMachine,
    sample_generation_enabled: bool,
) -> Option<ProfileAudio> {
    let mut audio = sample_generation_enabled.then(ProfileAudio::new);
    let warmup_frames = std::env::var("ZEFF_PROFILE_WARMUP_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    for _ in 0..warmup_frames {
        machine.run_until_frame().expect("synthetic PCE frame");
        if let Some(audio) = &mut audio {
            audio.drain(machine);
            audio.hash_and_clear();
        }
    }

    machine.reset_profiling();
    super::reset_allocation_counts();
    let mut elapsed = Duration::ZERO;
    let mut master_ticks = 0_u64;
    for _ in 0..frames {
        let start = Instant::now();
        master_ticks += machine
            .run_until_frame()
            .expect("synthetic PCE frame")
            .master_ticks();
        if let Some(audio) = &mut audio {
            audio.drain(machine);
        }
        elapsed += start.elapsed();
        if let Some(audio) = &mut audio {
            audio.hash_and_clear();
        }
    }
    let (allocations, reallocations, allocated_bytes) = super::allocation_counts();
    let fps = f64::from(frames) / elapsed.as_secs_f64();
    let million_ticks_per_second = master_ticks as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!(
        "{label:30} {frames:5} frames  {elapsed:>9.2?}  {fps:>8.0} fps  {million_ticks_per_second:>8.2} M master ticks / s"
    );
    println!(
        "{:30} {:9} alloc  {:7} realloc  {:9.1} KiB",
        "",
        allocations,
        reallocations,
        allocated_bytes as f64 / 1024.0
    );
    let snapshot = machine.profiling_snapshot();
    println!(
        "  PCE work: CPU {} (instruction {} interrupt {})  bus R/W/D/I {}/{}/{}/{}  device {}/{} chunks",
        snapshot.cpu_boundaries,
        snapshot.cpu_instruction_boundaries,
        snapshot.cpu_interrupt_boundaries,
        snapshot.bus_reads,
        snapshot.bus_writes,
        snapshot.bus_dummy_reads + snapshot.bus_dummy_writes,
        snapshot.bus_idle_cycles,
        snapshot.device_advance_calls,
        snapshot.device_advance_chunks,
    );
    println!(
        "  PCE video: VDC {} calls {} pixels {} phases DMA {}/{} active  raster {} lines {} pixels  PSG {} clocks {} mixes source checks/changes {}/{}",
        snapshot.vdc_advance_calls,
        snapshot.vdc_pixel_clocks,
        snapshot.vdc_phase_transitions,
        snapshot.vdc_dma_active_slots,
        snapshot.vdc_dma_slots,
        snapshot.raster_active_lines,
        snapshot.raster_pixels,
        snapshot.psg_internal_clocks,
        snapshot.psg_mix_scans,
        snapshot.psg_mixer_source_examinations,
        snapshot.psg_mixer_source_transitions,
    );
    audio
}

pub(super) fn print_accuracy_hashes(
    machine: &mut zeff_pce_core::hardware::PceMachine,
    audio: Option<ProfileAudio>,
) {
    let framebuffer_hash = Sha256::digest(machine.framebuffer());
    let state = zeff_pce_core::hardware::save_state::encode_state(machine)
        .expect("encode synthetic PCE state");
    let state_hash = Sha256::digest(&state);
    if let Some(audio) = audio {
        println!(
            "  framebuffer {}  state {}  audio {}",
            super::hash_string(&framebuffer_hash),
            super::hash_string(&state_hash),
            super::hash_string(&audio.finalize()),
        );
    } else {
        let mut audio = Vec::new();
        machine.drain_audio_samples_into(&mut audio);
        super::print_accuracy_hashes(machine.framebuffer(), &state, &audio);
    }
}
