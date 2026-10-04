use super::*;

fn options(arguments: &[&str]) -> Result<Options> {
    Options::parse(&arguments.iter().map(OsString::from).collect::<Vec<_>>())
}

#[test]
fn proof_options_keep_fixture_defaults_and_accept_explicit_cartridges() {
    for (arguments, frames, cartridge) in [
        (vec!["output"], 300, None),
        (vec!["output", "24"], 24, None),
        (
            vec!["output", "--rom", "a game.nes"],
            300,
            Some("a game.nes"),
        ),
        (
            vec!["output", "24", "--rom", "a game.nes"],
            24,
            Some("a game.nes"),
        ),
    ] {
        let parsed = options(&arguments).unwrap();
        assert_eq!(parsed.root, PathBuf::from("output"));
        assert_eq!(parsed.frames, frames);
        assert_eq!(parsed.cartridge, cartridge.map(PathBuf::from));
        assert_eq!(parsed.timing, TimingMode::Ntsc);
    }
    for (arguments, frames, timing) in [
        (vec!["output", "--timing", "pal"], 300, TimingMode::Pal),
        (vec!["output", "24", "--timing", "pal"], 24, TimingMode::Pal),
        (
            vec!["output", "24", "--timing", "dendy"],
            24,
            TimingMode::Dendy,
        ),
        (
            vec!["output", "24", "--timing", "ntsc"],
            24,
            TimingMode::Ntsc,
        ),
    ] {
        let parsed = options(&arguments).unwrap();
        assert_eq!(parsed.frames, frames);
        assert_eq!(parsed.timing, timing);
        assert!(parsed.cartridge.is_none());
    }
}

#[test]
fn proof_options_reject_missing_media_extra_arguments_and_invalid_frame_bounds() {
    for arguments in [
        vec![],
        vec!["output", "--rom"],
        vec!["output", "24", "--rom"],
        vec!["output", "24", "--rom", "game.nes", "extra"],
        vec!["output", "0"],
        vec!["output", "100001"],
        vec!["output", "invalid"],
        vec!["output", "--timing"],
        vec!["output", "--timing", "unknown"],
        vec!["output", "--rom", "game.nes", "--timing", "pal"],
    ] {
        assert!(options(&arguments).is_err(), "{arguments:?}");
    }
}

#[test]
fn cartridge_proof_rejects_archives_and_invalid_media_before_copying() {
    let directory = crate::test_support::test_directory("netplay-cartridge-input").unwrap();
    let path = directory.path().join("input.zip");
    std::fs::write(&path, zeff_netplay::fixture::rom()).unwrap();
    assert!(Media::cartridge(&path).is_err());
    let path = path.with_extension("nes");
    for bytes in [b"NES\x1a".as_slice(), &[0; 16]] {
        std::fs::write(&path, bytes).unwrap();
        assert!(Media::cartridge(&path).is_err());
    }
}

#[test]
fn cartridge_proof_refuses_existing_media_and_save_destinations() {
    let directory = crate::test_support::test_directory("netplay-cartridge-existing").unwrap();
    let media = Media::fixture();
    let path = directory.path().join("one.nes");
    std::fs::write(&path, b"existing media").unwrap();
    assert!(media.load(directory.path(), "one").is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"existing media");
    let path = directory.path().join("two.sav");
    std::fs::write(&path, b"existing save").unwrap();
    assert!(media.load(directory.path(), "two").is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"existing save");
    assert!(!directory.path().join("two.nes").exists());
}

#[test]
fn proof_refuses_archive_named_ancestors_before_copying_or_writing_saves() {
    let directory = crate::test_support::test_directory("netplay-proof-save-target").unwrap();
    for extension in ["zip", "7z", "rar", "ZIP"] {
        let ancestor = directory.path().join(format!("output.{extension}"));
        let root = ancestor.join("nested");
        std::fs::create_dir_all(&root).unwrap();
        let save = ancestor.with_extension("sav");
        std::fs::write(&save, b"existing player save").unwrap();
        let error = Media::fixture().load(&root, "one").err().unwrap();
        assert!(error.to_string().contains("archive-named"), "{error}");
        assert_eq!(std::fs::read(save).unwrap(), b"existing player save");
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
    }
}

