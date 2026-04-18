use crate::debug::{CpuDebugSnapshot, DebugSection, WatchHitDisplay, WatchpointDisplay};

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

    let mut recent_op_lines = Vec::new();
    let ops = &info.recent_ops;
    let mut seen: Vec<((u16, u8, bool), usize)> = Vec::new();
    for &entry in ops {
        if let Some(slot) = seen.iter_mut().find(|e| e.0 == entry) {
            slot.1 += 1;
        } else {
            seen.push((entry, 1));
        }
    }
    for ((pc, op, is_cb), count) in seen.into_iter().take(16) {
        let line = if is_cb {
            if count > 1 {
                format!("{:04X}: CB {:02X} (x{})", pc, op, count)
            } else {
                format!("{:04X}: CB {:02X}", pc, op)
            }
        } else if count > 1 {
            format!("{:04X}: {:02X} (x{})", pc, op, count)
        } else {
            format!("{:04X}: {:02X}", pc, op)
        };
        recent_op_lines.push(line);
    }

    CpuDebugSnapshot {
        register_lines,
        flags,
        status_text,
        cpu_state: info.cpu_state.to_string(),
        cycles: info.cycles,
        last_opcode_line: format!("@ {:04X} = {:02X}", info.last_opcode_pc, info.last_opcode),
        sections,
        mem_around_pc: info.mem_around_pc,
        recent_op_lines,
        breakpoints: info.breakpoints.clone(),
        watchpoints: info
            .watchpoints
            .iter()
            .map(|w| WatchpointDisplay {
                address: w.address,
                watch_type: w.watch_type,
            })
            .collect(),
        hit_breakpoint: info.hit_breakpoint,
        hit_watchpoint: info.hit_watchpoint.as_ref().map(|h| WatchHitDisplay {
            address: h.address,
            old_value: h.old_value,
            new_value: h.new_value,
            watch_type: h.watch_type,
        }),
    }
}
