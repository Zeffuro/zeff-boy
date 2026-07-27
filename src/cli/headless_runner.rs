use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use zeff_gb_core::emulator::Emulator as GbEmulator;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::constants::CYCLES_PER_FRAME as GBA_CYCLES_PER_FRAME;
use zeff_nes_core::emulator::Emulator as NesEmulator;

use crate::emu_backend::ActiveSystem;

use super::output::{
    TraceContext, format_headless_breakpoint, format_headless_serial, format_headless_summary,
    format_op_line, format_op_tail_line,
};
use super::trace_filters::{ime_short, mode_short, should_trace_op};
use super::types::{HeadlessBusTraceAccess, HeadlessOptions};
use debug_state::*;
use gb::run_gb_headless;
use gba::run_gba_headless;
use nes::run_nes_headless;
use screenshots::*;
use trace::*;

mod debug_state;
mod gb;
mod gba;
mod nes;
mod screenshots;
mod trace;

#[derive(Clone)]
struct StuckReport {
    frame: u64,
    window_frames: usize,
    unique_pcs: usize,
    framebuffer_changed: bool,
    first_pc: u64,
    last_pc: u64,
    classification: Option<String>,
    expected_wait: bool,
}

struct StuckTracker {
    window_frames: usize,
    pc_threshold: usize,
    pcs: VecDeque<u64>,
    framebuffer_hashes: VecDeque<u64>,
    current_report: Option<StuckReport>,
}

#[derive(Clone, Copy, Debug, Default)]
struct AudioStats {
    frames_with_samples: u64,
    sample_count: u64,
    nonzero_samples: u64,
    peak_abs: f32,
    sum_abs: f64,
}

impl AudioStats {
    fn observe(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        self.frames_with_samples = self.frames_with_samples.wrapping_add(1);
        self.sample_count = self.sample_count.wrapping_add(samples.len() as u64);
        for &sample in samples {
            let abs = sample.abs();
            if abs > 0.000_001 {
                self.nonzero_samples = self.nonzero_samples.wrapping_add(1);
            }
            self.peak_abs = self.peak_abs.max(abs);
            self.sum_abs += f64::from(abs);
        }
    }

    fn mean_abs(self) -> f64 {
        if self.sample_count == 0 {
            0.0
        } else {
            self.sum_abs / self.sample_count as f64
        }
    }
}

#[derive(Clone, Copy, Default)]
struct InputMasks {
    buttons: u8,
    dpad: u8,
    reset: bool,
}

impl StuckTracker {
    fn from_options(opts: &HeadlessOptions) -> Option<Self> {
        if opts.stuck_window_frames == 0 {
            return None;
        }

        Some(Self {
            window_frames: opts.stuck_window_frames.min(usize::MAX as u64) as usize,
            pc_threshold: opts.stuck_pc_threshold,
            pcs: VecDeque::new(),
            framebuffer_hashes: VecDeque::new(),
            current_report: None,
        })
    }

    fn observe(
        &mut self,
        frame: u64,
        pc: u64,
        framebuffer: &[u8],
        classification: Option<&str>,
        expected_wait: bool,
    ) -> Option<&StuckReport> {
        self.pcs.push_back(pc);
        self.framebuffer_hashes
            .push_back(framebuffer_fingerprint(framebuffer));

        while self.pcs.len() > self.window_frames {
            self.pcs.pop_front();
        }
        while self.framebuffer_hashes.len() > self.window_frames {
            self.framebuffer_hashes.pop_front();
        }

        if self.pcs.len() < self.window_frames {
            return self.current_report.as_ref();
        }

        let unique_pcs = self.pcs.iter().copied().collect::<HashSet<_>>().len();
        let framebuffer_changed = self
            .framebuffer_hashes
            .iter()
            .copied()
            .collect::<HashSet<_>>()
            .len()
            > 1;

        if unique_pcs <= self.pc_threshold && !framebuffer_changed {
            self.current_report = Some(StuckReport {
                frame,
                window_frames: self.window_frames,
                unique_pcs,
                framebuffer_changed,
                first_pc: self.pcs.front().copied().unwrap_or(pc),
                last_pc: pc,
                classification: classification.map(str::to_owned),
                expected_wait,
            });
        } else {
            self.current_report = None;
        }

        self.current_report.as_ref()
    }

    fn current_report(&self) -> Option<&StuckReport> {
        self.current_report.as_ref()
    }
}

