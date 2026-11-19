use super::{HeadlessOptions, ReplayPlayer, endpoint};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use std::path::Path;

pub(in crate::cli::headless_runner) fn run_md_replay_headless(
    source: &Path,
    path: &Path,
    bytes: Option<Vec<u8>>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    validate_md_replay_options(opts)?;
    let player = ReplayPlayer::load(opts.replay_path.as_ref().expect("MD replay path"))?;
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        source,
        path,
        bytes,
        BackendLoadConfig {
            apply_mods: opts.apply_mods,
            md_region: opts.md_region,
            md_timing: opts.md_timing.unwrap_or_default(),
            md_controller: opts
                .md_controller
                .unwrap_or(zeff_md_core::ControllerKind::SixButton),
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )?
    .backend;
    backend.set_apu_sample_generation_enabled(!opts.no_apu);
    let summary = endpoint::run_loaded_replay_headless(backend, player, opts)?;
    println!(
        "[headless] replay frames={} events={} final_state_sha256={} final_framebuffer_sha256={}",
        summary.frames,
        summary.events_applied,
        summary.final_state_hash,
        summary.final_framebuffer_hash
    );
    Ok(())
}

pub(in crate::cli::headless_runner) fn validate_md_replay_options(
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        opts.load_state_path.is_none() && opts.md_save_state_path.is_none(),
        "Mega Drive replay controls its start state; --load-state and --md-save-state-out cannot be combined with --replay"
    );
    anyhow::ensure!(
        opts.input_events.is_empty()
            && opts.input_events_p2.is_empty()
            && opts.input_events_p3.is_empty()
            && opts.input_events_p4.is_empty()
            && opts.input_events_p5.is_empty(),
        "Mega Drive replay controls pad input; --press and --input cannot be combined with --replay"
    );
    anyhow::ensure!(
        opts.audio_trace_path.is_none()
            && opts.audio_dump_path.is_none()
            && opts.screenshot_path.is_none()
            && opts.screenshot_dir.is_none()
            && !opts.print_debug_state
            && opts.debug_state_path.is_none()
            && opts.stuck_window_frames == 0
            && !opts.fail_on_stuck
            && opts.trace_start_t == 0,
        "Mega Drive replay does not support trace, capture, debug-state or stuck-detection options"
    );
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
