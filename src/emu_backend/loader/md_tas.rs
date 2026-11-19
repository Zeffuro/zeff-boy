use std::io::Read as _;
use std::path::Path;

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::time::FrameLifecycle;
use zeff_md_core::emulator::rollback::{MdRollbackConfig, MdRollbackHashes};
use zeff_md_core::{ControllerKind, ControllerPort, Region, TimingModel};

use super::{BackendLoadConfig, EmuBackend, has_extension};
use crate::emu_core_trait::EmulatorCore;

mod seed;
pub(crate) use seed::MdTasSeed;

pub(crate) const MAX_MD_TAS_ROM_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_MD_TAS_OWNED_PAYLOAD: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_MD_TAS_ZIP_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MD_TAS_DETERMINISM_ABI: &str = "md-standard-us-concurrent-two-pad-v1";
pub(crate) const MD_TAS_STATE_COMPATIBILITY: &str = "md-raw-v1-fresh-project-owned-runtime-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MdTasProfile {
    ThreeButton,
    SixButton,
}

impl MdTasProfile {
    pub(crate) fn kind(self) -> ControllerKind {
        match self {
            Self::ThreeButton => ControllerKind::ThreeButton,
            Self::SixButton => ControllerKind::SixButton,
        }
    }

    pub(crate) fn button_mask(self) -> u8 {
        match self {
            Self::ThreeButton => 0x4b,
            Self::SixButton => 0xff,
        }
    }

    pub(crate) fn device(self) -> &'static str {
        match self {
            Self::ThreeButton => "md-three-button",
            Self::SixButton => "md-six-button",
        }
    }

    pub(crate) fn sync_hash(self, member: Option<&str>) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(
            b"zeff-md-tas-standard-us-concurrent-48000-full-no-save-firmware-mod-serial-v1\0",
        );
        hash.update(self.device().as_bytes());
        hash.update([u8::from(member.is_some())]);
        if let Some(member) = member {
            hash.update((member.len() as u64).to_le_bytes());
            hash.update(member.as_bytes());
        }
        hash.finalize().into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MdTasSource {
    Direct {
        sha256: [u8; 32],
        len: u64,
    },
    Zip {
        archive_sha256: [u8; 32],
        archive_len: u64,
        member: String,
        member_sha256: [u8; 32],
        member_len: u64,
    },
}

impl MdTasSource {
    pub(crate) fn sha256(&self) -> [u8; 32] {
        match self {
            Self::Direct { sha256, .. } => *sha256,
            Self::Zip { archive_sha256, .. } => *archive_sha256,
        }
    }