#[test]
fn cartridge_proof_preserves_original_media_and_saves_with_and_without_battery() {
    for battery in [false, true] {
        let directory = crate::test_support::test_directory("netplay-cartridge-proof").unwrap();
        let source = directory.path().join("source.nes");
        let mut bytes = zeff_netplay::fixture::rom();
        if !battery {
            bytes[6] &= !2;
        }
        std::fs::write(&source, &bytes).unwrap();
        std::fs::write(source.with_extension("sav"), b"original player save").unwrap();
        let root = directory.path().join("proof");
        std::fs::create_dir(&root).unwrap();
        let report = run_media(&root, 24, [9; 32], &Media::cartridge(&source).unwrap()).unwrap();
        assert_eq!(report["frames"], 24);
        assert_eq!(report["pause_rounds"], 9);
        assert_eq!(report["battery"], battery);
        assert_eq!(report["save_policy"], "disabled");
        assert_eq!(
            report["source_sha256"],
            const_hex::encode(Sha256::digest(&bytes))
        );
        assert_eq!(report["exact_restore"], true);
        assert_eq!(report["delayed_reference"], true);
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert_eq!(
            std::fs::read(source.with_extension("sav")).unwrap(),
            b"original player save"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 3);
        for name in ["one", "two", "reference"] {
            assert_eq!(
                std::fs::read(root.join(format!("{name}.nes"))).unwrap(),
                bytes
            );
        }
    }
}

#[test]
fn cartridge_proof_keeps_pal_loader_timing_authoritative() {
    assert_cartridge_timing(TimingMode::Pal, "pal");
}

#[test]
fn cartridge_proof_keeps_dendy_loader_timing_authoritative() {
    assert_cartridge_timing(TimingMode::Dendy, "dendy");
}

fn assert_cartridge_timing(timing: TimingMode, label: &str) {
    let directory = crate::test_support::test_directory("netplay-cartridge-pal").unwrap();
    let source = directory.path().join("source.nes");
    let bytes = fixture_rom(timing);
    std::fs::write(&source, bytes).unwrap();
    let report = run_media(
        directory.path(),
        24,
        [9; 32],
        &Media::cartridge(&source).unwrap(),
    )
    .unwrap();
    assert_eq!(report["resolved_timing"], label);
    assert_eq!(report["compatibility_contract"], "00".repeat(32));
    assert_eq!(report["pause_rounds"], 9);
    assert_eq!(report["exact_restore"], true);
    assert_eq!(report["save_protection"], true);
    for name in ["source", "one", "two", "reference"] {
        assert!(!directory.path().join(format!("{name}.sav")).exists());
    }
}

#[test]
fn dendy_nes2_fixture_loads_declared_nonvolatile_ram_and_resolved_timing() {
    let directory = crate::test_support::test_directory("netplay-dendy-fixture").unwrap();
    let (_, backend) = Media::fixture_timing(TimingMode::Dendy)
        .load(directory.path(), "game")
        .unwrap();
    let emu = &backend.nes().unwrap().emu;
    let header = emu.cartridge_header();
    assert_eq!(
        header.format,
        zeff_nes_core::hardware::cartridge::RomFormat::Nes2
    );
    assert_eq!(header.mapper_id, 34);
    assert_eq!(header.submapper_id, 0);
    assert_eq!(header.prg_nvram_size, 8192);
    assert_eq!(emu.resolved_timing_mode(), TimingMode::Dendy);
    assert_eq!(emu.dump_persistent_data().unwrap().len(), 8192);
    assert!(identity::identity(&backend, [9; 32]).is_ok());
}
