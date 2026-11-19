use std::ops::ControlFlow;

use super::EmuLoop;
use crate::emu_thread::{EmuCommand, EmuResponse};
use crate::netplay::{Response, session::Session};

pub(super) struct MdWorkerBaseline {
    battery: super::BatteryFlushSchedule,
    discontinuities: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    recovery: super::RecoveryCoordinator,
    save_recovery: bool,
}

impl MdWorkerBaseline {
    fn capture(worker: &EmuLoop) -> Self {
        Self {
            battery: worker.battery_flush.clone(),
            discontinuities: worker.pending_audio_discontinuities.clone(),
            recovery: worker.recovery.clone(),
            save_recovery: worker.save_recovery_on_shutdown,
        }
    }

    fn verify(&self, worker: &EmuLoop) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.battery == worker.battery_flush
                && self.discontinuities == worker.pending_audio_discontinuities
                && self.recovery == worker.recovery
                && self.save_recovery == worker.save_recovery_on_shutdown,
            "MD worker persistence baseline differs"
        );
        Ok(())
    }
}

impl EmuLoop {
    pub(super) fn dispatch_netplay(
        &mut self,
        command: EmuCommand,
    ) -> ControlFlow<bool, EmuCommand> {
        if self.netplay_restore_failed && !matches!(command, EmuCommand::Shutdown) {
            let reason = "netplay restoration failed; worker requires replacement".into();
            let response = if matches!(self.backend, crate::emu_backend::EmuBackend::Md(_)) {
                Response::Stopped {
                    reason,
                    restored: false,
                }
            } else {
                Response::Rejected(reason)
            };
            return ControlFlow::Break(self.send_resp(EmuResponse::Netplay(response)));
        }
        let result = match command {
            EmuCommand::StartNetplay(start) => {
                if self.netplay.is_some()
                    || self.tas_control.is_leased()
                    || self.tas_repair.identity.is_some()
                    || self.uncapped_mode
                    || !self.last_cheats.is_empty()
                    || self.audio_recording_capture.active
                    || !self.drain_rx.is_empty()
                    || !self.runtime_fault.can_step()
                    || self.pending_tcp_link.is_some()
                    || self.tcp_link.is_some()
                    || self.game_boy_replay_link.is_some()
                    || self.wonder_swan_replay_link.is_some()
                {
                    Err(anyhow::anyhow!(
                        "worker is occupied by another execution owner"
                    ))
                } else {
                    let baseline = matches!(self.backend, crate::emu_backend::EmuBackend::Md(_))
                        .then(|| MdWorkerBaseline::capture(self));
                    match Session::start(&mut self.backend, *start) {
                        Ok(session) => {
                            self.md_netplay_baseline = baseline;
                            self.netplay = Some(session);
                            self.speculation.invalidate();
                            Ok(())
                        }
                        Err(mut error) => {
                            self.netplay_restore_failed = matches!(&self.backend,
                                crate::emu_backend::EmuBackend::Md(md) if md.rollback_owned());
                            if let Some(baseline) = baseline
                                && let Err(worker) = baseline.verify(self)
                            {
                                self.netplay_restore_failed = true;
                                error = error.context(format!("MD worker verification: {worker}"));
                            }
                            if self.netplay_restore_failed {
                                return ControlFlow::Break(self.send_resp(EmuResponse::Netplay(
                                    Response::Stopped {
                                        reason: error.to_string(),
                                        restored: false,
                                    },
                                )));
                            }
                            Err(error)
                        }
                    }
                }
            }
            EmuCommand::StepNetplay(buttons) => match self.netplay.as_mut() {
                Some(session) => {
                    let result = session.step(&mut self.backend, buttons);
                    if let Err(error) = result {
                        self.stop_netplay(error.to_string());
                    }
                    Ok(())
                }
                None => Err(anyhow::anyhow!("no netplay session")),
            },
            EmuCommand::SetNetplayPaused(paused) => match self.netplay.as_mut() {
                Some(session) => session.request_pause(paused),
                None => Err(anyhow::anyhow!("no netplay session")),
            },
            EmuCommand::SendNetplayChat(text) => {
                let result = self
                    .netplay
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Connect before sending a message."))
                    .and_then(|session| session.send_chat(text));
                if let Err(error) = result {
                    let _ = self
                        .send_resp(EmuResponse::Netplay(Response::ChatError(error.to_string())));
                }
                Ok(())
            }
            EmuCommand::StopNetplay => {
                if self.netplay.is_none() {
                    Err(anyhow::anyhow!("no netplay session"))
                } else {
                    self.stop_netplay("local stop".into());
                    Ok(())
                }
            }
            EmuCommand::Shutdown => return ControlFlow::Continue(EmuCommand::Shutdown),
            command if self.netplay.is_none() => return ControlFlow::Continue(command),
            _ => Err(anyhow::anyhow!(
                "netplay owns worker execution and persistence"
            )),
        };
        ControlFlow::Break(result.map_or_else(
            |error| self.send_resp(EmuResponse::Netplay(Response::Rejected(error.to_string()))),
            |()| true,
        ))
    }

