use anyhow::{Result, ensure};

use crate::emu_backend::loader::{
    TasProjectRuntimeWitness, classify_direct_tas_execution_profile, validate_fds_tas_branch_scope,
    validate_tas_project_witness,
};
use crate::emu_thread::{
    TasControlLeaseWitness, TasExecutionCacheProof, TasExecutionPredecessorWindow,
    TasExecutionProfile, TasFdsMediaEvent, TasInputFrame, tas_intermediate_cache_cursors,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasDigest, TasEditorSession};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TasEditorControlSnapshot {
    pub(super) profile: TasExecutionProfile,
    pub(super) edit_generation: u64,
    pub(super) project_content_sha256: TasDigest,
    pub(super) sync_identity_sha256: TasDigest,
    pub(super) branch_id: String,
    pub(super) branch_frame_count: u64,
    pub(super) cursor: u64,
    pub(super) execution_prefix_len: u64,
    pub(super) branch_prefix_sha256: TasDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TasControlHeldProof {
    pub(super) frame_count: u64,
    pub(super) current_state_sha256: TasDigest,
}

impl TasControlHeldProof {
    pub(super) fn from_witness(witness: &TasControlLeaseWitness) -> Self {
        Self {
            frame_count: witness.frame_count,
            current_state_sha256: TasDigest::from_bytes(&witness.current_state_bytes),
        }
    }
}

pub(super) struct TasAcquiredProjectBinding {
    pub(super) snapshot: TasEditorControlSnapshot,
    pub(super) intermediate_cache_proofs: Vec<TasExecutionCacheProof>,
    pub(super) predecessor_window: Option<TasExecutionPredecessorWindow>,
    pub(super) start_state_bytes: Vec<u8>,
    pub(super) input_prefix: Vec<TasInputFrame>,
    pub(super) total_input_frames: u64,
}

impl TasEditorControlSnapshot {
    pub(super) fn cache_proof(&self) -> TasExecutionCacheProof {
        TasExecutionCacheProof {
            sync_identity_sha256: self.sync_identity_sha256,
            branch_prefix_sha256: self.branch_prefix_sha256,
            target_cursor: self.execution_prefix_len,
        }
    }

    pub(super) fn capture(session: &TasEditorSession) -> Result<Self> {
        Self::capture_at(session, session.cursor())
    }

    pub(super) fn capture_at(session: &TasEditorSession, cursor: u64) -> Result<Self> {
        let project = session.project();
        let profile = classify_direct_tas_execution_profile(project)?;
        let branch_id = session.selected_branch_id().to_owned();
        let frame_count = session.selected_branch().frame_count();
        ensure!(
            cursor <= frame_count,
            "selected TAS cursor is past the branch end"
        );
        let execution_prefix_len = cursor;
        Ok(Self {
            profile,
            edit_generation: project.edit_generation(),
            project_content_sha256: session.project_content_sha256(),
            sync_identity_sha256: project.sync_identity_sha256_from_validated()?,
            branch_prefix_sha256: project
                .branch_prefix_sha256_from_validated(&branch_id, execution_prefix_len)?,
            branch_id,
            branch_frame_count: frame_count,
            cursor,
            execution_prefix_len,
        })
    }

    pub(super) fn validate_acquired(
        session: &TasEditorSession,
        witness: &TasControlLeaseWitness,
    ) -> Result<TasAcquiredProjectBinding> {
        let snapshot = Self::capture(session)?;
        ensure!(
            witness.profile == snapshot.profile,
            "worker execution profile does not match the TAS project"
        );
        validate_tas_project_witness(
            session.project(),
            &snapshot.branch_id,
            TasProjectRuntimeWitness {
                profile: snapshot.profile,
                source_media_sha256: witness.source_media_sha256,
                effective_media_sha256: witness.effective_media_sha256,
                current_state_bytes: &witness.current_state_bytes,
                current_state_sha256: witness.current_state_sha256,
                determinism_abi: witness.determinism_abi,
                state_format_compatibility_id: witness.state_format_compatibility_id,
                sync_config_sha256: witness.sync_config_sha256,
            },
        )?;
        Self::materialize(session, snapshot, &[])
    }

    pub(super) fn prepare_linked_seek(
        session: &TasEditorSession,
        profile: TasExecutionProfile,
        sync_identity_sha256: TasDigest,
        cache_candidate_cursors: &[u64],
    ) -> Result<TasAcquiredProjectBinding> {
        Self::prepare_linked_seek_at(
            session,
            session.cursor(),
            profile,
            sync_identity_sha256,
            cache_candidate_cursors,
        )
    }

    pub(super) fn prepare_linked_seek_at(
        session: &TasEditorSession,
        cursor: u64,
        profile: TasExecutionProfile,
        sync_identity_sha256: TasDigest,
        cache_candidate_cursors: &[u64],
    ) -> Result<TasAcquiredProjectBinding> {
        let snapshot = Self::capture_at(session, cursor)?;
        ensure!(
            snapshot.profile == profile && snapshot.sync_identity_sha256 == sync_identity_sha256,
            "the TAS project identity changed while linked"
        );
        Self::materialize(session, snapshot, cache_candidate_cursors)
    }

    fn materialize(
        session: &TasEditorSession,
        snapshot: TasEditorControlSnapshot,
        cache_candidate_cursors: &[u64],
    ) -> Result<TasAcquiredProjectBinding> {
        if snapshot.profile == TasExecutionProfile::DirectFdsDisk {
            validate_fds_tas_branch_scope(session.project(), &snapshot.branch_id)?;
        }
        let branch = session.selected_branch();
        let prefix_len = snapshot.execution_prefix_len;
        let initial_input_frames = prefix_len.min(MAX_EDITOR_SEEK_EXECUTION_FRAMES);
        let input_prefix = (0..initial_input_frames)
            .map(|cursor| materialize_profile_input(branch, cursor, snapshot.profile))
            .collect::<Result<Vec<_>>>()?;
        let predecessor_cursor = cache_candidate_cursors
            .iter()
            .copied()
            .filter(|cursor| *cursor > 0 && *cursor < prefix_len)
            .max();
        let intermediate_cursors = tas_intermediate_cache_cursors(prefix_len);
        let mut proof_cursors = Vec::with_capacity(
            intermediate_cursors.len() + usize::from(predecessor_cursor.is_some()),
        );
        if let Some(source_cursor) = predecessor_cursor {
            proof_cursors.push(source_cursor);
        }
        proof_cursors.extend(intermediate_cursors.iter().copied());
        let mut proof_hashes = session
            .project()
            .branch_prefix_sha256_many_from_validated(&snapshot.branch_id, &proof_cursors)?
            .into_iter();
        let predecessor_window = predecessor_cursor
            .map(|source_cursor| {
                let input_end_cursor = source_cursor
                    .saturating_add(MAX_EDITOR_SEEK_EXECUTION_FRAMES)
                    .min(prefix_len);
                let input_frames = (source_cursor..input_end_cursor)
                    .map(|cursor| materialize_profile_input(branch, cursor, snapshot.profile))
                    .collect::<Result<Vec<_>>>()?;
                let source_proofs = vec![
                    snapshot.cache_proof(),
                    TasExecutionCacheProof {
                        sync_identity_sha256: snapshot.sync_identity_sha256,
                        branch_prefix_sha256: proof_hashes
                            .next()
                            .expect("source proof hash was batched"),
                        target_cursor: source_cursor,
                    },
                ];
                Ok::<_, anyhow::Error>(TasExecutionPredecessorWindow {
                    source_proofs,
                    input_start_cursor: source_cursor,
                    input_frames,
                })
            })
            .transpose()?;
        let intermediate_cache_proofs = intermediate_cursors
            .into_iter()
            .map(|target_cursor| TasExecutionCacheProof {
                sync_identity_sha256: snapshot.sync_identity_sha256,
                branch_prefix_sha256: proof_hashes
                    .next()
                    .expect("intermediate proof hash was batched"),
                target_cursor,
            })
            .collect();
        Ok(TasAcquiredProjectBinding {
            snapshot,
            intermediate_cache_proofs,
            predecessor_window,
            start_state_bytes: session.project().start_state().to_vec(),
            input_prefix,
            total_input_frames: prefix_len,
        })
    }

    pub(super) fn input_at(
        session: &TasEditorSession,
        snapshot: &TasEditorControlSnapshot,
        cursor: u64,
    ) -> Result<TasInputFrame> {
        ensure!(
            snapshot.matches_session_guard(session),
            "the TAS project changed during staged execution"
        );
        ensure!(
            cursor < snapshot.execution_prefix_len,
            "staged TAS execution cursor is past the selected input"
        );
        materialize_profile_input(session.selected_branch(), cursor, snapshot.profile)
    }

    pub(super) fn input_at_linked(
        session: &TasEditorSession,
        snapshot: &TasEditorControlSnapshot,
        current: &TasEditorControlSnapshot,
        cursor: u64,
    ) -> Result<TasInputFrame> {
        ensure!(
            current == snapshot,
            "the TAS project changed during playback"
        );
        ensure!(
            cursor < snapshot.branch_frame_count,
            "linked TAS playback cannot consume the end boundary"
        );
        materialize_profile_input(session.selected_branch(), cursor, snapshot.profile)
    }

    pub(super) fn matches_session_guard(&self, session: &TasEditorSession) -> bool {
        session.project().edit_generation() == self.edit_generation
            && session.selected_branch_id() == self.branch_id
            && session.cursor() == self.cursor
    }

    pub(super) fn matches_project_revision(&self, session: &TasEditorSession) -> bool {
        self.project_content_sha256 == session.project_content_sha256()
            && self.edit_generation == session.project().edit_generation()
            && self.branch_id == session.selected_branch_id()
            && self.branch_frame_count == session.selected_branch().frame_count()
    }

    pub(super) fn matches_linked_project(&self, current: Option<&Self>) -> bool {
        current.is_some_and(|current| {
            self.profile == current.profile
                && self.edit_generation == current.edit_generation
                && self.project_content_sha256 == current.project_content_sha256
                && self.sync_identity_sha256 == current.sync_identity_sha256
                && self.branch_id == current.branch_id
                && self.branch_frame_count == current.branch_frame_count
        })
    }

    pub(super) fn can_rebind_at_same_execution(&self, current: Option<&Self>) -> bool {
        current.is_some_and(|current| {
            self.profile == current.profile
                && self.sync_identity_sha256 == current.sync_identity_sha256
                && self.execution_prefix_len == current.execution_prefix_len
                && self.branch_prefix_sha256 == current.branch_prefix_sha256
        })
    }
}

fn materialize_profile_input(
    branch: &crate::tas_project::TasBranch,
    cursor: u64,
    profile: TasExecutionProfile,
) -> Result<TasInputFrame> {
    let input = branch.input_at(cursor);
    let [p3, p4, p5] = if matches!(
        profile,
        TasExecutionProfile::DirectPceMultitapHuCard | TasExecutionProfile::DirectPceMultitapCd
    ) {
        [input.players[2], input.players[3], input.players[4]]
    } else {
        [Default::default(); 3]
    };
    let input = TasInputFrame {
        p1_buttons: input.players[0].buttons,
        p1_dpad: input.players[0].dpad,
        p2_buttons: input.players[1].buttons,
        p2_dpad: input.players[1].dpad,
        p3_buttons: p3.buttons,
        p3_dpad: p3.dpad,
        p4_buttons: p4.buttons,
        p4_dpad: p4.dpad,
        p5_buttons: p5.buttons,
        p5_dpad: p5.dpad,
        coleco: input.coleco,
        zapper: zeff_emu_common::replay::ReplayZapperFrame {
            enabled: input.zapper.enabled,
            trigger: input.zapper.trigger,
            hit: input.zapper.hit,
            screen_pos: input.zapper.screen_pos.map(|[x, y]| (x, y)),
        },
        tilt_x_bits: input.tilt_x_bits,
        tilt_y_bits: input.tilt_y_bits,
        fds_disk_side: (profile == TasExecutionProfile::DirectFdsDisk)
            .then(|| {
                branch.events().iter().find_map(|event| match event {
                    zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame, side }
                        if *frame == cursor =>
                    {
                        Some(*side)
                    }
                    _ => None,
                })
            })
            .flatten(),
        fds_write_protected: (profile == TasExecutionProfile::DirectFdsDisk)
            .then(|| {
                branch.events().iter().find_map(|event| match event {
                    zeff_emu_common::replay::ReplayEvent::Media {
                        frame,
                        event:
                            zeff_emu_common::media::MediaEvent::SetWriteProtected {
                                write_protected,
                                ..
                            },
                        ..
                    } if *frame == cursor => Some(*write_protected),
                    _ => None,
                })
            })
            .flatten(),
        fds_media_event: (profile == TasExecutionProfile::DirectFdsDisk)
            .then(|| {
                branch.events().iter().find_map(|event| match event {
                    zeff_emu_common::replay::ReplayEvent::Media {
                        frame,
                        event: zeff_emu_common::media::MediaEvent::Eject { .. },
                        ..
                    } if *frame == cursor => Some(TasFdsMediaEvent::Eject),
                    zeff_emu_common::replay::ReplayEvent::Media {
                        frame,
                        event:
                            zeff_emu_common::media::MediaEvent::Insert {
                                side: Some(side),
                                write_protected,
                                ..
                            },
                        ..
                    } if *frame == cursor => Some(TasFdsMediaEvent::Insert {
                        side: *side,
                        write_protected: *write_protected,
                    }),
                    _ => None,
                })
            })
            .flatten(),
    };
    validate_profile_input(profile, input)?;
    Ok(input)
}

