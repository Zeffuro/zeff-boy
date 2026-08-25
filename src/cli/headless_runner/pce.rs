use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use zeff_emu_common::time::FrameLifecycle;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use crate::cli::types::HeadlessOptions;
use crate::emu_backend::loader::{PreparedNativeArchiveBackend, prepare_native_archive_backend};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, EmuBackend, PceBackend};
use crate::emu_core_trait::{DebuggableEmulator, EmulatorCore};

use super::{
    AudioStats, PceDebugStateRequest, StuckTracker, emit_debug_state, ensure_no_reset_events,
    ensure_system_headless_options, fail_on_stuck_if_needed, input_for_frame, input_p2_for_frame,
    input_p3_for_frame, input_p4_for_frame, input_p5_for_frame, observe_stuck, pce_debug_state,
    print_memory_region_dumps, print_perf, read_headless_state_if_requested,
    screenshot_path_if_written, write_audio_dump_f32le, write_final_screenshot_if_needed,
    write_screenshot_if_requested, write_screenshot_sequence_if_requested,
};

const PCE_HEADLESS_SAMPLE_RATE: u32 = 44_100;
const PCE_FRAMEBUFFER_DIMENSIONS: (usize, usize) = (
    crate::emu_backend::pce::PCE_PRESENTED_WIDTH,
    crate::emu_backend::pce::PCE_PRESENTED_HEIGHT,
);
const PCE_TEST_STATUS_ADDRESS: u16 = 0x2000;
const PCE_TEST_STATUS_MAGIC: [u8; 4] = *b"ZPCE";

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
        pce_load_config(
            mode_preference,
            firmware_search_dirs,
            !opts.no_sram,
            opts.apply_mods,
            opts.pce_arcade_card_mode
                .unwrap_or(zeff_pce_core::hardware::PceArcadeCardMode::Automatic),
        ),
    )?;
    run_loaded_pce_headless(loaded.backend, opts)
}

pub(super) fn run_pce_archive_headless(
    source_path: &Path,
    mode_preference: HardwareModePreference,
    firmware_search_dirs: Vec<PathBuf>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default());
    let prepared = prepare_native_archive_backend(
        source_path,
        None,
        None,
        &pce_load_config(
            mode_preference,
            firmware_search_dirs,
            !opts.no_sram,
            opts.apply_mods,
            opts.pce_arcade_card_mode
                .unwrap_or(zeff_pce_core::hardware::PceArcadeCardMode::Automatic),
        ),
        &cancel,
        &progress,
    )?;
    match prepared {
        PreparedNativeArchiveBackend::Ready {
            system: ActiveSystem::Pce,
            loaded,
            ..
        } => run_loaded_pce_headless(loaded.backend, opts),
        PreparedNativeArchiveBackend::Ready { system, .. } => anyhow::bail!(
            "headless archive loading currently supports PC Engine content; archive contains {}",
            system.code()
        ),
        PreparedNativeArchiveBackend::Selection(entries) => anyhow::bail!(
            "headless archive loading requires a single ROM or PC Engine CD set; archive contains {} selectable ROMs",
            entries.len()
        ),
    }
}

