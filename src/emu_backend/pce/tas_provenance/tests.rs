use std::fs;

use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};

fn setup() -> PceTasLoadSetup {
    PceTasLoadSetup {
        loaded_from_source_path: true,
        any_mod_enabled: false,
        any_mod_applied: false,
        initial_input: None,
        configured_sample_rate: None,
        selected_wiring: None,
        selected_board: None,
        selected_hardware: None,
        selected_controller_mode: PceControllerMode::Automatic,
        selected_memory_base_mode: PceMemoryBaseMode::Automatic,
        selected_arcade_card_mode: PceArcadeCardMode::Automatic,
        tas_source_media: None,
    }
}

fn cd_media(
    cdda_source: Option<PceTasCdSource>,
    selected_member: Option<[u8; 32]>,
    explicitly_selected: bool,
) -> PceTasCdLoadMedia {
    PceTasCdLoadMedia {
        raw_source_media_sha256: [3; 32],
        raw_source_media_len: 0x2000,
        source_disc_sha256: [5; 32],
        effective_disc_sha256: [7; 32],
        cdda_source,
        cdda_selected_member_path_sha256: selected_member,
        cdda_selected_member_explicitly_selected: explicitly_selected,
        archive_ppf_patches: Vec::new(),
    }
}

#[test]
fn qualified_cd_sources_keep_cdda_labels_and_tas_witnesses() {
    let sources = [
        (
            PceTasCdSource::DirectCue,
            "cue",
            false,
            false,
            false,
            false,
            false,
            false,
        ),
        (
            PceTasCdSource::DirectCuePpf,
            "cue_ppf",
            false,
            false,
            true,
            false,
            false,
            false,
        ),
        (
            PceTasCdSource::DirectChd,
            "chd",
            true,
            false,
            false,
            false,
            false,
            false,
        ),
        (
            PceTasCdSource::DirectIsoCue,
            "iso_cue",
            false,
            true,
            false,
            false,
            false,
            false,
        ),
        (
            PceTasCdSource::ArchiveCue(PceTasCdArchiveCarrier::SevenZip),
            "archive_cue",
            false,
            false,
            false,
            true,
            false,
            false,
        ),
        (
            PceTasCdSource::ArchiveCue(PceTasCdArchiveCarrier::Rar),
            "rar_cue",
            false,
            false,
            false,
            false,
            true,
            false,
        ),
        (
            PceTasCdSource::ArchiveCue(PceTasCdArchiveCarrier::Zip),
            "zip_cue",
            false,
            false,
            false,
            false,
            false,
            true,
        ),
        (
            PceTasCdSource::ArchiveCuePpf(PceTasCdArchiveCarrier::Zip),
            "archive_cue_ppf",
            false,
            false,
            false,
            false,
            false,
            true,
        ),
    ];
    for (index, (source, label, chd, iso, ppf, archive, rar, zip)) in
        sources.into_iter().enumerate()
    {
        let member = source.archive_carrier().map(|_| [index as u8; 32]);
        let seed = PceTasLoadProvenanceSeed::new_cd(cd_media(Some(source), member, true), setup());

        assert_eq!(source.cdda_label(), label);
        assert_eq!(seed.cdda_source, Some(source));
        assert_eq!(seed.cdda_selected_member_path_sha256, member);
        assert!(seed.direct_pce_cd);
        assert_eq!(seed.direct_pce_cd_chd, chd);
        assert_eq!(seed.direct_pce_cd_iso, iso);
        assert_eq!(seed.direct_pce_cd_ppf, ppf);
        assert_eq!(
            seed.direct_pce_cd_archive_ppf,
            matches!(source, PceTasCdSource::ArchiveCuePpf(_))
        );
        assert_eq!(seed.direct_pce_cd_archive, archive);
        assert_eq!(seed.direct_pce_cd_rar, rar);
        assert_eq!(seed.direct_pce_cd_zip, zip);
        assert_eq!(
            seed.archive_cue_member_path_sha256,
            archive.then_some(member).flatten()
        );
        assert_eq!(
            seed.rar_cue_member_path_sha256,
            rar.then_some(member).flatten()
        );
        assert_eq!(
            seed.zip_cue_member_path_sha256,
            zip.then_some(member).flatten()
        );
        assert_eq!(
            seed.archive_cue_explicitly_selected,
            archive && member.is_some()
        );
        assert_eq!(seed.rar_cue_explicitly_selected, rar && member.is_some());
        assert_eq!(seed.zip_cue_explicitly_selected, zip && member.is_some());
    }
}

#[test]
fn unqualified_archives_do_not_gain_cdda_or_tas_admission() {
    let seed = PceTasLoadProvenanceSeed::new_cd(cd_media(None, None, false), setup());

    assert_eq!(seed.cdda_source, None);
    assert_eq!(seed.cdda_selected_member_path_sha256, None);
    assert!(!seed.direct_pce_cd);
    assert!(!seed.direct_pce_cd_archive);
    assert!(!seed.direct_pce_cd_rar);
    assert!(!seed.direct_pce_cd_zip);
    assert_eq!(seed.raw_source_media_sha256, [3; 32]);
    assert_eq!(seed.raw_source_media_len, 0x2000);
}

#[test]
fn direct_cd_sources_cannot_retain_an_archive_member() {
    let seed = PceTasLoadProvenanceSeed::new_cd(
        cd_media(Some(PceTasCdSource::DirectCue), Some([9; 32]), true),
        setup(),
    );

    assert_eq!(seed.cdda_selected_member_path_sha256, None);
    assert_eq!(seed.archive_cue_member_path_sha256, None);
    assert!(!seed.archive_cue_explicitly_selected);
}