fn validate_profile_input(profile: TasExecutionProfile, input: TasInputFrame) -> Result<()> {
    if !matches!(
        profile,
        TasExecutionProfile::DirectGbCartridgeDmg
            | TasExecutionProfile::DirectGbCartridgeCgb
            | TasExecutionProfile::DirectGbaCartridge
    ) && (input.tilt_x_bits != 0 || input.tilt_y_bits != 0)
    {
        anyhow::bail!("the selected TAS input contains tilt sensor input");
    }
    if profile != TasExecutionProfile::DirectFdsDisk
        && (input.fds_disk_side.is_some()
            || input.fds_write_protected.is_some()
            || input.fds_media_event.is_some())
    {
        anyhow::bail!("the selected TAS input contains an FDS drive event");
    }
    match profile {
        TasExecutionProfile::DirectNesCartridge | TasExecutionProfile::DirectFdsDisk
            if input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2] =>
        {
            anyhow::bail!("the selected TAS input is outside the direct NES profile");
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb
            if input.p1_buttons & !0x0F != 0
                || input.p1_dpad & !0x0F != 0
                || input.p2_buttons != 0
                || input.p2_dpad != 0
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper.enabled
                || input.zapper.trigger
                || input.zapper.hit
                || input.zapper.screen_pos.is_some() =>
        {
            anyhow::bail!("the selected TAS input is outside the direct Game Boy profile");
        }
        TasExecutionProfile::DirectColecoCartridge
            if input.p1_buttons != 0
                || input.p1_dpad != 0
                || input.p2_buttons != 0
                || input.p2_dpad != 0
                || input.zapper != Default::default() =>
        {
            anyhow::bail!("the selected TAS input is outside the direct ColecoVision profile");
        }
        TasExecutionProfile::DirectSmsCartridge | TasExecutionProfile::DirectSg1000Cartridge
            if input.p1_buttons & !0x03 != 0
                || input.p1_dpad & !0x0F != 0
                || input.p2_buttons & !0x03 != 0
                || input.p2_dpad & !0x0F != 0
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default() =>
        {
            anyhow::bail!("the selected TAS input is outside the direct Sega 8-bit profile");
        }
        TasExecutionProfile::DirectGameGearCartridge
            if input.p1_buttons & !0x0B != 0
                || input.p1_dpad & !0x0F != 0
                || input.p2_buttons != 0
                || input.p2_dpad != 0
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default() =>
        {
            anyhow::bail!("the selected TAS input is outside the direct Game Gear profile");
        }
        TasExecutionProfile::DirectGbaCartridge
            if input.p1_buttons & !0x3F != 0
                || input.p1_dpad & !0x0F != 0
                || input.p2_buttons != 0
                || input.p2_dpad != 0
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default() =>
        {
            anyhow::bail!("the selected TAS input is outside the direct GBA profile");
        }
        TasExecutionProfile::DirectPceHuCard
            if input.p1_buttons & !0x0F != 0
                || input.p1_dpad & !0x0F != 0
                || input.p2_buttons != 0
                || input.p2_dpad != 0
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default() =>
        {
            anyhow::bail!("the selected TAS input is outside the direct PC Engine profile");
        }
        TasExecutionProfile::DirectPceSixButtonHuCard
            if input.p1_dpad & !0x0F != 0
                || input.p2_buttons != 0
                || input.p2_dpad != 0
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default() =>
        {
            anyhow::bail!(
                "the selected TAS input is outside the direct PC Engine six-button profile"
            );
        }
        TasExecutionProfile::DirectPceMultitapHuCard | TasExecutionProfile::DirectPceMultitapCd
            if input.p1_buttons & !0x0F != 0
                || input.p1_dpad & !0x0F != 0
                || input.p2_buttons & !0x0F != 0
                || input.p2_dpad & !0x0F != 0
                || input.p3_buttons & !0x0F != 0
                || input.p3_dpad & !0x0F != 0
                || input.p4_buttons & !0x0F != 0
                || input.p4_dpad & !0x0F != 0
                || input.p5_buttons & !0x0F != 0
                || input.p5_dpad & !0x0F != 0
                || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
                || input.zapper != Default::default() =>
        {
            anyhow::bail!(
                "the selected TAS input is outside the direct PC Engine multitap profile"
            );
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use anyhow::Result;

    use super::*;
    use crate::emu_backend::loader::DirectNesTasExecutionLoader;
    use crate::tas_project::{
        TasAutosaveConfig, TasAutosaveStore, TasInitialBranch, TasProject, TasSeekStateCache,
    };

    #[test]
    fn capture_refuses_a_same_system_project_outside_the_direct_profile() -> Result<()> {
        let directory = crate::test_support::test_directory("tas-control-profile-classifier")?;
        let source_path = directory.path().join("game.nes");
        std::fs::write(&source_path, crate::test_support::build_nes_test_rom())?;
        let project = DirectNesTasExecutionLoader::new(source_path, Vec::new()).create_project()?;
        let mut identity = project.identity().clone();
        identity.core_family = "wrong-native-core".to_owned();
        let project = TasProject::new(
            "wrong-profile",
            identity,
            project.start_state().to_vec(),
            Default::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count: 1,
                input_spans: Vec::new(),
                events: Vec::new(),
            },
            BTreeMap::new(),
        )?;
        let manual_path = directory.path().join("movie.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
        let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
        let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;

        assert!(TasEditorControlSnapshot::capture(&session).is_err());
        Ok(())
    }
}
