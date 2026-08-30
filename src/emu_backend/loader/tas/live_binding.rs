use anyhow::{Result, ensure};

use super::*;

pub(crate) struct DirectNesTasRuntimeWitness<'a> {
    pub(crate) source_media_sha256: TasDigest,
    pub(crate) effective_media_sha256: TasDigest,
    pub(crate) current_state_bytes: &'a [u8],
    pub(crate) current_state_sha256: TasDigest,
    pub(crate) determinism_abi: &'a str,
    pub(crate) state_format_compatibility_id: &'a str,
    pub(crate) sync_config_sha256: TasDigest,
}

pub(crate) fn validate_direct_nes_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: DirectNesTasRuntimeWitness<'_>,
) -> Result<()> {
    project.validate()?;
    validate_direct_nes_tas_branch_scope(project, branch_id)?;
    validate_direct_nes_tas_project_identity(project)?;
    let identity = project.identity();

    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256,
        "worker media identity does not match the TAS project"
    );
    ensure!(
        witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker execution profile does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "worker current-state witness digest is inconsistent"
    );
    validate_current_nes_start_state(witness.current_state_bytes)
}

#[cfg(test)]
mod tests;
