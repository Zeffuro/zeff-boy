use std::io::Read;
use std::path::Path;

use zeff_md_core::{CartridgeHeader, Emulator, LoadConfig, Region};

use super::{BackendLoadConfig, EmuBackend};
use crate::emu_backend::MdBackend;

const CONSOLE_START: usize = 0x100;
const CONSOLE_END: usize = 0x110;

pub(super) fn read_rom(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(zeff_md_core::constants::MAX_CARTRIDGE_SIZE as u64 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= zeff_md_core::constants::MAX_CARTRIDGE_SIZE,
        "Mega Drive cartridge exceeds the supported 4 MiB limit"
    );
    Ok(bytes)
}

pub(super) fn region(bytes: &[u8], requested: Option<Region>) -> Region {
    if let Some(region) = requested {
        return region;
    }
    CartridgeHeader::parse(bytes)
        .map(|header| header.preferred_region())
        .unwrap_or_default()
}

pub(super) fn load(
    bytes: &[u8],
    source: &Path,
    path: &Path,
    config: &BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    anyhow::ensure!(
        bytes.len() <= zeff_md_core::constants::MAX_CARTRIDGE_SIZE,
        "Mega Drive cartridge exceeds the supported 4 MiB limit"
    );
    let console = bytes.get(CONSOLE_START..CONSOLE_END).unwrap_or_default();
    anyhow::ensure!(
        !console.windows(3).any(|s| s == b"32X") && !console.windows(2).any(|s| s == b"CD"),
        "Mega Drive expansion media is not supported yet"
    );
    let mut emu = Emulator::new(
        bytes.to_vec(),
        LoadConfig {
            region: region(bytes, config.md_region),
            concurrent_sound: true,
        },
    )
    .map_err(|error| anyhow::anyhow!("Invalid Mega Drive cartridge: {error:?}"))?;
    if let Some(rate) = config.sample_rate {
        emu.set_sample_rate(rate);
    }
    let rom_hash = zeff_firmware::sha256_bytes(bytes);
    if config.md_load_save_ram {
        super::super::md::try_load_save_ram(&mut emu, path, rom_hash)?;
    }
    let mut backend = MdBackend::new(emu, path.to_owned(), source.to_owned(), rom_hash);
    backend.set_controllers(config.md_controller);
    Ok(EmuBackend::Md(Box::new(backend)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAM_HEADER_START: usize = 0x1b0;

    #[test]
    fn md_loader_rejects_unimplemented_cartridge_hardware() {
        let mut bytes = vec![0; 0x400];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
        let path = Path::new("hardware.md");
        bytes[RAM_HEADER_START..RAM_HEADER_START + 2].copy_from_slice(b"RA");
        assert!(load(&bytes, path, path, &BackendLoadConfig::default()).is_err());
        bytes[RAM_HEADER_START..RAM_HEADER_START + 2].fill(0);
        bytes[CONSOLE_START..CONSOLE_END].copy_from_slice(b"SEGA 32X        ");
        assert!(load(&bytes, path, path, &BackendLoadConfig::default()).is_err());
        bytes[CONSOLE_START..CONSOLE_END].copy_from_slice(b"SEGA MEGA DRIVE ");
        assert!(load(&bytes, path, path, &BackendLoadConfig::default()).is_ok());
    }

    #[test]
    fn md_direct_reader_applies_the_cartridge_limit() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("md-bounded-reader")?;
        let path = directory.path().join("oversize.md");
        std::fs::File::create(&path)?
            .set_len(zeff_md_core::constants::MAX_CARTRIDGE_SIZE as u64 + 1)?;
        assert!(read_rom(&path).is_err());
        std::fs::write(&path, b"small")?;
        assert_eq!(read_rom(&path)?, b"small");
        Ok(())
    }
}
