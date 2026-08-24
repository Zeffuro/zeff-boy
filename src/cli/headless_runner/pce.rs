use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use crate::cli::types::HeadlessOptions;
use crate::emu_backend::loader::{PreparedSevenZipBackend, prepare_seven_zip_backend};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, EmuBackend, PceBackend};
use crate::emu_core_trait::EmulatorCore;

use super::{
    AudioStats, PceDebugStateRequest, StuckTracker, emit_debug_state, ensure_no_reset_events,
    ensure_system_headless_options, fail_on_stuck_if_needed, input_for_frame, input_p2_for_frame,
    observe_stuck, pce_debug_state, print_perf, screenshot_path_if_written, write_audio_dump_f32le,
    write_final_screenshot_if_needed, write_screenshot_if_requested,
    write_screenshot_sequence_if_requested,
};

const PCE_HEADLESS_SAMPLE_RATE: u32 = 44_100;
const PCE_FRAMEBUFFER_DIMENSIONS: (usize, usize) = (
    crate::emu_backend::pce::PCE_PRESENTED_WIDTH,
    crate::emu_backend::pce::PCE_PRESENTED_HEIGHT,
);

pub(super) fn run_pce_headless(
    source_path: &Path,
    rom_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    mode_preference: HardwareModePreference,
    firmware_search_dirs: Vec<PathBuf>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let loaded = crate::emu_backend::load_backend_from_rom_source(
        ActiveSystem::Pce,
        source_path,
        rom_path,
        preloaded_data,
        pce_load_config(mode_preference, firmware_search_dirs, !opts.no_sram),
    )?;
    run_loaded_pce_headless(loaded.backend, opts)
}

pub(super) fn run_pce_seven_zip_headless(
    source_path: &Path,
    mode_preference: HardwareModePreference,
    firmware_search_dirs: Vec<PathBuf>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let cancel = AtomicBool::new(false);
    let progress = crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default();
    let prepared = prepare_seven_zip_backend(
        source_path,
        None,
        None,
        &pce_load_config(mode_preference, firmware_search_dirs, !opts.no_sram),
        &cancel,
        &progress,
    )?;
    match prepared {
        PreparedSevenZipBackend::Ready {
            system: ActiveSystem::Pce,
            loaded,
            ..
        } => run_loaded_pce_headless(loaded.backend, opts),
        PreparedSevenZipBackend::Ready { system, .. } => anyhow::bail!(
            "headless 7z loading currently supports PC Engine content; archive contains {}",
            system.code()
        ),
        PreparedSevenZipBackend::Selection(entries) => anyhow::bail!(
            "headless 7z loading requires a single ROM or PC Engine CD set; archive contains {} selectable ROMs",
            entries.len()
        ),
    }
}

fn pce_load_config(
    mode_preference: HardwareModePreference,
    firmware_search_dirs: Vec<PathBuf>,
    load_battery_bram: bool,
) -> BackendLoadConfig {
    BackendLoadConfig {
        gb_hardware_mode_preference: mode_preference,
        sample_rate: Some(PCE_HEADLESS_SAMPLE_RATE),
        firmware_search_dirs,
        pce_load_battery_bram: load_battery_bram,
        ..BackendLoadConfig::default()
    }
}

