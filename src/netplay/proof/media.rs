use std::ffi::OsString;
use std::io::{Read, Write};
use zeff_nes_core::hardware::cartridge::TimingMode;

use super::*;

const MAX_CARTRIDGE_BYTES: u64 = 64 * 1024 * 1024;
const USAGE: &str = "usage: --netplay-worker-proof OUTPUT_DIRECTORY [FRAMES] [--rom DIRECT_NES_FILE | --timing ntsc|pal|dendy]";

pub(crate) fn fixture_rom(timing: TimingMode) -> Vec<u8> {
    let mut bytes = zeff_netplay::fixture::rom();
    match timing {
        TimingMode::Ntsc => {}
        TimingMode::Pal => bytes[9] = 1,
        TimingMode::Dendy => {
            bytes[7] = (bytes[7] & 0xf0) | 0x08;
            bytes[10] = 0x70; // NES 2.0 declares 8 KiB of nonvolatile PRG RAM.
            bytes[12] = 3;
        }
        TimingMode::MultiRegion => unreachable!("proof fixtures require resolved timing"),
    }
    bytes
}

pub(super) struct Options {
    pub(super) root: PathBuf,
    pub(super) frames: u64,
    pub(super) cartridge: Option<PathBuf>,
    pub(super) timing: TimingMode,
}

impl Options {
    pub(super) fn parse(arguments: &[OsString]) -> Result<Self> {
        let (root, tail) = arguments.split_first().context(USAGE)?;
        let (frames, selection) = match tail {
            [] => (None, None),
            [frames] => (Some(frames), None),
            [flag, value] => (None, Some((flag, value))),
            [frames, flag, value] => (Some(frames), Some((flag, value))),
            _ => bail!(USAGE),
        };
        let (cartridge, timing) = match selection {
            None => (None, TimingMode::Ntsc),
            Some((flag, path)) if flag == "--rom" => (Some(PathBuf::from(path)), TimingMode::Ntsc),
            Some((flag, value)) if flag == "--timing" && value == "ntsc" => {
                (None, TimingMode::Ntsc)
            }
            Some((flag, value)) if flag == "--timing" && value == "pal" => (None, TimingMode::Pal),
            Some((flag, value)) if flag == "--timing" && value == "dendy" => {
                (None, TimingMode::Dendy)
            }
            _ => bail!(USAGE),
        };
        let frames = frames
            .map(|value| value.to_string_lossy().parse::<u64>())
            .transpose()
            .context(USAGE)?
            .unwrap_or(300);
        ensure!((8..=100_000).contains(&frames), "frames must be 8..100000");
        Ok(Self {
            root: PathBuf::from(root),
            frames,
            cartridge,
            timing,
        })
    }
}

pub(super) struct Media {
    bytes: Vec<u8>,
    pub(super) synthetic: bool,
}

impl Media {
    pub(super) fn fixture() -> Self {
        Self::fixture_timing(TimingMode::Ntsc)
    }

    pub(super) fn fixture_timing(timing: TimingMode) -> Self {
        Self {
            bytes: fixture_rom(timing),
            synthetic: true,
        }
    }

    pub(super) fn cartridge(path: &Path) -> Result<Self> {
        ensure!(
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nes")),
            "proof requires a direct .nes file"
        );
        let file = std::fs::File::open(path).context("opening proof cartridge")?;
        ensure!(file.metadata()?.is_file(), "proof cartridge must be a file");
        let mut bytes = Vec::new();
        file.take(MAX_CARTRIDGE_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            (16..=MAX_CARTRIDGE_BYTES).contains(&(bytes.len() as u64)),
            "proof cartridge must be 16 bytes..64 MiB"
        );
        ensure!(
            &bytes[..4] == b"NES\x1a",
            "proof requires an iNES cartridge"
        );
        Ok(Self {
            bytes,
            synthetic: false,
        })
    }

    pub(super) fn load(&self, root: &Path, name: &str) -> Result<(PathBuf, EmuBackend)> {
        let path = root.join(format!("{name}.nes"));
        let save_path = crate::save_paths::sram_path_for_rom(&path);
        ensure!(
            save_path == path.with_extension("sav"),
            "proof output must not have archive-named ancestors"
        );
        ensure!(!save_path.exists(), "proof needs unused save paths");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("proof needs unused media paths")?
            .write_all(&self.bytes)?;
        let mut backend = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &path,
            &path,
            None,
            BackendLoadConfig {
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?
        .backend;
        ensure!(
            backend.rom_hash() == <[u8; 32]>::from(Sha256::digest(&self.bytes)),
            "proof cartridge bytes changed during loading"
        );
        let EmuBackend::Nes(nes) = &mut backend else {
            bail!("proof lost NES backend");
        };
        if self.synthetic {
            ensure!(nes.emu.has_battery(), "fixture lacks SRAM");
        } else {
            // A cartridge proof must never publish even restored save data.
            nes.set_host_persistence_enabled(false);
        }
        Ok((path, backend))
    }
}

#[cfg(test)]
mod tests;
