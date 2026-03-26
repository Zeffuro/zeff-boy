use crate::debug::{
    DebugUiActions, DisassemblyView, MemorySearchResult,
    PerfInfo, RomSearchResult, disassemble_around,
    CpuDebugSnapshot, ApuDebugInfo, OamDebugInfo, PaletteDebugInfo,
    RomDebugInfo, InputDebugInfo, ConsoleGraphicsData, GbGraphicsData,
};
use crate::debug::common::{
    DebugSection, WatchpointDisplay, WatchHitDisplay,
    ApuChannelDebug, RomInfoSection,
    PaletteGroupDebug, PaletteRowDebug,
};
use crate::emu_thread::SnapshotRequest;
use zeff_gb_core::emulator::Emulator;
use zeff_gb_core::hardware::types::hardware_mode::HardwareMode;
use zeff_gb_core::hardware::ppu::{PALETTE_COLORS, apply_palette, cgb_palette_rgba, correct_color};

pub(crate) struct UiFrameData {
    pub(crate) cpu_debug: Option<CpuDebugSnapshot>,
    pub(crate) perf_info: Option<PerfInfo>,
    pub(crate) apu_debug: Option<ApuDebugInfo>,
    pub(crate) oam_debug: Option<OamDebugInfo>,
    pub(crate) palette_debug: Option<PaletteDebugInfo>,
    pub(crate) rom_debug: Option<RomDebugInfo>,
    pub(crate) input_debug: Option<InputDebugInfo>,
    pub(crate) graphics_data: Option<ConsoleGraphicsData>,
    pub(crate) disassembly_view: Option<DisassemblyView>,
    pub(crate) memory_page: Option<Vec<(u16, u8)>>,
    pub(crate) memory_search_results: Option<Vec<MemorySearchResult>>,
    pub(crate) rom_page: Option<Vec<(u32, u8)>>,
    pub(crate) rom_size: u32,
    pub(crate) rom_search_results: Option<Vec<RomSearchResult>>,
}

pub(crate) fn empty_frame_data() -> UiFrameData {
    UiFrameData {
        cpu_debug: None,
        perf_info: None,
        apu_debug: None,
        oam_debug: None,
        palette_debug: None,
        rom_debug: None,
        input_debug: None,
        graphics_data: None,
        disassembly_view: None,
        memory_page: None,
        memory_search_results: None,
        rom_page: None,
        rom_size: 0,
        rom_search_results: None,
    }
}

