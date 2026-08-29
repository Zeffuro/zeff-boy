use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Instant;

use zeff_coleco_core::constants::{DEFAULT_SAMPLE_RATE, SCREEN_HEIGHT, SCREEN_WIDTH};
use zeff_coleco_core::{Emulator, KeypadKey, StandardController};

use crate::cli::types::HeadlessOptions;
use crate::emu_backend::firmware::resolve_coleco_bios_with_manifest;

use self::trace::step_coleco_frame_with_trace;
use super::{
    AudioStats, InputMasks, check_tas_assertions, emit_debug_state, ensure_no_reset_events,
    ensure_system_headless_options, ensure_tas_completed, input_for_frame, input_p2_for_frame,
    print_perf, read_headless_state_if_requested, write_audio_dump_f32le,
    write_coleco_state_artifact, write_final_screenshot_if_needed, write_screenshot_if_requested,
    write_screenshot_sequence_if_requested,
};

mod trace;

const COLECO_FRAMEBUFFER_DIMENSIONS: (usize, usize) = (SCREEN_WIDTH, SCREEN_HEIGHT);
const COLECO_BIOS_PSG_WRITE_COUNT: u64 = 4;
const COLECO_BIOS_STARTUP_NONZERO_SAMPLES: u64 = 6;

pub(super) fn run_coleco_headless(
    rom_path: &Path,
    rom_data: &[u8],
    firmware_search_dirs: &[PathBuf],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options("coleco", opts)?;
    ensure_no_reset_events("ColecoVision", opts)?;
    ensure_coleco_headless_options(opts)?;

    let bios = resolve_coleco_bios_with_manifest(None, firmware_search_dirs, Some(rom_path))?;
    let mut emulator = Emulator::new(rom_data, &bios.bytes, DEFAULT_SAMPLE_RATE)?;
    if let Some(bytes) = read_headless_state_if_requested(opts)? {
        emulator.load_state(&bytes)?;
        log::info!(
            "Loaded save state from {}",
            opts.load_state_path.as_ref().unwrap().display()
        );
    }
    if opts.no_apu {
        emulator.set_audio_generation_enabled(false);
        log::info!("PSG sample generation disabled for profiling");
    }

    let mut screenshot_written = false;
    let mut frames_run = 0u64;
    let mut traced = 0u64;
    let mut bus_traced = 0u64;
    let mut tail = VecDeque::with_capacity(64);
    let mut audio_scratch = Vec::new();
    let mut audio_dump = Vec::new();
    let mut audio_stats = AudioStats::default();
    let start = Instant::now();

    emulator.set_opcode_log_enabled(
        opts.trace_opcodes || opts.print_debug_state || opts.debug_state_path.is_some(),
    );

    write_screenshot_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        COLECO_FRAMEBUFFER_DIMENSIONS,
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        COLECO_FRAMEBUFFER_DIMENSIONS,
    )?;

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        let current_input = input_for_frame(opts, frame_number);
        let current_input_p2 = input_p2_for_frame(opts, frame_number);
        emulator.set_controller(0, standard_controller(current_input));
        emulator.set_controller(1, standard_controller(current_input_p2));
        if opts.trace_opcodes || !opts.trace_bus_filters.is_empty() {
            step_coleco_frame_with_trace(
                opts,
                &mut emulator,
                &mut traced,
                &mut bus_traced,
                &mut tail,
            );
        } else {
            emulator.step_frame();
        }
        frames_run = frame_number;
        check_tas_assertions(
            opts,
            frames_run,
            u32::from(emulator.cpu().regs().pc),
            emulator.framebuffer(),
            || emulator.save_state(),
        )?;

        if !opts.no_apu {
            emulator.drain_audio_samples_into(&mut audio_scratch);
            audio_stats.observe(&audio_scratch);
            if opts.audio_dump_path.is_some() {
                audio_dump.extend_from_slice(&audio_scratch);
            }
            audio_scratch.clear();
        }

        write_screenshot_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            COLECO_FRAMEBUFFER_DIMENSIONS,
            &mut screenshot_written,
        )?;
        write_screenshot_sequence_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            COLECO_FRAMEBUFFER_DIMENSIONS,
        )?;
    }
    ensure_tas_completed(opts, frames_run)?;

    if opts.trace_opcodes {
        println!("[coleco-op-tail] ---- last {} ops ----", tail.len());
        for line in tail {
            println!("{line}");
        }
    }

    println!(
        "[headless] system=coleco rom={} frames={} cycles={} pc={:04X} status=ok",
        rom_path.display(),
        frames_run,
        emulator.effective_cycles(),
        emulator.cpu().regs().pc,
    );
    print_perf("coleco", frames_run, start);
    write_final_screenshot_if_needed(
        opts,
        frames_run,
        emulator.framebuffer(),
        COLECO_FRAMEBUFFER_DIMENSIONS,
        &mut screenshot_written,
    )?;
    emit_debug_state(
        opts,
        serde_json::json!({
            "system": "coleco",
            "frames": frames_run,
            "cycles": emulator.effective_cycles(),
            "cpu_cycles": emulator.cpu_cycles(),
            "pc": format!("{:04X}", emulator.cpu().regs().pc),
            "psg": {
                "writes": emulator.bus().psg().write_count(),
                "last_write": emulator.bus().psg().last_write(),
                "tone_periods": emulator.bus().psg().tone_periods(),
                "volumes": emulator.bus().psg().volumes(),
                "noise_control": emulator.bus().psg().noise_control(),
            },
            "audio": {
                "sample_rate": emulator.sample_rate(),
                "samples": audio_stats.sample_count,
                "nonzero_samples": audio_stats.nonzero_samples,
                "peak_abs": audio_stats.peak_abs,
            },
        }),
    )?;
    if let Some(path) = &opts.audio_dump_path {
        write_audio_dump_f32le(path, &audio_dump, emulator.sample_rate())?;
    }
    if opts.expect_coleco_audio {
        ensure_coleco_audio_output(audio_stats, emulator.bus().psg().write_count())?;
    }
    if let Some(path) = &opts.coleco_save_state_path {
        let state = emulator.save_state()?;
        let saved_path = write_coleco_state_artifact(path, &state)?;
        println!(
            "[headless] coleco-save-state={} bytes={} sha256={}",
            saved_path.display(),
            state.len(),
            zeff_firmware::sha256_hex(&state),
        );
    }
    Ok(())
}

