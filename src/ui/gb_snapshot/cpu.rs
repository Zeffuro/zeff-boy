use crate::debug::{
    CallStackDisplay, CpuDebugSnapshot, DebugSection, IoBitDisplay, IoRegisterDisplay,
};
use zeff_emu_common::address::Address;

pub(super) fn gb_cpu_snapshot(info: &zeff_gb_core::debug::DebugInfo) -> CpuDebugSnapshot {
    let register_lines = vec![
        format!(
            "A:{:02X}  F:{:02X}    AF:{:04X}",
            info.a,
            info.f,
            (info.a as u16) << 8 | info.f as u16
        ),
        format!(
            "B:{:02X}  C:{:02X}    BC:{:04X}",
            info.b,
            info.c,
            (info.b as u16) << 8 | info.c as u16
        ),
        format!(
            "D:{:02X}  E:{:02X}    DE:{:04X}",
            info.d,
            info.e,
            (info.d as u16) << 8 | info.e as u16
        ),
        format!(
            "H:{:02X}  L:{:02X}    HL:{:04X}",
            info.h,
            info.l,
            (info.h as u16) << 8 | info.l as u16
        ),
        format!("PC:{:04X}  SP:{:04X}", info.pc, info.sp),
    ];

    let flags = vec![
        ('Z', info.f & 0x80 != 0),
        ('N', info.f & 0x40 != 0),
        ('H', info.f & 0x20 != 0),
        ('C', info.f & 0x10 != 0),
    ];
    let status_text = format!("IME: {}  State: {}", info.ime, info.cpu_state);

    let int_names = ["VBlank", "STAT", "Timer", "Serial", "Joypad"];
    let mut int_lines = vec![format!(
        "IF:{:02X}  IE:{:02X}  pending:{:02X}",
        info.if_reg,
        info.ie,
        info.if_reg & info.ie
    )];
    let mut int_detail = String::new();
    for (i, name) in int_names.iter().enumerate() {
        let ie = if info.ie & (1 << i) != 0 { "E" } else { "." };
        let ifr = if info.if_reg & (1 << i) != 0 {
            "F"
        } else {
            "."
        };
        if !int_detail.is_empty() {
            int_detail.push_str("  ");
        }
        int_detail.push_str(&format!("{}:{}{}", name, ie, ifr));
    }
    int_lines.push(int_detail);

    let mode = info.ppu.stat & 0x03;
    let mode_name = match mode {
        0 => "HBlank",
        1 => "VBlank",
        2 => "OAM Scan",
        3 => "Drawing",
        _ => "?",
    };
    let ppu_lines = vec![
        format!(
            "LY:{:02X}({:3})  LCDC:{:02X}  STAT:{:02X}",
            info.ppu.ly, info.ppu.ly, info.ppu.lcdc, info.ppu.stat
        ),
        format!("Mode: {} ({})", mode, mode_name),
    ];

    let timer_lines = vec![
        format!(
            "DIV:{:02X}  TIMA:{:02X}  TMA:{:02X}  TAC:{:02X}",
            info.div, info.tima, info.tma, info.tac
        ),
        format!(
            "Timer: {} @ {}",
            if info.tac & 0x04 != 0 { "ON" } else { "OFF" },
            match info.tac & 0x03 {
                0 => "4096 Hz",
                1 => "262144 Hz",
                2 => "65536 Hz",
                3 => "16384 Hz",
                _ => "?",
            }
        ),
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
        DebugSection {
            heading: "Timer",
            lines: timer_lines,
        },
    ];

    let recent_opcodes =
        super::super::opcodes::gb_recent_opcode_display(info.recent_ops.iter().copied());
    let call_stack = info
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
                zeff_gb_core::debug::CallStackKind::Call => "CALL",
                zeff_gb_core::debug::CallStackKind::Restart => "RST",
                zeff_gb_core::debug::CallStackKind::Interrupt => "INT",
            },
        })
        .collect();

    let debug_controls = super::super::build_debug_control_snapshot(
        info.breakpoints.iter().copied().map(Address::from),
        info.one_shot_breakpoints.iter().copied().map(Address::from),
        info.watchpoints.iter().map(|watch| {
            (
                Address::from(watch.address),
                Address::from(watch.end_address),
                watch.watch_type,
            )
        }),
        info.hit_breakpoint.map(Address::from),
        info.hit_watchpoint.as_ref().map(|hit| {
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
        cpu_state: info.cpu_state.to_string(),
        pc: info.pc.into(),
        cycles: info.cycles,
        last_opcode_line: format!("@ {:04X} = {:02X}", info.last_opcode_pc, info.last_opcode),
        sections,
        io_registers: gb_io_registers(info),
        recent_opcodes,
        call_stack,
        call_stack_available: true,
        breakpoints: debug_controls.breakpoints,
        one_shot_breakpoints: debug_controls.one_shot_breakpoints,
        rom_breakpoints: info
            .rom_breakpoints
            .iter()
            .filter_map(|&offset| u64::try_from(offset).ok())
            .collect(),
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_rom_breakpoint: info
            .hit_rom_breakpoint
            .and_then(|offset| u64::try_from(offset).ok()),
        hit_watchpoint: debug_controls.hit_watchpoint,
    }
}

fn gb_io_registers(info: &zeff_gb_core::debug::DebugInfo) -> Vec<IoRegisterDisplay> {
    vec![
        io_register(
            "LCDC",
            0xFF40,
            info.ppu.lcdc,
            0xFF,
            &[
                (7, "LCD"),
                (6, "Win map"),
                (5, "Window"),
                (4, "Tile data"),
                (3, "BG map"),
                (2, "OBJ 16"),
                (1, "OBJ"),
                (0, "BG"),
            ],
        ),
        io_register(
            "STAT",
            0xFF41,
            info.ppu.stat,
            0x78,
            &[
                (6, "LYC IRQ"),
                (5, "OAM IRQ"),
                (4, "VBlank IRQ"),
                (3, "HBlank IRQ"),
            ],
        ),
        io_register(
            "TAC",
            0xFF07,
            info.tac,
            0x07,
            &[(2, "Timer"), (1, "Clock 1"), (0, "Clock 0")],
        ),
        io_register("IF", 0xFF0F, info.if_reg, 0x1F, &interrupt_bits()),
        io_register("IE", 0xFFFF, info.ie, 0x1F, &interrupt_bits()),
    ]
}

fn interrupt_bits() -> [(u8, &'static str); 5] {
    [
        (4, "Joypad"),
        (3, "Serial"),
        (2, "Timer"),
        (1, "STAT"),
        (0, "VBlank"),
    ]
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
