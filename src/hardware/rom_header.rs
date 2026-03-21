use crate::hardware::types::CartridgeType;
use crate::hardware::types::RamSize;
use crate::hardware::types::RomSize;
use crate::hardware::types::header_offsets as header_constants;
use anyhow::{Result, anyhow};

#[derive(Debug)]
pub(crate) struct RomHeader {
    pub(crate) logo: [u8; 48],
    pub(crate) title: String,
    pub(crate) manufacturer_code: Option<String>,
    pub(crate) cartridge_type: CartridgeType,
    pub(crate) rom_size: RomSize,
    pub(crate) ram_size: RamSize,
    pub(crate) cgb_flag: u8,
    pub(crate) sgb_flag: u8,
    pub(crate) is_cgb_compatible: bool,
    pub(crate) is_cgb_exclusive: bool,
    pub(crate) is_sgb_supported: bool,
    pub(crate) destination: u8,
    pub(crate) is_overseas: bool,
    pub(crate) old_licensee_code: u8,
    pub(crate) new_licensee_code: Option<String>,
    pub(crate) use_new_licensee_code: bool,
    pub(crate) game_version: u8,
    pub(crate) header_checksum: u8,
    pub(crate) global_checksum: u16,
}

impl RomHeader {
    pub(crate) fn from_rom(rom: &[u8]) -> Result<Self> {
        let logo = rom
            .get(header_constants::LOGO_START..header_constants::LOGO_END)
            .ok_or_else(|| {
                anyhow!(
                    "ROM too small for logo: expected {} bytes",
                    header_constants::LOGO_END - header_constants::LOGO_START
                )
            })?
            .try_into()
            .map_err(|_| anyhow!("Logo conversion failed: logo bytes incorrect length"))?;

        let cartridge_type = CartridgeType::from_byte(
            *rom.get(header_constants::CARTRIDGE_TYPE_IDX)
                .ok_or_else(|| anyhow!("cartridge_type missing"))?,
        );
        let rom_size = RomSize::from_byte(
            *rom.get(header_constants::ROM_SIZE_IDX)
                .ok_or_else(|| anyhow!("rom_size missing"))?,
        );
        let ram_size = RamSize::from_byte(
            *rom.get(header_constants::RAM_SIZE_IDX)
                .ok_or_else(|| anyhow!("ram_size missing"))?,
        );

        let cgb_flag = *rom
            .get(header_constants::CGB_FLAG_IDX)
            .ok_or_else(|| anyhow!("CGB flag missing"))?;
        let sgb_flag = *rom
            .get(header_constants::SGB_FLAG_IDX)
            .ok_or_else(|| anyhow!("SGB flag missing"))?;

        let is_cgb_compatible = cgb_flag == header_constants::CGB_FLAG_COMPATIBLE
            || cgb_flag == header_constants::CGB_FLAG_EXCLUSIVE;
        let is_cgb_exclusive = cgb_flag == header_constants::CGB_FLAG_EXCLUSIVE;
        let is_sgb_supported = sgb_flag == header_constants::SGB_FLAG_SUPPORTED;

        let destination = *rom
            .get(header_constants::DESTINATION_IDX)
            .ok_or_else(|| anyhow!("destination missing"))?;
        let is_overseas = destination == header_constants::OVERSEAS_CODE;
        let old_licensee_code = *rom
            .get(header_constants::LICENSEE_CODE_IDX)
            .ok_or_else(|| anyhow!("old_licensee_code missing"))?;
        let use_new_licensee_code = old_licensee_code == header_constants::NEW_LICENSEE_CODE_MAGIC;

        let new_licensee_code = if use_new_licensee_code {
            let code_bytes = rom
                .get(
                    header_constants::NEW_LICENSEE_CODE_START
                        ..header_constants::NEW_LICENSEE_CODE_END,
                )
                .ok_or_else(|| anyhow!("new licensee code missing"))?;
            Some(String::from_utf8_lossy(code_bytes).trim().to_string())
        } else {
            None
        };

        let game_version = *rom
            .get(header_constants::GAME_VERSION_IDX)
            .ok_or_else(|| anyhow!("game_version missing"))?;

        let (title_range, manufacturer_range) = if is_cgb_compatible {
            (
                header_constants::TITLE_START..header_constants::TITLE_END_CGB,
                header_constants::MANUFACTURER_CODE_START..header_constants::MANUFACTURER_CODE_END,
            )
        } else if use_new_licensee_code {
            (
                header_constants::TITLE_START..header_constants::TITLE_END_NEW,
                header_constants::MANUFACTURER_CODE_START
                    ..header_constants::MANUFACTURER_CODE_START,
            )
        } else {
            (
                header_constants::TITLE_START..header_constants::TITLE_END_OLD,
                header_constants::MANUFACTURER_CODE_START
                    ..header_constants::MANUFACTURER_CODE_START,
            )
        };

        let title_bytes = rom
            .get(title_range.clone())
            .ok_or_else(|| anyhow!("ROM too small for title: expected {:?} bytes", title_range))?;
        let title = String::from_utf8_lossy(title_bytes)
            .trim_end_matches('\0')
            .to_string();

        let manufacturer_code = if is_cgb_compatible {
            let code_bytes = rom.get(manufacturer_range.clone()).ok_or_else(|| {
                anyhow!(
                    "ROM too small for manufacturer code: expected {:?} bytes",
                    manufacturer_range
                )
            })?;
            let code_string = String::from_utf8_lossy(code_bytes)
                .trim_end_matches('\0')
                .to_string();
            if code_string.is_empty() {
                None
            } else {
                Some(code_string)
            }
        } else {
            None
        };

        let header_checksum = *rom
            .get(header_constants::HEADER_CHECKSUM_IDX)
            .ok_or_else(|| anyhow!("header_checksum missing"))?;

        let global_checksum_bytes = rom
            .get(header_constants::GLOBAL_CHECKSUM_START..header_constants::GLOBAL_CHECKSUM_END)
            .ok_or_else(|| anyhow!("global_checksum missing"))?;
        let global_checksum =
            u16::from_be_bytes([global_checksum_bytes[0], global_checksum_bytes[1]]);

        Ok(Self {
            logo,
            title,
            manufacturer_code,
            cartridge_type,
            rom_size,
            ram_size,
            cgb_flag,
            sgb_flag,
            is_cgb_compatible,
            is_cgb_exclusive,
            is_sgb_supported,
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

    pub(crate) fn display_info(&self, rom: &[u8]) {
        log::info!("--- ROM HEADER INFO ---");
        log::info!("Title: {}", self.title);
        log::info!(
            "Manufacturer: {}",
            self.manufacturer_code.as_deref().unwrap_or("Unknown")
        );
        log::info!("Cartridge Type: {:?}", self.cartridge_type);
        log::info!("ROM Size: {:?}", self.rom_size);
        log::info!("RAM Size: {:?}", self.ram_size);
        log::info!(
            "CGB Flag: {:#04X} (Compatible: {}, Exclusive: {})",
            self.cgb_flag,
            self.is_cgb_compatible,
            self.is_cgb_exclusive
        );
        log::info!(
            "SGB Flag: {:#04X} (Supported: {})",
            self.sgb_flag,
            self.is_sgb_supported
        );
        log::info!(
            "Destination: {:#04X} (Overseas: {})",
            self.destination,
            self.is_overseas
        );
        log::info!("Licensee (old): {:#04X}", self.old_licensee_code);
        log::info!(
            "Licensee (new): {}",
            self.new_licensee_code.as_deref().unwrap_or("N/A")
        );
        log::info!("Game Version: {}", self.game_version);
        log::info!("Header Checksum: {:#04X}", self.header_checksum);
        log::info!("Global Checksum: {:#06X}", self.global_checksum);
        log::info!(
            "Header checksum valid: {}",
            self.verify_header_checksum(rom)
        );
        log::info!(
            "Global checksum valid: {}",
            self.verify_global_checksum(rom)
        );
        log::info!("Publisher: {}", self.publisher());
        log::info!("-----------------------");
    }

    pub(crate) fn verify_header_checksum(&self, rom: &[u8]) -> bool {
        let mut checksum: u8 = 0;
        for addr in 0x0134..=0x014C {
            let byte = rom.get(addr).copied().unwrap_or(0);
            checksum = checksum.wrapping_sub(byte).wrapping_sub(1);
        }
        checksum == self.header_checksum
    }

    pub(crate) fn verify_global_checksum(&self, rom: &[u8]) -> bool {
        let mut checksum: u16 = 0;
        for (i, &byte) in rom.iter().enumerate() {
            if i != 0x014E && i != 0x014F {
                checksum = checksum.wrapping_add(byte as u16);
            }
        }
        checksum == self.global_checksum
    }

    pub(crate) fn publisher(&self) -> &'static str {
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
