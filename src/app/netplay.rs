use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use zeff_netplay::wire::Message;

use super::App;
use crate::emu_thread::{EmuCommand, EmuResponse};
use crate::netplay::{
    Response,
    connect::{Connector, HostOptions},
};
use zeff_netplay::endpoint::ConnectionScope;

mod window;

#[derive(PartialEq, Eq)]
enum Request {
    Host(HostOptions, bool),
    Join {
        invitation: String,
        scope: ConnectionScope,
        allow_different_versions: bool,
    },
}

#[derive(Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    Preparing(Request),
    Connecting,
    Admission,
    Running,
    Stopping,
    Poisoned,
}

#[derive(Default)]
pub(super) struct Frontend {
    phase: Phase,
    connector: Option<Connector>,
    in_flight: bool,
    confirmed: u64,
    presented: u64,
    published_confirmed: u64,
    audio_started: bool,
    audio_prebuffer: Vec<f32>,
    audio_prebuffer_frames: usize,
    rollback_frames: u64,
    admitted: bool,
    paused: bool,
    local_pause: bool,
    next_frame: Option<Instant>,
    proof: Option<proof::Observation>,
    #[cfg(test)]
    observed_frames: Vec<(Message, [u8; 2], Vec<f32>)>,
    #[cfg(test)]
    pub(in crate::app) queued_audio: Option<(Vec<f32>, usize)>,
    #[cfg(test)]
    observed_connection: Option<(std::net::SocketAddr, std::net::SocketAddr, ConnectionScope)>,
}

impl Frontend {
    pub(super) fn fenced(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub(super) fn running(&self) -> bool {
        self.phase == Phase::Running
    }

    pub(super) fn playing(&self) -> bool {
        self.running() && !self.paused
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.next_frame
            .filter(|_| self.running() && !self.in_flight)
    }

    pub(super) fn permits(&self, command: &EmuCommand) -> bool {
        !self.fenced()
            || matches!(command, EmuCommand::Shutdown)
            || matches!(
                (command, &self.phase),
                (EmuCommand::StartNetplay(_), Phase::Connecting)
                    | (EmuCommand::StepNetplay(_), Phase::Running)
                    | (EmuCommand::SetNetplayPaused(_), Phase::Running)
                    | (EmuCommand::SendNetplayChat(_), Phase::Running)
                    | (
                        EmuCommand::StopNetplay,
                        Phase::Admission | Phase::Running | Phase::Stopping
                    )
            )
    }
}

impl App {
    pub(in crate::app) fn begin_netplay(&mut self, invitation: Option<String>) -> Result<()> {
        let request = if let Some(invitation) = invitation {
            let scope = self.debug_windows.netplay.scope();
            crate::netplay::connect::validate_invitation(&invitation, scope)?;
            self.debug_windows.netplay.input_delay = self
                .debug_windows
                .netplay
                .invitation_delay(&invitation)?
                .frames();
            Request::Join {
                invitation,
                scope,
                allow_different_versions: self.debug_windows.netplay.allow_different_versions,
            }
        } else {
            Request::Host(
                self.debug_windows.netplay.host_options()?,
                self.debug_windows.netplay.allow_different_versions,
            )
        };
        ensure!(
            self.worker_gameplay_commands_allowed(),
            "another execution owner is active"
        );
        ensure!(
            self.active_system == crate::emu_backend::ActiveSystem::Nes
                && self.emu_thread.is_some(),
            "load an NES cartridge first"
        );
        ensure!(
            !self.recording.is_replay_active() && self.recording.audio_recorder.is_none(),
            "stop recording or replay before netplay"
        );
        ensure!(
            !self.tcp_link_active && !self.live_control.is_enabled(),
            "disconnect link and live control before netplay"
        );
        ensure!(
            self.pending_rom_preparation.is_none() && self.pending_archive_selection.is_none(),
            "finish loading media before netplay"
        );
        ensure!(
            !self.show_settings_window
                && !self.show_mods_window
                && !self.show_cheats_window
                && !self.show_audio_explorer,
            "close configuration windows before netplay"
        );
        self.sync_effective_input();
        ensure!(
            !self
                .input_configuration
                .autofire
                .iter()
                .flatten()
                .any(Option::is_some),
            "disable autofire before netplay"
        );
        ensure!(
            crate::cheats::collect_enabled_patches(
                &self.debug_windows.cheat.user_codes,
                &self.debug_windows.cheat.libretro_codes
            )
            .is_empty(),
            "disable cheats before netplay"
        );
        ensure!(
            self.settings.audio.output_sample_rate == 48_000
                && self.last_audio_output_sample_rate == 48_000,
            "set audio output to 48000 Hz before netplay"
        );
        ensure!(
            !self.settings.emulation.uncapped_speed && !self.settings.emulation.slow_motion_enabled,
            "select normal speed before netplay"
        );
        ensure!(
            self.audio
                .as_ref()
                .is_none_or(|audio| audio.emulator_sample_rate() == 48_000),
            "netplay requires a 48000 Hz audio source"
        );
        self.set_user_paused(true);
        if let Some(audio) = &mut self.audio {
            audio.discard_queued_samples();
        }
        self.clear_all_frontend_holds();
        self.debug_requests = Default::default();
        self.pending_debug_actions = crate::debug::DebugUiActions::none();
        self.netplay.phase = Phase::Preparing(request);
        self.debug_windows.netplay.clear_chat();
        self.debug_windows.netplay.allow_different_versions = false;
        self.debug_windows.netplay.active = true;
        self.debug_windows.netplay.status = "Preparing fresh cartridge load…".into();
        self.recompute_pause();
        Ok(())
    }

