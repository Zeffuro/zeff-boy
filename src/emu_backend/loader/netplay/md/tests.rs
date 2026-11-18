#[cfg(target_arch = "wasm32")]
use super::super::NetplayRomMedia;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::netplay::md_fixture as fixture;
#[cfg(target_arch = "wasm32")]
use std::path::Path;
use zeff_md_core::{ControllerKind, Region, TimingModel};

fn config() -> BackendLoadConfig {
    fixture::config(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    )
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn md_direct_and_selected_zip_provenance_authenticates_actual_unmodified_selected_bytes() {
    let (directory, backend) = fixture::loaded(config());
    assert!(fixture::md(&backend).netplay_admission().is_ok());
    let path = directory.path().join("actual.md");
    let preloaded = load_backend_from_rom_source(
        ActiveSystem::Md,
        &path,
        &path,
        Some(fixture::rom()),
        config(),
    )
    .unwrap()
    .backend;
    assert!(fixture::md(&preloaded).netplay_admission().is_err());
    let zip = directory.path().join("selected.zip");
    let bytes = fixture::rom();
    crate::test_support::write_zip(&zip, &[("one/game.md", &bytes), ("other/game.md", &bytes)])
        .unwrap();
    let selected = crate::rom_archive::extract_bounded_zip_member(
        &zip,
        Some(&zip.join("one/game.md")),
        "md",
        128 * 1024 * 1024,
        4 * 1024 * 1024,
    )
    .unwrap();
    let zip_backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        &zip,
        &selected.rom_path,
        Some(selected.bytes.clone()),
        config(),
    )
    .unwrap()
    .backend;
    assert!(fixture::md(&zip_backend).netplay_admission().is_ok());
    let mut changed = selected.bytes;
    changed[0x3ff] ^= 1;
    let bad = load_backend_from_rom_source(
        ActiveSystem::Md,
        &zip,
        &selected.rom_path,
        Some(changed),
        config(),
    )
    .unwrap()
    .backend;
    assert!(fixture::md(&bad).netplay_admission().is_err());
    let missing = load_backend_from_rom_source(
        ActiveSystem::Md,
        &zip,
        &zip.join("missing.md"),
        Some(bytes),
        config(),
    )
    .unwrap()
    .backend;
    assert!(fixture::md(&missing).netplay_admission().is_err());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn md_browser_selected_media_requires_current_byte_digest_length_and_safe_path() {
    let bytes = fixture::rom();
    let direct = fixture::loaded(config());
    assert!(fixture::md(&direct).netplay_admission().is_ok());
    for (source, selected, media) in [
        ("direct.md", "direct.md", None),
        (
            "selected.zip",
            "selected.zip/../game.md",
            NetplayRomMedia::browser(&bytes),
        ),
        (
            "selected.zip",
            "elsewhere/game.md",
            NetplayRomMedia::browser(&bytes),
        ),
        (
            "direct.md",
            "direct.md",
            NetplayRomMedia::browser(&bytes[..bytes.len() - 1]),
        ),
    ] {
        let mut config = config();
        config.netplay_browser_media = media;
        let loaded = load_backend_from_rom_source(
            ActiveSystem::Md,
            Path::new(source),
            Path::new(selected),
            Some(bytes.clone()),
            config,
        )
        .unwrap()
        .backend;
        assert!(fixture::md(&loaded).netplay_admission().is_err());
    }
    let mut config = config();
    config.netplay_browser_media = NetplayRomMedia::browser(&bytes);
    let selected = load_backend_from_rom_source(
        ActiveSystem::Md,
        Path::new("selected.zip"),
        Path::new("selected.zip/folder/game.md"),
        Some(bytes),
        config,
    )
    .unwrap()
    .backend;
    assert!(fixture::md(&selected).netplay_admission().is_ok());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_loaded_factory_witness_refuses_actual_state_changes_without_normalizing_them() {
    for change in 0..6 {
        #[cfg(not(target_arch = "wasm32"))]
        let (_directory, mut backend) = fixture::loaded(config());
        #[cfg(target_arch = "wasm32")]
        let mut backend = fixture::loaded(config());
        assert!(fixture::md(&backend).netplay_admission().is_ok());
        match change {
            0 => backend.set_input(0x04, 0),
            1 => backend.set_sample_rate(44_100),
            2 => backend.set_apu_channel_mutes(&[true; 10]),
            3 => backend.set_apu_sample_generation_enabled(false),
            4 => backend.step_frame(),
            5 => {
                let EmuBackend::Md(md) = &mut backend else {
                    unreachable!()
                };
                md.set_controllers(ControllerKind::SixButton);
            }
            _ => unreachable!(),
        }
        let before = fixture::md(&backend).core().clone();
        let address = fixture::md(&backend).core() as *const _ as usize;
        assert!(fixture::md(&backend).netplay_admission().is_err());
        assert!(fixture::md(&backend).core() == &before);
        assert_eq!(fixture::md(&backend).core() as *const _ as usize, address);
        assert!(!fixture::md(&backend).rollback_owned());
        assert!(fixture::md(&backend).host_persistence_enabled());
    }
}
