use crate::debug::{CpuDebugSnapshot, DebugSection};
use crate::ui::{DebugControlSources, build_debug_control_snapshot};
use zeff_coleco_core::Emulator;
use zeff_z80::{
    Z80_FLAG_BIT_3, Z80_FLAG_BIT_5, Z80_FLAG_CARRY, Z80_FLAG_HALF_CARRY, Z80_FLAG_PARITY_OVERFLOW,
    Z80_FLAG_SIGN, Z80_FLAG_SUBTRACT, Z80_FLAG_ZERO,
};

pub(super) fn coleco_cpu_snapshot(emu: &Emulator) -> CpuDebugSnapshot {
    let cpu = emu.cpu();
    let regs = cpu.regs();
    let vdp = emu.bus().vdp();
    let psg = emu.bus().psg();
    let vdp_snapshot = vdp.debug_snapshot();
    let psg_snapshot = psg.debug_snapshot();
    let input = emu.controller_ports();
    let debug_controls = build_debug_control_snapshot(DebugControlSources {
        breakpoints: emu.iter_breakpoints(),
        one_shot_breakpoints: emu.iter_one_shot_breakpoints(),
        breakpoint_hit_conditions: emu.iter_breakpoint_hit_conditions(),
        event_breakpoints: emu.iter_event_breakpoints(),
        watchpoints: emu
            .debug_watchpoints()
            .iter()
            .map(|watch| (watch.address, watch.end_address, watch.watch_type)),
        hit_breakpoint: emu.debug_hit_breakpoint(),
        hit_watchpoint: emu
            .debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.old_value, hit.new_value, hit.watch_type)),
        hit_event: emu.debug_hit_event(),
    });

    CpuDebugSnapshot {
        register_lines: vec![
            format!(
                "AF:{:04X}  BC:{:04X}  DE:{:04X}  HL:{:04X}",
                regs.af(),
                regs.bc(),
                regs.de(),
                regs.hl()
            ),
            format!(
                "IX:{:04X}  IY:{:04X}  SP:{:04X}  PC:{:04X}",
                regs.ix, regs.iy, regs.sp, regs.pc
            ),
            format!(
                "A:{:02X} F:{:02X} B:{:02X} C:{:02X} D:{:02X} E:{:02X} H:{:02X} L:{:02X} I:{:02X} R:{:02X}",
                regs.a, regs.f, regs.b, regs.c, regs.d, regs.e, regs.h, regs.l, regs.i, regs.r
            ),
        ],
        flags: vec![
            ('S', regs.f & Z80_FLAG_SIGN != 0),
            ('Z', regs.f & Z80_FLAG_ZERO != 0),
            ('5', regs.f & Z80_FLAG_BIT_5 != 0),
            ('H', regs.f & Z80_FLAG_HALF_CARRY != 0),
            ('3', regs.f & Z80_FLAG_BIT_3 != 0),
            ('P', regs.f & Z80_FLAG_PARITY_OVERFLOW != 0),
            ('N', regs.f & Z80_FLAG_SUBTRACT != 0),
            ('C', regs.f & Z80_FLAG_CARRY != 0),
        ],
        status_text: match cpu.trap() {
            Some(trap) => format!("State: {:?}  Trap: {:?}", cpu.state(), trap),
            None => format!("State: {:?}", cpu.state()),
        },
        cpu_state: format!("{:?}", cpu.state()),
        pc: regs.pc.into(),
        cycles: emu.effective_cycles(),
        last_opcode_line: format!(
            "PC={:04X} opcode={:02X} cpu_cycles={} effective_cycles={}",
            cpu.last_opcode_pc(),
            cpu.last_opcode(),
            cpu.cycles(),
            emu.effective_cycles()
        ),
        sections: vec![
            DebugSection {
                heading: "Interrupts",
                lines: vec![
                    format!(
                        "IM={:?} IFF1={} IFF2={} NMI={}",
                        cpu.interrupt_mode(),
                        on_off(cpu.interrupts_enabled()),
                        on_off(cpu.saved_interrupts_enabled()),
                        on_off(vdp_snapshot.nmi_line)
                    ),
                    format!(
                        "scanline={} cycle={} frame={}",
                        vdp_snapshot.scanline,
                        vdp_snapshot.cycles_into_line,
                        vdp_snapshot.frame_count
                    ),
                ],
            },
            DebugSection {
                heading: "VDP",
                lines: vec![
                    format!(
                        "status={:02X} addr={:04X} mode={:?} display={}",
                        vdp_snapshot.status,
                        vdp_snapshot.address,
                        vdp_snapshot.mode,
                        on_off(vdp_snapshot.display_enabled)
                    ),
                    format!("regs {}", hex_bytes(vdp.registers())),
                    format!(
                        "name={:04X} pattern={:04X} color={:04X} sat={:04X}",
                        vdp_snapshot.name_table_base,
                        vdp_snapshot.pattern_table_base,
                        vdp_snapshot.color_table_base,
                        vdp_snapshot.sprite_attribute_table_base
                    ),
                    format!(
                        "VRAM nonzero={} framebuffer={} bytes",
                        vdp.vram().iter().filter(|&&byte| byte != 0).count(),
                        vdp.framebuffer().len()
                    ),
                ],
            },
            DebugSection {
                heading: "PSG",
                lines: vec![
                    format!(
                        "tone={:03X},{:03X},{:03X} volume={:X},{:X},{:X},{:X} noise={:X}",
                        psg_snapshot.tone_periods[0],
                        psg_snapshot.tone_periods[1],
                        psg_snapshot.tone_periods[2],
                        psg_snapshot.volumes[0],
                        psg_snapshot.volumes[1],
                        psg_snapshot.volumes[2],
                        psg_snapshot.volumes[3],
                        psg_snapshot.noise_control
                    ),
                    format!(
                        "sample_rate={} generation={} buffered_samples={} writes={} ready={} hold={}",
                        psg_snapshot.sample_rate,
                        on_off(psg_snapshot.sample_generation_enabled),
                        psg_snapshot.buffered_sample_count,
                        psg_snapshot.write_count,
                        on_off(psg_snapshot.ready),
                        psg_snapshot.ready_clocks_remaining
                    ),
                ],
            },
            DebugSection {
                heading: "Controllers",
                lines: vec![format!(
                    "mux={:?} P1={:02X} P2={:02X}",
                    input.mux(),
                    input.read_player(0).unwrap_or(0xFF),
                    input.read_player(1).unwrap_or(0xFF)
                )],
            },
        ],
        io_registers: Vec::new(),
        recent_opcodes: super::super::opcodes::sega8_recent_opcode_display(
            emu.recent_opcodes(super::super::opcodes::RECENT_OPCODE_LINE_COUNT),
        ),
        call_stack: Vec::new(),
        call_stack_available: false,
        breakpoints: debug_controls.breakpoints,
        one_shot_breakpoints: debug_controls.one_shot_breakpoints,
        breakpoint_hit_conditions: debug_controls.breakpoint_hit_conditions,
        supported_events: vec![zeff_emu_common::debug::DebugEvent::Interrupt],
        event_breakpoints: debug_controls.event_breakpoints,
        rom_breakpoints: Vec::new(),
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_rom_breakpoint: None,
        hit_watchpoint: debug_controls.hit_watchpoint,
        hit_event: debug_controls.hit_event,
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
