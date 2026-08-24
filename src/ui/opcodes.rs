use crate::debug::RecentOpcodeDisplay;
use zeff_emu_common::address::Address;
use zeff_gba_core::emulator::GbaOpcodeRecord;
use zeff_pce_core::hardware::PceOpcodeHistoryEntry;
use zeff_ws_core::emulator::WsOpcodeRecord;

pub(super) const RECENT_OPCODE_LINE_COUNT: usize = 16;

pub(super) fn gb_recent_opcode_display(
    entries: impl IntoIterator<Item = (u16, u8, bool, Option<usize>)>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(entries, RECENT_OPCODE_LINE_COUNT, |entry, repeat_count| {
        let (pc, opcode, cb_prefix, storage_offset) = entry;
        RecentOpcodeDisplay {
            address: Address::from(pc),
            storage_offset: storage_offset.and_then(|value| u64::try_from(value).ok()),
            bytes: if cb_prefix {
                vec![0xCB, opcode]
            } else {
                vec![opcode]
            },
            detail: None,
            repeat_count,
            thumb: None,
        }
    })
}

pub(super) fn gba_recent_opcode_display(
    entries: impl IntoIterator<Item = GbaOpcodeRecord>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(entries, RECENT_OPCODE_LINE_COUNT, |record, repeat_count| {
        RecentOpcodeDisplay {
            address: Address::from(record.pc),
            storage_offset: None,
            bytes: gba_opcode_bytes(record.raw, record.width_bytes),
            detail: Some(format!(
                "{:?} fetch={}c",
                record.instruction_set, record.fetch_cycles
            )),
            repeat_count,
            thumb: Some(matches!(
                record.instruction_set,
                zeff_gba_core::hardware::cpu::InstructionSet::Thumb
            )),
        }
    })
}

pub(super) fn nes_recent_opcode_display(
    entries: impl IntoIterator<Item = (u16, u8, Option<usize>)>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(
        entries,
        RECENT_OPCODE_LINE_COUNT,
        |(pc, opcode, storage_offset), repeat_count| RecentOpcodeDisplay {
            address: Address::from(pc),
            storage_offset: storage_offset.and_then(|value| u64::try_from(value).ok()),
            bytes: vec![opcode],
            detail: None,
            repeat_count,
            thumb: None,
        },
    )
}

pub(super) fn pce_recent_opcode_display(
    entries: impl IntoIterator<Item = PceOpcodeHistoryEntry>,
) -> Vec<RecentOpcodeDisplay> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Record {
        logical_pc: u16,
        physical_pc: u32,
        opcode: u8,
    }

    build_recent_opcode_display(
        entries.into_iter().map(|entry| Record {
            logical_pc: entry.logical_pc(),
            physical_pc: entry.physical_pc(),
            opcode: entry.opcode(),
        }),
        RECENT_OPCODE_LINE_COUNT,
        |record, repeat_count| RecentOpcodeDisplay {
            address: Address::from(record.logical_pc),
            storage_offset: None,
            bytes: vec![record.opcode],
            detail: Some(format!("phys={:06X}", record.physical_pc)),
            repeat_count,
            thumb: None,
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
            storage_offset: None,
            bytes: vec![opcode],
            detail: Some(format!("{cycles} cyc")),
            repeat_count,
            thumb: None,
        },
    )
}

pub(super) fn ws_recent_opcode_display(
    entries: impl IntoIterator<Item = WsOpcodeRecord>,
) -> Vec<RecentOpcodeDisplay> {
    build_recent_opcode_display(entries, RECENT_OPCODE_LINE_COUNT, |record, repeat_count| {
        RecentOpcodeDisplay {
            address: Address::from(record.pc),
            storage_offset: None,
            bytes: vec![record.opcode],
            detail: Some(format!(
                "CS:IP={:04X}:{:04X} {} cyc",
                record.cs, record.ip, record.cycles
            )),
            repeat_count,
            thumb: None,
        }
    })
}

fn build_recent_opcode_display<E: Copy + Eq>(
    entries: impl IntoIterator<Item = E>,
    limit: usize,
    build: impl Fn(E, usize) -> RecentOpcodeDisplay,
) -> Vec<RecentOpcodeDisplay> {
    let mut runs: Vec<(E, usize)> = Vec::new();
    for entry in entries {
        if let Some(slot) = runs.last_mut().filter(|slot| slot.0 == entry) {
            slot.1 += 1;
        } else {
            runs.push((entry, 1));
        }
    }
    runs.into_iter()
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
        let display = gb_recent_opcode_display([(0x1234, 0x7C, true, Some(0x9234))]);

        assert_eq!(display[0].line(), "1234: CB 7C");
        assert_eq!(display[0].storage_offset, Some(0x9234));
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
        let display = nes_recent_opcode_display([(0xC000, 0xEA, Some(0x4000))]);

        assert_eq!(display[0].line(), "C000: EA");
        assert_eq!(display[0].storage_offset, Some(0x4000));
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
        let display = nes_recent_opcode_display([(0x8000, 0xEA, Some(0)), (0x8000, 0xEA, Some(0))]);

        assert_eq!(display[0].line(), "8000: EA (x2)");
    }

    #[test]
    fn recent_opcode_display_preserves_repeated_loop_order() {
        let display = nes_recent_opcode_display([
            (0x8000, 0xEA, Some(0)),
            (0x8001, 0xD0, Some(1)),
            (0x8000, 0xEA, Some(0)),
        ]);

        assert_eq!(display.len(), 3);
        assert_eq!(display[0].address, 0x8000);
        assert_eq!(display[1].address, 0x8001);
        assert_eq!(display[2].address, 0x8000);
    }
}
