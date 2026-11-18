use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_md_core::svp::FunctionalProfile;
use zeff_md_core::{Emulator, LoadConfig, Region};

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn write_words(bytes: &mut [u8], offset: usize, words: &[u16]) {
    for (index, word) in words.iter().enumerate() {
        bytes[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
    }
}

fn fixture(region: Region, sound: bool) -> MdBackend {
    let mut rom = vec![0; 0x20000];
    rom[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 2, 0]);
    write_words(
        &mut rom,
        0x200,
        &[
            0x13fc, 0x80, 0xc0, 0x11, 0x13fc, 0x10, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x4ef9,
            0xff, 0x2000,
        ],
    );
    write_words(&mut rom, 0x800, &[0x0810, 0x1111, 0x4c00, 0x400]);
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0x400;
    let mut core = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region,
            concurrent_sound: sound,
        },
        irom,
        FunctionalProfile::new(0x507, 1, 3, [1, 1, 64, 2, 1, 1]).unwrap(),
    )
    .unwrap();
    for (address, words) in [
        (0xff2000, &[0x4e71, 0x60fc][..]),
        (0xff3000, &[0x13fc, 0x5a, 0xff, 0x0200, 0x4e75][..]),
        (0xff4000, &[0x60fe, 0x4e71][..]),
    ] {
        let writes = words
            .iter()
            .flat_map(|word: &u16| word.to_be_bytes())
            .enumerate()
            .map(|(offset, byte)| (address + offset as u32, byte))
            .collect::<Vec<_>>();
        core.debug_write_ram(&writes).unwrap();
    }
    core.install_rom_patches(&[patch(0x801, 0x20), patch(0x802, 0xa5), patch(0x803, 0xa5)])
        .unwrap();
    for _ in 0..300 {
        core.step_instruction().unwrap();
        if core.registers().pc >= 0xff2000
            && core
                .encode_state()
                .unwrap()
                .windows(8)
                .any(|part| part == b"ZSINST02")
        {
            break;
        }
    }
    assert!(core.registers().pc >= 0xff2000);
    assert!(
        core.encode_state()
            .unwrap()
            .windows(8)
            .any(|part| part == b"ZSINST02")
    );
    assert!(core.check_debug_call(0xff3000).is_ok());
    let mut backend = MdBackend::new(
        core,
        "synthetic-svp.md".into(),
        "synthetic-svp.md".into(),
        [0; 32],
        false,
    );
    backend.suspended = true;
    backend
}

fn request(target: u32, budget: u64) -> GuestCallRequest {
    GuestCallRequest {
        name: "SVP historical RAM call".into(),
        target,
        storage_offset: None,
        explicit_overlay: false,
        exec_mode: ExecMode::M68000,
        instruction_budget: budget,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn svp_guest_call_host_undo_keeps_receiver_mask_historical_inputs_and_failed_call_token() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for sound in [false, true] {
            for current in [Some(0xb6), None] {
                let mut backend = fixture(region, sound);
                let before = backend.core().clone();
                let front = before.encode_frontend_state().unwrap();
                let (_, token) = backend.execute_guest_call(&request(0xff3000, 10)).unwrap();
                assert_eq!(backend.core().peek(0xff0200), Some(0x5a));
                let after_call = backend.core().clone();
                let prior_checkpoint = backend.guest_call_undo.as_ref().unwrap().checkpoint.clone();
                assert!(backend.execute_guest_call(&request(0xff4000, 1)).is_err());
                assert_eq!(backend.core(), &after_call);
                assert_eq!(
                    backend.guest_call_undo.as_ref().unwrap().token.as_slice(),
                    token
                );
                assert_eq!(
                    backend.guest_call_undo.as_ref().unwrap().checkpoint,
                    prior_checkpoint
                );
                assert!(
                    backend.suspended
                        && backend.runtime_fault.is_none()
                        && !backend.execution_faulted
                );
                let core = backend.normal_core_mut().unwrap();
                core.clear_rom_patches();
                if let Some(value) = current {
                    core.install_rom_patches(&[patch(0x802, value), patch(0x803, value)])
                        .unwrap();
                }
                let before_bad = backend.core().clone();
                assert!(backend.undo_guest_call(&[0; 16]).is_err());
                assert_eq!(backend.core(), &before_bad);
                assert_eq!(
                    backend.guest_call_undo.as_ref().unwrap().token.as_slice(),
                    token
                );
                backend.undo_guest_call(&token).unwrap();
                assert_eq!(backend.core().encode_frontend_state().unwrap(), front);
                assert_eq!(backend.core().peek(0x802), Some(current.unwrap_or(0x11)));
                assert!(
                    backend
                        .core()
                        .encode_state()
                        .unwrap()
                        .windows(8)
                        .any(|part| part == b"ZSINST02")
                );
                assert!(backend.guest_call_undo.is_none() && backend.suspended);
                let mut reference = before;
                reference.clear_rom_patches();
                if let Some(value) = current {
                    reference
                        .install_rom_patches(&[patch(0x802, value), patch(0x803, value)])
                        .unwrap();
                }
                for _ in 0..256 {
                    backend
                        .normal_core_mut()
                        .unwrap()
                        .step_instruction()
                        .unwrap();
                    reference.step_instruction().unwrap();
                }
                assert_eq!(
                    backend.core().encode_frontend_state().unwrap(),
                    reference.encode_frontend_state().unwrap()
                );
                let pcm = backend.normal_core_mut().unwrap().drain_audio_samples();
                assert!(pcm.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm, reference.drain_audio_samples());
            }
        }
    }
}
