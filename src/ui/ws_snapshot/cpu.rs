use crate::debug::{CpuDebugSnapshot, DebugSection};
use zeff_emu_common::address::Address;
use zeff_ws_core::emulator::Emulator;

pub(super) fn ws_cpu_snapshot(emu: &Emulator) -> CpuDebugSnapshot {
    let regs = emu.cpu_registers();
    let segs = emu.cpu_segments();
    let flags = emu.cpu_flags();
    let pc = emu.cpu_pc();
    let ppu = emu.ppu_debug_snapshot();
    let mem_around_pc: [(Address, u8); 32] = std::array::from_fn(|i| {
        let addr = pc.wrapping_add(i as u32);
        (addr, emu.cpu_peek8(addr))
    });
    let debug_controls = super::super::build_debug_control_snapshot(
        emu.iter_breakpoints(),
        emu.debug_watchpoints()
            .iter()
            .map(|watch| (watch.address, watch.watch_type)),
        emu.debug_hit_breakpoint(),
        emu.debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.old_value, hit.new_value, hit.watch_type)),
    );
    let recent_opcodes = super::super::opcodes::ws_recent_opcode_display(
        emu.recent_opcodes(super::super::opcodes::RECENT_OPCODE_LINE_COUNT),
    );

    CpuDebugSnapshot {
        register_lines: vec![
            format!(
                "AX:{:04X}  CX:{:04X}  DX:{:04X}  BX:{:04X}",
                regs[0], regs[1], regs[2], regs[3]
            ),
            format!(
                "SP:{:04X}  BP:{:04X}  SI:{:04X}  DI:{:04X}",
                regs[4], regs[5], regs[6], regs[7]
            ),
            format!(
                "ES:{:04X}  CS:{:04X}  SS:{:04X}  DS:{:04X}  IP:{:04X}",
                segs[0],
                segs[1],
                segs[2],
                segs[3],
                emu.cpu_ip()
            ),
            format!("PC:{pc:06X}  FLAGS:{flags:04X}"),
        ],
        flags: vec![
            ('O', flags & 0x0800 != 0),
            ('D', flags & 0x0400 != 0),
            ('I', flags & 0x0200 != 0),
            ('S', flags & 0x0080 != 0),
            ('Z', flags & 0x0040 != 0),
            ('A', flags & 0x0010 != 0),
            ('P', flags & 0x0004 != 0),
            ('C', flags & 0x0001 != 0),
        ],
        status_text: match emu.last_trap() {
            Some(trap) => format!("State: {:?}  Trap: {:?}", emu.cpu_state(), trap),
            None => format!("State: {:?}", emu.cpu_state()),
        },
        cpu_state: format!("{:?}", emu.cpu_state()),
        cycles: emu.cpu_cycles(),
        last_opcode_line: emu.last_fetch().map_or_else(
            || "no instruction fetched yet".into(),
            |fetch| {
                format!(
                    "opcode={:02X} CS:IP={:04X}:{:04X} PC={:06X} cycles={}",
                    fetch.opcode, fetch.cs, fetch.ip, fetch.pc, fetch.cycles
                )
            },
        ),
        sections: vec![
            DebugSection {
                heading: "Interrupts",
                lines: vec![
                    format!(
                        "IVB={:02X} IE={:02X} IRQ={:02X} ACK={:02X}",
                        emu.io_peek8(0xB0),
                        emu.io_peek8(0xB2),
                        emu.io_peek8(0xB4),
                        emu.io_peek8(0xB6)
                    ),
                    format!(
                        "Keypad={:02X} TimerCtrl={:02X} LineCmp={:02X}",
                        emu.io_peek8(0xB5),
                        emu.io_peek8(0xA2),
                        emu.io_peek8(0x03)
                    ),
                ],
            },
            DebugSection {
                heading: "Video",
                lines: vec![
                    format!(
                        "VCOUNT={} line_cycles={} vblank={} frame_ready={}",
                        ppu.vcount, ppu.line_cycles, ppu.in_vblank, ppu.frame_ready
                    ),
                    format!(
                        "LCD={:02X} Display={:02X} Mode={:02X} System={:02X}",
                        emu.io_peek8(0x14),
                        emu.io_peek8(0x00),
                        emu.io_peek8(0x60),
                        emu.io_peek8(0xA0)
                    ),
                ],
            },
            DebugSection {
                heading: "Cartridge",
                lines: vec![
                    format!("CRC32={:08X}", emu.rom_crc32()),
                    format!(
                        "ROM banks C0/C2/C3 = {:02X}/{:02X}/{:02X}",
                        emu.io_peek8(0xC0),
                        emu.io_peek8(0xC2),
                        emu.io_peek8(0xC3)
                    ),
                ],
            },
        ],
        mem_around_pc,
        recent_opcodes,
        breakpoints: debug_controls.breakpoints,
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_watchpoint: debug_controls.hit_watchpoint,
    }
}
