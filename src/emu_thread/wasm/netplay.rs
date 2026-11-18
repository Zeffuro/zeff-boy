use super::*;
use crate::netplay::{Response, session::Session};

#[cfg(test)]
mod md_tests;
mod qualification;

pub(super) struct MdWorkerBaseline {
    dirty: crate::platform::DirtyEpoch<Vec<crate::platform::SaveWrite>>,
    deadline: web_time::Instant,
    requested: bool,
    potentially_dirty: bool,
    discontinuities: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    recovery: RecoveryCoordinator,
    save_recovery: bool,
}

impl MdWorkerBaseline {
    fn capture(inner: &Inner) -> Self {
        Self {
            dirty: inner.battery_dirty.clone(),
            deadline: inner.next_battery_flush,
            requested: inner.battery_flush_requested,
            potentially_dirty: inner.battery_potentially_dirty,
            discontinuities: inner.pending_audio_discontinuities.clone(),
            recovery: inner.recovery.clone(),
            save_recovery: inner.save_recovery_on_shutdown,
        }
    }

    fn verify(&self, inner: &Inner) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.dirty == inner.battery_dirty
                && self.deadline == inner.next_battery_flush
                && self.requested == inner.battery_flush_requested
                && self.potentially_dirty == inner.battery_potentially_dirty
                && self.discontinuities == inner.pending_audio_discontinuities
                && self.recovery == inner.recovery
                && self.save_recovery == inner.save_recovery_on_shutdown
                && inner.pending_storage.is_none()
                && inner.deferred_storage_commands.is_empty()
                && !inner.shutdown_requested,
            "MD browser worker persistence baseline differs"
        );
        Ok(())
    }
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
mod tests;

