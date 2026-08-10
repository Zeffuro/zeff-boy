use crate::debug::{
    ApuChannelDebug, ApuDebugInfo, CpuDebugSnapshot, DebugSection, InputDebugInfo, PerfInfo,
    RomDebugInfo, RomInfoSection,
};
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};
use std::borrow::Cow;
use zeff_emu_common::address::Address;
use zeff_ws_core::emulator::Emulator;
use zeff_ws_core::hardware::cartridge::{MinimumSystem, RomFooter, RomOrientation};

const WS_SEARCH_RANGES: &[(Address, Address)] = &[(0, 0x0F_FFFF)];

pub(crate) fn collect_ws_snapshot(
    emu: &Emulator,
    snapshot: &SnapshotRequest,
    buffers: ReusableBuffers,
) -> super::UiFrameData {
    let rom_bytes = emu.cartridge_rom_bytes();
    let mut data = super::UiFrameData {
        perf_info: snapshot.want_perf_info.then(|| ws_perf_snapshot(emu)),
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
            WS_SEARCH_RANGES,
            |addr| emu.cpu_peek8(addr),
        ),
        ..Default::default()
    };

    if snapshot.want_debug_info {
        data.cpu_debug = Some(ws_cpu_snapshot(emu));
        data.input_debug = Some(ws_input_snapshot(emu));
    }

    if snapshot.show_apu_viewer {
        data.apu_debug = Some(ws_apu_snapshot(emu));
    }

    if snapshot.show_rom_info {
        data.rom_debug = Some(ws_rom_info(emu));
    }

    data
}

fn ws_apu_snapshot(emu: &Emulator) -> ApuDebugInfo {
    let apu = emu.apu_debug_snapshot();
    let channels = (0..4usize)
        .map(|index| {
            let volume = apu.volume[index];
            let left = volume >> 4;
            let right = volume & 0x0F;
            let enabled = apu.control & (1 << index) != 0;
            ApuChannelDebug {
                name: match index {
                    0 => "CH0 Wave",
                    1 => "CH1 Wave/Voice",
                    2 => "CH2 Wave/Sweep",
                    _ => "CH3 Wave/Noise",
                },
                enabled,
                muted: apu.channel_mutes[index],
                register_lines: vec![format!(
                    "period={:03X} volume={:02X} L={} R={} sample_pos={} mode={}",
                    apu.period[index],
                    volume,
                    left,
                    right,
                    apu.sample_pos[index],
                    ws_apu_channel_mode(index, apu.control, apu.noise_control)
                )],
                detail_line: format!(
                    "freq={}Hz {}",
                    ws_apu_frequency_label(apu.period[index]),
                    if enabled { "enabled" } else { "disabled" }
                ),
                waveform: ws_apu_channel_waveform(emu, apu.sample_ram_pos, index),
            }
        })
        .collect();

    ApuDebugInfo {
        master_lines: vec![
            format!(
                "CTRL={:02X} OUT={:02X} sample_base={:02X} buffered_samples={} generation={}",
                apu.control,
                apu.output_control,
                apu.sample_ram_pos,
                apu.buffered_samples,
                if apu.sample_generation_enabled {
                    "on"
                } else {
                    "off"
                }
            ),
            format!(
                "sample_rate={} noise={:02X} lfsr={:04X} sweep_value={:02X} sweep_step={:02X}",
                apu.sample_rate, apu.noise_control, apu.nreg, apu.sweep_value, apu.sweep_step
            ),
            format!(
                "voice_volume={:02X} hyper_sample={:02X} hyper_ctrl={:02X} hyper_ch={:02X}",
                apu.voice_volume,
                apu.hyper_voice_sample,
                apu.hyper_voice_control,
                apu.hyper_voice_channel_control
            ),
        ],
        master_waveform: Vec::new(),
        channels,
        extra_sections: vec![DebugSection {
            heading: "Wave RAM",
            lines: ws_apu_wave_ram_lines(emu, apu.sample_ram_pos),
        }],
    }
}