fn gb_cpu_snapshot(info: &zeff_gb_core::debug::DebugInfo) -> CpuDebugSnapshot {
    let register_lines = vec![
        format!("A:{:02X}  F:{:02X}    AF:{:04X}", info.a, info.f, (info.a as u16) << 8 | info.f as u16),
        format!("B:{:02X}  C:{:02X}    BC:{:04X}", info.b, info.c, (info.b as u16) << 8 | info.c as u16),
        format!("D:{:02X}  E:{:02X}    DE:{:04X}", info.d, info.e, (info.d as u16) << 8 | info.e as u16),
        format!("H:{:02X}  L:{:02X}    HL:{:04X}", info.h, info.l, (info.h as u16) << 8 | info.l as u16),
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
    let mut int_lines = vec![
        format!("IF:{:02X}  IE:{:02X}  pending:{:02X}", info.if_reg, info.ie, info.if_reg & info.ie),
    ];
    let mut int_detail = String::new();
    for (i, name) in int_names.iter().enumerate() {
        let ie = if info.ie & (1 << i) != 0 { "E" } else { "." };
        let ifr = if info.if_reg & (1 << i) != 0 { "F" } else { "." };
        if !int_detail.is_empty() { int_detail.push_str("  "); }
        int_detail.push_str(&format!("{}:{}{}", name, ie, ifr));
    }
    int_lines.push(int_detail);

    let mode = info.ppu.stat & 0x03;
    let mode_name = match mode { 0 => "HBlank", 1 => "VBlank", 2 => "OAM Scan", 3 => "Drawing", _ => "?" };
    let ppu_lines = vec![
        format!("LY:{:02X}({:3})  LCDC:{:02X}  STAT:{:02X}", info.ppu.ly, info.ppu.ly, info.ppu.lcdc, info.ppu.stat),
        format!("Mode: {} ({})", mode, mode_name),
    ];

    let timer_lines = vec![
        format!("DIV:{:02X}  TIMA:{:02X}  TMA:{:02X}  TAC:{:02X}", info.div, info.tima, info.tma, info.tac),
        format!("Timer: {} @ {}", if info.tac & 0x04 != 0 { "ON" } else { "OFF" },
            match info.tac & 0x03 { 0 => "4096 Hz", 1 => "262144 Hz", 2 => "65536 Hz", 3 => "16384 Hz", _ => "?" }),
    ];

    let sections = vec![
        DebugSection { heading: "Interrupts".into(), lines: int_lines },
        DebugSection { heading: "PPU".into(), lines: ppu_lines },
        DebugSection { heading: "Timer".into(), lines: timer_lines },
    ];

    let mut recent_op_lines = Vec::new();
    let ops = &info.recent_ops;
    let mut i = 0;
    while i < ops.len() {
        let (pc, op, is_cb) = ops[i];
        let mut count = 1usize;
        while i + count < ops.len() && ops[i + count] == (pc, op, is_cb) { count += 1; }
        let line = if is_cb {
            if count > 1 { format!("{:04X}: CB {:02X} (x{})", pc, op, count) }
            else { format!("{:04X}: CB {:02X}", pc, op) }
        } else if count > 1 {
            format!("{:04X}: {:02X} (x{})", pc, op, count)
        } else {
            format!("{:04X}: {:02X}", pc, op)
        };
        recent_op_lines.push(line);
        i += count;
    }

    CpuDebugSnapshot {
        register_lines,
        flags,
        status_text,
        cpu_state: info.cpu_state.to_string(),
        pc: info.pc,
        cycles: info.cycles,
        last_opcode_line: format!("@ {:04X} = {:02X}", info.last_opcode_pc, info.last_opcode),
        sections,
        mem_around_pc: info.mem_around_pc.to_vec(),
        recent_op_lines,
        breakpoints: info.breakpoints.clone(),
        watchpoints: info.watchpoints.iter().map(|w| WatchpointDisplay {
            address: w.address,
            watch_type: match w.watch_type {
                zeff_gb_core::debug::WatchType::Read => crate::debug::WatchType::Read,
                zeff_gb_core::debug::WatchType::Write => crate::debug::WatchType::Write,
                zeff_gb_core::debug::WatchType::ReadWrite => crate::debug::WatchType::ReadWrite,
            },
        }).collect(),
        hit_breakpoint: info.hit_breakpoint,
        hit_watchpoint: info.hit_watchpoint.as_ref().map(|h| WatchHitDisplay {
            address: h.address,
            old_value: h.old_value,
            new_value: h.new_value,
            watch_type: match h.watch_type {
                zeff_gb_core::debug::WatchType::Read => crate::debug::WatchType::Read,
                zeff_gb_core::debug::WatchType::Write => crate::debug::WatchType::Write,
                zeff_gb_core::debug::WatchType::ReadWrite => crate::debug::WatchType::ReadWrite,
            },
        }),
    }
}

fn gb_input_snapshot(info: &zeff_gb_core::debug::DebugInfo) -> InputDebugInfo {
    let sections = vec![
        DebugSection {
            heading: "Input State".into(),
            lines: vec![
                format!("MBC7 active: {}", if info.tilt_is_mbc7 { "yes" } else { "no" }),
                format!("Left stick routes to: {}", if info.tilt_stick_controls_tilt { "tilt" } else { "d-pad" }),
            ],
        },
        DebugSection {
            heading: "Tilt Sources".into(),
            lines: vec![
                format!("Keyboard  x:{:>6.2} y:{:>6.2}", info.tilt_keyboard.0, info.tilt_keyboard.1),
                format!("Mouse     x:{:>6.2} y:{:>6.2}", info.tilt_mouse.0, info.tilt_mouse.1),
                format!("LeftStick x:{:>6.2} y:{:>6.2}", info.tilt_left_stick.0, info.tilt_left_stick.1),
            ],
        },
        DebugSection {
            heading: "Tilt Output".into(),
            lines: vec![
                format!("Target    x:{:>6.2} y:{:>6.2}", info.tilt_target.0, info.tilt_target.1),
                format!("Smoothed  x:{:>6.2} y:{:>6.2}", info.tilt_smoothed.0, info.tilt_smoothed.1),
            ],
        },
    ];
    let smoothed_x = ((info.tilt_smoothed.0 + 1.0) * 0.5).clamp(0.0, 1.0);
    let smoothed_y = ((info.tilt_smoothed.1 + 1.0) * 0.5).clamp(0.0, 1.0);
    InputDebugInfo {
        sections,
        progress_bars: vec![
            ("Smoothed X (-1..1)".into(), smoothed_x),
            ("Smoothed Y (-1..1)".into(), smoothed_y),
        ],
    }
}

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

    let int_lines = vec![
        format!("NMI pending: {}  IRQ line: {}", snap.nmi_pending, snap.irq_line),
    ];

    let ppu_lines = vec![
        format!("Scanline:{:3}  Dot:{:3}  Frame:{}", snap.ppu_scanline, snap.ppu_dot, snap.ppu_frame_count),
        format!("CTRL:{:02X}  MASK:{:02X}  STATUS:{:02X}", snap.ppu_ctrl, snap.ppu_mask, snap.ppu_status),
        format!("V:{:04X}  T:{:04X}  FineX:{}", snap.ppu_v, snap.ppu_t, snap.ppu_fine_x),
        format!("VBlank: {}", snap.ppu_in_vblank),
    ];

    let sections = vec![
        DebugSection { heading: "Interrupts".into(), lines: int_lines },
        DebugSection { heading: "PPU".into(), lines: ppu_lines },
    ];

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
        recent_op_lines: Vec::new(),
        breakpoints: Vec::new(),
        watchpoints: Vec::new(),
        hit_breakpoint: None,
        hit_watchpoint: None,
    }
}

