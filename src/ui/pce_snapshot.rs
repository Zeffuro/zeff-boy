use std::borrow::Cow;

use crate::debug::{
    ApuChannelDebug, ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugSection,
    DisassemblyTarget, InputDebugInfo, OamDebugInfo, PaletteDebugInfo, PaletteGroupDebug,
    PaletteRowDebug, PceGraphicsData, PceVdcGraphicsData, PerfInfo, RomDebugInfo, RomInfoSection,
    huc6280_disassemble_around,
};
use crate::emu_backend::PceBackend;
use crate::emu_backend::pce::PceGraphicsSnapshot;
use crate::emu_core_trait::EmulatorCore;
use crate::emu_thread::SnapshotRequest;
use crate::symbols::{DebugAddressResolver, pce::pce_cpu_location};
use zeff_emu_common::{address::Address, debug::DebugEvent};
use zeff_pce_core::hardware::{
    ArcadeCardDebugSnapshot, CdRom2DebugSnapshot, ControllerDeviceDebugSnapshot,
    MultitapDeviceDebugSnapshot, PceHardwareDebugSnapshot, PsgChannelDebugSnapshot,
    VdcDebugSnapshot, VdcRegister,
};

pub(crate) fn collect_pce_snapshot(
    backend: &PceBackend,
    request: &SnapshotRequest,
    reusable_memory_page: Option<Vec<(Address, u8)>>,
) -> super::UiFrameData {
    let rom = backend.hucard_rom();
    let cpu_snapshot = backend.debug_cpu_snapshot();
    let hardware_snapshot = (request.want_debug_info
        || request.show_apu_viewer
        || request.show_oam_viewer
        || request.any_viewer_open)
        .then(|| backend.debug_hardware_snapshot());
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
    let cpu_debug = request.want_debug_info.then(|| {
        pce_cpu_snapshot(
            backend,
            cpu_snapshot,
            hardware_snapshot.as_ref().unwrap(),
            super::opcodes::pce_recent_opcode_display(
                backend.recent_opcodes(super::opcodes::RECENT_OPCODE_LINE_COUNT),
            ),
        )
    });
    let rom_debug = request.show_rom_info.then(|| pce_rom_info(backend));

    let mut data = super::UiFrameData {
        perf_info: request.want_perf_info.then(|| PerfInfo {
            fps: 0.0,
            target_fps: zeff_emu_common::system::System::Pce.target_fps(),
            speed_mode_label: super::normal_speed_mode_label(),
            frames_in_flight: 0,
            cycles: cpu_snapshot.master_ticks(),
            platform_name: "PC Engine",
            hardware_label: format!(
                "{:?} / {:?}",
                backend.hardware_topology(),
                backend.hucard_board()
            )
            .into(),
            hardware_pref_label: format!("{:?}", backend.console_wiring()).into(),
        }),
        cpu_debug,
        memory_page,
        memory_search_results,
        rom_page: super::build_rom_page(request.show_rom_viewer, request.rom_view_start, rom),
        rom_search_results: super::build_rom_search(request.rom_search.as_ref(), rom),
        rom_size: rom.len() as u32,
        rom_debug,
        input_debug: request.want_debug_info.then(|| {
            pce_input_snapshot(
                &hardware_snapshot.as_ref().unwrap().controller,
                backend.controller_mode(),
                backend.memory_base_mode(),
            )
        }),
        apu_debug: request
            .show_apu_viewer
            .then(|| pce_apu_snapshot(backend, hardware_snapshot.as_ref().unwrap())),
        oam_debug: request
            .show_oam_viewer
            .then(|| pce_oam_snapshot(hardware_snapshot.as_ref().unwrap())),
        palette_debug: request
            .any_viewer_open
            .then(|| pce_palette_snapshot(&hardware_snapshot.as_ref().unwrap().vce)),
        graphics_data: request
            .any_vram_viewer_open
            .then(|| pce_graphics_snapshot(backend.debug_graphics_snapshot())),
        ..Default::default()
    };
    let runtime_pc = cpu_snapshot.registers().pc;
    let disasm_target = request.disasm_target.filter(|target| {
        u16::try_from(target.cpu_address).is_ok()
            && target.bank.is_none_or(|bank| u8::try_from(bank).is_ok())
            && (target.bank.is_some() || target.storage_offset.is_some())
    });
    let pc = disasm_target
        .and_then(|target| u16::try_from(target.cpu_address).ok())
        .unwrap_or(runtime_pc);
    let mapping = disasm_target.map_or_else(
        || Some(backend.rom_mapping_token()),
        |target| {
            target
                .storage_offset
                .or_else(|| target.bank.map(|bank| (1_u64 << 63) | u64::from(bank)))
        },
    );
    data.disassembly_view = super::build_disassembly_view(
        request.show_disassembler,
        request
            .last_disasm_pc
            .map(|pc| (pc, request.last_disasm_mapping)),
        (pc.into(), mapping),
        || {
            huc6280_disassemble_around(
                |addr| {
                    disasm_target.map_or_else(
                        || backend.debug_peek8(addr.into()),
                        |target| pce_target_byte(backend, target, addr),
                    )
                },
                pc,
                12,
                26,
            )
        },
        std::iter::empty(),
        std::iter::empty(),
    )
    .map(|mut view| {
        view.is_navigation_target = disasm_target.is_some();
        view.is_static_target = disasm_target.is_some();
        for line in &mut view.lines {
            if let Ok(address) = u16::try_from(line.address) {
                if let Some(target) = disasm_target {
                    line.storage_offset = pce_target_storage_offset(target, address);
                    line.bank =
                        pce_target_physical_address(target, address).map(|physical| physical >> 13);
                } else {
                    let location = backend.resolve_exec(pce_cpu_location(address));
                    line.storage_offset = location.storage.map(|storage| storage.offset);
                    line.bank = location.bank;
                }
            }
            if let Some(target) = line.control_target
                && let Ok(address) = u16::try_from(target)
            {
                if let Some(target) = disasm_target {
                    line.control_target_storage = pce_target_storage_offset(target, address);
                    line.control_target_bank =
                        pce_target_physical_address(target, address).map(|physical| physical >> 13);
                } else {
                    let location = backend.resolve_exec(pce_cpu_location(address));
                    line.control_target_storage = location.storage.map(|storage| storage.offset);
                    line.control_target_bank = location.bank;
                }
            }
        }
        view
    });
    data
}

