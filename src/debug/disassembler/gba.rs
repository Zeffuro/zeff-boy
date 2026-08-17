use super::{DisassembledLine, Mnemonic};

pub(super) fn disassemble_around(
    bus_read: &impl Fn(u32) -> u8,
    pc: u32,
    thumb: bool,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    let width = if thumb { 2 } else { 4 };
    let mut address = pc & !(width - 1);
    address = address.wrapping_sub((lines_before_pc as u32) * width);
    (0..total_lines)
        .map(|_| {
            let line = if thumb {
                decode_thumb(bus_read, address)
            } else {
                decode_arm(bus_read, address)
            };
            address = address.wrapping_add(line.bytes.len().max(width as usize) as u32);
            line
        })
        .collect()
}

fn decode_arm(bus_read: &impl Fn(u32) -> u8, address: u32) -> DisassembledLine {
    let raw = read_u32(bus_read, address);
    let mut line = line(
        address,
        [
            bus_read(address),
            bus_read(address.wrapping_add(1)),
            bus_read(address.wrapping_add(2)),
            bus_read(address.wrapping_add(3)),
        ],
    );
    if raw & 0x0FFF_FFF0 == 0x012F_FF10 {
        line.mnemonic = mn!("BX r{}", raw & 0xF);
    } else if raw & 0x0FC0_00F0 == 0x0000_0090 {
        let accumulate = raw & (1 << 21) != 0;
        line.mnemonic = if accumulate {
            mn!(
                "MLA r{}, r{}, r{}, r{}",
                (raw >> 16) & 0xF,
                raw & 0xF,
                (raw >> 8) & 0xF,
                (raw >> 12) & 0xF
            )
        } else {
            mn!(
                "MUL r{}, r{}, r{}",
                (raw >> 16) & 0xF,
                raw & 0xF,
                (raw >> 8) & 0xF
            )
        };
    } else if raw & 0x0E00_0000 == 0x0A00_0000 {
        let offset = ((raw << 8) as i32) >> 6;
        let target = address.wrapping_add(8).wrapping_add_signed(offset);
        line.mnemonic = mn!(
            "{}{} ${target:08X}",
            if raw & (1 << 24) != 0 { "BL" } else { "B" },
            arm_condition(raw >> 28)
        );
        set_target(&mut line, target);
    } else if raw & 0x0F00_0000 == 0x0F00_0000 {
        line.mnemonic = mn!("SWI #${:06X}", raw & 0x00FF_FFFF);
    } else if raw & 0x0C00_0000 == 0x0400_0000 {
        let load = if raw & (1 << 20) != 0 { "LDR" } else { "STR" };
        let byte = if raw & (1 << 22) != 0 { "B" } else { "" };
        let offset = if raw & (1 << 25) != 0 {
            mn!("r{}", raw & 0xF)
        } else {
            mn!("#${:03X}", raw & 0xFFF)
        };
        line.mnemonic = mn!(
            "{load}{byte} r{}, [r{}, {offset}]",
            (raw >> 12) & 0xF,
            (raw >> 16) & 0xF
        );
    } else if raw & 0x0E00_0000 == 0x0800_0000 {
        line.mnemonic = mn!(
            "{}{} r{}, {{...}}",
            if raw & (1 << 20) != 0 { "LDM" } else { "STM" },
            arm_condition(raw >> 28),
            (raw >> 16) & 0xF
        );
    } else if raw & 0x0C00_0000 == 0 {
        let opcode = [
            "AND", "EOR", "SUB", "RSB", "ADD", "ADC", "SBC", "RSC", "TST", "TEQ", "CMP", "CMN",
            "ORR", "MOV", "BIC", "MVN",
        ][((raw >> 21) & 0xF) as usize];
        let rhs = arm_operand2(raw);
        let rd = (raw >> 12) & 0xF;
        let rn = (raw >> 16) & 0xF;
        let test = matches!(opcode, "TST" | "TEQ" | "CMP" | "CMN");
        line.mnemonic = if test {
            mn!("{opcode}{} r{rn}, {rhs}", arm_condition(raw >> 28))
        } else if matches!(opcode, "MOV" | "MVN") {
            mn!("{opcode}{} r{rd}, {rhs}", arm_condition(raw >> 28))
        } else {
            mn!("{opcode}{} r{rd}, r{rn}, {rhs}", arm_condition(raw >> 28))
        };
    } else {
        line.mnemonic = mn!(".word ${raw:08X}");
    }
    line
}