fn ensure_coleco_audio_output(audio_stats: AudioStats, psg_writes: u64) -> anyhow::Result<()> {
    if audio_stats.nonzero_samples <= COLECO_BIOS_STARTUP_NONZERO_SAMPLES
        || psg_writes <= COLECO_BIOS_PSG_WRITE_COUNT
    {
        anyhow::bail!(
            "expected ColecoVision audio output, but no programmed nonzero audio was observed"
        );
    }
    println!(
        "[headless] coleco-audio-check nonzero_samples={} peak_abs={:.6} mean_abs={:.6}",
        audio_stats.nonzero_samples,
        audio_stats.peak_abs,
        audio_stats.mean_abs()
    );
    Ok(())
}

fn standard_controller(input: InputMasks) -> StandardController {
    StandardController {
        up: input.dpad & (1 << 2) != 0,
        right: input.dpad & (1 << 0) != 0,
        down: input.dpad & (1 << 3) != 0,
        left: input.dpad & (1 << 1) != 0,
        left_button: input.buttons & (1 << 0) != 0,
        right_button: input.buttons & (1 << 1) != 0,
        keypad: keypad_from_input(input),
    }
}

fn keypad_from_input(input: InputMasks) -> Option<KeypadKey> {
    match input.coleco_keypad {
        Some(0) => Some(KeypadKey::Zero),
        Some(1) => Some(KeypadKey::One),
        Some(2) => Some(KeypadKey::Two),
        Some(3) => Some(KeypadKey::Three),
        Some(4) => Some(KeypadKey::Four),
        Some(5) => Some(KeypadKey::Five),
        Some(6) => Some(KeypadKey::Six),
        Some(7) => Some(KeypadKey::Seven),
        Some(8) => Some(KeypadKey::Eight),
        Some(9) => Some(KeypadKey::Nine),
        Some(10) => Some(KeypadKey::Star),
        Some(11) => Some(KeypadKey::Pound),
        Some(_) => None,
        None if input.buttons & (1 << 3) != 0 => Some(KeypadKey::Two),
        None if input.buttons & (1 << 2) != 0 => Some(KeypadKey::One),
        None => None,
    }
}

fn ensure_coleco_headless_options(opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.trace_watch_interrupts {
        anyhow::bail!("--trace-watch-interrupts is not supported for ColecoVision headless runs");
    }
    if opts.expect_test_pass {
        anyhow::bail!("--expect-test-pass is not implemented for ColecoVision headless runs yet");
    }
    if opts.expect_coleco_audio && opts.no_apu {
        anyhow::bail!("--expect-coleco-audio cannot be used together with --no-apu");
    }
    if !opts.zapper_events.is_empty() {
        anyhow::bail!("--zapper is not supported for ColecoVision headless runs");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_coleco_keypad_is_available_to_headless_input() {
        let keys = [
            KeypadKey::Zero,
            KeypadKey::One,
            KeypadKey::Two,
            KeypadKey::Three,
            KeypadKey::Four,
            KeypadKey::Five,
            KeypadKey::Six,
            KeypadKey::Seven,
            KeypadKey::Eight,
            KeypadKey::Nine,
            KeypadKey::Star,
            KeypadKey::Pound,
        ];

        for (tag, key) in keys.into_iter().enumerate() {
            assert_eq!(
                keypad_from_input(InputMasks {
                    coleco_keypad: Some(tag as u8),
                    ..InputMasks::default()
                }),
                Some(key)
            );
        }
    }

    #[test]
    fn coleco_audio_assertion_requires_a_nonzero_sample() {
        assert!(ensure_coleco_audio_output(AudioStats::default(), 0).is_err());
        assert!(
            ensure_coleco_audio_output(
                AudioStats {
                    sample_count: COLECO_BIOS_STARTUP_NONZERO_SAMPLES,
                    frames_with_samples: 1,
                    nonzero_samples: COLECO_BIOS_STARTUP_NONZERO_SAMPLES,
                    peak_abs: 0.5,
                    sum_abs: 3.0,
                },
                COLECO_BIOS_PSG_WRITE_COUNT,
            )
            .is_err()
        );
        assert!(
            ensure_coleco_audio_output(
                AudioStats {
                    sample_count: COLECO_BIOS_STARTUP_NONZERO_SAMPLES + 1,
                    frames_with_samples: 1,
                    nonzero_samples: COLECO_BIOS_STARTUP_NONZERO_SAMPLES + 1,
                    peak_abs: 0.25,
                    sum_abs: 0.25,
                },
                COLECO_BIOS_PSG_WRITE_COUNT + 1,
            )
            .is_ok()
        );
    }
}
