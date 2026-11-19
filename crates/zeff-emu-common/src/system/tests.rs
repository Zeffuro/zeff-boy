use super::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[test]
fn detects_supported_rom_extensions() {
    assert_eq!(
        System::from_path(&PathBuf::from("GAME.GB")),
        Some(System::Gb)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.gbc")),
        Some(System::Gb)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.sgb")),
        Some(System::Gb)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.gba")),
        Some(System::Gba)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.nes")),
        Some(System::Nes)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.fds")),
        Some(System::Nes)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.col")),
        Some(System::Coleco)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.pce")),
        Some(System::Pce)
    );
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(
        System::from_path(&PathBuf::from("game.cue")),
        Some(System::Pce)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.chd")),
        Some(System::Pce)
    );
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(
        System::from_path(&PathBuf::from("game.iso")),
        Some(System::Pce)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.ws")),
        Some(System::Ws)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.wsc")),
        Some(System::Ws)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.sms")),
        Some(System::Sms)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.gg")),
        Some(System::Gg)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.sg")),
        Some(System::Sg)
    );
    assert_eq!(
        System::from_path(&PathBuf::from("game.sc")),
        Some(System::Sg)
    );
}

#[test]
fn package_archive_is_native_only_and_never_a_rom_extension() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        assert_eq!(archive_extensions(), &["zip", "7z", "rar"]);
        assert!(ROM_AND_ARCHIVE_EXTENSIONS.contains(&"7z"));
        assert!(ROM_AND_ARCHIVE_EXTENSIONS.contains(&"rar"));
    }
    #[cfg(target_arch = "wasm32")]
    {
        assert_eq!(archive_extensions(), &["zip"]);
        assert!(!ROM_AND_ARCHIVE_EXTENSIONS.contains(&"7z"));
    }
    assert!(!ROM_EXTENSIONS.contains(&"7z"));
    assert!(!ROM_EXTENSIONS.contains(&"rar"));
    assert_eq!(System::from_path(Path::new("disc.7z")), None);
}

#[test]
fn specs_cover_supported_rom_extensions() {
    let from_specs = system_specs()
        .iter()
        .flat_map(|spec| spec.rom_extensions.iter().copied())
        .collect::<BTreeSet<_>>();
    let from_constant = ROM_EXTENSIONS.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(from_specs, from_constant);
    for spec in system_specs() {
        for extension in spec.rom_extensions {
            assert_eq!(System::from_extension(extension), Some(spec.system));
        }
        assert_eq!(System::from_code(spec.code), Some(spec.system));
        assert!(!spec.storage_subdir.is_empty());
        assert!(!spec.state_extension.is_empty());
        assert!(!spec.file_dialog_filter_name.is_empty());
        assert!(spec.frame_duration_ns > 0);
        assert!(spec.target_fps() > 0.0);
        assert_eq!(
            spec.framebuffer_len(),
            rgba_framebuffer_len(spec.screen_size)
        );
    }
}

#[test]
fn app_aliases_match_romtest_style_variants() {
    assert_eq!(System::GameBoy, System::Gb);
    assert_eq!(System::GameBoyAdvance, System::Gba);
    assert_eq!(System::WonderSwan, System::Ws);
    assert_eq!(System::PcEngine, System::Pce);
    assert_eq!(System::MasterSystem, System::Sms);
    assert_eq!(System::GameGear, System::Gg);
    assert_eq!(System::Sg1000, System::Sg);
}

#[test]
fn systems_map_to_expected_core_families() {
    assert_eq!(System::Gb.core_family(), CoreFamily::GameBoy);
    assert_eq!(System::Gba.core_family(), CoreFamily::GameBoyAdvance);
    assert_eq!(System::Nes.core_family(), CoreFamily::Nes);
    assert_eq!(System::Coleco.core_family(), CoreFamily::ColecoVision);
    assert_eq!(System::Pce.core_family(), CoreFamily::PcEngine);
    assert_eq!(System::Ws.core_family(), CoreFamily::WonderSwan);
    assert_eq!(System::Sms.core_family(), CoreFamily::Sega8);
    assert_eq!(System::Gg.core_family(), CoreFamily::Sega8);
    assert_eq!(System::Sg.core_family(), CoreFamily::Sega8);
}

