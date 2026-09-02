use std::ops::ControlFlow;

use crate::emu_thread::{
    EmuCommand, EmuResponse, TasControlAcquireRejectedReason, TasPersistenceContract,
    TasPersistencePublicationOutcome, TasRepairAction, TasRepairActionRejectedReason,
    TasRepairIdentity, TasRepairSuspendRejectedReason, TasRepairSuspensionProof,
};
use crate::tas_project::TasDigest;

use super::EmuLoop;
use super::tas_control::TasControlContext;

#[derive(Default)]
pub(super) struct TasRepairWorkerState {
    pub(super) identity: Option<TasRepairIdentity>,
    pub(super) nonpersistent_exit_requested: bool,
}

impl EmuLoop {
    pub(in crate::emu_thread) fn set_tas_repair_identity(&mut self, identity: TasRepairIdentity) {
        self.tas_repair.identity = Some(identity);
    }

    pub(super) fn dispatch_tas_authority(
        &mut self,
        command: EmuCommand,
    ) -> ControlFlow<bool, EmuCommand> {
        match self.dispatch_tas_repair(command) {
            ControlFlow::Continue(command) => self.dispatch_tas_control(command),
            ControlFlow::Break(keep_running) => ControlFlow::Break(keep_running),
        }
    }

    pub(super) fn dispatch_tas_repair(
        &mut self,
        command: EmuCommand,
    ) -> ControlFlow<bool, EmuCommand> {
        match command {
            EmuCommand::SuspendTasRepair { identity } => {
                ControlFlow::Break(self.suspend_for_tas_repair(identity))
            }
            EmuCommand::DiscardRepairedTasWorker { identity } => {
                ControlFlow::Break(self.discard_repaired_tas_worker(identity))
            }
            EmuCommand::CommitRepairedTasWorker {
                identity,
                save_recovery_on_shutdown,
            } => ControlFlow::Break(
                self.commit_repaired_tas_worker(identity, save_recovery_on_shutdown),
            ),
            EmuCommand::ResumeTasRepair { identity, .. } => ControlFlow::Break(
                self.reject_inactive_repair(identity, TasRepairAction::ResumeOriginal),
            ),
            EmuCommand::DiscardTasRepair { identity } => ControlFlow::Break(
                self.reject_inactive_repair(identity, TasRepairAction::DiscardOriginal),
            ),
            command => ControlFlow::Continue(command),
        }
    }

    fn suspend_for_tas_repair(&mut self, identity: TasRepairIdentity) -> bool {
        let proof = match self.capture_tas_repair_proof(identity) {
            Ok(proof) => proof,
            Err(reason) => {
                return self.send_resp(EmuResponse::TasRepairSuspendRejected { identity, reason });
            }
        };
        if !self.send_resp(EmuResponse::TasRepairSuspended {
            proof: Box::new(proof.clone()),
        }) {
            self.tas_repair.nonpersistent_exit_requested = true;
            return false;
        }

        loop {
            let command = match self.cmd_rx.recv() {
                Ok(command) => command,
                Err(_) => {
                    self.tas_repair.nonpersistent_exit_requested = true;
                    return false;
                }
            };
            match command {
                EmuCommand::ResumeTasRepair {
                    identity: requested,
                    expected_proof,
                } => {
                    if requested != identity {
                        if !self.reject_parked_action(
                            requested,
                            TasRepairAction::ResumeOriginal,
                            stale_reason(identity, requested),
                        ) {
                            self.tas_repair.nonpersistent_exit_requested = true;
                            return false;
                        }
                        continue;
                    }
                    if *expected_proof != proof {
                        let _ = self.reject_parked_action(
                            requested,
                            TasRepairAction::ResumeOriginal,
                            TasRepairActionRejectedReason::SuspensionProofMismatch,
                        );
                        self.tas_repair.nonpersistent_exit_requested = true;
                        return false;
                    }
                    let resumed_proof = match self.capture_tas_repair_proof(identity) {
                        Ok(resumed_proof) => resumed_proof,
                        Err(reason) => {
                            let _ = self.reject_parked_action(
                                requested,
                                TasRepairAction::ResumeOriginal,
                                resume_capture_reason(reason),
                            );
                            self.tas_repair.nonpersistent_exit_requested = true;
                            return false;
                        }
                    };
                    if resumed_proof != proof {
                        let _ = self.reject_parked_action(
                            requested,
                            TasRepairAction::ResumeOriginal,
                            proof_mismatch_reason(&proof, &resumed_proof),
                        );
                        self.tas_repair.nonpersistent_exit_requested = true;
                        return false;
                    }
                    return self.send_resp(EmuResponse::TasRepairOriginalResumed {
                        proof: Box::new(resumed_proof),
                    });
                }
                EmuCommand::DiscardTasRepair {
                    identity: requested,
                } => {
                    if requested != identity {
                        if !self.reject_parked_action(
                            requested,
                            TasRepairAction::DiscardOriginal,
                            stale_reason(identity, requested),
                        ) {
                            self.tas_repair.nonpersistent_exit_requested = true;
                            return false;
                        }
                        continue;
                    }
                    let _ = self.send_resp(EmuResponse::TasRepairOriginalDiscarded { identity });
                    self.tas_repair.nonpersistent_exit_requested = true;
                    return false;
                }
                _ => {
                    if !self.reject_parked_action(
                        identity,
                        TasRepairAction::ResumeOriginal,
                        TasRepairActionRejectedReason::NoMatchingRepair,
                    ) {
                        self.tas_repair.nonpersistent_exit_requested = true;
                        return false;
                    }
                }
            }
        }
    }

