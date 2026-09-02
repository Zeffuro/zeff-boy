use serde_json::{Value, json};

use super::App;

impl App {
    pub(super) fn live_create_tas_project(
        &mut self,
        path: std::path::PathBuf,
        replace_existing: bool,
    ) -> anyhow::Result<Value> {
        if !self.worker_gameplay_commands_allowed() {
            anyhow::bail!("finish the current live TAS action before creating another project");
        }
        let created_path = path.display().to_string();
        self.create_tas_project_for_live_control(path, replace_existing)?;
        Ok(json!({
            "created_path": created_path,
            "replaced_existing": replace_existing,
            "tas": self.live_tas_status_json(),
        }))
    }

    pub(super) fn live_open_tas_project(
        &mut self,
        path: std::path::PathBuf,
    ) -> anyhow::Result<Value> {
        if !self.worker_gameplay_commands_allowed() {
            anyhow::bail!("finish the current live TAS action before opening another project");
        }
        self.debug_windows.tas_editor.open_project(path)?;
        self.reevaluate_tas_execution_attachment();
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_link_tas(&mut self, at_end: bool, record: bool) -> anyhow::Result<Value> {
        if at_end {
            self.debug_windows
                .tas_editor
                .select_end_cursor_for_live_control()?;
        }
        if record {
            self.begin_tas_control_recording_acquire()?;
        } else {
            self.begin_tas_control_acquire()?;
        }
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_select_tas_boundary(&mut self, boundary: u64) -> anyhow::Result<Value> {
        self.debug_windows
            .tas_editor
            .select_cursor_for_live_control(boundary)?;
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_select_tas_range(&mut self, start: u64, end: u64) -> anyhow::Result<Value> {
        self.debug_windows
            .tas_editor
            .select_input_range_for_live_control(start, end)?;
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_delete_selected_tas_frames(&mut self) -> anyhow::Result<Value> {
        self.debug_windows
            .tas_editor
            .delete_selected_input_range_for_live_control()?;
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_insert_neutral_tas_frames(
        &mut self,
        boundary: u64,
        count: u64,
    ) -> anyhow::Result<Value> {
        self.debug_windows
            .tas_editor
            .insert_neutral_frames_for_live_control(boundary, count)?;
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_set_tas_digital_input(
        &mut self,
        frame: u64,
        player: u8,
        input: crate::live_control::TasDigitalInput,
        pressed: bool,
    ) -> anyhow::Result<Value> {
        self.debug_windows
            .tas_editor
            .set_digital_input_for_live_control(frame, player, input, pressed)?;
        if let Some(request) = self.debug_windows.tas_editor.take_pending_host_request() {
            match request {
                crate::debug::TasEditorHostRequest::Live(
                    crate::debug::TasEditorLiveAction::ReconstructAfterEdit { start, end },
                ) => self.reconstruct_linked_tas_after_edit(start, end)?,
                request => self.handle_tas_editor_host_request(request),
            }
        }
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_go_to_tas_selection(&mut self) -> anyhow::Result<Value> {
        self.seek_linked_tas_to_editor_cursor()?;
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_fork_tas_branch(
        &mut self,
        id: String,
        name: Option<String>,
    ) -> anyhow::Result<Value> {
        let selected_boundary = self
            .debug_windows
            .tas_editor
            .active_session()
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))?
            .cursor();
        let linked_boundary =
            require_selected_linked_boundary(selected_boundary, self.tas_control.linked_cursor())?;
        let name = name.unwrap_or_else(|| id.clone());
        self.debug_windows
            .tas_editor
            .fork_branch_at_linked_boundary_for_live_control(linked_boundary, id, name)?;
        self.reconcile_tas_control_project_binding();
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_set_tas_realtime_recording(
        &mut self,
        active: bool,
    ) -> anyhow::Result<Value> {
        if active {
            self.start_realtime_tas_recording()?;
        } else {
            self.stop_realtime_tas_recording();
        }
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_set_tas_playback(&mut self, active: bool) -> anyhow::Result<Value> {
        if active {
            self.start_linked_tas_playback()?;
        } else {
            self.pause_linked_tas_playback();
        }
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_reload_tas_game(&mut self) -> anyhow::Result<Value> {
        if !self.worker_gameplay_commands_allowed() {
            anyhow::bail!("finish the current live TAS action before reloading the game");
        }
        if self.emu_thread.is_none() || self.rom_info.source_path.is_none() {
            anyhow::bail!("no source-loaded game is available to reload");
        }
        self.repair_loaded_game_and_connect_tas()?;
        Ok(json!({
            "repair_activated": true,
            "tas": self.live_tas_status_json(),
        }))
    }

    pub(super) fn live_disconnect_tas(&mut self, keep: bool) -> anyhow::Result<Value> {
        anyhow::ensure!(
            matches!(
                self.tas_control_live_status(),
                crate::debug::TasEditorLiveStatus::Linked { .. }
            ),
            "pause TAS playback or recording before disconnecting"
        );
        if keep {
            self.commit_tas_control()?;
        } else {
            self.cancel_tas_control();
        }
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_record_tas_frame(
        &mut self,
        mode: crate::live_control::TasRecordMode,
    ) -> anyhow::Result<Value> {
        let mode = match mode {
            crate::live_control::TasRecordMode::Replace => {
                crate::tas_project::TasLiveRecordingMode::ReplaceExistingInput
            }
            crate::live_control::TasRecordMode::Insert => {
                crate::tas_project::TasLiveRecordingMode::InsertNewFrames
            }
        };
        self.debug_windows.tas_editor.set_live_recording_mode(mode);
        self.record_current_tas_input_and_advance()?;
        Ok(self.live_tas_status_json())
    }

    pub(super) fn live_tas_status_json(&mut self) -> Value {
        self.refresh_tas_editor_live_status();
        let selected_range = self
            .debug_windows
            .tas_editor
            .selected_input_range_for_live_control()
            .ok()
            .flatten();
        let project = self
            .debug_windows
            .tas_editor
            .active_session()
            .map(|session| {
                let frame_count = session.selected_branch().frame_count();
                let selected_boundary = session.cursor();
                json!({
                    "file_name": session.manual_path().file_name().and_then(|name| name.to_str()),
                    "selected_branch": session.selected_branch_id(),
                    "branches": tas_branches_json(
                        session.project(),
                        session.selected_branch_id(),
                    ),
                    "cursor": selected_boundary,
                    "frame_count": frame_count,
                    "timeline": tas_timeline_json(
                        selected_boundary,
                        frame_count,
                        selected_range,
                    ),
                })
            });
        let status = self.debug_windows.tas_editor.live_status();
        let realtime_recording_active = self.realtime_tas_recording_active();
        let realtime_waiting_for_game_input = self.realtime_tas_recording_waiting_for_game_input();
        json!({
            "project": project,
            "live": live_tas_status_json(status),
            "readiness": tas_readiness_report_json(self.tas_control_readiness_report())
                .unwrap_or_else(|| tas_readiness_json(status)),
            "last_failure": tas_last_failure_json(status),
            "recording": realtime_tas_recording_json(
                realtime_recording_active,
                realtime_waiting_for_game_input,
            ),
            "repair": tas_repair_state_json(self.tas_repair_state()),
            "frames_in_flight": self.frames_in_flight,
            "paused": self.speed.paused,
        })
    }
}

fn realtime_tas_recording_json(active: bool, waiting_for_game_input: bool) -> Value {
    json!({
        "realtime_active": active,
        "waiting_for_game_input": active && waiting_for_game_input,
    })
}

fn tas_repair_state_json(state: crate::app::tas_control::repair::TasRepairState) -> Value {
    match state {
        crate::app::tas_control::repair::TasRepairState::Detached => {
            json!({ "state": "detached" })
        }
        crate::app::tas_control::repair::TasRepairState::RepairedDetached {
            identity,
            original_generation,
            repaired_generation,
            original_proof,
        } => json!({
            "state": "active",
            "repair_id": identity.repair_id,
            "original_generation": original_generation,
            "repaired_generation": repaired_generation,
            "parked_frame_count": original_proof.frame_count,
            "parked_state_sha256": original_proof.state_sha256.to_hex(),
            "parked_framebuffer_sha256": original_proof.framebuffer_sha256.to_hex(),
        }),
    }
}

fn live_tas_status_json(status: &crate::debug::TasEditorLiveStatus) -> Value {
    match status {
        crate::debug::TasEditorLiveStatus::Unavailable(reason) => {
            json!({ "state": "unavailable", "reason": reason, "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::ReloadRequired(reason) => {
            json!({ "state": "reload_required", "reason": reason, "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Ready {
            recording_available,
        } => {
            json!({ "state": "ready", "recording_available": recording_available, "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Acquiring => {
            json!({ "state": "acquiring", "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Staging { completed, total } => {
            json!({ "state": "staging", "completed": completed, "total": total, "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Linked {
            cursor,
            recording_available,
        } => json!({
            "state": "linked",
            "cursor": cursor,
            "execution_boundary": cursor,
            "recording_available": recording_available,
        }),
        crate::debug::TasEditorLiveStatus::Playing {
            cursor,
            pause_pending,
        } => json!({
            "state": if *pause_pending { "pausing" } else { "playing" },
            "cursor": cursor,
            "execution_boundary": cursor,
            "pause_pending": pause_pending,
        }),
        crate::debug::TasEditorLiveStatus::AdvancingFrame => {
            json!({ "state": "advancing_frame", "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Recording => {
            json!({ "state": "recording", "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Returning => {
            json!({ "state": "returning", "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Keeping => {
            json!({ "state": "keeping", "execution_boundary": Value::Null })
        }
        crate::debug::TasEditorLiveStatus::Terminal(reason) => {
            json!({ "state": "terminal", "reason": reason, "execution_boundary": Value::Null })
        }
    }
}

fn tas_branches_json(project: &crate::tas_project::TasProject, active_branch_id: &str) -> Value {
    Value::Array(
        project
            .branches()
            .iter()
            .map(|branch| {
                let origin = branch.parent().map(|origin| {
                    json!({
                        "branch_id": origin.branch_id,
                        "fork_boundary": origin.fork_cursor,
                    })
                });
                json!({
                    "id": branch.id(),
                    "name": branch.name(),
                    "active": branch.id() == active_branch_id,
                    "frame_count": branch.frame_count(),
                    "origin": origin,
                })
            })
            .collect(),
    )
}

fn tas_timeline_json(
    selected_boundary: u64,
    end_boundary: u64,
    selected_range: Option<(u64, u64)>,
) -> Value {
    json!({
        "selected_boundary": selected_boundary,
        "selected_row": (selected_boundary < end_boundary).then_some(selected_boundary),
        "selected_range": selected_range.map(|(start, end)| json!({
            "start": start,
            "end": end,
            "length": end - start,
        })),
        "end_boundary": end_boundary,
        "next_append_row": end_boundary,
    })
}

fn require_selected_linked_boundary(
    selected_boundary: u64,
    linked_boundary: Option<u64>,
) -> anyhow::Result<u64> {
    let linked_boundary = linked_boundary
        .ok_or_else(|| anyhow::anyhow!("the loaded game is not linked to the TAS editor"))?;
    anyhow::ensure!(
        selected_boundary == linked_boundary,
        "move the linked game to the selected TAS boundary before creating a branch"
    );
    Ok(linked_boundary)
}

fn tas_readiness_json(status: &crate::debug::TasEditorLiveStatus) -> Value {
    match status {
        crate::debug::TasEditorLiveStatus::Unavailable(reason) => {
            json!({ "state": "unavailable", "reason": reason })
        }
        crate::debug::TasEditorLiveStatus::ReloadRequired(reason) => {
            json!({ "state": "reload_required", "reason": reason })
        }
        crate::debug::TasEditorLiveStatus::Ready {
            recording_available,
        } => json!({ "state": "ready", "recording_available": recording_available }),
        crate::debug::TasEditorLiveStatus::Terminal(reason) => {
            json!({ "state": "unavailable", "reason": reason })
        }
        _ => Value::Null,
    }
}

fn tas_readiness_report_json(
    report: Option<&crate::app::tas_control::readiness::TasReadinessReport>,
) -> Option<Value> {
    report.map(|report| {
        json!({
            "state": tas_readiness_status_name(report.status),
            "worker_generation": report.worker_generation,
            "profile": tas_execution_profile_name(report.profile),
            "checks": report.checks.iter().map(|check| json!({
                "id": {
                    "worker_generation": check.id.worker_generation,
                    "code": tas_readiness_code_name(check.id.code),
                    "resource": tas_readiness_resource_name(check.id.resource),
                },
                "expected": tas_readiness_value_json(&check.expected),
                "loaded": check.loaded.as_ref().map(tas_readiness_value_json),
                "configured": check.configured.as_ref().map(tas_readiness_value_json),
                "state": tas_readiness_status_name(check.status),
                "repair": tas_readiness_repair_name(check.repair),
            })).collect::<Vec<_>>(),
        })
    })
}

fn tas_execution_profile_name(profile: crate::emu_thread::TasExecutionProfile) -> &'static str {
    match profile {
        crate::emu_thread::TasExecutionProfile::DirectNesCartridge => "direct_nes_cartridge",
        crate::emu_thread::TasExecutionProfile::DirectFdsDisk => "direct_fds_disk",
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg => "direct_gb_cartridge_dmg",
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb => "direct_gb_cartridge_cgb",
        crate::emu_thread::TasExecutionProfile::DirectColecoCartridge => "direct_coleco_cartridge",
        crate::emu_thread::TasExecutionProfile::DirectSmsCartridge => "direct_sms_cartridge",
        crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge => {
            "direct_game_gear_cartridge"
        }
        crate::emu_thread::TasExecutionProfile::DirectGbaCartridge => "direct_gba_cartridge",
        crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge => "direct_sg1000_cartridge",
        crate::emu_thread::TasExecutionProfile::DirectWsCartridge => "direct_ws_cartridge",
        crate::emu_thread::TasExecutionProfile::DirectPceHuCard => "direct_pce_hucard",
        crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard => {
            "direct_pce_six_button_hucard"
        }
        crate::emu_thread::TasExecutionProfile::DirectPceCd => "direct_pce_cd",
    }
}

fn tas_readiness_status_name(
    status: crate::app::tas_control::readiness::TasReadinessStatus,
) -> &'static str {
    match status {
        crate::app::tas_control::readiness::TasReadinessStatus::Ready => "ready",
        crate::app::tas_control::readiness::TasReadinessStatus::ReloadRequired => "reload_required",
        crate::app::tas_control::readiness::TasReadinessStatus::Incompatible => "incompatible",
    }
}

fn tas_readiness_code_name(
    code: crate::app::tas_control::readiness::TasReadinessCode,
) -> &'static str {
    match code {
        crate::app::tas_control::readiness::TasReadinessCode::System => "system",
        crate::app::tas_control::readiness::TasReadinessCode::CoreIdentity => "core_identity",
        crate::app::tas_control::readiness::TasReadinessCode::LoadProvenance => "load_provenance",
        crate::app::tas_control::readiness::TasReadinessCode::DirectSource => "direct_source",
        crate::app::tas_control::readiness::TasReadinessCode::SourceMedia => "source_media",
        crate::app::tas_control::readiness::TasReadinessCode::EffectiveMedia => "effective_media",
        crate::app::tas_control::readiness::TasReadinessCode::Mods => "mods",
        crate::app::tas_control::readiness::TasReadinessCode::PersistentState => "persistent_state",
        crate::app::tas_control::readiness::TasReadinessCode::InitialInput => "initial_input",
        crate::app::tas_control::readiness::TasReadinessCode::SampleRate => "sample_rate",
        crate::app::tas_control::readiness::TasReadinessCode::Firmware => "firmware",
        crate::app::tas_control::readiness::TasReadinessCode::Hardware => "hardware",
        crate::app::tas_control::readiness::TasReadinessCode::Controllers => "controllers",
        crate::app::tas_control::readiness::TasReadinessCode::RemovableMedia => "removable_media",
        crate::app::tas_control::readiness::TasReadinessCode::Cheats => "cheats",
    }
}

fn tas_readiness_resource_name(
    resource: crate::app::tas_control::readiness::TasReadinessResource,
) -> &'static str {
    match resource {
        crate::app::tas_control::readiness::TasReadinessResource::RunningCore => "running_core",
        crate::app::tas_control::readiness::TasReadinessResource::LoadedMedia => "loaded_media",
        crate::app::tas_control::readiness::TasReadinessResource::LoadedProfile => "loaded_profile",
        crate::app::tas_control::readiness::TasReadinessResource::ControllerTopology => {
            "controller_topology"
        }
    }
}

fn tas_readiness_repair_name(
    repair: crate::app::tas_control::readiness::TasReadinessRepair,
) -> &'static str {
    match repair {
        crate::app::tas_control::readiness::TasReadinessRepair::None => "none",
        crate::app::tas_control::readiness::TasReadinessRepair::ReloadLoadedGame => {
            "reload_loaded_game"
        }
        crate::app::tas_control::readiness::TasReadinessRepair::LoadMatchingGame => {
            "load_matching_game"
        }
        crate::app::tas_control::readiness::TasReadinessRepair::ResolveProfileMismatch => {
            "resolve_profile_mismatch"
        }
    }
}

fn tas_readiness_value_json(
    value: &crate::app::tas_control::readiness::TasReadinessValue,
) -> Value {
    match value {
        crate::app::tas_control::readiness::TasReadinessValue::Text(value) => {
            json!({ "kind": "text", "value": value })
        }
        crate::app::tas_control::readiness::TasReadinessValue::Digest(value) => {
            json!({ "kind": "sha256", "value": value.to_hex() })
        }
        crate::app::tas_control::readiness::TasReadinessValue::Boolean(value) => {
            json!({ "kind": "boolean", "value": value })
        }
        crate::app::tas_control::readiness::TasReadinessValue::SampleRateProfile {
            initial,
            current,
        } => json!({
            "kind": "sample_rate_profile",
            "initial_hz": initial,
            "current_hz": current,
        }),
        crate::app::tas_control::readiness::TasReadinessValue::SampleRate(value) => {
            json!({ "kind": "sample_rate_hz", "value": value })
        }
    }
}

fn tas_last_failure_json(status: &crate::debug::TasEditorLiveStatus) -> Value {
    match status {
        crate::debug::TasEditorLiveStatus::Terminal(reason) => {
            json!({ "kind": "terminal", "message": reason })
        }
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_keeps_selection_end_and_execution_boundaries_distinct() {
        let timeline = tas_timeline_json(4, 9, Some((2, 5)));
        assert_eq!(timeline["selected_boundary"], 4);
        assert_eq!(timeline["selected_row"], 4);
        assert_eq!(timeline["selected_range"]["start"], 2);
        assert_eq!(timeline["selected_range"]["end"], 5);
        assert_eq!(timeline["selected_range"]["length"], 3);
        assert_eq!(timeline["end_boundary"], 9);
        assert_eq!(timeline["next_append_row"], 9);

        let end = tas_timeline_json(9, 9, None);
        assert_eq!(end["selected_row"], Value::Null);
        assert_eq!(end["selected_range"], Value::Null);

        let linked = live_tas_status_json(&crate::debug::TasEditorLiveStatus::Linked {
            cursor: 7,
            recording_available: true,
        });
        assert_eq!(linked["execution_boundary"], 7);
        assert_eq!(linked["cursor"], 7);
    }

    #[test]
    fn branch_creation_requires_the_settled_linked_boundary() {
        assert_eq!(require_selected_linked_boundary(9, Some(9)).unwrap(), 9);
        assert!(require_selected_linked_boundary(8, Some(9)).is_err());
        assert!(require_selected_linked_boundary(9, None).is_err());
    }

    #[test]
    fn status_exposes_branch_routes_and_the_active_branch() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("remote-tas-branches")?;
        let source_path = directory.path().join("game.nes");
        std::fs::write(&source_path, crate::test_support::build_nes_test_rom())?;
        let mut project =
            crate::emu_backend::loader::DirectNesTasExecutionLoader::new(source_path, Vec::new())
                .create_project()?;
        project.edit_transaction(|edit| {
            edit.fork_branch("main", 1, "route-b", "Route B")?;
            edit.set_active_branch("route-b")
        })?;

        let branches = tas_branches_json(&project, project.active_branch_id());
        assert_eq!(branches[0]["id"], "main");
        assert_eq!(branches[0]["active"], false);
        assert_eq!(branches[0]["origin"], Value::Null);
        assert_eq!(branches[1]["id"], "route-b");
        assert_eq!(branches[1]["name"], "Route B");
        assert_eq!(branches[1]["active"], true);
        assert_eq!(branches[1]["origin"]["branch_id"], "main");
        assert_eq!(branches[1]["origin"]["fork_boundary"], 1);
        Ok(())
    }

    #[test]
    fn status_distinguishes_active_recording_from_waiting_for_game_input() {
        assert_eq!(
            realtime_tas_recording_json(false, true)["realtime_active"],
            false
        );
        assert_eq!(
            realtime_tas_recording_json(false, true)["waiting_for_game_input"],
            false
        );
        assert_eq!(
            realtime_tas_recording_json(true, true)["waiting_for_game_input"],
            true
        );
        assert_eq!(
            realtime_tas_recording_json(true, false)["waiting_for_game_input"],
            false
        );
    }

    #[test]
    fn status_reports_playback_boundary_and_pending_pause() {
        let playing = live_tas_status_json(&crate::debug::TasEditorLiveStatus::Playing {
            cursor: 12,
            pause_pending: false,
        });
        assert_eq!(playing["state"], "playing");
        assert_eq!(playing["execution_boundary"], 12);
        let pausing = live_tas_status_json(&crate::debug::TasEditorLiveStatus::Playing {
            cursor: 12,
            pause_pending: true,
        });
        assert_eq!(pausing["state"], "pausing");
        assert_eq!(pausing["pause_pending"], true);
    }

    #[test]
    fn status_reports_readiness_and_terminal_failure_separately() {
        let unavailable =
            crate::debug::TasEditorLiveStatus::Unavailable("loaded sample rate differs".to_owned());
        assert_eq!(tas_readiness_json(&unavailable)["state"], "unavailable");
        assert_eq!(tas_last_failure_json(&unavailable), Value::Null);

        let reload = crate::debug::TasEditorLiveStatus::ReloadRequired(
            "loaded sample rate differs".to_owned(),
        );
        assert_eq!(live_tas_status_json(&reload)["state"], "reload_required");
        assert_eq!(tas_readiness_json(&reload)["state"], "reload_required");

        let terminal = crate::debug::TasEditorLiveStatus::Terminal(
            "the emulator worker became unavailable".to_owned(),
        );
        assert_eq!(tas_readiness_json(&terminal)["state"], "unavailable");
        assert_eq!(tas_last_failure_json(&terminal)["kind"], "terminal");
    }

    #[test]
    fn readiness_report_preserves_effective_and_configured_sample_rates() {
        use crate::app::tas_control::readiness::{
            TasReadinessCheck, TasReadinessCode, TasReadinessConditionId, TasReadinessRepair,
            TasReadinessReport, TasReadinessResource, TasReadinessStatus, TasReadinessValue,
        };

        let report = TasReadinessReport {
            worker_generation: 12,
            profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
            status: TasReadinessStatus::ReloadRequired,
            checks: vec![TasReadinessCheck {
                id: TasReadinessConditionId {
                    worker_generation: 12,
                    code: TasReadinessCode::SampleRate,
                    resource: TasReadinessResource::LoadedProfile,
                },
                expected: TasReadinessValue::SampleRate(48_000),
                loaded: Some(TasReadinessValue::SampleRateProfile {
                    initial: 44_100,
                    current: 44_100,
                }),
                configured: Some(TasReadinessValue::SampleRate(48_000)),
                status: TasReadinessStatus::ReloadRequired,
                repair: TasReadinessRepair::ReloadLoadedGame,
            }],
        };

        let json = tas_readiness_report_json(Some(&report)).unwrap();
        assert_eq!(json["state"], "reload_required");
        assert_eq!(json["checks"][0]["id"]["code"], "sample_rate");
        assert_eq!(json["checks"][0]["loaded"]["initial_hz"], 44_100);
        assert_eq!(json["checks"][0]["loaded"]["current_hz"], 44_100);
        assert_eq!(json["checks"][0]["configured"]["value"], 48_000);
        assert_eq!(json["checks"][0]["repair"], "reload_loaded_game");
    }

    #[test]
    fn readiness_profile_names_direct_cartridges() {
        assert_eq!(
            tas_execution_profile_name(
                crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg
            ),
            "direct_gb_cartridge_dmg"
        );
        assert_eq!(
            tas_execution_profile_name(crate::emu_thread::TasExecutionProfile::DirectSmsCartridge),
            "direct_sms_cartridge"
        );
        assert_eq!(
            tas_execution_profile_name(
                crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge
            ),
            "direct_game_gear_cartridge"
        );
        assert_eq!(
            tas_execution_profile_name(
                crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge
            ),
            "direct_sg1000_cartridge"
        );
        assert_eq!(
            tas_execution_profile_name(crate::emu_thread::TasExecutionProfile::DirectWsCartridge),
            "direct_ws_cartridge"
        );
    }
}
