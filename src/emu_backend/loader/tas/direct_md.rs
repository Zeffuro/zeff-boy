use anyhow::{Context, Result, ensure};

use super::{ActiveSystem, EmuBackend};
use crate::emu_backend::loader::md_tas::{
    MD_TAS_DETERMINISM_ABI, MD_TAS_STATE_COMPATIBILITY, MdTasProfile,
};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasInputFrame, TasProject,
    TasProjectIdentity,
};

fn devices(profile: MdTasProfile) -> Vec<TasDeviceIdentity> {
    ["p1", "p2"]
        .into_iter()
        .map(|port| TasDeviceIdentity {
            port: port.to_owned(),
            device: profile.device().to_owned(),
            configuration_sha256: TasDigest::from_bytes(profile.device().as_bytes()),
        })
        .collect()
}

pub(super) fn identity(backend: &EmuBackend, start_state: &[u8]) -> Result<TasProjectIdentity> {
    let EmuBackend::Md(md) = backend else {
        anyhow::bail!("MD TAS requires an MD backend")
    };
    let load = md.tas_load()?;
    load.validate_fresh(md)?;
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "MD TAS start bytes differ from the fresh normal-loaded baseline"
    );
    let metadata = backend.replay_metadata();
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::Md.code())
            && metadata.core_family.as_deref() == Some("MegaDrive")
            && metadata.rom_sha256 == Some(load.effective_sha256())
            && metadata.firmware.is_empty(),
        "MD backend metadata differs from loader-owned provenance"
    );
    Ok(TasProjectIdentity {
        system: ActiveSystem::Md.code().to_owned(),
        core_family: "MegaDrive".to_owned(),
        determinism_abi: MD_TAS_DETERMINISM_ABI.to_owned(),
        source_media_sha256: TasDigest(load.source().sha256()),
        effective_media_sha256: TasDigest(load.effective_sha256()),
        patches: Vec::new(),
        firmware: Vec::new(),
        devices: devices(load.profile()),
        sync_config_sha256: TasDigest(load.sync_hash()),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: MD_TAS_STATE_COMPATIBILITY.to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(super) fn project_profile(project: &TasProject) -> Result<MdTasProfile> {
    project.validate()?;
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::Md.code()
            && identity.core_family == "MegaDrive"
            && identity.determinism_abi == MD_TAS_DETERMINISM_ABI
            && identity.state_format_compatibility_id == MD_TAS_STATE_COMPATIBILITY
            && identity.patches.is_empty()
            && identity.firmware.is_empty()
            && identity.persistent_state == TasExternalIdentity::Absent
            && identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project does not identify the factory MD native profile"
    );
    [MdTasProfile::ThreeButton, MdTasProfile::SixButton]
        .into_iter()
        .find(|&profile| identity.devices == devices(profile))
        .context("MD TAS requires two equal Three or Six button pads")
}

pub(super) fn validate_input(profile: MdTasProfile, input: TasInputFrame) -> Result<()> {
    ensure!(
        input.players[..2].iter().all(|player| {
            player.buttons & !profile.button_mask() == 0 && player.dpad & !0x0f == 0
        }) && input.players[2..]
            .iter()
            .all(|player| *player == Default::default())
            && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
            && input.zapper == Default::default()
            && input.tilt_x_bits == 0
            && input.tilt_y_bits == 0
            && matches!(input.camera, TasCameraInput::None),
        "MD TAS input contains inactive controls or another device"
    );
    Ok(())
}

pub(super) fn canonicalize_local_host_input(
    profile: MdTasProfile,
    mut input: TasInputFrame,
) -> Result<TasInputFrame> {
    for player in &mut input.players[..2] {
        player.buttons &= profile.button_mask();
    }
    validate_input(profile, input)?;
    Ok(input)
}

pub(super) fn validate_branch_scope(project: &TasProject, branch_id: &str) -> Result<()> {
    let profile = project_profile(project)?;
    ensure!(
        project.replay_start() == &Default::default() && project.assets().is_empty(),
        "MD TAS excludes external replay start metadata and assets"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(branch.events().is_empty(), "MD TAS excludes replay events");
    for span in branch.input_spans() {
        validate_input(profile, span.input)?;
    }
    Ok(())
}
