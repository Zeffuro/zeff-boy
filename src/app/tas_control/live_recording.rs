use anyhow::{Result, bail};

use crate::emu_thread::{TasFrameAdvanceRequest, TasInputFrame};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasPreparedLiveFrame};

use super::lifecycle::{ResponseDisposition, WorkerBoundCommand};
use super::project_binding::TasEditorControlSnapshot;
use super::{TasControlCoordinator, TasControlState};

pub(in crate::app) const fn profile_supports_live_input_recording(
    profile: crate::emu_thread::TasExecutionProfile,
) -> bool {
    matches!(
        profile,
        crate::emu_thread::TasExecutionProfile::DirectNesCartridge
            | crate::emu_thread::TasExecutionProfile::DirectFdsDisk
            | crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg
            | crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb
            | crate::emu_thread::TasExecutionProfile::DirectColecoCartridge
            | crate::emu_thread::TasExecutionProfile::DirectSmsCartridge
            | crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge
            | crate::emu_thread::TasExecutionProfile::DirectGbaCartridge
            | crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge
            | crate::emu_thread::TasExecutionProfile::DirectWsCartridge
            | crate::emu_thread::TasExecutionProfile::DirectPceHuCard
            | crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard
            | crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard
            | crate::emu_thread::TasExecutionProfile::DirectPceCd
            | crate::emu_thread::TasExecutionProfile::DirectPceMultitapCd
    )
}

impl TasControlCoordinator {
    pub(in crate::app) fn can_record_live_input(&self) -> bool {
        !self.playback_active
            && matches!(
                self.state,
                TasControlState::AwaitingDecision {
                    project: TasEditorControlSnapshot { profile, .. },
                    ..
                } if super::profile_supports_live_input_recording(profile)
            )
    }

    pub(in crate::app) fn live_frame_in_flight(&self) -> bool {
        matches!(
            self.state,
            TasControlState::FrameAdvancePending { .. }
                | TasControlState::FrameRecordCommitPending { .. }
        )
    }

    #[allow(dead_code)]
    pub(in crate::app) fn start_realtime_recording(&mut self) -> Result<()> {
        let TasControlState::AwaitingDecision { project, .. } = &self.state else {
            bail!("no completed TAS execution is awaiting a realtime recording frame");
        };
        if self.playback_active {
            bail!("pause TAS movie playback before recording");
        }
        if !super::profile_supports_live_input_recording(project.profile) {
            bail!("live host-input recording is unavailable for this TAS profile");
        }
        self.start_mode = super::TasControlStartMode::Preview;
        self.realtime_recording_active = true;
        Ok(())
    }

    pub(in crate::app) fn take_realtime_recording_start_request(&mut self) -> bool {
        if self.start_mode == super::TasControlStartMode::Record && self.can_record_live_input() {
            self.start_mode = super::TasControlStartMode::Preview;
            true
        } else {
            false
        }
    }

    #[allow(dead_code)]
    pub(in crate::app) fn stop_realtime_recording(&mut self) {
        self.start_mode = super::TasControlStartMode::Preview;
        self.realtime_recording_active = false;
    }

    #[allow(dead_code)]
    pub(in crate::app) fn realtime_recording_active(&self) -> bool {
        self.realtime_recording_active
    }

    pub(in crate::app) fn begin_live_frame_advance(
        &mut self,
        prepared: TasPreparedLiveFrame,
    ) -> Result<WorkerBoundCommand> {
        let TasControlState::AwaitingDecision {
            worker_generation,
            lease_id,
            run_id,
            next_advance_id,
            proof,
            project,
            candidate_segment_id,
            candidate_segment_frame_count,
            candidate_executed_project_frames,
            candidate_frame_count,
            candidate_state_sha256,
        } = &self.state
        else {
            bail!("no completed TAS execution is awaiting a live input frame");
        };
        if !super::profile_supports_live_input_recording(project.profile) {
            bail!("live host-input recording is unavailable for this TAS profile");
        }
        if prepared.cursor() != *candidate_executed_project_frames {
            bail!("the selected TAS boundary does not match the linked game position");
        }
        let advance_id = *next_advance_id;
        let next_advance_id = advance_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS live-frame advance IDs are exhausted"))?;
        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let run_id = *run_id;
        let proof = proof.clone();
        let project = project.clone();
        let profile = project.profile;
        let candidate_segment_id = *candidate_segment_id;
        let candidate_segment_frame_count = *candidate_segment_frame_count;
        let candidate_executed_project_frames = *candidate_executed_project_frames;
        let candidate_frame_count = *candidate_frame_count;
        let candidate_state_sha256 = *candidate_state_sha256;
        let starts_next_segment = candidate_segment_frame_count == MAX_EDITOR_SEEK_EXECUTION_FRAMES;
        let segment_id = if starts_next_segment {
            candidate_segment_id
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS live-frame segment IDs are exhausted"))?
        } else {
            candidate_segment_id
        };
        let expected_segment_frame_count = if starts_next_segment {
            1
        } else {
            candidate_segment_frame_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS live-frame segment length overflows"))?
        };
        let expected_executed_project_frames = candidate_executed_project_frames
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TAS live-frame project position overflows"))?;
        let input = input_from_prepared(profile, &prepared)?;
        self.pending_live_frame = Some(prepared);
        self.state = TasControlState::FrameAdvancePending {
            worker_generation,
            lease_id,
            run_id,
            advance_id,
            next_advance_id,
            segment_id,
            expected_segment_frame_count,
            expected_executed_project_frames,
            proof,
            project,
        };
        Ok(WorkerBoundCommand::advance_frame(
            worker_generation,
            TasFrameAdvanceRequest {
                profile,
                lease_id,
                run_id,
                advance_id,
                segment_id,
                expected_segment_frame_count: candidate_segment_frame_count,
                expected_executed_project_frames: candidate_executed_project_frames,
                expected_frame_count: candidate_frame_count,
                expected_state_sha256: candidate_state_sha256,
                input,
                snapshot: None,
            },
        ))
    }

