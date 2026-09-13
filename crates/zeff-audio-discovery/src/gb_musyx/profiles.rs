use crate::{Budget, ScanStop};

pub(super) struct Profile {
    pub name: &'static str,
    pub start: usize,
    pub len: usize,
    pub bank_literal: Option<usize>,
    pub prefix: [u8; 8],
    pub hash: &'static str,
    pub bank0: usize,
    pub bank0_len: usize,
    pub bank0_hashes: &'static [&'static str],
    pub init: u16,
    pub handle: u16,
    pub start_song: u16,
    pub sample: u16,
    pub current_bank: u16,
}

#[derive(Clone, Copy)]
pub(super) struct Driver {
    pub profile: &'static Profile,
    pub bank: u8,
    pub offset: usize,
}

impl Driver {
    pub fn project(self) -> usize {
        self.offset + self.profile.len
    }
}

static PROFILES: &[Profile] = &[
    Profile {
        name: "gb-musyx-01",
        start: 0x4000,
        len: 0x1a44,
        bank_literal: Some(0x50c),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "d0666e9daf2d9137b409044e9578bf7e20bea6e5aaec4e2cca3684d2d262a7dc",
        bank0: 0x3580,
        bank0_len: 0x54b,
        bank0_hashes: &["481a298a1528abbad5c7e231dc332125f5a986e3b66ea93080bbf7759c627d43"],
        init: 0x44ef,
        handle: 0x4356,
        start_song: 0x56cd,
        sample: 0x365f,
        current_bank: 0xdf07,
    },
    Profile {
        name: "gb-musyx-02",
        start: 0x4000,
        len: 0x1a87,
        bank_literal: Some(0x512),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "9d0e59caa95648a4b5190f53a7f5ea0bac89118c645cace34d9a05e15e15f3fe",
        bank0: 0x3ab0,
        bank0_len: 0x527,
        bank0_hashes: &["264ddbf8c226ac5f9b5930f14f27a7788c3a7df1d5f2a5de8c1f21e9f968dc7a"],
        init: 0x44f1,
        handle: 0x4356,
        start_song: 0x5710,
        sample: 0x3ad0,
        current_bank: 0xfffe,
    },
    Profile {
        name: "gb-musyx-03",
        start: 0x4000,
        len: 0x1a63,
        bank_literal: Some(0x50c),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "80babed059c7f5e6252e4105a2fe058cf5f21cfa69b9ffb8aa67c7fd187ba096",
        bank0: 0x3ab0,
        bank0_len: 0x54b,
        bank0_hashes: &["0481d83300ff8eaf0f2d31fb12c4e2d699213234dde108128236f63929936286"],
        init: 0x44ef,
        handle: 0x4356,
        start_song: 0x56ec,
        sample: 0x3ab0,
        current_bank: 0xdf01,
    },
    Profile {
        name: "gb-musyx-04",
        start: 0x4000,
        len: 0x1a61,
        bank_literal: Some(0x50c),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "0d52c183f66e3621cc9a6f00b5e1787eaad7433e79bd3de6e218dbddc5533ad0",
        bank0: 0x3ab0,
        bank0_len: 0x54b,
        bank0_hashes: &[
            "58d02661019c8679d79feea6b261467c23a7fed2a1192a933949a4a25eac8e25",
            "882fe257fc8d329567af71ec9314c0d67c8656ce72a843513c70cd030b9a7415",
        ],
        init: 0x44ef,
        handle: 0x4356,
        start_song: 0x56ea,
        sample: 0x3b8f,
        current_bank: 0xdf07,
    },
    Profile {
        name: "gb-musyx-05",
        start: 0x6000,
        len: 0x1a44,
        bank_literal: Some(0x50c),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "7a4bf7d72fa1f75debdd3446f48e434c9dfc5181211f9125de108824d64b0657",
        bank0: 0x3ab0,
        bank0_len: 0x54b,
        bank0_hashes: &["1a9de0ab97561344ca58510ef400f23e5464634bcee67f56ac4fecdffb5ffd2e"],
        init: 0x64ef,
        handle: 0x6356,
        start_song: 0x76cd,
        sample: 0x3b8f,
        current_bank: 0xdf07,
    },
    Profile {
        name: "gb-musyx-06",
        start: 0x4000,
        len: 0x1a79,
        bank_literal: Some(0x510),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "08c0dfba4da88f47ab3a331e339ae15bc92e5dba86293a95a0744afa9da80f7b",
        bank0: 0x3ab0,
        bank0_len: 0x51f,
        bank0_hashes: &["97fe0991e7cdfd3cb9dbc9651b073d8626008cffc8e040df3d224a5530889355"],
        init: 0x44ef,
        handle: 0x4356,
        start_song: 0x5702,
        sample: 0x3ac0,
        current_bank: 0xfffe,
    },
    Profile {
        name: "gb-musyx-07",
        start: 0x4000,
        len: 0x1aa0,
        bank_literal: None,
        prefix: [0xc3, 0x34, 0x45, 0xc3, 0x57, 0x42, 0xc3, 0x2d],
        hash: "990e5c4e785d165bee1cc2442bbdb610433327a474429665ae3de20cf1d6247b",
        bank0: 0x3ab0,
        bank0_len: 0x54b,
        bank0_hashes: &["0481d83300ff8eaf0f2d31fb12c4e2d699213234dde108128236f63929936286"],
        init: 0x4534,
        handle: 0x439b,
        start_song: 0x5729,
        sample: 0x3ab0,
        current_bank: 0xdf01,
    },
    Profile {
        name: "gb-musyx-08",
        start: 0x4000,
        len: 0x1a44,
        bank_literal: Some(0x50c),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "9851b989c837c4cd4f6d4febe9686a1c31c272e415b4d092b71a8f1d03dac090",
        bank0: 0x3ab0,
        bank0_len: 0x54b,
        bank0_hashes: &[
            "58d02661019c8679d79feea6b261467c23a7fed2a1192a933949a4a25eac8e25",
            "71dcf3b9da01ec4e569ac06122813c3f7582608c6e5ce6b3dbe9e5288a2912ba",
        ],
        init: 0x44ef,
        handle: 0x4356,
        start_song: 0x56cd,
        sample: 0x3b8f,
        current_bank: 0xdf07,
    },
    Profile {
        name: "gb-musyx-09",
        start: 0x4000,
        len: 0x1a79,
        bank_literal: Some(0x510),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "e6913c20237ffe200076a6e6feddb5ce5516334e803cd720d39429944c7e8205",
        bank0: 0x3ae0,
        bank0_len: 0x51f,
        bank0_hashes: &["e05be25e26d92d63fa42fcc9e93f6660a1866066abaa5e418fa44fcd8aa476bb"],
        init: 0x44ef,
        handle: 0x4356,
        start_song: 0x5702,
        sample: 0x3af0,
        current_bank: 0xfffe,
    },
    Profile {
        name: "gb-musyx-10",
        start: 0x4000,
        len: 0x1a79,
        bank_literal: Some(0x510),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "8d5b31922a4de4f351d80f09446b05b8d0a4057a8c2ee246703f977f25ff59d6",
        bank0: 0x38c0,
        bank0_len: 0x51f,
        bank0_hashes: &["46f672f40def21055a0acb98c8cb96b4ffd25d6f17aa65b9d85e56c39630f50b"],
        init: 0x44ef,
        handle: 0x4356,
        start_song: 0x5702,
        sample: 0x38d0,
        current_bank: 0xfffe,
    },
    Profile {
        name: "gb-musyx-11",
        start: 0x4000,
        len: 0x1ac2,
        bank_literal: None,
        prefix: [0xc3, 0x3d, 0x45, 0xc3, 0x60, 0x42, 0xc3, 0x36],
        hash: "d209ea6528b40b553c4bc66a11013f5ff8568616d1d2428014889970c378122b",
        bank0: 0x3ab0,
        bank0_len: 0x51f,
        bank0_hashes: &["97fe0991e7cdfd3cb9dbc9651b073d8626008cffc8e040df3d224a5530889355"],
        init: 0x453d,
        handle: 0x43a4,
        start_song: 0x574b,
        sample: 0x3ac0,
        current_bank: 0xfffe,
    },
    Profile {
        name: "gb-musyx-12",
        start: 0x4000,
        len: 0x1a87,
        bank_literal: Some(0x512),
        prefix: [0x6, 0x0, 0x21, 0x0, 0x0, 0xf, 0x30, 0x1],
        hash: "02e727d688a678fd90c4c188090f1e6657cff23916dd98970b3ae0bf23dba250",
        bank0: 0x3a80,
        bank0_len: 0x527,
        bank0_hashes: &["6965b2e8556c396c0a680d3c8ef8d66d90c212291cbcaead054e3e7b6a9d4650"],
        init: 0x44f1,
        handle: 0x4356,
        start_song: 0x5710,
        sample: 0x3aa0,
        current_bank: 0xfffe,
    },
    Profile {
        name: "gb-musyx-13",
        start: 0x4000,
        len: 0x1ad0,
        bank_literal: None,
        prefix: [0xc3, 0x3f, 0x45, 0xc3, 0x60, 0x42, 0xc3, 0x36],
        hash: "3d61a02d277cfdfa9802dd0a35e3fcbf1ec051b4c2d4ab89b6ce979b4d1fd42f",
        bank0: 0x3ab0,
        bank0_len: 0x527,
        bank0_hashes: &["264ddbf8c226ac5f9b5930f14f27a7788c3a7df1d5f2a5de8c1f21e9f968dc7a"],
        init: 0x453f,
        handle: 0x43a4,
        start_song: 0x5759,
        sample: 0x3ad0,
        current_bank: 0xfffe,
    },
];