    pub(super) fn poll_netplay(&mut self) {
        let mut service = true;
        for _ in 0..64 {
            let Some(session) = self.netplay.as_mut() else {
                return;
            };
            let result = if service {
                service = false;
                session.poll(&mut self.backend)
            } else {
                Ok(session.take_response())
            };
            if !self.deliver_netplay_response(result) {
                return;
            }
        }
    }

    pub(super) fn wait_netplay_command(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<Option<EmuCommand>, crossbeam_channel::RecvError> {
        let session = self.netplay.as_ref().unwrap();
        let timeout = if session.has_pending_work() {
            std::time::Duration::ZERO
        } else {
            timeout
        };
        crossbeam_channel::select! {
            recv(self.cmd_rx) -> command => command.map(Some),
            recv(session.events()) -> event => {
                if let Ok(event) = event {
                    let result = self.netplay.as_mut().unwrap().accept_event(&mut self.backend, event);
                    self.deliver_netplay_response(result);
                }
                Ok(None)
            }
            default(timeout) => Ok(None),
        }
    }

    fn deliver_netplay_response(&mut self, result: anyhow::Result<Option<Response>>) -> bool {
        match result {
            Ok(Some(response)) => {
                if matches!(response, Response::Presented { changed: true, .. }) {
                    super::super::types::publish_backend_framebuffer(
                        &self.shared_framebuffer,
                        &self.backend,
                    );
                }
                let _ = self.send_resp(EmuResponse::Netplay(response));
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.stop_netplay(error.to_string());
                false
            }
        }
    }

    pub(super) fn stop_netplay(&mut self, reason: String) {
        let Some(session) = self.netplay.take() else {
            return;
        };
        let baseline = self.md_netplay_baseline.take();
        let worker = match &baseline {
            Some(baseline) => baseline.verify(self),
            None if matches!(self.backend, crate::emu_backend::EmuBackend::Md(_)) => {
                Err(anyhow::anyhow!("MD worker baseline is missing"))
            }
            None => Ok(()),
        };
        let result = if baseline.is_none()
            && !matches!(self.backend, crate::emu_backend::EmuBackend::Md(_))
        {
            session.restore(&mut self.backend)
        } else {
            session.restore_with_worker(&mut self.backend, worker)
        };
        self.netplay_restore_failed = result.is_err();
        let restored = !self.netplay_restore_failed;
        let reason = match result {
            Ok(()) => reason,
            Err(error) => format!("{reason}; restoration: {error}"),
        };
        if restored {
            super::super::types::publish_backend_framebuffer(
                &self.shared_framebuffer,
                &self.backend,
            );
            if baseline.is_none() {
                self.pending_audio_discontinuities.clear();
            }
        }
        let _ = self.send_resp(EmuResponse::Netplay(Response::Stopped { reason, restored }));
    }
}

#[cfg(test)]
mod tests;
