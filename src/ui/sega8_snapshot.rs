use crate::debug::{
    ConsoleGraphicsData, CpuDebugSnapshot, DebugSection, InputDebugInfo, OamDebugInfo,
    PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug, PerfInfo, RomDebugInfo, RomInfoSection,
    Sega8GraphicsData, WatchHitDisplay, WatchpointDisplay, z80_disassemble_around,
};
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};
use std::borrow::Cow;
use zeff_emu_common::address::{Address, narrow_u16};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::cartridge::{CodemastersHeader, RomHeader, Sega8System};
use zeff_sega8_core::hardware::constants::{
    GG_COLOR_CHANNEL_SCALE_4BIT, MODE4_SPRITE_TABLE_BYTES, MODE4_SPRITE_TERMINATOR_Y,
    MODE4_SPRITE_X_TILE_TABLE_OFFSET, SMS_COLOR_CHANNEL_SCALE_2BIT, SMS_CRAM_SIZE,
    SMS_GG_COLOR_INDEX_MASK, SMS_VISIBLE_SCANLINES, SMS_VRAM_SIZE, Z80_FLAG_BIT_3, Z80_FLAG_BIT_5,
    Z80_FLAG_CARRY, Z80_FLAG_HALF_CARRY, Z80_FLAG_PARITY_OVERFLOW, Z80_FLAG_SIGN,
    Z80_FLAG_SUBTRACT, Z80_FLAG_ZERO,
};
use zeff_sega8_core::hardware::input::ControllerPort;

const SEGA8_ADDRESS_START: Address = 0x0000;
const SEGA8_ADDRESS_END: Address = 0xFFFF;
const SEGA8_SEARCH_RANGES: &[(Address, Address)] = &[(SEGA8_ADDRESS_START, SEGA8_ADDRESS_END)];
const MODE4_SPRITE_COUNT: usize = 64;
const PALETTE_COLORS_PER_ROW: usize = 16;
const PALETTE_ROW_COUNT: usize = 2;
const RECENT_OPCODE_LINE_COUNT: usize = 16;

pub(crate) fn collect_sega8_snapshot(
    emu: &Emulator,
    snapshot: &SnapshotRequest,
    mut buffers: ReusableBuffers,
) -> super::UiFrameData {
    let rom_bytes = emu.bus().cartridge.rom();
    let mut data = super::UiFrameData {
        perf_info: snapshot.want_perf_info.then(|| sega8_perf_snapshot(emu)),
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
            buffers.memory_page.take(),
            |addr| emu.bus().cpu_read(narrow_u16(addr)),
        ),
        memory_search_results: super::build_memory_search_ranges(
            snapshot.memory_search.as_ref(),
            SEGA8_SEARCH_RANGES,
            |addr| emu.bus().cpu_read(narrow_u16(addr)),
        ),
        ..Default::default()
    };

    if snapshot.want_debug_info {
        data.cpu_debug = Some(sega8_cpu_snapshot(emu));
        data.input_debug = Some(sega8_input_snapshot(emu));
    }

    data.disassembly_view = super::build_disassembly_view(
        snapshot.show_disassembler,
        snapshot.last_disasm_pc,
        Address::from(emu.cpu().regs().pc),
        || z80_disassemble_around(|addr| emu.bus().cpu_read(addr), emu.cpu().regs().pc, 12, 26),
        emu.iter_breakpoints(),
    );

    if snapshot.show_rom_info {
        data.rom_debug = Some(sega8_rom_info(emu));
    }

    if snapshot.any_vram_viewer_open {
        data.graphics_data = Some(sega8_graphics_snapshot(
            emu,
            buffers.vram.take(),
            buffers.oam.take(),
        ));
    }

    if snapshot.show_oam_viewer {
        data.oam_debug = Some(sega8_oam_snapshot(emu));
    }

    if snapshot.any_viewer_open {
        data.palette_debug = Some(sega8_palette_snapshot(emu));
    }

    data
}

fn sega8_perf_snapshot(emu: &Emulator) -> PerfInfo {
    let system = common_system_for_sega8(emu.system());
    PerfInfo {
        fps: 0.0,
        target_fps: system.target_fps(),
        speed_mode_label: "1Ã—",
        frames_in_flight: 0,
        cycles: emu.cpu().cycles(),
        platform_name: "Sega 8-bit",
        hardware_label: sega8_system_label(emu.system()).into(),
        hardware_pref_label: "Auto".into(),
    }
}

