use super::Sega8MapperKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MapperOverride {
    crc32: u32,
    mapper_kind: Sega8MapperKind,
}

// Normalized-ROM CRC32 mapper overrides for supported non-standard hardware.
const MAPPER_OVERRIDES: &[MapperOverride] = &[
    // MSX-style 8 KiB page mappers.
    MapperOverride {
        crc32: 0x4455_25E2,
        mapper_kind: Sega8MapperKind::Msx,
    },
    MapperOverride {
        crc32: 0x83F0_EEDE,
        mapper_kind: Sega8MapperKind::Msx,
    },
    MapperOverride {
        crc32: 0xA052_58F5,
        mapper_kind: Sega8MapperKind::Msx,
    },
    MapperOverride {
        crc32: 0x0696_5ED9,
        mapper_kind: Sega8MapperKind::Msx,
    },
    MapperOverride {
        crc32: 0x77EF_E84A,
        mapper_kind: Sega8MapperKind::Msx,
    },
    MapperOverride {
        crc32: 0xF89A_F3CC,
        mapper_kind: Sega8MapperKind::Msx,
    },
    MapperOverride {
        crc32: 0x9195_C34C,
        mapper_kind: Sega8MapperKind::Msx,
    },
    MapperOverride {
        crc32: 0x0A77_FA5E,
        mapper_kind: Sega8MapperKind::Msx,
    },
    // Nemesis-specific first-page mapping.
    MapperOverride {
        crc32: 0xE316_C06D,
        mapper_kind: Sega8MapperKind::Nemesis,
    },
    // Korean 16 KiB slot-2 mapper using the $A000 register.
    MapperOverride {
        crc32: 0x89B7_9E77,
        mapper_kind: Sega8MapperKind::Korean,
    },
    MapperOverride {
        crc32: 0x9292_22C4,
        mapper_kind: Sega8MapperKind::Korean,
    },
    MapperOverride {
        crc32: 0x18FB_98A3,
        mapper_kind: Sega8MapperKind::Korean,
    },
    MapperOverride {
        crc32: 0x97D0_3541,
        mapper_kind: Sega8MapperKind::Korean,
    },
    // Janggun-style 8 KiB mapper with bit-reversed reads.
    MapperOverride {
        crc32: 0x1929_49D5,
        mapper_kind: Sega8MapperKind::Janggun,
    },
];

const SG_TYPE_B_RAM_EXTENSION_CRCS: &[u32] = &[
    0x69FC_1494,
    0xFFC4_EE3F,
    0x2E36_6CCF,
    0xAAAC_12CF,
    0xD2ED_D329,
];

pub(super) fn mapper_kind_for_crc32(crc32: u32) -> Option<Sega8MapperKind> {
    MAPPER_OVERRIDES
        .iter()
        .find(|entry| entry.crc32 == crc32)
        .map(|entry| entry.mapper_kind)
}

pub(super) fn uses_sg_type_b_ram_extension(crc32: u32) -> bool {
    SG_TYPE_B_RAM_EXTENSION_CRCS.contains(&crc32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_supported_nonstandard_mapper_crcs() {
        assert_eq!(
            mapper_kind_for_crc32(0x4455_25E2),
            Some(Sega8MapperKind::Msx)
        );
        assert_eq!(
            mapper_kind_for_crc32(0xE316_C06D),
            Some(Sega8MapperKind::Nemesis)
        );
        assert_eq!(
            mapper_kind_for_crc32(0x89B7_9E77),
            Some(Sega8MapperKind::Korean)
        );
        assert_eq!(
            mapper_kind_for_crc32(0x1929_49D5),
            Some(Sega8MapperKind::Janggun)
        );
    }

    #[test]
    fn leaves_unknown_or_unsupported_mapper_crcs_unmapped() {
        assert_eq!(mapper_kind_for_crc32(0), None);
        assert_eq!(
            mapper_kind_for_crc32(0x76C5_BDFB),
            None,
            "Korean $4000 16 KiB variant is not implemented yet"
        );
        assert_eq!(
            mapper_kind_for_crc32(0x5E7B_18C8),
            None,
            "MSX 16 KiB variant is not implemented yet"
        );
    }

    #[test]
    fn recognizes_sg_type_b_ram_extension_titles() {
        assert!(uses_sg_type_b_ram_extension(0xD2ED_D329));
        assert!(uses_sg_type_b_ram_extension(0x69FC_1494));
        assert!(!uses_sg_type_b_ram_extension(0));
    }
}