pub(crate) fn nes_rom_info(emu: &zeff_nes_core::emulator::Emulator) -> RomDebugInfo {
    use crate::debug::common::RomInfoSection;

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
                ("Mapper".into(), format!("{} (sub {})", header.mapper_id, header.submapper_id)),
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
                ("Expansion Device".into(), format!("{}", header.default_expansion_device)),
            ],
        });
    } else {
        sections.push(RomInfoSection {
            heading: "RAM".into(),
            fields: vec![
                ("PRG-RAM".into(), format!("{} B", header.prg_ram_size)),
            ],
        });
    }

    RomDebugInfo { sections }
}

fn gb_apu_snapshot(emu: &Emulator, show: bool) -> Option<ApuDebugInfo> {
    if !show { return None; }
    use zeff_gb_core::hardware::types::constants::*;

    let regs = emu.bus.apu_regs_snapshot();
    let wave_ram = emu.bus.apu_wave_ram_snapshot();
    let nr52 = emu.bus.apu_nr52_raw();
    let channel_samples = [
        emu.bus.apu_channel_debug_samples_ordered(0),
        emu.bus.apu_channel_debug_samples_ordered(1),
        emu.bus.apu_channel_debug_samples_ordered(2),
        emu.bus.apu_channel_debug_samples_ordered(3),
    ];
    let master_samples = emu.bus.apu_master_debug_samples_ordered();
    let muted = emu.bus.apu_channel_mutes();

    let ri = |addr: u16| (addr - NR10) as usize;
    let duty = |val: u8| match (val >> 6) & 0x03 { 0 => "12.5%", 1 => "25%", 2 => "50%", 3 => "75%", _ => "?" };

    let master_lines = vec![
        format!("NR50:{:02X}  NR51:{:02X}  NR52:{:02X}", regs[ri(NR50)], regs[ri(NR51)], nr52),
        format!("Power:{}  CH1:{} CH2:{} CH3:{} CH4:{}",
            if nr52 & 0x80 != 0 { "ON" } else { "OFF" },
            if nr52 & 0x01 != 0 { "1" } else { "-" },
            if nr52 & 0x02 != 0 { "1" } else { "-" },
            if nr52 & 0x04 != 0 { "1" } else { "-" },
            if nr52 & 0x08 != 0 { "1" } else { "-" },
        ),
    ];

    let channels = vec![
        ApuChannelDebug {
            name: "CH1 (Square + Sweep)".into(),
            enabled: nr52 & 0x01 != 0,
            muted: muted[0],
            register_lines: vec![format!("NR10:{:02X} NR11:{:02X} NR12:{:02X} NR13:{:02X} NR14:{:02X}",
                regs[ri(NR10)], regs[ri(NR11)], regs[ri(NR12)], regs[ri(NR13)], regs[ri(NR14)])],
            detail_line: format!("Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X}",
                duty(regs[ri(NR11)]), regs[ri(NR11)] & 0x3F, regs[ri(NR12)] >> 4,
                if regs[ri(NR12)] & 0x08 != 0 { "+" } else { "-" }, regs[ri(NR12)] & 0x07,
                (u16::from(regs[ri(NR14)] & 0x07) << 8) | u16::from(regs[ri(NR13)])),
            waveform: channel_samples[0].to_vec(),
        },
        ApuChannelDebug {
            name: "CH2 (Square)".into(),
            enabled: nr52 & 0x02 != 0,
            muted: muted[1],
            register_lines: vec![format!("NR21:{:02X} NR22:{:02X} NR23:{:02X} NR24:{:02X}",
                regs[ri(NR21)], regs[ri(NR22)], regs[ri(NR23)], regs[ri(NR24)])],
            detail_line: format!("Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X}",
                duty(regs[ri(NR21)]), regs[ri(NR21)] & 0x3F, regs[ri(NR22)] >> 4,
                if regs[ri(NR22)] & 0x08 != 0 { "+" } else { "-" }, regs[ri(NR22)] & 0x07,
                (u16::from(regs[ri(NR24)] & 0x07) << 8) | u16::from(regs[ri(NR23)])),
            waveform: channel_samples[1].to_vec(),
        },
        ApuChannelDebug {
            name: "CH3 (Wave)".into(),
            enabled: nr52 & 0x04 != 0,
            muted: muted[2],
            register_lines: vec![format!("NR30:{:02X} NR31:{:02X} NR32:{:02X} NR33:{:02X} NR34:{:02X}",
                regs[ri(NR30)], regs[ri(NR31)], regs[ri(NR32)], regs[ri(NR33)], regs[ri(NR34)])],
            detail_line: format!("DAC:{} Len:{} Level:{} Freq:{:03X}",
                if regs[ri(NR30)] & 0x80 != 0 { "ON" } else { "OFF" },
                regs[ri(NR31)], (regs[ri(NR32)] >> 5) & 0x03,
                (u16::from(regs[ri(NR34)] & 0x07) << 8) | u16::from(regs[ri(NR33)])),
            waveform: channel_samples[2].to_vec(),
        },
        ApuChannelDebug {
            name: "CH4 (Noise)".into(),
            enabled: nr52 & 0x08 != 0,
            muted: muted[3],
            register_lines: vec![format!("NR41:{:02X} NR42:{:02X} NR43:{:02X} NR44:{:02X}",
                regs[ri(NR41)], regs[ri(NR42)], regs[ri(NR43)], regs[ri(NR44)])],
            detail_line: format!("Len:{} Vol:{} Env:{} P:{} Poly(s={},w={},r={})",
                regs[ri(NR41)] & 0x3F, regs[ri(NR42)] >> 4,
                if regs[ri(NR42)] & 0x08 != 0 { "+" } else { "-" }, regs[ri(NR42)] & 0x07,
                regs[ri(NR43)] >> 4,
                if regs[ri(NR43)] & 0x08 != 0 { "7" } else { "15" }, regs[ri(NR43)] & 0x07),
            waveform: channel_samples[3].to_vec(),
        },
    ];

    let mut wave_lines = Vec::new();
    for row in 0..4usize {
        let mut line = String::new();
        for col in 0..4usize {
            let idx = row * 4 + col;
            if !line.is_empty() { line.push(' '); }
            line.push_str(&format!("{:02X}", wave_ram[idx]));
        }
        wave_lines.push(line);
    }

    Some(ApuDebugInfo {
        master_lines,
        master_waveform: master_samples.to_vec(),
        channels,
        extra_sections: vec![DebugSection { heading: "Wave RAM".into(), lines: wave_lines }],
    })
}

