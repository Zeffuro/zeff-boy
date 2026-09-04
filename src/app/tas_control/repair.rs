use crate::app::App;
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    EmuThread, SuspendedEmuThread, TasControlAcquireRejectedReason, TasExecutionProfile,
    TasPersistenceContract, TasPersistencePublicationOutcome, TasRepairIdentity,
    TasRepairReleaseFailure, TasRepairSuspendFailure, TasRepairSuspensionProof,
};
use crate::tas_project::TasDigest;
use crate::tas_project::{TasExternalIdentity, TasProject};

pub(in crate::app) struct TasPreparedRepair {
    identity: TasRepairIdentity,
    backend: EmuBackend,
}

pub(in crate::app) struct TasRepairTarget {
    pub(in crate::app) project_content_sha256: TasDigest,
    pub(in crate::app) profile: TasExecutionProfile,
    pub(in crate::app) source_media_sha256: TasDigest,
    pub(in crate::app) effective_media_sha256: TasDigest,
    pub(in crate::app) required_sample_rate: u32,
    pub(in crate::app) persistence: TasPersistenceContract,
}

struct TasRepairTransaction {
    identity: TasRepairIdentity,
    original_generation: u64,
    repaired_generation: u64,
    parked_original: SuspendedEmuThread,
}

pub(in crate::app) struct TasRepairManager {
    next_repair_id: u64,
    next_suspension_token: u64,
    transaction: Option<TasRepairTransaction>,
    pending_resolution: Option<TasRepairResolution>,
    connect_pending: bool,
    #[cfg(test)]
    repaired_recovery: Option<crate::emu_thread::RecoveryTestConfig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasRepairResolution {
    Restore,
    Keep,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasRepairPrepareFailure {
    AlreadyActive,
    IdentityExhausted,
    BackendRejected(TasControlAcquireRejectedReason),
    ProfileMismatch,
    SourceMediaMismatch,
    EffectiveMediaMismatch,
    SampleRateMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasRepairBeginFailureReason {
    AlreadyActive,
    InvalidWorkerGeneration,
    OriginalSuspend(TasRepairSuspendFailure),
    RepairedSpawnFailed,
    OriginalResume(TasRepairReleaseFailure),
}

pub(in crate::app) struct TasRepairBeginFailure {
    pub(in crate::app) reason: TasRepairBeginFailureReason,
    pub(in crate::app) original_worker: Option<Box<EmuThread>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasRepairResolveFailure {
    NoActiveTransaction,
    WrongRepairedGeneration,
    InvalidResumeGeneration,
    RepairedRelease(TasRepairReleaseFailure),
    PublicationNotPublished(String),
    OriginalRelease(TasRepairReleaseFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum TasRepairState {
    Detached,
    RepairedDetached {
        identity: Box<TasRepairIdentity>,
        original_generation: u64,
        repaired_generation: u64,
        original_proof: Box<TasRepairSuspensionProof>,
    },
}

pub(in crate::app) struct TasRepairRestoreResult {
    pub(in crate::app) worker: EmuThread,
    pub(in crate::app) worker_generation: u64,
    pub(in crate::app) original_proof: TasRepairSuspensionProof,
    pub(in crate::app) repaired_release_warning: Option<TasRepairReleaseFailure>,
}

pub(in crate::app) fn persistence_contract_for_project(
    project: &TasProject,
    backend: &EmuBackend,
    profile: TasExecutionProfile,
) -> anyhow::Result<TasPersistenceContract> {
    if project.identity().rtc_state != TasExternalIdentity::Absent {
        return match profile {
            TasExecutionProfile::DirectGbCartridgeDmg
            | TasExecutionProfile::DirectGbCartridgeCgb => {
                let witness = crate::emu_backend::loader::gb_rtc_persistence_witness(backend)?;
                anyhow::ensure!(
                    witness.persistent_state == project.identity().persistent_state
                        && witness.rtc_state == project.identity().rtc_state,
                    "repaired backend RAM or RTC state does not match the TAS project"
                );
                Ok(TasPersistenceContract::GbRtcBattery {
                    persistent_state: witness.persistent_state,
                    rtc_state: witness.rtc_state,
                    byte_len: witness.complete_byte_len,
                    initial_sha256: witness.complete_sha256,
                    target_baseline: backend.gb_tas_battery_baseline()?.into(),
                })
            }
            TasExecutionProfile::DirectGbaCartridge => {
                let witness = crate::emu_backend::gba::gba_rtc_persistence_witness(backend)?;
                anyhow::ensure!(
                    witness.persistent_state == project.identity().persistent_state
                        && witness.rtc_state == project.identity().rtc_state,
                    "repaired backend backup or RTC state does not match the TAS project"
                );
                Ok(TasPersistenceContract::GbaRtcBattery {
                    kind: witness.backup_kind,
                    persistent_state: witness.persistent_state,
                    rtc_state: witness.rtc_state,
                    byte_len: witness.complete_byte_len,
                    initial_sha256: witness.complete_sha256,
                    target_baseline: backend.gba_tas_battery_baseline()?.into(),
                })
            }
            TasExecutionProfile::DirectWsCartridge => {
                let witness = crate::emu_backend::ws::ws_rtc_persistence_witness(backend)?;
                anyhow::ensure!(
                    witness.persistent_state == project.identity().persistent_state
                        && witness.rtc_state == project.identity().rtc_state,
                    "repaired backend backup or RTC state does not match the TAS project"
                );
                Ok(TasPersistenceContract::WsRtcBattery {
                    save_kind: witness.save_kind,
                    persistent_state: witness.persistent_state,
                    rtc_state: witness.rtc_state,
                    byte_len: witness.complete_byte_len,
                    initial_sha256: witness.complete_sha256,
                    target_baseline: backend.ws_tas_battery_baseline()?.into(),
                })
            }
            _ => anyhow::bail!("RTC persistence is incompatible with this TAS execution profile"),
        };
    }
    let TasExternalIdentity::ExternalSha256(initial_sha256) = project.identity().persistent_state
    else {
        return Ok(TasPersistenceContract::Absent);
    };
    let bytes = match profile {
        TasExecutionProfile::DirectNesCartridge => backend.nes_tas_battery_bytes(),
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            backend.gb_tas_battery_bytes()
        }
        TasExecutionProfile::DirectGbaCartridge => {
            backend.gba_tas_battery_component()?.map(|(_, bytes)| bytes)
        }
        TasExecutionProfile::DirectGameGearCartridge => backend.game_gear_tas_battery_bytes(),
        TasExecutionProfile::DirectWsCartridge => backend.ws_tas_battery_bytes(),
        _ => {
            anyhow::bail!("linked project-owned battery state is not enabled for this TAS profile")
        }
    }
    .ok_or_else(|| anyhow::anyhow!("TAS project declares unavailable battery state"))?;
    anyhow::ensure!(
        TasDigest::from_bytes(&bytes) == initial_sha256,
        "repaired backend battery state does not match the TAS project"
    );
    let baseline = match profile {
        TasExecutionProfile::DirectNesCartridge => backend.nes_tas_battery_baseline()?,
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            backend.gb_tas_battery_baseline()?
        }
        TasExecutionProfile::DirectGbaCartridge => backend.gba_tas_battery_baseline()?,
        TasExecutionProfile::DirectGameGearCartridge => backend.game_gear_tas_battery_baseline()?,
        TasExecutionProfile::DirectWsCartridge => backend.ws_tas_battery_baseline()?,
        _ => unreachable!(),
    };
    let target_baseline = baseline.into();
    let contract = match profile {
        TasExecutionProfile::DirectNesCartridge => TasPersistenceContract::NesBattery {
            byte_len: bytes.len() as u64,
            initial_sha256,
            target_baseline,
        },
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            TasPersistenceContract::GbBattery {
                byte_len: bytes.len() as u64,
                initial_sha256,
                target_baseline,
            }
        }
        TasExecutionProfile::DirectGbaCartridge => {
            let (kind, component) = backend
                .gba_tas_battery_component()?
                .ok_or_else(|| anyhow::anyhow!("TAS project declares unavailable GBA backup"))?;
            anyhow::ensure!(
                component == bytes,
                "GBA backup changed during repair preparation"
            );
            TasPersistenceContract::GbaBattery {
                kind,
                byte_len: bytes.len() as u64,
                initial_sha256,
                target_baseline,
            }
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            anyhow::ensure!(
                bytes.len() == 8 * 1024
                    && backend.save_ram_kind()
                        == zeff_emu_common::save_ram::SaveRamKind::known_battery_backed(8 * 1024),
                "Game Gear backup topology is not supported for linked TAS"
            );
            TasPersistenceContract::GameGearBattery8KiB {
                byte_len: bytes.len() as u64,
                initial_sha256,
                target_baseline,
            }
        }
        TasExecutionProfile::DirectWsCartridge => {
            anyhow::ensure!(
                !backend.ws().is_some_and(|ws| ws.emu.footer().rtc_present),
                "linked WonderSwan TAS does not own external RTC persistence"
            );
            let save_kind = backend
                .ws_tas_battery_save_kind()
                .ok_or_else(|| anyhow::anyhow!("TAS project declares unavailable WS backup"))?;
            anyhow::ensure!(
                matches!(
                    save_kind,
                    zeff_ws_core::hardware::cartridge::SaveKind::Sram32KId1
                        | zeff_ws_core::hardware::cartridge::SaveKind::Sram32K
                        | zeff_ws_core::hardware::cartridge::SaveKind::Sram128K
                        | zeff_ws_core::hardware::cartridge::SaveKind::Sram256K
                        | zeff_ws_core::hardware::cartridge::SaveKind::Sram512K
                        | zeff_ws_core::hardware::cartridge::SaveKind::Eeprom128
                        | zeff_ws_core::hardware::cartridge::SaveKind::Eeprom1K
                        | zeff_ws_core::hardware::cartridge::SaveKind::Eeprom2K
                ) && save_kind.size() == bytes.len(),
                "WonderSwan backup topology is not supported for linked TAS"
            );
            TasPersistenceContract::WsBattery {
                save_kind,
                byte_len: bytes.len() as u64,
                initial_sha256,
                target_baseline,
            }
        }
        _ => unreachable!(),
    };
    Ok(contract)
}

impl TasRepairManager {
    pub(in crate::app) fn new() -> Self {
        Self {
            next_repair_id: 1,
            next_suspension_token: 1,
            transaction: None,
            pending_resolution: None,
            connect_pending: false,
            #[cfg(test)]
            repaired_recovery: None,
        }
    }

    pub(in crate::app) fn prepare(
        &mut self,
        target: TasRepairTarget,
        backend: EmuBackend,
    ) -> Result<TasPreparedRepair, TasRepairPrepareFailure> {
        if self.transaction.is_some() {
            return Err(TasRepairPrepareFailure::AlreadyActive);
        }
        let repair_id = self.next_repair_id;
        let suspension_token = self.next_suspension_token;
        self.next_repair_id = repair_id
            .checked_add(1)
            .ok_or(TasRepairPrepareFailure::IdentityExhausted)?;
        self.next_suspension_token = suspension_token
            .checked_add(1)
            .ok_or(TasRepairPrepareFailure::IdentityExhausted)?;
        let identity = TasRepairIdentity {
            repair_id,
            suspension_token,
            project_content_sha256: target.project_content_sha256,
            profile: target.profile,
            source_media_sha256: target.source_media_sha256,
            effective_media_sha256: target.effective_media_sha256,
            required_sample_rate: target.required_sample_rate,
            persistence: target.persistence,
        };
        validate_prepared_backend(&backend, identity)?;
        Ok(TasPreparedRepair { identity, backend })
    }

    pub(in crate::app) fn begin(
        &mut self,
        prepared: TasPreparedRepair,
        original_generation: u64,
        repaired_generation: u64,
        original_worker: EmuThread,
    ) -> Result<EmuThread, TasRepairBeginFailure> {
        #[cfg(test)]
        {
            let recovery = self.repaired_recovery.take();
            self.begin_with_spawn(
                prepared,
                original_generation,
                repaired_generation,
                original_worker,
                move |backend, identity| match recovery {
                    Some(recovery) => {
                        EmuThread::try_spawn_repaired_with_recovery(backend, identity, recovery)
                    }
                    None => EmuThread::try_spawn_repaired(backend, identity),
                },
            )
        }
        #[cfg(not(test))]
        self.begin_with_spawn(
            prepared,
            original_generation,
            repaired_generation,
            original_worker,
            EmuThread::try_spawn_repaired,
        )
    }

    #[cfg(test)]
    pub(in crate::app) fn set_repaired_recovery_for_test(
        &mut self,
        config: crate::emu_thread::RecoveryTestConfig,
    ) {
        self.repaired_recovery = Some(config);
    }

    fn begin_with_spawn<F>(
        &mut self,
        prepared: TasPreparedRepair,
        original_generation: u64,
        repaired_generation: u64,
        original_worker: EmuThread,
        spawn: F,
    ) -> Result<EmuThread, TasRepairBeginFailure>
    where
        F: FnOnce(EmuBackend, TasRepairIdentity) -> std::io::Result<EmuThread>,
    {
        if self.transaction.is_some() {
            return Err(TasRepairBeginFailure {
                reason: TasRepairBeginFailureReason::AlreadyActive,
                original_worker: Some(Box::new(original_worker)),
            });
        }
        if original_generation == 0 || repaired_generation <= original_generation {
            return Err(TasRepairBeginFailure {
                reason: TasRepairBeginFailureReason::InvalidWorkerGeneration,
                original_worker: Some(Box::new(original_worker)),
            });
        }
        let identity = prepared.identity;
        let parked_original = match original_worker.suspend_for_tas_repair(identity) {
            Ok(parked) => parked,
            Err(error) => {
                return Err(TasRepairBeginFailure {
                    reason: TasRepairBeginFailureReason::OriginalSuspend(error.reason),
                    original_worker: error.original_worker,
                });
            }
        };
        let repaired_worker = match spawn(prepared.backend, identity) {
            Ok(worker) => worker,
            Err(_) => {
                return match parked_original.resume() {
                    Ok(worker) => Err(TasRepairBeginFailure {
                        reason: TasRepairBeginFailureReason::RepairedSpawnFailed,
                        original_worker: Some(Box::new(worker)),
                    }),
                    Err(reason) => Err(TasRepairBeginFailure {
                        reason: TasRepairBeginFailureReason::OriginalResume(reason),
                        original_worker: None,
                    }),
                };
            }
        };
        self.transaction = Some(TasRepairTransaction {
            identity,
            original_generation,
            repaired_generation,
            parked_original,
        });
        self.pending_resolution = None;
        self.connect_pending = true;
        Ok(repaired_worker)
    }

    pub(in crate::app) fn restore(
        &mut self,
        repaired_worker: EmuThread,
        repaired_generation: u64,
        resumed_generation: u64,
    ) -> Result<TasRepairRestoreResult, (TasRepairResolveFailure, Option<Box<EmuThread>>)> {
        let Some(transaction) = self.transaction.take() else {
            return Err((
                TasRepairResolveFailure::NoActiveTransaction,
                Some(Box::new(repaired_worker)),
            ));
        };
        self.pending_resolution = None;
        self.connect_pending = false;
        if repaired_generation != transaction.repaired_generation {
            self.transaction = Some(transaction);
            return Err((
                TasRepairResolveFailure::WrongRepairedGeneration,
                Some(Box::new(repaired_worker)),
            ));
        }
        if resumed_generation <= repaired_generation
            || resumed_generation == transaction.original_generation
        {
            self.transaction = Some(transaction);
            return Err((
                TasRepairResolveFailure::InvalidResumeGeneration,
                Some(Box::new(repaired_worker)),
            ));
        }
        let original_proof = transaction.parked_original.proof().clone();
        let repaired_release_warning = repaired_worker
            .discard_repaired_for_tas_restore(transaction.identity)
            .err();
        match transaction.parked_original.resume() {
            Ok(worker) => Ok(TasRepairRestoreResult {
                worker,
                worker_generation: resumed_generation,
                original_proof,
                repaired_release_warning,
            }),
            Err(reason) => Err((TasRepairResolveFailure::OriginalRelease(reason), None)),
        }
    }

    pub(in crate::app) fn keep(
        &mut self,
        repaired_generation: u64,
        repaired_worker: &EmuThread,
        save_recovery_on_shutdown: bool,
    ) -> Result<TasPersistencePublicationOutcome, TasRepairResolveFailure> {
        let Some(transaction) = self.transaction.as_ref() else {
            return Err(TasRepairResolveFailure::NoActiveTransaction);
        };
        if repaired_generation != transaction.repaired_generation {
            return Err(TasRepairResolveFailure::WrongRepairedGeneration);
        }
        let publication = repaired_worker
            .commit_repaired_tas_worker(transaction.identity, save_recovery_on_shutdown)
            .map_err(TasRepairResolveFailure::RepairedRelease)?;
        if let TasPersistencePublicationOutcome::NotPublished { error } = &publication {
            return Err(TasRepairResolveFailure::PublicationNotPublished(
                error.clone(),
            ));
        }
        let transaction = self
            .transaction
            .take()
            .expect("validated TAS repair transaction should remain active");
        self.pending_resolution = None;
        self.connect_pending = false;
        transaction
            .parked_original
            .discard()
            .map_err(TasRepairResolveFailure::OriginalRelease)?;
        Ok(publication)
    }

    pub(in crate::app) fn state(&self) -> TasRepairState {
        match self.transaction.as_ref() {
            None => TasRepairState::Detached,
            Some(transaction) => TasRepairState::RepairedDetached {
                identity: Box::new(transaction.identity),
                original_generation: transaction.original_generation,
                repaired_generation: transaction.repaired_generation,
                original_proof: Box::new(transaction.parked_original.proof().clone()),
            },
        }
    }

    pub(in crate::app) fn request_resolution(&mut self, resolution: TasRepairResolution) -> bool {
        if self.transaction.is_none() {
            return false;
        }
        self.connect_pending = false;
        self.pending_resolution = Some(resolution);
        true
    }

    pub(in crate::app) fn take_pending_resolution(&mut self) -> Option<TasRepairResolution> {
        self.pending_resolution.take()
    }

    pub(in crate::app) fn connect_pending(&self) -> bool {
        self.transaction.is_some() && self.connect_pending
    }

    pub(in crate::app) fn has_active_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    pub(in crate::app) fn complete_pending_connect(&mut self) {
        self.connect_pending = false;
    }
}

impl App {
    pub(in crate::app) fn queue_prepared_tas_repair(&mut self, prepared: TasPreparedRepair) {
        self.pending_tas_repair_activation = Some(prepared);
        self.recompute_pause();
    }

    pub(in crate::app) fn pump_pending_tas_repair_activation(&mut self) {
        if self.frames_in_flight != 0 {
            return;
        }
        let Some(prepared) = self.pending_tas_repair_activation.take() else {
            return;
        };
        if let Err(error) = self.activate_prepared_tas_repair(prepared) {
            log::error!("Could not activate TAS repair: {error:#}");
            self.toast_manager
                .error(format!("Could not reload and connect TAS: {error:#}"));
        } else {
            self.refresh_tas_control_readiness();
        }
        self.recompute_pause();
    }

    pub(in crate::app) fn prepare_tas_repair(
        &mut self,
        target: TasRepairTarget,
        backend: EmuBackend,
    ) -> anyhow::Result<TasPreparedRepair> {
        self.tas_repair
            .prepare(target, backend)
            .map_err(|reason| anyhow::anyhow!("TAS repair preparation failed: {reason:?}"))
    }

    pub(in crate::app) fn activate_prepared_tas_repair(
        &mut self,
        prepared: TasPreparedRepair,
    ) -> anyhow::Result<()> {
        if !self.tas_control.gameplay_commands_allowed()
            || self.frames_in_flight != 0
            || self.recording.replay_timeline_active()
            || self.rewind.pending
            || self.rewind.backstep_pending
            || self.recording.audio_recorder.is_some()
            || self.timing.uncapped_speed
        {
            anyhow::bail!("current emulator activity is incompatible with TAS repair");
        }
        let original_generation = self.emu_worker_generation;
        let repaired_generation = original_generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("emulator worker generations are exhausted"))?;
        let original_worker = self
            .emu_thread
            .take()
            .ok_or_else(|| anyhow::anyhow!("no emulator worker is running"))?;
        match self.tas_repair.begin(
            prepared,
            original_generation,
            repaired_generation,
            original_worker,
        ) {
            Ok(repaired_worker) => {
                self.emu_worker_generation = repaired_generation;
                self.latest_frame = repaired_worker.shared_framebuffer().load_full();
                self.emu_thread = Some(repaired_worker);
                self.tas_control.clear_readiness();
                self.recompute_pause();
                Ok(())
            }
            Err(failure) => {
                self.emu_thread = failure.original_worker.map(|worker| *worker);
                Err(anyhow::anyhow!(
                    "TAS repair activation failed: {:?}",
                    failure.reason
                ))
            }
        }
    }

    pub(in crate::app) fn restore_tas_repair(
        &mut self,
    ) -> anyhow::Result<Option<TasRepairReleaseFailure>> {
        if !self.tas_control.gameplay_commands_allowed() {
            anyhow::bail!("finish returning the repaired TAS lease before restoring the old game");
        }
        let repaired_generation = self.emu_worker_generation;
        let resumed_generation = repaired_generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("emulator worker generations are exhausted"))?;
        let repaired_worker = self
            .emu_thread
            .take()
            .ok_or_else(|| anyhow::anyhow!("no repaired emulator worker is running"))?;
        match self
            .tas_repair
            .restore(repaired_worker, repaired_generation, resumed_generation)
        {
            Ok(restored) => {
                let latest_frame = restored.worker.shared_framebuffer().load_full();
                let framebuffer_matches = latest_frame.as_ref().is_some_and(|frame| {
                    frame.len() == restored.original_proof.framebuffer_len
                        && TasDigest::from_bytes(frame.as_slice())
                            == restored.original_proof.framebuffer_sha256
                });
                self.emu_worker_generation = restored.worker_generation;
                self.latest_frame = latest_frame;
                self.emu_thread = Some(restored.worker);
                self.tas_control.clear_readiness();
                self.recompute_pause();
                if !framebuffer_matches {
                    anyhow::bail!("restored TAS repair framebuffer proof mismatch");
                }
                Ok(restored.repaired_release_warning)
            }
            Err((reason, worker)) => {
                self.emu_thread = worker.map(|worker| *worker);
                Err(anyhow::anyhow!("TAS repair restore failed: {reason:?}"))
            }
        }
    }

    pub(in crate::app) fn keep_tas_repair(&mut self) -> anyhow::Result<Option<String>> {
        let worker = self
            .emu_thread
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no repaired emulator worker is running"))?;
        let publication = self
            .tas_repair
            .keep(
                self.emu_worker_generation,
                worker,
                self.settings.emulation.save_recovery_state,
            )
            .map_err(|reason| anyhow::anyhow!("TAS repair keep failed: {reason:?}"))?;
        self.tas_control.clear_readiness();
        Ok(match publication {
            TasPersistencePublicationOutcome::PublishedDurabilityUncertain { error, .. } => {
                Some(error)
            }
            TasPersistencePublicationOutcome::NotRequired
            | TasPersistencePublicationOutcome::PublishedDurable { .. } => None,
            TasPersistencePublicationOutcome::NotPublished { .. } => {
                unreachable!("non-publication retains the repair transaction")
            }
        })
    }

    pub(in crate::app) fn tas_repair_state(&self) -> TasRepairState {
        self.tas_repair.state()
    }

    pub(in crate::app) fn pump_tas_repair_connect_after_readiness(&mut self) {
        if !self.tas_repair.connect_pending() {
            return;
        }
        let Some(status) = self
            .tas_control_readiness_report()
            .map(|report| report.status)
        else {
            return;
        };
        self.tas_repair.complete_pending_connect();
        match status {
            crate::app::tas_control::readiness::TasReadinessStatus::Ready => {
                if let Err(error) = self.begin_tas_control_acquire() {
                    log::error!("Could not connect repaired TAS worker: {error:#}");
                    self.toast_manager
                        .error(format!("Could not connect repaired TAS worker: {error:#}"));
                    self.request_tas_repair_resolution(TasRepairResolution::Restore);
                    self.pump_tas_repair_resolution();
                }
            }
            crate::app::tas_control::readiness::TasReadinessStatus::ReloadRequired
            | crate::app::tas_control::readiness::TasReadinessStatus::Incompatible => {
                self.request_tas_repair_resolution(TasRepairResolution::Restore);
                self.pump_tas_repair_resolution();
            }
        }
    }

    pub(in crate::app) fn request_tas_repair_resolution(
        &mut self,
        resolution: TasRepairResolution,
    ) -> bool {
        self.tas_repair.request_resolution(resolution)
    }

    pub(in crate::app) fn pump_tas_repair_resolution(&mut self) {
        if !self.tas_control.gameplay_commands_allowed() {
            return;
        }
        let Some(resolution) = self.tas_repair.take_pending_resolution() else {
            return;
        };
        let result = match resolution {
            TasRepairResolution::Restore => self
                .restore_tas_repair()
                .map(|warning| warning.map(|warning| format!("{warning:?}"))),
            TasRepairResolution::Keep => self.keep_tas_repair(),
        };
        match result {
            Ok(Some(warning)) => {
                log::error!(
                    "TAS repair completed with a repaired-worker release warning: {warning}"
                );
                self.toast_manager
                    .error("The previous game was restored after a repair cleanup warning");
            }
            Ok(None) => {}
            Err(error) => {
                log::error!("Could not resolve TAS repair: {error:#}");
                self.toast_manager
                    .error(format!("Could not resolve TAS repair: {error:#}"));
            }
        }
    }
}

fn validate_prepared_backend(
    backend: &EmuBackend,
    identity: TasRepairIdentity,
) -> Result<(), TasRepairPrepareFailure> {
    let witness = crate::emu_thread::build_tas_repair_witness_for_persistence(
        backend,
        identity.profile,
        identity.persistence,
    )
    .map_err(TasRepairPrepareFailure::BackendRejected)?;
    let observation = crate::emu_thread::observe_tas_repair_profile(backend, identity.profile);
    if witness.profile != identity.profile || observation.profile != identity.profile {
        return Err(TasRepairPrepareFailure::ProfileMismatch);
    }
    if witness.source_media_sha256 != identity.source_media_sha256
        || observation.source_media_sha256 != Some(identity.source_media_sha256)
    {
        return Err(TasRepairPrepareFailure::SourceMediaMismatch);
    }
    if witness.effective_media_sha256 != identity.effective_media_sha256
        || observation.effective_media_sha256 != Some(identity.effective_media_sha256)
    {
        return Err(TasRepairPrepareFailure::EffectiveMediaMismatch);
    }
    let configured_matches = match identity.profile {
        TasExecutionProfile::DirectNesCartridge => observation
            .configured_at_load_sample_rate
            .is_none_or(|rate| rate == identity.required_sample_rate),
        TasExecutionProfile::DirectFdsDisk => {
            observation.configured_at_load_sample_rate == Some(identity.required_sample_rate)
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectColecoCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectSmsCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectGbaCartridge => {
            observation.configured_at_load_sample_rate == Some(identity.required_sample_rate)
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectWsCartridge => {
            observation.configured_at_load_sample_rate.is_none()
        }
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceMultitapHuCard
        | TasExecutionProfile::DirectPceCd
        | TasExecutionProfile::DirectPceMultitapCd => {
            observation.configured_at_load_sample_rate == Some(identity.required_sample_rate)
        }
    };
    if !configured_matches
        || observation.initial_sample_rate != Some(identity.required_sample_rate)
        || observation.current_sample_rate != Some(identity.required_sample_rate)
    {
        return Err(TasRepairPrepareFailure::SampleRateMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod gba_rtc_tests;
#[cfg(test)]
mod rtc_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ws_rtc_tests;
