use std::path::Path;
use std::{collections::VecDeque, time::Instant};

use zeff_sega8_core::emulator::Emulator as Sega8Emulator;

use crate::cli::types::HeadlessOptions;
use crate::emu_backend::ActiveSystem;

use self::sdsc::Sega8SdscCapture;
use self::trace::{Sega8FrameTraceConfig, Sega8FrameTraceState, step_sega8_frame_with_trace};
use super::{
    AudioStats, Sega8DebugStateRequest, StuckTracker, emit_debug_state, ensure_no_reset_events,
    ensure_system_headless_options, fail_on_stuck_if_needed, flush_battery, input_for_frame,
    input_p2_for_frame, observe_stuck, print_perf, read_headless_state_if_requested,
    screenshot_path_if_written, sega8_debug_state, write_audio_dump_f32le,
    write_final_screenshot_if_needed, write_screenshot_if_requested,
    write_screenshot_sequence_if_requested,
};

mod sdsc;
mod trace;

pub(super) fn run_sega8_headless(
    rom_path: &Path,
    rom_data: &[u8],
    system: ActiveSystem,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options(system.code(), opts)?;
    ensure_no_reset_events(system.code(), opts)?;
    ensure_sega8_headless_options(opts)?;

    let hint = crate::emu_backend::sega8::hint_for_active_system(system)
        .expect("Sega 8-bit systems must have a core hint");
    let video_standard = opts
        .sega8_video_standard
        .or_else(|| crate::emu_backend::sega8::video_standard_from_paths(rom_path, rom_path))
        .unwrap_or_default();
    let console_region_fallback =
        crate::emu_backend::sega8::console_region_from_paths(rom_path, rom_path);
    let mut emulator = Sega8Emulator::new_with_hint_video_standard_region_fallback(
        rom_data,
        zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE,
        hint,
        video_standard,
        opts.sega8_console_region,
        console_region_fallback,
    )?;
    if !opts.no_sram
        && let Some(sram_path) =
            crate::emu_backend::sega8::try_load_battery_sram(&mut emulator, rom_path)
                .unwrap_or_else(|e| {
                    log::warn!("Failed to load battery save: {e}");
                    None
                })
    {
        log::info!("Loaded battery save from {}", sram_path);
    }
    if let Some(bytes) = read_headless_state_if_requested(opts)? {
        emulator.load_state_from_bytes(bytes)?;
        log::info!(
            "Loaded save state from {}",
            opts.load_state_path.as_ref().unwrap().display()
        );
    }
    let dimensions = emulator.framebuffer_dimensions();
    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let mut screenshot_written = false;
    let mut current_input = Default::default();
    let mut current_input_p2 = Default::default();
    let start = Instant::now();
    let mut frames_run = 0u64;
    let mut traced = 0u64;
    let mut bus_traced = 0u64;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let mut audio_scratch: Vec<f32> = Vec::new();
    let mut audio_dump: Vec<f32> = Vec::new();
    let mut audio_stats = AudioStats::default();
    let mut sdsc_capture = Sega8SdscCapture::default();
    let expected_sdsc_text = opts.expect_sega8_sdsc.as_deref();
    let sdsc_capture_active = expected_sdsc_text.is_some();
    let bus_trace_active = !opts.trace_bus_filters.is_empty() || sdsc_capture_active;

    emulator.set_opcode_log_enabled(
        opts.trace_opcodes || opts.print_debug_state || opts.debug_state_path.is_some(),
    );

    if opts.no_apu {
        emulator.set_apu_sample_generation_enabled(false);
        log::info!("APU sample generation disabled for profiling");
    }

    write_screenshot_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        dimensions,
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(opts, 0, emulator.framebuffer(), dimensions)?;

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        current_input_p2 = input_p2_for_frame(opts, frame_number);
        emulator.set_input(current_input.buttons, current_input.dpad);
        emulator.set_input_p2(current_input_p2.buttons, current_input_p2.dpad);
        let mut sdsc_expected_seen = false;
        if opts.trace_opcodes || bus_trace_active {
            let config = Sega8FrameTraceConfig {
                bus_trace_active,
                sdsc_capture_active,
                expected_sdsc_text,
            };
            let mut state = Sega8FrameTraceState {
                traced: &mut traced,
                bus_traced: &mut bus_traced,
                tail: &mut tail,
                sdsc_capture: &mut sdsc_capture,
            };
            sdsc_expected_seen =
                step_sega8_frame_with_trace(opts, &mut emulator, config, &mut state);
        } else {
            emulator.step_frame();
        }
        if !opts.no_apu {
            emulator.drain_audio_samples_into(&mut audio_scratch);
            audio_stats.observe(&audio_scratch);
            if opts.audio_dump_path.is_some() {
                audio_dump.extend_from_slice(&audio_scratch);
            }
            audio_scratch.clear();
        }
        frames_run = frame_number;

        write_screenshot_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            dimensions,
            &mut screenshot_written,
        )?;
        write_screenshot_sequence_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            dimensions,
        )?;

        let wait_classification = sega8_wait_classification(&emulator);
        observe_stuck(
            &mut stuck,
            system.code(),
            4,
            frames_run,
            u64::from(emulator.cpu().regs().pc),
            emulator.framebuffer(),
            None,
            wait_classification,
            wait_classification.is_some(),
            &mut stuck_active,
        );

        if emulator.is_suspended() {
            anyhow::bail!(
                "Sega 8-bit CPU suspended at frame {} trap={:?}",
                frames_run,
                emulator.cpu_trap()
            );
        }

        if sdsc_expected_seen && !opts.expect_sega8_audio {
            break;
        }
    }

    if opts.trace_opcodes {
        println!("[sega8-op-tail] ---- last {} ops ----", tail.len());
        for line in tail {
            println!("{}", line);
        }
    }

    if let Some(expected) = expected_sdsc_text {
        if !sdsc_capture.text().contains(expected) {
            anyhow::bail!(
                "expected Sega 8-bit SDSC output containing {:?}, got {:?}",
                expected,
                sdsc_capture.preview()
            );
        }
        println!(
            "[headless] sega8-sdsc bytes={} commands={} suspend_seen={} contains={:?}",
            sdsc_capture.text().len(),
            sdsc_capture.command_count,
            u8::from(sdsc_capture.suspend_seen),
            expected
        );
    }

    if opts.expect_sega8_audio {
        if audio_stats.nonzero_samples == 0 {
            anyhow::bail!("expected Sega 8-bit audio output, but no nonzero samples were observed");
        }
        println!(
            "[headless] sega8-audio-check nonzero_samples={} peak_abs={:.6} mean_abs={:.6}",
            audio_stats.nonzero_samples,
            audio_stats.peak_abs,
            audio_stats.mean_abs()
        );
    }

    println!(
        "[headless] system={} rom={} frames={} cycles={} pc={:04X} status=ok",
        system,
        rom_path.display(),
        frames_run,
        emulator.cpu().cycles(),
        emulator.cpu().regs().pc
    );
    print_perf(system.code(), frames_run, start);
    write_final_screenshot_if_needed(
        opts,
        frames_run,
        emulator.framebuffer(),
        dimensions,
        &mut screenshot_written,
    )?;
    emit_debug_state(
        opts,
        sega8_debug_state(Sega8DebugStateRequest {
            emulator: &emulator,
            frames_run,
            opts,
            input: current_input,
            input_p2: current_input_p2,
            stuck: stuck.as_ref().and_then(StuckTracker::current_report),
            screenshot: screenshot_path_if_written(opts, screenshot_written),
            audio_stats,
        }),
    )?;
    if !opts.no_sram {
        flush_battery(rom_path, emulator.dump_battery_sram());
    }
    if let Some(path) = &opts.audio_dump_path {
        write_audio_dump_f32le(path, &audio_dump, emulator.sample_rate())?;
        println!(
            "[headless] sega8-audio drained_samples={} drained_frames={} nonzero_samples={} peak_abs={:.6} mean_abs={:.6}",
            audio_stats.sample_count,
            audio_stats.frames_with_samples,
            audio_stats.nonzero_samples,
            audio_stats.peak_abs,
            audio_stats.mean_abs()
        );
    }
    fail_on_stuck_if_needed(system.code(), stuck.as_ref(), opts)?;
    Ok(())
}