#[test]
fn pce_uses_the_provisional_263_line_nominal_frame_duration() {
    assert_eq!(
        System::Pce.frame_duration_ns(),
        PROVISIONAL_PCE_263_LINE_FRAME_DURATION_NS
    );
    assert_eq!(PROVISIONAL_PCE_263_LINE_FRAME_DURATION_NS, 16_715_111);
}

#[test]
fn coleco_uses_the_ntsc_z80_cycle_derived_nominal_frame_duration() {
    assert_eq!(COLECO_NTSC_Z80_CLOCK_HZ, 3_579_545);
    assert_eq!(COLECO_NTSC_Z80_CYCLES_PER_FRAME, 59_736);
    assert_eq!(
        COLECO_NTSC_FRAME_DURATION_NS,
        (COLECO_NTSC_Z80_CYCLES_PER_FRAME * 1_000_000_000 + COLECO_NTSC_Z80_CLOCK_HZ / 2)
            / COLECO_NTSC_Z80_CLOCK_HZ
    );
    assert_eq!(COLECO_NTSC_FRAME_DURATION_NS, 16_688_155);
    assert_eq!(
        System::Coleco.frame_duration_ns(),
        COLECO_NTSC_FRAME_DURATION_NS
    );
}

#[test]
fn mega_drive_registry_keeps_binary_media_ambiguous() {
    for extension in ["md", "GEN"] {
        assert_eq!(System::from_extension(extension), Some(System::Md));
    }
    assert_eq!(System::from_extension("bin"), None);
    assert!(!ROM_EXTENSIONS.contains(&"bin"));
    assert!(ROM_AND_ARCHIVE_EXTENSIONS.contains(&"bin"));
    assert_eq!(System::Md.core_family(), CoreFamily::MegaDrive);
    assert_eq!(System::Md.storage_subdir(), "md");
    assert_eq!(System::Md.state_extension(), "mdstate");
    assert_eq!(System::Md.screen_size(), (320, 240));
    assert_eq!(System::Md.framebuffer_len(), 320 * 240 * 4);
    assert_eq!(System::Md.frame_duration_ns(), MD_NTSC_FRAME_DURATION_NS);
    assert_eq!(MD_NTSC_FRAME_DURATION_NS, 16_688_155);
    assert_eq!(System::from_code("md"), Some(System::Md));
    let states = system_specs()
        .iter()
        .map(|spec| spec.state_extension)
        .collect::<BTreeSet<_>>();
    assert_eq!(states.len(), system_specs().len());
}

#[test]
fn binary_console_header_probe_is_bounded_and_preserves_other_systems() {
    let mut bytes = [0; MEDIA_PROBE_LEN + 16];
    for console in [
        b"SEGA MEGA DRIVE ",
        b"SEGA GENESIS    ",
        b"SEGA            ",
    ] {
        bytes[0x100..MEDIA_PROBE_LEN].copy_from_slice(console);
        assert_eq!(
            System::from_media(Path::new("game.BIN"), &bytes),
            Some(System::Md)
        );
        assert_eq!(
            System::from_media(Path::new("game.nes"), &bytes),
            Some(System::Nes)
        );
        assert_eq!(
            System::from_media(Path::new("game.bin"), &bytes[..MEDIA_PROBE_LEN - 1]),
            None
        );
    }
    for console in [
        b"SEGA 32X        ",
        b"SEGA CD         ",
        b"UNRELATED       ",
    ] {
        bytes[0x100..MEDIA_PROBE_LEN].copy_from_slice(console);
        assert_eq!(System::from_media(Path::new("track.bin"), &bytes), None);
    }
    bytes.fill(0);
    bytes[MEDIA_PROBE_LEN..].copy_from_slice(b"SEGA MEGA DRIVE ");
    assert_eq!(System::from_media(Path::new("track.bin"), &bytes), None);
    assert_eq!(
        System::from_media(Path::new("game.md"), &[]),
        Some(System::Md)
    );
    assert_eq!(System::from_media(Path::new("game.rom"), &bytes), None);
}