    fn capture_tas_repair_proof(
        &self,
        identity: TasRepairIdentity,
    ) -> Result<TasRepairSuspensionProof, TasRepairSuspendRejectedReason> {
        if identity.repair_id == 0
            || identity.suspension_token == 0
            || identity.required_sample_rate == 0
            || self.tas_repair.identity.is_some()
        {
            return Err(TasRepairSuspendRejectedReason::InvalidIdentity);
        }
        let context = TasControlContext {
            uncapped_execution: self.uncapped_mode,
            audio_recording_active: self.audio_recording_capture.active,
            link_activity: self.pending_tcp_link.is_some()
                || self.tcp_link.is_some()
                || self.game_boy_replay_link.is_some()
                || self.wonder_swan_replay_link.is_some(),
            pending_frame_delivery: !self.drain_rx.is_empty(),
            runtime_fault: !self.runtime_fault.can_step(),
        };
        if let Some(reason) = self.tas_control.acquire_blocker(&context) {
            return Err(suspend_blocker(reason));
        }
        let loaded_profile = super::tas_control::witness::observe_loaded_profile(
            &self.backend,
            !self.last_cheats.is_empty(),
            identity.profile,
        );
        validate_suspend_profile(identity, &loaded_profile, &self.backend)?;
        let frame_count = self.backend.frame_count();
        let state_bytes = self
            .backend
            .encode_state_bytes()
            .map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
        if self.backend.frame_count() != frame_count {
            return Err(TasRepairSuspendRejectedReason::StateChangedDuringCapture);
        }
        match identity.profile {
            crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg => {
                crate::emu_backend::loader::validate_direct_gb_tas_state(&state_bytes)
                    .map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
            }
            crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb => {
                let validation = if matches!(
                    identity.persistence,
                    crate::emu_thread::TasPersistenceContract::GbRtcBattery { .. }
                ) {
                    crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_rtc(
                        &self.backend,
                        &state_bytes,
                        false,
                    )
                } else if matches!(
                    identity.persistence,
                    crate::emu_thread::TasPersistenceContract::GbBattery { .. }
                ) {
                    crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_sram(
                        &self.backend,
                        &state_bytes,
                        false,
                    )
                } else {
                    crate::emu_backend::loader::validate_direct_gbc_state_for_backend(
                        &self.backend,
                        &state_bytes,
                        false,
                    )
                };
                validation.map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
            }
            crate::emu_thread::TasExecutionProfile::DirectWsCartridge => {
                crate::emu_backend::loader::validate_direct_ws_tas_private_execution_runtime(
                    &self.backend,
                    false,
                )
                .map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
                let ws = self
                    .backend
                    .ws()
                    .ok_or(TasRepairSuspendRejectedReason::StateCaptureFailed)?;
                zeff_ws_core::save_state::inspect_current_native_wonder_swan_tas_state(
                    &ws.emu,
                    &state_bytes,
                )
                .map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
            }
            crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge => {
                crate::emu_backend::loader::validate_direct_game_gear_tas_private_runtime(
                    &self.backend,
                    false,
                )
                .map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
                let sega8 = self
                    .backend
                    .sega8()
                    .ok_or(TasRepairSuspendRejectedReason::StateCaptureFailed)?;
                zeff_sega8_core::save_state::inspect_current_native_game_gear_tas_state(
                    &sega8.emu,
                    &state_bytes,
                )
                .map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
            }
            crate::emu_thread::TasExecutionProfile::DirectGbaCartridge => {
                super::tas_control::execution::gba::validate_direct_gba_start_state(
                    &self.backend,
                    &state_bytes,
                    identity.persistence,
                )
                .map_err(|_| TasRepairSuspendRejectedReason::StateCaptureFailed)?;
            }
            _ => {}
        }
        let framebuffer = self
            .shared_framebuffer
            .load_full()
            .ok_or(TasRepairSuspendRejectedReason::FramebufferUnavailable)?;
        Ok(TasRepairSuspensionProof {
            identity,
            state_sha256: TasDigest::from_bytes(&state_bytes),
            frame_count,
            framebuffer_sha256: TasDigest::from_bytes(framebuffer.as_slice()),
            framebuffer_len: framebuffer.len(),
            loaded_profile,
        })
    }

