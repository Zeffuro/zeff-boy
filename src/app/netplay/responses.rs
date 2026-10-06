use super::*;

impl App {
    pub(in crate::app) fn consume_netplay_response(
        &mut self,
        response: EmuResponse,
    ) -> Option<EmuResponse> {
        let EmuResponse::Netplay(response) = response else {
            return Some(response);
        };
        if matches!(
            self.netplay.phase,
            Phase::Idle | Phase::Preparing(_) | Phase::Connecting
        ) {
            return None;
        }
        match response {
            Response::NetworkStats(stats)
                if matches!(self.netplay.phase, Phase::Running | Phase::Stopping) =>
            {
                self.debug_windows.netplay.network_metrics = Some(stats);
            }
            Response::Chat { local, text } if self.netplay.running() => {
                if local {
                    self.debug_windows.netplay.chat_sent(&text);
                }
                self.debug_windows.netplay.chat.push(local, text);
            }
            Response::ChatError(error) if self.netplay.running() => {
                self.debug_windows.netplay.chat_failed(error);
            }
            Response::Chat { .. } | Response::ChatError(_)
                if self.netplay.phase == Phase::Stopping => {}
            Response::Ready if self.netplay.phase == Phase::Stopping => {
                self.netplay.admitted = true;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(proof) = &mut self.netplay.proof {
                    proof.admitted = true;
                }
            }
            Response::Ready if self.netplay.phase == Phase::Admission => {
                self.netplay.admitted = true;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(proof) = &mut self.netplay.proof {
                    proof.admitted = true;
                }
                self.netplay.phase = Phase::Running;
                self.debug_windows.netplay.connected = true;
                self.netplay.next_frame = None;
                self.debug_windows.netplay.invitation.clear();
                self.debug_windows.netplay.status = "Connected".into();
                if let Some(audio) = &mut self.audio {
                    audio.discard_queued_samples();
                }
                self.recompute_pause();
            }
            Response::Frame {
                checkpoint: checkpoint @ Message::Checkpoint { frame, .. },
                audio,
                ports,
            } if matches!(self.netplay.phase, Phase::Running | Phase::Stopping)
                && self.netplay.confirmed.checked_add(1) == Some(frame) =>
            {
                self.netplay.confirmed = frame;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(proof) = &mut self.netplay.proof
                    && proof.record_frame(&checkpoint, ports, &audio).is_err()
                {
                    self.request_netplay_stop();
                    return None;
                }
                #[cfg(all(test, not(target_arch = "wasm32")))]
                self.netplay
                    .observed_frames
                    .push((checkpoint, ports, audio.clone()));
                #[cfg(any(not(test), target_arch = "wasm32"))]
                let _ = (checkpoint, ports);
                if self.netplay.running() {
                    self.queue_netplay_audio(&audio);
                }
            }
            Response::Audio { frame, audio }
                if matches!(self.netplay.phase, Phase::Running | Phase::Stopping)
                    && self.netplay.confirmed.checked_add(1) == Some(frame) =>
            {
                self.netplay.confirmed = frame;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(proof) = &mut self.netplay.proof
                    && proof.record_audio(&audio).is_err()
                {
                    self.request_netplay_stop();
                    return None;
                }
                if self.netplay.running() {
                    self.queue_netplay_audio(&audio);
                }
            }
            Response::Presented {
                frame,
                confirmed,
                step_complete,
                prediction_depth,
                rollback_frames,
                retained_bytes,
                ..
            } if matches!(self.netplay.phase, Phase::Running | Phase::Stopping)
                && frame >= self.netplay.presented
                && frame <= self.netplay.presented.saturating_add(1)
                && (!step_complete || self.netplay.in_flight) =>
            {
                let advanced = frame > self.netplay.presented;
                self.netplay.presented = frame;
                self.netplay.published_confirmed = confirmed;
                self.netplay.rollback_frames =
                    self.netplay.rollback_frames.saturating_add(rollback_frames);
                if step_complete {
                    self.netplay.in_flight = false;
                }
                self.debug_windows.netplay.metrics = format!(
                    "Frame {frame} · predicted {prediction_depth}\nReplayed {rollback_frames} · state {:.1} MiB",
                    retained_bytes as f64 / (1024.0 * 1024.0)
                );
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(proof) = &mut self.netplay.proof {
                    proof.depth_max = proof.depth_max.max(prediction_depth);
                    proof.rollback_frames += rollback_frames;
                    proof.retained_payload_max = proof.retained_payload_max.max(retained_bytes);
                    if advanced {
                        proof.presented.push((frame, Instant::now()));
                    } else if step_complete && !self.netplay.paused {
                        proof.stalls += 1;
                    }
                }
                if self.netplay.running() {
                    self.latest_frame = self
                        .emu_thread
                        .as_ref()
                        .and_then(|thread| thread.shared_framebuffer().load_full());
                    if advanced {
                        self.fps_tracker.tick_n(1);
                    }
                }
            }
            Response::Paused { frame, local, peer }
                if matches!(self.netplay.phase, Phase::Running | Phase::Stopping)
                    && frame <= self.netplay.presented.saturating_add(1) =>
            {
                self.netplay.paused = local || peer;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(proof) = &mut self.netplay.proof {
                    proof.pause_rounds += 1;
                    proof.last_pause = Some((frame, local, peer));
                }
                if self.netplay.running() {
                    self.debug_windows.netplay.status = match (local, peer) {
                        (true, true) => "Paused by both players",
                        (true, false) => "Paused by you",
                        (false, true) => "Paused by the other player",
                        (false, false) => "Connected",
                    }
                    .into();
                    if self.netplay.paused
                        && let Some(audio) = &mut self.audio
                    {
                        audio.discard_queued_samples();
                    }
                    self.netplay.audio_started = false;
                    self.netplay.audio_prebuffer.clear();
                    self.netplay.audio_prebuffer_frames = 0;
                    self.netplay.next_frame = None;
                    self.recompute_pause();
                }
            }
            Response::Stopped { reason, restored } => self.finish_netplay(reason, restored),
            Response::Rejected(reason)
                if !self.netplay.admitted
                    && matches!(self.netplay.phase, Phase::Admission | Phase::Stopping) =>
            {
                self.finish_netplay(reason, true)
            }
            Response::Rejected(reason) => {
                self.toast_manager.error(reason);
                self.request_netplay_stop();
            }
            _ => {
                self.toast_manager.error("Unexpected netplay response");
                self.request_netplay_stop();
            }
        }
        None
    }

    fn queue_netplay_audio(&mut self, audio: &[f32]) {
        if !self.netplay.audio_started {
            self.netplay.audio_prebuffer.extend_from_slice(audio);
            self.netplay.audio_prebuffer_frames += 1;
            if self.netplay.audio_prebuffer_frames < 3 {
                return;
            }
            self.netplay.audio_started = true;
            let buffered = std::mem::take(&mut self.netplay.audio_prebuffer);
            self.queue_emulator_audio(&buffered, 1);
        } else {
            self.queue_emulator_audio(audio, 1);
        }
    }
}