fn pce_load_config(
    mode_preference: HardwareModePreference,
    firmware_search_dirs: Vec<PathBuf>,
    load_battery_bram: bool,
    apply_mods: bool,
    arcade_card_mode: zeff_pce_core::hardware::PceArcadeCardMode,
) -> BackendLoadConfig {
    BackendLoadConfig {
        gb_hardware_mode_preference: mode_preference,
        sample_rate: Some(PCE_HEADLESS_SAMPLE_RATE),
        firmware_search_dirs,
        pce_load_battery_bram: load_battery_bram,
        apply_mods,
        pce_arcade_card_mode: arcade_card_mode,
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
    backend.set_pce_memory_base_mode(
        opts.pce_memory_base_mode
            .unwrap_or(zeff_pce_core::hardware::PceMemoryBaseMode::Automatic),
    );
    if opts.no_apu {
        backend.set_apu_sample_generation_enabled(false);
        log::info!("APU sample generation disabled for profiling");
    }
    if let Some(bytes) = read_headless_state_if_requested(opts)? {
        backend.load_state_from_bytes(bytes)?;
        log::info!(
            "Loaded save state from {}",
            opts.load_state_path.as_ref().unwrap().display()
        );
    }
    if let Some(addr) = opts.break_at {
        backend.add_breakpoint(u32::from(addr));
    }
    let trace_capacity = usize::try_from(opts.trace_opcode_limit).unwrap_or(usize::MAX);
    if opts.trace_opcodes && opts.trace_start_t == 0 {
        backend.set_instruction_trace_capacity(trace_capacity);
        backend.set_instruction_trace_enabled(true);
    }

    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let mut screenshot_written = false;
    let mut current_input = Default::default();
    let mut current_input_p2 = Default::default();
    let mut current_input_p3 = Default::default();
    let mut current_input_p4 = Default::default();
    let mut current_input_p5 = Default::default();
    let mut frames_run = 0u64;
    let mut audio_scratch = Vec::new();
    let mut audio_dump = Vec::new();
    let mut audio_stats = AudioStats::default();
    let mut last_cd_state = None;
    let mut test_pass_seen = false;
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
        if opts.trace_opcodes
            && !backend.instruction_trace().is_enabled()
            && backend.debug_cpu_snapshot().master_ticks() >= opts.trace_start_t
        {
            backend.set_instruction_trace_capacity(trace_capacity);
            backend.set_instruction_trace_enabled(true);
        }
        current_input = input_for_frame(opts, frame_number);
        current_input_p2 = input_p2_for_frame(opts, frame_number);
        current_input_p3 = input_p3_for_frame(opts, frame_number);
        current_input_p4 = input_p4_for_frame(opts, frame_number);
        current_input_p5 = input_p5_for_frame(opts, frame_number);
        backend.set_input(current_input.buttons, current_input.dpad);
        backend.set_input_p2(current_input_p2.buttons, current_input_p2.dpad);
        backend.set_input_p3(current_input_p3.buttons, current_input_p3.dpad);
        backend.set_input_p4(current_input_p4.buttons, current_input_p4.dpad);
        backend.set_input_p5(current_input_p5.buttons, current_input_p5.dpad);
        if opts.break_at.is_some() {
            backend.step_frame();
        } else {
            backend.step_frame_bounded()?;
        }
        frames_run = frame_number;

        if !opts.no_apu {
            backend.drain_audio_samples_into(&mut audio_scratch);
            audio_stats.observe(&audio_scratch);
            if opts.audio_dump_path.is_some() {
                audio_dump.extend_from_slice(&audio_scratch);
            }
            audio_scratch.clear();
        }

        if opts.expect_test_pass
            && let Some(status) = pce_test_status(&backend)
        {
            match status.code {
                0 => {}
                1 => {
                    println!("[headless] pce-test {}", status.summary());
                    test_pass_seen = true;
                }
                code => anyhow::bail!(
                    "PC Engine test fixture failed at frame {frames_run}: code={code:02X} {}",
                    status.summary()
                ),
            }
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
            if let Some(detail) = backend.take_runtime_fault() {
                anyhow::bail!("PC Engine core fault at frame {frames_run}: {detail}");
            }
            println!(
                "[headless] pce-break frame={frames_run} pc={:04X}",
                backend.debug_cpu_snapshot().registers().pc
            );
            break;
        }
        if test_pass_seen {
            break;
        }
    }

    let snapshot = backend.debug_cpu_snapshot();
    print_pce_instruction_trace(&backend, opts);
    println!(
        "[headless] system=pce frames={} master_ticks={} pc={:04X} controller={:?} mb128={:?} arcade_card={:?} cdrom2={} audio_samples={} audio_nonzero={} audio_peak={:.6}",
        frames_run,
        snapshot.master_ticks(),
        snapshot.registers().pc,
        backend.controller_mode(),
        backend.memory_base_mode(),
        backend.arcade_card_mode(),
        u8::from(backend.cdrom2().is_some()),
        audio_stats.sample_count,
        audio_stats.nonzero_samples,
        audio_stats.peak_abs,
    );
    print_perf("pce", frames_run, start);
    print_pce_memory_dumps(&backend, opts);
    print_memory_region_dumps(backend.as_mut(), opts)?;
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
            input_p3: current_input_p3,
            input_p4: current_input_p4,
            input_p5: current_input_p5,
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
    if opts.expect_test_pass && !test_pass_seen {
        let detail = pce_test_status(&backend).map_or_else(
            || "no ZPCE work-RAM status record was observed".to_owned(),
            |status| status.summary(),
        );
        anyhow::bail!("expected PC Engine test fixture pass before max frame limit; {detail}");
    }
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
    if !opts.trace_bus_filters.is_empty() {
        anyhow::bail!(
            "PC Engine headless bus-read tracing is not available; use --trace-opcodes for instruction writes"
        );
    }
    Ok(())
}

