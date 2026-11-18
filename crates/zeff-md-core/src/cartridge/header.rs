use crate::Region;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CartridgeHeader {
    pub console: String,
    pub copyright: String,
    pub domestic_title: String,
    pub international_title: String,
    pub product: String,
    pub checksum: u16,
    pub calculated_checksum: u16,
    pub peripherals: String,
    pub rom_range: (u32, u32),
    pub work_ram_range: (u32, u32),
    pub save_declaration: [u8; 12],
    pub countries: String,
}

impl CartridgeHeader {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let header = bytes.get(..0x200)?;
        let text = |start, end| {
            encoding_rs::SHIFT_JIS
                .decode_without_bom_handling(&header[start..end])
                .0
                .trim_matches(|character: char| {
                    character.is_ascii_whitespace() || character == '\0'
                })
                .replace(|character: char| character.is_control(), " ")
        };
        let address = |start| u32::from_be_bytes(header[start..start + 4].try_into().unwrap());
        let calculated_checksum = bytes[0x200..].chunks(2).fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes([
                pair[0],
                pair.get(1).copied().unwrap_or(0),
            ]))
        });
        Some(Self {
            console: text(0x100, 0x110),
            copyright: text(0x110, 0x120),
            domestic_title: text(0x120, 0x150),
            international_title: text(0x150, 0x180),
            product: text(0x180, 0x18e),
            checksum: u16::from_be_bytes(header[0x18e..0x190].try_into().unwrap()),
            calculated_checksum,
            peripherals: text(0x190, 0x1a0),
            rom_range: (address(0x1a0), address(0x1a4)),
            work_ram_range: (address(0x1a8), address(0x1ac)),
            save_declaration: header[0x1b0..0x1bc].try_into().unwrap(),
            countries: text(0x1f0, 0x200),
        })
    }

    pub fn preferred_region_for_rom(bytes: &[u8]) -> Option<Region> {
        use sha2::{Digest, Sha256};
        let header = Self::parse(bytes)?;
        if bytes.len() == 0x80000
            && header.product == "GM 00001011-00"
            && header.calculated_checksum == 0x532e
        {
            return Some(Region::NtscJapan);
        }
        Some(
            content_region_identity(bytes.len(), || Sha256::digest(bytes).into())
                .unwrap_or_else(|| header.preferred_region()),
        )
    }

    pub fn preferred_region(&self) -> Region {
        let countries = self.countries.to_ascii_uppercase();
        if matches!(countries.as_str(), "EUR" | "EUROPE") {
            Region::Pal
        } else if countries.contains('U') {
            Region::NtscUs
        } else if countries.contains('E') {
            Region::Pal
        } else if countries.contains('J') || countries.contains('K') {
            Region::NtscJapan
        } else {
            let mask = countries
                .chars()
                .filter_map(|character| character.to_digit(16))
                .fold(0, |mask, digit| mask | digit);
            if mask & 4 != 0 {
                Region::NtscUs
            } else if mask & 8 != 0 {
                Region::Pal
            } else if mask & 1 != 0 {
                Region::NtscJapan
            } else {
                Region::NtscUs
            }
        }
    }

    pub fn declares_save_hardware(&self) -> bool {
        self.save_declaration.starts_with(b"RA")
    }
}

