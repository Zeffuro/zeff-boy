use crate::{Budget, ScanStop};

#[derive(Clone, Copy)]
pub(super) enum OrderLayout {
    Indexed {
        selectors: u16,
        groups: u16,
        banks: u16,
        speed: u16,
    },
    Indirect {
        selectors: [u16; 3],
    },
}

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub(super) name: &'static str,
    pub(super) bank: u16,
    pub(super) tick: u16,
    pub(super) tick_end: u16,
    pub(super) tone: u16,
    pub(super) wram_bank: u8,
    pub(super) selector_count: u16,
    pub(super) instruments: u16,
    pub(super) instruments_end: u16,
    pub(super) arp: u16,
    pub(super) duty: u16,
    pub(super) wave: u16,
    pub(super) wave_end: u16,
    pub(super) order: u16,
    pub(super) order_end: u16,
    pub(super) layout: OrderLayout,
    pub(super) fixed_hash: &'static str,
    pub(super) bank_hash: &'static str,
}

#[derive(Clone, Copy)]
pub(super) struct Recognition {
    pub(super) bank: u16,
    pub(super) profile: &'static Profile,
}

const PROFILES: [Profile; 5] = [
    Profile {
        name: "mplay-byte-order-v1",
        bank: 0x001d,
        tick: 0x2c66,
        tick_end: 0x2e9e,
        tone: 0x57a9,
        instruments: 0x5cd2,
        instruments_end: 0x5d6a,
        arp: 0x5da4,
        duty: 0x5ddc,
        wave: 0x5df6,
        wave_end: 0x5e36,
        order: 0x5d6a,
        order_end: 0x5da4,
        wram_bank: 1,
        selector_count: 11,
        layout: OrderLayout::Indexed {
            selectors: 0x5e36,
            groups: 0x5e41,
            banks: 0x5f49,
            speed: 0x5cd1,
        },
        fixed_hash: "543fb2b441110711d5c6fd13b5a8990a2658b94a1d7554872efb966b411526cf",
        bank_hash: "e5e70a267a547a7ccea90d67324f82d14e04d038dbc687f7c72cd9cb075fab79",
    },
    Profile {
        name: "mplay-byte-order-v2",
        bank: 0x0003,
        tick: 0x08a2,
        tick_end: 0x0ade,
        tone: 0x56b0,
        instruments: 0x5e32,
        instruments_end: 0x5eba,
        arp: 0x5eef,
        duty: 0x5f48,
        wave: 0x5f6f,
        wave_end: 0x606f,
        order: 0x5eba,
        order_end: 0x5eef,
        wram_bank: 2,
        selector_count: 8,
        layout: OrderLayout::Indexed {
            selectors: 0x606f,
            groups: 0x6077,
            banks: 0x619f,
            speed: 0x5e31,
        },
        fixed_hash: "4536dcec4e7056e94b6017e18c3623f97154dc54ef04fb9651ac1be52f961197",
        bank_hash: "1703668b1864d3deee824b9e363417a46dd33657f293a1e95ccd5ae9b83b4835",
    },
    Profile {
        name: "mplay-pointer-order-v3",
        bank: 0x001e,
        tick: 0x3514,
        tick_end: 0x3760,
        tone: 0x57f3,
        instruments: 0x608e,
        instruments_end: 0x6116,
        arp: 0x6116,
        duty: 0x6181,
        wave: 0x619b,
        wave_end: 0x61bb,
        order: 0x61bb,
        order_end: 0x6223,
        wram_bank: 1,
        selector_count: 21,
        layout: OrderLayout::Indirect {
            selectors: [0x6223, 0x624d, 0x6277],
        },
        fixed_hash: "8dca022841406b49da71a01b8f2e0ca93f5b9c922dd66cfed6b6c5c9bb9537a6",
        bank_hash: "722a7c4f79b43d2db1148e89d120df2c2d5b95e7f62aa5b2ed9a9947e732b6df",
    },
    Profile {
        name: "mplay-pointer-order-v4",
        bank: 0x0001,
        tick: 0x3205,
        tick_end: 0x345b,
        tone: 0x5804,
        instruments: 0x5ede,
        instruments_end: 0x5fa6,
        arp: 0x5fa6,
        duty: 0x6012,
        wave: 0x6028,
        wave_end: 0x6038,
        order: 0x6038,
        order_end: 0x60b7,
        wram_bank: 1,
        selector_count: 22,
        layout: OrderLayout::Indirect {
            selectors: [0x60b7, 0x60e3, 0x610f],
        },
        fixed_hash: "aab1262b95f6179a9aa226b87449f6f3896866936652aa6a97bfef6552c91945",
        bank_hash: "00343e8cfbf9fec20fb3e5153e0e288a09020297e88e01a7508c857b9586d4d8",
    },
    Profile {
        name: "mplay-pointer-order-v5",
        bank: 0x0001,
        tick: 0x3406,
        tick_end: 0x365c,
        tone: 0x56c9,
        instruments: 0x5a75,
        instruments_end: 0x5b25,
        arp: 0x5b25,
        duty: 0x5ba8,
        wave: 0x5bbc,
        wave_end: 0x5c1c,
        order: 0x5c1c,
        order_end: 0x5d82,
        wram_bank: 1,
        selector_count: 33,
        layout: OrderLayout::Indirect {
            selectors: [0x5d82, 0x5dc4, 0x5e06],
        },
        fixed_hash: "a4c0d5003b78d0a9db2b999d8865b75fe253209f62010d18f9b19515aed429be",
        bank_hash: "d37f7ce334f04a171007d460f957e1c2c1520e0e8acdc44a22fb2ec1e7c86ccd",
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
) -> Result<Vec<Recognition>, ScanStop> {
    let mut result = Vec::new();
    if !super::supports_cartridge(bytes) {
        return Ok(result);
    }
    #[cfg(any(test, feature = "test-support"))]
    if super::tests::recognized(bytes) {
        budget.charge()?;
        result.push(Recognition {
            bank: super::tests::PROFILE.bank,
            profile: &super::tests::PROFILE,
        });
    }
    for profile in &PROFILES {
        budget.charge()?;
        let bank = usize::from(profile.bank) * 0x4000;
        let code_end = bank + usize::from(profile.tone - 0x4000);
        if code_end > bytes.len() {
            continue;
        }
        if zeff_firmware::sha256_hex(
            &bytes[usize::from(profile.tick)..usize::from(profile.tick_end)],
        ) == profile.fixed_hash
            && zeff_firmware::sha256_hex(&bytes[bank..code_end]) == profile.bank_hash
        {
            result.push(Recognition {
                bank: profile.bank,
                profile,
            });
        }
    }
    Ok(result)
}
