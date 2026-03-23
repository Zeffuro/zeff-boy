#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheatType {
    GameShark,
    Raw,
}

#[derive(Debug, Clone)]
pub(crate) struct CheatCode {
    pub(crate) name: String,
    pub(crate) code_text: String,
    pub(crate) address: u16,
    pub(crate) value: u8,
    pub(crate) enabled: bool,
    pub(crate) code_type: CheatType,
}

pub(crate) fn parse_cheat(input: &str) -> Result<(u16, u8, CheatType), &'static str> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();

    if let Some((addr_str, val_str)) = cleaned.split_once(':') {
        let addr = u16::from_str_radix(addr_str, 16)
            .map_err(|_| "Invalid hex address")?;
        let value = u8::from_str_radix(val_str, 16)
            .map_err(|_| "Invalid hex value")?;
        return Ok((addr, value, CheatType::Raw));
    }

    if cleaned.len() == 8 {
        let digits: Vec<u8> = cleaned
            .chars()
            .map(|c| {
                c.to_digit(16)
                    .map(|d| d as u8)
                    .ok_or("Invalid hex digit")
            })
            .collect::<Result<Vec<_>, _>>()?;

        let code_type_byte = (digits[0] << 4) | digits[1];
        let value = (digits[2] << 4) | digits[3];
        let addr_hi = (digits[4] << 4) | digits[5];
        let addr_lo = (digits[6] << 4) | digits[7];
        let address = u16::from(addr_hi) << 8 | u16::from(addr_lo);

        if code_type_byte != 0x01 {
            log::warn!(
                "GameShark code type {:02X} (expected 01), treating as RAM write",
                code_type_byte
            );
        }

        return Ok((address, value, CheatType::GameShark));
    }

    Err("Unrecognized format. Use GameShark (01VVAAAA) or raw (AAAA:VV)")
}

pub(crate) fn collect_active_cheats(cheats: &[CheatCode]) -> Vec<(u16, u8)> {
    cheats
        .iter()
        .filter(|c| c.enabled)
        .map(|c| (c.address, c.value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gameshark() {
        let (addr, val, ty) = parse_cheat("01FF C0DE").unwrap();
        assert_eq!(addr, 0xC0DE);
        assert_eq!(val, 0xFF);
        assert_eq!(ty, CheatType::GameShark);
    }

    #[test]
    fn parse_raw() {
        let (addr, val, ty) = parse_cheat("C000:42").unwrap();
        assert_eq!(addr, 0xC000);
        assert_eq!(val, 0x42);
        assert_eq!(ty, CheatType::Raw);
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_cheat("not a code").is_err());
    }
}