    fn discard_repaired_tas_worker(&mut self, identity: TasRepairIdentity) -> bool {
        let Some(active_identity) = self.tas_repair.identity else {
            return self.reject_inactive_repair(identity, TasRepairAction::DiscardRepaired);
        };
        if identity != active_identity {
            return self.reject_parked_action(
                identity,
                TasRepairAction::DiscardRepaired,
                stale_reason(active_identity, identity),
            );
        }
        if self.prepare_tas_control_retirement().is_err() {
            let _ = self.reject_parked_action(
                identity,
                TasRepairAction::DiscardRepaired,
                TasRepairActionRejectedReason::TasRollbackFailed,
            );
            self.tas_repair.nonpersistent_exit_requested = true;
            return false;
        }
        let _ = self.send_resp(EmuResponse::TasRepairRepairedWorkerDiscarded { identity });
        self.tas_repair.nonpersistent_exit_requested = true;
        false
    }

    fn commit_repaired_tas_worker(
        &mut self,
        identity: TasRepairIdentity,
        save_recovery_on_shutdown: bool,
    ) -> bool {
        let Some(active_identity) = self.tas_repair.identity else {
            return self.reject_inactive_repair(identity, TasRepairAction::CommitRepaired);
        };
        if identity != active_identity {
            return self.reject_parked_action(
                identity,
                TasRepairAction::CommitRepaired,
                stale_reason(active_identity, identity),
            );
        }
        if self.tas_control.is_leased() {
            return self.reject_parked_action(
                identity,
                TasRepairAction::CommitRepaired,
                TasRepairActionRejectedReason::TasRollbackFailed,
            );
        }
        let publication = match identity.persistence {
            TasPersistenceContract::Absent => TasPersistencePublicationOutcome::NotRequired,
            persistence => {
                let (Some(byte_len), Some(target_baseline)) =
                    (persistence.byte_len(), persistence.target_baseline())
                else {
                    unreachable!();
                };
                let (system_label, bytes) = match persistence {
                    TasPersistenceContract::NesBattery { .. } => {
                        ("NES", self.backend.nes_tas_battery_bytes())
                    }
                    TasPersistenceContract::GbBattery { .. } => {
                        ("Game Boy", self.backend.gb_tas_battery_bytes())
                    }
                    TasPersistenceContract::GbRtcBattery { .. } => {
                        ("Game Boy RTC", self.backend.gb_tas_rtc_battery_bytes())
                    }
                    TasPersistenceContract::GbaBattery { kind, .. } => (
                        "GBA",
                        self.backend
                            .gba_tas_battery_component()
                            .ok()
                            .flatten()
                            .and_then(|(actual_kind, bytes)| {
                                (actual_kind == kind).then_some(bytes)
                            }),
                    ),
                    TasPersistenceContract::GbaRtcBattery { .. } => {
                        ("GBA RTC", self.backend.gba_tas_rtc_battery_bytes())
                    }
                    TasPersistenceContract::GameGearBattery8KiB { .. } => {
                        ("Game Gear", self.backend.game_gear_tas_battery_bytes())
                    }
                    TasPersistenceContract::WsBattery { save_kind, .. } => (
                        "WonderSwan",
                        self.backend
                            .ws_tas_battery_save_kind()
                            .filter(|actual_kind| *actual_kind == save_kind)
                            .and_then(|_| self.backend.ws_tas_battery_bytes()),
                    ),
                    TasPersistenceContract::WsRtcBattery { save_kind, .. } => (
                        "WonderSwan RTC",
                        self.backend
                            .ws_tas_battery_save_kind()
                            .filter(|actual_kind| *actual_kind == save_kind)
                            .and_then(|_| self.backend.ws_tas_rtc_battery_bytes()),
                    ),
                    TasPersistenceContract::Absent => unreachable!(),
                };
                let Some(bytes) = bytes else {
                    return self.send_resp(EmuResponse::TasRepairRepairedWorkerCommitted {
                        identity: Box::new(identity),
                        publication: TasPersistencePublicationOutcome::NotPublished {
                            error: format!("repaired {system_label} worker has no battery state"),
                        },
                    });
                };
                if bytes.len() as u64 != byte_len {
                    return self.send_resp(EmuResponse::TasRepairRepairedWorkerCommitted {
                        identity: Box::new(identity),
                        publication: TasPersistencePublicationOutcome::NotPublished {
                            error: format!("repaired {system_label} battery topology changed"),
                        },
                    });
                }
                let expected: crate::save_paths::SaveTargetBaseline = target_baseline.into();
                let published = match persistence {
                    TasPersistenceContract::NesBattery { .. } => Ok(self
                        .backend
                        .publish_nes_tas_battery_if_unchanged(expected)
                        .map(|(path, outcome)| (path, outcome, None))),
                    TasPersistenceContract::GbBattery { .. } => Ok(self
                        .backend
                        .publish_gb_tas_battery_if_unchanged(expected)
                        .map(|(path, outcome)| (path, outcome, None))),
                    TasPersistenceContract::GbRtcBattery { .. } => Ok(self
                        .backend
                        .publish_gb_tas_rtc_battery_if_unchanged(expected)
                        .map(|(path, outcome, receipt)| (path, outcome, Some(receipt)))),
                    TasPersistenceContract::GbaBattery { .. } => self
                        .backend
                        .publish_gba_tas_battery_if_unchanged(expected)
                        .map(|published| published.map(|(path, outcome)| (path, outcome, None))),
                    TasPersistenceContract::GbaRtcBattery { .. } => Ok(self
                        .backend
                        .publish_gba_tas_rtc_battery_if_unchanged(expected)
                        .map(|(path, outcome, receipt)| (path, outcome, Some(receipt)))),
                    TasPersistenceContract::GameGearBattery8KiB { .. } => Ok(self
                        .backend
                        .publish_game_gear_tas_battery_if_unchanged(expected)
                        .map(|(path, outcome)| (path, outcome, None))),
                    TasPersistenceContract::WsBattery { .. } => Ok(self
                        .backend
                        .publish_ws_tas_battery_if_unchanged(expected)
                        .map(|(path, outcome)| (path, outcome, None))),
                    TasPersistenceContract::WsRtcBattery { .. } => Ok(self
                        .backend
                        .publish_ws_tas_rtc_battery_if_unchanged(expected)
                        .map(|(path, outcome, receipt)| (path, outcome, Some(receipt)))),
                    TasPersistenceContract::Absent => unreachable!(),
                };
                let published = match published {
                    Ok(published) => published,
                    Err(error) => {
                        return self.send_resp(EmuResponse::TasRepairRepairedWorkerCommitted {
                            identity: Box::new(identity),
                            publication: TasPersistencePublicationOutcome::NotPublished {
                                error: error.to_string(),
                            },
                        });
                    }
                };
                let Some((path, outcome, receipt)) = published else {
                    return self.send_resp(EmuResponse::TasRepairRepairedWorkerCommitted {
                        identity: Box::new(identity),
                        publication: TasPersistencePublicationOutcome::NotPublished {
                            error: "TAS battery publication is unavailable for this worker"
                                .to_owned(),
                        },
                    });
                };
                match outcome {
                    crate::save_paths::SavePublicationOutcome::NotPublished(error) => {
                        TasPersistencePublicationOutcome::NotPublished {
                            error: error.to_string(),
                        }
                    }
                    crate::save_paths::SavePublicationOutcome::PublishedDurabilityUncertain(
                        error,
                    ) => TasPersistencePublicationOutcome::PublishedDurabilityUncertain {
                        path: Some(path),
                        error: error.to_string(),
                    },
                    crate::save_paths::SavePublicationOutcome::PublishedDurable => {
                        let generation = if let Some(receipt) = &receipt {
                            self.recovery.write_generation_for_receipt(receipt)
                        } else {
                            self.recovery.write_generation(&self.backend)
                        };
                        match generation.and_then(|record| {
                            if save_recovery_on_shutdown {
                                self.recovery.write_recovery_state(&self.backend, record)?;
                            }
                            Ok(record)
                        }) {
                            Ok(record) => TasPersistencePublicationOutcome::PublishedDurable {
                                path,
                                generation: record.generation,
                                component_sha256: TasDigest(record.component_sha256),
                            },
                            Err(error) => {
                                TasPersistencePublicationOutcome::PublishedDurabilityUncertain {
                                    path: Some(path),
                                    error: error.to_string(),
                                }
                            }
                        }
                    }
                }
            }
        };
        if !matches!(
            publication,
            TasPersistencePublicationOutcome::NotPublished { .. }
        ) {
            self.tas_repair.identity = None;
            self.save_recovery_on_shutdown = save_recovery_on_shutdown;
        }
        self.send_resp(EmuResponse::TasRepairRepairedWorkerCommitted {
            identity: Box::new(identity),
            publication,
        })
    }