pub(super) fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<Driver>, ScanStop> {
    budget.charge()?;
    let mut drivers = Vec::new();
    if bytes.len() < 0x8000
        || !matches!(bytes[0x143], 0x80 | 0xc0)
        || !(0x19..=0x1e).contains(&bytes[0x147])
        || bytes[0x148] > 8
        || bytes.len() != (0x8000usize << bytes[0x148])
    {
        return Ok(drivers);
    }
    let profiles = PROFILES.iter();
    #[cfg(any(test, feature = "test-support"))]
    let profiles = profiles.chain(std::iter::once(&super::tests::PROFILE));
    for profile in profiles {
        budget.charge()?;
        let Some(bank0) = bytes.get(profile.bank0..profile.bank0 + profile.bank0_len) else {
            continue;
        };
        let hash = const_hex::encode(zeff_firmware::sha256_bytes(bank0));
        if !profile.bank0_hashes.contains(&hash.as_str()) {
            continue;
        }
        for bank in 1..(bytes.len() / 0x4000).min(256) {
            budget.charge()?;
            let offset = bank * 0x4000 + profile.start - 0x4000;
            let Some(code) = bytes.get(offset..offset + profile.len) else {
                continue;
            };
            if code.get(..8) != Some(profile.prefix.as_slice()) {
                continue;
            }
            let mut normalized = code.to_vec();
            if let Some(at) = profile.bank_literal {
                if normalized.get(at) != Some(&(bank as u8)) {
                    continue;
                }
                normalized[at] = 0;
            }
            let hash = const_hex::encode(zeff_firmware::sha256_bytes(&normalized));
            budget.charge()?;
            if hash == profile.hash {
                drivers.push(Driver {
                    profile,
                    bank: bank as u8,
                    offset,
                });
            }
        }
    }
    Ok(drivers)
}