fn gb_oam_snapshot(emu: &Emulator, show: bool, reusable_oam: Option<Vec<u8>>) -> (Option<OamDebugInfo>, Option<Vec<u8>>) {
    if !show {
        return (None, reusable_oam.map(|mut v| { v.clear(); v }));
    }
    use zeff_gb_core::hardware::ppu::SpriteEntry;
    let src = emu.oam();
    let mut buf = reusable_oam.unwrap_or_default();
    buf.resize(src.len(), 0);
    buf.copy_from_slice(src);

    let headers = vec!["#","X","Y","Tile","Flags","FlipX","FlipY","Prio","Pal","CGB Pal","VRAM"]
        .into_iter().map(String::from).collect();
    let mut rows = Vec::with_capacity(40);
    for i in 0..40usize {
        let sprite = SpriteEntry::from_oam(&buf, i);
        rows.push(vec![
            format!("{:02}", i),
            format!("{:4}", sprite.x),
            format!("{:4}", sprite.y),
            format!("{:02X}", sprite.tile),
            format!("{:02X}", sprite.flags),
            (if sprite.flip_x() { "Y" } else { "N" }).to_string(),
            (if sprite.flip_y() { "Y" } else { "N" }).to_string(),
            (if sprite.bg_priority() { "BG" } else { "FG" }).to_string(),
            format!("{}", sprite.palette_number()),
            format!("{}", sprite.cgb_obj_palette_index()),
            format!("{}", sprite.cgb_vram_bank()),
        ]);
    }
    (Some(OamDebugInfo { headers, rows }), Some(buf))
}

