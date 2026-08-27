use std::collections::VecDeque;

use zeff_coleco_core::Emulator;
#[cfg(test)]
use zeff_emu_common::debug::TraceWriteWidth;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind};
use zeff_z80::FetchedInstruction;

use crate::cli::types::{HeadlessBusTraceAccess, HeadlessOptions};

use super::super::format_pc;

pub(super) fn step_coleco_frame_with_trace(
    opts: &HeadlessOptions,
    emulator: &mut Emulator,
    traced: &mut u64,
    bus_traced: &mut u64,
    tail: &mut VecDeque<String>,
) {
    let start_frame = emulator.frame_count();
    while emulator.frame_count() == start_frame && !emulator.is_suspended() {
        let before_cycles = emulator.effective_cycles();
        let bus_trace_printing = !opts.trace_bus_filters.is_empty()
            && (opts.trace_bus_limit == 0 || *bus_traced < opts.trace_bus_limit);
        let (fetched, bus_events) = if bus_trace_printing {
            emulator.step_instruction_with_bus_trace()
        } else {
            (emulator.step_instruction(), Vec::new())
        };
        let Some(fetched) = fetched else {
            break;
        };
        let step_cycles = emulator.effective_cycles().wrapping_sub(before_cycles);
        if opts.trace_opcodes && emulator.effective_cycles() >= opts.trace_start_t {
            let tail_line = format_coleco_op_tail_line(emulator, fetched, step_cycles);
            if tail.len() == 64 {
                tail.pop_front();
            }
            tail.push_back(tail_line);
        }
        if opts.trace_opcodes
            && should_trace_coleco_op(opts, fetched, emulator.effective_cycles())
            && (opts.trace_opcode_limit == 0 || *traced < opts.trace_opcode_limit)
        {
            println!(
                "{}",
                format_coleco_op_line(*traced, emulator, fetched, step_cycles)
            );
            *traced = traced.wrapping_add(1);
        }
        for event in bus_events {
            if bus_trace_printing
                && emulator.effective_cycles() >= opts.trace_start_t
                && should_trace_coleco_bus_event(opts, event)
                && (opts.trace_bus_limit == 0 || *bus_traced < opts.trace_bus_limit)
            {
                println!(
                    "{}",
                    format_coleco_bus_trace_line(*bus_traced, emulator, fetched, event)
                );
                *bus_traced = bus_traced.wrapping_add(1);
            }
        }
    }
}

fn should_trace_coleco_op(
    opts: &HeadlessOptions,
    fetched: FetchedInstruction,
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
    opts.trace_opcode_filter.is_empty() || opts.trace_opcode_filter.contains(&fetched.opcode)
}

fn format_coleco_op_line(
    index: u64,
    emulator: &Emulator,
    fetched: FetchedInstruction,
    step_cycles: u64,
) -> String {
    format!(
        "[coleco-op] #{index} t={} pc={} op={} op1={} op2={} step={} {}",
        emulator.effective_cycles(),
        format_pc(u64::from(fetched.pc), 4),
        format_pc(u64::from(fetched.opcode), 2),
        format_pc(u64::from(emulator.cpu_peek8(fetched.pc.wrapping_add(1))), 2),
        format_pc(u64::from(emulator.cpu_peek8(fetched.pc.wrapping_add(2))), 2),
        step_cycles,
        coleco_cpu_trace_suffix(emulator),
    )
}

fn format_coleco_op_tail_line(
    emulator: &Emulator,
    fetched: FetchedInstruction,
    step_cycles: u64,
) -> String {
    format_coleco_op_line(0, emulator, fetched, step_cycles).replacen(
        "[coleco-op] #0",
        "[coleco-op-tail]",
        1,
    )
}