fn decode_thumb(bus_read: &impl Fn(u32) -> u8, address: u32) -> DisassembledLine {
    let raw = read_u16(bus_read, address);
    if raw & 0xF800 == 0xF000 && read_u16(bus_read, address.wrapping_add(2)) & 0xF800 == 0xF800 {
        let next = read_u16(bus_read, address.wrapping_add(2));
        let high = (((raw & 0x07FF) as i32) << 21) >> 9;
        let low = i32::from(next & 0x07FF) << 1;
        let target = address.wrapping_add(4).wrapping_add_signed(high + low);
        let mut line = line(
            address,
            [
                bus_read(address),
                bus_read(address.wrapping_add(1)),
                bus_read(address.wrapping_add(2)),
                bus_read(address.wrapping_add(3)),
            ],
        );
        line.mnemonic = mn!("BL ${target:08X}");
        set_target(&mut line, target);
        return line;
    }

    let mut line = line(
        address,
        [bus_read(address), bus_read(address.wrapping_add(1))],
    );
    if raw & 0xF800 <= 0x1000 {
        let op = ["LSL", "LSR", "ASR"][((raw >> 11) & 3) as usize];
        line.mnemonic = mn!(
            "{op} r{}, r{}, #${:02X}",
            raw & 7,
            (raw >> 3) & 7,
            (raw >> 6) & 0x1F
        );
    } else if raw & 0xF800 == 0x1800 {
        let op = if raw & (1 << 9) != 0 { "SUB" } else { "ADD" };
        let rhs = if raw & (1 << 10) != 0 {
            mn!("#${:X}", (raw >> 6) & 7)
        } else {
            mn!("r{}", (raw >> 6) & 7)
        };
        line.mnemonic = mn!("{op} r{}, r{}, {rhs}", raw & 7, (raw >> 3) & 7);
    } else if raw & 0xF800 == 0x2000 {
        line.mnemonic = mn!("MOV r{}, #${:02X}", (raw >> 8) & 7, raw & 0xFF);
    } else if raw & 0xF800 == 0x2800 {
        line.mnemonic = mn!("CMP r{}, #${:02X}", (raw >> 8) & 7, raw & 0xFF);
    } else if raw & 0xF800 == 0x3000 {
        line.mnemonic = mn!("ADD r{}, #${:02X}", (raw >> 8) & 7, raw & 0xFF);
    } else if raw & 0xF800 == 0x3800 {
        line.mnemonic = mn!("SUB r{}, #${:02X}", (raw >> 8) & 7, raw & 0xFF);
    } else if raw & 0xFC00 == 0x4000 {
        let op = [
            "AND", "EOR", "LSL", "LSR", "ASR", "ADC", "SBC", "ROR", "TST", "NEG", "CMP", "CMN",
            "ORR", "MUL", "BIC", "MVN",
        ][((raw >> 6) & 0xF) as usize];
        let rd = raw & 7;
        let rs = (raw >> 3) & 7;
        line.mnemonic = if matches!(op, "TST" | "CMP" | "CMN") {
            mn!("{op} r{rd}, r{rs}")
        } else if op == "MVN" {
            mn!("MVN r{rd}, r{rs}")
        } else {
            mn!("{op} r{rd}, r{rs}")
        };
    } else if raw & 0xFC00 == 0x4400 {
        let op = ["ADD", "CMP", "MOV", "BX"][((raw >> 8) & 3) as usize];
        let rd = (raw & 7) | ((raw >> 4) & 8);
        let rs = (raw >> 3) & 0xF;
        line.mnemonic = if op == "BX" {
            mn!("BX r{rs}")
        } else {
            mn!("{op} r{rd}, r{rs}")
        };
    } else if raw & 0xF800 == 0x4800 {
        let target = address.wrapping_add(4) & !3;
        let target = target.wrapping_add(u32::from(raw & 0xFF) << 2);
        line.mnemonic = mn!("LDR r{}, [PC, #${:02X}]", (raw >> 8) & 7, (raw & 0xFF) << 2);
        set_target(&mut line, target);
    } else if raw & 0xF200 == 0x5000 {
        let load = raw & (1 << 11) != 0;
        let kind = match ((raw >> 10) & 1, (raw >> 9) & 1) {
            (0, 0) => "",
            (0, 1) => "B",
            (1, 0) => "H",
            (1, 1) => {
                if load {
                    "SH"
                } else {
                    "SB"
                }
            }
            _ => "",
        };
        line.mnemonic = mn!(
            "{}{} r{}, [r{}, r{}]",
            if load { "LDR" } else { "STR" },
            kind,
            raw & 7,
            (raw >> 3) & 7,
            (raw >> 6) & 7
        );
    } else if raw & 0xE000 == 0x6000 {
        let load = raw & (1 << 11) != 0;
        let byte = raw & (1 << 12) != 0;
        let offset = (raw >> 6) & 0x1F;
        let offset = if byte { offset } else { offset << 2 };
        line.mnemonic = mn!(
            "{}{} r{}, [r{}, #${offset:02X}]",
            if load { "LDR" } else { "STR" },
            if byte { "B" } else { "" },
            raw & 7,
            (raw >> 3) & 7
        );
    } else if raw & 0xF000 == 0x8000 {
        line.mnemonic = mn!(
            "{}H r{}, [r{}, #${:02X}]",
            if raw & (1 << 11) != 0 { "LDR" } else { "STR" },
            raw & 7,
            (raw >> 3) & 7,
            ((raw >> 6) & 0x1F) << 1
        );
    } else if raw & 0xF000 == 0x9000 {
        line.mnemonic = mn!(
            "{} r{}, [SP, #${:03X}]",
            if raw & (1 << 11) != 0 { "LDR" } else { "STR" },
            (raw >> 8) & 7,
            (raw & 0xFF) << 2
        );
    } else if raw & 0xF000 == 0xA000 {
        line.mnemonic = mn!(
            "ADD r{}, {}, #${:03X}",
            (raw >> 8) & 7,
            if raw & (1 << 11) != 0 { "SP" } else { "PC" },
            (raw & 0xFF) << 2
        );
    } else if raw & 0xFF00 == 0xB000 {
        line.mnemonic = mn!(
            "ADD SP, #{}${:02X}",
            if raw & (1 << 7) != 0 { "-" } else { "+" },
            (raw & 0x7F) << 2
        );
    } else if raw & 0xF600 == 0xB400 {
        let load = raw & (1 << 11) != 0;
        let extra = if raw & (1 << 8) != 0 {
            if load { ", PC" } else { ", LR" }
        } else {
            ""
        };
        line.mnemonic = mn!(
            "{} {{{:02X}{extra}}}",
            if load { "POP" } else { "PUSH" },
            raw & 0xFF
        );
    } else if raw & 0xF000 == 0xC000 {
        line.mnemonic = mn!(
            "{} r{}, {{{:02X}}}",
            if raw & (1 << 11) != 0 {
                "LDMIA"
            } else {
                "STMIA"
            },
            (raw >> 8) & 7,
            raw & 0xFF
        );
    } else if raw & 0xF800 == 0xE000 {
        let offset = (((raw & 0x07FF) as i32) << 21) >> 20;
        let target = address.wrapping_add(4).wrapping_add_signed(offset);
        line.mnemonic = mn!("B ${target:08X}");
        set_target(&mut line, target);
    } else if raw & 0xF000 == 0xD000 && raw & 0x0F00 != 0x0F00 {
        let offset = i32::from((raw & 0x00FF) as u8 as i8) << 1;
        let target = address.wrapping_add(4).wrapping_add_signed(offset);
        line.mnemonic = mn!("B{cond} ${target:08X}", cond = condition((raw >> 8) & 0xF));
        set_target(&mut line, target);
    } else if raw & 0xFF00 == 0xDF00 {
        line.mnemonic = mn!("SWI #${:02X}", raw & 0xFF);
    } else {
        line.mnemonic = mn!("THUMB ${raw:04X}");
    }
    line
}

