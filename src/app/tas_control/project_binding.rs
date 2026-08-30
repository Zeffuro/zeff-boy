use anyhow::{Result, ensure};

use crate::emu_backend::loader::{
    TasProjectRuntimeWitness, classify_direct_tas_execution_profile, validate_tas_project_witness,
};
use crate::emu_thread::{TasControlLeaseWitness, TasExecutionProfile, TasInputFrame};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasDigest, TasEditorSession};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TasEditorControlSnapshot {
    pub(super) profile: TasExecutionProfile,
    pub(super) edit_generation: u64,
    pub(super) project_content_sha256: TasDigest,
    pub(super) sync_identity_sha256: TasDigest,
    pub(super) branch_id: String,
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
    pub(super) start_state_bytes: Vec<u8>,
    pub(super) input_prefix: Vec<TasInputFrame>,
    pub(super) total_input_frames: u64,
}

impl TasEditorControlSnapshot {
    pub(super) fn capture(session: &TasEditorSession) -> Result<Self> {
        let project = session.project();
        let profile = classify_direct_tas_execution_profile(project)?;
        let branch_id = session.selected_branch_id().to_owned();
        let cursor = session.cursor();
        let frame_count = session.selected_branch().frame_count();
        ensure!(
            cursor <= frame_count,
            "selected TAS cursor is past the branch end"
        );
        let execution_prefix_len = cursor
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("selected TAS input prefix overflows"))?
            .min(frame_count);
        Ok(Self {
            profile,
            edit_generation: project.edit_generation(),
            project_content_sha256: session.project_content_sha256(),
            sync_identity_sha256: project.sync_identity_sha256()?,
            branch_prefix_sha256: project.branch_prefix_sha256(&branch_id, execution_prefix_len)?,
            branch_id,
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
        Self::materialize(session, snapshot)
    }

    pub(super) fn prepare_linked_seek(
        session: &TasEditorSession,
        profile: TasExecutionProfile,
        sync_identity_sha256: TasDigest,
    ) -> Result<TasAcquiredProjectBinding> {
        let snapshot = Self::capture(session)?;
        ensure!(
            snapshot.profile == profile && snapshot.sync_identity_sha256 == sync_identity_sha256,
            "the TAS project identity changed while linked"
        );
        Self::materialize(session, snapshot)
    }

    fn materialize(
        session: &TasEditorSession,
        snapshot: TasEditorControlSnapshot,
    ) -> Result<TasAcquiredProjectBinding> {
        let branch = session.selected_branch();
        let prefix_len = snapshot.execution_prefix_len;
        let initial_input_frames = prefix_len.min(MAX_EDITOR_SEEK_EXECUTION_FRAMES);
        let input_prefix = (0..initial_input_frames)
            .map(|cursor| {
                let input = branch.input_at(cursor);
                let input = TasInputFrame {
                    p1_buttons: input.players[0].buttons,
                    p1_dpad: input.players[0].dpad,
                    p2_buttons: input.players[1].buttons,
                    p2_dpad: input.players[1].dpad,
                };
                validate_profile_input(snapshot.profile, input)?;
                Ok(input)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(TasAcquiredProjectBinding {
            snapshot,
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
        let input = session.selected_branch().input_at(cursor);
        let input = TasInputFrame {
            p1_buttons: input.players[0].buttons,
            p1_dpad: input.players[0].dpad,
            p2_buttons: input.players[1].buttons,
            p2_dpad: input.players[1].dpad,
        };
        validate_profile_input(snapshot.profile, input)?;
        Ok(input)
    }

    pub(super) fn matches_session_guard(&self, session: &TasEditorSession) -> bool {
        session.project().edit_generation() == self.edit_generation
            && session.selected_branch_id() == self.branch_id
            && session.cursor() == self.cursor
    }
}

fn validate_profile_input(profile: TasExecutionProfile, input: TasInputFrame) -> Result<()> {
    if profile == TasExecutionProfile::DirectGbRomOnlyDmg
        && (input.p1_buttons & !0x0F != 0
            || input.p1_dpad & !0x0F != 0
            || input.p2_buttons != 0
            || input.p2_dpad != 0)
    {
        anyhow::bail!("the selected TAS input is outside the direct Game Boy profile");
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
