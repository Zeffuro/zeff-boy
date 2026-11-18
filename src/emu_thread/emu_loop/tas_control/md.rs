use super::{EmuLoop, TasControlCheckpoint, TasDigest, TasRestoredCheckpoint, WorkerTasStateCache};
use crate::emu_backend::md::tas::{MdTasLiveLease, MdTasLiveProof};
use crate::emu_backend::{EmuBackend, loader::md_product};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasControlAcquireRejectedReason, TasExecutionProfile,
};
use anyhow::{Result, ensure};
use std::ops::ControlFlow;
use std::sync::atomic::Ordering;

mod execution;
mod finalization;

pub(in crate::emu_thread::emu_loop) struct MdLiveControl {
    lease: MdTasLiveLease,
    baseline: WorkerBaseline,
    proof: Option<MdTasLiveProof>,
    seeded: bool,
    failed: bool,
    reservation: usize,
}

struct WorkerBaseline {
    battery: crate::emu_thread::persistence::BatteryFlushSchedule,
    recovery: crate::emu_thread::recovery::RecoveryCoordinator,
    fault: crate::emu_thread::WorkerRuntimeFault,
    discontinuities: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    recording: crate::emu_thread::AudioRecordingCapture,
    duration: u64,
    rewind_seconds: usize,
    save_recovery: bool,
}

impl WorkerBaseline {
    fn capture(worker: &EmuLoop) -> Self {
        Self {
            battery: worker.battery_flush.clone(),
            recovery: worker.recovery.clone(),
            fault: worker.runtime_fault.clone(),
            discontinuities: worker.pending_audio_discontinuities.clone(),
            recording: worker.audio_recording_capture,
            duration: worker.frame_duration_ns.load(Ordering::Acquire),
            rewind_seconds: worker.rewind_seconds,
            save_recovery: worker.save_recovery_on_shutdown,
        }
    }

    fn verify(&self, worker: &EmuLoop) -> Result<()> {
        ensure!(
            self.battery == worker.battery_flush
                && self.recovery == worker.recovery
                && self.fault == worker.runtime_fault
                && self.discontinuities == worker.pending_audio_discontinuities
                && self.recording == worker.audio_recording_capture
                && self.duration == worker.frame_duration_ns.load(Ordering::Acquire)
                && self.rewind_seconds == worker.rewind_seconds
                && self.save_recovery == worker.save_recovery_on_shutdown,
            "MD TAS frozen worker host state changed"
        );
        Ok(())
    }
}

impl EmuLoop {
    pub(super) fn dispatch_md_tas(
        &mut self,
        command: EmuCommand,
    ) -> ControlFlow<EmuResponse, EmuCommand> {
        if let EmuCommand::AcquireTasControl {
            request_id,
            profile,
        } = command
        {
            if md_product::md_pad_profile(profile).is_none() {
                return ControlFlow::Continue(EmuCommand::AcquireTasControl {
                    request_id,
                    profile,
                });
            }
            return ControlFlow::Break(self.acquire_md_tas(request_id, profile));
        }
        if self.md_tas.is_none() {
            return ControlFlow::Continue(command);
        }
        match command {
            EmuCommand::ExecuteTasControl(request) => {
                let mut owned = self.md_tas.take().unwrap();
                let mut cache = std::mem::replace(
                    &mut self.tas_control.state_cache,
                    WorkerTasStateCache::new(),
                );
                let backend = &mut self.backend;
                let response = self.tas_control.execute(*request, |request, _| {
                    owned.execute(backend, &mut cache, request)
                });
                self.tas_control.state_cache = cache;
                self.md_tas = Some(owned);
                if matches!(response, EmuResponse::TasExecutionCompleted { .. }) {
                    self.publish_md_tas();
                }
                ControlFlow::Break(response)
            }
            EmuCommand::AdvanceTasControl(request) => {
                let mut owned = self.md_tas.take().unwrap();
                let mut cache = std::mem::replace(
                    &mut self.tas_control.state_cache,
                    WorkerTasStateCache::new(),
                );
                let backend = &mut self.backend;
                let proof = self.tas_control.candidate_cache_proof();
                let response =
                    self.tas_control
                        .advance(*request, |candidate, input, snapshot, _| {
                            owned.advance(
                                execution::MdAdvanceContext {
                                    backend,
                                    cache: &mut cache,
                                    cache_proof: proof,
                                },
                                candidate,
                                input,
                                snapshot,
                            )
                        });
                self.tas_control.state_cache = cache;
                self.md_tas = Some(owned);
                if matches!(response, EmuResponse::TasFrameAdvanced { .. }) {
                    self.publish_md_tas();
                }
                ControlFlow::Break(response)
            }
            EmuCommand::RollbackTasControl { lease_id } => {
                ControlFlow::Break(self.rollback_md_tas(lease_id))
            }
            EmuCommand::CommitTasControl { lease_id } => {
                ControlFlow::Break(self.commit_md_tas(lease_id))
            }
            command => ControlFlow::Continue(command),
        }
    }