fn line<const N: usize>(address: u32, bytes: [u8; N]) -> DisassembledLine {
    DisassembledLine {
        address,
        storage_offset: rom_offset(address),
        symbol: None,
        control_target: None,
        control_target_storage: None,
        control_target_symbol: None,
        source: None,
        bytes: bytes.into_iter().collect(),
        mnemonic: Mnemonic::new(),
    }
}

fn set_target(line: &mut DisassembledLine, target: u32) {
    line.control_target = Some(target);
    line.control_target_storage = rom_offset(target);
}

fn rom_offset(address: u32) -> Option<u64> {
    (0x0800_0000..=0x0DFF_FFFF)
        .contains(&address)
        .then(|| u64::from((address - 0x0800_0000) & 0x01FF_FFFF))
}

fn read_u16(bus_read: &impl Fn(u32) -> u8, address: u32) -> u16 {
    u16::from(bus_read(address)) | (u16::from(bus_read(address.wrapping_add(1))) << 8)
}

fn read_u32(bus_read: &impl Fn(u32) -> u8, address: u32) -> u32 {
    u32::from(read_u16(bus_read, address))
        | (u32::from(read_u16(bus_read, address.wrapping_add(2))) << 16)
}

fn condition(value: u16) -> &'static str {
    [
        "EQ", "NE", "CS", "CC", "MI", "PL", "VS", "VC", "HI", "LS", "GE", "LT", "GT", "LE", "", "",
    ][value as usize]
}

