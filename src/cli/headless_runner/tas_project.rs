use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::emu_backend::{
    ActiveSystem,
    loader::{PrivateTasExecutionLoader, select_private_tas_execution_loader_for_project},
};
use crate::tas_project::{TasExecutionWitness, TasProject};

use super::HeadlessOptions;

pub(super) fn run_tas_project_headless(
    rom_source_path: &Path,
    firmware_search_dirs: Vec<PathBuf>,
    opts: &HeadlessOptions,
) -> Result<()> {
    let project_path = opts
        .tas_project_path
        .as_deref()
        .context("missing --tas-verify project path")?;
    let project = TasProject::load(project_path)
        .with_context(|| format!("failed to load TAS project {}", project_path.display()))?;
    let branch_id = opts
        .tas_branch_id
        .as_deref()
        .unwrap_or(project.active_branch_id())
        .to_owned();
    let system = ActiveSystem::from_code(&project.identity().system)
        .context("TAS project has an unsupported system identity")?;
    let plan = select_private_tas_execution_loader_for_project(
        rom_source_path.to_path_buf(),
        system,
        firmware_search_dirs,
        &project,
    )?;
    run_tas_project_headless_with_plan(plan, project_path, &branch_id, opts)
}

fn run_tas_project_headless_with_plan(
    plan: PrivateTasExecutionLoader,
    project_path: &Path,
    branch_id: &str,
    opts: &HeadlessOptions,
) -> Result<()> {
    let mut project = TasProject::load(project_path)
        .with_context(|| format!("failed to load TAS project {}", project_path.display()))?;
    plan.validate_project_branch_scope(&project, branch_id)?;
    let start_state = project.start_state().to_vec();
    let witness_session = plan.load_session(&start_state)?;
    let witness = TasExecutionWitness {
        identity: witness_session.identity().clone(),
    };
    let verification = project
        .verify_branch_with_factory(branch_id, &witness, || plan.load_session(&start_state))?;

    project.save_atomic(project_path).with_context(|| {
        format!(
            "failed to save verified TAS project {}",
            project_path.display()
        )
    })?;
    println!(
        "[tas] verify project={} branch={} frames={} checkpoints={} final_state_sha256={} status=verified",
        project_path.display(),
        branch_id,
        project
            .branch(branch_id)
            .expect("verified branch still exists")
            .frame_count(),
        verification.checkpoints.len(),
        verification
            .final_state_sha256
            .map_or_else(|| "none".to_owned(), |digest| digest.to_hex()),
    );
    println!("[tas] project_saved={}", project_path.display());

    if let Some(export_path) = opts.tas_export_path.as_deref() {
        project.export_verified_zrpl_with_factory(branch_id, export_path, &witness, || {
            plan.load_session(&start_state)
        })?;
        println!(
            "[tas] export status=exported output={}",
            export_path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
