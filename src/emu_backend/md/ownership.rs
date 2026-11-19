use super::MdBackend;
use std::sync::{Arc, Weak};
use zeff_md_core::Emulator;
use zeff_md_core::emulator::rollback::{MdRollbackConfig, MdRollbackSession};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MdOwnershipError {
    RollbackOwned,
    Transfer,
    Restoring,
    #[cfg(not(target_arch = "wasm32"))]
    TasOwned,
    #[cfg(not(target_arch = "wasm32"))]
    TasCommitting,
}

impl std::fmt::Display for MdOwnershipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mega Drive backend ownership: {self:?}")
    }
}
impl std::error::Error for MdOwnershipError {}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MdWarmResetError {
    Ownership(MdOwnershipError),
    Core(zeff_md_core::WarmResetError),
}

impl std::fmt::Display for MdWarmResetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mega Drive warm reset: {self:?}")
    }
}
impl std::error::Error for MdWarmResetError {}

#[derive(Debug)]
pub(super) enum MdExecutionError {
    Ownership(MdOwnershipError),
    Core(zeff_md_core::MachineFault),
}
impl From<MdOwnershipError> for MdExecutionError {
    fn from(error: MdOwnershipError) -> Self {
        Self::Ownership(error)
    }
}
impl From<zeff_md_core::MachineFault> for MdExecutionError {
    fn from(error: zeff_md_core::MachineFault) -> Self {
        Self::Core(error)
    }
}
impl std::fmt::Display for MdExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ownership(error) => error.fmt(f),
            Self::Core(error) => write!(f, "Mega Drive machine fault: {error:?}"),
        }
    }
}

pub(super) enum MdMachine {
    Normal(Box<Emulator>),
    Leased(MdOwnedRollback),
    Restoring {
        original: Box<Emulator>,
        owner: Arc<()>,
        host: MdHostBaseline,
    },
    Transfer(Box<Emulator>),
    #[cfg(not(target_arch = "wasm32"))]
    TasLeased(Box<super::tas::live::LiveState>),
    #[cfg(not(target_arch = "wasm32"))]
    TasCommitting(Box<super::tas::live::LiveCommit>),
    #[cfg(not(target_arch = "wasm32"))]
    TasRestoring(Box<super::tas::live::LiveRestore>),
}

pub(super) struct MdOwnedRollback {
    pub(super) session: MdRollbackSession,
    pub(super) owner: Arc<()>,
    pub(super) host: MdHostBaseline,
}

pub(super) struct MdHostBaseline {
    pub(super) original_address: usize,
    pub(super) state: Vec<u8>,
    pub(super) frontend: Vec<u8>,
    pub(super) publication: bool,
    pub(super) observation: [u8; 32],
    pub(super) generation: Option<bool>,
    pub(super) rewind_changed: bool,
    pub(super) sound: super::sound_trace::FrozenSound,
}

pub(crate) struct MdBackendRollbackLease {
    pub(super) owner: Weak<()>,
}
pub(crate) struct MdBackendRollbackSnapshot {
    pub(super) owner: Weak<()>,
    pub(super) core: zeff_md_core::emulator::rollback::MdRollbackSnapshot,
}