fn gb_palette_snapshot(emu: &Emulator, show: bool, req: &SnapshotRequest) -> Option<PaletteDebugInfo> {
    if !show { return None; }
    let ppu = emu.ppu_registers();
    let cgb_mode = matches!(emu.hardware_mode, HardwareMode::CGBNormal | HardwareMode::CGBDouble);
    let bg_pal = emu.bus.ppu_bg_palette_ram_snapshot();
    let obj_pal = emu.bus.ppu_obj_palette_ram_snapshot();

    let mut groups = Vec::new();

    let dmg_row = |label: &str, val: u8| -> PaletteRowDebug {
        let colors = (0..4u8).map(|cid| apply_palette(val, cid)).collect();
        PaletteRowDebug { label: format!("{} ({:02X})", label, val), colors }
    };
    groups.push(PaletteGroupDebug {
        title: "DMG Palettes".into(),
        rows: vec![dmg_row("BGP", ppu.bgp), dmg_row("OBP0", ppu.obp0), dmg_row("OBP1", ppu.obp1)],
    });

    groups.push(PaletteGroupDebug {
        title: "Base DMG shades".into(),
        rows: vec![PaletteRowDebug { label: String::new(), colors: PALETTE_COLORS.to_vec() }],
    });

    if cgb_mode {
        let cc = req.color_correction;
        let ccm = req.color_correction_matrix;
        let cgb_group = |title: &str, prefix: &str, ram: &[u8; 64]| -> PaletteGroupDebug {
            let rows = (0u8..8).map(|pal| {
                let colors = (0u8..4).map(|cid| correct_color(cgb_palette_rgba(ram, pal, cid), cc, ccm)).collect();
                PaletteRowDebug { label: format!("{}{}", prefix, pal), colors }
            }).collect();
            PaletteGroupDebug { title: title.into(), rows }
        };
        groups.push(cgb_group("CGB BG palettes", "BG", &bg_pal));
        groups.push(cgb_group("CGB OBJ palettes", "OB", &obj_pal));
    }

    Some(PaletteDebugInfo { groups })
}

