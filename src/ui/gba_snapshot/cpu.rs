pub(super) fn gba_cpu_snapshot(
    emu: &zeff_gba_core::emulator::Emulator,
) -> crate::debug::CpuDebugSnapshot {
    let regs = emu.cpu_registers();
    let cpsr = emu.cpu_cpsr();
    let pc = emu.cpu_pc();
    let ppu = emu.ppu_debug_snapshot();
    let debug_controls =
        super::super::build_debug_control_snapshot(super::super::DebugControlSources {
            breakpoints: emu.iter_breakpoints(),
            one_shot_breakpoints: emu.iter_one_shot_breakpoints(),
            breakpoint_hit_conditions: emu.iter_breakpoint_hit_conditions(),
            event_breakpoints: emu.iter_event_breakpoints(),
            watchpoints: emu
                .debug_watchpoints()
                .iter()
                .map(|watch| (watch.address, watch.end_address, watch.watch_type)),
            hit_breakpoint: emu.debug_hit_breakpoint(),
            hit_watchpoint: emu
                .debug_hit_watchpoint()
                .map(|hit| (hit.address, hit.old_value, hit.new_value, hit.watch_type)),
            hit_event: emu.debug_hit_event(),
        });
    let recent_opcodes = super::super::opcodes::gba_recent_opcode_display(
        emu.recent_opcodes(super::super::opcodes::RECENT_OPCODE_LINE_COUNT),
    );

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
        pc,
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
                        super::on_off(ppu.bg_enabled[0]),
                        super::on_off(ppu.bg_enabled[1]),
                        super::on_off(ppu.bg_enabled[2]),
                        super::on_off(ppu.bg_enabled[3]),
                        super::on_off(ppu.obj_enabled),
                        if ppu.obj_mapping_1d { "1D" } else { "2D" }
                    ),
                    format!(
                        "Debug masks: BG={} BG0={} BG1={} BG2={} BG3={} Window={} OBJ={}",
                        super::on_off(ppu.debug_flags.bg),
                        super::on_off(ppu.debug_flags.bg_layers[0]),
                        super::on_off(ppu.debug_flags.bg_layers[1]),
                        super::on_off(ppu.debug_flags.bg_layers[2]),
                        super::on_off(ppu.debug_flags.bg_layers[3]),
                        super::on_off(ppu.debug_flags.window),
                        super::on_off(ppu.debug_flags.sprites)
                    ),
                    format!(
                        "BGCNT: {:04X} {:04X} {:04X} {:04X}",
                        ppu.bgcnt[0], ppu.bgcnt[1], ppu.bgcnt[2], ppu.bgcnt[3]
                    ),
                    format!("Non-black framebuffer pixels: {}", ppu.non_black_pixels),
                ],
            },
        ],
        io_registers: gba_io_registers(emu, ppu.dispcnt),
        recent_opcodes,
        call_stack: Vec::new(),
        call_stack_available: false,
        breakpoints: debug_controls.breakpoints,
        one_shot_breakpoints: debug_controls.one_shot_breakpoints,
        breakpoint_hit_conditions: debug_controls.breakpoint_hit_conditions,
        supported_events: vec![
            zeff_emu_common::debug::DebugEvent::Interrupt,
            zeff_emu_common::debug::DebugEvent::Dma,
        ],
        event_breakpoints: debug_controls.event_breakpoints,
        rom_breakpoints: Vec::new(),
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_rom_breakpoint: None,
        hit_watchpoint: debug_controls.hit_watchpoint,
        hit_event: debug_controls.hit_event,
    }
}