impl MdMachine {
    pub(super) fn core(&self) -> &Emulator {
        match self {
            Self::Normal(core) | Self::Transfer(core) => core,
            Self::Leased(owned) => owned.session.core(),
            Self::Restoring { original, .. } => original,
            #[cfg(not(target_arch = "wasm32"))]
            Self::TasLeased(state) => state.core(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::TasCommitting(state) => state.core(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::TasRestoring(state) => state.core(),
        }
    }

    pub(super) fn normal_mut(&mut self) -> Result<&mut Emulator, MdOwnershipError> {
        match self {
            Self::Normal(core) => Ok(core),
            Self::Leased(_) => Err(MdOwnershipError::RollbackOwned),
            Self::Restoring { .. } => Err(MdOwnershipError::Restoring),
            Self::Transfer(_) => Err(MdOwnershipError::Transfer),
            #[cfg(not(target_arch = "wasm32"))]
            Self::TasLeased(_) => Err(MdOwnershipError::TasOwned),
            #[cfg(not(target_arch = "wasm32"))]
            Self::TasCommitting(_) => Err(MdOwnershipError::TasCommitting),
            #[cfg(not(target_arch = "wasm32"))]
            Self::TasRestoring(_) => Err(MdOwnershipError::Restoring),
        }
    }
}

impl MdBackend {
    pub(crate) fn core(&self) -> &Emulator {
        self.machine.core()
    }

    pub(crate) fn rollback_owned(&self) -> bool {
        !matches!(self.machine, MdMachine::Normal(_))
    }

    #[cfg(test)]
    pub(crate) fn test_normal_core_mut(&mut self) -> Result<&mut Emulator, MdOwnershipError> {
        self.normal_core_mut()
    }

    pub(crate) fn take_rewind_audio_change(&self) -> bool {
        self.require_normal().is_ok() && self.rewind_audio_changed.replace(false)
    }

    pub(in crate::emu_backend) fn try_install_rom_patches(
        &mut self,
        cheats: &[crate::cheats::CheatPatch],
    ) -> anyhow::Result<()> {
        self.same_branch_core_mut()?.install_rom_patches(cheats)
    }

    pub(super) fn rollback_host_baseline(&self) -> anyhow::Result<MdHostBaseline> {
        self.require_normal()?;
        #[cfg(not(target_arch = "wasm32"))]
        anyhow::ensure!(
            self.tas_editor_owner
                .as_ref()
                .and_then(Weak::upgrade)
                .is_none(),
            "MD offline editor already owns this backend"
        );
        anyhow::ensure!(
            !self.suspended && !self.execution_faulted && self.runtime_fault.is_none(),
            "MD rollback requires a healthy running backend"
        );
        anyhow::ensure!(
            self.guest_call_undo.is_none()
                && self.debugger.rollback_idle()
                && self.sound_trace.idle(),
            "MD rollback excludes debugger or guest-call ownership"
        );
        Ok(MdHostBaseline {
            original_address: self.core() as *const Emulator as usize,
            state: self.core().encode_state()?,
            frontend: self.core().encode_frontend_state()?,
            publication: self.host_persistence_enabled,
            observation: self.rollback_host_observation()?,
            generation: self.audio_generation_enabled,
            rewind_changed: self.rewind_audio_changed.get(),
            sound: self.sound_trace.frozen()?,
        })
    }

    pub(super) fn verify_rollback_host(
        &self,
        original: &Emulator,
        host: &MdHostBaseline,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            original as *const Emulator as usize == host.original_address,
            "MD original allocation changed"
        );
        anyhow::ensure!(
            original.encode_state()? == host.state
                && original.encode_frontend_state()? == host.frontend,
            "MD original observation changed"
        );
        anyhow::ensure!(
            self.rollback_host_observation()? == host.observation
                && self.sound_trace.frozen()? == host.sound,
            "MD frozen host observation changed"
        );
        anyhow::ensure!(
            self.audio_generation_enabled == host.generation
                && self.rewind_audio_changed.get() == host.rewind_changed
                && !self.suspended
                && !self.execution_faulted
                && self.runtime_fault.is_none()
                && self.guest_call_undo.is_none()
                && self.debugger.rollback_idle()
                && !self.host_persistence_enabled,
            "MD backend baseline changed while owned"
        );
        Ok(())
    }

    pub(crate) fn begin_rollback(
        &mut self,
        expected: &MdRollbackConfig,
    ) -> anyhow::Result<MdBackendRollbackLease> {
        let host = self.rollback_host_baseline()?;
        let observation = Box::new(self.core().clone());
        let owner = Arc::new(());
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let MdMachine::Normal(original) = moved else {
            self.machine = moved;
            anyhow::bail!("MD backend ownership changed during acquisition");
        };
        // A caught panic leaves only a fenced observation, never a runnable placeholder.
        self.host_persistence_enabled = false;
        match MdRollbackSession::acquire(original, expected) {
            Ok(session) => {
                let lease = MdBackendRollbackLease {
                    owner: Arc::downgrade(&owner),
                };
                self.machine = MdMachine::Leased(MdOwnedRollback {
                    session,
                    owner,
                    host,
                });
                Ok(lease)
            }
            Err((original, error)) => {
                self.machine = MdMachine::Normal(original);
                self.host_persistence_enabled = host.publication;
                Err(error)
            }
        }
    }
}
