use crate::emu_thread::{ReusableBuffers, SnapshotRequest};
use zeff_emu_common::address::Address;

const GBA_BASE_SEARCH_RANGES: &[(u32, u32)] = &[
    (0x0200_0000, 0x0203_FFFF),
    (0x0300_0000, 0x0300_7FFF),
    (0x0400_0000, 0x0400_03FF),
    (0x0500_0000, 0x0500_03FF),
    (0x0600_0000, 0x0601_7FFF),
    (0x0700_0000, 0x0700_03FF),
];

pub(crate) fn collect_gba_snapshot(
    emu: &zeff_gba_core::emulator::Emulator,
    snapshot: &SnapshotRequest,
    mut buffers: ReusableBuffers,
) -> super::UiFrameData {
    let rom_bytes = emu.cartridge_rom_bytes();
    let mut search_ranges = GBA_BASE_SEARCH_RANGES.to_vec();
    if !rom_bytes.is_empty() {
        let rom_end = 0x0800_0000u32.saturating_add(rom_bytes.len() as u32 - 1);
        search_ranges.push((0x0800_0000, rom_end.min(0x09FF_FFFF)));
    }
    if emu.has_battery() {
        search_ranges.push((0x0E00_0000, 0x0E00_FFFF));
    }
    let mut data = super::UiFrameData {
        rom_page: super::build_rom_page(
            snapshot.show_rom_viewer,
            snapshot.rom_view_start,
            rom_bytes,
        ),
        rom_size: rom_bytes.len() as u32,
        rom_search_results: super::build_rom_search(snapshot.rom_search.as_ref(), rom_bytes),
        memory_page: super::build_memory_page(
            snapshot.show_memory_viewer,
            snapshot.memory_view_start,
            buffers.memory_page,
            |addr| emu.cpu_peek8(addr),
        ),
        memory_search_results: super::build_memory_search_ranges(
            snapshot.memory_search.as_ref(),
            &search_ranges,
            |addr| emu.cpu_peek8(addr),
        ),
        ..Default::default()
    };

    if snapshot.show_rom_info {
        data.rom_debug = Some(crate::debug::RomDebugInfo {
            sections: vec![
                crate::debug::RomInfoSection {
                    heading: "GBA Header",
                    fields: vec![
                        ("Title", emu.cartridge_header().title.clone()),
                        ("Game Code", emu.cartridge_header().game_code.clone()),
                        ("Maker Code", emu.cartridge_header().maker_code.clone()),
                        ("Backup", format!("{:?}", emu.backup_kind())),
                    ],
                },
                crate::debug::RomInfoSection {
                    heading: "Core",
                    fields: vec![(
                        "Status",
                        "Experimental ARM/Thumb interpreter; bitmap video modes enabled".into(),
                    )],
                },
            ],
        });
    }

    if snapshot.want_debug_info {
        data.cpu_debug = Some(gba_cpu_snapshot(emu));
    }

    if snapshot.any_vram_viewer_open {
        let mut vram = buffers.vram.take().unwrap_or_default();
        let src = emu.vram_snapshot();
        vram.resize(src.len(), 0);
        vram.copy_from_slice(src);

        data.graphics_data = Some(crate::debug::ConsoleGraphicsData::Gba(
            crate::debug::GbaGraphicsData {
                vram,
                palette_ram: emu.palette_ram_snapshot().to_vec(),
                oam: emu.oam_snapshot().to_vec(),
                ppu: emu.ppu_debug_snapshot(),
            },
        ));
    }

    if snapshot.show_oam_viewer {
        data.oam_debug = Some(gba_oam_snapshot(emu));
    }

    if snapshot.any_viewer_open {
        data.palette_debug = Some(gba_palette_snapshot(emu));
    }

    if snapshot.want_perf_info {
        data.perf_info = Some(crate::debug::PerfInfo {
            fps: 0.0,
            speed_mode_label: "1×",
            frames_in_flight: 0,
            cycles: emu.cpu_cycles(),
            platform_name: "GBA",
            hardware_label: "Game Boy Advance".into(),
            hardware_pref_label: "Auto".into(),
        });
    }

    data
}

fn gba_oam_snapshot(emu: &zeff_gba_core::emulator::Emulator) -> crate::debug::OamDebugInfo {
    let oam = emu.oam_snapshot();
    let rows = (0..128usize)
        .filter_map(|i| {
            let base = i * 8;
            let attr0 = read_le16(oam, base);
            let attr1 = read_le16(oam, base + 2);
            let attr2 = read_le16(oam, base + 4);
            let disabled = attr0 & 0x0300 == 0x0200;
            if disabled && i >= 32 {
                return None;
            }
            Some(vec![
                format!("{i:03}"),
                format!("{attr0:04X}"),
                format!("{attr1:04X}"),
                format!("{attr2:04X}"),
                format!("x={} y={}", attr1 & 0x01FF, attr0 & 0x00FF),
                format!("tile={:03X}", attr2 & 0x03FF),
                format!("pal={}", (attr2 >> 12) & 0xF),
                if disabled {
                    "disabled".into()
                } else {
                    "on".into()
                },
            ])
        })
        .collect();
    crate::debug::OamDebugInfo {
        headers: &[
            "#", "Attr0", "Attr1", "Attr2", "Pos", "Tile", "Pal", "State",
        ],
        rows,
    }
}

