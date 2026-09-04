#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use zeff_emu_common::replay::ReplayPlayer;

use crate::cli::run_loaded_replay_for_verification;
use crate::emu_backend::EmuBackend;

use super::model::{
    TasDigest, TasProject, TasProjectIdentity, TasVerificationCheckpoint, TasVerificationProvenance,
};
use super::zrpl::{
    CompiledReplay, decode_zrpl_file_bounded, require_zrpl_path, validate_compiled_replay,
    zrpl_load_limits,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasExecutionWitness {
    pub identity: TasProjectIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TasVerifiedReplayExportPhase {
    Preparing,
    CaptureVerification,
    ReproductionVerification,
    SavingProject,
    PublishingReplay,
}

pub(crate) struct TasExecutionSession {
    backend: EmuBackend,
    identity: TasProjectIdentity,
}

impl TasExecutionSession {
    pub(crate) fn new(backend: EmuBackend, identity: TasProjectIdentity) -> Self {
        Self { backend, identity }
    }

    pub(crate) fn identity(&self) -> &TasProjectIdentity {
        &self.identity
    }

    pub(super) fn into_parts(self) -> (EmuBackend, TasProjectIdentity) {
        (self.backend, self.identity)
    }
}

impl TasProject {
    pub(crate) fn verify_branch_with_factory(
        &mut self,
        branch_id: &str,
        witness: &TasExecutionWitness,
        mut load_backend: impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<TasVerificationProvenance> {
        if uses_direct_coleco_controller_topology(&self.identity) {
            return self.verify_direct_coleco_branch(branch_id, witness, &mut load_backend);
        }
        let candidate =
            self.prepare_verification_candidate(branch_id, witness, &mut load_backend)?;
        let (bytes, _) = self.compile_zrpl_with_provenance(branch_id, Some(&candidate))?;
        let player = ReplayPlayer::decode_bounded(&bytes, zrpl_load_limits())
            .context("compiled TAS replay failed bounded validation")?;
        let session = load_backend()?;
        self.validate_execution_identity(&session.identity, "reproduction session")?;
        validate_backend_observable_identity(&player, &session.backend)?;
        let second = run_loaded_replay_for_verification(session.backend, player, false)
            .context("TAS verification reproduction pass failed")?;
        ensure_frame_count(self, branch_id, second.frames)?;

        self.set_branch_verification(branch_id, candidate.clone());
        Ok(candidate)
    }

    pub(crate) fn verify_branch_with_factory_cancellable(
        &mut self,
        branch_id: &str,
        witness: &TasExecutionWitness,
        cancellation: &AtomicBool,
        progress: &mut impl FnMut(TasVerifiedReplayExportPhase),
        mut load_backend: impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<TasVerificationProvenance> {
        check_cancelled(cancellation)?;
        let candidate = self.prepare_verification_candidate_cancellable(
            branch_id,
            witness,
            cancellation,
            progress,
            &mut load_backend,
        )?;
        check_cancelled(cancellation)?;
        let (bytes, _) = self.compile_zrpl_with_provenance(branch_id, Some(&candidate))?;
        let player = ReplayPlayer::decode_bounded(&bytes, zrpl_load_limits())
            .context("compiled TAS replay failed bounded validation")?;
        let session = load_backend()?;
        self.validate_execution_identity(&session.identity, "reproduction session")?;
        validate_backend_observable_identity(&player, &session.backend)?;
        progress(TasVerifiedReplayExportPhase::ReproductionVerification);
        let second = run_loaded_replay_for_verification(session.backend, player, false)
            .context("TAS verification reproduction pass failed")?;
        check_cancelled(cancellation)?;
        ensure_frame_count(self, branch_id, second.frames)?;

        self.set_branch_verification(branch_id, candidate.clone());
        Ok(candidate)
    }

    pub(crate) fn verify_and_export_zrpl_with_factory(
        &mut self,
        branch_id: &str,
        path: &Path,
        witness: &TasExecutionWitness,
        mut load_backend: impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<PathBuf> {
        require_zrpl_path(path)?;
        if path.exists() {
            bail!("refusing to overwrite existing replay {}", path.display());
        }

        let candidate =
            self.prepare_verification_candidate(branch_id, witness, &mut load_backend)?;
        let (bytes, expected) = self.compile_zrpl_with_provenance(branch_id, Some(&candidate))?;
        crate::platform::write_new_file_atomically_validated(path, &bytes, |temp_file| {
            validate_and_execute_temporary_replay(
                self,
                branch_id,
                temp_file,
                &expected,
                load_backend()?,
            )
        })
        .with_context(|| format!("failed to publish verified replay {}", path.display()))?;

        self.set_branch_verification(branch_id, candidate);
        Ok(path.to_path_buf())
    }

    pub(crate) fn export_verified_zrpl_with_factory(
        &self,
        branch_id: &str,
        path: &Path,
        witness: &TasExecutionWitness,
        mut load_backend: impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<PathBuf> {
        require_zrpl_path(path)?;
        if path.exists() {
            bail!("refusing to overwrite existing replay {}", path.display());
        }
        self.validate()?;
        self.validate_execution_witness(witness)?;
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        let verification = branch
            .verification
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TAS branch {branch_id:?} is not verified"))?;
        if verification.branch_movie_sha256 != self.branch_movie_sha256(branch_id)? {
            bail!("TAS branch {branch_id:?} has stale verification provenance");
        }

        let (bytes, expected) = self.compile_zrpl_with_provenance(branch_id, Some(verification))?;
        crate::platform::write_new_file_atomically_validated(path, &bytes, |temp_file| {
            validate_and_execute_temporary_replay(
                self,
                branch_id,
                temp_file,
                &expected,
                load_backend()?,
            )
        })
        .with_context(|| format!("failed to publish verified replay {}", path.display()))?;
        Ok(path.to_path_buf())
    }

    pub(crate) fn export_verified_zrpl_with_factory_cancellable(
        &self,
        branch_id: &str,
        path: &Path,
        witness: &TasExecutionWitness,
        cancellation: &AtomicBool,
        mut load_backend: impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<PathBuf> {
        require_zrpl_path(path)?;
        if path.exists() {
            bail!("refusing to overwrite existing replay {}", path.display());
        }
        self.validate()?;
        self.validate_execution_witness(witness)?;
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        let verification = branch
            .verification
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TAS branch {branch_id:?} is not verified"))?;
        if verification.branch_movie_sha256 != self.branch_movie_sha256(branch_id)? {
            bail!("TAS branch {branch_id:?} has stale verification provenance");
        }
        check_cancelled(cancellation)?;

        let (bytes, expected) = self.compile_zrpl_with_provenance(branch_id, Some(verification))?;
        crate::platform::write_new_file_atomically_validated_cancellable(
            path,
            &bytes,
            |temp_file| {
                validate_and_execute_temporary_replay(
                    self,
                    branch_id,
                    temp_file,
                    &expected,
                    load_backend()?,
                )?;
                check_cancelled(cancellation)
            },
            || check_cancelled(cancellation),
        )
        .with_context(|| format!("failed to publish verified replay {}", path.display()))?;
        Ok(path.to_path_buf())
    }

    fn prepare_verification_candidate(
        &self,
        branch_id: &str,
        witness: &TasExecutionWitness,
        load_backend: &mut impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<TasVerificationProvenance> {
        self.validate()?;
        self.validate_execution_witness(witness)?;
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        let movie_hash = self.branch_movie_sha256(branch_id)?;
        let current = branch
            .verification
            .as_ref()
            .filter(|verification| verification.branch_movie_sha256 == movie_hash);
        let (bytes, _) = self.compile_zrpl_with_provenance(branch_id, current)?;
        let player = ReplayPlayer::decode_bounded(&bytes, zrpl_load_limits())
            .context("compiled TAS replay failed bounded validation")?;
        let session = load_backend()?;
        self.validate_execution_identity(&session.identity, "capture session")?;
        validate_backend_observable_identity(&player, &session.backend)?;
        let first = run_loaded_replay_for_verification(session.backend, player, current.is_none())
            .context("TAS verification capture pass failed")?;
        ensure_frame_count(self, branch_id, first.frames)?;

        let checkpoints = current.map_or_else(
            || {
                first
                    .checkpoints
                    .into_iter()
                    .map(|checkpoint| TasVerificationCheckpoint {
                        cursor: checkpoint.frame,
                        state_sha256: TasDigest(checkpoint.state_sha256),
                    })
                    .collect()
            },
            |verification| verification.checkpoints.clone(),
        );
        Ok(TasVerificationProvenance {
            branch_movie_sha256: movie_hash,
            checkpoints,
            final_state_sha256: Some(TasDigest(first.final_state_sha256)),
        })
    }

    fn prepare_verification_candidate_cancellable(
        &self,
        branch_id: &str,
        witness: &TasExecutionWitness,
        cancellation: &AtomicBool,
        progress: &mut impl FnMut(TasVerifiedReplayExportPhase),
        load_backend: &mut impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<TasVerificationProvenance> {
        self.validate()?;
        self.validate_execution_witness(witness)?;
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        let movie_hash = self.branch_movie_sha256(branch_id)?;
        let current = branch
            .verification
            .as_ref()
            .filter(|verification| verification.branch_movie_sha256 == movie_hash);
        let (bytes, _) = self.compile_zrpl_with_provenance(branch_id, current)?;
        let player = ReplayPlayer::decode_bounded(&bytes, zrpl_load_limits())
            .context("compiled TAS replay failed bounded validation")?;
        check_cancelled(cancellation)?;
        let session = load_backend()?;
        self.validate_execution_identity(&session.identity, "capture session")?;
        validate_backend_observable_identity(&player, &session.backend)?;
        progress(TasVerifiedReplayExportPhase::CaptureVerification);
        let first = run_loaded_replay_for_verification(session.backend, player, current.is_none())
            .context("TAS verification capture pass failed")?;
        check_cancelled(cancellation)?;
        ensure_frame_count(self, branch_id, first.frames)?;

        let checkpoints = current.map_or_else(
            || {
                first
                    .checkpoints
                    .into_iter()
                    .map(|checkpoint| TasVerificationCheckpoint {
                        cursor: checkpoint.frame,
                        state_sha256: TasDigest(checkpoint.state_sha256),
                    })
                    .collect()
            },
            |verification| verification.checkpoints.clone(),
        );
        Ok(TasVerificationProvenance {
            branch_movie_sha256: movie_hash,
            checkpoints,
            final_state_sha256: Some(TasDigest(first.final_state_sha256)),
        })
    }

    fn validate_execution_witness(&self, witness: &TasExecutionWitness) -> Result<()> {
        self.validate_execution_identity(&witness.identity, "execution witness")
    }

    pub(super) fn validate_execution_identity(
        &self,
        identity: &TasProjectIdentity,
        label: &str,
    ) -> Result<()> {
        let mut witnessed = self.clone();
        witnessed.identity = identity.clone();
        witnessed
            .validate()
            .with_context(|| format!("invalid TAS {label} identity"))?;
        if witnessed.canonical_identity() != self.canonical_identity() {
            bail!("TAS {label} does not match the complete project identity");
        }
        Ok(())
    }

    fn set_branch_verification(
        &mut self,
        branch_id: &str,
        verification: TasVerificationProvenance,
    ) {
        let branch = self
            .branches
            .iter_mut()
            .find(|branch| branch.id == branch_id)
            .expect("verified branch still exists");
        branch.verification = Some(verification);
    }
}

impl TasProject {
    fn verify_direct_coleco_branch(
        &mut self,
        branch_id: &str,
        witness: &TasExecutionWitness,
        load_backend: &mut impl FnMut() -> Result<TasExecutionSession>,
    ) -> Result<TasVerificationProvenance> {
        self.validate()?;
        self.validate_execution_witness(witness)?;
        let movie_hash = self.branch_movie_sha256(branch_id)?;
        let current = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?
            .verification()
            .filter(|verification| verification.branch_movie_sha256 == movie_hash);
        let first = run_direct_coleco_project(
            self,
            branch_id,
            load_backend()?,
            current.map(|verification| verification.checkpoints.as_slice()),
            current.and_then(|verification| verification.final_state_sha256),
            current.is_none(),
        )?;
        let candidate = TasVerificationProvenance {
            branch_movie_sha256: movie_hash,
            checkpoints: current.map_or(first.checkpoints, |verification| {
                verification.checkpoints.clone()
            }),
            final_state_sha256: Some(first.final_state_sha256),
        };
        let second = run_direct_coleco_project(
            self,
            branch_id,
            load_backend()?,
            Some(candidate.checkpoints.as_slice()),
            candidate.final_state_sha256,
            false,
        )?;
        ensure_frame_count(
            self,
            branch_id,
            usize::try_from(second.frames)
                .context("ColecoVision frame count exceeds this platform")?,
        )?;
        self.set_branch_verification(branch_id, candidate.clone());
        Ok(candidate)
    }
}

struct DirectColecoVerificationRun {
    frames: u64,
    checkpoints: Vec<TasVerificationCheckpoint>,
    final_state_sha256: TasDigest,
}

fn uses_direct_coleco_controller_topology(identity: &TasProjectIdentity) -> bool {
    identity
        .devices
        .iter()
        .any(|device| device.device == "coleco-standard-controller-keypad")
}

fn run_direct_coleco_project(
    project: &TasProject,
    branch_id: &str,
    session: TasExecutionSession,
    expected_checkpoints: Option<&[TasVerificationCheckpoint]>,
    expected_final_state_sha256: Option<TasDigest>,
    capture_checkpoints: bool,
) -> Result<DirectColecoVerificationRun> {
    project.validate_execution_identity(session.identity(), "ColecoVision verification session")?;
    let (mut backend, _) = session.into_parts();
    if backend.system() != crate::emu_backend::ActiveSystem::Coleco {
        bail!("ColecoVision TAS verification requires a ColecoVision backend");
    }
    let branch = project
        .branch(branch_id)
        .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
    let mut checkpoints = Vec::new();
    for cursor in 0..branch.frame_count() {
        let input = branch.input_at(cursor);
        if input
            .players
            .iter()
            .any(|player| *player != Default::default())
            || input.zapper != Default::default()
            || input.tilt_x_bits != 0
            || input.tilt_y_bits != 0
            || !matches!(input.camera, super::model::TasCameraInput::None)
        {
            bail!("ColecoVision TAS verification encountered unsupported input data");
        }
        backend.apply_coleco_tas_input(input.coleco)?;
        let frame_count = backend.frame_count();
        backend.step_frame();
        if backend.frame_count()
            != frame_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("ColecoVision frame count overflow"))?
        {
            bail!("ColecoVision TAS verification made no frame progress");
        }
        let completed_cursor = cursor + 1;
        if completed_cursor % 300 == 0 {
            let state_sha256 = TasDigest::from_bytes(&backend.encode_replay_hash_state_bytes()?);
            if let Some(expected) = expected_checkpoints.and_then(|checkpoints| {
                checkpoints
                    .iter()
                    .find(|checkpoint| checkpoint.cursor == completed_cursor)
            }) {
                if expected.state_sha256 != state_sha256 {
                    bail!(
                        "ColecoVision TAS verification diverged at checkpoint cursor {completed_cursor}"
                    );
                }
            } else if capture_checkpoints {
                checkpoints.push(TasVerificationCheckpoint {
                    cursor: completed_cursor,
                    state_sha256,
                });
            }
        }
    }
    let final_state_sha256 = TasDigest::from_bytes(&backend.encode_replay_hash_state_bytes()?);
    if let Some(expected) = expected_final_state_sha256
        && expected != final_state_sha256
    {
        bail!("ColecoVision TAS verification final state differs from its provenance");
    }
    Ok(DirectColecoVerificationRun {
        frames: branch.frame_count(),
        checkpoints,
        final_state_sha256,
    })
}

fn check_cancelled(cancellation: &AtomicBool) -> Result<()> {
    if cancellation.load(Ordering::Acquire) {
        bail!("verified replay export was canceled");
    }
    Ok(())
}

fn validate_and_execute_temporary_replay(
    project: &TasProject,
    branch_id: &str,
    temp_file: &mut std::fs::File,
    expected: &CompiledReplay,
    session: TasExecutionSession,
) -> Result<()> {
    let player = decode_zrpl_file_bounded(temp_file).context("temporary replay is invalid")?;
    validate_compiled_replay(&player, expected)?;
    project.validate_execution_identity(&session.identity, "export session")?;
    validate_backend_observable_identity(&player, &session.backend)?;
    let run = run_loaded_replay_for_verification(session.backend, player, false)
        .context("temporary replay execution failed")?;
    ensure_frame_count(project, branch_id, run.frames)
}

pub(super) fn validate_backend_observable_identity(
    player: &ReplayPlayer,
    backend: &EmuBackend,
) -> Result<()> {
    let expected = player.metadata();
    let actual = backend.replay_metadata();
    if expected.system != actual.system {
        bail!("TAS execution backend system differs from its witness");
    }
    if expected.core_family != actual.core_family {
        bail!("TAS execution backend core family differs from its witness");
    }
    if expected.rom_sha256 != actual.rom_sha256 {
        bail!("TAS execution backend effective media differs from its witness");
    }
    if !zeff_emu_common::replay::firmware_manifests_match(&expected.firmware, &actual.firmware) {
        bail!("TAS execution backend firmware differs from its witness");
    }
    Ok(())
}

fn ensure_frame_count(project: &TasProject, branch_id: &str, actual: usize) -> Result<()> {
    let expected = project
        .branch(branch_id)
        .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?
        .frame_count;
    let actual = u64::try_from(actual).context("verified replay frame count does not fit u64")?;
    if actual != expected {
        bail!("verified replay frame count differs: expected {expected}, got {actual}");
    }
    Ok(())
}