    pub(in crate::app::tas_control) fn finish_live_frame_commit(
        &mut self,
        committed: Result<TasEditorControlSnapshot>,
    ) -> ResponseDisposition {
        let TasControlState::FrameRecordCommitPending {
            worker_generation,
            lease_id,
            run_id,
            advance_id: _,
            next_advance_id,
            candidate_segment_id,
            candidate_segment_frame_count,
            candidate_executed_project_frames,
            proof,
            candidate_frame_count,
            candidate_state_sha256,
        } = &self.state
        else {
            return Self::stale_response();
        };
        let worker_generation = *worker_generation;
        let lease_id = *lease_id;
        let run_id = *run_id;
        let proof = proof.clone();
        let next_advance_id = *next_advance_id;
        let candidate_segment_id = *candidate_segment_id;
        let candidate_segment_frame_count = *candidate_segment_frame_count;
        let candidate_executed_project_frames = *candidate_executed_project_frames;
        let candidate_frame_count = *candidate_frame_count;
        let candidate_state_sha256 = *candidate_state_sha256;
        match committed {
            Ok(project) => {
                self.state = TasControlState::AwaitingDecision {
                    worker_generation,
                    lease_id,
                    run_id,
                    next_advance_id,
                    proof,
                    project,
                    candidate_segment_id,
                    candidate_segment_frame_count,
                    candidate_executed_project_frames,
                    candidate_frame_count,
                    candidate_state_sha256,
                };
                self.framebuffer_refresh_pending = true;
                ResponseDisposition::Consumed { follow_up: None }
            }
            Err(error) => {
                self.stop_realtime_recording();
                self.pending_error = Some(format!(
                    "Could not record the live TAS input; the loaded game will be restored: {error:#}"
                ));
                self.state = TasControlState::RollbackPending {
                    worker_generation,
                    lease_id,
                    checkpoint_sha256: proof.current_state_sha256,
                    checkpoint_frame_count: proof.frame_count,
                };
                ResponseDisposition::Consumed {
                    follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
                }
            }
        }
    }
}