fn pce_graphics_snapshot(snapshot: PceGraphicsSnapshot) -> ConsoleGraphicsData {
    let vdc = |vdc: crate::emu_backend::pce::PceVdcGraphicsSnapshot| PceVdcGraphicsData {
        vram: vdc.vram,
        registers: vdc.registers,
    };
    ConsoleGraphicsData::Pce(Box::new(PceGraphicsData {
        vdc1: vdc(snapshot.vdc1),
        vdc2: snapshot.vdc2.map(vdc),
        palette: snapshot.palette,
    }))
}

fn pce_target_physical_address(target: DisassemblyTarget, address: u16) -> Option<u32> {
    let target_cpu = u16::try_from(target.cpu_address).ok()?;
    let page = u8::try_from(target.bank?).ok()?;
    let base = zeff_pce_core::hardware::physical_address_for_page(target_cpu, page);
    let delta = address.wrapping_sub(target_cpu) as i16;
    Some(base.wrapping_add_signed(i32::from(delta)) & 0x1F_FFFF)
}

fn pce_target_storage_offset(target: DisassemblyTarget, address: u16) -> Option<u64> {
    let target_cpu = u16::try_from(target.cpu_address).ok()?;
    let delta = i64::from(address.wrapping_sub(target_cpu) as i16);
    target.storage_offset?.checked_add_signed(delta)
}

fn pce_target_byte(backend: &PceBackend, target: DisassemblyTarget, address: u16) -> u8 {
    pce_target_physical_address(target, address).map_or_else(
        || {
            pce_target_storage_offset(target, address)
                .and_then(|offset| usize::try_from(offset).ok())
                .and_then(|offset| backend.hucard_rom().get(offset).copied())
                .unwrap_or(0xFF)
        },
        |physical| backend.debug_peek_physical8(physical),
    )
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
                ("Arcade Card", format!("{:?}", backend.arcade_card_mode())),
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
    if let Some(title) = backend.canonical_title_metadata() {
        sections.insert(
            1,
            RomInfoSection {
                heading: "Canonical Title",
                fields: vec![
                    ("ID", title.id.to_owned()),
                    ("Title", title.title.to_owned()),
                    ("Region", title.region.to_owned()),
                    ("Controller", format!("{:?}", title.controller_mode)),
                    ("Memory Base 128", on_off(title.memory_base_128).to_owned()),
                    ("Arcade Card", on_off(title.arcade_card).to_owned()),
                    (
                        "Minimum System Card",
                        title
                            .minimum_system_card
                            .map_or_else(|| "Unknown".to_owned(), |tier| format!("{tier:?}")),
                    ),
                ],
            },
        );
    }
    if !firmware.is_empty() {
        sections.push(RomInfoSection {
            heading: "Firmware",
            fields: vec![("Resolved", firmware)],
        });
    }
    RomDebugInfo { sections }
}

fn pce_cpu_snapshot(
    backend: &PceBackend,
    snapshot: zeff_pce_core::hardware::PceCpuDebugSnapshot,
    hardware: &PceHardwareDebugSnapshot,
    recent_opcodes: Vec<crate::debug::RecentOpcodeDisplay>,
) -> CpuDebugSnapshot {
    let registers = snapshot.registers();
    let mpr = snapshot.mapping_registers();
    let status = registers.status.bits();
    let debug_controls = super::build_debug_control_snapshot(super::DebugControlSources {
        breakpoints: backend.iter_breakpoints(),
        one_shot_breakpoints: backend.iter_one_shot_breakpoints(),
        breakpoint_hit_conditions: backend.iter_breakpoint_hit_conditions(),
        event_breakpoints: backend.iter_event_breakpoints(),
        watchpoints: backend
            .debug_watchpoints()
            .iter()
            .map(|watch| (watch.address, watch.end_address, watch.watch_type)),
        hit_breakpoint: backend.debug_hit_breakpoint(),
        hit_watchpoint: backend
            .debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.old_value, hit.new_value, hit.watch_type)),
        hit_event: backend.debug_hit_event(),
    });
    CpuDebugSnapshot {
        register_lines: vec![
            format!(
                "A:{:02X}  X:{:02X}  Y:{:02X}",
                registers.a, registers.x, registers.y
            ),
            format!(
                "PC:{:04X}  PHYS:{:06X}  SP:{:02X}  P:{:02X}",
                registers.pc,
                snapshot.physical_pc(),
                registers.sp,
                status
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
            format!(
                "State: {:?}  Speed: {:?}",
                snapshot.execution_state(),
                snapshot.speed_mode()
            )
        },
        cpu_state: if snapshot.faulted() {
            "Faulted".to_owned()
        } else {
            format!("{:?}", snapshot.execution_state())
        },
        pc: Address::from(registers.pc),
        cycles: snapshot.master_ticks(),
        last_opcode_line: recent_opcodes
            .first()
            .map(crate::debug::RecentOpcodeDisplay::line)
            .unwrap_or_else(|| "No completed instruction".to_owned()),
        sections: {
            let mut sections = vec![
                DebugSection {
                    heading: "Machine",
                    lines: vec![format!(
                        "Master ticks: {}  VCE line: {}",
                        snapshot.master_ticks(),
                        snapshot.vce_line_index()
                    )],
                },
                DebugSection {
                    heading: "Timer",
                    lines: vec![format!(
                        "Counter: {:02X}  Reload: {:02X}  Running: {}  Prescaler: {}",
                        snapshot.timer_counter(),
                        snapshot.timer_reload(),
                        snapshot.timer_running(),
                        snapshot.timer_prescaler_ticks()
                    )],
                },
                DebugSection {
                    heading: "Interrupts",
                    lines: vec![format!(
                        "Disable: {:02X}  Request: {:02X}  Sampled: {:?}",
                        snapshot.irq_disable(),
                        snapshot.irq_request(),
                        snapshot.sampled_interrupt()
                    )],
                },
            ];
            sections.extend(pce_video_sections(hardware));
            if let Some(cdrom2) = &hardware.cdrom2 {
                sections.extend(pce_cd_sections(cdrom2));
            }
            if let Some(arcade_card) = &hardware.arcade_card {
                sections.push(pce_arcade_card_section(arcade_card));
            }
            sections.push(DebugSection {
                heading: "Debug limits",
                lines: vec![
                    "Side-effect-free CPU/hardware snapshots and logical debugger writes are available"
                        .to_owned(),
                    "Logical CPU breakpoints, watchpoints, execution controls, and opcode history are available"
                        .to_owned(),
                    "MPR changes remap storage without changing logical breakpoint/watchpoint addresses"
                        .to_owned(),
                    "IRQ / NMI event breakpoints and HuC6280 instruction trace are available"
                        .to_owned(),
                    "DMA event breakpoints remain unavailable".to_owned(),
                ],
            });
            sections
        },
        io_registers: Vec::new(),
        recent_opcodes,
        call_stack: Vec::new(),
        call_stack_available: false,
        breakpoints: debug_controls.breakpoints,
        one_shot_breakpoints: debug_controls.one_shot_breakpoints,
        breakpoint_hit_conditions: debug_controls.breakpoint_hit_conditions,
        supported_events: vec![DebugEvent::Interrupt, DebugEvent::Dma],
        event_breakpoints: debug_controls.event_breakpoints,
        rom_breakpoints: Vec::new(),
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_rom_breakpoint: None,
        hit_watchpoint: debug_controls.hit_watchpoint,
        hit_event: debug_controls.hit_event,
    }
}

