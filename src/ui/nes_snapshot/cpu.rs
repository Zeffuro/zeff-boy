use crate::debug::{CpuDebugSnapshot, DebugSection, WatchHitDisplay, WatchpointDisplay};

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

    let mut recent_op_lines = Vec::new();
    let ops = &snap.recent_ops;
    let mut seen: Vec<((u16, u8), usize)> = Vec::new();
    for &(pc, op) in ops {
        if let Some(entry) = seen.iter_mut().find(|e| e.0 == (pc, op)) {
            entry.1 += 1;
        } else {
            seen.push(((pc, op), 1));
        }
    }
    for ((pc, op), count) in seen.into_iter().take(16) {
        let line = if count > 1 {
            format!("{:04X}: {:02X} (x{})", pc, op, count)
        } else {
            format!("{:04X}: {:02X}", pc, op)
        };
        recent_op_lines.push(line);
    }

    let breakpoints: Vec<u16> = emu.iter_breakpoints().collect();
    let watchpoints: Vec<WatchpointDisplay> = emu
        .debug_watchpoints()
        .iter()
        .map(|w| WatchpointDisplay {
            address: w.address,
            watch_type: w.watch_type,
        })
        .collect();
    let hit_breakpoint = emu.debug_hit_breakpoint();
    let hit_watchpoint = emu.debug_hit_watchpoint().map(|h| WatchHitDisplay {
        address: h.address,
        old_value: h.old_value,
        new_value: h.new_value,
        watch_type: h.watch_type,
    });

    CpuDebugSnapshot {
        register_lines,
        flags,
        status_text,
        cpu_state: snap.cpu_state.to_string(),
        cycles: snap.cycles,
        last_opcode_line: format!("@ {:04X} = {:02X}", snap.last_opcode_pc, snap.last_opcode),
        sections,
        mem_around_pc: snap.mem_around_pc,
        recent_op_lines,
        breakpoints,
        watchpoints,
        hit_breakpoint,
        hit_watchpoint,
    }
}
