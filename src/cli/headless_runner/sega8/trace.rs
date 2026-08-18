use std::collections::VecDeque;

use zeff_emu_common::debug::TraceWriteKind;
#[cfg(test)]
use zeff_emu_common::debug::TraceWriteWidth;
use zeff_sega8_core::emulator::Emulator as Sega8Emulator;
use zeff_sega8_core::hardware::bus::CpuAccessTraceEvent as Sega8BusTraceEvent;
use zeff_sega8_core::hardware::constants::SMS_Z80_CYCLES_PER_FRAME;
use zeff_sega8_core::hardware::cpu::FetchedInstruction as Sega8FetchedInstruction;

use crate::cli::types::{HeadlessBusTraceAccess, HeadlessOptions};

use super::super::format_pc;
use super::sdsc::Sega8SdscCapture;

pub(super) struct Sega8FrameTraceConfig<'a> {
    pub(super) bus_trace_active: bool,
    pub(super) sdsc_capture_active: bool,
    pub(super) expected_sdsc_text: Option<&'a str>,
}

pub(super) struct Sega8FrameTraceState<'a> {
    pub(super) traced: &'a mut u64,
    pub(super) bus_traced: &'a mut u64,
    pub(super) tail: &'a mut VecDeque<String>,
    pub(super) sdsc_capture: &'a mut Sega8SdscCapture,
}

pub(super) fn step_sega8_frame_with_trace(
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
        Sega8BusTraceEvent::Read {
            space: TraceWriteKind::Memory,
            addr,
            value,
            ..
        } => {
            format!("read addr={addr:04X} value={value:02X}")
        }
        Sega8BusTraceEvent::Write {
            space: TraceWriteKind::Memory,
            addr,
            old_value,
            new_value,
            ..
        } => {
            format!("write addr={addr:04X} old={old_value:02X} new={new_value:02X}")
        }
        Sega8BusTraceEvent::Read {
            space: TraceWriteKind::Io,
            addr,
            value,
            ..
        } => {
            format!("ioread port={addr:02X} value={value:02X}")
        }
        Sega8BusTraceEvent::Write {
            space: TraceWriteKind::Io,
            addr,
            written_value,
            ..
        } => {
            format!("iowrite port={addr:02X} value={written_value:02X}")
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
        "a={} f={} bc={} de={} hl={} ix={} iy={} sp={} i={} r={} iff={} im={:?} v={} h={} status={} line={} mapper={} memctl={} banks={:02X},{:02X},{:02X} cart_ram={} cart_ram_bank={}",
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
        format_pc(u64::from(emulator.bus().memory_control()), 2),
        mapper.slot_banks()[0],
        mapper.slot_banks()[1],
        mapper.slot_banks()[2],
        u8::from(mapper.slot2_cartridge_ram_enabled()),
        mapper.cartridge_ram_bank(),
    )
}

#[cfg(test)]
mod tests {
    use crate::cli::types::HeadlessBusTraceFilter;
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    use super::*;

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
            Sega8BusTraceEvent::Write {
                at: None,
                space: TraceWriteKind::Io,
                addr: 0x7F,
                old_value: 0x90,
                written_value: 0x90,
                new_value: 0x90,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            }
        ));
        assert!(!should_trace_sega8_bus_event(
            &opts,
            Sega8BusTraceEvent::Read {
                at: None,
                space: TraceWriteKind::Io,
                addr: 0x7F,
                value: 0xFF,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            }
        ));
        assert!(!should_trace_sega8_bus_event(
            &opts,
            Sega8BusTraceEvent::Write {
                addr: 0xC000,
                old_value: 0,
                written_value: 1,
                new_value: 1,
                at: None,
                space: TraceWriteKind::Memory,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
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
            Sega8BusTraceEvent::Write {
                at: None,
                space: TraceWriteKind::Io,
                addr: 0x7F,
                old_value: 0x90,
                written_value: 0x90,
                new_value: 0x90,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            },
        );

        assert!(line.contains("[sega8-bus] n=3"));
        assert!(line.contains("pc=0002"));
        assert!(line.contains("op=D3"));
        assert!(line.contains("iowrite port=7F value=90"));
    }
}
