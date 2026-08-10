use std::path::Path;
use std::{collections::VecDeque, time::Instant};

use zeff_sega8_core::emulator::Emulator as Sega8Emulator;
use zeff_sega8_core::hardware::bus::CpuAccessTraceEvent as Sega8BusTraceEvent;
use zeff_sega8_core::hardware::constants::SMS_Z80_CYCLES_PER_FRAME;
use zeff_sega8_core::hardware::cpu::FetchedInstruction as Sega8FetchedInstruction;

use crate::cli::types::{HeadlessBusTraceAccess, HeadlessOptions};
use crate::emu_backend::ActiveSystem;

use super::{
    AudioStats, Sega8DebugStateRequest, StuckTracker, emit_debug_state, ensure_no_reset_events,
    ensure_system_headless_options, fail_on_stuck_if_needed, flush_battery, format_pc,
    input_for_frame, observe_stuck, print_perf, read_headless_state_if_requested,
    screenshot_path_if_written, sega8_debug_state, write_audio_dump_f32le,
    write_final_screenshot_if_needed, write_screenshot_if_requested,
    write_screenshot_sequence_if_requested,
};

const SDSC_DEBUG_CONSOLE_COMMAND_PORT: u8 = 0xFC;
const SDSC_DEBUG_CONSOLE_DATA_PORT: u8 = 0xFD;
const SDSC_DEBUG_CONSOLE_SUSPEND_COMMAND: u8 = 0x01;
const SDSC_DEBUG_CONSOLE_CLEAR_SCREEN_COMMAND: u8 = 0x02;
const SDSC_TEXT_PREVIEW_MAX_CHARS: usize = 512;

#[derive(Default)]
struct Sega8SdscCapture {
    text: String,
    command_count: u64,
    suspend_seen: bool,
}

