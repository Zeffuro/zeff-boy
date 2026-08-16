use crate::debug::{CpuDebugSnapshot, DebugSection};
use zeff_emu_common::address::Address;

pub(super) fn nes_cpu_snapshot(emu: &zeff_nes_core::emulator::Emulator) -> CpuDebugSnapshot {
    let snap = zeff_nes_core::debug::NesDebugSnapshot::capture(emu);

    let register_lines = vec![
        format!("A:{:02X}  X:{:02X}  Y:{:02X}", snap.a, snap.x, snap.y),
        format!("PC:{:04X}  SP:{:02X}  P:{:02X}", snap.pc, snap.sp, snap.p),
    ];

    let flags = vec![
        ('N', snap.flag_n),
        ('V', snap.flag_v),
        ('D', snap.flag_d),
        ('I', snap.flag_i),
        ('Z', snap.flag_z),
        ('C', snap.flag_c),
    ];

    let status_text = format!("State: {}", snap.cpu_state);

    let int_lines = vec![format!(
        "NMI pending: {}  IRQ line: {}",
        snap.nmi_pending, snap.irq_line
    )];

    let ppu_lines = vec![
        format!(
            "Scanline:{:3}  Dot:{:3}  Frame:{}",
            snap.ppu_scanline, snap.ppu_dot, snap.ppu_frame_count
        ),
        format!(
            "CTRL:{:02X}  MASK:{:02X}  STATUS:{:02X}",
            snap.ppu_ctrl, snap.ppu_mask, snap.ppu_status
        ),
        format!(
            "V:{:04X}  T:{:04X}  FineX:{}",
            snap.ppu_v, snap.ppu_t, snap.ppu_fine_x
        ),
        format!("VBlank: {}", snap.ppu_in_vblank),
    ];

    let sections = vec![
        DebugSection {
            heading: "Interrupts",
            lines: int_lines,
        },
        DebugSection {
            heading: "PPU",
            lines: ppu_lines,
        },
    ];

    let recent_opcodes =
        super::super::opcodes::nes_recent_opcode_display(snap.recent_ops.iter().copied());

    let debug_controls = super::super::build_debug_control_snapshot(
        emu.iter_breakpoints().map(Address::from),
        emu.debug_watchpoints()
            .iter()
            .map(|watch| (Address::from(watch.address), watch.watch_type)),
        emu.debug_hit_breakpoint().map(Address::from),
        emu.debug_hit_watchpoint().map(|hit| {
            (
                Address::from(hit.address),
                hit.old_value,
                hit.new_value,
                hit.watch_type,
            )
        }),
    );

    CpuDebugSnapshot {
        register_lines,
        flags,
        status_text,
        cpu_state: snap.cpu_state.to_string(),
        cycles: snap.cycles,
        last_opcode_line: format!("@ {:04X} = {:02X}", snap.last_opcode_pc, snap.last_opcode),
        sections,
        mem_around_pc: snap.mem_around_pc.map(|(addr, value)| (addr.into(), value)),
        recent_opcodes,
        breakpoints: debug_controls.breakpoints,
        rom_breakpoints: Vec::new(),
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_rom_breakpoint: None,
        hit_watchpoint: debug_controls.hit_watchpoint,
    }
}