    fn acquire_md_tas(&mut self, request_id: u64, profile: TasExecutionProfile) -> EmuResponse {
        let context = self.tas_control_context();
        let cheats = !self.last_cheats.is_empty();
        let backend = &self.backend;
        let prepared =
            match self
                .tas_control
                .prepare_acquire(request_id, profile, context, |profile, _| {
                    md_product::build_md_tas_witness(backend, cheats, profile)
                }) {
                Ok(prepared) => prepared,
                Err(reason) => {
                    return EmuResponse::TasControlAcquireRejected { request_id, reason };
                }
            };
        let baseline = WorkerBaseline::capture(self);
        let reservation = prepared
            .retained_bytes()
            .saturating_add(std::mem::size_of::<MdLiveControl>())
            .saturating_add(std::mem::size_of::<super::execution::TasExecutionResult>())
            .saturating_add(
                self.tas_control.discarded_audio_samples.capacity() * std::mem::size_of::<f32>(),
            )
            .saturating_add(baseline.recovery.retained_heap_bytes())
            .saturating_add(baseline.fault.retained_heap_bytes())
            .saturating_add(
                baseline.discontinuities.capacity()
                    * std::mem::size_of::<crate::audio_recorder::AudioTimelineDiscontinuity>(),
            );
        let EmuBackend::Md(md) = &mut self.backend else {
            unreachable!()
        };
        match md.begin_tas_live(cheats) {
            Ok(lease) => {
                self.tas_control.state_cache.clear_md();
                self.tas_control.state_cache.reserve_md(reservation);
                self.md_tas = Some(MdLiveControl {
                    lease,
                    baseline,
                    proof: None,
                    seeded: false,
                    failed: false,
                    reservation,
                });
                self.tas_control.accept_acquire(prepared)
            }
            Err(error) => {
                if let Some(lease) = error.fenced_lease {
                    self.md_tas = Some(MdLiveControl {
                        lease,
                        baseline,
                        proof: None,
                        seeded: false,
                        failed: true,
                        reservation,
                    });
                    let EmuResponse::TasControlAcquired {
                        request_id,
                        lease_id,
                        witness,
                    } = self.tas_control.accept_acquire(prepared)
                    else {
                        unreachable!()
                    };
                    EmuResponse::TasControlAcquireFenced {
                        request_id,
                        lease_id,
                        witness,
                        error: error.error.to_string(),
                    }
                } else {
                    EmuResponse::TasControlAcquireRejected {
                        request_id,
                        reason: TasControlAcquireRejectedReason::StateWitnessUnavailable,
                    }
                }
            }
        }
    }

    fn publish_md_tas(&self) {
        crate::emu_thread::types::publish_backend_framebuffer(
            &self.shared_framebuffer,
            &self.backend,
        );
    }
}

#[cfg(test)]
impl EmuLoop {
    pub(in crate::emu_thread::emu_loop) fn md_tas_cache_observation_for_test(
        &self,
    ) -> (usize, usize, u64) {
        let owned = self.md_tas.as_ref().unwrap();
        let EmuBackend::Md(md) = &self.backend else {
            unreachable!()
        };
        let (entries, hits) = self.tas_control.state_cache.md_cache_observation();
        (
            entries,
            hits,
            self.tas_control
                .state_cache
                .md_retained_bytes(md, &owned.lease, &[])
                .unwrap(),
        )
    }

    pub(in crate::emu_thread::emu_loop) fn md_tas_retained_graph_for_test(
        &self,
    ) -> zeff_md_core::emulator::rollback::MdRetainedPayload {
        let EmuBackend::Md(md) = &self.backend else {
            unreachable!()
        };
        self.tas_control
            .state_cache
            .md_retained_graph(md, &[])
            .unwrap()
    }

    pub(in crate::emu_thread::emu_loop) fn corrupt_md_tas_cache_for_test(&mut self) {
        self.tas_control.state_cache.corrupt_md_for_test();
    }
}
