use super::MdBackend;
use crate::emu_backend::capabilities::TasSourceMediaIdentity;
use crate::emu_backend::loader::md_tas::{MdTasLoad, MdTasSource};
use anyhow::{Result, ensure};

mod editor;
pub(crate) use editor::{MdTasEditorCheckpoint, MdTasEditorOwner};
pub(in crate::emu_backend::md) mod live;
pub(crate) use live::{MdTasLiveGuard, MdTasLiveLease, MdTasLiveProof, MdTasLiveSnapshot};

impl MdBackend {
    pub(crate) fn tas_source_media_identity(&self) -> Option<TasSourceMediaIdentity> {
        let source = self.tas_load.as_ref()?.source();
        let length = match source {
            MdTasSource::Direct { len, .. } => *len,
            MdTasSource::Zip { archive_len, .. } => *archive_len,
        };
        Some(TasSourceMediaIdentity::new(
            source.sha256(),
            usize::try_from(length).ok()?,
        ))
    }

    pub(in crate::emu_backend) fn install_tas_load(&mut self, load: MdTasLoad) {
        if self.require_normal().is_ok() && self.tas_load.is_none() {
            self.tas_load = Some(load);
        }
    }

    pub(crate) fn tas_load(&self) -> Result<&MdTasLoad> {
        self.require_normal()?;
        ensure!(
            !self.suspended
                && !self.execution_faulted
                && self.runtime_fault.is_none()
                && self.guest_call_undo.is_none()
                && self.debugger.rollback_idle()
                && self.sound_trace.idle(),
            "MD TAS excludes active debugger, fault or guest-call ownership"
        );
        self.tas_load
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("MD TAS requires loader-owned media provenance"))
    }
}
