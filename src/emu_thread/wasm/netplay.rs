use super::*;
use crate::netplay::{Response, session::Session};

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
                    EmuResponse::Netplay(Response::Rejected(
                        "Restoration failed. Stop the game before continuing.".into(),
                    ))
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
                    Session::start(&mut inner.backend, *start).map(|session| {
                        inner.netplay = Some(session);
                        inner.speculation.invalidate();
                        inner.battery_potentially_dirty = false;
                        inner.battery_flush_requested = false;
                    })
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
        let inner = &mut *self.inner.borrow_mut();
        for _ in 0..32 {
            let Some(session) = &mut inner.netplay else {
                break;
            };
            if inner.pending_responses.len() >= 64 {
                break;
            }
            match session.poll(&mut inner.backend) {
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
        let result = session.restore(&mut inner.backend);
        inner.netplay_restore_failed = result.is_err();
        let restored = !inner.netplay_restore_failed;
        let reason = match result {
            Ok(()) => reason,
            Err(error) => format!("{reason}. Restoration: {error}"),
        };
        if restored {
            types::publish_backend_framebuffer(&self.shared_framebuffer, &inner.backend);
            inner.pending_audio_discontinuities.clear();
            inner.speculation.invalidate();
            inner.rewind_buffer.clear();
        }
        inner
            .pending_responses
            .push_back(EmuResponse::Netplay(Response::Stopped { reason, restored }));
    }
}