#[test]
fn archive_ppf_carriers_keep_tas_witnesses_and_one_cdda_member() {
    for (carrier, archive, rar, zip) in [
        (PceTasCdArchiveCarrier::SevenZip, true, false, false),
        (PceTasCdArchiveCarrier::Rar, false, true, false),
        (PceTasCdArchiveCarrier::Zip, false, false, true),
    ] {
        let member = [u8::from(archive) + 2 * u8::from(rar) + 3 * u8::from(zip); 32];
        let seed = PceTasLoadProvenanceSeed::new_cd(
            cd_media(
                Some(PceTasCdSource::ArchiveCuePpf(carrier)),
                Some(member),
                true,
            ),
            setup(),
        );

        assert_eq!(seed.cdda_source.unwrap().cdda_label(), "archive_cue_ppf");
        assert_eq!(seed.cdda_selected_member_path_sha256, Some(member));
        assert!(seed.direct_pce_cd);
        assert!(seed.direct_pce_cd_archive_ppf);
        assert_eq!(seed.direct_pce_cd_archive, archive);
        assert_eq!(seed.direct_pce_cd_rar, rar);
        assert_eq!(seed.direct_pce_cd_zip, zip);
        assert_eq!(
            seed.archive_cue_member_path_sha256,
            archive.then_some(member)
        );
        assert_eq!(seed.rar_cue_member_path_sha256, rar.then_some(member));
        assert_eq!(seed.zip_cue_member_path_sha256, zip.then_some(member));
    }
}

#[test]
fn seed_accepts_only_a_direct_pce_file() {
    let path = Path::new("game.pce");
    let backend = PceBackend::new(vec![0; 0x2000], path.to_path_buf()).unwrap();
    let direct = PceTasLoadProvenanceSeed::new([3; 32], 0x2000, path, path, setup())
        .finish(&backend, PceTasPersistentLoadOutcome::Absent);
    assert!(direct.direct_pce_file);

    let archive = Path::new("game.zip");
    let nested = Path::new("game.pce");
    let rejected = PceTasLoadProvenanceSeed::new([3; 32], 0x2000, archive, nested, setup())
        .finish(&backend, PceTasPersistentLoadOutcome::Unknown);
    assert!(!rejected.direct_pce_file);
}

#[test]
fn shared_loader_retains_raw_and_effective_hucard_facts() {
    let dir = crate::test_support::test_directory("pce-tas-provenance").unwrap();
    let path = dir.path().join("synthetic.pce");
    let mut raw = vec![0; 512];
    raw[0] = 1;
    raw.extend(vec![0xEA; 0x2000]);
    fs::write(&path, &raw).unwrap();

    let loaded = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &path,
        &path,
        None,
        BackendLoadConfig {
            sample_rate: Some(48_000),
            initial_input: Some((0x01, 0x01)),
            pce_console_wiring: Some(PceConsoleWiring::PcEngine),
            pce_hucard_board: Some(PceHuCardBoard::Plain),
            pce_cartridge_hardware: Some(zeff_pce_core::hardware::PceCartridgeHardware::Base),
            pce_arcade_card_mode: PceArcadeCardMode::Disabled,
            pce_load_battery_bram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();
    let crate::emu_backend::EmuBackend::Pce(backend) = loaded.backend else {
        panic!("PC Engine loader returned a different backend");
    };
    let view = backend.tas_load_provenance().unwrap();
    let provenance = view.load;

    assert!(provenance.direct_pce_file);
    assert_eq!(
        provenance.raw_source_media_sha256,
        zeff_firmware::sha256_bytes(&raw)
    );
    assert_eq!(provenance.raw_source_media_len, raw.len());
    assert_eq!(
        provenance.persistent_load,
        PceTasPersistentLoadOutcome::Skipped
    );
    assert_eq!(provenance.initial_input, Some((0x01, 0x01)));
    assert_eq!(provenance.configured_sample_rate, Some(48_000));
    assert_eq!(provenance.initial_sample_rate, 48_000);
    assert_eq!(view.current_sample_rate, 48_000);
    assert_eq!(backend.pce_sample_rate(), 48_000);
    assert_eq!(provenance.selected_wiring, Some(PceConsoleWiring::PcEngine));
    assert_eq!(provenance.effective_wiring, PceConsoleWiring::PcEngine);
    assert_eq!(provenance.selected_board, Some(PceHuCardBoard::Plain));
    assert_eq!(provenance.effective_board, PceHuCardBoard::Plain);
    assert_eq!(provenance.effective_topology, PceHardwareTopology::Base);
    assert_eq!(
        provenance.effective_controller_mode,
        PceControllerMode::TwoButton
    );
    assert_eq!(
        provenance.effective_memory_base_mode,
        PceMemoryBaseMode::Disabled
    );
    assert_eq!(
        provenance.effective_arcade_card_mode,
        PceArcadeCardMode::Disabled
    );
    assert_eq!(
        backend.tas_source_media_identity(),
        Some(TasSourceMediaIdentity::new(
            zeff_firmware::sha256_bytes(&raw),
            raw.len(),
        ))
    );
}

#[test]
fn persistence_outcomes_fail_closed() {
    assert_eq!(
        pce_persistent_load_outcome(&Ok(Some("memory-base.bin".to_owned()))),
        PceTasPersistentLoadOutcome::Loaded
    );
    assert_eq!(
        pce_persistent_load_outcome(&Ok(None)),
        PceTasPersistentLoadOutcome::Absent
    );
    assert_eq!(
        pce_persistent_load_outcome(&Err(anyhow::anyhow!("load failed"))),
        PceTasPersistentLoadOutcome::Unknown
    );
}