impl Sega8SdscCapture {
    fn record_bus_event(&mut self, event: Sega8BusTraceEvent) {
        let Sega8BusTraceEvent::IoWrite { port, value } = event else {
            return;
        };

        match port {
            SDSC_DEBUG_CONSOLE_DATA_PORT => self.text.push(char::from(value)),
            SDSC_DEBUG_CONSOLE_COMMAND_PORT => {
                self.command_count = self.command_count.wrapping_add(1);
                match value {
                    SDSC_DEBUG_CONSOLE_SUSPEND_COMMAND => self.suspend_seen = true,
                    SDSC_DEBUG_CONSOLE_CLEAR_SCREEN_COMMAND => self.text.clear(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn preview(&self) -> String {
        let mut preview = String::new();
        for ch in self.text.chars().take(SDSC_TEXT_PREVIEW_MAX_CHARS) {
            if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
                preview.push(' ');
            } else {
                preview.push(ch);
            }
        }
        if self.text.chars().count() > SDSC_TEXT_PREVIEW_MAX_CHARS {
            preview.push_str("...");
        }
        preview
    }
}

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
    let mut emulator = Sega8Emulator::new_with_hint(
        rom_data,
        zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE,
        hint,
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
        emulator.set_input(current_input.buttons, current_input.dpad);
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

        observe_stuck(
            &mut stuck,
            system.code(),
            4,
            frames_run,
            u64::from(emulator.cpu().regs().pc),
            emulator.framebuffer(),
            None,
            sega8_wait_classification(&emulator),
            sega8_wait_classification(&emulator).is_some(),
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

struct Sega8FrameTraceConfig<'a> {
    bus_trace_active: bool,
    sdsc_capture_active: bool,
    expected_sdsc_text: Option<&'a str>,
}

struct Sega8FrameTraceState<'a> {
    traced: &'a mut u64,
    bus_traced: &'a mut u64,
    tail: &'a mut VecDeque<String>,
    sdsc_capture: &'a mut Sega8SdscCapture,
}

fn step_sega8_frame_with_trace(
    opts: &HeadlessOptions,
    emulator: &mut Sega8Emulator,
    config: Sega8FrameTraceConfig<'_>,
    state: &mut Sega8FrameTraceState<'_>,
) -> bool {
    let mut expected_sdsc_seen = false;
    let target_cycles = emulator
        .cpu()
        .cycles()
        .wrapping_add(u64::from(SMS_Z80_CYCLES_PER_FRAME));
    while emulator.cpu().cycles() < target_cycles && !emulator.is_suspended() {
        let before_cycles = emulator.cpu().cycles();
        let bus_trace_printing = !opts.trace_bus_filters.is_empty()
            && (opts.trace_bus_limit == 0 || *state.bus_traced < opts.trace_bus_limit);
        let bus_trace_collecting =
            config.bus_trace_active && (config.sdsc_capture_active || bus_trace_printing);
        let (fetched, bus_events) = if bus_trace_collecting {
            emulator.step_instruction_with_bus_trace()
        } else {
            (emulator.step_instruction(), Vec::new())
        };
        let Some(fetched) = fetched else {
            break;
        };
        let step_cycles = emulator.cpu().cycles().wrapping_sub(before_cycles);
        if opts.trace_opcodes && emulator.cpu().cycles() >= opts.trace_start_t {
            let tail_line = format_sega8_op_tail_line(emulator, fetched, step_cycles);
            if state.tail.len() == 64 {
                state.tail.pop_front();
            }
            state.tail.push_back(tail_line);
        }
        if opts.trace_opcodes
            && should_trace_sega8_op(opts, fetched, emulator.cpu().cycles())
            && (opts.trace_opcode_limit == 0 || *state.traced < opts.trace_opcode_limit)
        {
            println!(
                "{}",
                format_sega8_op_line(*state.traced, emulator, fetched, step_cycles)
            );
            *state.traced = state.traced.wrapping_add(1);
        }
        if bus_trace_collecting {
            for event in bus_events {
                if config.sdsc_capture_active {
                    state.sdsc_capture.record_bus_event(event);
                    if config.expected_sdsc_text.is_some_and(|expected| {
                        !expected.is_empty() && state.sdsc_capture.text().contains(expected)
                    }) {
                        expected_sdsc_seen = true;
                    }
                }

                if bus_trace_printing
                    && emulator.cpu().cycles() >= opts.trace_start_t
                    && should_trace_sega8_bus_event(opts, event)
                    && (opts.trace_bus_limit == 0 || *state.bus_traced < opts.trace_bus_limit)
                {
                    println!(
                        "{}",
                        format_sega8_bus_trace_line(*state.bus_traced, emulator, fetched, event)
                    );
                    *state.bus_traced = state.bus_traced.wrapping_add(1);
                }
            }
        }
        if expected_sdsc_seen {
            break;
        }
    }
    emulator.finish_frame();
    expected_sdsc_seen
}

fn should_trace_sega8_op(
    opts: &HeadlessOptions,
    fetched: Sega8FetchedInstruction,
    cycles: u64,
) -> bool {
    if cycles < opts.trace_start_t {
        return false;
    }
    if let Some((start, end)) = opts.trace_pc_range
        && !(start..=end).contains(&u64::from(fetched.pc))
    {
        return false;
    }
    if !opts.trace_opcode_filter.is_empty() && !opts.trace_opcode_filter.contains(&fetched.opcode) {
        return false;
    }
    true
}

fn format_sega8_op_line(
    index: u64,
    emulator: &Sega8Emulator,
    fetched: Sega8FetchedInstruction,
    step_cycles: u64,
) -> String {
    format!(
        "[sega8-op] #{index} t={} pc={} op={} op1={} op2={} step={} {}",
        emulator.cpu().cycles(),
        format_pc(u64::from(fetched.pc), 4),
        format_pc(u64::from(fetched.opcode), 2),
        format_pc(
            u64::from(emulator.bus().cpu_read(fetched.pc.wrapping_add(1))),
            2
        ),
        format_pc(
            u64::from(emulator.bus().cpu_read(fetched.pc.wrapping_add(2))),
            2
        ),
        step_cycles,
        sega8_cpu_trace_suffix(emulator),
    )
}

fn format_sega8_op_tail_line(
    emulator: &Sega8Emulator,
    fetched: Sega8FetchedInstruction,
    step_cycles: u64,
) -> String {
    format_sega8_op_line(0, emulator, fetched, step_cycles).replacen(
        "[sega8-op] #0",
        "[sega8-op-tail]",
        1,
    )
}

fn should_trace_sega8_bus_event(opts: &HeadlessOptions, event: Sega8BusTraceEvent) -> bool {
    let (addr, is_read) = match event {
        Sega8BusTraceEvent::Read { addr, .. } => (u64::from(addr), true),
        Sega8BusTraceEvent::Write { addr, .. } => (u64::from(addr), false),
        Sega8BusTraceEvent::IoRead { port, .. } => (u64::from(port), true),
        Sega8BusTraceEvent::IoWrite { port, .. } => (u64::from(port), false),
    };

    opts.trace_bus_filters.iter().any(|filter| {
        addr >= filter.start_addr
            && addr <= filter.end_addr
            && matches!(
                (filter.access, is_read),
                (HeadlessBusTraceAccess::ReadWrite, _)
                    | (HeadlessBusTraceAccess::Read, true)
                    | (HeadlessBusTraceAccess::Write, false)
            )
    })
}

fn format_sega8_bus_trace_line(
    traced: u64,
    emulator: &Sega8Emulator,
    fetched: Sega8FetchedInstruction,
    event: Sega8BusTraceEvent,
) -> String {
    let access = match event {
        Sega8BusTraceEvent::Read { addr, value } => {
            format!("read addr={addr:04X} value={value:02X}")
        }
        Sega8BusTraceEvent::Write {
            addr,
            old_value,
            new_value,
        } => {
            format!("write addr={addr:04X} old={old_value:02X} new={new_value:02X}")
        }
        Sega8BusTraceEvent::IoRead { port, value } => {
            format!("ioread port={port:02X} value={value:02X}")
        }
        Sega8BusTraceEvent::IoWrite { port, value } => {
            format!("iowrite port={port:02X} value={value:02X}")
        }
    };

    format!(
        "[sega8-bus] n={} t={} pc={} op={} {} {}",
        traced,
        emulator.cpu().cycles(),
        format_pc(u64::from(fetched.pc), 4),
        format_pc(u64::from(fetched.opcode), 2),
        access,
        sega8_cpu_trace_suffix(emulator),
    )
}

fn sega8_cpu_trace_suffix(emulator: &Sega8Emulator) -> String {
    let regs = emulator.cpu().regs();
    let vdp = emulator.bus().vdp();
    let mapper = emulator.bus().mapper();
    format!(
        "a={} f={} bc={} de={} hl={} ix={} iy={} sp={} i={} r={} iff={} im={:?} v={} h={} status={} line={} mapper={} banks={:02X},{:02X},{:02X} cart_ram={} cart_ram_bank={}",
        format_pc(u64::from(regs.a), 2),
        format_pc(u64::from(regs.f), 2),
        format_pc(u64::from(regs.bc()), 4),
        format_pc(u64::from(regs.de()), 4),
        format_pc(u64::from(regs.hl()), 4),
        format_pc(u64::from(regs.ix), 4),
        format_pc(u64::from(regs.iy), 4),
        format_pc(u64::from(regs.sp), 4),
        format_pc(u64::from(regs.i), 2),
        format_pc(u64::from(regs.r), 2),
        u8::from(emulator.cpu().interrupts_enabled()),
        emulator.cpu().interrupt_mode(),
        vdp.v_counter(),
        vdp.h_counter(),
        format_pc(u64::from(vdp.status()), 2),
        vdp.line_counter(),
        mapper.kind_label(),
        mapper.slot_banks()[0],
        mapper.slot_banks()[1],
        mapper.slot_banks()[2],
        u8::from(mapper.slot2_cartridge_ram_enabled()),
        mapper.cartridge_ram_bank(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::types::HeadlessBusTraceFilter;
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    #[test]
    fn sega8_bus_trace_filter_honors_access_type_for_memory_and_io() {
        let mut opts = HeadlessOptions::default();
        opts.trace_bus_filters.push(HeadlessBusTraceFilter {
            start_addr: 0x7F,
            end_addr: 0x7F,
            access: HeadlessBusTraceAccess::Write,
        });

        assert!(should_trace_sega8_bus_event(
            &opts,
            Sega8BusTraceEvent::IoWrite {
                port: 0x7F,
                value: 0x90
            }
        ));
        assert!(!should_trace_sega8_bus_event(
            &opts,
            Sega8BusTraceEvent::IoRead {
                port: 0x7F,
                value: 0xFF
            }
        ));
        assert!(!should_trace_sega8_bus_event(
            &opts,
            Sega8BusTraceEvent::Write {
                addr: 0xC000,
                old_value: 0,
                new_value: 1
            }
        ));
    }

    #[test]
    fn sega8_bus_trace_line_labels_io_events() {
        let emulator = Sega8Emulator::new_with_hint(&[0x76], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");
        let fetched = Sega8FetchedInstruction {
            pc: 0x0002,
            opcode: 0xD3,
            cycles: 11,
        };

        let line = format_sega8_bus_trace_line(
            3,
            &emulator,
            fetched,
            Sega8BusTraceEvent::IoWrite {
                port: 0x7F,
                value: 0x90,
            },
        );

        assert!(line.contains("[sega8-bus] n=3"));
        assert!(line.contains("pc=0002"));
        assert!(line.contains("op=D3"));
        assert!(line.contains("iowrite port=7F value=90"));
    }

    #[test]
    fn sega8_sdsc_capture_collects_text_and_commands() {
        let mut capture = Sega8SdscCapture::default();

        for value in b"OK" {
            capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
                port: SDSC_DEBUG_CONSOLE_DATA_PORT,
                value: *value,
            });
        }
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_COMMAND_PORT,
            value: SDSC_DEBUG_CONSOLE_SUSPEND_COMMAND,
        });

        assert_eq!(capture.text(), "OK");
        assert_eq!(capture.command_count, 1);
        assert!(capture.suspend_seen);
    }

    #[test]
    fn sega8_sdsc_clear_screen_command_clears_captured_text() {
        let mut capture = Sega8SdscCapture::default();
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_DATA_PORT,
            value: b'X',
        });
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_COMMAND_PORT,
            value: SDSC_DEBUG_CONSOLE_CLEAR_SCREEN_COMMAND,
        });
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_DATA_PORT,
            value: b'Y',
        });

        assert_eq!(capture.text(), "Y");
    }
}
