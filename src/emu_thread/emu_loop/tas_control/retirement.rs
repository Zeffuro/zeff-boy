use std::time::{Duration, Instant};

use crate::emu_thread::EmuResponse;

use super::{EmuLoop, restore_backend_checkpoint};
use crate::emu_thread::emu_loop::PENDING_LINK_POLL_INTERVAL;

impl EmuLoop {
    pub(in crate::emu_thread::emu_loop) fn command_wait_timeout(
        &self,
        now: Instant,
    ) -> Option<Duration> {
        let link_timeout = self
            .pending_tcp_link
            .is_some()
            .then_some(PENDING_LINK_POLL_INTERVAL);
        let save_timeout = if self.periodic_battery_flush_blocked() {
            None
        } else {
            self.battery_flush.wait_timeout(now)
        };
        match (link_timeout, save_timeout) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(timeout), None) | (None, Some(timeout)) => Some(timeout),
            (None, None) => None,
        }
    }

    pub(in crate::emu_thread::emu_loop) fn periodic_battery_flush_blocked(&self) -> bool {
        self.tas_control.is_leased()
            || self.tas_repair.identity.is_some()
            || self.pending_tcp_link.is_some()
            || self.tcp_link.is_some()
    }

    pub(in crate::emu_thread::emu_loop) fn prepare_tas_control_retirement(
        &mut self,
    ) -> Result<(), String> {
        let Some(lease_id) = self.tas_control.active_lease_id() else {
            return Ok(());
        };
        let backend = &mut self.backend;
        let persistence = self.tas_repair.identity.map_or(
            crate::emu_thread::TasPersistenceContract::Absent,
            |identity| identity.persistence,
        );
        let response = self.tas_control.rollback(lease_id, |checkpoint| {
            restore_backend_checkpoint(backend, checkpoint, persistence)
        });
        match response {
            EmuResponse::TasControlRolledBack { .. } => {
                self.finalize_tas_loaded_observables("(TAS rollback)");
                Ok(())
            }
            EmuResponse::TasControlRollbackRejected { reason, .. } => {
                Err(format!("TAS checkpoint restoration failed: {reason:?}"))
            }
            _ => unreachable!("worker retirement produced a non-rollback response"),
        }
    }

    pub(in crate::emu_thread::emu_loop) fn finish_shutdown(&mut self) {
        self.disconnect_tcp_link();
        if let Err(error) = self.prepare_tas_control_retirement() {
            let _ = self
                .resp_tx
                .send(EmuResponse::SramFlushFailed(error.clone()));
            if self.save_recovery_on_shutdown {
                let _ = self.resp_tx.send(EmuResponse::RecoverySaveFailed(error));
            }
            let _ = self.resp_tx.send(EmuResponse::ShutdownComplete);
            return;
        }
        if self.tas_repair.identity.is_some() {
            self.tas_repair.nonpersistent_exit_requested = true;
            let _ = self.resp_tx.send(EmuResponse::SramFlushed(None));
            let _ = self.resp_tx.send(EmuResponse::ShutdownComplete);
            return;
        }
        self.handle_shutdown();
    }

    pub(super) fn finalize_tas_loaded_observables(&mut self, path: &str) {
        super::super::super::types::publish_framebuffer(
            &self.shared_framebuffer,
            self.backend.framebuffer(),
        );
        self.pending_audio_discontinuities.clear();
        let response = EmuResponse::LoadStateOk {
            path: path.to_owned(),
            warning: None,
            media_slot_snapshot: self.backend.media_slot_snapshot(),
            game_boy_serial_device: self.backend.game_boy_serial_device(),
        };
        let loaded = (super::super::super::commands::LoadFinalizationContext {
            rewind_buffer: &mut self.rewind_buffer,
            rewind_seconds: self.rewind_seconds,
            backend: &mut self.backend,
            cheats: &self.last_cheats,
            audio_recording_capture: self.audio_recording_capture,
            pending_audio_discontinuities: &mut self.pending_audio_discontinuities,
        })
        .finalize(&response, |frame_duration_ns| {
            self.frame_duration_ns
                .store(frame_duration_ns, std::sync::atomic::Ordering::Release);
        });
        debug_assert!(loaded);
        self.runtime_fault = crate::emu_thread::WorkerRuntimeFault::default();
    }
}
