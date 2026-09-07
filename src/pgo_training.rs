//! Explicit owned-firmware workload for native release profile generation.
//! Normal cartridge loading and retail firmware recognition remain separate.

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

pub fn run_if_requested() -> anyhow::Result<bool> {
    let Some(mode) = std::env::var_os("ZEFF_PGO_TRAINING") else {
        return Ok(false);
    };
    if mode != "coleco" {
        bail!("unknown ZEFF_PGO_TRAINING mode; expected coleco");
    }
    let frames = std::env::var("ZEFF_PGO_FRAMES")
        .context("ZEFF_PGO_FRAMES is required for internal training")?
        .parse::<u32>()
        .context("ZEFF_PGO_FRAMES must be an integer")?;
    if !(1..=10_000).contains(&frames) {
        bail!("ZEFF_PGO_FRAMES must be between 1 and 10000");
    }
    let (rom, bios) = zeff_pgo_corpus::coleco_fixture();
    let mut machine = zeff_coleco_core::Emulator::new(&rom, &bios, 48_000)?;
    machine.set_audio_generation_enabled(true);
    let mut audio = Vec::new();
    let mut audio_hash = Sha256::new();
    let mut video_hash = Sha256::new();
    let mut sample_count = 0_u64;
    for _ in 0..frames {
        machine.step_frame();
        if let Some(trap) = machine.cpu_trap() {
            bail!("synthetic Coleco training trapped: {trap:?}");
        }
        video_hash.update(machine.framebuffer());
        audio.clear();
        machine.drain_audio_samples_into(&mut audio);
        sample_count += audio.len() as u64;
        for sample in &audio {
            audio_hash.update(sample.to_bits().to_le_bytes());
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "schema": 1,
            "training": "coleco",
            "frames": machine.frame_count(),
            "audio_samples": sample_count,
            "video_sha256": const_hex::encode(video_hash.finalize()),
            "audio_sha256": const_hex::encode(audio_hash.finalize()),
            "state_sha256": zeff_pgo_corpus::sha256_hex(&machine.save_state()?),
        })
    );
    Ok(true)
}
