use crate::{Budget, ScanStop};

pub(super) struct Selection {
    pub index: u16,
    pub header: u16,
    pub data_len: u16,
}

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub init: u16,
    pub start: u16,
    pub tick: u16,
    pub table: u16,
    pub bank: u16,
    pub resident_len: u16,
    pub timer_modulo: Option<u8>,
    pub selections: &'static [Selection],
}

const UPPER: [Selection; 4] = [
    Selection {
        index: 2,
        header: 0x75ce,
        data_len: 0x46b,
    },
    Selection {
        index: 4,
        header: 0x7000,
        data_len: 0x5cd,
    },
    Selection {
        index: 6,
        header: 0x7a3a,
        data_len: 0x3ec,
    },
    Selection {
        index: 8,
        header: 0x7e27,
        data_len: 0x1d7,
    },
];

const LOWER: [Selection; 4] = [
    Selection {
        index: 2,
        header: 0x45ce,
        data_len: 0x46b,
    },
    Selection {
        index: 4,
        header: 0x4000,
        data_len: 0x5cd,
    },
    Selection {
        index: 6,
        header: 0x4a3a,
        data_len: 0x3ec,
    },
    Selection {
        index: 8,
        header: 0x4e27,
        data_len: 0x1d7,
    },
];

const PROFILES: [Profile; 8] = [
    Profile {
        name: "resident-four-channel-v1",
        hash: "2f3ba49c83f6d5ce54fd91779552557699c5a09ea134ce880f7c97b39a2a16e1",
        init: 0x061a,
        start: 0x069e,
        tick: 0x070c,
        table: 0x0ce7,
        bank: 15,
        resident_len: 0x6e0,
        timer_modulo: None,
        selections: &UPPER,
    },
    Profile {
        name: "resident-four-channel-v2",
        hash: "24d4a2521ea1a9098c511a64ad5e25c1f98dfd64ba12e5d75379f05664979258",
        init: 0x061d,
        start: 0x06a1,
        tick: 0x070f,
        table: 0x0cea,
        bank: 15,
        resident_len: 0x6e0,
        timer_modulo: None,
        selections: &UPPER,
    },
    Profile {
        name: "resident-four-channel-v3",
        hash: "898fcc85e1e0c5102ac7d994fabc82127da0249318759e83d3d607ffaad2f647",
        init: 0x061d,
        start: 0x06a1,
        tick: 0x070f,
        table: 0x0cea,
        bank: 15,
        resident_len: 0x6e0,
        timer_modulo: None,
        selections: &UPPER,
    },
    Profile {
        name: "resident-four-channel-v4",
        hash: "f569e3e80659cdad294978d0b259015db9484a2ff00e96f34a1d9e69ed1dcc28",
        init: 0x061d,
        start: 0x06a1,
        tick: 0x070f,
        table: 0x0cea,
        bank: 15,
        resident_len: 0x6e0,
        timer_modulo: None,
        selections: &UPPER,
    },
    Profile {
        name: "resident-four-channel-v5",
        hash: "d7a22e004c4c7ce65f527a1787ad9a228d557c97d4d42edb16512bd86d174fb8",
        init: 0x061a,
        start: 0x069e,
        tick: 0x070c,
        table: 0x0ce7,
        bank: 15,
        resident_len: 0x6e0,
        timer_modulo: None,
        selections: &LOWER,
    },
    Profile {
        name: "resident-four-channel-v6",
        hash: "45b15fb0fdede42f3c5f8b01fd7306fdbf9da63a61d91bbff0803bf31013e92c",
        init: 0x061d,
        start: 0x06a1,
        tick: 0x070f,
        table: 0x0cea,
        bank: 15,
        resident_len: 0x6e0,
        timer_modulo: None,
        selections: &UPPER,
    },
    Profile {
        name: "resident-four-channel-v7",
        hash: "a0de1b942d93d020b04dd6c3ac564cd5c3e878279464cc0121eb5f1975df9e01",
        init: 0x05ac,
        start: 0x0630,
        tick: 0x069e,
        table: 0x0c79,
        bank: 31,
        resident_len: 0x6e0,
        timer_modulo: None,
        selections: &UPPER,
    },
    Profile {
        name: "resident-four-channel-v8",
        hash: "9b2642204ff2e851906d892d8cd1d0a552468ff68444b5725b33fd292d03d621",
        init: 0x2cb6,
        start: 0x2d3a,
        tick: 0x2da8,
        table: 0x3383,
        bank: 15,
        resident_len: 0x6e0,
        timer_modulo: Some(0xb8),
        selections: &UPPER,
    },
];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    #[cfg(any(test, feature = "test-support"))]
    for profile in [&super::tests::PROFILE, &super::tests::TIMER_PROFILE] {
        if profile.name == name {
            return Some(profile);
        }
    }
    PROFILES.iter().find(|profile| profile.name == name)
}

pub(super) fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if !super::supports_cartridge(bytes) {
        return Ok(None);
    }
    let hash = zeff_firmware::sha256_hex(bytes);
    budget.charge()?;
    #[cfg(any(test, feature = "test-support"))]
    for (profile, fixture) in [
        (&super::tests::PROFILE, super::tests::synthetic_rom()),
        (
            &super::tests::TIMER_PROFILE,
            super::tests::synthetic_timer_rom(),
        ),
    ] {
        if hash == zeff_firmware::sha256_hex(&fixture) {
            return Ok(Some(profile));
        }
    }
    Ok(PROFILES.iter().find(|profile| profile.hash == hash))
}
