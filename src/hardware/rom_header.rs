use anyhow::{anyhow, Result};
use crate::hardware::types::cartridge_type::CartridgeType;
use crate::hardware::types::ram_size::RamSize;
use crate::hardware::types::rom_size::RomSize;
use crate::hardware::types::header_constants;

pub struct RomHeader {
    pub logo: [u8; 48],
    pub title: String,
    pub cartridge_type: CartridgeType,
    pub rom_size: RomSize,
    pub ram_size: RamSize,
    pub destination: u8,
    pub is_overseas: bool,
    pub old_licensee_code: u8,
    pub new_licensee_code: Option<String>,
    pub use_new_licensee_code: bool,
    pub game_version: u8,
    pub header_checksum: u8,
    pub global_checksum: u16,
}

impl RomHeader {
    pub fn from_rom(rom: &[u8]) -> Result<Self> {
        let logo = rom.get(header_constants::LOGO_START..header_constants::LOGO_END)
            .ok_or_else(|| anyhow!("ROM too small for logo: expected {} bytes", header_constants::LOGO_END - header_constants::LOGO_START))?
            .try_into()
            .map_err(|_| anyhow!("Logo conversion failed: logo bytes incorrect length"))?;

        let cartridge_type = CartridgeType::from_byte(*rom.get(header_constants::CARTRIDGE_TYPE_IDX).ok_or_else(|| anyhow!("cartridge_type missing"))?);
        let rom_size = RomSize::from_byte(*rom.get(header_constants::ROM_SIZE_IDX).ok_or_else(|| anyhow!("rom_size missing"))?);
        let ram_size = RamSize::from_byte(*rom.get(header_constants::RAM_SIZE_IDX).ok_or_else(|| anyhow!("ram_size missing"))?);
        let destination = *rom.get(header_constants::DESTINATION_IDX).ok_or_else(|| anyhow!("destination missing"))?;
        let is_overseas = destination == header_constants::OVERSEAS_CODE;
        let old_licensee_code = *rom.get(header_constants::LICENSEE_CODE_IDX).ok_or_else(|| anyhow!("old_licensee_code missing"))?;
        let use_new_licensee_code = old_licensee_code == header_constants::NEW_LICENSEE_CODE_MAGIC;

        let new_licensee_code = if use_new_licensee_code {
            let code_bytes = rom.get(header_constants::NEW_LICENSEE_CODE_START..header_constants::NEW_LICENSEE_CODE_END)
                .ok_or_else(|| anyhow!("new licensee code missing"))?;
            Some(String::from_utf8_lossy(code_bytes).trim().to_string())
        } else {
            None
        };

        let game_version = *rom.get(header_constants::GAME_VERSION_IDX).ok_or_else(|| anyhow!("game_version missing"))?;

        let title_range = if use_new_licensee_code {
            header_constants::TITLE_START..header_constants::TITLE_END_NEW
        } else {
            header_constants::TITLE_START..header_constants::TITLE_END_OLD
        };
        
        let title_bytes = rom.get(title_range.clone())
            .ok_or_else(|| anyhow!("ROM too small for title: expected {:?} bytes", title_range))?;
        let title = String::from_utf8_lossy(title_bytes)
            .trim_end_matches('\0')
            .to_string();

        let header_checksum = *rom.get(header_constants::HEADER_CHECKSUM_IDX).ok_or_else(|| anyhow!("header_checksum missing"))?;

        let global_checksum_bytes = rom.get(header_constants::GLOBAL_CHECKSUM_START..header_constants::GLOBAL_CHECKSUM_END)
            .ok_or_else(|| anyhow!("global_checksum missing"))?;
        let global_checksum = u16::from_be_bytes([global_checksum_bytes[0], global_checksum_bytes[1]]);

        Ok(Self {
            logo,
            title,
            cartridge_type,
            rom_size,
            ram_size,
            destination,
            is_overseas,
            old_licensee_code,
            new_licensee_code,
            use_new_licensee_code,
            game_version,
            header_checksum,
            global_checksum,
        })
    }

    pub fn publisher(&self) -> &'static str {
        if self.use_new_licensee_code {
            if let Some(ref code) = self.new_licensee_code {
                crate::hardware::types::new_licensee::new_licensee_name(code)
            } else {
                "Unknown"
            }
        } else {
            crate::hardware::types::old_licensee::old_licensee_name(self.old_licensee_code)
        }
    }
}