fn gb_rom_info(emu: &Emulator) -> RomDebugInfo {
    let header = emu.rom_info();
    let rom_bytes = emu.bus.cartridge.rom_bytes();
    let rom_crc32 = crc32fast::hash(rom_bytes);
    let is_gbc = header.is_cgb_compatible || header.is_cgb_exclusive;
    let libretro_meta = crate::libretro_metadata::lookup_cached(rom_crc32, is_gbc);
    let manufacturer = header.manufacturer_code.as_deref().unwrap_or("N/A").to_string();

    let yes_no = |v: bool| if v { "Yes" } else { "No" };
    let pass_fail = |v: bool| if v { "Valid" } else { "Invalid" };
    let cart_state = emu.cartridge_state();

    let mut sections = vec![
        RomInfoSection {
            heading: "Header".into(),
            fields: vec![
                ("Title".into(), header.title.clone()),
                ("Manufacturer".into(), manufacturer),
                ("Publisher".into(), header.publisher().to_string()),
                ("Cartridge".into(), format!("{:?}", header.cartridge_type)),
                ("ROM Size".into(), format!("{:?}", header.rom_size)),
                ("RAM Size".into(), format!("{:?}", header.ram_size)),
            ],
        },
        RomInfoSection {
            heading: "Compatibility".into(),
            fields: vec![
                ("Hardware Mode".into(), format!("{:?}", emu.hardware_mode)),
                ("CGB Flag".into(), format!("{:02X}", header.cgb_flag)),
                ("SGB Flag".into(), format!("{:02X}", header.sgb_flag)),
                ("CGB Compatible".into(), yes_no(header.is_cgb_compatible).into()),
                ("CGB Exclusive".into(), yes_no(header.is_cgb_exclusive).into()),
                ("SGB Supported".into(), yes_no(header.is_sgb_supported).into()),
            ],
        },
        RomInfoSection {
            heading: "Checksums".into(),
            fields: vec![
                ("Header".into(), pass_fail(header.verify_header_checksum(rom_bytes)).into()),
                ("Global".into(), pass_fail(header.verify_global_checksum(rom_bytes)).into()),
                ("CRC32".into(), format!("{:08X}", rom_crc32)),
            ],
        },
    ];

    let libretro_fields = match &libretro_meta {
        Some(meta) => vec![
            ("Title".into(), meta.title.clone()),
            ("ROM File".into(), meta.rom_name.clone()),
        ],
        None => vec![("Status".into(), "No local metadata match".into())],
    };
    sections.push(RomInfoSection { heading: "libretro Metadata".into(), fields: libretro_fields });

    let mut cart_fields: Vec<(String, String)> = vec![
        ("Mapper".into(), cart_state.mapper.to_string()),
        ("ROM Bank".into(), format!("{}", cart_state.active_rom_bank)),
        ("RAM Bank".into(), format!("{}", cart_state.active_ram_bank)),
        ("RAM Enabled".into(), yes_no(cart_state.ram_enabled).into()),
    ];
    if let Some(mode) = cart_state.banking_mode {
        cart_fields.push(("Banking Mode".into(), if mode { "RAM" } else { "ROM" }.into()));
    }
    sections.push(RomInfoSection { heading: "Cartridge State".into(), fields: cart_fields });

    RomDebugInfo { sections }
}