fn pce_arcade_card_section(arcade: &ArcadeCardDebugSnapshot) -> DebugSection {
    let mut lines = arcade
        .ports
        .iter()
        .enumerate()
        .map(|(index, port)| {
            format!(
                "P{index} base={:06X} offset={:04X} inc={:04X} control={:02X} effective={:06X}",
                port.base, port.offset, port.increment, port.control, port.effective_address
            )
        })
        .collect::<Vec<_>>();
    lines.push(format!(
        "ALU={:08X} shift={:02X} rotate={:02X}",
        arcade.value, arcade.shift, arcade.rotate
    ));
    DebugSection {
        heading: "Arcade Card",
        lines,
    }
}

fn pce_video_sections(hardware: &PceHardwareDebugSnapshot) -> Vec<DebugSection> {
    let mut sections = vec![pce_vdc_section("VDC1", &hardware.vdc)];
    if let Some(vdc2) = &hardware.vdc2 {
        sections.push(pce_vdc_section("VDC2", vdc2));
    }
    sections.push(DebugSection {
        heading: "VCE",
        lines: vec![
            format!(
                "CR={:02X} CTA={:03X} clock={:?} frame={} lines",
                hardware.vce.control,
                hardware.vce.color_table_address,
                hardware.vce.pixel_clock,
                hardware.vce.frame_length.scanlines()
            ),
            format!(
                "blur={} monochrome={}",
                on_off(hardware.vce.blur_enabled),
                on_off(hardware.vce.monochrome_enabled)
            ),
        ],
    });
    if let Some(vpc) = hardware.vpc {
        sections.push(DebugSection {
            heading: "VPC",
            lines: vec![
                format!(
                    "PRIO={:02X} {:02X} WINDOW={:03X} {:03X}",
                    vpc.priority_control[0],
                    vpc.priority_control[1],
                    vpc.window_width[0],
                    vpc.window_width[1]
                ),
                format!("Direct CPU VDC target: {:?}", vpc.direct_vdc),
            ],
        });
    }
    sections
}

fn pce_vdc_section(heading: &'static str, vdc: &VdcDebugSnapshot) -> DebugSection {
    let r = |register: VdcRegister| vdc.registers[register as usize];
    DebugSection {
        heading,
        lines: vec![
            format!(
                "SEL={:02X}({:?}) STATUS={:02X} IRQ={} READ={:04X}",
                vdc.selected_register_id,
                vdc.selected_register,
                vdc.status.bits(),
                asserted(vdc.irq_asserted),
                vdc.vram_read_buffer
            ),
            format!(
                "MAWR={:04X} MARR={:04X} VWR={:04X} CR={:04X} RCR={:04X}",
                r(VdcRegister::MemoryAddressWrite),
                r(VdcRegister::MemoryAddressRead),
                r(VdcRegister::VramData),
                r(VdcRegister::Control),
                r(VdcRegister::RasterCounter)
            ),
            format!(
                "BXR={:04X} BYR={:04X} MWR={:04X} HSR={:04X} HDR={:04X}",
                r(VdcRegister::BackgroundScrollX),
                r(VdcRegister::BackgroundScrollY),
                r(VdcRegister::MemoryWidth),
                r(VdcRegister::HorizontalSync),
                r(VdcRegister::HorizontalDisplay)
            ),
            format!(
                "VPR={:04X} VDW={:04X} VCR={:04X} DCR={:04X} SATB={:04X}",
                r(VdcRegister::VerticalSync),
                r(VdcRegister::VerticalDisplay),
                r(VdcRegister::VerticalDisplayEnd),
                r(VdcRegister::DmaControl),
                r(VdcRegister::SatbSource)
            ),
            format!(
                "SOUR={:04X} DESR={:04X} LENR={:04X}",
                r(VdcRegister::DmaSource),
                r(VdcRegister::DmaDestination),
                r(VdcRegister::DmaLength)
            ),
            format!(
                "H={:?} rem={} dma_phase={} burst={}  V={:?} {}/{} frame={} raster={}",
                vdc.horizontal_phase,
                vdc.horizontal_pixels_remaining,
                vdc.dma_pixel_remainder,
                on_off(vdc.frame_burst),
                vdc.vertical_phase,
                vdc.vertical_phase_line,
                vdc.vertical_phase_duration,
                vdc.frame_line,
                vdc.raster_counter
            ),
            format!(
                "sync out H={} V={} VRAM DMA={} SATB DMA={}",
                on_off(vdc.sync_output.horizontal()),
                on_off(vdc.sync_output.vertical()),
                vdc.active_vram_dma
                    .or(vdc.pending_vram_dma)
                    .map(|dma| format!(
                        "{:04X}->{:04X} rem={} {:?}/{:?}",
                        dma.source(),
                        dma.destination(),
                        dma.remaining_words(),
                        dma.source_direction(),
                        dma.destination_direction()
                    ))
                    .unwrap_or_else(|| "idle".into()),
                vdc.active_satb_dma
                    .or(vdc.pending_satb_dma)
                    .map(|dma| format!(
                        "{:04X} word={} rem={}",
                        dma.source(),
                        dma.next_word(),
                        dma.remaining_words()
                    ))
                    .unwrap_or_else(|| "idle".into())
            ),
        ],
    }
}

