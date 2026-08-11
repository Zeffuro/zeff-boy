use crate::debug::RecentOpcodeDisplay;
use zeff_emu_common::address::Address;
use zeff_gba_core::emulator::GbaOpcodeRecord;
use zeff_ws_core::emulator::WsOpcodeRecord;

pub(super) const RECENT_OPCODE_LINE_COUNT: usize = 16;

pub(super) fn gb_recent_opcode_display(
    entries: impl IntoIterator<Item = (u16, u8, bool)>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(entries, RECENT_OPCODE_LINE_COUNT, |entry, repeat_count| {
        let (pc, opcode, cb_prefix) = entry;
        RecentOpcodeDisplay {
            address: Address::from(pc),
            bytes: if cb_prefix {
                vec![0xCB, opcode]
            } else {
                vec![opcode]
            },
            detail: None,
            repeat_count,
        }
    })
}

pub(super) fn gba_recent_opcode_display(
    entries: impl IntoIterator<Item = GbaOpcodeRecord>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(entries, RECENT_OPCODE_LINE_COUNT, |record, repeat_count| {
        RecentOpcodeDisplay {
            address: Address::from(record.pc),
            bytes: gba_opcode_bytes(record.raw, record.width_bytes),
            detail: Some(format!(
                "{:?} fetch={}c",
                record.instruction_set, record.fetch_cycles
            )),
            repeat_count,
        }
    })
}

pub(super) fn nes_recent_opcode_display(
    entries: impl IntoIterator<Item = (u16, u8)>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(
        entries,
        RECENT_OPCODE_LINE_COUNT,
        |(pc, opcode), repeat_count| RecentOpcodeDisplay {
            address: Address::from(pc),
            bytes: vec![opcode],
            detail: None,
            repeat_count,
        },
    )
}

pub(super) fn sega8_recent_opcode_display(
    entries: impl IntoIterator<Item = (u16, u8, u32)>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(
        entries,
        RECENT_OPCODE_LINE_COUNT,
        |(pc, opcode, cycles), repeat_count| RecentOpcodeDisplay {
            address: Address::from(pc),
            bytes: vec![opcode],
            detail: Some(format!("{cycles} cyc")),
            repeat_count,
        },
    )
}

pub(super) fn ws_recent_opcode_display(
    entries: impl IntoIterator<Item = WsOpcodeRecord>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(entries, RECENT_OPCODE_LINE_COUNT, |record, repeat_count| {
        RecentOpcodeDisplay {
            address: Address::from(record.pc),
            bytes: vec![record.opcode],
            detail: Some(format!(
                "CS:IP={:04X}:{:04X} {} cyc",
                record.cs, record.ip, record.cycles
            )),
            repeat_count,
        }
    })
}

fn build_recent_opcode_display<E: Copy + Eq>(
    entries: impl IntoIterator<Item = E>,
    limit: usize,
    build: impl Fn(E, usize) -> RecentOpcodeDisplay,
) -> Vec<RecentOpcodeDisplay> {
    let mut seen: Vec<(E, usize)> = Vec::new();
    for entry in entries {
        if let Some(slot) = seen.iter_mut().find(|slot| slot.0 == entry) {
            slot.1 += 1;
        } else {
            seen.push((entry, 1));
        }
    }
    seen.into_iter()
        .take(limit)
        .map(|(entry, repeat_count)| build(entry, repeat_count))
        .collect()
}

fn gba_opcode_bytes(raw: u32, width_bytes: u8) -> Vec<u8> {
    let bytes = raw.to_le_bytes();
    bytes[..usize::from(width_bytes).min(bytes.len())].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_gba_core::hardware::cpu::InstructionSet;

    #[test]
    fn gb_recent_opcode_display_expands_cb_prefixes() {
        let display = gb_recent_opcode_display([(0x1234, 0x7C, true)]);

        assert_eq!(display[0].line(), "1234: CB 7C");
    }

    #[test]
    fn gba_recent_opcode_display_trims_to_fetch_width() {
        let display = gba_recent_opcode_display([GbaOpcodeRecord {
            pc: 0x0800_0000,
            raw: 0xE12F_FF1E,
            instruction_set: InstructionSet::Thumb,
            width_bytes: 2,
            fetch_cycles: 3,
        }]);

        assert_eq!(display[0].line(), "08000000: 1E FF (Thumb fetch=3c)");
    }

    #[test]
    fn nes_recent_opcode_display_uses_cpu_address_and_byte() {
        let display = nes_recent_opcode_display([(0xC000, 0xEA)]);

        assert_eq!(display[0].line(), "C000: EA");
    }

    #[test]
    fn sega8_recent_opcode_display_keeps_cycle_detail() {
        let display = sega8_recent_opcode_display([(0x0100, 0x3E, 7)]);

        assert_eq!(display[0].line(), "0100: 3E (7 cyc)");
    }

    #[test]
    fn ws_recent_opcode_display_keeps_segmented_address_detail() {
        let display = ws_recent_opcode_display([WsOpcodeRecord {
            cs: 0xF000,
            ip: 0xFFF0,
            pc: 0x0F_FFF0,
            opcode: 0xEA,
            cycles: 15,
        }]);

        assert_eq!(display[0].line(), "000FFFF0: EA (CS:IP=F000:FFF0 15 cyc)");
    }

    #[test]
    fn recent_opcode_display_collapses_repeated_records() {
        let display = nes_recent_opcode_display([(0x8000, 0xEA), (0x8000, 0xEA)]);

        assert_eq!(display[0].line(), "8000: EA (x2)");
    }
}
