use crate::{Budget, ScanStop};

pub(super) struct Selection {
    pub index: u16,
    pub bank: u16,
    pub address: u16,
    pub caller_bank: u16,
    pub caller: u16,
    pub spans: &'static [(u16, u16, u16)],
}

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub init: u16,
    pub stop: u16,
    pub start: u16,
    pub tick: u16,
    pub second_tick: u16,
    pub shadow: u16,
    pub selections: &'static [Selection],
}

const PROFILES: [Profile; 2] = [
    Profile {
        name: "timed-event-v1",
        hash: "9ecb72731dce21bbef9b98ea05bb5cd1552673705377f55963d41fcc0e530513",
        init: 0x34b5,
        stop: 0x352c,
        start: 0x34f4,
        tick: 0x3559,
        second_tick: 0x35e9,
        shadow: 0xc399,
        selections: &[
            Selection {
                index: 1,
                bank: 60,
                address: 0x4000,
                caller_bank: 0,
                caller: 0x3d20,
                spans: &[(0, 0x33fc, 0x3d4), (60, 0x4000, 0x261a)],
            },
            Selection {
                index: 2,
                bank: 60,
                address: 0x661a,
                caller_bank: 11,
                caller: 0x661e,
                spans: &[(0, 0x33fc, 0x3d4), (60, 0x4000, 0x2d3c)],
            },
        ],
    },
    Profile {
        name: "timed-event-v2",
        hash: "3e3eaca840d2bf6f23253191bb7f0e081a84499a26586a14c117507f462ceb9f",
        init: 0x35b3,
        stop: 0x362a,
        start: 0x35f2,
        tick: 0x3657,
        second_tick: 0x36e7,
        shadow: 0xc367,
        selections: &[
            Selection {
                index: 1,
                bank: 2,
                address: 0x4000,
                caller_bank: 0,
                caller: 0x3e57,
                spans: &[(0, 0x34fa, 0x3d4), (2, 0x4000, 0x242b)],
            },
            Selection {
                index: 2,
                bank: 3,
                address: 0x598e,
                caller_bank: 11,
                caller: 0x68bf,
                spans: &[(0, 0x34fa, 0x3d4), (2, 0x4000, 0x242b), (3, 0x598e, 0x722)],
            },
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
    let hash = zeff_firmware::sha256_hex(bytes);
    budget.charge()?;
    #[cfg(any(test, feature = "test-support"))]
    if bytes == super::tests::synthetic_rom() {
        return Ok(Some(&super::tests::PROFILE));
    }
    Ok(PROFILES.iter().find(|profile| profile.hash == hash))
}