fn should_trace_coleco_bus_event(opts: &HeadlessOptions, event: BusAccessEvent) -> bool {
    let (addr, is_read) = match event {
        BusAccessEvent::Read { addr, .. } => (u64::from(addr), true),
        BusAccessEvent::Write { addr, .. } => (u64::from(addr), false),
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

fn format_coleco_bus_trace_line(
    traced: u64,
    emulator: &Emulator,
    fetched: FetchedInstruction,
    event: BusAccessEvent,
) -> String {
    let access = match event {
        BusAccessEvent::Read {
            space: TraceWriteKind::Memory,
            addr,
            value,
            ..
        } => format!("read addr={addr:04X} value={value:02X}"),
        BusAccessEvent::Write {
            space: TraceWriteKind::Memory,
            addr,
            old_value,
            new_value,
            ..
        } => format!("write addr={addr:04X} old={old_value:02X} new={new_value:02X}"),
        BusAccessEvent::Read {
            space: TraceWriteKind::Io,
            addr,
            value,
            ..
        } => format!("ioread port={addr:02X} value={value:02X}"),
        BusAccessEvent::Write {
            space: TraceWriteKind::Io,
            addr,
            written_value,
            ..
        } => format!("iowrite port={addr:02X} value={written_value:02X}"),
    };
    format!(
        "[coleco-bus] n={traced} t={} pc={} op={} {} {}",
        emulator.effective_cycles(),
        format_pc(u64::from(fetched.pc), 4),
        format_pc(u64::from(fetched.opcode), 2),
        access,
        coleco_cpu_trace_suffix(emulator),
    )
}

fn coleco_cpu_trace_suffix(emulator: &Emulator) -> String {
    let regs = emulator.cpu().regs();
    let vdp = emulator.bus().vdp().debug_snapshot();
    format!(
        "a={:02X} f={:02X} bc={:04X} de={:04X} hl={:04X} ix={:04X} iy={:04X} sp={:04X} i={:02X} r={:02X} iff={} im={:?} v={} h={} status={:02X} ready={}",
        regs.a,
        regs.f,
        regs.bc(),
        regs.de(),
        regs.hl(),
        regs.ix,
        regs.iy,
        regs.sp,
        regs.i,
        regs.r,
        u8::from(emulator.cpu().interrupts_enabled()),
        emulator.cpu().interrupt_mode(),
        vdp.scanline,
        vdp.cycles_into_line,
        vdp.status,
        u8::from(emulator.bus().psg().ready()),
    )
}

#[cfg(test)]
mod tests {
    use crate::cli::types::HeadlessBusTraceFilter;
    use zeff_coleco_core::constants::BIOS_SIZE;

    use super::*;

    #[test]
    fn coleco_bus_trace_filter_honors_access_type_for_memory_and_io() {
        let mut opts = HeadlessOptions::default();
        opts.trace_bus_filters.push(HeadlessBusTraceFilter {
            start_addr: 0xE0,
            end_addr: 0xE0,
            access: HeadlessBusTraceAccess::Write,
        });

        assert!(should_trace_coleco_bus_event(
            &opts,
            BusAccessEvent::Write {
                at: None,
                space: TraceWriteKind::Io,
                addr: 0xE0,
                old_value: 0x90,
                written_value: 0x90,
                new_value: 0x90,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            }
        ));
        assert!(!should_trace_coleco_bus_event(
            &opts,
            BusAccessEvent::Read {
                at: None,
                space: TraceWriteKind::Io,
                addr: 0xE0,
                value: 0xFF,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            }
        ));
    }

    #[test]
    fn traced_frame_path_matches_normal_frame_boundaries() {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..3].copy_from_slice(&[0xC3, 0x00, 0x00]);
        let mut cartridge = vec![0; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        let mut normal = Emulator::new(&cartridge, &bios, 48_000).unwrap();
        let mut traced = Emulator::new(&cartridge, &bios, 48_000).unwrap();
        let opts = HeadlessOptions::default();
        let mut traced_ops = 0;
        let mut traced_bus = 0;
        let mut tail = VecDeque::new();

        for _ in 0..200 {
            normal.step_frame();
            step_coleco_frame_with_trace(
                &opts,
                &mut traced,
                &mut traced_ops,
                &mut traced_bus,
                &mut tail,
            );
        }

        assert_eq!(normal.effective_cycles(), traced.effective_cycles());
        assert_eq!(normal.frame_count(), traced.frame_count());
        assert_eq!(normal.framebuffer(), traced.framebuffer());
    }
}