fn content_region_identity(len: usize, digest: impl FnOnce() -> [u8; 32]) -> Option<Region> {
    if !matches!(len, 0x4_0000 | 0x8_0000) {
        return None;
    }
    match (len, digest()) {
        (
            0x8_0000,
            [
                0x0b,
                0x8b,
                0xa2,
                0x2e,
                0xfa,
                0x9a,
                0x0e,
                0x5a,
                0x0e,
                0x92,
                0xa0,
                0x00,
                0xc7,
                0x48,
                0xab,
                0x9a,
                0xbb,
                0xf3,
                0x38,
                0x5d,
                0xec,
                0xd4,
                0x26,
                0xc3,
                0xc0,
                0x27,
                0x70,
                0x6a,
                0x96,
                0xac,
                0x97,
                0xb7,
            ],
        ) => Some(Region::Pal),
        (
            0x8_0000,
            [
                0x5d,
                0x9c,
                0xc9,
                0x71,
                0xa4,
                0xc9,
                0x5f,
                0xf4,
                0xe4,
                0xa6,
                0x23,
                0xc8,
                0xbc,
                0x05,
                0xdd,
                0xc7,
                0x9e,
                0x91,
                0x1d,
                0x81,
                0xfb,
                0xa8,
                0x7d,
                0x67,
                0x43,
                0x9e,
                0xe3,
                0x32,
                0xad,
                0x91,
                0xec,
                0x16,
            ],
        ) => Some(Region::NtscUs),
        (
            0x4_0000,
            [
                0xa3,
                0xd9,
                0x86,
                0x99,
                0xa7,
                0xdb,
                0xf7,
                0x0f,
                0x0b,
                0xed,
                0xac,
                0xc6,
                0x80,
                0xed,
                0x00,
                0x1d,
                0x2f,
                0xa5,
                0xca,
                0x2f,
                0x74,
                0xf6,
                0xb4,
                0x3a,
                0x4e,
                0xa7,
                0x51,
                0x15,
                0x98,
                0x8b,
                0x98,
                0x21,
            ],
        ) => Some(Region::NtscUs),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT_REGIONS: [(usize, [u8; 32], Region); 3] = [
        (
            0x8_0000,
            [
                0x0b, 0x8b, 0xa2, 0x2e, 0xfa, 0x9a, 0x0e, 0x5a, 0x0e, 0x92, 0xa0, 0x00, 0xc7, 0x48,
                0xab, 0x9a, 0xbb, 0xf3, 0x38, 0x5d, 0xec, 0xd4, 0x26, 0xc3, 0xc0, 0x27, 0x70, 0x6a,
                0x96, 0xac, 0x97, 0xb7,
            ],
            Region::Pal,
        ),
        (
            0x8_0000,
            [
                0x5d, 0x9c, 0xc9, 0x71, 0xa4, 0xc9, 0x5f, 0xf4, 0xe4, 0xa6, 0x23, 0xc8, 0xbc, 0x05,
                0xdd, 0xc7, 0x9e, 0x91, 0x1d, 0x81, 0xfb, 0xa8, 0x7d, 0x67, 0x43, 0x9e, 0xe3, 0x32,
                0xad, 0x91, 0xec, 0x16,
            ],
            Region::NtscUs,
        ),
        (
            0x4_0000,
            [
                0xa3, 0xd9, 0x86, 0x99, 0xa7, 0xdb, 0xf7, 0x0f, 0x0b, 0xed, 0xac, 0xc6, 0x80, 0xed,
                0x00, 0x1d, 0x2f, 0xa5, 0xca, 0x2f, 0x74, 0xf6, 0xb4, 0x3a, 0x4e, 0xa7, 0x51, 0x15,
                0x98, 0x8b, 0x98, 0x21,
            ],
            Region::NtscUs,
        ),
    ];

    #[test]
    fn bounded_header_preserves_declarations_and_checksums_actual_rom_bytes() {
        assert!(CartridgeHeader::parse(&[0; 0x1ff]).is_none());
        let mut bytes = vec![0; 0x205];
        bytes[0x120..0x127].copy_from_slice(b" Title ");
        bytes[0x150..0x154].copy_from_slice(&[b'A', 0x81, 0, b'B']);
        bytes[0x18e..0x190].copy_from_slice(&0x1234_u16.to_be_bytes());
        bytes[0x1a0..0x1a8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0xff, 0xff]);
        bytes[0x1b0..0x1bc]
            .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 3, 0xff]);
        bytes[0x200..].copy_from_slice(&[0xff, 0xff, 0, 2, 0x80]);
        let header = CartridgeHeader::parse(&bytes).unwrap();
        assert_eq!(header.domestic_title, "Title");
        assert_eq!(header.international_title, "A\u{fffd} B");
        assert_eq!(header.checksum, 0x1234);
        assert_eq!(header.calculated_checksum, 0x8001);
        assert_eq!(header.rom_range, (0, 0xffff));
        assert!(header.declares_save_hardware());
        assert_eq!(
            &header.save_declaration[4..],
            &[0, 0x20, 0, 1, 0, 0x20, 3, 0xff]
        );
        bytes[0x120..0x150].fill(b' ');
        bytes[0x120..0x124].copy_from_slice(&[0x93, 0x56, 0x8b, 0xf3]);
        assert_eq!(
            CartridgeHeader::parse(&bytes).unwrap().domestic_title,
            "天空"
        );
    }

    fn region_profile_rom() -> Vec<u8> {
        let mut bytes = vec![0; 0x80000];
        bytes[0x180..0x18e].copy_from_slice(b"GM 00001011-00");
        bytes[0x18e..0x190].copy_from_slice(&0xa4ed_u16.to_be_bytes());
        bytes[0x1f0..0x1f3].copy_from_slice(b"JUE");
        bytes[0x200..0x202].copy_from_slice(&0x532e_u16.to_be_bytes());
        bytes
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn exact_payload_profile_overrides_ambiguous_country_preference() {
        let mut bytes = region_profile_rom();
        for countries in ["JUE", "USA", "EUROPE", ""] {
            bytes[0x1f0..0x200].fill(b' ');
            bytes[0x1f0..0x1f0 + countries.len()].copy_from_slice(countries.as_bytes());
            assert_eq!(
                CartridgeHeader::parse(&bytes).unwrap().calculated_checksum,
                0x532e
            );
            assert_eq!(
                CartridgeHeader::preferred_region_for_rom(&bytes),
                Some(Region::NtscJapan)
            );
        }
        bytes[0x18e..0x190].copy_from_slice(&0x1111_u16.to_be_bytes());
        assert_eq!(
            CartridgeHeader::preferred_region_for_rom(&bytes),
            Some(Region::NtscJapan)
        );
        for (len, identity, expected) in CONTENT_REGIONS {
            assert_eq!(content_region_identity(len, || identity), Some(expected));
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn near_profile_payloads_keep_header_region_preference() {
        for size in [0x7ffff, 0x80001] {
            let mut bytes = region_profile_rom();
            bytes.resize(size, 0);
            let header = CartridgeHeader::parse(&bytes).unwrap();
            assert_eq!(header.calculated_checksum, 0x532e);
            assert_eq!(
                CartridgeHeader::preferred_region_for_rom(&bytes),
                Some(Region::NtscUs)
            );
        }
        for product in [b"GM 00001011-01", b"GM 00001012-00"] {
            let mut bytes = region_profile_rom();
            bytes[0x180..0x18e].copy_from_slice(product);
            assert_eq!(
                CartridgeHeader::preferred_region_for_rom(&bytes),
                Some(Region::NtscUs)
            );
        }
        let mut bytes = region_profile_rom();
        bytes[0x202] = 1;
        bytes[0x18e..0x190].copy_from_slice(&0x532e_u16.to_be_bytes());
        let header = CartridgeHeader::parse(&bytes).unwrap();
        assert_eq!(header.checksum, 0x532e);
        assert_eq!(header.calculated_checksum, 0x542e);
        assert_eq!(
            CartridgeHeader::preferred_region_for_rom(&bytes),
            Some(Region::NtscUs)
        );
        for size in [0, 0x200, 0x3_ffff, 0x4_0001, 0x7_ffff, 0x8_0001] {
            assert_eq!(
                content_region_identity(size, || panic!("size rejected before hashing")),
                None
            );
        }
        for (len, identity, _) in CONTENT_REGIONS {
            for other_len in [0x4_0000, 0x8_0000] {
                if other_len != len {
                    assert_eq!(content_region_identity(other_len, || identity), None);
                }
            }
            for index in 0..identity.len() {
                for bit in 0..8 {
                    let mut other = identity;
                    other[index] ^= 1 << bit;
                    assert_eq!(content_region_identity(len, || other), None);
                }
            }
        }
        bytes[0x202] = 0;
        bytes[0x180..0x18e].copy_from_slice(b"GM 00001012-00");
        bytes[0x120..0x125].copy_from_slice(b"BTTF3");
        assert_eq!(
            CartridgeHeader::parse(&bytes).unwrap().calculated_checksum,
            0x532e
        );
        assert_eq!(
            CartridgeHeader::preferred_region_for_rom(&bytes),
            Some(Region::NtscUs)
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn countries_support_letter_codes_and_hex_masks_with_stable_preference() {
        for (countries, expected) in [
            ("EUROPE", Region::Pal),
            ("Europe", Region::Pal),
            ("EUR", Region::Pal),
            ("eur", Region::Pal),
            ("USA", Region::NtscUs),
            ("Japan", Region::NtscJapan),
            ("JUE", Region::NtscUs),
            ("JE", Region::Pal),
            ("J", Region::NtscJapan),
            ("K", Region::NtscJapan),
            ("k", Region::NtscJapan),
            ("KU", Region::NtscUs),
            ("KE", Region::Pal),
            ("KJ", Region::NtscJapan),
            ("1", Region::NtscJapan),
            ("4", Region::NtscUs),
            ("8", Region::Pal),
            ("F", Region::NtscUs),
            ("2", Region::NtscUs),
            ("", Region::NtscUs),
        ] {
            let mut bytes = [0; 0x200];
            bytes[0x1f0..0x1f0 + countries.len()].copy_from_slice(countries.as_bytes());
            let header = CartridgeHeader::parse(&bytes).unwrap();
            assert_eq!(header.countries, countries);
            assert_eq!(header.preferred_region(), expected);
            assert_eq!(
                CartridgeHeader::preferred_region_for_rom(&bytes),
                Some(expected)
            );
        }
        let mut bytes = [0; 0x200];
        bytes[0x1f0..0x200].copy_from_slice(&[
            0x00, 0x04, 0x33, 0xfc, 0x8c, 0x01, 0x00, 0xc0, 0x00, 0x04, 0x33, 0xfc, 0x8d, 0x2b,
            0x00, 0xc0,
        ]);
        assert_eq!(
            CartridgeHeader::parse(&bytes).unwrap().preferred_region(),
            Region::NtscJapan
        );
        assert_eq!(
            CartridgeHeader::preferred_region_for_rom(&bytes),
            Some(Region::NtscJapan)
        );
    }
}
