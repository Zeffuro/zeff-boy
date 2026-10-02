use crate::{Budget, ScanStop};

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub reset: u16,
    pub bank_setter: u16,
    pub init_bank: u8,
    pub bank: u8,
    pub index: u8,
    pub header: u16,
    pub entry: u16,
    pub timer: bool,
    pub spans: &'static [(u8, u16, u16)],
}

const PROFILES: &[Profile] = &[
    Profile {
        name: "wave-timer-banked-v1",
        hash: "a54515bb6b3e364964d3c0226f5a6b0c8c0f7318c9296ef2e321df0bbb8541ce",
        reset: 0x03ec,
        bank_setter: 0,
        init_bank: 61,
        bank: 61,
        index: 1,
        header: 0x4f44,
        entry: 0x4f07,
        timer: true,
        spans: &[
            (0, 0x03ec, 0x3c14),
            (61, 0x4000, 0x4000),
            (63, 0x4000, 0x4000),
        ],
    },
    Profile {
        name: "wave-frame-banked-v1",
        hash: "27d2eb237362b5647b020d0b22a08b079b40e50347dacc43b732830b9519a852",
        reset: 0x095a,
        bank_setter: 0x0372,
        init_bank: 51,
        bank: 57,
        index: 20,
        header: 0x52ca,
        entry: 0x5206,
        timer: false,
        spans: &[
            (0, 0x20, 8),
            (0, 0x0372, 0x3c8e),
            (51, 0x4000, 0x4000),
            (57, 0x4000, 0x4000),
            (60, 0x4000, 0x4000),
        ],
    },
    Profile {
        name: "wave-frame-banked-v2",
        hash: "41529aff78a11f826824abe2a9b0646c0fa9196f8c8df1aaee6d6d161159eac7",
        reset: 0x0985,
        bank_setter: 0x039d,
        init_bank: 51,
        bank: 57,
        index: 20,
        header: 0x52ca,
        entry: 0x5206,
        timer: false,
        spans: &[
            (0, 0x20, 8),
            (0, 0x039d, 0x3c63),
            (51, 0x4000, 0x4000),
            (57, 0x4000, 0x4000),
            (60, 0x4000, 0x4000),
        ],
    },
    Profile {
        name: "wave-frame-banked-v3",
        hash: "5f7c1cdf6adbe6d6dcef488bd37d0da82fa356b79fd579b2022a149e983abc39",
        reset: 0x095a,
        bank_setter: 0x0372,
        init_bank: 51,
        bank: 57,
        index: 20,
        header: 0x52ca,
        entry: 0x5206,
        timer: false,
        spans: &[
            (0, 0x20, 8),
            (0, 0x0372, 0x3c8e),
            (51, 0x4000, 0x4000),
            (57, 0x4000, 0x4000),
            (60, 0x4000, 0x4000),
        ],
    },
];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    if let Some(profile) = PROFILES.iter().find(|profile| profile.name == name) {
        return Some(profile);
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        super::tests::named(name)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    None
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
    if let Some(profile) = PROFILES.iter().find(|profile| profile.hash == hash) {
        return Ok(Some(profile));
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        if hash == zeff_firmware::sha256_hex(&super::tests::synthetic_rom()) {
            return Ok(Some(&super::tests::TIMER));
        }
        if hash == zeff_firmware::sha256_hex(&super::tests::synthetic_frame_rom()) {
            return Ok(Some(&super::tests::FRAME));
        }
    }
    Ok(None)
}