fn ws_apu_channel_mode(channel: usize, control: u8, noise_control: u8) -> &'static str {
    match channel {
        1 if control & 0x20 != 0 => "direct voice",
        2 if control & 0x40 != 0 => "sweep",
        3 if control & 0x80 != 0 && noise_control & 0x10 != 0 => "noise",
        _ => "wave",
    }
}

fn ws_apu_frequency_label(period: u16) -> String {
    let clocks = 2048u16.saturating_sub(period & 0x07FF);
    if clocks <= 4 {
        "-".into()
    } else {
        format!("{:.1}", 3_072_000.0 / f64::from(clocks) / 32.0)
    }
}

fn ws_apu_channel_waveform(emu: &Emulator, sample_ram_pos: u8, channel: usize) -> Vec<f32> {
    (0..32usize)
        .map(|sample_pos| {
            let offset =
                (u32::from(sample_ram_pos) << 6) + (channel as u32) * 16 + (sample_pos as u32 / 2);
            let byte = emu.cpu_peek8(offset);
            let sample = if sample_pos & 1 == 0 {
                byte & 0x0F
            } else {
                byte >> 4
            };
            (f32::from(sample) - 7.5) / 7.5
        })
        .collect()
}

fn ws_apu_wave_ram_lines(emu: &Emulator, sample_ram_pos: u8) -> Vec<String> {
    (0..4u32)
        .map(|channel| {
            let base = (u32::from(sample_ram_pos) << 6) + channel * 16;
            let bytes = (0..16u32)
                .map(|offset| format!("{:02X}", emu.cpu_peek8(base + offset)))
                .collect::<Vec<_>>()
                .join(" ");
            format!("CH{channel}: {bytes}")
        })
        .collect()
}

fn ws_perf_snapshot(emu: &Emulator) -> PerfInfo {
    PerfInfo {
        fps: 0.0,
        target_fps: zeff_emu_common::system::System::WonderSwan.target_fps(),
        speed_mode_label: "1x",
        frames_in_flight: 0,
        cycles: emu.cpu_cycles(),
        platform_name: "WonderSwan",
        hardware_label: hardware_label(emu.footer()),
        hardware_pref_label: "Auto".into(),
    }
}

fn ws_cpu_snapshot(emu: &Emulator) -> CpuDebugSnapshot {
    let regs = emu.cpu_registers();
    let segs = emu.cpu_segments();
    let flags = emu.cpu_flags();
    let pc = emu.cpu_pc();
    let ppu = emu.ppu_debug_snapshot();
    let mem_around_pc: [(Address, u8); 32] = std::array::from_fn(|i| {
        let addr = pc.wrapping_add(i as u32);
        (addr, emu.cpu_peek8(addr))
    });

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
        recent_op_lines: Vec::new(),
        breakpoints: Vec::new(),
        watchpoints: Vec::new(),
        hit_breakpoint: None,
        hit_watchpoint: None,
    }
}

fn ws_input_snapshot(emu: &Emulator) -> InputDebugInfo {
    let keypad = emu.io_peek8(0xB5);
    InputDebugInfo {
        sections: vec![DebugSection {
            heading: "WonderSwan Keypad",
            lines: vec![format!(
                "IO B5={keypad:02X}  selected rows={}",
                selected_rows_label(keypad)
            )],
        }],
        progress_bars: Vec::new(),
    }
}

fn ws_rom_info(emu: &Emulator) -> RomDebugInfo {
    let footer = emu.footer();
    RomDebugInfo {
        sections: vec![
            RomInfoSection {
                heading: "WonderSwan Footer",
                fields: vec![
                    ("CRC32", format!("{:08X}", emu.rom_crc32())),
                    ("Developer ID", format!("{:02X}", footer.developer_id)),
                    ("Minimum System", format!("{:?}", footer.minimum_system)),
                    ("Cartridge ID", format!("{:02X}", footer.cartridge_id)),
                    ("Revision", footer.revision.to_string()),
                    ("ROM Size", rom_size_label(footer)),
                    ("Save", format!("{:?}", footer.save_kind)),
                    ("Flags", format!("{:02X}", footer.flags)),
                    ("Orientation", format!("{:?}", footer.orientation())),
                    ("RTC", on_off(footer.rtc_present).into()),
                    (
                        "Checksum",
                        format!(
                            "{:04X} computed={:04X} {}",
                            footer.checksum,
                            footer.computed_checksum,
                            if footer.checksum_valid {
                                "valid"
                            } else {
                                "invalid"
                            }
                        ),
                    ),
                ],
            },
            RomInfoSection {
                heading: "Core",
                fields: vec![(
                    "Status",
                    "Experimental WonderSwan/WonderSwan Color interpreter".into(),
                )],
            },
        ],
    }
}