impl EmuThread {
    pub(super) fn dispatch_netplay(&self, command: EmuCommand) -> Option<EmuCommand> {
        let inner = &mut *self.inner.borrow_mut();
        if inner.netplay_restore_failed {
            inner
                .pending_responses
                .push_back(if matches!(command, EmuCommand::Shutdown) {
                    EmuResponse::ShutdownComplete
                } else {
                    let reason = "Restoration failed. Stop the game before continuing.".into();
                    EmuResponse::Netplay(if matches!(inner.backend, EmuBackend::Md(_)) {
                        Response::Stopped {
                            reason,
                            restored: false,
                        }
                    } else {
                        Response::Rejected(reason)
                    })
                });
            return None;
        }
        let result = match command {
            EmuCommand::StartNetplay(start) => {
                if inner.netplay.is_some()
                    || inner.pending_storage.is_some()
                    || !inner.deferred_storage_commands.is_empty()
                    || inner.shutdown_requested
                    || inner.uncapped_mode
                    || !inner.last_cheats.is_empty()
                    || inner.audio_recording_capture.active
                    || !inner.pending_frames.is_empty()
                    || !inner.runtime_fault.can_step()
                {
                    Err(anyhow::anyhow!("Emulator or browser storage is busy"))
                } else {
                    let baseline = matches!(inner.backend, EmuBackend::Md(_))
                        .then(|| MdWorkerBaseline::capture(inner));
                    match Session::start(&mut inner.backend, *start) {
                        Ok(session) => {
                            if baseline.is_none() {
                                inner.battery_potentially_dirty = false;
                                inner.battery_flush_requested = false;
                            }
                            inner.md_netplay_baseline = baseline;
                            inner.netplay = Some(session);
                            inner.speculation.invalidate();
                            Ok(())
                        }
                        Err(mut error) => {
                            inner.netplay_restore_failed = matches!(&inner.backend,
                                EmuBackend::Md(md) if md.rollback_owned());
                            if let Some(baseline) = baseline
                                && let Err(worker) = baseline.verify(inner)
                            {
                                inner.netplay_restore_failed = true;
                                error = error.context(format!("MD worker verification: {worker}"));
                            }
                            if inner.netplay_restore_failed {
                                inner.pending_responses.push_back(EmuResponse::Netplay(
                                    Response::Stopped {
                                        reason: error.to_string(),
                                        restored: false,
                                    },
                                ));
                                return None;
                            }
                            Err(error)
                        }
                    }
                }
            }
            EmuCommand::StepNetplay(buttons) => {
                let result = inner
                    .netplay
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("No netplay session"))
                    .and_then(|session| session.step(&mut inner.backend, buttons));
                if let Err(error) = result {
                    self.stop_netplay_inner(inner, error.to_string());
                }
                Ok(())
            }
            EmuCommand::SetNetplayPaused(paused) => inner
                .netplay
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("No netplay session"))
                .and_then(|session| session.request_pause(paused)),
            EmuCommand::SendNetplayChat(text) => {
                let result = inner
                    .netplay
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Connect first"))
                    .and_then(|session| session.send_chat(text));
                if let Err(error) = result {
                    inner
                        .pending_responses
                        .push_back(EmuResponse::Netplay(Response::ChatError(error.to_string())));
                }
                Ok(())
            }
            EmuCommand::StopNetplay => {
                self.stop_netplay_inner(inner, "Disconnected".into());
                Ok(())
            }
            EmuCommand::Shutdown => {
                self.stop_netplay_inner(inner, "Game closed".into());
                if inner.netplay_restore_failed {
                    inner
                        .pending_responses
                        .push_back(EmuResponse::ShutdownComplete);
                    return None;
                }
                return Some(EmuCommand::Shutdown);
            }
            command if inner.netplay.is_none() => return Some(command),
            _ => Err(anyhow::anyhow!(
                "Disconnect netplay before changing the game"
            )),
        };
        if let Err(error) = result {
            inner
                .pending_responses
                .push_back(EmuResponse::Netplay(Response::Rejected(error.to_string())));
        }
        None
    }

    pub(super) fn poll_netplay(&self) {
        self.drain_netplay(false);
    }

    pub(crate) fn service_netplay(&self) {
        self.drain_netplay(true);
    }

    fn drain_netplay(&self, service_md: bool) {
        let inner = &mut *self.inner.borrow_mut();
        let mut service = service_md || !matches!(inner.backend, EmuBackend::Md(_));
        for _ in 0..32 {
            let Some(session) = &mut inner.netplay else {
                break;
            };
            if inner.pending_responses.len() >= 64 {
                break;
            }
            let result = if service {
                service = false;
                session.poll(&mut inner.backend)
            } else {
                Ok(session.take_response())
            };
            match result {
                Ok(Some(response)) => {
                    if matches!(response, Response::Presented { changed: true, .. }) {
                        types::publish_backend_framebuffer(
                            &self.shared_framebuffer,
                            &inner.backend,
                        );
                    }
                    inner
                        .pending_responses
                        .push_back(EmuResponse::Netplay(response));
                }
                Ok(None) => break,
                Err(error) => {
                    self.stop_netplay_inner(inner, error.to_string());
                    break;
                }
            }
        }
    }

    fn stop_netplay_inner(&self, inner: &mut Inner, reason: String) {
        let Some(session) = inner.netplay.take() else {
            return;
        };
        let baseline = inner.md_netplay_baseline.take();
        let worker = match &baseline {
            Some(baseline) => baseline.verify(inner),
            None if matches!(inner.backend, EmuBackend::Md(_)) => {
                Err(anyhow::anyhow!("MD browser worker baseline is missing"))
            }
            None => Ok(()),
        };
        let result = if baseline.is_none() && !matches!(inner.backend, EmuBackend::Md(_)) {
            session.restore(&mut inner.backend)
        } else {
            session.restore_with_worker(&mut inner.backend, worker)
        };
        inner.netplay_restore_failed = result.is_err();
        let restored = !inner.netplay_restore_failed;
        let reason = match result {
            Ok(()) => reason,
            Err(error) => format!("{reason}. Restoration: {error}"),
        };
        if restored {
            types::publish_backend_framebuffer(&self.shared_framebuffer, &inner.backend);
            if baseline.is_none() {
                inner.pending_audio_discontinuities.clear();
                inner.rewind_buffer.clear();
            }
            inner.speculation.invalidate();
        }
        inner
            .pending_responses
            .push_back(EmuResponse::Netplay(Response::Stopped { reason, restored }));
    }
}
