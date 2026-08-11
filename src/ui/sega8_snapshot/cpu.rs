use super::{on_off, sega8_system_label};
use crate::debug::{CpuDebugSnapshot, DebugSection, RecentOpcodeDisplay};
use zeff_emu_common::address::Address;
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::constants::{
    SMS_VISIBLE_SCANLINES, Z80_FLAG_BIT_3, Z80_FLAG_BIT_5, Z80_FLAG_CARRY, Z80_FLAG_HALF_CARRY,
    Z80_FLAG_PARITY_OVERFLOW, Z80_FLAG_SIGN, Z80_FLAG_SUBTRACT, Z80_FLAG_ZERO,
};

const RECENT_OPCODE_LINE_COUNT: usize = 16;

pub(super) fn sega8_cpu_snapshot(emu: &Emulator) -> CpuDebugSnapshot {
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
    let recent_opcodes = super::super::build_recent_opcode_display(
        emu.recent_opcodes(RECENT_OPCODE_LINE_COUNT),
        RECENT_OPCODE_LINE_COUNT,
        |(pc, opcode, cycles), repeat_count| RecentOpcodeDisplay {
            address: Address::from(pc),
            bytes: vec![opcode],
            detail: Some(format!("{cycles} cyc")),
            repeat_count,
        },
    );
    let debug_controls = super::super::build_debug_control_snapshot(
        emu.iter_breakpoints(),
        emu.debug_watchpoints()
            .iter()
            .map(|watch| (watch.address, watch.watch_type)),
        emu.debug_hit_breakpoint(),
        emu.debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.old_value, hit.new_value, hit.watch_type)),
    );

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
                    format!("memory_control={:02X}", emu.bus().memory_control()),
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
                        "video_standard={} console_region={} scanlines={} cycles/frame={}",
                        emu.video_standard().display_label(),
                        emu.console_region().display_label(),
                        emu.bus().vdp().total_scanlines(),
                        emu.video_standard().cycles_per_frame()
                    ),
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
        recent_opcodes,
        breakpoints: debug_controls.breakpoints,
        watchpoints: debug_controls.watchpoints,
        hit_breakpoint: debug_controls.hit_breakpoint,
        hit_watchpoint: debug_controls.hit_watchpoint,
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
