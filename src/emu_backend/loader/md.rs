use std::io::Read;
use std::path::Path;

use zeff_md_core::{CartridgeHeader, Emulator, LoadConfig, Region};

use super::{BackendLoadConfig, EmuBackend};
use crate::emu_backend::MdBackend;

const CONSOLE_START: usize = 0x100;
const CONSOLE_END: usize = 0x110;

pub(super) fn read_rom(path: &Path) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= zeff_md_core::constants::MAX_BANKED_CARTRIDGE_SIZE as u64,
        "Mega Drive cartridge exceeds the 128 MiB image limit"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(zeff_md_core::constants::MAX_BANKED_CARTRIDGE_SIZE as u64 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        Emulator::supports_cartridge_size(&bytes),
        "Unsupported Mega Drive cartridge size or mapper"
    );
    Ok(bytes)
}

pub(super) fn region(bytes: &[u8], requested: Option<Region>) -> Region {
    if let Some(region) = requested {
        return region;
    }
    CartridgeHeader::preferred_region_for_rom(bytes).unwrap_or_default()
}

pub(super) fn load(
    bytes: &[u8],
    source: &Path,
    path: &Path,
    config: &BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    anyhow::ensure!(
        Emulator::supports_cartridge_size(bytes),
        "Unsupported Mega Drive cartridge size or mapper"
    );
    let console = bytes.get(CONSOLE_START..CONSOLE_END).unwrap_or_default();
    anyhow::ensure!(
        !console.windows(3).any(|s| s == b"32X") && !console.windows(2).any(|s| s == b"CD"),
        "Mega Drive expansion media is not supported yet"
    );
    anyhow::ensure!(
        config.md_svp_functional_profile.is_none()
            || (config.md_timing == zeff_md_core::TimingModel::Standard && !config.md_use_tmss),
        "Functional SVP requires Standard timing with TMSS disabled; select those settings and reload the ROM"
    );
    anyhow::ensure!(
        !config.md_use_tmss || config.md_timing == zeff_md_core::TimingModel::Standard,
        "Mega Drive TMSS requires Standard timing; select Standard and reload the ROM"
    );
    let firmware = if config.md_svp_functional_profile.is_some() {
        Some(super::super::firmware::resolve_md_svp_with_manifest(
            config.firmware_inventory.as_deref(),
            &config.firmware_search_dirs,
            Some(source),
        )?)
    } else if config.md_use_tmss {
        Some(super::super::firmware::resolve_md_tmss_with_manifest(
            config.firmware_inventory.as_deref(),
            &config.firmware_search_dirs,
            Some(source),
        )?)
    } else {
        None
    };
    let load_config = LoadConfig {
        region: region(bytes, config.md_region),
        concurrent_sound: true,
    };
    let mut emu = if let Some(profile) = config.md_svp_functional_profile {
        let firmware = firmware.as_ref().unwrap();
        anyhow::ensure!(
            firmware.bytes.len() == 2048,
            "Invalid SVP internal ROM size"
        );
        let irom = firmware
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|word| u16::from_be_bytes(*word))
            .collect();
        Emulator::new_functional_svp(bytes.to_vec(), load_config, irom, profile)?
    } else if let Some(firmware) = &firmware {
        Emulator::new_with_tmss(bytes.to_vec(), load_config, firmware.bytes.clone())?
    } else {
        Emulator::new_with_timing(bytes.to_vec(), load_config, config.md_timing)
            .map_err(|error| anyhow::anyhow!("Invalid Mega Drive cartridge: {error:?}"))?
    };
    if let Some(rate) = config.sample_rate {
        emu.set_sample_rate(rate);
    }
    let rom_hash = zeff_firmware::sha256_bytes(bytes);
    if config.md_load_save_ram {
        super::super::md::try_load_save_ram(&mut emu, path, rom_hash)?;
    }
    let mut backend = MdBackend::new(
        emu,
        path.to_owned(),
        source.to_owned(),
        rom_hash,
        config.md_host_persistence,
    );
    if let Some(firmware) = firmware {
        backend.set_firmware_manifests(vec![firmware.manifest]);
    }
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
        let tmss_precise = BackendLoadConfig {
            md_use_tmss: true,
            md_timing: zeff_md_core::TimingModel::Precise,
            ..BackendLoadConfig::default()
        };
        let error = load(&bytes, path, path, &tmss_precise).err().unwrap();
        assert!(error.to_string().contains("TMSS requires Standard timing"));
        assert_functional_svp_loader(&bytes, path);
        bytes.resize(6 * 1024 * 1024, 0);
        bytes[CONSOLE_START..CONSOLE_END].fill(b' ');
        bytes[CONSOLE_START..CONSOLE_START + 8].copy_from_slice(b"SEGA SSF");
        let config = BackendLoadConfig {
            md_load_save_ram: false,
            md_host_persistence: false,
            ..BackendLoadConfig::default()
        };
        assert!(load(&bytes, path, path, &config).is_ok());
        bytes[CONSOLE_START..CONSOLE_START + 8].fill(0);
        assert!(load(&bytes, path, path, &config).is_err());
    }

    fn assert_functional_svp_loader(bytes: &[u8], path: &Path) {
        let profile =
            zeff_md_core::svp::FunctionalProfile::new(0x420, 1, 3, [1, 2, 3, 4, 5, 6]).unwrap();
        let source = Path::new("");
        let mut config = BackendLoadConfig {
            md_svp_functional_profile: Some(profile),
            md_load_save_ram: false,
            md_host_persistence: false,
            ..BackendLoadConfig::default()
        };
        for (timing, tmss) in [
            (zeff_md_core::TimingModel::Precise, false),
            (zeff_md_core::TimingModel::Standard, true),
            (zeff_md_core::TimingModel::Precise, true),
        ] {
            config.md_timing = timing;
            config.md_use_tmss = tmss;
            let error = load(bytes, source, path, &config).err().unwrap();
            assert!(
                error
                    .to_string()
                    .contains("Functional SVP requires Standard timing")
            );
        }
        config.md_timing = zeff_md_core::TimingModel::Standard;
        config.md_use_tmss = false;
        let error = load(bytes, source, path, &config).err().unwrap();
        assert!(error.to_string().contains("2048-byte svp.bin"));
        for size in [2048, 2047] {
            let mut inventory = zeff_firmware::FirmwareInventory::new();
            inventory.add(zeff_firmware::FirmwareInventoryEntry::from_bytes(
                vec![0; size],
                Some("svp.bin".to_owned()),
                zeff_firmware::catalog_specs(),
            ));
            config.firmware_inventory = Some(std::sync::Arc::new(inventory));
            assert!(load(bytes, source, path, &config).is_err());
        }
        config.md_svp_functional_profile = None;
        let EmuBackend::Md(ordinary) = load(bytes, source, path, &config).unwrap() else {
            unreachable!()
        };
        assert!(ordinary.core().synthetic_svp().is_none());
        assert!(ordinary.firmware_manifests().is_empty());
    }

    #[test]
    fn md_direct_reader_applies_the_cartridge_limit() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("md-bounded-reader")?;
        let path = directory.path().join("oversize.md");
        std::fs::File::create(&path)?
            .set_len(zeff_md_core::constants::MAX_CARTRIDGE_SIZE as u64 + 1)?;
        assert!(read_rom(&path).is_err());
        std::fs::File::create(&path)?
            .set_len(zeff_md_core::constants::MAX_BANKED_CARTRIDGE_SIZE as u64 + 1)?;
        assert!(read_rom(&path).is_err());
        let mut large = vec![0; 6 * 1024 * 1024];
        large[0x100..0x108].copy_from_slice(b"SEGA SSF");
        std::fs::write(&path, &large)?;
        assert_eq!(read_rom(&path)?, large);
        std::fs::write(&path, b"small")?;
        assert_eq!(read_rom(&path)?, b"small");
        Ok(())
    }

    #[test]
    fn md_korean_country_auto_region_preserves_explicit_region_requests() {
        let mut bytes = vec![0; 0x200];
        bytes[0x1f0] = b'K';
        assert_eq!(region(&bytes, None), Region::NtscJapan);
        for selected in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            assert_eq!(region(&bytes, Some(selected)), selected);
        }
    }

    #[test]
    fn md_profile_auto_region_preserves_explicit_region_requests() {
        let mut bytes = vec![0; 0x80000];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        bytes[0x180..0x18e].copy_from_slice(b"GM 00001011-00");
        bytes[0x1f0..0x1f3].copy_from_slice(b"JUE");
        bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
        bytes[0x7fffe..].copy_from_slice(&0xa3bf_u16.to_be_bytes());
        assert_eq!(
            CartridgeHeader::parse(&bytes).unwrap().calculated_checksum,
            0x532e
        );
        assert_eq!(region(&bytes, None), Region::NtscJapan);
        for selected in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            assert_eq!(region(&bytes, Some(selected)), selected);
        }
    }
}
