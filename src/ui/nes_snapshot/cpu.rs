use crate::debug::{
    CallStackDisplay, CpuDebugSnapshot, DebugSection, IoBitDisplay, IoRegisterDisplay,
};
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
    let call_stack = snap
        .call_stack
        .iter()
        .map(|frame| CallStackDisplay {
            target: frame.target.into(),
            return_address: frame.return_address.into(),
            target_rom_offset: frame
                .target_rom_offset
                .and_then(|value| u64::try_from(value).ok()),
            return_rom_offset: frame
                .return_rom_offset
                .and_then(|value| u64::try_from(value).ok()),
            kind: match frame.kind {
                zeff_nes_core::debug::CallStackKind::Call => "JSR",
                zeff_nes_core::debug::CallStackKind::Interrupt => "INT",
            },
        })
        .collect();

    let debug_controls =
        super::super::build_debug_control_snapshot(super::super::DebugControlSources {
            breakpoints: emu.iter_breakpoints().map(Address::from),
            one_shot_breakpoints: emu.iter_one_shot_breakpoints().map(Address::from),
            breakpoint_hit_conditions: emu.iter_breakpoint_hit_conditions(),
            event_breakpoints: emu.iter_event_breakpoints(),
            watchpoints: emu.debug_watchpoints().iter().map(|watch| {
                (
                    Address::from(watch.address),
                    Address::from(watch.end_address),
                    watch.watch_type,
                )
            }),
            hit_breakpoint: emu.debug_hit_breakpoint().map(Address::from),
            hit_watchpoint: emu.debug_hit_watchpoint().map(|hit| {
                (
                    Address::from(hit.address),
                    hit.old_value,
                    hit.new_value,
                    hit.watch_type,
                )
            }),
            hit_event: emu.debug_hit_event(),
        });

    CpuDebugSnapshot {
        register_lines,
        flags,
        status_text,
        cpu_state: snap.cpu_state.to_string(),
        pc: snap.pc.into(),
        cycles: snap.cycles,
        last_opcode_line: format!("@ {:04X} = {:02X}", snap.last_opcode_pc, snap.last_opcode),
        sections,
        io_registers: vec![
            io_register(
                "PPUCTRL",
                0x2000,
                snap.ppu_ctrl,
                0xFF,
                &[
                    (7, "NMI"),
                    (5, "OBJ 16"),
                    (4, "BG table"),
                    (3, "OBJ table"),
                    (2, "Inc 32"),
                    (1, "NT 1"),
                    (0, "NT 0"),
                ],
            ),
            io_register(
                "PPUMASK",
                0x2001,
                snap.ppu_mask,
                0xFF,
                &[
                    (7, "Blue"),
                    (6, "Green"),
                    (5, "Red"),
                    (4, "OBJ"),
                    (3, "BG"),
                    (2, "OBJ left"),
                    (1, "BG left"),
                    (0, "Gray"),
                ],
            ),
            io_register(
                "PPUSTATUS",
                0x2002,
                snap.ppu_status,
                0,
                &[(7, "VBlank"), (6, "OBJ 0"), (5, "Overflow")],
            ),
        ],
        recent_opcodes,
        call_stack,
        call_stack_available: true,
        breakpoints: debug_controls.breakpoints,
        one_shot_breakpoints: debug_controls.one_shot_breakpoints,
        breakpoint_hit_conditions: debug_controls.breakpoint_hit_conditions,
        supported_events: vec![
            zeff_emu_common::debug::DebugEvent::Interrupt,
            zeff_emu_common::debug::DebugEvent::Dma,
        ],
        event_breakpoints: debug_controls.event_breakpoints,
        rom_breakpoints: Vec::new(),
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_rom_breakpoint: None,
        hit_watchpoint: debug_controls.hit_watchpoint,
        hit_event: debug_controls.hit_event,
    }
}

fn io_register(
    name: &'static str,
    address: u32,
    value: u8,
    writable_mask: u8,
    bits: &[(u8, &'static str)],
) -> IoRegisterDisplay {
    IoRegisterDisplay {
        name,
        address,
        value: value.into(),
        width: 1,
        writable_mask: writable_mask.into(),
        bits: bits
            .iter()
            .map(|&(bit, label)| IoBitDisplay {
                mask: 1 << bit,
                label,
            })
            .collect(),
    }
}
