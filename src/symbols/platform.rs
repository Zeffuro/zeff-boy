use zeff_emu_common::system::System;

use super::{
    AddressSpaceId, Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SymbolId,
    SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) fn symbols(system: System) -> Vec<SymbolRecord> {
    let (entries, exec_mode, source): (&[(u64, &str)], ExecMode, &str) = match system {
        System::Gb => (&GB_REGISTERS, ExecMode::Sm83, "Game Boy hardware"),
        System::Gba => (&GBA_REGISTERS, ExecMode::Arm, "Game Boy Advance hardware"),
        System::Nes => (&NES_REGISTERS, ExecMode::Mos6502, "NES hardware"),
        System::Sms | System::Gg | System::Sg => {
            (&SEGA8_MAPPER, ExecMode::Z80, "Sega 8-bit mapper")
        }
        System::Ws => (&[], ExecMode::V30, "WonderSwan hardware"),
    };
    entries
        .iter()
        .map(|&(address, name)| SymbolRecord {
            id: SymbolId(0),
            name: name.to_owned(),
            location: SymbolLocation {
                cpu: Some(CpuLocation {
                    space: AddressSpaceId(0),
                    address,
                }),
                storage: None,
                bank: None,
                exec_mode,
            },
            value: None,
            size: None,
            kind: SymbolKind::Data,
            scope: SymbolScope::Global,
            provenance: Provenance {
                kind: ProvenanceKind::Platform,
                source: Some(source.to_owned()),
            },
            confidence: Confidence::Exact,
            comment: None,
        })
        .collect()
}

const GB_REGISTERS: [(u64, &str); 24] = [
    (0xFF00, "rJOYP"),
    (0xFF01, "rSB"),
    (0xFF02, "rSC"),
    (0xFF04, "rDIV"),
    (0xFF05, "rTIMA"),
    (0xFF06, "rTMA"),
    (0xFF07, "rTAC"),
    (0xFF0F, "rIF"),
    (0xFF40, "rLCDC"),
    (0xFF41, "rSTAT"),
    (0xFF42, "rSCY"),
    (0xFF43, "rSCX"),
    (0xFF44, "rLY"),
    (0xFF45, "rLYC"),
    (0xFF46, "rDMA"),
    (0xFF47, "rBGP"),
    (0xFF48, "rOBP0"),
    (0xFF49, "rOBP1"),
    (0xFF4A, "rWY"),
    (0xFF4B, "rWX"),
    (0xFF4D, "rKEY1"),
    (0xFF4F, "rVBK"),
    (0xFF70, "rSVBK"),
    (0xFFFF, "rIE"),
];

const GBA_REGISTERS: [(u64, &str); 28] = [
    (0x0400_0000, "REG_DISPCNT"),
    (0x0400_0004, "REG_DISPSTAT"),
    (0x0400_0006, "REG_VCOUNT"),
    (0x0400_0008, "REG_BG0CNT"),
    (0x0400_000A, "REG_BG1CNT"),
    (0x0400_000C, "REG_BG2CNT"),
    (0x0400_000E, "REG_BG3CNT"),
    (0x0400_00B0, "REG_DMA0SAD"),
    (0x0400_00B4, "REG_DMA0DAD"),
    (0x0400_00B8, "REG_DMA0CNT"),
    (0x0400_00BC, "REG_DMA1SAD"),
    (0x0400_00C0, "REG_DMA1DAD"),
    (0x0400_00C4, "REG_DMA1CNT"),
    (0x0400_00C8, "REG_DMA2SAD"),
    (0x0400_00CC, "REG_DMA2DAD"),
    (0x0400_00D0, "REG_DMA2CNT"),
    (0x0400_00D4, "REG_DMA3SAD"),
    (0x0400_00D8, "REG_DMA3DAD"),
    (0x0400_00DC, "REG_DMA3CNT"),
    (0x0400_0100, "REG_TM0CNT_L"),
    (0x0400_0102, "REG_TM0CNT_H"),
    (0x0400_0130, "REG_KEYINPUT"),
    (0x0400_0132, "REG_KEYCNT"),
    (0x0400_0200, "REG_IE"),
    (0x0400_0202, "REG_IF"),
    (0x0400_0204, "REG_WAITCNT"),
    (0x0400_0208, "REG_IME"),
    (0x0400_0300, "REG_POSTFLG"),
];

const NES_REGISTERS: [(u64, &str); 15] = [
    (0x2000, "PPUCTRL"),
    (0x2001, "PPUMASK"),
    (0x2002, "PPUSTATUS"),
    (0x2003, "OAMADDR"),
    (0x2004, "OAMDATA"),
    (0x2005, "PPUSCROLL"),
    (0x2006, "PPUADDR"),
    (0x2007, "PPUDATA"),
    (0x4014, "OAMDMA"),
    (0x4015, "APUSTATUS"),
    (0x4016, "JOY1"),
    (0x4017, "JOY2_APUFRAME"),
    (0xFFFA, "NMI_VECTOR"),
    (0xFFFC, "RESET_VECTOR"),
    (0xFFFE, "IRQ_VECTOR"),
];

const SEGA8_MAPPER: [(u64, &str); 4] = [
    (0xFFFC, "MAPPER_RAM_CONTROL"),
    (0xFFFD, "MAPPER_BANK0"),
    (0xFFFE, "MAPPER_BANK1"),
    (0xFFFF, "MAPPER_BANK2"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_only_known_cpu_memory_labels() {
        assert!(
            symbols(System::Gb)
                .iter()
                .any(|symbol| symbol.name == "rLCDC")
        );
        assert!(
            symbols(System::Gba)
                .iter()
                .any(|symbol| symbol.name == "REG_IME")
        );
        assert!(
            symbols(System::Nes)
                .iter()
                .any(|symbol| symbol.name == "PPUCTRL")
        );
        assert!(symbols(System::Ws).is_empty());
    }
}