    pub(in crate::app) fn pump_netplay(&mut self) {
        if matches!(self.netplay.phase, Phase::Preparing(_)) && self.frames_in_flight == 0 {
            let Phase::Preparing(request) = std::mem::take(&mut self.netplay.phase) else {
                unreachable!()
            };
            let consent = match &request {
                Request::Host(_, allow)
                | Request::Join {
                    allow_different_versions: allow,
                    ..
                } => *allow,
            };
            let result = self.prepare_netplay_game().and_then(|()| match request {
                Request::Join {
                    invitation, scope, ..
                } => {
                    Connector::join(&invitation, scope).map(|connector| (connector, String::new()))
                }
                Request::Host(options, _) => Connector::host(options),
            });
            match result {
                Ok((mut connector, invitation)) => {
                    connector.set_version_consent(consent);
                    self.netplay.connector = Some(connector);
                    self.netplay.phase = Phase::Connecting;
                    self.pause_state.set_user_paused(true);
                    self.recompute_pause();
                    self.debug_windows.netplay.active = true;
                    self.debug_windows.netplay.invitation = invitation;
                    self.debug_windows.netplay.status = "Waiting for the other player…".into();
                }
                Err(error) => self.finish_netplay(error.to_string(), true),
            }
        }
        if self.netplay.phase == Phase::Connecting {
            let result = self.netplay.connector.as_mut().unwrap().poll();
            match result {
                Ok(Some(mut start)) => {
                    self.debug_windows.netplay.input_delay = start.input_delay.frames();
                    start.verify_every_frame = self
                        .netplay
                        .proof
                        .as_ref()
                        .is_some_and(|proof| !proof.cadence)
                        || cfg!(test);
                    if let Some(proof) = &mut self.netplay.proof {
                        proof.connection = start
                            .stream
                            .local_addr()
                            .ok()
                            .zip(start.stream.peer_addr().ok())
                            .map(|(local, peer)| (local, peer, start.scope));
                    }
                    #[cfg(test)]
                    {
                        self.netplay.observed_connection = Some((
                            start.stream.local_addr().unwrap(),
                            start.stream.peer_addr().unwrap(),
                            start.scope,
                        ));
                    }
                    let sent =
                        self.send_emu_command_checked(EmuCommand::StartNetplay(Box::new(start)));
                    self.netplay.connector = None;
                    match sent {
                        Ok(()) => {
                            self.netplay.phase = Phase::Admission;
                            self.debug_windows.netplay.status =
                                "Checking cartridge, save and build identity…".into();
                        }
                        Err(error) => self.finish_netplay(error.to_string(), false),
                    }
                }
                Ok(None) => {}
                Err(error) => self.finish_netplay(error.to_string(), true),
            }
        }
        let now = Instant::now();
        if self.netplay.running()
            && !self.netplay.in_flight
            && self
                .netplay
                .next_frame
                .is_none_or(|deadline| now >= deadline)
        {
            // about_to_wait can submit before the next render's controller poll.
            self.poll_gamepad();
            let (buttons, dpad) = if self.game_window_focused
                && self.game_view_focused
                && !self.egui_wants_keyboard
            {
                self.current_host_joypad_input()
            } else {
                (0, 0)
            };
            let raw = host_to_nes(buttons, dpad);
            match self.send_emu_command_checked(EmuCommand::StepNetplay(raw)) {
                Ok(()) => {
                    self.netplay.in_flight = true;
                    let interval = if self.netplay.paused {
                        Duration::from_millis(100)
                    } else {
                        Duration::from_nanos(self.nominal_frame_duration_ns())
                    };
                    let deadline = self.netplay.next_frame.unwrap_or(now) + interval;
                    self.netplay.next_frame = Some(deadline.max(now));
                }
                Err(error) => self.finish_netplay(error.to_string(), false),
            }
        }
    }

