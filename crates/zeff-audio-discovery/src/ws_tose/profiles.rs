use crate::{Budget, ScanStop};

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub name: &'static str,
    pub len: usize,
    pub fixed: usize,
    pub end: usize,
    pub hash: &'static str,
    pub segment: u16,
    pub init: u16,
    pub selector: u16,
    pub tick: u16,
    pub status: u16,
    pub slots: u16,
    pub wave: u16,
    pub envelope: u16,
    pub frequency: u16,
    pub counts: &'static [u16],
}

impl Profile {
    pub fn offset(self, address: u16) -> usize {
        (self.fixed & !0xffff) + usize::from(address)
    }
    pub fn span(self) -> crate::RomSpan {
        crate::RomSpan {
            effective_offset: self.fixed as u32,
            byte_len: (self.end - self.fixed) as u32,
            canonical_cpu_address: u32::from(self.segment) * 16 + (self.fixed & 0xffff) as u32,
        }
    }
}

const PROFILES: &[Profile] = &[
    Profile {
        name: "ws-tose-eight-slot-v1",
        len: 0x800000,
        fixed: 0x7f7e80,
        end: 0x7f9a8c,
        hash: "2d4f6c3dbd279b4015d59eded67c9b036ed62728b230aea9242c27200a38dd3a",
        segment: 0xf000,
        init: 0x8d8d,
        selector: 0x8ef4,
        tick: 0x90f2,
        status: 0x8e73,
        slots: 0x1e47,
        wave: 0x7ea1,
        envelope: 0x88a1,
        frequency: 0x8943,
        counts: &[21, 235, 96],
    },
    Profile {
        name: "ws-tose-eight-slot-v2",
        len: 0x200000,
        fixed: 0x1f4900,
        end: 0x1f5ad5,
        hash: "91bfe33817fc0247821ccfab65504155545751094ac4d4245e93b0098359d32d",
        segment: 0xf000,
        init: 0x4edb,
        selector: 0x5042,
        tick: 0x5217,
        status: 0x4fc1,
        slots: 0x1521,
        wave: 0x4921,
        envelope: 0x4a21,
        frequency: 0x4ac3,
        counts: &[29, 104],
    },
    Profile {
        name: "ws-tose-eight-slot-v3",
        len: 0x400000,
        fixed: 0x3f1be0,
        end: 0x3f3bbe,
        hash: "47410f403c803db8cd2b723563210e3edc73162723cb886ea64d7b26454ef726",
        segment: 0xf000,
        init: 0x2df1,
        selector: 0x2f58,
        tick: 0x312d,
        status: 0x2ed7,
        slots: 0x120d,
        wave: 0x1c01,
        envelope: 0x2601,
        frequency: 0x27c3,
        counts: &[25, 213, 336],
    },
    Profile {
        name: "ws-tose-eight-slot-v4",
        len: 0x400000,
        fixed: 0x3f8c40,
        end: 0x3f9e7b,
        hash: "eeae6f2941f56c882ceceec4e2cbba748d4171cb43d4e09c8ac67c8e532114de",
        segment: 0xf000,
        init: 0x926b,
        selector: 0x953c,
        tick: 0x9711,
        status: 0x9362,
        slots: 0xe1,
        wave: 0x8c61,
        envelope: 0x8dc1,
        frequency: 0x8e63,
        counts: &[87, 72],
    },
    Profile {
        name: "ws-tose-eight-slot-v5",
        len: 0x400000,
        fixed: 0x3fae50,
        end: 0x3fca5c,
        hash: "7c7e2f09b22a1f4dc2e19aa10adb16d1c2a3a295fa2129f6b4df57287e2a9130",
        segment: 0xf000,
        init: 0xbd5d,
        selector: 0xbec4,
        tick: 0xc0c2,
        status: 0xbe43,
        slots: 0x1e47,
        wave: 0xae71,
        envelope: 0xb871,
        frequency: 0xb913,
        counts: &[21, 235, 96],
    },
    Profile {
        name: "ws-tose-eight-slot-v6",
        len: 0x400000,
        fixed: 0x3f2980,
        end: 0x3f445b,
        hash: "9d8966480b03a78fb4edfbced4c99f028b23f41cb0b8d945681cddec3fd33a1f",
        segment: 0xf000,
        init: 0x3861,
        selector: 0x39c8,
        tick: 0x3b9d,
        status: 0x3947,
        slots: 0x1a1,
        wave: 0x29a1,
        envelope: 0x33a1,
        frequency: 0x3443,
        counts: &[24, 96],
    },
    Profile {
        name: "ws-tose-eight-slot-v7",
        len: 0x200000,
        fixed: 0x1f4600,
        end: 0x1f58b6,
        hash: "5193d83c806b7a21dd02de8fa1e530913bc74c26ddb61a89d0c13fb78edbebe4",
        segment: 0xf000,
        init: 0x4b8d,
        selector: 0x4cf4,
        tick: 0x4ef2,
        status: 0x4c73,
        slots: 0x1e47,
        wave: 0x4621,
        envelope: 0x4721,
        frequency: 0x47c3,
        counts: &[87],
    },
];

pub(super) fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<Profile>, ScanStop> {
    for &profile in PROFILES {
        budget.charge()?;
        if bytes.len() != profile.len {
            continue;
        }
        for _ in 0..(profile.end - profile.fixed).div_ceil(256) {
            budget.charge()?;
        }
        if zeff_firmware::sha256_hex(&bytes[profile.fixed..profile.end]) == profile.hash {
            return Ok(Some(profile));
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    if super::tests::recognized(bytes) {
        return Ok(Some(super::tests::PROFILE));
    }
    Ok(None)
}
