use super::*;
use crate::cheats::{collect_enabled_patches, export_cht_file, parse_cht_file_for_system};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use std::path::Path;
use zeff_md_core::TimingModel;

mod database;

fn backend(timing: TimingModel) -> EmuBackend {
    let mut bytes = vec![0; 0x400];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    bytes[0x1f0] = b'U';
    let words = [
        0x13fc_u16, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x13fc,
        0x11, 0xff, 0x100, 0x13f9, 0xff, 0x100, 0xff, 0x101, 0x5239, 0xff, 0x102, 0x60ee,
    ];
    for (index, word) in words.into_iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = Path::new("ram-cheats.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(bytes),
        BackendLoadConfig {
            md_timing: timing,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn step(
    backend: &mut EmuBackend,
    frames: usize,
    cheats: &[crate::cheats::CheatPatch],
) -> FrameResult {
    EmuThread::handle_step_frames(
        backend,
        md_audio_tests::input(frames, false),
        cheats,
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 0,
        &mut WorkerRuntimeFault::default(),
    )
}

fn byte(backend: &EmuBackend, address: u32) -> u8 {
    let EmuBackend::Md(md) = backend else {
        panic!("MD backend")
    };
    md.core().peek(address).unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_cheat_import_worker_batches_disable_and_restore_reach_guest_reads() {
    let imported = parse_cht_file_for_system(
        "cheat0_code = \"FF0100:A5\"\ncheat0_enable = true\n\
         cheat1_code = \"A10003:00\"\ncheat1_enable = true\n\
         cheat2_code = \"FF0110:A55A\"\ncheat2_enable = true\n",
        ActiveSystem::Md,
    );
    assert_eq!(imported.len(), 2);
    let roundtrip = parse_cht_file_for_system(&export_cht_file(&imported), ActiveSystem::Md);
    let patches = collect_enabled_patches(&roundtrip, &[]);
    assert_eq!(patches.len(), 3);
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut batched = backend(timing);
        let caps = batched.capabilities();
        assert!(caps.supports_cheats && caps.cheat_features.supports_ram_writes);
        assert!(caps.cheat_features.supports_user_cheats);
        assert!(caps.cheat_features.supports_rom_patches);
        assert!(caps.cheat_features.supports_libretro_database);
        assert_eq!(caps.cheat_features.formats, &["Raw", "Game Genie"]);
        let initial = batched.encode_replay_hash_state_bytes().unwrap();
        let mut singles = backend(timing);
        let result = step(&mut batched, 2, &patches);
        assert!(result.runtime_fault.is_none() && result.replay_error.is_none());
        assert_eq!(result.advanced_frames, 2);
        assert_eq!(byte(&batched, 0xff0100), 0xa5);
        assert_eq!(byte(&batched, 0xff0101), 0xa5);
        assert_eq!(byte(&batched, 0xff0110), 0xa5);
        assert_eq!(byte(&batched, 0xff0111), 0x5a);
        let mut pcm = Vec::new();
        for _ in 0..2 {
            let single = step(&mut singles, 1, &patches);
            assert!(single.runtime_fault.is_none());
            pcm.extend(single.audio_samples);
        }
        assert_eq!(result.audio_samples, pcm);
        assert!(pcm.iter().any(|sample| *sample != 0.0));
        assert_eq!(batched.framebuffer(), singles.framebuffer());
        assert_eq!(
            batched.encode_replay_hash_state_bytes().unwrap(),
            singles.encode_replay_hash_state_bytes().unwrap()
        );

        batched.load_state_from_bytes(initial.clone()).unwrap();
        let mut disabled = imported.clone();
        disabled[0].enabled = false;
        let result = step(&mut batched, 2, &collect_enabled_patches(&disabled, &[]));
        assert!(result.runtime_fault.is_none());
        assert_eq!(byte(&batched, 0xff0101), 0x11);
        assert_eq!(byte(&batched, 0xff0110), 0xa5);
        assert_eq!(byte(&batched, 0xff0111), 0x5a);
        batched.load_state_from_bytes(initial).unwrap();
        let result = step(&mut batched, 2, &patches);
        assert!(result.runtime_fault.is_none());
        assert_eq!(byte(&batched, 0xff0101), 0xa5);
        assert_eq!(byte(&batched, 0xff0110), 0xa5);
        assert_eq!(byte(&batched, 0xff0111), 0x5a);
    }
}
