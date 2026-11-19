use super::*;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_md_core::emulator::rollback::{MdRollbackConfig, MdRollbackHashes};

pub(crate) struct MdNetplayAdmission {
    pub(crate) source: [u8; 32],
    pub(crate) len: u64,
    pub(crate) policy: [u8; 32],
    pub(crate) config: MdRollbackConfig,
    pub(crate) hashes: MdRollbackHashes,
    pub(crate) persistent: [u8; 32],
    pub(crate) initial_field: u64,
}
pub(crate) struct MdSessionRestore<'a> {
    pub(crate) config: &'a MdRollbackConfig,
    pub(crate) hashes: &'a MdRollbackHashes,
    pub(crate) raw: &'a [u8],
    pub(crate) persistent: &'a [u8; 32],
    pub(crate) initial_field: u64,
}
impl MdBackend {
    pub(in crate::emu_backend) fn install_netplay_load(
        &mut self,
        load: super::super::loader::netplay::md::MdNetplayLoad,
    ) {
        if !self.rollback_owned() {
            self.netplay_load = Some(load);
        }
    }
    pub(crate) fn netplay_admission(&self) -> Result<MdNetplayAdmission> {
        self.require_normal()?;
        #[cfg(not(target_arch = "wasm32"))]
        ensure!(
            self.tas_editor_owner
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .is_none(),
            "MD offline editor already owns this backend"
        );
        ensure!(
            !self.suspended
                && !self.execution_faulted
                && self.runtime_fault.is_none()
                && self.debugger.rollback_idle()
                && self.sound_trace.idle()
                && self.guest_call_undo.is_none(),
            "MD debugger or another execution owner is active"
        );
        self.netplay_load
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("MD requires loader-owned authenticated media"))?
            .validate(self)
    }
    pub(crate) fn validate_netplay_frame(&self, frame: u64) -> Result<()> {
        self.netplay_load
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("MD requires loader-owned authenticated media"))?
            .validate_frame(self, frame)
    }
    pub(crate) fn netplay_load_payload_bytes(&self) -> usize {
        std::mem::size_of_val(&self.netplay_load)
    }
    pub(crate) fn netplay_persistent_hash(&self) -> Result<[u8; 32]> {
        ensure!(
            self.core().save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None
                && self.core().dump_save_ram().is_none(),
            "MD netplay excludes save hardware"
        );
        Ok(Sha256::digest(b"MD-no-persistent-owner-v1").into())
    }
    pub(super) fn rollback_host_observation(&self) -> Result<[u8; 32]> {
        let publication = match &self.machine {
            MdMachine::Normal(_) => self.host_persistence_enabled,
            MdMachine::Leased(owned) => owned.host.publication,
            MdMachine::Restoring { host, .. } => host.publication,
            #[cfg(not(target_arch = "wasm32"))]
            MdMachine::TasLeased(_) | MdMachine::TasCommitting(_) | MdMachine::TasRestoring(_) => {
                self.tas_live_publication()?
            }
            MdMachine::Transfer(_) => {
                anyhow::bail!("MD ownership transfer observation unavailable")
            }
        };
        self.sound_trace.frozen()?;
        let mut hash = Sha256::new();
        hash.update(b"MD-backend-frozen-host-v1");
        hash.update(self.rom_hash);
        hash.update([
            u8::from(publication),
            u8::from(self.suspended),
            u8::from(self.execution_faulted),
            u8::from(self.rewind_audio_changed.get()),
            u8::from(self.debugger.rollback_idle()),
            u8::from(self.guest_call_undo.is_some()),
        ]);
        for value in self.debugger.frozen_observation()? {
            hash.update(value.to_le_bytes());
        }
        hash.update([match self.audio_generation_enabled {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        }]);
        hash.update([u8::from(self.runtime_fault.is_some())]);
        if let Some(fault) = &self.runtime_fault {
            hash.update((fault.len() as u64).to_le_bytes());
            hash.update(fault.as_bytes());
        }
        Ok(hash.finalize().into())
    }
    pub(super) fn verify_session_original(
        &self,
        original: &Emulator,
        expected: &MdSessionRestore<'_>,
    ) -> Result<()> {
        ensure!(
            original.pacing_field_count() == expected.initial_field
                && original.encode_state()?.as_slice() == expected.raw,
            "MD session RAW restoration differs"
        );
        ensure!(
            expected
                .config
                .boundary_hashes(original)?
                .eq(expected.hashes),
            "MD session logical/local restoration differs"
        );
        ensure!(
            original.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None
                && original.dump_save_ram().is_none()
                && <[u8; 32]>::from(Sha256::digest(b"MD-no-persistent-owner-v1"))
                    == *expected.persistent,
            "MD session persistence restoration differs"
        );
        Ok(())
    }
}
