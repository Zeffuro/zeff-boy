use std::path::{Path, PathBuf};

use crate::emu_backend::loader::md_tas::{
    MD_TAS_DETERMINISM_ABI, MD_TAS_STATE_COMPATIBILITY, MdTasProfile,
};
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{
    TasControlAcquireRejectedReason as Rejected, TasControlLeaseWitness, TasExecutionProfile,
    TasLoadedProfileObservation,
};
use crate::tas_project::{TasDigest, TasInputFrame, TasProject};
use anyhow::{Result, ensure};

pub(crate) fn md_execution_profile(profile: MdTasProfile) -> TasExecutionProfile {
    match profile {
        MdTasProfile::ThreeButton => TasExecutionProfile::DirectMdThreeButton,
        MdTasProfile::SixButton => TasExecutionProfile::DirectMdSixButton,
    }
}

pub(crate) fn md_pad_profile(profile: TasExecutionProfile) -> Option<MdTasProfile> {
    match profile {
        TasExecutionProfile::DirectMdThreeButton => Some(MdTasProfile::ThreeButton),
        TasExecutionProfile::DirectMdSixButton => Some(MdTasProfile::SixButton),
        _ => None,
    }
}

pub(crate) fn classify_md_tas_project(project: &TasProject) -> Result<TasExecutionProfile> {
    super::direct_md::project_profile(project).map(md_execution_profile)
}

pub(crate) fn validate_md_tas_branch_scope(project: &TasProject, branch: &str) -> Result<()> {
    super::direct_md::validate_branch_scope(project, branch)
}

pub(crate) fn select_md_tas_loader(
    source: PathBuf,
    member: Option<PathBuf>,
    profile: MdTasProfile,
) -> Result<super::PrivateTasExecutionLoader> {
    let loader = if super::has_extension(&source, "zip") {
        super::DirectMdTasExecutionLoader::new_zip(
            source,
            member.ok_or_else(|| anyhow::anyhow!("MD TAS requires an explicit ZIP member"))?,
            profile,
        )
    } else {
        ensure!(
            member.as_ref().is_none_or(|member| member == &source),
            "MD direct member differs"
        );
        super::DirectMdTasExecutionLoader::new(source, profile)
    };
    loader.create_project()?;
    Ok(super::PrivateTasExecutionLoader::DirectMd(loader))
}

pub(crate) fn loaded_md_tas_profile(backend: &EmuBackend) -> Option<MdTasProfile> {
    let EmuBackend::Md(md) = backend else {
        return None;
    };
    let load = md.tas_load().ok()?;
    load.validate_fixed(md).ok()?;
    load.validate_source(md).ok()?;
    load.config().boundary_hashes(md.core()).ok()?;
    Some(load.profile())
}

pub(crate) fn build_md_tas_witness(
    backend: &EmuBackend,
    cheats: bool,
    profile: TasExecutionProfile,
) -> Result<TasControlLeaseWitness, Rejected> {
    if cheats || backend.replay_metadata().cheat_sha256.is_some() {
        return Err(Rejected::CheatsPresent);
    }
    let expected = md_pad_profile(profile).ok_or(Rejected::UnsupportedSystem)?;
    let EmuBackend::Md(md) = backend else {
        return Err(Rejected::UnsupportedSystem);
    };
    let load = md
        .tas_load()
        .map_err(|_| Rejected::LoadProvenanceUnavailable)?;
    load.validate_fixed(md)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    load.validate_source(md)
        .map_err(|_| Rejected::SourceMediaMismatch)?;
    load.config()
        .boundary_hashes(md.core())
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    if load.profile() != expected {
        return Err(Rejected::NonStandardControllerTopology);
    }
    let metadata = backend.replay_metadata();
    if metadata.system.as_deref() != Some(ActiveSystem::Md.code())
        || metadata.core_family.as_deref() != Some("MegaDrive")
        || metadata.rom_sha256 != Some(load.effective_sha256())
        || !metadata.firmware.is_empty()
    {
        return Err(Rejected::IdentityMetadataMismatch);
    }
    let frame_count = backend.frame_count();
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    load.validate_source(md)
        .map_err(|_| Rejected::SourceMediaMismatch)?;
    if frame_count != backend.frame_count() {
        return Err(Rejected::StateWitnessUnavailable);
    }
    Ok(TasControlLeaseWitness {
        profile,
        frame_count,
        source_media_sha256: TasDigest(load.source().sha256()),
        effective_media_sha256: TasDigest(load.effective_sha256()),
        current_state_sha256: TasDigest::from_bytes(&state),
        current_state_bytes: state,
        determinism_abi: MD_TAS_DETERMINISM_ABI,
        state_format_compatibility_id: MD_TAS_STATE_COMPATIBILITY,
        sync_config_sha256: TasDigest(load.sync_hash()),
    })
}

