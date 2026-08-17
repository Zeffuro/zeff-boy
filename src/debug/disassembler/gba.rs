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
    } else if raw & 0x0E00_0000 == 0x0A00_0000 {
        let offset = ((raw << 8) as i32) >> 6;
        let target = address.wrapping_add(8).wrapping_add_signed(offset);
        line.mnemonic = mn!(
            "{} ${target:08X}",
            if raw & (1 << 24) != 0 { "BL" } else { "B" }
        );
        set_target(&mut line, target);
    } else if raw & 0x0F00_0000 == 0x0F00_0000 {
        line.mnemonic = mn!("SWI #${:06X}", raw & 0x00FF_FFFF);
    } else if raw & 0x0C00_0000 == 0x0400_0000 {
        let load = if raw & (1 << 20) != 0 { "LDR" } else { "STR" };
        let byte = if raw & (1 << 22) != 0 { "B" } else { "" };
        line.mnemonic = mn!(
            "{load}{byte} r{}, [r{}]",
            (raw >> 12) & 0xF,
            (raw >> 16) & 0xF
        );
    } else if raw & 0x0E00_0000 == 0x0800_0000 {
        line.mnemonic = mn!(
            "{} r{}, {{...}}",
            if raw & (1 << 20) != 0 { "LDM" } else { "STM" },
            (raw >> 16) & 0xF
        );
    } else if raw & 0x0C00_0000 == 0 {
        line.mnemonic = mn!("ARM ${raw:08X}");
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
    if raw & 0xF800 == 0xE000 {
        let offset = (((raw & 0x07FF) as i32) << 21) >> 20;
        let target = address.wrapping_add(4).wrapping_add_signed(offset);
        line.mnemonic = mn!("B ${target:08X}");
        set_target(&mut line, target);
    } else if raw & 0xF000 == 0xD000 && raw & 0x0F00 != 0x0F00 {
        let offset = i32::from((raw & 0x00FF) as u8 as i8) << 1;
        let target = address.wrapping_add(4).wrapping_add_signed(offset);
        line.mnemonic = mn!("B{cond} ${target:08X}", cond = condition((raw >> 8) & 0xF));
        set_target(&mut line, target);
    } else if raw & 0xFF87 == 0x4700 {
        line.mnemonic = mn!("BX r{}", (raw >> 3) & 0xF);
    } else if raw & 0xF800 == 0x4800 {
        line.mnemonic = mn!("LDR r{}, [PC, #${:02X}]", (raw >> 8) & 7, (raw & 0xFF) << 2);
    } else if raw & 0xF800 == 0x2000 {
        line.mnemonic = mn!("MOV r{}, #${:02X}", (raw >> 8) & 7, raw & 0xFF);
    } else if raw & 0xF800 == 0x3000 {
        line.mnemonic = mn!("ADD r{}, #${:02X}", (raw >> 8) & 7, raw & 0xFF);
    } else if raw & 0xF800 == 0x3800 {
        line.mnemonic = mn!("SUB r{}, #${:02X}", (raw >> 8) & 7, raw & 0xFF);
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
}
