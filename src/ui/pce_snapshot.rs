use crate::debug::{CpuDebugSnapshot, DebugSection, RomDebugInfo, RomInfoSection};
use crate::emu_backend::PceBackend;
use crate::emu_core_trait::EmulatorCore;
use crate::emu_thread::SnapshotRequest;
use zeff_emu_common::address::Address;

pub(crate) fn collect_pce_snapshot(
    backend: &PceBackend,
    request: &SnapshotRequest,
    reusable_memory_page: Option<Vec<(Address, u8)>>,
) -> super::UiFrameData {
    let rom = backend.hucard_rom();
    let memory_page = super::build_memory_page(
        request.show_memory_viewer,
        request.memory_view_start,
        reusable_memory_page,
        |address| backend.debug_peek8(address),
    );
    let memory_search_results =
        super::build_memory_search(request.memory_search.as_ref(), |address| {
            backend.debug_peek8(address)
        });
    let cpu_debug = request
        .want_debug_info
        .then(|| pce_cpu_snapshot(backend.debug_cpu_snapshot()));
    let rom_debug = request.show_rom_info.then(|| pce_rom_info(backend));

    super::UiFrameData {
        cpu_debug,
        memory_page,
        memory_search_results,
        rom_page: super::build_rom_page(request.show_rom_viewer, request.rom_view_start, rom),
        rom_search_results: super::build_rom_search(request.rom_search.as_ref(), rom),
        rom_size: rom.len() as u32,
        rom_debug,
        ..Default::default()
    }
}