fn pce_cd_sections(cd: &CdRom2DebugSnapshot) -> Vec<DebugSection> {
    let event = cd.pending_event.map_or_else(
        || "none".to_owned(),
        |event| format!("{:?}/{}", event.kind, event.ticks_remaining),
    );
    let command = hex_bytes(&cd.command);
    let last_command = cd
        .recent_commands
        .last()
        .map(|command| hex_bytes(command.bytes()))
        .unwrap_or_else(|| "--".into());
    vec![
        DebugSection {
            heading: "CD/SCSI",
            lines: vec![
                format!(
                    "phase={:?} BUS={:02X} REQ={} ACK={} auto={} pending={} reset={}",
                    cd.phase,
                    cd.bus_status,
                    on_off(cd.request),
                    on_off(cd.acknowledge),
                    on_off(cd.auto_acknowledge),
                    on_off(cd.request_pending),
                    on_off(cd.reset_asserted)
                ),
                format!(
                    "data in={:02X} reg={:02X} out={:02X} command={} last={}",
                    cd.current_input_data, cd.data_register, cd.output_latch, command, last_command
                ),
                format!(
                    "response={}/{} status={:02X} sense={:02X}/{:02X} event={}",
                    cd.response_index,
                    cd.response_available,
                    cd.status,
                    cd.sense_key,
                    cd.additional_sense_code,
                    event
                ),
                format!(
                    "sectors={} arrival={:?} remainder={} commands={}",
                    cd.sectors_pending,
                    cd.sector_arrival_ticks,
                    cd.sector_tick_remainder,
                    cd.recent_commands.len()
                ),
            ],
        },
        DebugSection {
            heading: "CD IRQ",
            lines: vec![
                format!(
                    "IRQ2={} BRAM={} DRDY enable={} condition={} STATUS enable={} condition={}",
                    asserted(cd.irq2_asserted),
                    if cd.bram_unlocked {
                        "unlocked"
                    } else {
                        "locked"
                    },
                    on_off(cd.data_ready_irq_enabled),
                    on_off(cd.data_ready_condition),
                    on_off(cd.status_irq_enabled),
                    on_off(cd.status_condition)
                ),
                format!(
                    "ADPCM end enable={} flag={} half enable={} flag={}",
                    on_off(cd.audio_end_irq_enabled),
                    on_off(cd.adpcm_end_irq),
                    on_off(cd.audio_half_irq_enabled),
                    on_off(cd.adpcm_half_irq)
                ),
            ],
        },
        pce_cdda_section(cd),
        pce_adpcm_section(cd),
    ]
}

fn pce_cdda_section(cd: &CdRom2DebugSnapshot) -> DebugSection {
    DebugSection {
        heading: "CDDA",
        lines: vec![
            format!(
                "{:?} LBA {}..{} current={} sample={} end={:?}",
                cd.audio.status,
                cd.audio.start_lba,
                cd.audio.end_lba,
                cd.audio.current_lba,
                cd.audio.current_sample,
                cd.audio.end_mode
            ),
            format!(
                "sample L={} R={} latch={}({}) queued={} tick={}",
                cd.audio_left_sample,
                cd.audio_right_sample,
                cd.audio_sample_latch,
                if cd.audio_sample_latch_right {
                    "R"
                } else {
                    "L"
                },
                cd.audio.queued_source_frames,
                cd.audio.tick_accumulator
            ),
            format!(
                "fade={:02X} target={:?} level={:05X} step={} next={} rate={} generation={}",
                cd.audio.fade_control,
                cd.audio.fade_target,
                cd.audio.fade_level_q16,
                cd.audio.fade_step_ticks,
                cd.audio.fade_ticks_to_next,
                cd.audio_sample_rate,
                on_off(cd.audio_sample_generation_enabled)
            ),
        ],
    }
}

fn pce_adpcm_section(cd: &CdRom2DebugSnapshot) -> DebugSection {
    DebugSection {
        heading: "ADPCM",
        lines: vec![
            format!(
                "playing={} stop_pending={} length={} latch={:04X} read={:04X} write={:04X}",
                on_off(cd.adpcm_playing),
                on_off(cd.adpcm_stop_pending),
                cd.adpcm_length,
                cd.adpcm_address_latch,
                cd.adpcm_read_address,
                cd.adpcm_write_address
            ),
            format!(
                "D7={:02X} DMA={:02X} rate={:02X} read_buf={:02X} next_nibble={}",
                cd.adpcm_address_control,
                cd.adpcm_dma_control,
                cd.adpcm_playback_rate,
                cd.adpcm_read_buffer,
                if cd.adpcm_high_nibble_next {
                    "high"
                } else {
                    "low"
                }
            ),
            format!(
                "predictor={:04X} step={} clock={} buffered_samples={}",
                cd.adpcm_predictor,
                cd.adpcm_step_index,
                cd.adpcm_clock_accumulator,
                cd.adpcm_buffered_samples
            ),
        ],
    }
}