pub(crate) fn validate_md_tas_project_witness(
    project: &TasProject,
    branch: &str,
    witness: super::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_md_tas_branch_scope(project, branch)?;
    ensure!(
        classify_md_tas_project(project)? == witness.profile,
        "MD pad profile differs"
    );
    let identity = project.identity();
    ensure!(
        identity.source_media_sha256 == witness.source_media_sha256
            && identity.effective_media_sha256 == witness.effective_media_sha256
            && identity.determinism_abi == witness.determinism_abi
            && identity.state_format_compatibility_id == witness.state_format_compatibility_id
            && identity.sync_config_sha256 == witness.sync_config_sha256,
        "MD worker identity differs from project"
    );
    ensure!(
        !witness.current_state_bytes.is_empty()
            && TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "MD current-state observation digest differs"
    );
    Ok(())
}

pub(crate) fn observe_md_tas_profile(
    backend: &EmuBackend,
    cheats: bool,
    profile: TasExecutionProfile,
) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let load = match backend {
        EmuBackend::Md(md) => md.tas_load().ok(),
        _ => None,
    };
    let actual = loaded_md_tas_profile(backend);
    let eligible = actual.is_some();
    TasLoadedProfileObservation {
        profile,
        system: backend.system(),
        sync_config_sha256: load.map(|load| TasDigest(load.sync_hash())),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::Md.code())
            && metadata.core_family.as_deref() == Some("MegaDrive"),
        load_provenance_available: load.is_some(),
        direct_source: load.map(|_| eligible),
        source_media_sha256: load.map(|load| TasDigest(load.source().sha256())),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|_| true),
        persistent_state_absent: load.map(|_| true),
        project_owned_persistence: None,
        initial_input_neutral: load.map(|_| true),
        configured_at_load_sample_rate: None,
        initial_sample_rate: load.map(|_| 48_000),
        current_sample_rate: match backend {
            EmuBackend::Md(md) => Some(md.core().sample_rate()),
            _ => None,
        },
        firmware_profile_matches: metadata.firmware.is_empty(),
        hardware_profile_matches: eligible,
        controller_profile_matches: actual.is_some() && actual == md_pad_profile(profile),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats && metadata.cheat_sha256.is_none(),
    }
}

pub(crate) fn canonicalize_md_tas_host_input(
    profile: TasExecutionProfile,
    input: TasInputFrame,
) -> Result<TasInputFrame> {
    let profile =
        md_pad_profile(profile).ok_or_else(|| anyhow::anyhow!("not an MD TAS profile"))?;
    super::direct_md::canonicalize_local_host_input(profile, input)
}

pub(crate) fn validate_md_tas_project_input(
    profile: TasExecutionProfile,
    input: TasInputFrame,
) -> Result<()> {
    let profile =
        md_pad_profile(profile).ok_or_else(|| anyhow::anyhow!("not an MD TAS profile"))?;
    super::direct_md::validate_input(profile, input)
}

pub(crate) fn validate_md_tas_worker_input(
    profile: TasExecutionProfile,
    input: crate::emu_thread::TasInputFrame,
) -> Result<()> {
    let profile =
        md_pad_profile(profile).ok_or_else(|| anyhow::anyhow!("not an MD TAS profile"))?;
    ensure!(
        input.fds_disk_side.is_none()
            && input.fds_write_protected.is_none()
            && input.fds_media_event.is_none(),
        "MD TAS excludes FDS events"
    );
    let mut frame = TasInputFrame::default();
    for (player, (buttons, dpad)) in frame.players.iter_mut().zip([
        (input.p1_buttons, input.p1_dpad),
        (input.p2_buttons, input.p2_dpad),
        (input.p3_buttons, input.p3_dpad),
        (input.p4_buttons, input.p4_dpad),
        (input.p5_buttons, input.p5_dpad),
    ]) {
        player.buttons = buttons;
        player.dpad = dpad;
    }
    frame.coleco = input.coleco;
    frame.zapper.enabled = input.zapper.enabled;
    frame.zapper.trigger = input.zapper.trigger;
    frame.zapper.hit = input.zapper.hit;
    frame.zapper.screen_pos = input.zapper.screen_pos.map(|(x, y)| [x, y]);
    frame.tilt_x_bits = input.tilt_x_bits;
    frame.tilt_y_bits = input.tilt_y_bits;
    super::direct_md::validate_input(profile, frame)
}

pub(crate) fn md_source_extension(path: &Path) -> bool {
    super::super::md_tas::extension(path).is_some()
}

#[cfg(test)]
mod tests;
