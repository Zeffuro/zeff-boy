use zeff_emu_common::replay::ReplayStartMetadata;

use super::*;
use crate::tas_project::{TasInitialBranch, TasPatchIdentity};

fn synthetic_nes_rom() -> Vec<u8> {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg] = 0xEA;
    rom[prg + 1] = 0x4C;
    rom[prg + 2] = 0x00;
    rom[prg + 3] = 0x80;
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn project_and_distinct_current_state() -> Result<(TasProject, Vec<u8>)> {
    let directory = crate::test_support::test_directory("tas-live-binding")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let (mut backend, _) = loader.load_fresh_backend()?;
    backend.step_frame();
    let current_state = backend.encode_state_bytes()?;
    assert_ne!(current_state, project.start_state());
    Ok((project, current_state))
}

fn validate(project: &TasProject, current_state: &[u8]) -> Result<()> {
    let identity = project.identity();
    validate_direct_nes_tas_project_witness(
        project,
        project.active_branch_id(),
        DirectNesTasRuntimeWitness {
            source_media_sha256: identity.source_media_sha256,
            effective_media_sha256: identity.effective_media_sha256,
            current_state_bytes: current_state,
            current_state_sha256: TasDigest::from_bytes(current_state),
            determinism_abi: &identity.determinism_abi,
            state_format_compatibility_id: &identity.state_format_compatibility_id,
            sync_config_sha256: identity.sync_config_sha256,
        },
    )
}

fn rebuild(project: &TasProject, identity: TasProjectIdentity) -> Result<TasProject> {
    let branch = &project.branches()[0];
    TasProject::new(
        project.project_id(),
        identity,
        project.start_state().to_vec(),
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: branch.id().to_owned(),
            name: branch.name().to_owned(),
            frame_count: branch.frame_count(),
            input_spans: branch.input_spans().to_vec(),
            events: branch.events().to_vec(),
        },
        Default::default(),
    )
}

#[test]
fn binding_accepts_distinct_current_state_and_unrelated_worker_frame_position() -> Result<()> {
    let (project, current_state) = project_and_distinct_current_state()?;

    validate(&project, &current_state)
}

#[test]
fn binding_rejects_static_direct_profile_divergence() -> Result<()> {
    let (project, current_state) = project_and_distinct_current_state()?;
    let mut identities = Vec::new();
    let mut identity = project.identity().clone();
    identity.system = "gb".to_owned();
    identities.push(identity);
    let mut identity = project.identity().clone();
    identity.patches.push(TasPatchIdentity {
        format: "ips".to_owned(),
        sha256: TasDigest([0x51; 32]),
    });
    identities.push(identity);
    let mut identity = project.identity().clone();
    identity.devices.pop();
    identities.push(identity);
    let mut identity = project.identity().clone();
    identity.persistent_state = TasExternalIdentity::ExternalSha256(TasDigest([0x52; 32]));
    identities.push(identity);

    for identity in identities {
        assert!(validate(&rebuild(&project, identity)?, &current_state).is_err());
    }
    Ok(())
}

#[test]
fn binding_rejects_variable_worker_witness_divergence() -> Result<()> {
    let (project, current_state) = project_and_distinct_current_state()?;
    let identity = project.identity();
    let mismatches = [
        DirectNesTasRuntimeWitness {
            source_media_sha256: TasDigest([0x61; 32]),
            effective_media_sha256: identity.effective_media_sha256,
            current_state_bytes: &current_state,
            current_state_sha256: TasDigest::from_bytes(&current_state),
            determinism_abi: &identity.determinism_abi,
            state_format_compatibility_id: &identity.state_format_compatibility_id,
            sync_config_sha256: identity.sync_config_sha256,
        },
        DirectNesTasRuntimeWitness {
            source_media_sha256: identity.source_media_sha256,
            effective_media_sha256: TasDigest([0x64; 32]),
            current_state_bytes: &current_state,
            current_state_sha256: TasDigest::from_bytes(&current_state),
            determinism_abi: &identity.determinism_abi,
            state_format_compatibility_id: &identity.state_format_compatibility_id,
            sync_config_sha256: identity.sync_config_sha256,
        },
        DirectNesTasRuntimeWitness {
            source_media_sha256: identity.source_media_sha256,
            effective_media_sha256: identity.effective_media_sha256,
            current_state_bytes: &current_state,
            current_state_sha256: TasDigest([0x62; 32]),
            determinism_abi: &identity.determinism_abi,
            state_format_compatibility_id: &identity.state_format_compatibility_id,
            sync_config_sha256: identity.sync_config_sha256,
        },
        DirectNesTasRuntimeWitness {
            source_media_sha256: identity.source_media_sha256,
            effective_media_sha256: identity.effective_media_sha256,
            current_state_bytes: &current_state,
            current_state_sha256: TasDigest::from_bytes(&current_state),
            determinism_abi: &identity.determinism_abi,
            state_format_compatibility_id: "wrong-state-format",
            sync_config_sha256: identity.sync_config_sha256,
        },
        DirectNesTasRuntimeWitness {
            source_media_sha256: identity.source_media_sha256,
            effective_media_sha256: identity.effective_media_sha256,
            current_state_bytes: &current_state,
            current_state_sha256: TasDigest::from_bytes(&current_state),
            determinism_abi: "wrong-abi",
            state_format_compatibility_id: &identity.state_format_compatibility_id,
            sync_config_sha256: identity.sync_config_sha256,
        },
        DirectNesTasRuntimeWitness {
            source_media_sha256: identity.source_media_sha256,
            effective_media_sha256: identity.effective_media_sha256,
            current_state_bytes: &current_state,
            current_state_sha256: TasDigest::from_bytes(&current_state),
            determinism_abi: &identity.determinism_abi,
            state_format_compatibility_id: &identity.state_format_compatibility_id,
            sync_config_sha256: TasDigest([0x63; 32]),
        },
    ];
    for witness in mismatches {
        assert!(
            validate_direct_nes_tas_project_witness(&project, project.active_branch_id(), witness)
                .is_err()
        );
    }
    Ok(())
}