fn pce_apu_snapshot(backend: &PceBackend, hardware: &PceHardwareDebugSnapshot) -> ApuDebugInfo {
    let psg = hardware.psg;
    let channels = psg
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            pce_psg_channel(
                index,
                channel,
                psg.channel_mutes[index],
                backend.psg_channel_debug_samples_ordered(index),
            )
        })
        .collect();
    let mut extra_sections = vec![DebugSection {
        heading: "PSG Wave RAM",
        lines: psg
            .channels
            .iter()
            .enumerate()
            .map(|(index, channel)| format!("CH{index}: {}", hex_bytes(&channel.waveform)))
            .collect(),
    }];
    if let Some(cdrom2) = &hardware.cdrom2 {
        extra_sections.push(pce_cdda_section(cdrom2));
        extra_sections.push(pce_adpcm_section(cdrom2));
    }
    ApuDebugInfo {
        master_lines: vec![
            format!(
                "revision={:?} selected={} main={:02X} LFO freq={:02X} ctrl={:02X} depth={} active={}",
                psg.revision,
                psg.selected_channel,
                psg.main_amplitude,
                psg.lfo_frequency,
                psg.lfo_control,
                psg.lfo_control & 3,
                on_off(psg.lfo_control & 0x80 == 0 && psg.lfo_control & 3 != 0)
            ),
            format!(
                "sample_rate={} buffered={} generation={} mix=[{}, {}] resampler=[{}, {}] @{}",
                psg.sample_rate,
                psg.buffered_sample_frames,
                on_off(psg.sample_generation_enabled),
                psg.mixed_output[0],
                psg.mixed_output[1],
                psg.resampler_levels[0],
                psg.resampler_levels[1],
                psg.resampler_clock
            ),
            format!(
                "gain_scan active={} queued={} clock={} latch={} master_phase={} capture={} history={}",
                on_off(psg.gain_scan_active),
                on_off(psg.gain_scan_queued),
                psg.gain_scan_clock,
                psg.attenuation_latch,
                psg.master_tick_remainder,
                on_off(psg.debug_capture_enabled),
                psg.debug_waveform_samples
            ),
        ],
        master_waveform: backend.psg_master_debug_samples_ordered(),
        channels,
        extra_sections,
    }
}

fn pce_psg_channel(
    index: usize,
    channel: &PsgChannelDebugSnapshot,
    muted: bool,
    waveform: Vec<f32>,
) -> ApuChannelDebug {
    let noise = index >= 4 && channel.noise_control & 0x80 != 0;
    let mode = if channel.control & 0x40 != 0 {
        "DDA"
    } else if noise {
        "noise"
    } else {
        "wave"
    };
    ApuChannelDebug {
        name: match index {
            0 => "PSG 0 Wave/LFO",
            1 => "PSG 1 Wave/LFO",
            2 => "PSG 2 Wave",
            3 => "PSG 3 Wave",
            4 => "PSG 4 Wave/Noise",
            _ => "PSG 5 Wave/Noise",
        },
        enabled: channel.control & 0x80 != 0,
        muted,
        register_lines: vec![
            format!(
                "freq={:03X} ctrl={:02X} balance={:02X} wave_pos={} dda={:02X}",
                channel.frequency,
                channel.control,
                channel.balance,
                channel.wave_index,
                channel.dda_hold
            ),
            format!(
                "noise={:02X} wave_counter={} noise_counter={} seed={:05X}",
                channel.noise_control,
                channel.wave_counter,
                channel.noise_counter,
                channel.noise_seed
            ),
        ],
        detail_line: format!(
            "mode={} freq={}Hz amp={} attenuation=L{} R{}",
            mode,
            if noise {
                "noise".into()
            } else {
                format!("{:.1}", psg_frequency_hz(channel.frequency))
            },
            channel.control & 0x1F,
            channel.effective_left_attenuation,
            channel.effective_right_attenuation
        ),
        waveform,
    }
}

fn pce_input_snapshot(
    controller: &zeff_pce_core::hardware::ControllerPortDebugSnapshot,
    configured_mode: zeff_pce_core::hardware::PceControllerMode,
    memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode,
) -> InputDebugInfo {
    let mut sections = vec![DebugSection {
        heading: "Controller Port",
        lines: vec![format!(
            "configured={configured_mode:?} SEL={} CLR={} nibble={:X} (active-low)",
            if controller.select_high { "H" } else { "L" },
            if controller.clear_high { "H" } else { "L" },
            controller.input_nibble
        )],
    }];
    sections.extend(match controller.device {
        ControllerDeviceDebugSnapshot::Disconnected => vec![DebugSection {
            heading: "Device",
            lines: vec!["Disconnected".into()],
        }],
        ControllerDeviceDebugSnapshot::TwoButton { buttons } => vec![DebugSection {
            heading: "Two-button Pad",
            lines: vec![format!("pressed={}", pad_buttons_label(buttons))],
        }],
        ControllerDeviceDebugSnapshot::SixButton {
            buttons,
            extra_buttons,
            phase,
        } => vec![DebugSection {
            heading: "Six-button Pad",
            lines: vec![
                format!("pressed={}", pad_buttons_label(buttons)),
                format!("extra={extra_buttons:?} phase={phase:?}"),
            ],
        }],
        ControllerDeviceDebugSnapshot::Multitap { active_port, ports } => vec![DebugSection {
            heading: "Five-port Multitap",
            lines: std::iter::once(format!("active={active_port:?}"))
                .chain(ports.into_iter().enumerate().map(|(index, device)| {
                    format!("P{}: {}", index + 1, multitap_device_label(device))
                }))
                .collect(),
        }],
        ControllerDeviceDebugSnapshot::Mouse(mouse) => vec![DebugSection {
            heading: "Mouse",
            lines: vec![
                format!(
                    "buttons={} pending=({}, {}) latched=({:02X}, {:02X})",
                    pad_buttons_label(mouse.buttons),
                    mouse.pending_x,
                    mouse.pending_y,
                    mouse.latched_x,
                    mouse.latched_y
                ),
                format!(
                    "scan={} phase={:?} select_ticks={} clear_ticks={}",
                    on_off(mouse.scan_active),
                    mouse.phase,
                    mouse.select_elapsed,
                    mouse.clear_elapsed
                ),
            ],
        }],
    });
    let memory_base = controller.memory_base;
    sections.push(DebugSection {
        heading: "Memory Base 128",
        lines: vec![
            format!(
                "configured={memory_base_mode:?} connected={} active={} phase={:?}",
                on_off(memory_base.connected),
                on_off(memory_base.active),
                memory_base.phase
            ),
            format!(
                "address={:05X} remaining_bits={} bit={} output={:X} dirty={}",
                memory_base.address,
                memory_base.remaining_bits,
                memory_base.bit_index,
                memory_base.output_nibble,
                on_off(memory_base.dirty)
            ),
        ],
    });
    InputDebugInfo {
        sections,
        progress_bars: Vec::new(),
    }
}

