use crate::{Budget, ScanStop};

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub index: u16,
    pub bank: u16,
    pub api: u16,
    pub caller: u16,
    pub entry: u16,
    pub vblank: u16,
    pub reserve: u16,
    pub flag: u16,
    pub spans: &'static [(u32, u32)],
}

const PROFILES: [Profile; 11] = [
    Profile {
        name: "cache-pulse-v1",
        hash: "3843f2cdb0746ff0cf7dc97e1aab4fc6a15219e7ac8e1970ead475a835c57aea",
        index: 0x0,
        bank: 0x3,
        api: 0x4000,
        caller: 0x236d,
        entry: 0x150,
        vblank: 0x9dd,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[
            (0x40, 0x2f24),
            (0x669e, 0x810),
            (0xc000, 0x3fc0),
            (0x13d00, 0x300),
        ],
    },
    Profile {
        name: "cache-pulse-v2",
        hash: "e335408688c130181da0a3ae596abccd37ddc147b011b1c5a9d5eb2fcb221b4c",
        index: 0x8,
        bank: 0x6,
        api: 0x4000,
        caller: 0x889,
        entry: 0x150,
        vblank: 0x91c,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[(0x40, 0x964), (0x15b3d, 0x48), (0x18000, 0x1885)],
    },
    Profile {
        name: "cache-pulse-v3",
        hash: "5fa11359e8147b295bebd1e5631c7b96908c649d9d33fdc45a2dd3de8d69ca73",
        index: 0x0,
        bank: 0x4,
        api: 0x4000,
        caller: 0x1b3d,
        entry: 0x150,
        vblank: 0x153,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[(0x40, 0x34e6), (0x74be, 0xb42), (0x10000, 0x151f)],
    },
    Profile {
        name: "cache-pulse-v4",
        hash: "2fd6cc8f52e0c5f665b5c70368e4594aa93a4cbda7c76fd9c3ecc2b0724a5785",
        index: 0x3,
        bank: 0xf,
        api: 0x4000,
        caller: 0x82e,
        entry: 0x70,
        vblank: 0x150,
        reserve: 0x3f00,
        flag: 0xdfd4,
        spans: &[
            (0x40, 0x3b9b),
            (0x4028, 0x3b45),
            (0xf000, 0x224),
            (0x3c000, 0x3e11),
        ],
    },
    Profile {
        name: "cache-pulse-v5",
        hash: "2b059983d79efc5a4f77b41a4efbad68c65ae259715dc008f20a3c11226a943b",
        index: 0x8,
        bank: 0x6,
        api: 0x4000,
        caller: 0x889,
        entry: 0x150,
        vblank: 0x91c,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[(0x40, 0x964), (0x15b3d, 0x48), (0x18000, 0x1885)],
    },
    Profile {
        name: "cache-pulse-v6",
        hash: "3b550ead7630a355fee97d8ac1327dd3325af6c4b082538a775d0156e044c8a1",
        index: 0xe,
        bank: 0x3,
        api: 0x4000,
        caller: 0x717,
        entry: 0x150,
        vblank: 0x36f,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[
            (0x40, 0x39e8),
            (0x8000, 0x1fc),
            (0xc000, 0x3146),
            (0x10010, 0x2f2),
            (0x14748, 0x24),
            (0x3404c, 0x26),
            (0x39264, 0x2cc1),
            (0x3c000, 0xaa9),
        ],
    },
    Profile {
        name: "cache-pulse-v7",
        hash: "4790f55d0917f4418c63d29c5c528362b1c7ae39546f6fc3ae94e867d1e9f270",
        index: 0x0,
        bank: 0x6,
        api: 0x4000,
        caller: 0xd99,
        entry: 0x150,
        vblank: 0x153,
        reserve: 0x3ea0,
        flag: 0xdffe,
        spans: &[
            (0x40, 0x3e60),
            (0x3ef4, 0x8a),
            (0x18000, 0x38f0),
            (0x1f022, 0xfde),
        ],
    },
    Profile {
        name: "cache-pulse-v8",
        hash: "4424900339aad0514538250aaea1d407869dad2b0df7310b8ce778de8c35c5c3",
        index: 0x0,
        bank: 0x3,
        api: 0x63d7,
        caller: 0x13df,
        entry: 0x150,
        vblank: 0xc7bd,
        reserve: 0x3ea0,
        flag: 0xdffe,
        spans: &[(0x40, 0x3e60), (0x3ef4, 0xb0), (0xe3d7, 0x10b9)],
    },
    Profile {
        name: "cache-pulse-v9",
        hash: "cac662f7425aa5284363b1761ae6f3b77b118168869e0daaae88106de5c79bae",
        index: 0x3,
        bank: 0x2,
        api: 0x4000,
        caller: 0x203e,
        entry: 0x15a,
        vblank: 0x163f,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[(0x40, 0x3627), (0x41dc, 0xc), (0x8000, 0x3fda)],
    },
    Profile {
        name: "cache-pulse-v10",
        hash: "944df9d20c715099ef6bf0e418d928684b284d4a4562391a661a7978fa7f5417",
        index: 0x0,
        bank: 0x7,
        api: 0x4100,
        caller: 0x344d,
        entry: 0x150,
        vblank: 0xc000,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[
            (0x18, 0x3b77),
            (0x8000, 0x88a),
            (0xfbcc, 0x1a0),
            (0x1af77, 0xac6),
            (0x1c100, 0x1ba4),
        ],
    },
    Profile {
        name: "cache-pulse-v11",
        hash: "1a62bbcaff8d12b752826472beac346e5fce9d8640f7ae23c6e390b79e55346d",
        index: 0x0,
        bank: 0x4,
        api: 0x69f0,
        caller: 0xb2f,
        entry: 0x150,
        vblank: 0x2e32,
        reserve: 0x3f00,
        flag: 0xdffe,
        spans: &[
            (0x40, 0x3429),
            (0x129f0, 0x1601),
            (0x180e6, 0x108),
            (0x1dca9, 0x1a2c),
        ],
    },
];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    #[cfg(any(test, feature = "test-support"))]
    if name == super::tests::PROFILE.name {
        return Some(&super::tests::PROFILE);
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
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let hash = zeff_firmware::sha256_hex(bytes);
    budget.charge()?;
    #[cfg(any(test, feature = "test-support"))]
    if bytes == super::tests::synthetic_rom() {
        return Ok(Some(&super::tests::PROFILE));
    }
    Ok(PROFILES.iter().find(|profile| profile.hash == hash))
}