pub(crate) fn collect_emu_snapshot(
    emu: &Emulator,
    req: &SnapshotRequest,
    reusable_vram: Option<Vec<u8>>,
    reusable_oam: Option<Vec<u8>>,
    reusable_memory_page: Option<Vec<(u16, u8)>>,
) -> UiFrameData {
    let gb_info = if req.want_debug_info {
        Some(emu.snapshot())
    } else {
        None
    };

    let cpu_debug = gb_info.as_ref().map(|info| gb_cpu_snapshot(info));

    let input_debug = gb_info.as_ref().map(|info| gb_input_snapshot(info));

    let apu_debug = gb_apu_snapshot(emu, req.show_apu_viewer);

    let (oam_debug, _reusable_oam) = gb_oam_snapshot(emu, req.show_oam_viewer, reusable_oam);

    let palette_debug = gb_palette_snapshot(emu, req.any_viewer_open, req);

    let graphics_data = if req.any_vram_viewer_open {
        let ppu = emu.ppu_registers();
        let cgb_mode = matches!(emu.hardware_mode, HardwareMode::CGBNormal | HardwareMode::CGBDouble);
        let src = emu.vram();
        let mut vram_buf = reusable_vram.unwrap_or_default();
        vram_buf.resize(src.len(), 0);
        vram_buf.copy_from_slice(src);
        Some(ConsoleGraphicsData::Gb(GbGraphicsData {
            vram: vram_buf,
            ppu,
            cgb_mode,
            bg_palette_ram: emu.bus.ppu_bg_palette_ram_snapshot(),
            obj_palette_ram: emu.bus.ppu_obj_palette_ram_snapshot(),
            color_correction: req.color_correction,
            color_correction_matrix: req.color_correction_matrix,
        }))
    } else {
        None
    };

    let disassembly_view = if req.show_disassembler {
        let pc_changed = req.last_disasm_pc != Some(emu.cpu.pc);
        if pc_changed {
            let mut breakpoints: Vec<u16> = emu.debug.iter_breakpoints().collect();
            breakpoints.sort_unstable();
            Some(DisassemblyView {
                pc: emu.cpu.pc,
                lines: disassemble_around(|addr| emu.bus.read_byte(addr), emu.cpu.pc, 12, 26),
                breakpoints,
            })
        } else {
            None
        }
    } else {
        None
    };

    let rom_debug = if req.show_rom_info { Some(gb_rom_info(emu)) } else { None };

    let memory_page = if req.show_memory_viewer {
        let mut buf = reusable_memory_page.unwrap_or_else(|| Vec::with_capacity(256));
        buf.clear();
        let start = req.memory_view_start;
        for i in 0..256u16 {
            let addr = start.wrapping_add(i);
            buf.push((addr, emu.bus.read_byte(addr)));
        }
        Some(buf)
    } else {
        reusable_memory_page.map(|mut v| { v.clear(); v })
    };

    let memory_search_results = if let Some(ref search) = req.memory_search {
        let mut results = Vec::new();
        if !search.pattern.is_empty() {
            let mut flat = vec![0u8; 0x10000];
            for addr in 0u32..=0xFFFFu32 {
                flat[addr as usize] = emu.bus.read_byte_raw(addr as u16);
            }
            let pattern_len = search.pattern.len();
            for start_addr in 0..=(0x10000usize - pattern_len) {
                if results.len() >= search.max_results { break; }
                if flat[start_addr..start_addr + pattern_len] == search.pattern[..] {
                    results.push(MemorySearchResult {
                        address: start_addr as u16,
                        matched_bytes: flat[start_addr..start_addr + pattern_len].to_vec(),
                    });
                }
            }
        }
        Some(results)
    } else {
        None
    };

    let rom_bytes = emu.bus.cartridge.rom_bytes();
    let rom_size = rom_bytes.len() as u32;

    let rom_page = if req.show_rom_viewer {
        let start = req.rom_view_start as usize;
        let mut buf = Vec::with_capacity(256);
        for i in 0..256usize {
            let offset = start + i;
            if offset < rom_bytes.len() { buf.push((offset as u32, rom_bytes[offset])); }
        }
        Some(buf)
    } else {
        None
    };

    let rom_search_results = if let Some(ref search) = req.rom_search {
        let mut results = Vec::new();
        if !search.pattern.is_empty() {
            let pattern_len = search.pattern.len();
            let end = rom_bytes.len().saturating_sub(pattern_len.saturating_sub(1));
            for start_offset in 0..end {
                if results.len() >= search.max_results { break; }
                if rom_bytes[start_offset..start_offset + pattern_len] == search.pattern[..] {
                    results.push(RomSearchResult {
                        offset: start_offset as u32,
                        matched_bytes: rom_bytes[start_offset..start_offset + pattern_len].to_vec(),
                    });
                }
            }
        }
        Some(results)
    } else {
        None
    };

    let perf_info = gb_info.as_ref().map(|di| PerfInfo {
        fps: di.fps,
        speed_mode_label: di.speed_mode_label.to_string(),
        frames_in_flight: di.frames_in_flight,
        cycles: di.cycles,
        platform_name: "Game Boy",
        hardware_label: format!("{:?}", di.hardware_mode),
        hardware_pref_label: format!("{:?}", di.hardware_mode_preference),
    });

    UiFrameData {
        cpu_debug,
        perf_info,
        apu_debug,
        oam_debug,
        palette_debug,
        rom_debug,
        input_debug,
        graphics_data,
        disassembly_view,
        memory_page,
        memory_search_results,
        rom_page,
        rom_size,
        rom_search_results,
    }
}

pub(crate) fn apply_debug_actions(
    actions: &DebugUiActions,
    debug_step_requested: &mut bool,
    debug_continue_requested: &mut bool,
    backstep_requested: &mut bool,
) {
    if actions.step_requested { *debug_step_requested = true; }
    if actions.continue_requested { *debug_continue_requested = true; }
    if actions.backstep_requested { *backstep_requested = true; }
}
