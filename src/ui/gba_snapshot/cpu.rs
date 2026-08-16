use zeff_emu_common::address::Address;

pub(super) fn gba_cpu_snapshot(
    emu: &zeff_gba_core::emulator::Emulator,
) -> crate::debug::CpuDebugSnapshot {
    let regs = emu.cpu_registers();
    let cpsr = emu.cpu_cpsr();
    let pc = emu.cpu_pc();
    let ppu = emu.ppu_debug_snapshot();
    let mem_around_pc: [(Address, u8); 32] = std::array::from_fn(|i| {
        let addr = pc.wrapping_add(i as u32);
        (addr, emu.cpu_peek8(addr))
    });
    let debug_controls = super::super::build_debug_control_snapshot(
        emu.iter_breakpoints(),
        emu.debug_watchpoints()
            .iter()
            .map(|watch| (watch.address, watch.watch_type)),
        emu.debug_hit_breakpoint(),
        emu.debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.old_value, hit.new_value, hit.watch_type)),
    );
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
        mem_around_pc,
        recent_opcodes,
        breakpoints: debug_controls.breakpoints,
        rom_breakpoints: Vec::new(),
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_rom_breakpoint: None,
        hit_watchpoint: debug_controls.hit_watchpoint,
    }
}