    pub(crate) fn member(&self) -> Option<&str> {
        match self {
            Self::Direct { .. } => None,
            Self::Zip { member, .. } => Some(member),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MdTasLoad {
    source: MdTasSource,
    effective_sha256: [u8; 32],
    effective_len: u64,
    profile: MdTasProfile,
    config: MdRollbackConfig,
    fresh_raw: [u8; 32],
    fresh_hashes: MdRollbackHashes,
}

impl MdTasLoad {
    pub(crate) fn source(&self) -> &MdTasSource {
        &self.source
    }

    pub(crate) fn profile(&self) -> MdTasProfile {
        self.profile
    }

    pub(crate) fn effective_sha256(&self) -> [u8; 32] {
        self.effective_sha256
    }

    pub(crate) fn sync_hash(&self) -> [u8; 32] {
        self.profile.sync_hash(self.source.member())
    }

    pub(crate) fn config(&self) -> &MdRollbackConfig {
        &self.config
    }

    pub(crate) fn validate_fresh(&self, md: &crate::emu_backend::md::MdBackend) -> Result<()> {
        self.validate_fixed(md)?;
        let config = MdRollbackConfig::capture(md.core())?;
        ensure!(
            md.frame_count() == 0
                && <[u8; 32]>::from(Sha256::digest(md.encode_state_bytes()?)) == self.fresh_raw,
            "MD TAS fresh configuration or RAW baseline changed"
        );
        ensure!(
            config.boundary_hashes(md.core())? == self.fresh_hashes,
            "MD TAS fresh owned runtime changed"
        );
        Ok(())
    }

    pub(crate) fn validate_fixed(&self, md: &crate::emu_backend::md::MdBackend) -> Result<()> {
        ensure!(
            MdRollbackConfig::capture(md.core())? == self.config
                && md.rom_hash() == self.effective_sha256
                && md.core().rom_bytes().len() as u64 == self.effective_len
                && md.firmware_manifests().is_empty(),
            "MD TAS fixed loaded configuration changed"
        );
        Ok(())
    }

    pub(crate) fn validate_source(&self, md: &crate::emu_backend::md::MdBackend) -> Result<()> {
        self.validate_paths(md.source_path(), md.rom_path())
    }

    fn validate_paths(&self, source: &Path, rom: &Path) -> Result<()> {
        match &self.source {
            MdTasSource::Direct { sha256, len } => {
                ensure!(
                    std::fs::metadata(source)?.len() == *len,
                    "MD TAS source length changed"
                );
                let mut bytes = Vec::new();
                std::fs::File::open(source)?
                    .take(*len + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 == *len && zeff_firmware::sha256_bytes(&bytes) == *sha256,
                    "MD TAS source bytes changed"
                );
            }
            MdTasSource::Zip {
                archive_sha256,
                archive_len,
                member,
                member_sha256,
                member_len,
            } => {
                let extension =
                    extension(rom).ok_or_else(|| anyhow::anyhow!("MD member extension changed"))?;
                let selected = crate::rom_archive::extract_bounded_zip_member(
                    source,
                    Some(rom),
                    extension,
                    MAX_MD_TAS_ZIP_BYTES,
                    MAX_MD_TAS_ROM_BYTES,
                )?;
                ensure!(
                    selected.archive_sha256 == *archive_sha256
                        && selected.archive_len as u64 == *archive_len
                        && selected.member_name == *member
                        && selected.bytes.len() as u64 == *member_len
                        && zeff_firmware::sha256_bytes(&selected.bytes) == *member_sha256,
                    "MD TAS archive or selected member changed"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn retained_source_bytes(&self) -> usize {
        match &self.source {
            MdTasSource::Direct { .. } => 0,
            MdTasSource::Zip { member, .. } => member.capacity(),
        }
    }
}

pub(super) fn extension(path: &Path) -> Option<&'static str> {
    ["md", "gen", "bin"]
        .into_iter()
        .find(|extension| has_extension(path, extension))
}

pub(super) struct MdTasLoadContext<'a> {
    pub(super) source: &'a Path,
    pub(super) rom: &'a Path,
    pub(super) from_path: bool,
    pub(super) settings: &'a BackendLoadConfig,
    pub(super) raw_sha256: Option<[u8; 32]>,
    pub(super) len: usize,
    pub(super) unmodified: bool,
}

pub(super) fn capture(backend: &mut EmuBackend, context: MdTasLoadContext<'_>) {
    let MdTasLoadContext {
        source,
        rom,
        from_path,
        settings,
        raw_sha256,
        len,
        unmodified,
    } = context;
    let EmuBackend::Md(md) = backend else { return };
    let load = (|| -> Result<MdTasLoad> {
        let digest = raw_sha256.ok_or_else(|| anyhow::anyhow!("MD source digest missing"))?;
        ensure!(
            unmodified
                && settings.initial_input.unwrap_or_default() == (0, 0)
                && digest == md.rom_hash()
                && (8..=MAX_MD_TAS_ROM_BYTES as usize).contains(&len),
            "MD TAS requires unmodified neutral bounded factory media"
        );
        let extension = extension(rom).ok_or_else(|| anyhow::anyhow!("unsupported MD source"))?;
        let source = if from_path && source == rom {
            MdTasSource::Direct {
                sha256: digest,
                len: len as u64,
            }
        } else {
            ensure!(
                has_extension(source, "zip"),
                "MD source is not authenticated"
            );
            let selected = crate::rom_archive::extract_bounded_zip_member(
                source,
                Some(rom),
                extension,
                MAX_MD_TAS_ZIP_BYTES,
                MAX_MD_TAS_ROM_BYTES,
            )?;
            ensure!(
                selected.bytes.len() == len
                    && zeff_firmware::sha256_bytes(&selected.bytes) == digest,
                "MD selected archive member changed"
            );
            MdTasSource::Zip {
                archive_sha256: selected.archive_sha256,
                archive_len: selected.archive_len as u64,
                member: selected.member_name,
                member_sha256: digest,
                member_len: len as u64,
            }
        };
        let config = MdRollbackConfig::capture(md.core())?;
        let kinds = config.controller_kinds();
        ensure!(
            md.core().region() == Region::NtscUs
                && md.core().timing_model() == TimingModel::Standard
                && kinds[0] == kinds[1]
                && md.frame_count() == 0
                && !md.is_suspended()
                && md.firmware_manifests().is_empty()
                && md.core().pending_audio_frames() == 0
                && [ControllerPort::One, ControllerPort::Two]
                    .into_iter()
                    .all(|port| {
                        md.core().input().controller_raw(port) == 0xff
                            && md.core().input().controller_extra_raw(port) == 0x0f
                    }),
            "MD TAS requires fresh Standard US equal-pad factory runtime"
        );
        let profile = match kinds[0] {
            ControllerKind::ThreeButton => MdTasProfile::ThreeButton,
            ControllerKind::SixButton => MdTasProfile::SixButton,
        };
        Ok(MdTasLoad {
            source,
            effective_sha256: digest,
            effective_len: len as u64,
            profile,
            fresh_raw: Sha256::digest(md.encode_state_bytes()?).into(),
            fresh_hashes: config.boundary_hashes(md.core())?,
            config,
        })
    })();
    if let Ok(load) = load {
        md.install_tas_load(load);
    }
}