fn arm_condition(value: u32) -> &'static str {
    [
        "EQ", "NE", "CS", "CC", "MI", "PL", "VS", "VC", "HI", "LS", "GE", "LT", "GT", "LE", "", "",
    ][value as usize]
}

fn arm_operand2(raw: u32) -> Mnemonic {
    if raw & (1 << 25) != 0 {
        let immediate = raw & 0xFF;
        let rotate = ((raw >> 8) & 0xF) * 2;
        mn!("#${:X}", immediate.rotate_right(rotate))
    } else {
        mn!("r{}", raw & 0xF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_thumb_branch_with_rom_storage() {
        let bytes = [0x00, 0xE0, 0, 0];
        let lines = disassemble_around(
            &|address| bytes[(address - 0x0800_0000) as usize],
            0x0800_0000,
            true,
            0,
            1,
        );
        assert_eq!(lines[0].mnemonic.as_str(), "B $08000004");
        assert_eq!(lines[0].storage_offset, Some(0));
        assert_eq!(lines[0].control_target_storage, Some(4));
    }

    #[test]
    fn decodes_common_thumb_instructions() {
        let bytes = [0x91, 0x8B, 0x18, 0x1C, 0x40, 0x08, 0x00, 0x28, 0xFA, 0xD0];
        let lines = disassemble_around(
            &|address| bytes[(address - 0x0800_0000) as usize],
            0x0800_0000,
            true,
            0,
            5,
        );

        assert_eq!(lines[0].mnemonic.as_str(), "LDRH r1, [r2, #$1C]");
        assert_eq!(lines[1].mnemonic.as_str(), "ADD r0, r3, #$0");
        assert_eq!(lines[2].mnemonic.as_str(), "LSR r0, r0, #$01");
        assert_eq!(lines[3].mnemonic.as_str(), "CMP r0, #$00");
        assert_eq!(lines[4].mnemonic.as_str(), "BEQ $08000000");
    }

    #[test]
    fn decodes_arm_data_processing() {
        let bytes = 0xE3A0_0001u32.to_le_bytes();
        let lines = disassemble_around(
            &|address| bytes[(address - 0x0800_0000) as usize],
            0x0800_0000,
            false,
            0,
            1,
        );

        assert_eq!(lines[0].mnemonic.as_str(), "MOV r0, #$1");
    }
}