fn ensure_sega8_headless_options(opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.trace_watch_interrupts {
        anyhow::bail!("--trace-watch-interrupts is not supported for Sega 8-bit headless runs yet");
    }
    if opts.expect_test_pass {
        anyhow::bail!("--expect-test-pass is not implemented for Sega 8-bit headless runs yet");
    }
    if opts.expect_sega8_audio && opts.no_apu {
        anyhow::bail!("--expect-sega8-audio cannot be used together with --no-apu");
    }
    if !opts.zapper_events.is_empty() {
        anyhow::bail!("--zapper is not supported for Sega 8-bit headless runs");
    }
    Ok(())
}

fn sega8_wait_classification(emulator: &Sega8Emulator) -> Option<&'static str> {
    if emulator.cpu().is_halted() && emulator.bus().vdp().frame_interrupt_enabled() {
        Some("sega8-halt-waiting-for-vblank")
    } else if sega8_framebuffer_has_visible_content(emulator.framebuffer()) {
        Some("sega8-static-visible-frame")
    } else {
        None
    }
}

fn sega8_framebuffer_has_visible_content(framebuffer: &[u8]) -> bool {
    let mut chunks = framebuffer.chunks_exact(4);
    let Some(first) = chunks.next() else {
        return false;
    };
    chunks.any(|pixel| pixel[..3] != first[..3])
}