fn sega8_cpu_snapshot(emu: &Emulator) -> CpuDebugSnapshot {
    let cpu = emu.cpu();
    let regs = cpu.regs();
    let pc = regs.pc;
    let vdp = emu.bus().vdp();
    let mode4 = vdp.mode4_debug_snapshot();
    let psg = emu.bus().apu().debug_snapshot();
    let mapper = emu.bus().mapper();
    let mem_around_pc: [(Address, u8); 32] = std::array::from_fn(|i| {
        let addr = pc.wrapping_add(i as u16);
        (Address::from(addr), emu.bus().cpu_read(addr))
    });
    let recent_op_lines = emu
        .recent_opcodes(RECENT_OPCODE_LINE_COUNT)
        .into_iter()
        .map(|(pc, opcode, cycles)| format!("{pc:04X}: {opcode:02X} ({cycles} cyc)"))
        .collect();
    let watchpoints = emu
        .debug_watchpoints()
        .iter()
        .map(|watch| WatchpointDisplay {
            address: watch.address,
            watch_type: watch.watch_type,
        })
        .collect();
    let hit_watchpoint = emu.debug_hit_watchpoint().map(|hit| WatchHitDisplay {
        address: hit.address,
        old_value: hit.old_value,
        new_value: hit.new_value,
        watch_type: hit.watch_type,
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
        cycles: cpu.cycles(),
        last_opcode_line: format!(
            "PC={:04X} opcode={:02X} cycles={}",
            cpu.last_opcode_pc(),
            cpu.last_opcode(),
            cpu.cycles()
        ),
        sections: vec![
            DebugSection {
                heading: "Interrupts",
                lines: vec![
                    format!(
                        "IM={:?} IFF1={} IFF2={} pending={} frame_irq={} line_irq={} line_pending={}",
                        cpu.interrupt_mode(),
                        on_off(cpu.interrupts_enabled()),
                        on_off(cpu.saved_interrupts_enabled()),
                        on_off(vdp.interrupt_pending()),
                        on_off(vdp.frame_interrupt_enabled()),
                        on_off(vdp.line_interrupt_enabled()),
                        on_off(vdp.line_interrupt_pending())
                    ),
                    format!(
                        "VDP status={:02X} line_counter={} scanline={} dot_cycle={}",
                        vdp.status(),
                        vdp.line_counter(),
                        vdp.scanline(),
                        vdp.scanline_cycle()
                    ),
                ],
            },
            DebugSection {
                heading: "VDP",
                lines: vec![
                    format!(
                        "addr={:04X} code={} V={} H={} visible_scanlines={} tms_mode={:?}",
                        vdp.address(),
                        vdp.code(),
                        vdp.v_counter(),
                        vdp.h_counter(),
                        SMS_VISIBLE_SCANLINES,
                        vdp.tms9918_mode()
                    ),
                    format!("regs {}", hex_bytes(vdp.registers())),
                    format!(
                        "VRAM nonzero={} CRAM nonzero={}",
                        vdp.vram().iter().filter(|&&byte| byte != 0).count(),
                        vdp.cram().iter().filter(|&&byte| byte != 0).count()
                    ),
                    format!(
                        "mode4={} nt={:04X} sat={:04X} scroll={},{} backdrop={:02}",
                        on_off(mode4.enabled),
                        mode4.name_table_base,
                        mode4.sprite_table_base,
                        mode4.horizontal_scroll,
                        mode4.vertical_scroll,
                        mode4.backdrop_color_index
                    ),
                    format!(
                        "mode4 flags hlock={} vlock={} left_mask={} sprite_shift={} sprite_h={} max_sprites={}",
                        on_off(mode4.horizontal_scroll_lock),
                        on_off(mode4.vertical_scroll_lock),
                        on_off(mode4.hide_left_column),
                        on_off(mode4.sprite_shift_left),
                        mode4.sprite_height,
                        mode4.max_sprites_per_line
                    ),
                ],
            },
            DebugSection {
                heading: "PSG",
                lines: vec![
                    format!(
                        "tone={:03X},{:03X},{:03X} volume={:X},{:X},{:X},{:X} noise={:X} stereo={:02X}",
                        psg.tone_period[0],
                        psg.tone_period[1],
                        psg.tone_period[2],
                        psg.volume[0],
                        psg.volume[1],
                        psg.volume[2],
                        psg.volume[3],
                        psg.noise_control,
                        psg.stereo_control
                    ),
                    format!(
                        "sample_rate={} generation={} buffered_samples={} latch={} writes={}",
                        psg.sample_rate,
                        on_off(psg.sample_generation_enabled),
                        psg.buffered_samples,
                        psg.latched_register,
                        psg.write_count
                    ),
                    format!("mutes={:?}", psg.channel_mutes),
                ],
            },
            DebugSection {
                heading: "Mapper",
                lines: vec![
                    format!("kind={}", mapper.kind_label()),
                    format!("frame_control={:02X}", mapper.frame_control()),
                    format!("slot_banks={:?}", mapper.slot_banks()),
                    format!(
                        "slot2_cart_ram={} cart_ram_bank={} cart_ram_nonzero={}",
                        on_off(mapper.slot2_cartridge_ram_enabled()),
                        mapper.cartridge_ram_bank(),
                        emu.bus()
                            .cartridge_ram()
                            .iter()
                            .filter(|&&byte| byte != 0)
                            .count()
                    ),
                ],
            },
            DebugSection {
                heading: "Cartridge",
                lines: vec![
                    format!("system={}", sega8_system_label(emu.system())),
                    format!(
                        "raw={} normalized={} banks={} copier_header_stripped={}",
                        emu.bus().cartridge.raw_len(),
                        emu.bus().cartridge.normalized_len(),
                        emu.bus().cartridge.rom_bank_count(),
                        on_off(emu.bus().cartridge.copier_header_stripped())
                    ),
                ],
            },
        ],
        mem_around_pc,
        recent_op_lines,
        breakpoints: emu.iter_breakpoints().collect(),
        watchpoints,
        hit_breakpoint: emu.debug_hit_breakpoint(),
        hit_watchpoint,
    }
}

fn sega8_input_snapshot(emu: &Emulator) -> InputDebugInfo {
    InputDebugInfo {
        sections: vec![DebugSection {
            heading: "Controller Ports",
            lines: vec![
                format!(
                    "P1={:02X} P2={:02X} (active-low)",
                    emu.bus().input().read_controller(ControllerPort::One),
                    emu.bus().input().read_controller(ControllerPort::Two)
                ),
                "Host buttons map to D-pad plus Button 1/Button 2".into(),
            ],
        }],
        progress_bars: Vec::new(),
    }
}

fn sega8_rom_info(emu: &Emulator) -> RomDebugInfo {
    let cart = &emu.bus().cartridge;
    let mut sections = vec![RomInfoSection {
        heading: "Sega 8-bit Cartridge",
        fields: vec![
            ("System", sega8_system_label(cart.system()).into()),
            ("Mapper", cart.mapper_kind().label().into()),
            ("Raw Size", format!("{} bytes", cart.raw_len())),
            (
                "Normalized Size",
                format!("{} bytes", cart.normalized_len()),
            ),
            ("ROM Banks", cart.rom_bank_count().to_string()),
            (
                "Copier Header",
                if cart.copier_header_stripped() {
                    "stripped"
                } else {
                    "absent"
                }
                .into(),
            ),
        ],
    }];

    sections.push(match cart.header() {
        Some(header) => sega8_header_section(header),
        None => RomInfoSection {
            heading: "Sega Header",
            fields: vec![("Status", "No TMR SEGA header found".into())],
        },
    });

    if let Some(header) = cart.codemasters_header() {
        sections.push(sega8_codemasters_header_section(header));
    }

    sections.push(RomInfoSection {
        heading: "Core",
        fields: vec![(
            "Status",
            "Experimental Sega Master System/Game Gear/SG-1000 core; Z80 interpreter and SMS/GG Mode 4 video are in active bring-up".into(),
        )],
    });

    RomDebugInfo { sections }
}

fn sega8_header_section(header: RomHeader) -> RomInfoSection {
    RomInfoSection {
        heading: "Sega Header",
        fields: vec![
            ("Location", format!("{:?}", header.location)),
            ("Checksum", format!("{:04X}", header.checksum)),
            (
                "Product Code BCD",
                format!(
                    "{:02X} {:02X} {:X}",
                    header.product_code_bcd[0],
                    header.product_code_bcd[1],
                    header.product_code_bcd[2]
                ),
            ),
            ("Version", header.version.to_string()),
            (
                "Region",
                format!("{:?} ({:X})", header.region, header.region.code()),
            ),
            ("ROM Size Code", format!("{:X}", header.rom_size_code)),
        ],
    }
}

fn sega8_codemasters_header_section(header: CodemastersHeader) -> RomInfoSection {
    RomInfoSection {
        heading: "Codemasters Header",
        fields: vec![
            ("Checksum Banks", header.checksum_bank_count.to_string()),
            (
                "Build Date BCD",
                format!(
                    "{:02X}/{:02X}/{:02X}",
                    header.day_bcd, header.month_bcd, header.year_bcd
                ),
            ),
            (
                "Build Time BCD",
                format!("{:02X}:{:02X}", header.hour_bcd, header.minute_bcd),
            ),
            ("Checksum", format!("{:04X}", header.checksum)),
            (
                "Checksum Complement",
                format!("{:04X}", header.checksum_complement),
            ),
        ],
    }
}

fn sega8_graphics_snapshot(
    emu: &Emulator,
    reusable_vram: Option<Vec<u8>>,
    reusable_oam: Option<Vec<u8>>,
) -> ConsoleGraphicsData {
    let vdp = emu.bus().vdp();
    let mode4 = vdp.mode4_debug_snapshot();
    let mut vram = reusable_vram.unwrap_or_default();
    vram.resize(SMS_VRAM_SIZE, 0);
    vram.copy_from_slice(vdp.vram());

    let mut oam = reusable_oam.unwrap_or_default();
    copy_sprite_table(&mut oam, vdp.vram(), mode4.sprite_table_base);

    ConsoleGraphicsData::Sega8(Box::new(Sega8GraphicsData {
        system: emu.system(),
        vram,
        cram: vdp.cram().to_vec(),
        oam,
        registers: *vdp.registers(),
        status: vdp.status(),
        address: vdp.address(),
        code: vdp.code(),
        v_counter: vdp.v_counter(),
        h_counter: vdp.h_counter(),
        scanline: vdp.scanline(),
        scanline_cycle: vdp.scanline_cycle(),
        line_counter: vdp.line_counter(),
        frame_interrupt_enabled: vdp.frame_interrupt_enabled(),
        line_interrupt_enabled: vdp.line_interrupt_enabled(),
        interrupt_pending: vdp.interrupt_pending(),
        line_interrupt_pending: vdp.line_interrupt_pending(),
        display_enabled: vdp.display_enabled(),
        tms9918_mode: format!("{:?}", vdp.tms9918_mode()),
        sprite_table_base: mode4.sprite_table_base,
        mode4,
    }))
}

fn sega8_oam_snapshot(emu: &Emulator) -> OamDebugInfo {
    let vdp = emu.bus().vdp();
    let sprite_table_base = vdp.mode4_debug_snapshot().sprite_table_base;
    let vram = vdp.vram();
    let rows = (0..MODE4_SPRITE_COUNT)
        .filter_map(|index| {
            let y = vram[(sprite_table_base + index) % vram.len()];
            if y == MODE4_SPRITE_TERMINATOR_Y && index > 0 {
                return None;
            }
            let xt_base = sprite_table_base + MODE4_SPRITE_X_TILE_TABLE_OFFSET + index * 2;
            let x = vram[xt_base % vram.len()];
            let tile = vram[(xt_base + 1) % vram.len()];
            Some(vec![
                format!("{index:02}"),
                format!("{x:03}"),
                format!("{y:03}"),
                format!("{tile:02X}"),
                format!("{:04X}", xt_base % vram.len()),
            ])
        })
        .collect();
    OamDebugInfo {
        headers: &["#", "X", "Y", "Tile", "XT Addr"],
        rows,
    }
}

fn sega8_palette_snapshot(emu: &Emulator) -> PaletteDebugInfo {
    let rows = (0..PALETTE_ROW_COUNT)
        .map(|row| PaletteRowDebug {
            label: if row == 0 { "BG" } else { "OBJ" }.into(),
            colors: (0..PALETTE_COLORS_PER_ROW)
                .map(|col| {
                    sega8_palette_rgba(
                        emu.system(),
                        emu.bus().vdp().cram(),
                        row * PALETTE_COLORS_PER_ROW + col,
                    )
                })
                .collect(),
        })
        .collect();
    PaletteDebugInfo {
        groups: vec![PaletteGroupDebug {
            title: Cow::Borrowed("CRAM"),
            rows,
        }],
    }
}

fn copy_sprite_table(buf: &mut Vec<u8>, vram: &[u8], sprite_table_base: usize) {
    buf.resize(MODE4_SPRITE_TABLE_BYTES, 0);
    for (offset, byte) in buf.iter_mut().enumerate() {
        *byte = vram[(sprite_table_base + offset) % vram.len()];
    }
}

fn sega8_palette_rgba(
    system: Sega8System,
    cram: &[u8; SMS_CRAM_SIZE],
    color_index: usize,
) -> [u8; 4] {
    match system {
        Sega8System::GameGear => {
            let base = (color_index & SMS_GG_COLOR_INDEX_MASK) * 2;
            let raw = u16::from_le_bytes([cram[base], cram[(base + 1) % cram.len()]]);
            [
                ((raw & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
                (((raw >> 4) & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
                (((raw >> 8) & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
                0xFF,
            ]
        }
        Sega8System::MasterSystem | Sega8System::Sg1000 => {
            let raw = cram[color_index & SMS_GG_COLOR_INDEX_MASK];
            [
                (raw & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
                ((raw >> 2) & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
                ((raw >> 4) & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
                0xFF,
            ]
        }
    }
}

fn common_system_for_sega8(system: Sega8System) -> zeff_emu_common::system::System {
    match system {
        Sega8System::MasterSystem => zeff_emu_common::system::System::MasterSystem,
        Sega8System::GameGear => zeff_emu_common::system::System::GameGear,
        Sega8System::Sg1000 => zeff_emu_common::system::System::Sg1000,
    }
}

fn sega8_system_label(system: Sega8System) -> &'static str {
    match system {
        Sega8System::MasterSystem => "Sega Master System",
        Sega8System::GameGear => "Game Gear",
        Sega8System::Sg1000 => "SG-1000",
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu_thread::RenderSettings;
    use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    fn snapshot_request() -> SnapshotRequest {
        SnapshotRequest {
            want_debug_info: true,
            want_perf_info: true,
            any_viewer_open: true,
            any_vram_viewer_open: true,
            show_oam_viewer: true,
            show_apu_viewer: false,
            show_disassembler: false,
            show_rom_info: true,
            show_memory_viewer: true,
            memory_view_start: 0,
            show_rom_viewer: true,
            rom_view_start: 0,
            last_disasm_pc: None,
            memory_search: None,
            rom_search: None,
            render: RenderSettings {
                color_correction: ColorCorrection::None,
                color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                dmg_palette_preset: DmgPalettePreset::default(),
                nes_palette_mode: NesPaletteMode::default(),
                nes_custom_palette: None,
                sgb_border_enabled: false,
            },
        }
    }

    #[test]
    fn sega8_snapshot_exposes_live_debug_and_graphics_data() {
        let emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::MasterSystem).unwrap();

        let data = collect_sega8_snapshot(
            &emu,
            &snapshot_request(),
            ReusableBuffers {
                audio: None,
                vram: None,
                oam: None,
                memory_page: None,
                nes_chr: None,
                nes_nametable: None,
            },
        );

        assert!(data.cpu_debug.is_some());
        assert!(data.perf_info.is_some());
        assert!(data.rom_debug.is_some());
        assert!(data.memory_page.is_some());
        assert!(data.rom_page.is_some());
        assert!(data.oam_debug.is_some());
        assert!(data.palette_debug.is_some());
        let ConsoleGraphicsData::Sega8(gfx) = data
            .graphics_data
            .as_ref()
            .expect("Sega8 snapshot should include graphics data")
        else {
            panic!("Sega8 snapshot should include Sega8 graphics data");
        };
        assert!(!gfx.mode4.enabled);
        assert_eq!(gfx.mode4.name_table_base, 0);
        assert_eq!(gfx.mode4.sprite_table_base, 0);
        let cpu = data
            .cpu_debug
            .as_ref()
            .expect("Sega8 snapshot should include CPU debug data");
        let vdp_section = cpu
            .sections
            .iter()
            .find(|section| section.heading == "VDP")
            .expect("Sega8 CPU debug should include a VDP section");
        assert!(vdp_section.lines.iter().any(|line| line.contains("mode4=")));
        assert_eq!(data.rom_size, 2);
    }

    #[test]
    fn sega8_snapshot_exposes_z80_disassembly() {
        let mut emu = Emulator::new_with_hint(
            &[0x3E, 0x90, 0xD3, 0x7F, 0x76],
            48_000,
            SystemHint::MasterSystem,
        )
        .unwrap();
        emu.add_breakpoint(2);
        let mut request = snapshot_request();
        request.show_disassembler = true;

        let data = collect_sega8_snapshot(
            &emu,
            &request,
            ReusableBuffers {
                audio: None,
                vram: None,
                oam: None,
                memory_page: None,
                nes_chr: None,
                nes_nametable: None,
            },
        );
        let disassembly = data
            .disassembly_view
            .expect("Sega8 snapshot should include Z80 disassembly");

        assert_eq!(disassembly.pc, 0);
        assert!(disassembly.breakpoints.contains(&2));
        assert!(
            disassembly
                .lines
                .iter()
                .any(|line| line.mnemonic.as_str() == "LD A,$90")
        );
        assert!(
            disassembly
                .lines
                .iter()
                .any(|line| line.mnemonic.as_str() == "OUT ($7F),A")
        );
    }
}
