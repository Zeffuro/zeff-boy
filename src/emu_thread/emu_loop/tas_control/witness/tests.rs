use super::*;
use crate::emu_backend::nes::{NesPersistentLoadOutcome, NesTasInitialInput};
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};

fn valid_facts() -> DirectNesProfileFacts {
    DirectNesProfileFacts {
        system: ActiveSystem::Nes,
        identity_metadata_matches: true,
        provenance: Some(NesTasLoadProvenance {
            raw_source_media_sha256: [7; 32],
            direct_nes_file: true,
            any_mod_enabled: false,
            any_mod_applied: false,
            persistent_load: NesPersistentLoadOutcome::Absent,
            initial_input: NesTasInitialInput {
                buttons: 0,
                dpad: 0,
            },
            configured_sample_rate: None,
            initial_sample_rate: 48_000,
        }),
        current_sample_rate: Some(48_000),
        effective_media_sha256: [7; 32],
        firmware_present: false,
        standard_console_hardware: true,
        supported_controller_topology: true,
        removable_media_present: false,
        cheats_present: false,
    }
}

fn rejection(facts: &DirectNesProfileFacts) -> Rejected {
    validate_direct_nes_profile(facts).unwrap_err()
}

#[test]
fn unsupported_system_is_typed() {
    let mut facts = valid_facts();
    facts.system = ActiveSystem::GameBoy;
    assert_eq!(rejection(&facts), Rejected::UnsupportedSystem);
}

#[test]
fn missing_load_provenance_is_typed() {
    let mut facts = valid_facts();
    facts.provenance = None;
    assert_eq!(rejection(&facts), Rejected::LoadProvenanceUnavailable);
}

#[test]
fn identity_metadata_mismatch_is_typed() {
    let mut facts = valid_facts();
    facts.identity_metadata_matches = false;
    assert_eq!(rejection(&facts), Rejected::IdentityMetadataMismatch);
}

#[test]
fn non_direct_source_is_typed() {
    let mut facts = valid_facts();
    facts.provenance.as_mut().unwrap().direct_nes_file = false;
    assert_eq!(rejection(&facts), Rejected::DirectNesFileRequired);
}

#[test]
fn source_effective_media_mismatch_is_typed() {
    let mut facts = valid_facts();
    facts.effective_media_sha256[0] ^= 1;
    assert_eq!(rejection(&facts), Rejected::SourceMediaMismatch);
}

#[test]
fn enabled_or_applied_mods_are_typed() {
    for (enabled, applied) in [(true, false), (false, true)] {
        let mut facts = valid_facts();
        let provenance = facts.provenance.as_mut().unwrap();
        provenance.any_mod_enabled = enabled;
        provenance.any_mod_applied = applied;
        assert_eq!(rejection(&facts), Rejected::ModsEnabledOrApplied);
    }
}

#[test]
fn loaded_or_unknown_persistent_state_is_typed() {
    for outcome in [
        NesPersistentLoadOutcome::Loaded,
        NesPersistentLoadOutcome::Unknown,
    ] {
        let mut facts = valid_facts();
        facts.provenance.as_mut().unwrap().persistent_load = outcome;
        assert_eq!(rejection(&facts), Rejected::PersistentStateNotAbsent);
    }
}

#[test]
fn non_neutral_initial_input_is_typed() {
    for input in [(1, 0), (0, 1)] {
        let mut facts = valid_facts();
        facts.provenance.as_mut().unwrap().initial_input = NesTasInitialInput {
            buttons: input.0,
            dpad: input.1,
        };
        assert_eq!(rejection(&facts), Rejected::NonNeutralInitialInput);
    }
}

#[test]
fn every_non_default_sample_rate_fact_is_typed() {
    let mut configured = valid_facts();
    configured
        .provenance
        .as_mut()
        .unwrap()
        .configured_sample_rate = Some(44_100);
    assert_eq!(rejection(&configured), Rejected::NonDefaultSampleRate);

    let mut initial = valid_facts();
    initial.provenance.as_mut().unwrap().initial_sample_rate = 44_100;
    assert_eq!(rejection(&initial), Rejected::NonDefaultSampleRate);

    let mut current = valid_facts();
    current.current_sample_rate = Some(44_100);
    assert_eq!(rejection(&current), Rejected::NonDefaultSampleRate);
}

#[test]
fn firmware_is_typed() {
    let mut facts = valid_facts();
    facts.firmware_present = true;
    assert_eq!(rejection(&facts), Rejected::FirmwarePresent);
}

#[test]
fn non_standard_controller_topology_is_typed() {
    let mut facts = valid_facts();
    facts.supported_controller_topology = false;
    assert_eq!(rejection(&facts), Rejected::NonStandardControllerTopology);
}

#[test]
fn non_standard_console_hardware_is_typed() {
    let mut facts = valid_facts();
    facts.standard_console_hardware = false;
    assert_eq!(rejection(&facts), Rejected::NonStandardConsoleHardware);
}

#[test]
fn removable_media_is_typed() {
    let mut facts = valid_facts();
    facts.removable_media_present = true;
    assert_eq!(rejection(&facts), Rejected::RemovableMediaPresent);
}

#[test]
fn cheats_are_typed() {
    let mut facts = valid_facts();
    facts.cheats_present = true;
    assert_eq!(rejection(&facts), Rejected::CheatsPresent);
}

#[test]
fn state_capture_failure_or_frame_mutation_is_typed() {
    let capture = capture_current_state(|| 9, || anyhow::bail!("encode failed"));
    assert_eq!(capture.unwrap_err(), Rejected::StateWitnessUnavailable);

    let mut frames = [9, 10].into_iter();
    let capture = capture_current_state(|| frames.next().unwrap(), || Ok(vec![1, 2, 3]));
    assert_eq!(capture.unwrap_err(), Rejected::StateWitnessUnavailable);
}

#[test]
fn direct_file_witness_reports_zapper_topology() {
    let directory = crate::test_support::test_directory("tas-witness-zapper").unwrap();
    let rom_path = directory.path().join("game.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;

    assert!(build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).is_ok());
    backend.set_zapper_state(true, false, false, None);
    assert!(build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).is_ok());
}