fn input_from_prepared(
    profile: crate::emu_thread::TasExecutionProfile,
    prepared: &TasPreparedLiveFrame,
) -> Result<TasInputFrame> {
    let input = prepared.input();
    let p1 = TasInputFrame {
        p1_buttons: input.players[0].buttons,
        p1_dpad: input.players[0].dpad,
        ..TasInputFrame::default()
    };
    match profile {
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg
        | crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb => {
            if p1.p1_buttons & !0x0F != 0
                || p1.p1_dpad & !0x0F != 0
                || input.players[1..]
                    .iter()
                    .any(|player| player.buttons != 0 || player.dpad != 0)
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
            {
                bail!("live input is outside the direct Game Boy profile");
            }
            Ok(TasInputFrame {
                tilt_x_bits: input.tilt_x_bits,
                tilt_y_bits: input.tilt_y_bits,
                ..p1
            })
        }
        crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge => {
            if p1.p1_buttons & !0x0B != 0
                || p1.p1_dpad & !0x0F != 0
                || input.players[1..]
                    .iter()
                    .any(|player| player.buttons != 0 || player.dpad != 0)
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
            {
                bail!("live input is outside the direct Game Gear profile");
            }
            Ok(p1)
        }
        crate::emu_thread::TasExecutionProfile::DirectGbaCartridge => {
            if p1.p1_buttons & !0x3F != 0
                || p1.p1_dpad & !0x0F != 0
                || input.players[1..]
                    .iter()
                    .any(|player| player.buttons != 0 || player.dpad != 0)
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
                || !matches!(input.camera, crate::tas_project::TasCameraInput::None)
            {
                bail!("live input is outside the direct GBA profile");
            }
            Ok(TasInputFrame {
                tilt_x_bits: input.tilt_x_bits,
                tilt_y_bits: input.tilt_y_bits,
                ..p1
            })
        }
        crate::emu_thread::TasExecutionProfile::DirectNesCartridge
        | crate::emu_thread::TasExecutionProfile::DirectFdsDisk => {
            if input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || (profile == crate::emu_thread::TasExecutionProfile::DirectFdsDisk
                    && input.zapper != Default::default())
            {
                bail!("live input is outside the direct NES profile");
            }
            Ok(TasInputFrame {
                p1_buttons: p1.p1_buttons,
                p1_dpad: p1.p1_dpad,
                p2_buttons: input.players[1].buttons,
                p2_dpad: input.players[1].dpad,
                coleco: [crate::tas_project::TasColecoControllerInput::default(); 2],
                zapper: zeff_emu_common::replay::ReplayZapperFrame {
                    enabled: input.zapper.enabled,
                    trigger: input.zapper.trigger,
                    hit: input.zapper.hit,
                    screen_pos: input.zapper.screen_pos.map(|[x, y]| (x, y)),
                },
                tilt_x_bits: 0,
                tilt_y_bits: 0,
                fds_disk_side: None,
                fds_write_protected: None,
                fds_media_event: None,
                ..TasInputFrame::default()
            })
        }
        crate::emu_thread::TasExecutionProfile::DirectColecoCartridge => {
            if input
                .players
                .iter()
                .any(|player| *player != Default::default())
                || input.zapper != Default::default()
                || input.tilt_x_bits != 0
                || input.tilt_y_bits != 0
                || !matches!(input.camera, crate::tas_project::TasCameraInput::None)
            {
                bail!("live input is outside the direct ColecoVision profile");
            }
            Ok(TasInputFrame {
                coleco: input.coleco,
                ..TasInputFrame::default()
            })
        }
        crate::emu_thread::TasExecutionProfile::DirectSmsCartridge
        | crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge => {
            if input.players[0].buttons & !0x03 != 0
                || input.players[0].dpad & !0x0F != 0
                || input.players[1].buttons & !0x03 != 0
                || input.players[1].dpad & !0x0F != 0
                || input.players[2..]
                    .iter()
                    .any(|player| *player != Default::default())
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
                || input.tilt_x_bits != 0
                || input.tilt_y_bits != 0
                || !matches!(input.camera, crate::tas_project::TasCameraInput::None)
            {
                bail!("live input is outside the direct Sega 8-bit TAS profile");
            }
            Ok(TasInputFrame {
                p1_buttons: input.players[0].buttons,
                p1_dpad: input.players[0].dpad,
                p2_buttons: input.players[1].buttons,
                p2_dpad: input.players[1].dpad,
                ..TasInputFrame::default()
            })
        }
        crate::emu_thread::TasExecutionProfile::DirectWsCartridge => {
            if input.players[0].buttons & !0xFB != 0
                || input.players[0].dpad & !0x0F != 0
                || input.players[1..]
                    .iter()
                    .any(|player| *player != Default::default())
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
                || input.tilt_x_bits != 0
                || input.tilt_y_bits != 0
                || !matches!(input.camera, crate::tas_project::TasCameraInput::None)
            {
                bail!("live input is outside the direct WonderSwan TAS profile");
            }
            Ok(TasInputFrame {
                p1_buttons: input.players[0].buttons,
                p1_dpad: input.players[0].dpad,
                ..TasInputFrame::default()
            })
        }
        crate::emu_thread::TasExecutionProfile::DirectPceHuCard
        | crate::emu_thread::TasExecutionProfile::DirectPceCd => {
            if input.players[0].buttons & !0x0F != 0
                || input.players[0].dpad & !0x0F != 0
                || input.players[1..]
                    .iter()
                    .any(|player| *player != Default::default())
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
                || input.tilt_x_bits != 0
                || input.tilt_y_bits != 0
                || !matches!(input.camera, crate::tas_project::TasCameraInput::None)
            {
                bail!("live input is outside the direct PC Engine TAS profile");
            }
            Ok(p1)
        }
        crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard => {
            if input.players[0].dpad & !0x0F != 0
                || input.players[1..]
                    .iter()
                    .any(|player| *player != Default::default())
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
                || input.tilt_x_bits != 0
                || input.tilt_y_bits != 0
                || !matches!(input.camera, crate::tas_project::TasCameraInput::None)
            {
                bail!("live input is outside the direct PC Engine six-button TAS profile");
            }
            Ok(p1)
        }
        crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard
        | crate::emu_thread::TasExecutionProfile::DirectPceMultitapCd => {
            if input
                .players
                .iter()
                .any(|player| player.buttons & !0x0F != 0 || player.dpad & !0x0F != 0)
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default()
                || input.tilt_x_bits != 0
                || input.tilt_y_bits != 0
                || !matches!(input.camera, crate::tas_project::TasCameraInput::None)
            {
                bail!("live input is outside the direct PC Engine multitap TAS profile");
            }
            Ok(TasInputFrame {
                p1_buttons: input.players[0].buttons,
                p1_dpad: input.players[0].dpad,
                p2_buttons: input.players[1].buttons,
                p2_dpad: input.players[1].dpad,
                p3_buttons: input.players[2].buttons,
                p3_dpad: input.players[2].dpad,
                p4_buttons: input.players[3].buttons,
                p4_dpad: input.players[3].dpad,
                p5_buttons: input.players[4].buttons,
                p5_dpad: input.players[4].dpad,
                ..TasInputFrame::default()
            })
        }
    }
}