pub(crate) fn run_headless(
    path: &Path,
    mode_preference: HardwareModePreference,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let (rom_path, rom_data, system) = load_headless_rom(path)?;

    match system {
        ActiveSystem::GameBoy => run_gb_headless(&rom_path, &rom_data, mode_preference, opts),
        ActiveSystem::GameBoyAdvance => run_gba_headless(&rom_path, &rom_data, opts),
        ActiveSystem::Nes => run_nes_headless(&rom_path, &rom_data, opts),
    }
}

fn load_headless_rom(path: &Path) -> anyhow::Result<(PathBuf, Vec<u8>, ActiveSystem)> {
    let (rom_path, preloaded_data, system) = crate::app::detect_and_extract_rom(path)?;
    let rom_data = match preloaded_data {
        Some(data) => data,
        None => std::fs::read(path)?,
    };
    Ok((rom_path, rom_data, system))
}

fn ensure_system_headless_options(system: &str, opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.expect_serial.is_some() {
        anyhow::bail!("--expect-serial is only supported for GB/GBC headless runs");
    }
    if system == "gba" && opts.expect_test_pass {
        anyhow::bail!("--expect-test-pass is only supported for GB/GBC and NES headless runs");
    }
    if !opts.memory_dumps.is_empty() {
        anyhow::bail!("--dump-mem is only supported for GB/GBC headless runs");
    }
    if opts.break_at.is_some() {
        anyhow::bail!("--break-at is only supported for GB/GBC headless runs");
    }

    if system == "gba" {
        let unsupported_gba_trace =
            !opts.trace_opcode_filter.is_empty() || opts.trace_watch_interrupts;
        if unsupported_gba_trace {
            anyhow::bail!(
                "GBA headless tracing supports --trace-opcodes, --trace-opcode-limit, --trace-start-t, --trace-pc-range, and --trace-bus/--trace-bus-read/--trace-bus-write"
            );
        }
    }
    if system == "nes" && opts.trace_watch_interrupts {
        anyhow::bail!("--trace-watch-interrupts is only supported for GB/GBC headless runs");
    }
    if system != "gba"
        && (opts.gba_hidden_bg_layers.iter().any(|&hidden| hidden) || opts.gba_hide_sprites)
    {
        anyhow::bail!(
            "--gba-hide-bg and --gba-hide-sprites are only supported for GBA headless runs"
        );
    }
    if system != "gba" && opts.gba_dump_memory_dir.is_some() {
        anyhow::bail!("--gba-dump-memory is only supported for GBA headless runs");
    }
    if system != "gba" && opts.gba_audio_mutes.iter().any(|&muted| muted) {
        anyhow::bail!("--gba-mute-audio is only supported for GBA headless runs");
    }
    if system != "gba" && opts.audio_dump_path.is_some() {
        anyhow::bail!("--audio-dump is currently only supported for GBA headless runs");
    }
    if opts.gb_dmg_palette_preset.is_some() {
        anyhow::bail!("--gb-dmg-palette/--dmg-palette is only supported for GB/GBC headless runs");
    }

    log::info!("Running {system} headless smoke test");
    Ok(())
}

fn read_headless_state_if_requested(opts: &HeadlessOptions) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(path) = &opts.load_state_path else {
        return Ok(None);
    };
    fs::read(path)
        .map(Some)
        .map_err(|err| anyhow::anyhow!("failed to read save state {}: {err}", path.display()))
}

fn ensure_no_reset_events(system: &str, opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.input_events.iter().any(|event| event.reset) {
        anyhow::bail!("reset input events are not supported for {system} headless runs yet");
    }
    Ok(())
}