fn run_loaded_pce_headless(backend: EmuBackend, opts: &HeadlessOptions) -> anyhow::Result<()> {
    ensure_system_headless_options("pce", opts)?;
    ensure_no_reset_events("pce", opts)?;
    ensure_pce_headless_options(opts)?;

    let EmuBackend::Pce(mut backend) = backend else {
        anyhow::bail!("PC Engine headless loader returned a different core");
    };
    backend.set_pce_mouse_state(
        opts.pce_controller_mode
            .unwrap_or(zeff_pce_core::hardware::PceControllerMode::Automatic),
        0,
        0,
        0,
    );
    if opts.no_apu {
        backend.set_apu_sample_generation_enabled(false);
        log::info!("APU sample generation disabled for profiling");
    }

    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let mut screenshot_written = false;
    let mut current_input = Default::default();
    let mut current_input_p2 = Default::default();
    let mut frames_run = 0u64;
    let mut audio_scratch = Vec::new();
    let mut audio_dump = Vec::new();
    let mut audio_stats = AudioStats::default();
    let mut last_cd_state = None;
    let start = Instant::now();

    write_screenshot_if_requested(
        opts,
        0,
        backend.framebuffer(),
        PCE_FRAMEBUFFER_DIMENSIONS,
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(
        opts,
        0,
        backend.framebuffer(),
        PCE_FRAMEBUFFER_DIMENSIONS,
    )?;

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        current_input_p2 = input_p2_for_frame(opts, frame_number);
        backend.set_input(current_input.buttons, current_input.dpad);
        backend.set_input_p2(current_input_p2.buttons, current_input_p2.dpad);
        backend.step_frame_bounded()?;
        frames_run = frame_number;

        if !opts.no_apu {
            backend.drain_audio_samples_into(&mut audio_scratch);
            audio_stats.observe(&audio_scratch);
            if opts.audio_dump_path.is_some() {
                audio_dump.extend_from_slice(&audio_scratch);
            }
            audio_scratch.clear();
        }

        write_screenshot_if_requested(
            opts,
            frames_run,
            backend.framebuffer(),
            PCE_FRAMEBUFFER_DIMENSIONS,
            &mut screenshot_written,
        )?;
        write_screenshot_sequence_if_requested(
            opts,
            frames_run,
            backend.framebuffer(),
            PCE_FRAMEBUFFER_DIMENSIONS,
        )?;

        let snapshot = backend.debug_cpu_snapshot();
        let cd_state = backend.cdrom2().map(|cdrom| {
            (
                cdrom.phase(),
                cdrom.audio_status(),
                cdrom.command_trace().len(),
            )
        });
        if cd_state != last_cd_state {
            if let Some((phase, audio, command_count)) = cd_state {
                let last_command = backend
                    .cdrom2()
                    .and_then(|cdrom| cdrom.command_trace().back())
                    .map(|command| {
                        command
                            .bytes()
                            .iter()
                            .map(|byte| format!("{byte:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_else(|| "none".to_owned());
                println!(
                    "[headless] pce-cd frame={frames_run} phase={phase:?} audio={audio:?} commands={command_count} last={last_command}"
                );
            }
            last_cd_state = cd_state;
        }

        let (progress_marker, wait_classification) = pce_wait_state(&backend);
        observe_stuck(
            &mut stuck,
            "pce",
            4,
            frames_run,
            u64::from(snapshot.registers().pc),
            backend.framebuffer(),
            progress_marker,
            wait_classification.as_deref(),
            wait_classification.is_some(),
            &mut stuck_active,
        );

        if backend.is_suspended() {
            let detail = backend
                .take_runtime_fault()
                .unwrap_or_else(|| "unknown core fault".to_owned());
            anyhow::bail!("PC Engine core fault at frame {frames_run}: {detail}");
        }
    }

    let snapshot = backend.debug_cpu_snapshot();
    println!(
        "[headless] system=pce frames={} master_ticks={} pc={:04X} controller={:?} cdrom2={} audio_samples={} audio_nonzero={} audio_peak={:.6}",
        frames_run,
        snapshot.master_ticks(),
        snapshot.registers().pc,
        backend.controller_mode(),
        u8::from(backend.cdrom2().is_some()),
        audio_stats.sample_count,
        audio_stats.nonzero_samples,
        audio_stats.peak_abs,
    );
    print_perf("pce", frames_run, start);
    print_pce_memory_dumps(&backend, opts);
    write_final_screenshot_if_needed(
        opts,
        frames_run,
        backend.framebuffer(),
        PCE_FRAMEBUFFER_DIMENSIONS,
        &mut screenshot_written,
    )?;
    emit_debug_state(
        opts,
        pce_debug_state(PceDebugStateRequest {
            backend: &backend,
            frames_run,
            opts,
            input: current_input,
            input_p2: current_input_p2,
            stuck: stuck.as_ref().and_then(StuckTracker::current_report),
            screenshot: screenshot_path_if_written(opts, screenshot_written),
            audio_samples: audio_stats.sample_count,
            audio_nonzero_samples: audio_stats.nonzero_samples,
            audio_peak_abs: audio_stats.peak_abs,
        }),
    )?;
    if let Some(path) = &opts.audio_dump_path {
        write_audio_dump_f32le(path, &audio_dump, PCE_HEADLESS_SAMPLE_RATE)?;
    }
    fail_on_stuck_if_needed("pce", stuck.as_ref(), opts)?;
    if !opts.no_sram {
        match backend.flush_battery_sram() {
            Ok(Some(path)) => log::info!("Saved battery RAM to {path}"),
            Ok(None) => {}
            Err(err) => log::error!("Failed to save battery RAM: {err}"),
        }
    }
    Ok(())
}

fn pce_wait_state(backend: &PceBackend) -> (Option<u64>, Option<String>) {
    let Some(cdrom) = backend.cdrom2() else {
        return (None, None);
    };
    let progress_marker = Some(cdrom.command_trace().len() as u64);
    let classification = if cdrom.audio_status() == zeff_pce_core::hardware::CdAudioStatus::Playing
    {
        Some("pce-cdda-playing".to_owned())
    } else if cdrom.phase() != zeff_pce_core::hardware::CdScsiPhase::BusFree {
        Some(format!("pce-cd-{:?}", cdrom.phase()).to_ascii_lowercase())
    } else {
        None
    };
    (progress_marker, classification)
}

fn ensure_pce_headless_options(opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.trace_opcodes
        || opts.trace_pc_range.is_some()
        || !opts.trace_opcode_filter.is_empty()
        || opts.trace_watch_interrupts
        || !opts.trace_bus_filters.is_empty()
    {
        anyhow::bail!(
            "PC Engine opcode/bus tracing is not exposed headlessly yet; use --debug-state and CD transition output"
        );
    }
    if opts.expect_test_pass {
        anyhow::bail!("--expect-test-pass is not defined for PC Engine headless runs");
    }
    if opts.load_state_path.is_some() {
        anyhow::bail!("PC Engine save states are not supported");
    }
    Ok(())
}

fn print_pce_memory_dumps(backend: &PceBackend, opts: &HeadlessOptions) {
    for dump in &opts.memory_dumps {
        let start = dump.start_addr;
        let len = dump.len;
        println!("[mem] start={start:04X} len={len}");
        let mut offset = 0u16;
        while offset < len {
            let line_len = (len - offset).min(16);
            let address = start.wrapping_add(offset);
            let bytes = (0..line_len)
                .map(|index| {
                    format!(
                        "{:02X}",
                        backend.debug_peek8(u32::from(address.wrapping_add(index)))
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            println!("[mem] {address:04X}: {bytes}");
            offset += line_len;
        }
    }
}
