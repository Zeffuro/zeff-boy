use crate::debug::common::{ApuChannelDebug, DebugSection, RomInfoSection, WatchHitDisplay, WatchpointDisplay};
use crate::debug::{ApuDebugInfo, CpuDebugSnapshot, DisassemblyView, RomDebugInfo, nes_disassemble_around};

pub(crate) fn nes_cpu_snapshot(emu: &zeff_nes_core::emulator::Emulator) -> CpuDebugSnapshot {
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
        format!("V:{:04X}  T:{:04X}  FineX:{}", snap.ppu_v, snap.ppu_t, snap.ppu_fine_x),
        format!("VBlank: {}", snap.ppu_in_vblank),
    ];

    let sections = vec![
        DebugSection {
            heading: "Interrupts".into(),
            lines: int_lines,
        },
        DebugSection {
            heading: "PPU".into(),
            lines: ppu_lines,
        },
    ];

    let mut recent_op_lines = Vec::new();
    let ops = &snap.recent_ops;
    let mut i = 0;
    while i < ops.len() {
        let (pc, op) = ops[i];
        let mut count = 1usize;
        while i + count < ops.len() && ops[i + count] == (pc, op) {
            count += 1;
        }
        let line = if count > 1 {
            format!("{:04X}: {:02X} (x{})", pc, op, count)
        } else {
            format!("{:04X}: {:02X}", pc, op)
        };
        recent_op_lines.push(line);
        i += count;
    }

    let breakpoints: Vec<u16> = emu.debug.iter_breakpoints().collect();
    let watchpoints: Vec<WatchpointDisplay> = emu.debug.watchpoints
        .iter()
        .map(|w| WatchpointDisplay {
            address: w.address,
            watch_type: match w.watch_type {
                zeff_nes_core::debug::WatchType::Read => crate::debug::WatchType::Read,
                zeff_nes_core::debug::WatchType::Write => crate::debug::WatchType::Write,
                zeff_nes_core::debug::WatchType::ReadWrite => crate::debug::WatchType::ReadWrite,
            },
        })
        .collect();
    let hit_breakpoint = emu.debug.hit_breakpoint;
    let hit_watchpoint = emu.debug.hit_watchpoint.as_ref().map(|h| WatchHitDisplay {
        address: h.address,
        old_value: h.old_value,
        new_value: h.new_value,
        watch_type: match h.watch_type {
            zeff_nes_core::debug::WatchType::Read => crate::debug::WatchType::Read,
            zeff_nes_core::debug::WatchType::Write => crate::debug::WatchType::Write,
            zeff_nes_core::debug::WatchType::ReadWrite => crate::debug::WatchType::ReadWrite,
        },
    });

    CpuDebugSnapshot {
        register_lines,
        flags,
        status_text,
        cpu_state: snap.cpu_state.to_string(),
        pc: snap.pc,
        cycles: snap.cycles,
        last_opcode_line: format!("@ {:04X} = {:02X}", snap.last_opcode_pc, snap.last_opcode),
        sections,
        mem_around_pc: snap.mem_around_pc.to_vec(),
        recent_op_lines,
        breakpoints,
        watchpoints,
        hit_breakpoint,
        hit_watchpoint,
    }
}

