use crate::debug::{DebugSection, IoBitDisplay, IoRegisterDisplay};
use zeff_md_core::Emulator;

pub(super) fn registers(emu: &Emulator) -> Vec<IoRegisterDisplay> {
    let data = [
        (0, "D0"),
        (1, "D1"),
        (2, "D2"),
        (3, "D3"),
        (4, "D4"),
        (5, "D5"),
        (6, "TH"),
        (7, "D7"),
    ];
    let control = [
        (0, "D0 out"),
        (1, "D1 out"),
        (2, "D2 out"),
        (3, "D3 out"),
        (4, "D4 out"),
        (5, "D5 out"),
        (6, "TH out"),
    ];
    [
        ("VERSION", 0xa10001, &[(7, "Overseas"), (6, "PAL")][..]),
        ("DATA1", 0xa10003, data.as_slice()),
        ("DATA2", 0xa10005, data.as_slice()),
        ("DATA EXT", 0xa10007, data.as_slice()),
        ("CTRL1", 0xa10009, control.as_slice()),
        ("CTRL2", 0xa1000b, control.as_slice()),
        ("CTRL EXT", 0xa1000d, control.as_slice()),
    ]
    .into_iter()
    .filter_map(|(name, address, bits)| {
        emu.peek(address).map(|value| IoRegisterDisplay {
            name,
            address,
            value: u32::from(value),
            width: 1,
            writable_mask: if matches!(address, 0xa10009 | 0xa1000b | 0xa1000d) {
                0x7f
            } else {
                0
            },
            bits: bits
                .iter()
                .map(|&(bit, label)| IoBitDisplay {
                    mask: 1 << bit,
                    label,
                })
                .collect(),
        })
    })
    .collect()
}

fn vdp_section(values: [Option<u8>; 24]) -> DebugSection {
    let mut lines = Vec::new();
    if let Some(mode) = values[1] {
        lines.push(format!(
            "Display enable:{}  VBlank IRQ enable:{}  DMA enable:{}  Mode 5:{}",
            u8::from(mode & 0x40 != 0),
            u8::from(mode & 0x20 != 0),
            u8::from(mode & 0x10 != 0),
            u8::from(mode & 4 != 0),
        ));
    }
    for (group, values) in values.chunks(4).enumerate() {
        lines.push(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let value = value.map_or_else(|| "--".into(), |value| format!("{value:02X}"));
                    format!("R{:02}:{value}", group * 4 + index)
                })
                .collect::<Vec<_>>()
                .join("  "),
        );
    }
    DebugSection {
        heading: "VDP registers",
        lines,
    }
}

pub(super) fn sound_section(emu: &Emulator) -> DebugSection {
    let cpu = emu.sound_cpu();
    let registers = cpu.regs();
    DebugSection {
        heading: "SOUND Z80 (read-only published state)",
        lines: vec![
            format!(
                "AF:{:04X}  BC:{:04X}  DE:{:04X}  HL:{:04X}",
                registers.af(),
                registers.bc(),
                registers.de(),
                registers.hl()
            ),
            format!(
                "IX:{:04X}  IY:{:04X}  SP:{:04X}  PC:{:04X}",
                registers.ix, registers.iy, registers.sp, registers.pc
            ),
            format!(
                "I:{:02X}  R:{:02X}  Cycles:{}",
                registers.i,
                registers.r,
                cpu.cycles()
            ),
            format!(
                "Published state:{:?}  IM:{:?}  IFF1:{}  IFF2:{}  EI pending:{}",
                cpu.state(),
                cpu.interrupt_mode(),
                u8::from(cpu.interrupts_enabled()),
                u8::from(cpu.saved_interrupts_enabled()),
                u8::from(cpu.interrupt_enable_pending())
            ),
            format!(
                "Prefix pending:{}  Instruction pending:{}",
                u8::from(emu.sound_prefix_pending()),
                u8::from(emu.sound_instruction_pending())
            ),
            format!(
                "Segment deadline:{}",
                emu.sound_deadline().map_or_else(
                    || "Unknown".into(),
                    |deadline| format!("{} MCLK", deadline.get())
                )
            ),
        ],
    }
}

pub(super) fn sections(emu: &Emulator) -> [DebugSection; 3] {
    [
        vdp_section(emu.vdp_registers()),
        DebugSection {
            heading: "Bus and interrupts",
            lines: vec![
                format!("MAIN IRQ:{}", emu.interrupt_level()),
                format!(
                    "SOUND request:{}  granted:{}  reset:{}",
                    u8::from(emu.sound_bus_requested()),
                    u8::from(emu.sound_bus_granted()),
                    u8::from(emu.sound_reset_asserted())
                ),
                format!(
                    "SOUND bank:{:03X}  Prefix pending:{}  Instruction pending:{}",
                    emu.sound_bank(),
                    u8::from(emu.sound_prefix_pending()),
                    u8::from(emu.sound_instruction_pending())
                ),
                format!(
                    "SOUND deadline:{}",
                    emu.sound_deadline().map_or_else(
                        || "--".into(),
                        |deadline| format!("{} MCLK", deadline.get())
                    )
                ),
            ],
        },
        sound_section(emu),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn unknown_vdp_registers_remain_distinct_from_known_zero() {
        let mut values = [None; 24];
        values[0] = Some(0);
        values[23] = Some(0xa5);
        let section = vdp_section(values);
        assert_eq!(section.lines.len(), 6);
        assert_eq!(section.lines[0], "R00:00  R01:--  R02:--  R03:--");
        assert_eq!(section.lines[5], "R20:--  R21:--  R22:--  R23:A5");
        values[1] = Some(0);
        let section = vdp_section(values);
        assert_eq!(section.lines.len(), 7);
        assert_eq!(
            section.lines[0],
            "Display enable:0  VBlank IRQ enable:0  DMA enable:0  Mode 5:0"
        );
    }
}