fn pce_rom_info(backend: &PceBackend) -> RomDebugInfo {
    let hash = backend
        .rom_hash()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let media = if let Some(cdrom2) = backend.cdrom2() {
        let disc = cdrom2.disc();
        let audio_tracks = disc
            .tracks()
            .iter()
            .filter(|track| track.mode() == zeff_pce_core::hardware::CdTrackMode::Audio)
            .count();
        let layout = disc
            .tracks()
            .iter()
            .map(|track| {
                format!(
                    "{:02}:{:?}@{}+{}",
                    track.number(),
                    track.mode(),
                    track.index1_lba(),
                    track.sector_count()
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        RomInfoSection {
            heading: "Disc",
            fields: vec![
                ("Tracks", disc.tracks().len().to_string()),
                ("Audio Tracks", audio_tracks.to_string()),
                ("Lead-out LBA", disc.leadout_lba().to_string()),
                ("Layout", layout),
            ],
        }
    } else {
        RomInfoSection {
            heading: "HuCard",
            fields: vec![
                ("Board", format!("{:?}", backend.hucard_board())),
                ("ROM Size", format!("{} bytes", backend.hucard_rom().len())),
            ],
        }
    };
    let firmware = backend
        .firmware_manifests()
        .iter()
        .map(|manifest| format!("{manifest:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sections = vec![
        media,
        RomInfoSection {
            heading: "Machine",
            fields: vec![
                ("Topology", format!("{:?}", backend.hardware_topology())),
                ("Console Wiring", format!("{:?}", backend.console_wiring())),
                ("Controller", format!("{:?}", backend.controller_mode())),
                ("System Card", format!("{:?}", backend.hucard_board())),
            ],
        },
        RomInfoSection {
            heading: "Content",
            fields: vec![
                ("SHA-256", hash),
                ("ROM Path", backend.rom_path().display().to_string()),
                ("Source Path", backend.source_path().display().to_string()),
            ],
        },
    ];
    if !firmware.is_empty() {
        sections.push(RomInfoSection {
            heading: "Firmware",
            fields: vec![("Resolved", firmware)],
        });
    }
    RomDebugInfo { sections }
}

fn pce_cpu_snapshot(snapshot: zeff_pce_core::hardware::PceCpuDebugSnapshot) -> CpuDebugSnapshot {
    let registers = snapshot.registers();
    let mpr = snapshot.mapping_registers();
    let status = registers.status.bits();
    CpuDebugSnapshot {
        register_lines: vec![
            format!(
                "A:{:02X}  X:{:02X}  Y:{:02X}",
                registers.a, registers.x, registers.y
            ),
            format!(
                "PC:{:04X}  SP:{:02X}  P:{:02X}",
                registers.pc, registers.sp, status
            ),
            format!(
                "MPR:{:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                mpr[0], mpr[1], mpr[2], mpr[3], mpr[4], mpr[5], mpr[6], mpr[7]
            ),
        ],
        flags: vec![
            ('N', status & 0x80 != 0),
            ('V', status & 0x40 != 0),
            ('M', status & 0x20 != 0),
            ('B', status & 0x10 != 0),
            ('D', status & 0x08 != 0),
            ('I', status & 0x04 != 0),
            ('Z', status & 0x02 != 0),
            ('C', status & 0x01 != 0),
        ],
        status_text: if snapshot.faulted() {
            "State: faulted until reset".to_owned()
        } else {
            format!("State: {:?}", snapshot.speed_mode())
        },
        cpu_state: if snapshot.faulted() {
            "Faulted".to_owned()
        } else {
            format!("{:?}", snapshot.speed_mode())
        },
        pc: Address::from(registers.pc),
        cycles: snapshot.master_ticks(),
        last_opcode_line: "Instruction history is unavailable".to_owned(),
        sections: vec![
            DebugSection {
                heading: "Machine",
                lines: vec![format!(
                    "Master ticks: {}  VCE line: {}",
                    snapshot.master_ticks(),
                    snapshot.vce_line_index()
                )],
            },
            DebugSection {
                heading: "Debug limits",
                lines: vec![
                    "Read-only registers and ROM/WRAM peeks are available".to_owned(),
                    "Breakpoints, writes, disassembly, and stepping are unavailable".to_owned(),
                ],
            },
        ],
        io_registers: Vec::new(),
        recent_opcodes: Vec::new(),
        call_stack: Vec::new(),
        call_stack_available: false,
        breakpoints: Vec::new(),
        one_shot_breakpoints: Vec::new(),
        breakpoint_hit_conditions: Vec::new(),
        supported_events: Vec::new(),
        event_breakpoints: Vec::new(),
        rom_breakpoints: Vec::new(),
        watchpoints: Vec::new(),
        hit_breakpoint: None,
        hit_rom_breakpoint: None,
        hit_watchpoint: None,
        hit_event: None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zeff_pce_core::hardware::{
        CdDisc, CdTrack, CdTrackMode, PceConsoleWiring, PceHuCardBoard, SYSTEM_CARD_V1_V2_IMAGE_LEN,
    };

    use super::*;

    fn field<'a>(info: &'a RomDebugInfo, section: &str, name: &str) -> &'a str {
        info.sections
            .iter()
            .find(|candidate| candidate.heading == section)
            .and_then(|section| section.fields.iter().find(|(field, _)| *field == name))
            .map(|(_, value)| value.as_str())
            .unwrap()
    }

    #[test]
    fn hucard_rom_info_reports_board_size_and_wiring() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
        let info = pce_rom_info(&backend);

        assert_eq!(field(&info, "HuCard", "Board"), "Plain");
        assert_eq!(field(&info, "HuCard", "ROM Size"), "8192 bytes");
        assert_eq!(field(&info, "Machine", "Console Wiring"), "PcEngine");
        assert_eq!(field(&info, "Machine", "Controller"), "TwoButton");
        assert_eq!(field(&info, "Content", "ROM Path"), "game.pce");
    }

    #[test]
    fn cd_rom_info_reports_mixed_track_layout_and_system_card() {
        let disc = CdDisc::new(vec![
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])
                .unwrap(),
            CdTrack::from_index1_data(2, 0, None, 1, CdTrackMode::Audio, vec![0; 2352]).unwrap(),
        ])
        .unwrap();
        let backend = PceBackend::new_cdrom2(
            vec![0xEA; SYSTEM_CARD_V1_V2_IMAGE_LEN],
            disc,
            crate::emu_backend::pce::PceCdBackendConfig {
                system_card_board: PceHuCardBoard::SystemCardV3,
                cue_path: PathBuf::from("set.7z/disc.cue"),
                source_path: PathBuf::from("set.7z"),
                content_hash: [0xAB; 32],
                console_wiring: PceConsoleWiring::TurboGrafx16,
            },
        )
        .unwrap();
        let info = pce_rom_info(&backend);

        assert_eq!(field(&info, "Disc", "Tracks"), "2");
        assert_eq!(field(&info, "Disc", "Audio Tracks"), "1");
        assert!(field(&info, "Disc", "Layout").contains("02:Audio@1+1"));
        assert_eq!(field(&info, "Machine", "System Card"), "SystemCardV3");
        assert_eq!(field(&info, "Content", "Source Path"), "set.7z");
    }
}