fn framebuffer_fingerprint(framebuffer: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

    let mut hash = FNV_OFFSET ^ framebuffer.len() as u64;
    let stride = (framebuffer.len() / 4096).max(1);
    for byte in framebuffer.iter().step_by(stride) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn observe_stuck(
    tracker: &mut Option<StuckTracker>,
    system: &str,
    pc_width: usize,
    frame: u64,
    pc: u64,
    framebuffer: &[u8],
    classification: Option<&str>,
    expected_wait: bool,
    stuck_active: &mut bool,
) {
    let Some(tracker) = tracker.as_mut() else {
        return;
    };

    match tracker.observe(frame, pc, framebuffer, classification, expected_wait) {
        Some(report) if !*stuck_active => {
            println!("{}", format_stuck_report(system, report, pc_width));
            *stuck_active = true;
        }
        None if *stuck_active => {
            println!("[headless] system={system} stuck-cleared frame={frame}");
            *stuck_active = false;
        }
        _ => {}
    }
}

fn input_for_frame(opts: &HeadlessOptions, frame: u64) -> InputMasks {
    let mut input = InputMasks::default();
    for event in &opts.input_events {
        if (event.start_frame..=event.end_frame).contains(&frame) {
            input.buttons |= event.buttons;
            input.dpad |= event.dpad;
        }
        if event.reset && frame == event.start_frame {
            input.reset = true;
        }
    }
    input
}

fn map_host_to_nes_byte(buttons: u8, dpad: u8) -> u8 {
    (buttons & 0x0F)
        | ((dpad & 0x04) << 2)
        | ((dpad & 0x08) << 2)
        | ((dpad & 0x02) << 5)
        | ((dpad & 0x01) << 7)
}

fn format_pc(pc: u64, width: usize) -> String {
    format!("{:0width$X}", pc, width = width)
}

fn format_stuck_report(system: &str, report: &StuckReport, pc_width: usize) -> String {
    let event = if report.expected_wait {
        "idle-detected"
    } else {
        "stuck-detected"
    };
    let classification = report
        .classification
        .as_deref()
        .map(|value| format!(" classification={value}"))
        .unwrap_or_default();
    format!(
        "[headless] system={} {} frame={} window={} unique_pcs={} framebuffer_changed={} first_pc={} last_pc={}{}",
        system,
        event,
        report.frame,
        report.window_frames,
        report.unique_pcs,
        if report.framebuffer_changed { 1 } else { 0 },
        format_pc(report.first_pc, pc_width),
        format_pc(report.last_pc, pc_width),
        classification
    )
}

fn print_perf(system: &str, frames_run: u64, start: Instant) {
    let elapsed = start.elapsed();
    let fps = if elapsed.is_zero() {
        0.0
    } else {
        frames_run as f64 / elapsed.as_secs_f64()
    };
    println!(
        "[headless] system={} elapsed_ms={} fps={:.0}",
        system,
        elapsed.as_millis(),
        fps
    );
}

fn flush_battery(path: &Path, sram_bytes: Option<Vec<u8>>) {
    match crate::save_paths::flush_battery_sram(path, sram_bytes) {
        Ok(Some(save_path)) => log::info!("Saved battery RAM to {}", save_path),
        Ok(None) => {}
        Err(err) => log::error!("Failed to save battery RAM: {}", err),
    }
}

fn fail_on_stuck_if_needed(
    system: &str,
    tracker: Option<&StuckTracker>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if opts.fail_on_stuck
        && let Some(report) = tracker.and_then(StuckTracker::current_report)
        && !report.expected_wait
    {
        anyhow::bail!(
            "{system} headless run detected a stuck window: {} frames, {} unique PCs",
            report.window_frames,
            report.unique_pcs
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(expected_wait: bool) -> StuckReport {
        StuckReport {
            frame: 120,
            window_frames: 60,
            unique_pcs: 1,
            framebuffer_changed: false,
            first_pc: 0x0800_1234,
            last_pc: 0x0800_1234,
            classification: expected_wait.then(|| "gba-swi-halt-idle".to_owned()),
            expected_wait,
        }
    }

    #[test]
    fn stuck_report_formats_expected_gba_wait_as_idle() {
        let text = format_stuck_report("gba", &report(true), 8);

        assert!(text.contains("idle-detected"));
        assert!(text.contains("classification=gba-swi-halt-idle"));
    }

    #[test]
    fn screenshot_sequence_path_uses_frame_number() {
        assert_eq!(
            screenshot_sequence_path(Path::new("shots"), 42),
            PathBuf::from("shots").join("frame_000042.png")
        );
    }

    #[test]
    fn audio_stats_track_nonzero_samples_and_peak() {
        let mut stats = AudioStats::default();
        stats.observe(&[]);
        stats.observe(&[0.0, 0.25, -0.5, 0.000_000_1]);

        assert_eq!(stats.frames_with_samples, 1);
        assert_eq!(stats.sample_count, 4);
        assert_eq!(stats.nonzero_samples, 2);
        assert_eq!(stats.peak_abs, 0.5);
        assert!(stats.mean_abs() > 0.18);
    }

    #[test]
    fn fail_on_stuck_ignores_expected_waits() {
        let mut opts = HeadlessOptions {
            fail_on_stuck: true,
            ..HeadlessOptions::default()
        };
        let mut tracker = StuckTracker::from_options(&HeadlessOptions {
            stuck_window_frames: 1,
            ..HeadlessOptions::default()
        })
        .unwrap();
        tracker.current_report = Some(report(true));

        assert!(fail_on_stuck_if_needed("gba", Some(&tracker), &opts).is_ok());

        opts.fail_on_stuck = true;
        tracker.current_report = Some(report(false));
        assert!(fail_on_stuck_if_needed("gba", Some(&tracker), &opts).is_err());
    }
}