    fn reject_inactive_repair(&self, identity: TasRepairIdentity, action: TasRepairAction) -> bool {
        self.reject_parked_action(
            identity,
            action,
            TasRepairActionRejectedReason::NoMatchingRepair,
        )
    }

    fn reject_parked_action(
        &self,
        identity: TasRepairIdentity,
        action: TasRepairAction,
        reason: TasRepairActionRejectedReason,
    ) -> bool {
        self.send_resp(EmuResponse::TasRepairActionRejected {
            identity,
            action,
            reason,
        })
    }
}

fn validate_suspend_profile(
    identity: TasRepairIdentity,
    observation: &crate::emu_thread::TasLoadedProfileObservation,
    backend: &crate::emu_backend::EmuBackend,
) -> Result<(), TasRepairSuspendRejectedReason> {
    if identity.profile == crate::emu_thread::TasExecutionProfile::DirectSmsCartridge {
        return Err(TasRepairSuspendRejectedReason::ProfileMismatch);
    }
    if observation.profile != identity.profile
        || observation.system
            != match identity.profile {
                crate::emu_thread::TasExecutionProfile::DirectNesCartridge
                | crate::emu_thread::TasExecutionProfile::DirectFdsDisk => {
                    crate::emu_backend::ActiveSystem::Nes
                }
                crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg => {
                    crate::emu_backend::ActiveSystem::GameBoy
                }
                crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb => {
                    crate::emu_backend::ActiveSystem::GameBoy
                }
                crate::emu_thread::TasExecutionProfile::DirectColecoCartridge => {
                    crate::emu_backend::ActiveSystem::Coleco
                }
                crate::emu_thread::TasExecutionProfile::DirectSmsCartridge => {
                    crate::emu_backend::ActiveSystem::MasterSystem
                }
                crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge => {
                    crate::emu_backend::ActiveSystem::GameGear
                }
                crate::emu_thread::TasExecutionProfile::DirectGbaCartridge => {
                    crate::emu_backend::ActiveSystem::GameBoyAdvance
                }
                crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge => {
                    crate::emu_backend::ActiveSystem::Sg1000
                }
                crate::emu_thread::TasExecutionProfile::DirectWsCartridge => {
                    crate::emu_backend::ActiveSystem::WonderSwan
                }
                crate::emu_thread::TasExecutionProfile::DirectPceHuCard
                | crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard
                | crate::emu_thread::TasExecutionProfile::DirectPceCd => {
                    crate::emu_backend::ActiveSystem::Pce
                }
            }
    {
        return Err(TasRepairSuspendRejectedReason::ProfileMismatch);
    }
    let fds_original = identity.profile == crate::emu_thread::TasExecutionProfile::DirectFdsDisk;
    if !fds_original && observation.source_media_sha256 != Some(identity.source_media_sha256) {
        return Err(TasRepairSuspendRejectedReason::SourceMediaMismatch);
    }
    if observation.effective_media_sha256 != Some(identity.effective_media_sha256) {
        return Err(TasRepairSuspendRejectedReason::EffectiveMediaMismatch);
    }
    if !observation.identity_metadata_matches
        || !observation.load_provenance_available
        || (!fds_original && observation.direct_source != Some(true))
        || observation.mods_absent != Some(true)
        || observation.initial_input_neutral != Some(true)
        || !observation.firmware_profile_matches
        || !observation.hardware_profile_matches
        || !observation.controller_profile_matches
        || !observation.removable_media_absent
        || !observation.cheats_absent
    {
        return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
    }
    let persistence_byte_len = identity.persistence.byte_len();
    match identity.persistence {
        crate::emu_thread::TasPersistenceContract::Absent => {
            if !fds_original && observation.persistent_state_absent != Some(true) {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::NesBattery { .. } => {
            let bytes = backend
                .nes_tas_battery_bytes()
                .ok_or(TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if identity.profile != crate::emu_thread::TasExecutionProfile::DirectNesCartridge
                || !backend.save_ram_kind().is_battery_backed()
                || Some(bytes.len() as u64) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::GbBattery { .. } => {
            let bytes = backend
                .gb_tas_battery_bytes()
                .ok_or(TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if !matches!(
                identity.profile,
                crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg
                    | crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb
            ) || !backend.save_ram_kind().is_battery_backed()
                || Some(bytes.len() as u64) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::GbRtcBattery { .. } => {
            let witness = crate::emu_backend::loader::gb_rtc_persistence_witness(backend)
                .map_err(|_| TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if !matches!(
                identity.profile,
                crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg
                    | crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb
            ) || Some(witness.complete_byte_len) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::GbaBattery { kind, .. } => {
            let (actual_kind, bytes) = backend
                .gba_tas_battery_component()
                .map_err(|_| TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?
                .ok_or(TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if identity.profile != crate::emu_thread::TasExecutionProfile::DirectGbaCartridge
                || actual_kind != kind
                || Some(bytes.len() as u64) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::GbaRtcBattery { kind, .. } => {
            let witness = crate::emu_backend::gba::gba_rtc_persistence_witness(backend)
                .map_err(|_| TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if identity.profile != crate::emu_thread::TasExecutionProfile::DirectGbaCartridge
                || witness.backup_kind != kind
                || Some(witness.complete_byte_len) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::GameGearBattery8KiB { .. } => {
            let bytes = backend
                .game_gear_tas_battery_bytes()
                .ok_or(TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if identity.profile != crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge
                || persistence_byte_len != Some(8 * 1024)
                || backend.save_ram_kind()
                    != zeff_emu_common::save_ram::SaveRamKind::known_battery_backed(8 * 1024)
                || Some(bytes.len() as u64) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::WsBattery { save_kind, .. } => {
            let bytes = backend
                .ws_tas_battery_bytes()
                .ok_or(TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if identity.profile != crate::emu_thread::TasExecutionProfile::DirectWsCartridge
                || backend.ws_tas_battery_save_kind() != Some(save_kind)
                || Some(bytes.len() as u64) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
        crate::emu_thread::TasPersistenceContract::WsRtcBattery { save_kind, .. } => {
            let witness = crate::emu_backend::ws::ws_rtc_persistence_witness(backend)
                .map_err(|_| TasRepairSuspendRejectedReason::UnsafeLoadedProfile)?;
            if identity.profile != crate::emu_thread::TasExecutionProfile::DirectWsCartridge
                || witness.save_kind != save_kind
                || Some(witness.complete_byte_len) != persistence_byte_len
            {
                return Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile);
            }
        }
    }
    Ok(())
}

fn suspend_blocker(reason: TasControlAcquireRejectedReason) -> TasRepairSuspendRejectedReason {
    match reason {
        TasControlAcquireRejectedReason::AlreadyLeased { .. } => {
            TasRepairSuspendRejectedReason::AlreadyLeased
        }
        TasControlAcquireRejectedReason::UncappedExecution => {
            TasRepairSuspendRejectedReason::UncappedExecution
        }
        TasControlAcquireRejectedReason::AudioRecordingActive => {
            TasRepairSuspendRejectedReason::AudioRecordingActive
        }
        TasControlAcquireRejectedReason::LinkActivity => {
            TasRepairSuspendRejectedReason::LinkActivity
        }
        TasControlAcquireRejectedReason::PendingFrameDelivery => {
            TasRepairSuspendRejectedReason::PendingFrameDelivery
        }
        TasControlAcquireRejectedReason::RuntimeFault => {
            TasRepairSuspendRejectedReason::RuntimeFault
        }
        TasControlAcquireRejectedReason::ReplayActivityUnwitnessed => {
            TasRepairSuspendRejectedReason::ReplayActivityUnwitnessed
        }
        _ => TasRepairSuspendRejectedReason::UnsafeLoadedProfile,
    }
}

fn stale_reason(
    active: TasRepairIdentity,
    requested: TasRepairIdentity,
) -> TasRepairActionRejectedReason {
    if active.repair_id == requested.repair_id {
        TasRepairActionRejectedReason::StaleToken
    } else {
        TasRepairActionRejectedReason::NoMatchingRepair
    }
}

fn resume_capture_reason(reason: TasRepairSuspendRejectedReason) -> TasRepairActionRejectedReason {
    match reason {
        TasRepairSuspendRejectedReason::StateCaptureFailed
        | TasRepairSuspendRejectedReason::StateChangedDuringCapture => {
            TasRepairActionRejectedReason::StateCaptureFailed
        }
        TasRepairSuspendRejectedReason::FramebufferUnavailable => {
            TasRepairActionRejectedReason::FramebufferMismatch
        }
        _ => TasRepairActionRejectedReason::LoadedProfileMismatch,
    }
}

fn proof_mismatch_reason(
    expected: &TasRepairSuspensionProof,
    actual: &TasRepairSuspensionProof,
) -> TasRepairActionRejectedReason {
    if expected.state_sha256 != actual.state_sha256 {
        TasRepairActionRejectedReason::StateDigestMismatch
    } else if expected.frame_count != actual.frame_count {
        TasRepairActionRejectedReason::FrameCountMismatch
    } else if expected.framebuffer_sha256 != actual.framebuffer_sha256
        || expected.framebuffer_len != actual.framebuffer_len
    {
        TasRepairActionRejectedReason::FramebufferMismatch
    } else {
        TasRepairActionRejectedReason::LoadedProfileMismatch
    }
}
