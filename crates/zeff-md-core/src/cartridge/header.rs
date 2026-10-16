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

    pub fn preferred_region(&self) -> Region {
        let countries = self.countries.to_ascii_uppercase();
        if countries.contains('U') {
            Region::NtscUs
        } else if countries.contains('E') {
            Region::Pal
        } else if countries.contains('J') {
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn countries_support_letter_codes_and_hex_masks_with_stable_preference() {
        for (countries, expected) in [
            ("JUE", Region::NtscUs),
            ("JE", Region::Pal),
            ("J", Region::NtscJapan),
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
        }
    }
}