    pub(in crate::app) fn request_netplay_stop(&mut self) {
        if matches!(self.netplay.phase, Phase::Preparing(_) | Phase::Connecting) {
            self.finish_netplay("Connection canceled".into(), true);
        } else if matches!(self.netplay.phase, Phase::Admission | Phase::Running) {
            match self.send_emu_command_checked(EmuCommand::StopNetplay) {
                Ok(()) => {
                    self.netplay.phase = Phase::Stopping;
                    self.debug_windows.netplay.status =
                        "Disconnecting and restoring the cartridge…".into();
                }
                Err(error) => self.finish_netplay(error.to_string(), false),
            }
        }
    }

    pub(in crate::app) fn set_netplay_paused(&mut self, paused: bool) {
        if !self.netplay.running() || paused == self.netplay.local_pause {
            return;
        }
        match self.send_emu_command_checked(EmuCommand::SetNetplayPaused(paused)) {
            Ok(()) => {
                self.netplay.local_pause = paused;
                self.debug_windows.netplay.local_pause = paused;
                self.debug_windows.netplay.status = if paused {
                    "Requesting pause at a confirmed frame boundary…"
                } else {
                    "Ready to resume · waiting for the other player…"
                }
                .into();
                if !self.netplay.in_flight {
                    self.netplay.next_frame = None;
                }
            }
            Err(error) => self.finish_netplay(error.to_string(), false),
        }
    }

    fn finish_netplay(&mut self, reason: String, restored: bool) {
        self.netplay = Frontend {
            proof: self.netplay.proof.take(),
            phase: if restored {
                Phase::Idle
            } else {
                Phase::Poisoned
            },
            ..Frontend::default()
        };
        self.debug_windows.netplay.active = !restored;
        self.debug_windows.netplay.connected = false;
        self.debug_windows.netplay.chat_failed(String::new());
        self.debug_windows.netplay.local_pause = false;
        self.debug_windows.netplay.metrics.clear();
        self.clear_all_frontend_holds();
        self.debug_windows.netplay.invitation.clear();
        self.debug_windows.netplay.status = if restored {
            reason
        } else {
            format!("{reason}. Stop the game before continuing.")
        };
        self.pause_state.set_user_paused(true);
        self.recompute_pause();
        self.frames_in_flight = 0;
        self.latest_frame = self
            .emu_thread
            .as_ref()
            .and_then(|thread| thread.shared_framebuffer().load_full());
        self.cached_ui_data = None;
        self.last_core_frame = None;
        self.last_displayed_frame = None;
        self.debug_requests = Default::default();
        self.pending_debug_actions = crate::debug::DebugUiActions::none();
        if let Some(audio) = &mut self.audio {
            audio.discard_queued_samples();
        }
        self.timing.last_frame_time = crate::platform::Instant::now();
    }

    pub(in crate::app) fn netplay_worker_lost(&mut self) {
        if self.netplay.fenced() {
            self.finish_netplay("Emulator worker disconnected".into(), false);
        }
    }

    pub(in crate::app) fn retire_netplay_frontend(&mut self) {
        if self.netplay.fenced() {
            self.netplay = Frontend::default();
            self.debug_windows.netplay.active = false;
            self.debug_windows.netplay.connected = false;
            self.debug_windows.netplay.chat_failed(String::new());
            self.debug_windows.netplay.local_pause = false;
            self.debug_windows.netplay.invitation.clear();
            self.debug_windows.netplay.status = "Disconnected".into();
            if let Some(audio) = &mut self.audio {
                audio.discard_queued_samples();
            }
        }
    }
}

fn host_to_nes(buttons: u8, dpad: u8) -> u8 {
    (buttons & 0x0f) | ((dpad & 4) << 2) | ((dpad & 8) << 2) | ((dpad & 2) << 5) | ((dpad & 1) << 7)
}

pub(super) mod proof;
mod responses;

#[cfg(test)]
mod chat_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod reference_tests;

#[cfg(test)]
mod rollback_tests;

#[cfg(test)]
mod pause_tests;

#[cfg(test)]
mod private_tests;

#[cfg(test)]
mod lan_tests;

#[cfg(test)]
mod compatibility_tests;