fn print_pce_instruction_trace(backend: &PceBackend, opts: &HeadlessOptions) {
    if !opts.trace_opcodes {
        return;
    }
    let entries = backend
        .instruction_trace()
        .iter()
        .filter(|entry| {
            opts.trace_pc_range
                .is_none_or(|(start, end)| (start..=end).contains(&u64::from(entry.pc)))
                && (opts.trace_opcode_filter.is_empty()
                    || entry
                        .instruction_bytes()
                        .first()
                        .is_some_and(|opcode| opts.trace_opcode_filter.contains(opcode)))
                && (!opts.trace_watch_interrupts || entry.event.is_some())
        })
        .collect::<Vec<_>>();
    println!(
        "[pce-op-tail] ---- last {} matching ops ----",
        entries.len()
    );
    for entry in entries {
        let bytes = entry
            .instruction_bytes()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let writes = entry
            .writes()
            .iter()
            .map(|write| {
                format!(
                    " {:04X}:{:02X}->{:02X}",
                    write.address, write.old_value, write.new_value
                )
            })
            .collect::<String>();
        let bank = entry
            .bank
            .map_or_else(|| "--".to_owned(), |bank| format!("{bank:02X}"));
        if let Some(event) = entry.event {
            println!(
                "[pce-op] seq={} f={} t={} bank={} pc={:04X} event={event:?}{writes}",
                entry.sequence, entry.frame, entry.cycle, bank, entry.pc
            );
        } else {
            println!(
                "[pce-op] seq={} f={} t={} bank={} pc={:04X} bytes={bytes}{writes}",
                entry.sequence, entry.frame, entry.cycle, bank, entry.pc
            );
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PceTestStatus {
    code: u8,
    events: u8,
    counter: u32,
    half_counter: u32,
    end_counter: u32,
}

impl PceTestStatus {
    fn summary(self) -> String {
        format!(
            "status={:02X} events={:02X} counter={:06X} half={:06X} end={:06X}",
            self.code, self.events, self.counter, self.half_counter, self.end_counter
        )
    }
}

fn pce_test_status(backend: &PceBackend) -> Option<PceTestStatus> {
    parse_pce_test_status(std::array::from_fn(|index| {
        backend.debug_peek8(u32::from(PCE_TEST_STATUS_ADDRESS) + index as u32)
    }))
}

fn parse_pce_test_status(bytes: [u8; 15]) -> Option<PceTestStatus> {
    if bytes[..4] != PCE_TEST_STATUS_MAGIC {
        return None;
    }
    let u24 = |offset: usize| {
        u32::from(bytes[offset])
            | (u32::from(bytes[offset + 1]) << 8)
            | (u32::from(bytes[offset + 2]) << 16)
    };
    Some(PceTestStatus {
        code: bytes[4],
        events: bytes[5],
        counter: u24(6),
        half_counter: u24(9),
        end_counter: u24(12),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pce_test_status_requires_magic_and_decodes_little_endian_u24_counters() {
        assert_eq!(parse_pce_test_status([0; 15]), None);

        let status = parse_pce_test_status([
            b'Z', b'P', b'C', b'E', 1, 3, 0x56, 0x34, 0x12, 0xEF, 0xCD, 0xAB, 0x03, 0x02, 0x01,
        ])
        .unwrap();
        assert_eq!(status.code, 1);
        assert_eq!(status.events, 3);
        assert_eq!(status.counter, 0x12_34_56);
        assert_eq!(status.half_counter, 0xAB_CD_EF);
        assert_eq!(status.end_counter, 0x01_02_03);
    }
}