fn hardware_label(footer: &RomFooter) -> Cow<'static, str> {
    let system = match footer.minimum_system {
        MinimumSystem::WonderSwan => "WonderSwan",
        MinimumSystem::WonderSwanColor => "WonderSwan Color",
        MinimumSystem::Unknown(_) => "WonderSwan (unknown minimum system)",
    };
    let orientation = match footer.orientation() {
        RomOrientation::Horizontal => "horizontal",
        RomOrientation::Vertical => "vertical",
    };
    format!("{system}, {orientation}").into()
}

fn rom_size_label(footer: &RomFooter) -> String {
    match footer.rom_size.declared_bytes {
        Some(bytes) => format!("{} bytes (code {:02X})", bytes, footer.rom_size.code),
        None => format!("unknown (code {:02X})", footer.rom_size.code),
    }
}

fn selected_rows_label(value: u8) -> &'static str {
    match (value & 0x10 != 0, value & 0x20 != 0, value & 0x40 != 0) {
        (false, false, false) => "none",
        (true, false, false) => "Y",
        (false, true, false) => "X",
        (false, false, true) => "Buttons",
        (true, true, false) => "Y X",
        (true, false, true) => "Y Buttons",
        (false, true, true) => "X Buttons",
        (true, true, true) => "Y X Buttons",
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu_thread::RenderSettings;
    use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};
    use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

    fn minimal_ws_rom() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        let reset = rom.len() - 16;
        rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        rom[0] = 0xF4;
        let footer = rom.len() - 10;
        rom[footer] = 0x01;
        rom[footer + 1] = 0x00;
        rom[footer + 2] = 0x23;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    fn snapshot_request() -> SnapshotRequest {
        SnapshotRequest {
            want_debug_info: true,
            want_perf_info: true,
            any_viewer_open: false,
            any_vram_viewer_open: false,
            show_oam_viewer: false,
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

    fn apu_snapshot_request() -> SnapshotRequest {
        SnapshotRequest {
            show_apu_viewer: true,
            ..snapshot_request()
        }
    }

    #[test]
    fn wonder_swan_snapshot_exposes_data_for_app_rendering() {
        let rom = minimal_ws_rom();
        let emu = Emulator::from_rom_data(&rom).unwrap();
        let data = collect_ws_snapshot(
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

        assert!(data.perf_info.is_some());
        assert!(data.cpu_debug.is_some());
        assert!(data.rom_debug.is_some());
        assert!(data.memory_page.is_some());
        assert!(data.rom_page.is_some());
        assert_eq!(data.rom_size, rom.len() as u32);
    }

    #[test]
    fn wonder_swan_snapshot_exposes_apu_debug_when_viewer_is_open() {
        let rom = minimal_ws_rom();
        let mut emu = Emulator::from_rom_data(&rom).unwrap();
        emu.cpu_write8(0x0000, 0x10);
        emu.io_write8(0x0080, 0x00);
        emu.io_write8(0x0081, 0x07);
        emu.io_write8(0x0088, 0xF8);
        emu.io_write8(0x0090, 0x01);

        let data = collect_ws_snapshot(
            &emu,
            &apu_snapshot_request(),
            ReusableBuffers {
                audio: None,
                vram: None,
                oam: None,
                memory_page: None,
                nes_chr: None,
                nes_nametable: None,
            },
        );

        let apu = data.apu_debug.expect("WS APU debug should be populated");
        assert_eq!(apu.channels.len(), 4);
        assert!(apu.channels[0].enabled);
        assert_eq!(apu.channels[0].waveform.len(), 32);
    }
}