fn gba_palette_snapshot(emu: &zeff_gba_core::emulator::Emulator) -> crate::debug::PaletteDebugInfo {
    let palette_ram = emu.palette_ram_snapshot();
    let groups = [("BG palettes", 0usize), ("OBJ palettes", 0x100usize)]
        .into_iter()
        .map(|(title, base)| crate::debug::PaletteGroupDebug {
            title: title.into(),
            rows: (0..16usize)
                .map(|pal| crate::debug::PaletteRowDebug {
                    label: format!("{pal:02}"),
                    colors: (0..16usize)
                        .map(|color| gba_palette_rgba(palette_ram, base + pal * 16 + color))
                        .collect(),
                })
                .collect(),
        })
        .collect();
    crate::debug::PaletteDebugInfo { groups }
}

fn read_le16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn gba_palette_rgba(palette_ram: &[u8], index: usize) -> [u8; 4] {
    let color = read_le16(palette_ram, index * 2);
    let r = (color & 0x1F) as u8;
    let g = ((color >> 5) & 0x1F) as u8;
    let b = ((color >> 10) & 0x1F) as u8;
    [expand5(r), expand5(g), expand5(b), 255]
}

fn expand5(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}

fn gba_cpu_snapshot(emu: &zeff_gba_core::emulator::Emulator) -> crate::debug::CpuDebugSnapshot {
    let regs = emu.cpu_registers();
    let cpsr = emu.cpu_cpsr();
    let pc = emu.cpu_pc();
    let ppu = emu.ppu_debug_snapshot();
    let mem_around_pc: [(Address, u8); 32] = std::array::from_fn(|i| {
        let addr = pc.wrapping_add(i as u32);
        (addr, emu.cpu_peek8(addr))
    });

    crate::debug::CpuDebugSnapshot {
        register_lines: vec![
            format!(
                "R0:{:08X}  R1:{:08X}  R2:{:08X}  R3:{:08X}",
                regs[0], regs[1], regs[2], regs[3]
            ),
            format!(
                "R4:{:08X}  R5:{:08X}  R6:{:08X}  R7:{:08X}",
                regs[4], regs[5], regs[6], regs[7]
            ),
            format!(
                "R8:{:08X}  R9:{:08X} R10:{:08X} R11:{:08X}",
                regs[8], regs[9], regs[10], regs[11]
            ),
            format!(
                "R12:{:08X} SP:{:08X} LR:{:08X} PC:{:08X}",
                regs[12], regs[13], regs[14], pc
            ),
            format!("CPSR:{cpsr:08X}  visible PC:{:08X}", emu.cpu_visible_pc()),
        ],
        flags: vec![
            ('N', cpsr & (1 << 31) != 0),
            ('Z', cpsr & (1 << 30) != 0),
            ('C', cpsr & (1 << 29) != 0),
            ('V', cpsr & (1 << 28) != 0),
            ('I', cpsr & (1 << 7) != 0),
            ('F', cpsr & (1 << 6) != 0),
            ('T', cpsr & (1 << 5) != 0),
        ],
        status_text: format!(
            "Mode: {:?}  State: {}",
            emu.cpu_mode(),
            if emu.is_cpu_suspended() {
                "Suspended"
            } else {
                "Running"
            }
        ),
        cpu_state: if emu.is_cpu_suspended() {
            "Suspended"
        } else {
            "Running"
        }
        .to_string(),
        cycles: emu.cpu_cycles(),
        last_opcode_line: emu.last_fetch().map_or_else(
            || "no instruction fetched yet".into(),
            |f| {
                format!(
                    "{:?} @ {:08X} raw={:08X} {:?} fetch={}c",
                    f.instruction_set, f.pc, f.raw, f.decoded, f.fetch_cycles
                )
            },
        ),
        sections: vec![
            crate::debug::DebugSection {
                heading: "Core",
                lines: vec!["Experimental GBA CPU/video core".into()],
            },
            crate::debug::DebugSection {
                heading: "PPU",
                lines: vec![
                    format!(
                        "DISPCNT:{:04X} mode:{} VCOUNT:{} VBlank:{}",
                        ppu.dispcnt, ppu.display_mode, ppu.vcount, ppu.in_vblank
                    ),
                    format!(
                        "Layers: BG0={} BG1={} BG2={} BG3={} OBJ={} OBJ-map={}",
                        on_off(ppu.bg_enabled[0]),
                        on_off(ppu.bg_enabled[1]),
                        on_off(ppu.bg_enabled[2]),
                        on_off(ppu.bg_enabled[3]),
                        on_off(ppu.obj_enabled),
                        if ppu.obj_mapping_1d { "1D" } else { "2D" }
                    ),
                    format!(
                        "BGCNT: {:04X} {:04X} {:04X} {:04X}",
                        ppu.bgcnt[0], ppu.bgcnt[1], ppu.bgcnt[2], ppu.bgcnt[3]
                    ),
                    format!("Non-black framebuffer pixels: {}", ppu.non_black_pixels),
                ],
            },
        ],
        mem_around_pc,
        recent_op_lines: Vec::new(),
        breakpoints: emu.iter_breakpoints().collect(),
        watchpoints: emu
            .debug_watchpoints()
            .iter()
            .map(|w| crate::debug::WatchpointDisplay {
                address: w.address,
                watch_type: w.watch_type,
            })
            .collect(),
        hit_breakpoint: emu.debug_hit_breakpoint(),
        hit_watchpoint: emu
            .debug_hit_watchpoint()
            .map(|h| crate::debug::WatchHitDisplay {
                address: h.address,
                old_value: h.old_value,
                new_value: h.new_value,
                watch_type: h.watch_type,
            }),
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