pub(crate) fn nes_rom_info(emu: &zeff_nes_core::emulator::Emulator) -> RomDebugInfo {
    let header = emu.bus.cartridge.header();
    let yes_no = |v: bool| if v { "Yes" } else { "No" };

    let chr_label = if header.chr_rom_size > 0 {
        format!("{} KiB", header.chr_rom_size / 1024)
    } else {
        "0 (CHR-RAM)".into()
    };

    let mut sections = vec![
        RomInfoSection {
            heading: "ROM Header".into(),
            fields: vec![
                ("Format".into(), format!("{:?}", header.format)),
                ("PRG ROM".into(), format!("{} KiB", header.prg_rom_size / 1024)),
                ("CHR ROM".into(), chr_label),
                (
                    "Mapper".into(),
                    header.mapper_label(),
                ),
                ("Mirroring".into(), format!("{:?}", header.mirroring)),
                ("Battery".into(), yes_no(header.has_battery).into()),
                ("Trainer".into(), yes_no(header.has_trainer).into()),
            ],
        },
        RomInfoSection {
            heading: "System".into(),
            fields: vec![
                ("Console".into(), format!("{:?}", header.console_type)),
                ("Timing".into(), format!("{:?}", header.timing)),
            ],
        },
    ];

    if header.format == zeff_nes_core::hardware::cartridge::RomFormat::Nes2 {
        sections.push(RomInfoSection {
            heading: "NES 2.0 Extended".into(),
            fields: vec![
                ("PRG-RAM".into(), format!("{} B", header.prg_ram_size)),
                ("PRG-NVRAM".into(), format!("{} B", header.prg_nvram_size)),
                ("CHR-RAM".into(), format!("{} B", header.chr_ram_size)),
                ("CHR-NVRAM".into(), format!("{} B", header.chr_nvram_size)),
                ("Misc ROMs".into(), format!("{}", header.misc_roms)),
                (
                    "Expansion Device".into(),
                    format!("{}", header.default_expansion_device),
                ),
            ],
        });
    } else {
        sections.push(RomInfoSection {
            heading: "RAM".into(),
            fields: vec![("PRG-RAM".into(), format!("{} B", header.prg_ram_size))],
        });
    }

    RomDebugInfo { sections }
}

pub(crate) fn nes_apu_snapshot(
    emu: &zeff_nes_core::emulator::Emulator,
    show: bool,
) -> Option<ApuDebugInfo> {
    if !show {
        return None;
    }

    let apu = &emu.bus.apu;
    let muted = apu.channel_mutes();
    let master_lines = vec![
        format!(
            "Frame mode:{}  IRQ inhibit:{}  Frame IRQ:{}",
            if apu.five_step_mode { "5-step" } else { "4-step" },
            apu.irq_inhibit,
            apu.frame_irq
        ),
        format!(
            "Sample rate:{:.1} Hz  Buffered samples:{}",
            apu.output_sample_rate,
            apu.sample_buffer.len()
        ),
    ];

    let channels = vec![
        ApuChannelDebug {
            name: "Pulse 1".into(),
            enabled: apu.pulse1.midi_active(),
            muted: muted[0],
            register_lines: vec![format!(
                "Len:{} Timer:{:03X} Vol:{}",
                apu.pulse1.length_counter,
                apu.pulse1.timer_period(),
                apu.pulse1.midi_volume()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(0),
        },
        ApuChannelDebug {
            name: "Pulse 2".into(),
            enabled: apu.pulse2.midi_active(),
            muted: muted[1],
            register_lines: vec![format!(
                "Len:{} Timer:{:03X} Vol:{}",
                apu.pulse2.length_counter,
                apu.pulse2.timer_period(),
                apu.pulse2.midi_volume()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(1),
        },
        ApuChannelDebug {
            name: "Triangle".into(),
            enabled: apu.triangle.midi_active(),
            muted: muted[2],
            register_lines: vec![format!(
                "Len:{} Timer:{:03X}",
                apu.triangle.length_counter,
                apu.triangle.timer_period()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(2),
        },
        ApuChannelDebug {
            name: "Noise".into(),
            enabled: apu.noise.midi_active(),
            muted: muted[3],
            register_lines: vec![format!(
                "Len:{} Vol:{}",
                apu.noise.length_counter,
                apu.noise.midi_volume()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(3),
        },
    ];

    Some(ApuDebugInfo {
        master_lines,
        master_waveform: apu.master_debug_samples_ordered(),
        channels,
        extra_sections: Vec::new(),
    })
}

pub(crate) fn nes_disassembly_view(emu: &zeff_nes_core::emulator::Emulator) -> DisassemblyView {
    DisassemblyView {
        pc: emu.cpu.pc,
        lines: nes_disassemble_around(
            |addr| nes_disasm_peek_byte(&emu.bus, addr),
            emu.cpu.pc,
            12,
            26,
        ),
        breakpoints: Vec::new(),
    }
}

fn nes_disasm_peek_byte(bus: &zeff_nes_core::hardware::bus::Bus, addr: u16) -> u8 {
    match addr {
        0x0000..=0x1FFF => bus.ram[(addr & 0x07FF) as usize],
        0x4020..=0xFFFF => bus.cartridge.cpu_read(addr),
        _ => 0,
    }
}