fn gba_io_registers(
    emu: &zeff_gba_core::emulator::Emulator,
    dispcnt: u16,
) -> Vec<crate::debug::IoRegisterDisplay> {
    let mut registers = vec![
        io_register(
            "DISPCNT",
            0x0400_0000,
            dispcnt,
            0xFFF7,
            &[
                (15, "OBJ win"),
                (14, "Win 1"),
                (13, "Win 0"),
                (12, "OBJ"),
                (11, "BG3"),
                (10, "BG2"),
                (9, "BG1"),
                (8, "BG0"),
                (7, "Blank"),
                (6, "OBJ 1D"),
            ],
        ),
        io_register(
            "DISPSTAT",
            0x0400_0004,
            read16(emu, 0x0400_0004),
            0xFF38,
            &[
                (5, "VCount IRQ"),
                (4, "HBlank IRQ"),
                (3, "VBlank IRQ"),
                (2, "VCount"),
                (1, "HBlank"),
                (0, "VBlank"),
            ],
        ),
        io_register("VCOUNT", 0x0400_0006, read16(emu, 0x0400_0006), 0, &[]),
    ];

    for (index, address) in [0x0400_0102, 0x0400_0106, 0x0400_010A, 0x0400_010E]
        .into_iter()
        .enumerate()
    {
        registers.push(io_register(
            ["TM0CNT", "TM1CNT", "TM2CNT", "TM3CNT"][index],
            address,
            read16(emu, address),
            0x00C7,
            &[
                (7, "Enable"),
                (6, "IRQ"),
                (2, "Count-up"),
                (1, "Clock 1"),
                (0, "Clock 0"),
            ],
        ));
    }

    for (index, address) in [0x0400_00BA, 0x0400_00C6, 0x0400_00D2, 0x0400_00DE]
        .into_iter()
        .enumerate()
    {
        let bits: &[(u16, &'static str)] = if index == 3 {
            &[
                (15, "Enable"),
                (14, "IRQ"),
                (11, "DRQ"),
                (10, "Word"),
                (9, "Repeat"),
            ]
        } else {
            &[(15, "Enable"), (14, "IRQ"), (10, "Word"), (9, "Repeat")]
        };
        registers.push(io_register(
            ["DMA0CNT", "DMA1CNT", "DMA2CNT", "DMA3CNT"][index],
            address,
            read16(emu, address),
            0xFFE0,
            bits,
        ));
    }

    registers.extend([
        io_register(
            "IE",
            0x0400_0200,
            read16(emu, 0x0400_0200),
            0x3FFF,
            &interrupt_bits(),
        ),
        io_register(
            "IF",
            0x0400_0202,
            read16(emu, 0x0400_0202),
            0,
            &interrupt_bits(),
        ),
        io_register(
            "WAITCNT",
            0x0400_0204,
            read16(emu, 0x0400_0204),
            0x7FFF,
            &[(14, "Prefetch")],
        ),
        io_register(
            "IME",
            0x0400_0208,
            read16(emu, 0x0400_0208),
            1,
            &[(0, "Master IRQ")],
        ),
        io_register("KEYINPUT", 0x0400_0130, read16(emu, 0x0400_0130), 0, &[]),
        io_register(
            "KEYCNT",
            0x0400_0132,
            read16(emu, 0x0400_0132),
            0xC3FF,
            &[(15, "AND"), (14, "IRQ")],
        ),
    ]);
    registers
}

fn read16(emu: &zeff_gba_core::emulator::Emulator, address: u32) -> u16 {
    u16::from_le_bytes([emu.cpu_peek8(address), emu.cpu_peek8(address + 1)])
}

fn interrupt_bits() -> [(u16, &'static str); 14] {
    [
        (13, "GamePak"),
        (12, "Keypad"),
        (11, "DMA3"),
        (10, "DMA2"),
        (9, "DMA1"),
        (8, "DMA0"),
        (7, "Serial"),
        (6, "Timer3"),
        (5, "Timer2"),
        (4, "Timer1"),
        (3, "Timer0"),
        (2, "VCount"),
        (1, "HBlank"),
        (0, "VBlank"),
    ]
}

fn io_register(
    name: &'static str,
    address: u32,
    value: u16,
    writable_mask: u16,
    bits: &[(u16, &'static str)],
) -> crate::debug::IoRegisterDisplay {
    crate::debug::IoRegisterDisplay {
        name,
        address,
        value: value.into(),
        width: 2,
        writable_mask: writable_mask.into(),
        bits: bits
            .iter()
            .map(|&(bit, label)| crate::debug::IoBitDisplay {
                mask: 1 << bit,
                label,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_io_covers_video_dma_timers_and_interrupts() {
        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        let emu = zeff_gba_core::emulator::Emulator::new(&rom, 48_000).unwrap();
        let registers = gba_io_registers(&emu, 0);
        for name in [
            "DISPCNT", "DISPSTAT", "DMA3CNT", "TM3CNT", "IE", "IF", "IME",
        ] {
            assert!(registers.iter().any(|register| register.name == name));
        }
        assert_eq!(
            registers
                .iter()
                .find(|register| register.name == "IF")
                .unwrap()
                .writable_mask,
            0
        );
    }
}