fn pce_palette_snapshot(vce: &zeff_pce_core::hardware::VceDebugSnapshot) -> PaletteDebugInfo {
    PaletteDebugInfo {
        groups: [
            ("Background palettes", 0usize),
            ("Sprite palettes", 0x100usize),
        ]
        .into_iter()
        .map(|(title, base)| PaletteGroupDebug {
            title: Cow::Borrowed(title),
            rows: (0..16)
                .map(|palette| PaletteRowDebug {
                    label: format!("{palette:X}"),
                    colors: (0..16)
                        .map(|color| {
                            let [red, green, blue] =
                                vce.palette[base + palette * 16 + color].rgb8();
                            [red, green, blue, 0xFF]
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect(),
    }
}

fn pce_oam_snapshot(hardware: &PceHardwareDebugSnapshot) -> OamDebugInfo {
    let mut rows = pce_vdc_oam_rows("1", &hardware.vdc);
    if let Some(vdc2) = &hardware.vdc2 {
        rows.extend(pce_vdc_oam_rows("2", vdc2));
    }
    OamDebugInfo {
        headers: &[
            "VDC", "#", "X", "Y", "Pattern", "Size", "Palette", "Priority", "Flip", "Raw",
        ],
        rows,
    }
}

fn pce_vdc_oam_rows(vdc_name: &str, vdc: &VdcDebugSnapshot) -> Vec<Vec<String>> {
    vdc.satb
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(index, words)| {
            let attributes = words[3];
            let width = if attributes & 0x0100 == 0 { 16 } else { 32 };
            let height = match (attributes >> 12) & 3 {
                0 => "16",
                1 => "32",
                2 => "invalid",
                _ => "64",
            };
            vec![
                vdc_name.to_owned(),
                format!("{index:02}"),
                (i32::from(words[1] & 0x03FF) - 32).to_string(),
                (i32::from(words[0] & 0x03FF) - 64).to_string(),
                format!("{:03X}", words[2] & 0x07FF),
                format!("{width}x{height}"),
                format!("{:X}", attributes & 0x000F),
                if attributes & 0x0080 == 0 {
                    "BG"
                } else {
                    "OBJ"
                }
                .into(),
                match (attributes & 0x0800 != 0, attributes & 0x8000 != 0) {
                    (false, false) => "-",
                    (true, false) => "H",
                    (false, true) => "V",
                    (true, true) => "HV",
                }
                .into(),
                format!(
                    "{:04X} {:04X} {:04X} {:04X}",
                    words[0], words[1], words[2], words[3]
                ),
            ]
        })
        .collect()
}

fn multitap_device_label(device: MultitapDeviceDebugSnapshot) -> String {
    match device {
        MultitapDeviceDebugSnapshot::Disconnected => "disconnected".into(),
        MultitapDeviceDebugSnapshot::TwoButton { buttons } => {
            format!("two-button [{}]", pad_buttons_label(buttons))
        }
        MultitapDeviceDebugSnapshot::SixButton {
            buttons,
            extra_buttons,
            phase,
        } => format!(
            "six-button [{}] extra={extra_buttons:?} phase={phase:?}",
            pad_buttons_label(buttons)
        ),
    }
}

fn pad_buttons_label(buttons: zeff_pce_core::hardware::PadButtons) -> String {
    use zeff_pce_core::hardware::PadButtons;
    let names = [
        (PadButtons::UP, "Up"),
        (PadButtons::RIGHT, "Right"),
        (PadButtons::DOWN, "Down"),
        (PadButtons::LEFT, "Left"),
        (PadButtons::I, "I"),
        (PadButtons::II, "II"),
        (PadButtons::SELECT, "Select"),
        (PadButtons::RUN, "Run"),
    ];
    let label = names
        .into_iter()
        .filter_map(|(button, name)| buttons.contains(button).then_some(name))
        .collect::<Vec<_>>()
        .join("+");
    if label.is_empty() { "-".into() } else { label }
}

fn psg_frequency_hz(frequency: u16) -> f64 {
    let divider = if frequency == 0 {
        f64::from(zeff_pce_core::hardware::PSG_ZERO_FREQUENCY_PERIOD)
    } else {
        f64::from(frequency)
    };
    (zeff_pce_core::hardware::PSG_CLOCK_NUMERATOR as f64
        / zeff_pce_core::hardware::PSG_CLOCK_DENOMINATOR as f64)
        / (divider * 32.0)
}

fn hex_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "--".into();
    }
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn asserted(value: bool) -> &'static str {
    if value { "asserted" } else { "clear" }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::emu_backend::pce_profiles::LEMMINGS_JAPAN_CANONICAL_DISC_SHA256;
    use crate::emu_core_trait::DebuggableEmulator;
    use crate::emu_thread::{RenderSettings, ReusableBuffers};
    use crate::settings::{ColorCorrection, DmgPalettePreset, NesPaletteMode};
    use zeff_emu_common::debug::TraceExecMode;
    use zeff_emu_common::time::FrameLifecycle;
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

    fn disassembly_request() -> SnapshotRequest {
        SnapshotRequest {
            want_debug_info: false,
            want_perf_info: false,
            any_viewer_open: false,
            any_vram_viewer_open: false,
            show_oam_viewer: false,
            show_apu_viewer: false,
            show_disassembler: true,
            show_rom_info: false,
            show_memory_viewer: false,
            memory_view_start: 0,
            show_rom_viewer: false,
            show_instruction_trace: false,
            trace_after_sequence: None,
            rom_view_start: 0,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: None,
            memory_search: None,
            rom_search: None,
            render: RenderSettings {
                color_correction: ColorCorrection::None,
                color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                dmg_palette_preset: DmgPalettePreset::default(),
                nes_palette_mode: NesPaletteMode::default(),
                nes_custom_palette: None,
                pce_overscan_mode: crate::settings::PceOverscanMode::default(),
                pce_palette_mode: crate::settings::PcePaletteMode::default(),
                sgb_border_enabled: false,
            },
        }
    }

    fn debugger_request() -> SnapshotRequest {
        let mut request = disassembly_request();
        request.want_debug_info = true;
        request.any_viewer_open = true;
        request.show_oam_viewer = true;
        request.show_apu_viewer = true;
        request.show_disassembler = false;
        request
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
    fn snapshot_populates_pce_performance_info() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
        let mut request = disassembly_request();
        request.show_disassembler = false;
        request.want_perf_info = true;

        let perf = collect_pce_snapshot(&backend, &request, None)
            .perf_info
            .unwrap();
        assert_eq!(perf.platform_name, "PC Engine");
        assert!(perf.hardware_label.contains("Base"));
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
                content_crc32: 0xAB,
                source_disc_hash: LEMMINGS_JAPAN_CANONICAL_DISC_SHA256,
                console_wiring: PceConsoleWiring::TurboGrafx16,
                arcade_card_mode: zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
            },
        )
        .unwrap();
        let info = pce_rom_info(&backend);

        assert_eq!(field(&info, "Disc", "Tracks"), "2");
        assert_eq!(field(&info, "Disc", "Audio Tracks"), "1");
        assert!(field(&info, "Disc", "Layout").contains("02:Audio@1+1"));
        assert_eq!(field(&info, "Machine", "System Card"), "SystemCardV3");
        assert_eq!(field(&info, "Content", "Source Path"), "set.7z");
        assert_eq!(field(&info, "Canonical Title", "ID"), "pce-cd:jp:lemmings");
        assert_eq!(field(&info, "Canonical Title", "Title"), "Lemmings");
    }

    #[test]
    fn snapshot_builds_huc6280_disassembly_with_rom_offsets() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0] = 0x73;
        rom[1..7].copy_from_slice(&[0x34, 0x12, 0x78, 0x56, 0xBC, 0x9A]);
        rom[0x1FFE..].copy_from_slice(&0x0000_u16.to_le_bytes());
        let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
        let request = disassembly_request();

        let data = collect_pce_snapshot(&backend, &request, None);
        let view = data.disassembly_view.unwrap();
        let line = view
            .lines
            .iter()
            .find(|line| line.address == Address::from(0u16))
            .unwrap();
        assert_eq!(line.mnemonic.as_str(), "TII $1234,$5678,$9ABC");
        assert_eq!(line.storage_offset, Some(0));
        assert_eq!(line.bank, Some(0));
    }

    #[test]
    fn snapshot_exposes_pce_execution_history() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0] = 0xD4;
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let mut backend = PceBackend::new(rom, PathBuf::from("history.pce")).unwrap();
        backend.set_opcode_history_enabled(true);
        backend.debug_suspend();
        backend.debug_step();
        backend.step_frame();

        let data = collect_pce_snapshot(&backend, &debugger_request(), None);
        let cpu = data.cpu_debug.unwrap();
        assert_eq!(cpu.cpu_state, "Suspended");
        assert_eq!(cpu.recent_opcodes[0].address, 0xE000);
        assert_eq!(cpu.recent_opcodes[0].bytes, [0xD4]);
        assert!(cpu.last_opcode_line.contains("E000"));
    }

    #[test]
    fn snapshot_exposes_logical_breakpoint_and_watchpoint_controls() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let mut backend = PceBackend::new(rom, PathBuf::from("debug.pce")).unwrap();
        DebuggableEmulator::add_breakpoint(&mut backend, 0xE000);
        DebuggableEmulator::add_one_shot_breakpoint(&mut backend, 0xE010);
        DebuggableEmulator::add_breakpoint_after(&mut backend, 0xE020, 4);
        DebuggableEmulator::add_watchpoint_range(
            &mut backend,
            0x2000,
            0x200F,
            zeff_emu_common::debug::WatchType::ReadWrite,
        );
        DebuggableEmulator::set_event_breakpoint(&mut backend, DebugEvent::Interrupt, true);
        DebuggableEmulator::set_event_breakpoint(&mut backend, DebugEvent::Dma, true);

        backend.step_frame();
        let data = collect_pce_snapshot(&backend, &debugger_request(), None);
        let cpu = data.cpu_debug.unwrap();

        assert_eq!(cpu.breakpoints, [0xE000, 0xE010, 0xE020]);
        assert_eq!(cpu.one_shot_breakpoints, [0xE010]);
        assert_eq!(cpu.breakpoint_hit_conditions[0].address, 0xE020);
        assert_eq!(cpu.breakpoint_hit_conditions[0].target_hits, 4);
        assert_eq!(cpu.watchpoints[0].address, 0x2000);
        assert_eq!(cpu.watchpoints[0].end_address, 0x200F);
        assert_eq!(cpu.hit_breakpoint, Some(0xE000));
        assert_eq!(
            cpu.supported_events,
            [DebugEvent::Interrupt, DebugEvent::Dma]
        );
        assert_eq!(
            cpu.event_breakpoints,
            [DebugEvent::Interrupt, DebugEvent::Dma]
        );
    }

    #[test]
    fn backend_snapshot_collects_page_qualified_huc6280_trace() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let mut backend = PceBackend::new(rom, PathBuf::from("trace.pce")).unwrap();
        DebuggableEmulator::set_instruction_trace_enabled(&mut backend, true);
        backend.debug_suspend();
        backend.debug_step();
        backend.step_frame();
        let mut backend = crate::emu_backend::EmuBackend::from_pce(backend);
        let mut request = disassembly_request();
        request.show_instruction_trace = true;

        let data = crate::ui::collect_backend_snapshot(
            &mut backend,
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

        let trace = data.instruction_trace.unwrap();
        assert!(trace.enabled);
        assert_eq!(trace.retained, 1);
        assert_eq!(trace.entries[0].mode, TraceExecMode::HuC6280);
        assert_eq!(trace.entries[0].pc, 0xE000);
        assert_eq!(trace.entries[0].bank, Some(0));
        assert_eq!(trace.entries[0].physical_rom_offset, Some(0));
        assert_eq!(trace.entries[0].instruction_bytes(), &[0xEA]);
    }

    #[test]
    fn snapshot_emits_base_vdc_graphics_when_a_vram_viewer_is_open() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let backend = PceBackend::new(rom, PathBuf::from("graphics.pce")).unwrap();
        let mut request = debugger_request();
        request.any_vram_viewer_open = true;

        let data = collect_pce_snapshot(&backend, &request, None);
        let Some(ConsoleGraphicsData::Pce(graphics)) = data.graphics_data else {
            panic!("PCE graphics snapshot missing");
        };
        assert_eq!(
            graphics.vdc1.vram.len(),
            zeff_pce_core::hardware::VDC_VRAM_WORDS
        );
        assert!(graphics.vdc2.is_none());
        assert_eq!(
            graphics.palette.len(),
            zeff_pce_core::hardware::VCE_PALETTE_COLORS
        );
    }

    #[test]
    fn graphics_snapshot_keeps_supergrafx_vdc2_separate() {
        let snapshot = PceGraphicsSnapshot {
            vdc1: crate::emu_backend::pce::PceVdcGraphicsSnapshot {
                vram: vec![0x1111],
                registers: [1; 0x14],
            },
            vdc2: Some(crate::emu_backend::pce::PceVdcGraphicsSnapshot {
                vram: vec![0x2222],
                registers: [2; 0x14],
            }),
            palette: [zeff_pce_core::hardware::VceColor::new(0); 512],
        };

        let ConsoleGraphicsData::Pce(graphics) = pce_graphics_snapshot(snapshot) else {
            unreachable!();
        };
        assert_eq!(graphics.vdc1.vram, [0x1111]);
        assert_eq!(graphics.vdc2.unwrap().vram, [0x2222]);
    }

    #[test]
    fn static_disassembly_target_reads_the_selected_physical_page() {
        let mut rom = vec![0xEA; 0x4000];
        rom[0x2000..0x2002].copy_from_slice(&[0xA9, 0x42]);
        rom[0x1FFE..0x2000].copy_from_slice(&0x0000_u16.to_le_bytes());
        let backend = PceBackend::new(rom, PathBuf::from("game.pce")).unwrap();
        let mut request = disassembly_request();
        request.disasm_target = Some(DisassemblyTarget {
            cpu_address: 0xA000,
            storage_offset: None,
            bank: Some(1),
            thumb: None,
        });

        let data = collect_pce_snapshot(&backend, &request, None);
        let view = data.disassembly_view.unwrap();
        let line = view
            .lines
            .iter()
            .find(|line| line.address == Address::from(0xA000u16))
            .unwrap();
        assert_eq!(line.mnemonic.as_str(), "LDA #$42");
        assert_eq!(line.bank, Some(1));
        assert!(view.is_static_target);
    }

    #[test]
    fn snapshot_populates_pce_hardware_debug_windows() {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let mut backend = PceBackend::new(rom, PathBuf::from("hardware.pce")).unwrap();
        backend.set_apu_debug_capture_enabled(true);
        backend.step_frame();

        let data = collect_pce_snapshot(&backend, &debugger_request(), None);
        let cpu = data.cpu_debug.unwrap();
        assert!(cpu.sections.iter().any(|section| section.heading == "VDC1"));
        assert!(cpu.sections.iter().any(|section| section.heading == "VCE"));

        let apu = data.apu_debug.unwrap();
        assert_eq!(apu.channels.len(), 6);
        assert!(!apu.master_waveform.is_empty());
        assert!(
            apu.channels
                .iter()
                .all(|channel| channel.waveform.len() == apu.master_waveform.len())
        );
        assert_eq!(apu.extra_sections[0].heading, "PSG Wave RAM");
        assert_eq!(apu.extra_sections[0].lines.len(), 6);

        let input = data.input_debug.unwrap();
        assert_eq!(input.sections[0].heading, "Controller Port");
        assert_eq!(input.sections[1].heading, "Two-button Pad");

        let palette = data.palette_debug.unwrap();
        assert_eq!(palette.groups.len(), 2);
        assert!(palette.groups.iter().all(|group| group.rows.len() == 16));

        let oam = data.oam_debug.unwrap();
        assert_eq!(oam.rows.len(), 64);
        assert_eq!(oam.rows[0][0], "1");
    }

    #[test]
    fn cd_snapshot_exposes_transport_cdda_and_adpcm_state() {
        let disc = CdDisc::new(vec![
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])
                .unwrap(),
        ])
        .unwrap();
        let backend = PceBackend::new_cdrom2(
            vec![0xEA; SYSTEM_CARD_V1_V2_IMAGE_LEN],
            disc,
            crate::emu_backend::pce::PceCdBackendConfig {
                system_card_board: PceHuCardBoard::SystemCardV1V2,
                cue_path: PathBuf::from("disc.cue"),
                source_path: PathBuf::from("disc.cue"),
                content_hash: [0xCD; 32],
                content_crc32: 0xCD,
                source_disc_hash: [0xCD; 32],
                console_wiring: PceConsoleWiring::PcEngine,
                arcade_card_mode: zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
            },
        )
        .unwrap();

        let data = collect_pce_snapshot(&backend, &debugger_request(), None);
        let cpu = data.cpu_debug.unwrap();
        for heading in ["CD/SCSI", "CD IRQ", "CDDA", "ADPCM"] {
            assert!(
                cpu.sections
                    .iter()
                    .any(|section| section.heading == heading)
            );
        }
        let apu = data.apu_debug.unwrap();
        assert!(
            apu.extra_sections
                .iter()
                .any(|section| section.heading == "CDDA")
        );
        assert!(
            apu.extra_sections
                .iter()
                .any(|section| section.heading == "ADPCM")
        );
    }
}
