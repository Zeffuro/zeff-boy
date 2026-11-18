#[cfg(test)]
mod tests;
use crate::emu_core_trait::EmulatorCore;
use sha2::{Digest, Sha256};
use zeff_emu_common::time::FrameLifecycle;
use zeff_md_core::emulator::rollback::{MdRollbackConfig, MdRollbackHashes};

pub(in crate::emu_backend) struct MdNetplayLoad {
    source: [u8; 32],
    len: u64,
    policy: [u8; 32],
    config: MdRollbackConfig,
    raw: [u8; 32],
    hashes: MdRollbackHashes,
    persistent: [u8; 32],
    initial_field: u64,
}
impl MdNetplayLoad {
    pub(in crate::emu_backend) fn validate_frame(
        &self,
        md: &crate::emu_backend::md::MdBackend,
        frame: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.initial_field.checked_add(frame) == Some(md.frame_count()),
            "MD netplay core field drift"
        );
        Ok(())
    }
    pub(in crate::emu_backend) fn validate(
        &self,
        md: &crate::emu_backend::md::MdBackend,
    ) -> anyhow::Result<crate::emu_backend::md::MdNetplayAdmission> {
        anyhow::ensure!(
            self.source == md.rom_hash() && self.len == md.core().rom_bytes().len() as u64,
            "MD loaded media changed"
        );
        let config = MdRollbackConfig::capture(md.core())?;
        anyhow::ensure!(
            config == self.config && md.frame_count() == self.initial_field && !md.is_suspended(),
            "MD netplay requires the fresh loaded configuration"
        );
        anyhow::ensure!(
            <[u8; 32]>::from(Sha256::digest(md.encode_state_bytes()?)) == self.raw,
            "MD loaded RAW state changed"
        );
        anyhow::ensure!(
            config.boundary_hashes(md.core())? == self.hashes,
            "MD loaded logical/local runtime changed"
        );
        anyhow::ensure!(
            md.netplay_persistent_hash()? == self.persistent,
            "MD persistence policy changed"
        );
        Ok(crate::emu_backend::md::MdNetplayAdmission {
            source: self.source,
            len: self.len,
            policy: self.policy,
            config,
            hashes: self.hashes.clone(),
            persistent: self.persistent,
            initial_field: self.initial_field,
        })
    }
}
pub(super) fn capture(
    md: &mut crate::emu_backend::md::MdBackend,
    source: [u8; 32],
    len: usize,
    authenticated: bool,
    unmodified: bool,
    neutral: bool,
) -> anyhow::Result<()> {
    if !authenticated
        || !unmodified
        || !neutral
        || !(8..=4 * 1024 * 1024).contains(&len)
        || source != md.rom_hash()
        || !md.firmware_manifests().is_empty()
    {
        return Ok(());
    }
    let Ok(config) = MdRollbackConfig::capture(md.core()) else {
        return Ok(());
    };
    let kinds = config.controller_kinds();
    if kinds[0] != kinds[1]
        || md.is_suspended()
        || ![
            zeff_md_core::ControllerPort::One,
            zeff_md_core::ControllerPort::Two,
        ]
        .into_iter()
        .all(|port| {
            md.core().input().controller_raw(port) == 0xff
                && md.core().input().controller_extra_raw(port) == 0x0f
        })
        || md.core().pending_audio_frames() != 0
    {
        return Ok(());
    }
    let Ok(hashes) = config.boundary_hashes(md.core()) else {
        return Ok(());
    };
    let persistent = md.netplay_persistent_hash()?;
    let mut policy = Sha256::new();
    policy.update(b"MD-authenticated-factory-no-save-firmware-mod-v1");
    policy.update((len as u64).to_le_bytes());
    md.install_netplay_load(MdNetplayLoad {
        source,
        len: len as u64,
        policy: policy.finalize().into(),
        config,
        raw: Sha256::digest(md.encode_state_bytes()?).into(),
        hashes,
        persistent,
        initial_field: md.frame_count(),
    });
    Ok(())
}
